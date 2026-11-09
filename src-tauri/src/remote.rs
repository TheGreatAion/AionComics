//! Reading from another device: a phone, a tablet or another computer opens AionComics in its web
//! browser while AionComics runs here. Off unless switched on in Settings.
//!
//! A small web server answers on this PC's network addresses. It hands out the app itself (the same
//! pages the window shows), the library's covers and pages, and a short list of what the window can
//! ask for: browsing, reading, and saving where you got to. Nothing that deletes, hides, adds or
//! edits is on that list. A device gets in by typing the code shown in Settings once; it's given a
//! key it keeps, and a new code takes every key back.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::db;

/// Whether it's switched on: it comes back on by itself at the next launch.
pub const SETTING: &str = "remote_on";
const CODE: &str = "remote_code";
const KEYS: &str = "remote_keys";
const PORT_SETTING: &str = "remote_port";
/// The port it tries first; one nearby if something else has it, remembered so the address stays put.
pub const PORT: u16 = 8642;
const COOKIE: &str = "aion_key";

struct Running {
    server: Arc<tiny_http::Server>,
    port: u16,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Wrong codes in a row, and until when (unix seconds) no more are listened to.
static WRONG: Mutex<(u32, i64)> = Mutex::new((0, 0));
/// Bumped by each reading change from another device, so the window hears of them once they settle.
static CHANGES: AtomicU64 = AtomicU64::new(0);

pub fn enabled(conn: &Connection) -> bool {
    db::get_setting(conn, SETTING).ok().flatten().is_some_and(|v| v == "1")
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    // Windows always has randomness to give; the clock is only a fallback that never happens.
    if getrandom::getrandom(&mut b).is_err() {
        let t = crate::now_ms() as u64;
        for (i, x) in b.iter_mut().enumerate() {
            *x = (t.rotate_left(i as u32 * 7) & 0xff) as u8;
        }
    }
    b
}

fn fresh_code() -> String {
    let n = u32::from_le_bytes(random_bytes::<4>()) % 1_000_000;
    format!("{n:06}")
}

/// The code a device types to get in, made the first time it's asked for.
pub fn code(conn: &Connection) -> anyhow::Result<String> {
    if let Some(c) = db::get_setting(conn, CODE)?.filter(|c| c.len() == 6) {
        return Ok(c);
    }
    let c = fresh_code();
    db::set_setting(conn, CODE, &c)?;
    Ok(c)
}

/// A new code, and every device that got in with the old one has to type it again.
pub fn new_code(conn: &Connection) -> anyhow::Result<String> {
    let c = fresh_code();
    db::set_setting(conn, CODE, &c)?;
    db::set_setting(conn, KEYS, "[]")?;
    Ok(c)
}

fn keys(conn: &Connection) -> Vec<String> {
    db::get_setting(conn, KEYS).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn give_key(conn: &Connection) -> anyhow::Result<String> {
    let key: String = random_bytes::<24>().iter().map(|b| format!("{b:02x}")).collect();
    let mut all = keys(conn);
    all.push(key.clone());
    // Plenty for a household; the oldest go first.
    let skip = all.len().saturating_sub(50);
    db::set_setting(conn, KEYS, &serde_json::to_string(&all[skip..])?)?;
    Ok(key)
}

/// Compared in full every time, so how long a wrong key takes says nothing about the right one.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn key_ok(conn: &Connection, key: &str) -> bool {
    keys(conn).iter().fold(false, |ok, k| ok | same(k, key))
}

/// What Settings shows about it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    on: bool,
    /// The address on your home network, e.g. "http://192.168.1.20:8642".
    home: Option<String>,
    /// The address through Tailscale, which works away from home too, when Tailscale is on this PC.
    away: Option<String>,
    /// Square codes for a phone's camera, as pictures (SVG).
    home_qr: Option<String>,
    away_qr: Option<String>,
    code: String,
    /// Devices that have got in with this code.
    devices: usize,
    /// Why it couldn't start, in plain words.
    problem: Option<String>,
}

fn qr(url: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(url.as_bytes()).ok()?;
    Some(code.render::<svg::Color>().quiet_zone(true).dark_color(svg::Color("#000000")).light_color(svg::Color("#ffffff")).build())
}

/// Tailscale gives each device an address from 100.64.0.0 to 100.127.255.255.
fn tailscale(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

/// Adapters that don't lead to the home network: virtual machines on this PC (WSL, Hyper-V,
/// VirtualBox, VMware) and VPNs (Cloudflare WARP and the like), whose addresses a phone can't reach.
fn virtual_adapter(name: &str) -> bool {
    let n = name.to_lowercase();
    ["vethernet", "virtualbox", "vmware", "wsl", "hyper-v", "loopback", "docker", "warp", "cloudflare", "vpn", "wireguard", "zerotier", "nordlynx", "proton", "tap-"]
        .iter()
        .any(|v| n.contains(v))
}

/// This PC's home network address and its Tailscale address, if it has them.
fn addresses() -> (Option<Ipv4Addr>, Option<Ipv4Addr>) {
    // The address Windows would use to reach the internet; no message is actually sent.
    let main = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("8.8.8.8:80").and_then(|_| s.local_addr()))
        .ok()
        .and_then(|a| match a.ip() {
            IpAddr::V4(ip) => Some(ip),
            _ => None,
        });
    let mut home = Vec::new();
    let mut away = None;
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        let IpAddr::V4(ip) = i.ip() else { continue };
        if ip.is_loopback() || ip.is_link_local() {
            continue;
        }
        if tailscale(ip) || i.name.to_lowercase().contains("tailscale") {
            away = away.or(Some(ip));
        } else if ip.is_private() && !virtual_adapter(&i.name) {
            home.push(ip);
        }
    }
    let rank = |ip: &Ipv4Addr| (Some(*ip) != main, ip.octets()[0] != 192, ip.octets()[0] != 10);
    home.sort_by_key(rank);
    (home.first().copied(), away)
}

pub fn status(conn: &Connection, problem: Option<String>) -> anyhow::Result<Status> {
    let port = running_port();
    let (home, away) = match port {
        Some(_) if local_only() => (Some(Ipv4Addr::LOCALHOST), None),
        Some(_) => addresses(),
        None => (None, None),
    };
    let url = |ip: Option<Ipv4Addr>| ip.zip(port).map(|(ip, p)| format!("http://{ip}:{p}"));
    let (home, away) = (url(home), url(away));
    Ok(Status {
        on: port.is_some(),
        home_qr: home.as_deref().and_then(qr),
        away_qr: away.as_deref().and_then(qr),
        home,
        away,
        code: code(conn)?,
        devices: keys(conn).len(),
        problem,
    })
}

pub fn running_port() -> Option<u16> {
    RUNNING.lock().unwrap().as_ref().map(|r| r.port)
}

/// For testing: only this PC can reach it, so Windows doesn't ask about letting it on the network.
fn local_only() -> bool {
    std::env::var_os("AION_REMOTE_LOCAL").is_some()
}

/// Start answering, on the port used last time if it's free.
pub fn start(app: &AppHandle, conn: &Connection) -> anyhow::Result<()> {
    if running_port().is_some() {
        return Ok(());
    }
    let first = db::get_setting(conn, PORT_SETTING)?.and_then(|p| p.parse().ok()).unwrap_or(PORT);
    let host = if local_only() { "127.0.0.1" } else { "0.0.0.0" };
    let mut found = None;
    for port in (first..first.saturating_add(10)).chain(PORT..PORT + 10) {
        if let Ok(s) = tiny_http::Server::http((host, port)) {
            found = Some((s, port));
            break;
        }
    }
    let Some((server, port)) = found else {
        anyhow::bail!("Another program is using the connection AionComics needs. Try again after closing it.");
    };
    db::set_setting(conn, PORT_SETTING, &port.to_string())?;
    let server = Arc::new(server);
    *RUNNING.lock().unwrap() = Some(Running { server: server.clone(), port });
    let app = app.clone();
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let app = app.clone();
            std::thread::spawn(move || handle(&app, req));
        }
    });
    eprintln!("remote reading on port {port}");
    Ok(())
}

pub fn stop() {
    if let Some(r) = RUNNING.lock().unwrap().take() {
        r.server.unblock();
    }
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

type Reply = tiny_http::Response<std::io::Cursor<Vec<u8>>>;

fn reply(status: u16, mime: &str, body: Vec<u8>) -> Reply {
    tiny_http::Response::from_data(body).with_status_code(status).with_header(header("Content-Type", mime))
}

fn json(status: u16, v: &serde_json::Value) -> Reply {
    reply(status, "application/json", serde_json::to_vec(v).unwrap_or_default()).with_header(header("Cache-Control", "no-store"))
}

fn cookie_key(req: &tiny_http::Request) -> Option<String> {
    let cookies = req.headers().iter().find(|h| h.field.equiv("Cookie"))?.value.as_str().to_string();
    cookies.split(';').find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}=")).map(str::to_string))
}

fn body(req: &mut tiny_http::Request) -> serde_json::Value {
    let mut buf = Vec::new();
    // Nothing the app sends is anywhere near this big.
    let _ = req.as_reader().take(1 << 20).read_to_end(&mut buf);
    serde_json::from_slice(&buf).unwrap_or(serde_json::Value::Null)
}

fn handle(app: &AppHandle, mut req: tiny_http::Request) {
    let url = req.url().to_string();
    let path = url.split(['?', '#']).next().unwrap_or("/").to_string();
    let state = app.state::<crate::Shared>().inner().clone();
    let key = cookie_key(&req);
    let signed_in = || key.as_deref().is_some_and(|k| key_ok(&state.conn.lock().unwrap(), k));
    let post = *req.method() == tiny_http::Method::Post;
    let response = if path == "/api/login" && post {
        let typed = body(&mut req).get("code").and_then(|c| c.as_str()).unwrap_or("").trim().to_string();
        login(&state.conn.lock().unwrap(), &typed)
    } else if path == "/api/hello" {
        if signed_in() { json(200, &serde_json::json!({})) } else { json(401, &"Type the code".into()) }
    } else if let Some(cmd) = path.strip_prefix("/api/").filter(|_| post) {
        if !signed_in() {
            json(401, &"Type the code".into())
        } else {
            let args = body(&mut req);
            let result = crate::remote_call(app, cmd, &args);
            if result.is_ok() && matches!(cmd, "save_progress" | "save_guided_spot" | "toggle_bookmark" | "set_wanted") {
                changed(app);
            }
            match result {
                Ok(v) => json(200, &v),
                Err(e) => json(400, &e.into()),
            }
        }
    } else if let Some(img) = path.strip_prefix("/img/") {
        if !signed_in() {
            reply(401, "text/plain", b"Type the code".to_vec())
        } else {
            match crate::image(&state, img) {
                Ok((bytes, mime)) => reply(200, mime, bytes).with_header(header("Cache-Control", "private, max-age=31536000, immutable")),
                Err(e) => reply(404, "text/plain", e.to_string().into_bytes()),
            }
        }
    } else {
        asset(app, &path)
    };
    let _ = req.respond(response);
}

fn login(conn: &Connection, typed: &str) -> Reply {
    let now = crate::scan::now();
    let mut wrong = WRONG.lock().unwrap();
    if now < wrong.1 {
        return json(429, &"Too many wrong codes. Wait a minute, then try again.".into());
    }
    let right = code(conn).is_ok_and(|c| same(&c, typed));
    if !right {
        wrong.0 += 1;
        // A few slips are fine; then a pause, so guessing all million codes would take years.
        if wrong.0 >= 5 {
            *wrong = (0, now + 60);
        }
        return json(401, &"That code isn't right.".into());
    }
    *wrong = (0, 0);
    match give_key(conn) {
        Ok(key) => json(200, &serde_json::json!({})).with_header(header(
            "Set-Cookie",
            &format!("{COOKIE}={key}; Path=/; Max-Age=315360000; HttpOnly; SameSite=Strict"),
        )),
        Err(e) => json(500, &e.to_string().into()),
    }
}

/// The app itself: the same files the window shows. Any address that isn't a file is a screen of
/// the app, which works out what to show by itself.
fn asset(app: &AppHandle, path: &str) -> Reply {
    let resolver = app.asset_resolver();
    let file = Some(path).filter(|p| *p != "/").and_then(|p| resolver.get(p.to_string()));
    match file.or_else(|| resolver.get("index.html".to_string())) {
        Some(a) => {
            // The app's own code changes only with a new version, and its files are named for what's in them.
            let cache = if path.starts_with("/_app/immutable/") { "public, max-age=31536000, immutable" } else { "no-cache" };
            reply(200, &a.mime_type, a.bytes).with_header(header("Cache-Control", cache))
        }
        None => reply(404, "text/plain", b"Not found".to_vec()),
    }
}

/// Tell the window that reading changed on another device, once a few seconds pass with no more:
/// a page turn each few seconds shouldn't have it refetch its screen each time.
fn changed(app: &AppHandle) {
    let n = CHANGES.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if CHANGES.load(Ordering::SeqCst) == n {
            let _ = app.emit("library-changed", ());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(name: &str) -> Connection {
        let dir = std::env::temp_dir().join(format!("aion-remote-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        db::open(&dir.join("t.db")).unwrap()
    }

    #[test]
    fn codes_and_keys() {
        let c = conn("keys");
        let first = code(&c).unwrap();
        assert_eq!(first.len(), 6);
        assert!(first.chars().all(|ch| ch.is_ascii_digit()));
        assert_eq!(code(&c).unwrap(), first, "the code stays until a new one is asked for");
        let key = give_key(&c).unwrap();
        assert!(key_ok(&c, &key));
        assert!(!key_ok(&c, "nope"));
        new_code(&c).unwrap();
        assert!(!key_ok(&c, &key), "a new code signs every device out");
    }

    #[test]
    fn wrong_codes_wait() {
        let c = conn("wrong");
        let right = code(&c).unwrap();
        let wrong = if right == "000000" { "111111" } else { "000000" };
        *WRONG.lock().unwrap() = (0, 0);
        for _ in 0..5 {
            assert_eq!(login(&c, wrong).status_code().0, 401);
        }
        assert_eq!(login(&c, &right).status_code().0, 429, "even the right code waits after five wrong ones");
        *WRONG.lock().unwrap() = (0, 0);
        assert_eq!(login(&c, &right).status_code().0, 200);
    }

    /// What this PC would show: `cargo test --lib remote::tests::these_addresses -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn these_addresses() {
        println!("{:?}", addresses());
    }

    #[test]
    fn square_code() {
        let svg = qr("http://192.168.1.20:8642").unwrap();
        assert!(svg.contains("<svg") && svg.contains("#000000"));
    }

    #[test]
    fn address_kinds() {
        assert!(tailscale(Ipv4Addr::new(100, 101, 2, 3)));
        assert!(!tailscale(Ipv4Addr::new(100, 1, 2, 3)));
        assert!(virtual_adapter("vEthernet (WSL (Hyper-V firewall))"));
        assert!(virtual_adapter("CloudflareWARP"));
        assert!(!virtual_adapter("Wi-Fi"));
        assert!(!virtual_adapter("Ethernet"));
    }
}
