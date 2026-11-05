//! Library scanning: walk the comics folder, mirror it into the database,
//! and pull metadata + reading progress from an existing YACReader library.
//!
//! Folder convention: `Collection / Series / [sections…] / file`, read loosely (see `lay_out`) so
//! other ways of keeping comics work too. Nothing on disk is ever rearranged to fit.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use walkdir::WalkDir;

use crate::archive::EXTENSIONS;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub comics: i64,
    pub series: i64,
    pub collections: i64,
    pub added: i64,
    pub removed: i64,
    pub yac_matched: i64,
    pub seconds: f64,
    pub finished_at: i64,
    /// Books that looked missing but were kept, because too much went missing at once to be real.
    pub kept_missing: i64,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Set when the comics folder has been changed on purpose, until the next scan has done its work.
pub const NEW_ROOT: &str = "new_root";
/// The highest number a book has been given, so none is ever used twice.
const HIGHEST_BOOK: &str = "comics_high";

/// Folders that are never comics, wherever they turn up: hidden ones, and Windows' own on a drive.
fn not_comics(name: &str) -> bool {
    name.starts_with('.') || ["$RECYCLE.BIN", "RECYCLER", "System Volume Information"].iter().any(|n| name.eq_ignore_ascii_case(n))
}

struct Found {
    collection: String,
    /// The library for series that sit straight in the comics folder, with no folder of its own.
    at_top: bool,
    series: String,
    series_rel: String,
    section: String,
    rel: String,
    file_name: String,
    size: i64,
    mtime: i64,
}

/// A comic file as it sits on disk: its path inside the comics folder, piece by piece.
struct Raw {
    parts: Vec<String>,
    size: i64,
    mtime: i64,
}

fn walk(root: &Path) -> Vec<Raw> {
    let mut out = Vec::new();
    let walker = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !not_comics(&e.file_name().to_string_lossy()));
    for e in walker.filter_map(|e| e.ok()) {
        if !e.file_type().is_file() {
            continue;
        }
        let ext = e
            .path()
            .extension()
            .map(|x| x.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(root) else { continue };
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let meta = e.metadata().ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Raw { parts, size: meta.map(|m| m.len() as i64).unwrap_or(0), mtime });
    }
    out
}

/// How the folders at the top of the comics folder are read.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Layout {
    /// Worked out folder by folder.
    Auto,
    /// Every one is a library.
    Libraries,
    /// Every one is a series.
    Series,
}

impl Layout {
    pub const SETTING: &str = "library_layout";
    pub fn parse(s: Option<&str>) -> Layout {
        match s {
            Some("libraries") => Layout::Libraries,
            Some("series") => Layout::Series,
            _ => Layout::Auto,
        }
    }
}

/// What a folder at the top holds.
#[derive(Default)]
struct Top<'a> {
    loose: Vec<&'a str>,
    subs: Vec<&'a str>,
}

/// The name of the series a top folder is, when it's a series rather than a library. It is when its
/// subfolders only number it ("Saga / Vol 1"), or its books are named after it, only by number, or
/// all alike. Comics in folders with names of their own make it a library.
fn top_series(name: &str, top: &Top, layout: Layout) -> Option<String> {
    use crate::intake::{norm, says_nothing, series_guess};
    match layout {
        Layout::Libraries => return None,
        Layout::Series => return Some(name.trim().to_string()),
        Layout::Auto => {}
    }
    if !top.subs.is_empty() {
        return top.subs.iter().all(|s| says_nothing(s)).then(|| name.trim().to_string());
    }
    let guesses: Vec<String> = top.loose.iter().map(|f| series_guess(f)).collect();
    let folder = norm(name);
    let fits = |g: &String| says_nothing(g) || norm(g) == folder || norm(g).starts_with(&format!("{folder} "));
    if guesses.iter().all(fits) {
        return Some(name.trim().to_string());
    }
    let alike = guesses.len() >= 2 && guesses.iter().all(|g| norm(g) == norm(&guesses[0]));
    alike.then(|| guesses[0].clone())
}

/// Which series each loose book in a folder belongs to: (name, the series' place in the comics
/// folder). Books that share a name are one series, and join a series folder beside them of that
/// name; the place is where a folder for it would be. A book on its own is a series of one.
fn group_loose(prefix: &str, files: &[&str], beside: &HashMap<String, (String, String)>) -> HashMap<String, (String, String)> {
    use crate::intake::{norm, safe_folder, says_nothing, series_guess};
    let place = |name: &str| if prefix.is_empty() { name.to_string() } else { format!("{prefix}/{name}") };
    let guesses: Vec<(&str, String)> = files.iter().map(|f| (*f, series_guess(f))).collect();
    let mut first: HashMap<String, (&str, usize)> = HashMap::new();
    for (_, g) in &guesses {
        first.entry(norm(g)).or_insert((g, 0)).1 += 1;
    }
    let mut out = HashMap::new();
    for (file, g) in &guesses {
        let key = norm(g);
        let (name, n) = first[&key];
        let series = match beside.get(&key) {
            Some(home) if !key.is_empty() => home.clone(),
            _ if n >= 2 && !says_nothing(g) && !safe_folder(name).is_empty() => (name.to_string(), place(&safe_folder(name))),
            _ => {
                let stem = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| file.to_string());
                (clean_name(&stem), place(file))
            }
        };
        out.insert(file.to_string(), series);
    }
    out
}

/// Comic files as libraries and series. `home` names the library for series at the very top.
fn lay_out(raw: Vec<Raw>, home: &str, layout: Layout) -> Vec<Found> {
    let mut tops: HashMap<&str, Top> = HashMap::new();
    let mut at_root: Vec<&str> = Vec::new();
    for r in &raw {
        match r.parts.as_slice() {
            [file] => at_root.push(file),
            [top, file] => tops.entry(top).or_default().loose.push(file),
            [top, sub, ..] => {
                let t = tops.entry(top).or_default();
                if !t.subs.contains(&sub.as_str()) {
                    t.subs.push(sub);
                }
            }
            [] => {}
        }
    }
    let series_tops: HashMap<&str, String> =
        tops.iter().filter_map(|(name, top)| top_series(name, top, layout).map(|s| (*name, s))).collect();
    // A library of the same name is its own folder: the top's library says which it is.
    let clash = tops.keys().any(|t| !series_tops.contains_key(t) && t.eq_ignore_ascii_case(home));
    let home = if clash { format!("{home} (main folder)") } else { home.to_string() };

    let mut root_beside = HashMap::new();
    for (folder, name) in &series_tops {
        root_beside.insert(crate::intake::norm(folder), (name.clone(), folder.to_string()));
        root_beside.insert(crate::intake::norm(name), (name.clone(), folder.to_string()));
    }
    let loose_at_root = group_loose("", &at_root, &root_beside);
    let loose_in: HashMap<&str, HashMap<String, (String, String)>> = tops
        .iter()
        .filter(|(name, _)| !series_tops.contains_key(*name))
        .map(|(name, top)| {
            let beside = top.subs.iter().map(|s| (crate::intake::norm(s), (s.trim().to_string(), format!("{name}/{s}")))).collect();
            (*name, group_loose(name, &top.loose, &beside))
        })
        .collect();

    raw.iter()
        .map(|r| {
            let parts = &r.parts;
            let file_name = parts[parts.len() - 1].clone();
            let top_name = series_tops.get(parts[0].as_str());
            let (collection, at_top, series, series_rel, section) = if parts.len() == 1 {
                let (s, rel) = loose_at_root[&file_name].clone();
                (home.clone(), true, s, rel, String::new())
            } else if let Some(s) = top_name {
                (home.clone(), true, s.clone(), parts[0].clone(), parts[1..parts.len() - 1].join(" / "))
            } else if parts.len() == 2 {
                let (s, rel) = loose_in[parts[0].as_str()][&file_name].clone();
                (parts[0].clone(), false, s, rel, String::new())
            } else {
                (parts[0].clone(), false, parts[1].trim().to_string(), format!("{}/{}", parts[0], parts[1]), parts[2..parts.len() - 1].join(" / "))
            };
            Found { collection, at_top, series, series_rel, section, rel: parts.join("/"), file_name, size: r.size, mtime: r.mtime }
        })
        .collect()
}

/// Folders at the very top with no comics in them: libraries waiting for their first series.
fn empty_libraries(root: &Path, raw: &[Raw], layout: Layout, hidden: &[String]) -> Vec<String> {
    if layout == Layout::Series {
        return Vec::new();
    }
    let used: HashSet<&str> = raw.iter().filter(|r| r.parts.len() > 1).map(|r| r.parts[0].as_str()).collect();
    let Ok(dir) = std::fs::read_dir(root) else { return Vec::new() };
    dir.flatten()
        .filter(|e| {
            let hidden = e.metadata().map(|m| hidden_or_system(&m)).unwrap_or(true);
            e.file_type().is_ok_and(|t| t.is_dir()) && !hidden && !not_comics(&e.file_name().to_string_lossy())
        })
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !used.contains(n.as_str()) && !hidden.contains(n))
        .collect()
}

/// Windows' own folders on a drive ("System Volume Information", the recycle bin) aren't libraries.
#[cfg(windows)]
fn hidden_or_system(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x6 != 0
}

#[cfg(not(windows))]
fn hidden_or_system(_: &std::fs::Metadata) -> bool {
    false
}

// ---------- YACReader import ----------

pub(crate) struct Yac {
    title: Option<String>,
    number: Option<String>,
    volume: Option<String>,
    date: Option<String>,
    writer: Option<String>,
    publisher: Option<String>,
    synopsis: Option<String>,
    cv_id: Option<String>,
    hash: Option<String>,
    num_pages: Option<i64>,
    pub(crate) current_page: Option<i64>,
    pub(crate) read: bool,
    pub(crate) last_opened: Option<i64>,
    added: Option<i64>,
    /// How YACReader reads it: 0 comic, 1 manga, 2 left-to-right manga, 3 webtoon.
    kind: Option<i64>,
    /// Credits and story details, as AionComics field name -> value.
    extra: Vec<(&'static str, String)>,
}

/// YACReader columns beyond the basics, and the AionComics fields they fill. Only the ones present
/// in this YACReader database are read, so an older YACReader never breaks the import.
const YAC_EXTRA: [(&str, &str); 12] = [
    ("penciller", "penciller"),
    ("inker", "inker"),
    ("colorist", "colorist"),
    ("letterer", "letterer"),
    ("coverArtist", "cover_artist"),
    ("characters", "characters"),
    ("teams", "teams"),
    ("locations", "locations"),
    ("storyArc", "story_arc"),
    ("arcNumber", "arc_number"),
    ("arcCount", "arc_count"),
    ("genere", "genre"),
];

/// Any SQLite value as text (YACReader stores some numbers as integers).
fn as_text(v: rusqlite::types::Value) -> Option<String> {
    use rusqlite::types::Value;
    let s = match v {
        Value::Text(t) => t,
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        _ => return None,
    };
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub(crate) fn read_yac(root: &Path) -> HashMap<String, Yac> {
    let mut map = HashMap::new();
    let db = root.join(".yacreaderlibrary").join("library.ydb");
    if !db.exists() {
        return map;
    }
    let Ok(conn) = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return map;
    };
    let present: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('comic_info')")
        .and_then(|mut st| st.query_map([], |r| r.get(0))?.collect())
        .unwrap_or_default();
    let extras: Vec<(&str, &'static str)> =
        YAC_EXTRA.iter().copied().filter(|(col, _)| present.iter().any(|p| p == col)).collect();
    let extra_cols: String = extras.iter().map(|(col, _)| format!(", ci.{col}")).collect();
    // Older YACReaders don't have reading types.
    let kind_col = if present.iter().any(|p| p == "type") { "ci.type" } else { "NULL" };
    let sql = format!(
        "SELECT c.path, ci.title, ci.number, ci.volume, ci.date, ci.writer, ci.publisher, ci.synopsis,
           ci.comicVineID, ci.hash, ci.numPages, ci.currentPage, ci.read, ci.lastTimeOpened, ci.added, {kind_col}{extra_cols}
         FROM comic c JOIN comic_info ci ON ci.id=c.comicInfoId"
    );
    let Ok(mut st) = conn.prepare(&sql) else { return map };
    let rows = st.query_map([], |r| {
        let path: String = r.get(0)?;
        Ok((
            path.trim_start_matches('/').to_string(),
            Yac {
                title: r.get(1)?,
                number: r.get(2)?,
                volume: r.get(3)?,
                date: r.get(4)?,
                writer: r.get(5)?,
                publisher: r.get(6)?,
                synopsis: r.get(7)?,
                cv_id: r.get(8)?,
                hash: r.get(9)?,
                num_pages: r.get(10)?,
                current_page: r.get(11)?,
                read: r.get::<_, Option<bool>>(12)?.unwrap_or(false),
                last_opened: r.get(13)?,
                added: r.get(14)?,
                kind: r.get(15)?,
                extra: extras
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_, field))| r.get::<_, rusqlite::types::Value>(16 + i).ok().and_then(as_text).map(|v| (*field, v)))
                    .collect(),
            },
        ))
    });
    if let Ok(rows) = rows {
        for (p, y) in rows.flatten() {
            map.insert(p, y);
        }
    }
    map
}

// ---------- labels ----------

static GROUPS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").unwrap());
static VOLUME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[\s_\-.])v(?:ol(?:ume)?)?\.?\s?(\d{1,4}(?:\.\d+)?)\b").unwrap());
static TRAILING_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s_#])#?(\d{1,4}(?:\.\d+)?)\s*$").unwrap());
static ISSUE_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s_#])#?(\d{1,4}(?:\.\d+)?)(?:\s|$|-)").unwrap());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\((\d{4})\)").unwrap());

/// Strip scene tags like "(2015) (Digital) (Group)" from a file stem.
/// YACReader keeps dates as "day/month/year"; Comic Vine and AionComics use "year-month-day".
fn iso_date(d: &str) -> Option<String> {
    let d = d.trim();
    let parts: Vec<&str> = d.split('/').collect();
    match parts.as_slice() {
        [day, month, year] if year.len() == 4 => Some(format!("{year}-{:0>2}-{:0>2}", month, day)),
        _ if !d.is_empty() => Some(d.to_string()),
        _ => None,
    }
}

pub(crate) fn clean_name(stem: &str) -> String {
    let s = GROUPS.replace_all(stem, "");
    s.trim().trim_end_matches(['-', '–', '_']).trim().to_string()
}

fn trim_num(n: &str) -> String {
    let t = n.trim_start_matches('0');
    if t.is_empty() || t.starts_with('.') {
        format!("0{t}")
    } else {
        t.to_string()
    }
}

fn label_for(file_name: &str, series: &str, is_manga: bool, yac: Option<&Yac>) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let clean = clean_name(&stem);
    let vol = VOLUME.captures(&clean).map(|c| trim_num(&c[1]));
    let yac_num = yac
        .and_then(|y| y.number.clone())
        .filter(|n| !n.trim().is_empty());
    if is_manga {
        if let Some(v) = vol {
            return format!("Vol. {v}");
        }
        if let Some(n) = yac_num {
            return format!("Vol. {}", trim_num(n.trim()));
        }
    } else {
        if let Some(n) = yac_num {
            return format!("#{}", trim_num(n.trim()));
        }
        if let Some(v) = vol {
            return format!("Vol. {v}");
        }
    }
    // A number right after the series name ("Saga 054 - …") or at the end, but not one that's
    // part of the series' own name ("Marvel 1985", "Ben 10 001").
    let rest = crate::editions::without_series(&clean, series).trim_start();
    if rest.is_empty() {
        return clean;
    }
    if let Some(c) = TRAILING_NUM.captures(rest).or_else(|| ISSUE_NUM.captures(rest)) {
        let n = trim_num(&c[1]);
        return if is_manga { format!("Vol. {n}") } else { format!("#{n}") };
    }
    clean
}

fn year_for(file_name: &str, yac: Option<&Yac>) -> Option<String> {
    yac.and_then(|y| y.date.as_deref())
        .and_then(|d| d.rsplit('/').next())
        .filter(|y| y.len() == 4)
        .map(str::to_string)
        .or_else(|| YEAR.captures(file_name).map(|c| c[1].to_string()))
}

fn clean_title(t: Option<&String>) -> Option<String> {
    t.map(|s| s.trim().trim_matches('"').trim().to_string())
        .filter(|s| !s.is_empty())
}

// ---------- scan ----------

pub fn scan(conn: &mut Connection, root: &Path) -> Result<ScanStats> {
    let started = std::time::Instant::now();
    // A disconnected or sleeping drive looks exactly like an empty library. Never act on that:
    // the database holds reading progress, bookmarks, panel edits and metadata for every book.
    if !root.is_dir() {
        anyhow::bail!("Your library folder ({}) isn't available. Is the drive connected?", root.display());
    }
    let layout = Layout::parse(crate::db::get_setting(conn, Layout::SETTING)?.as_deref());
    let home = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Library".to_string());
    // A new comics folder is taken as it is: what was hidden in the old one means nothing here.
    let new_root = crate::db::get_setting(conn, NEW_ROOT)?.is_some();
    if new_root {
        conn.execute_batch("DELETE FROM hidden; DELETE FROM hidden_reading;")?;
    }
    // What you've hidden is passed over: a file, or everything in a folder.
    let hidden: Vec<String> =
        conn.prepare("SELECT rel_path FROM hidden")?.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let mut raw = walk(root);
    // Before those are passed over: a folder of only hidden books isn't empty.
    let empties = empty_libraries(root, &raw, layout, &hidden);
    raw.retain(|r| {
        let rel = r.parts.join("/");
        !hidden.iter().any(|h| rel == *h || rel.starts_with(&format!("{h}/")))
    });
    let found = lay_out(raw, &home, layout);
    let known: i64 = conn.query_row("SELECT COUNT(*) FROM comics", [], |r| r.get(0))?;
    if found.is_empty() && known > 0 && !new_root {
        anyhow::bail!("No comics were found in {}. Nothing was changed.", root.display());
    }
    let yac = read_yac(root);
    let mut stats = ScanStats::default();
    let highest: i64 = crate::db::get_setting(conn, HIGHEST_BOOK)?.and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut next_book = highest.max(conn.query_row("SELECT COALESCE(MAX(id), 0) FROM comics", [], |r| r.get(0))?) + 1;

    let tx = conn.transaction()?;
    tx.execute("UPDATE comics SET seen=0", [])?;
    {
        let mut ins_col = tx.prepare(
            "INSERT INTO collections(name, is_manga, at_top) VALUES(?1, ?2, ?3)
             ON CONFLICT(name) DO UPDATE SET is_manga=excluded.is_manga, at_top=excluded.at_top RETURNING id, reading",
        )?;
        let mut ins_series = tx.prepare(
            "INSERT INTO series(collection_id, name, rel_path) VALUES(?1, ?2, ?3)
             ON CONFLICT(rel_path) DO UPDATE SET name=excluded.name, collection_id=excluded.collection_id RETURNING id, reading",
        )?;
        let mut existing = tx.prepare(
            "SELECT id, read, last_read_at, yac_read FROM comics WHERE rel_path=?1",
        )?;
        let mut insert = tx.prepare(
            "INSERT INTO comics(id, series_id, section, rel_path, file_name, size, mtime, added_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut update_file = tx.prepare(
            "UPDATE comics SET series_id=?2, section=?3, file_name=?4, size=?5, mtime=?6, seen=1 WHERE id=?1",
        )?;
        let mut update_meta = tx.prepare(
            "UPDATE comics SET label=?2, title=?3, number=?4, volume=?5, year=?6, writer=?7, publisher=?8,
               synopsis=?9, cv_id=?10, yac_hash=?11, num_pages=COALESCE(num_pages, ?12), meta_base=?13,
               date=?14, penciller=?15, inker=?16, colorist=?17, letterer=?18, cover_artist=?19,
               characters=?20, teams=?21, locations=?22, story_arc=?23, arc_number=?24, arc_count=?25,
               genre=?26, yac_type=?27, cv_volume_id=NULL WHERE id=?1",
        )?;
        let mut update_progress = tx.prepare(
            "UPDATE comics SET current_page=?2, read=?3, last_read_at=?4 WHERE id=?1",
        )?;
        let mut note_yac_read = tx.prepare("UPDATE comics SET yac_read=?2 WHERE id=?1")?;
        // A book that's disappeared, and one that's appeared with its name and size, or for a big
        // file its size and date, is the same book moved or renamed: it keeps its place, details and
        // number. Only when exactly one could be it.
        let mut move_book = tx.prepare("UPDATE comics SET rel_path=?2 WHERE id=?1")?;
        let mut moved_book = tx.prepare("SELECT read, last_read_at, yac_read FROM comics WHERE id=?1")?;
        const BIG_ENOUGH: i64 = 1 << 20;
        let here: HashSet<&str> = found.iter().map(|f| f.rel.as_str()).collect();
        let mut by_name: HashMap<(String, i64), Option<i64>> = HashMap::new();
        let mut by_date: HashMap<(i64, i64), Option<i64>> = HashMap::new();
        {
            let mut st = tx.prepare("SELECT id, rel_path, file_name, size, mtime FROM comics")?;
            let rows = st.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<i64>>(3)?, r.get::<_, Option<i64>>(4)?))
            })?;
            for row in rows {
                let (id, rel, name, size, mtime) = row?;
                if here.contains(rel.as_str()) {
                    continue;
                }
                let (size, mtime) = (size.unwrap_or(0), mtime.unwrap_or(0));
                by_name.entry((name, size)).and_modify(|v| *v = None).or_insert(Some(id));
                if size >= BIG_ENOUGH {
                    by_date.entry((size, mtime)).and_modify(|v| *v = None).or_insert(Some(id));
                }
            }
        }
        let mut claimed: HashSet<i64> = HashSet::new();
        let mut was_hidden = tx.prepare("DELETE FROM hidden_reading WHERE rel_path=?1 RETURNING current_page, read, last_read_at")?;

        let mut col_ids: HashMap<String, (i64, bool)> = HashMap::new();
        let mut series_ids: HashMap<String, (i64, Option<String>)> = HashMap::new();

        for f in &found {
            let (col_id, is_manga) = match col_ids.get(&f.collection) {
                Some(v) => *v,
                None => {
                    let manga = f.collection.eq_ignore_ascii_case("manga");
                    let (id, reading): (i64, Option<String>) =
                        ins_col.query_row(params![f.collection, manga, f.at_top], |r| Ok((r.get(0)?, r.get(1)?)))?;
                    // Your say for the library comes before its name.
                    let manga = reading.map_or(manga, |r| r == "manga");
                    col_ids.insert(f.collection.clone(), (id, manga));
                    (id, manga)
                }
            };
            let (series_id, reading) = match series_ids.get(&f.series_rel) {
                Some(s) => s.clone(),
                None => {
                    let s: (i64, Option<String>) =
                        ins_series.query_row(params![col_id, f.series, f.series_rel], |r| Ok((r.get(0)?, r.get(1)?)))?;
                    series_ids.insert(f.series_rel.clone(), s.clone());
                    s
                }
            };
            // Your say for the series comes before its library's name.
            let is_manga = reading.map_or(is_manga, |r| r == "manga");
            let y = yac.get(&f.rel);
            let prior: Option<(i64, bool, Option<i64>, Option<bool>)> = existing
                .query_row([&f.rel], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .optional()?;
            let (id, ours_read, ours_last_read, yac_read_before) = match prior {
                Some((id, read, last, yac_read)) => {
                    update_file.execute(params![id, series_id, f.section, f.file_name, f.size, f.mtime])?;
                    (id, read, last, yac_read)
                }
                None if let Some(id) = by_name
                    .get(&(f.file_name.clone(), f.size))
                    .copied()
                    .flatten()
                    .or_else(|| by_date.get(&(f.size, f.mtime)).copied().flatten())
                    .filter(|id| claimed.insert(*id)) =>
                {
                    move_book.execute(params![id, f.rel])?;
                    update_file.execute(params![id, series_id, f.section, f.file_name, f.size, f.mtime])?;
                    let (read, last, yac_read) = moved_book.query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                    (id, read, last, yac_read)
                }
                None => {
                    let added = y.and_then(|y| y.added).filter(|a| *a > 0).unwrap_or(f.mtime.max(1));
                    // Never a number a book has had: covers and details are kept by it.
                    let id = next_book;
                    next_book += 1;
                    insert.execute(params![id, series_id, f.section, f.rel, f.file_name, f.size, f.mtime, added])?;
                    stats.added += 1;
                    // Back from being hidden: where you'd read to.
                    let back: Option<(i64, bool, Option<i64>)> = was_hidden
                        .query_row([&f.rel], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                        .optional()?;
                    if let Some((page, read, last)) = back {
                        update_progress.execute(params![id, page, read, last])?;
                    }
                    (id, false, None, None)
                }
            };
            // What the scan imports is also kept as the base layer that Comic Vine data and hand edits sit on.
            let label = label_for(&f.file_name, &f.series, is_manga, y);
            let title = clean_title(y.and_then(|y| y.title.as_ref()));
            let number = y.and_then(|y| y.number.clone());
            let volume = y.and_then(|y| y.volume.clone());
            let year = year_for(&f.file_name, y);
            let writer = y.and_then(|y| y.writer.clone());
            let publisher = y.and_then(|y| y.publisher.clone());
            let synopsis = y.and_then(|y| y.synopsis.clone());
            let cv_id = y.and_then(|y| y.cv_id.clone()).filter(|s| !s.is_empty());
            let date = y.and_then(|y| y.date.as_deref()).and_then(iso_date);
            let extra = |field: &str| y.and_then(|y| y.extra.iter().find(|(f, _)| *f == field).map(|(_, v)| v.clone()));
            let mut base = serde_json::Map::new();
            let mut put = |k: &str, v: &Option<String>| {
                if let Some(v) = v {
                    base.insert(k.to_string(), serde_json::Value::String(v.clone()));
                }
            };
            put("label", &Some(label.clone()));
            put("title", &title);
            put("number", &number);
            put("volume", &volume);
            put("year", &year);
            put("date", &date);
            put("writer", &writer);
            put("publisher", &publisher);
            put("synopsis", &synopsis);
            put("cv_id", &cv_id);
            for (_, field) in YAC_EXTRA {
                put(field, &extra(field));
            }
            let base = serde_json::to_string(&base)?;
            update_meta.execute(params![
                id,
                label,
                title,
                number,
                volume,
                year,
                writer,
                publisher,
                synopsis,
                cv_id,
                y.and_then(|y| y.hash.clone()),
                y.and_then(|y| y.num_pages).filter(|n| *n > 0),
                base,
                date,
                extra("penciller"),
                extra("inker"),
                extra("colorist"),
                extra("letterer"),
                extra("cover_artist"),
                extra("characters"),
                extra("teams"),
                extra("locations"),
                extra("story_arc"),
                extra("arc_number"),
                extra("arc_count"),
                extra("genre"),
                y.and_then(|y| y.kind),
            ])?;
            if let Some(y) = y {
                stats.yac_matched += 1;
                // Take YACReader's progress when it's newer than ours.
                let yac_opened = y.last_opened.unwrap_or(0);
                let has_progress = y.read || y.current_page.unwrap_or(1) > 1;
                // A book marked read in YACReader without opening it has no time to compare, so
                // the mark is taken when it's new there: not again once you've marked it unread here.
                let marked = y.read && yac_opened == 0 && !ours_read && yac_read_before != Some(true);
                if has_progress && (yac_opened > ours_last_read.unwrap_or(0) || marked) {
                    let page = (y.current_page.unwrap_or(1) - 1).max(0);
                    let last = if yac_opened > 0 { Some(yac_opened) } else { ours_last_read };
                    update_progress.execute(params![id, page, y.read, last])?;
                }
                note_yac_read.execute(params![id, y.read])?;
            }
        }
        for name in &empties {
            ins_col.query_row(params![name, name.eq_ignore_ascii_case("manga"), false], |r| r.get::<_, i64>(0))?;
        }
    }
    crate::db::set_setting(&tx, HIGHEST_BOOK, &(next_book - 1).to_string())?;
    tx.execute("DELETE FROM settings WHERE key=?1", [NEW_ROOT])?;
    // Losing more than a third of the library in one scan means a folder or drive dropped out, not
    // that you deleted that much. Keep those books until they're really gone.
    let missing: i64 = tx.query_row("SELECT COUNT(*) FROM comics WHERE seen=0", [], |r| r.get(0))?;
    if missing > 0 && known > 30 && missing * 3 > known && !new_root {
        stats.kept_missing = missing;
    } else {
        stats.removed = tx.execute("DELETE FROM comics WHERE seen=0", [])? as i64;
    }
    crate::removal::clear_leftovers(&tx)?;
    tx.execute("DELETE FROM series WHERE id NOT IN (SELECT DISTINCT series_id FROM comics)", [])?;
    let unused: Vec<(i64, String)> = tx
        .prepare("SELECT id, name FROM collections WHERE id NOT IN (SELECT DISTINCT collection_id FROM series)")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, name) in unused {
        if !empties.contains(&name) {
            tx.execute("DELETE FROM collections WHERE id=?1", [id])?;
        }
    }
    // Put Comic Vine data and hand edits back on top of what was just imported.
    crate::db::apply_meta(&tx, None)?;
    assign_sort_keys(&tx)?;
    tx.commit()?;

    stats.comics = conn.query_row("SELECT COUNT(*) FROM comics", [], |r| r.get(0))?;
    stats.series = conn.query_row("SELECT COUNT(*) FROM series", [], |r| r.get(0))?;
    stats.collections = conn.query_row("SELECT COUNT(*) FROM collections", [], |r| r.get(0))?;
    stats.seconds = started.elapsed().as_secs_f64();
    stats.finished_at = now();
    Ok(stats)
}

/// Record each cover's shape (height / width) so grids can show covers at
/// their real proportions without jumping as images load. Reads only image
/// headers from YACReader's cover cache; covers AionComics generates itself get
/// their ratio when the thumbnail is made.
/// Work out the color of every cover that's been made (by AionComics or YACReader) but not yet looked at.
pub fn cover_colors(conn: &mut Connection, root: &Path, thumbs: &Path) -> Result<usize> {
    let yac = root.join(".yacreaderlibrary").join("covers");
    let todo: Vec<(i64, Option<String>)> = conn
        .prepare("SELECT id, yac_hash FROM comics WHERE cover_color IS NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut done = 0;
    for chunk in todo.chunks(250) {
        let found: Vec<(i64, String)> = chunk
            .iter()
            .filter_map(|(id, hash)| {
                let bytes = std::fs::read(thumbs.join(format!("{id}.jpg")))
                    .ok()
                    .or_else(|| hash.as_ref().and_then(|h| std::fs::read(yac.join(format!("{h}.jpg"))).ok()))?;
                Some((*id, crate::archive::cover_color(&bytes).ok()?))
            })
            .collect();
        let tx = conn.transaction()?;
        for (id, color) in &found {
            crate::db::set_cover_color(&tx, *id, color)?;
        }
        tx.commit()?;
        done += found.len();
    }
    Ok(done)
}

pub fn cover_ratios(conn: &mut Connection, root: &Path) -> Result<usize> {
    let covers = root.join(".yacreaderlibrary").join("covers");
    let todo: Vec<(i64, String)> = conn
        .prepare("SELECT id, yac_hash FROM comics WHERE cover_ratio IS NULL AND yac_hash IS NOT NULL")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut done = 0;
    for chunk in todo.chunks(250) {
        let tx = conn.transaction()?;
        for (id, hash) in chunk {
            let path = covers.join(format!("{hash}.jpg"));
            let dims = image::ImageReader::open(&path)
                .and_then(|r| r.with_guessed_format())
                .ok()
                .and_then(|r| r.into_dimensions().ok());
            if let Some((w, h)) = dims.filter(|(w, _)| *w > 0) {
                crate::db::set_cover_ratio(&tx, *id, h as f64 / w as f64)?;
                done += 1;
            }
        }
        tx.commit()?;
    }
    Ok(done)
}

/// Natural order within each series: root files first, then each section.
fn assign_sort_keys(conn: &Connection) -> Result<()> {
    let mut st = conn.prepare("SELECT id, series_id, section, file_name FROM comics")?;
    let mut rows: Vec<(i64, i64, String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<_>>()?;
    rows.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| a.2.is_empty().cmp(&b.2.is_empty()).reverse())
            .then_with(|| natord::compare_ignore_case(&a.2, &b.2))
            .then_with(|| natord::compare_ignore_case(&a.3, &b.3))
    });
    let mut up = conn.prepare("UPDATE comics SET sort_key=?2 WHERE id=?1")?;
    for (i, r) in rows.iter().enumerate() {
        up.execute(params![r.0, i as i64])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso_date("01/12/2021").as_deref(), Some("2021-12-01"));
        assert_eq!(iso_date("5/3/1999").as_deref(), Some("1999-03-05"));
        assert_eq!(iso_date("2021-12-01").as_deref(), Some("2021-12-01"));
        assert_eq!(iso_date(" "), None);
    }

    #[test]
    fn labels() {
        assert_eq!(
            label_for("A Certain Scientific Accelerator v01 (F2) (2015) (Digital) (DigitalMangaFan).cbz", "A Certain Scientific Accelerator", true, None),
            "Vol. 1"
        );
        assert_eq!(
            label_for("All Star Batman & Robin, The Boy Wonder 001 (2005) (digital) (Minutemen-PhD).cbr", "All Star Batman & Robin, The Boy Wonder", false, None),
            "#1"
        );
        assert_eq!(label_for("Doctor Strange - The Oath.cbr", "Doctor Strange - The Oath", false, None), "Doctor Strange - The Oath");
        assert_eq!(label_for("Saga 054 (2018).cbz", "Saga", false, None), "#54");
        assert_eq!(label_for("Ben 10 004 (2026) (digital) (Pyrate-DCP).cbz", "Ben 10", false, None), "#4");
        assert_eq!(label_for("Marvel 1985 (2009) (Digital) (Asgard-Empire).cbz", "Marvel 1985", false, None), "Marvel 1985");
        assert_eq!(
            label_for("Hellboy and the B.P.R.D.- 1952 - 1954 (2021) (digital).cbz", "Hellboy and the B.P.R.D. - 1952 – 1954", false, None),
            "Hellboy and the B.P.R.D.- 1952 - 1954"
        );
    }
}

#[cfg(test)]
mod yacreader_marks {
    use super::*;

    /// Books marked read in YACReader without being opened (no time to go by) come across once
    /// each, when the mark is new there; marking one unread here sticks.
    #[test]
    fn read_marks_without_a_time() {
        let base = std::env::temp_dir().join(format!("aion-yac-marks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Comics");
        let series = root.join("Manga").join("Test Series");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(root.join(".yacreaderlibrary")).unwrap();
        let yac = Connection::open(root.join(".yacreaderlibrary").join("library.ydb")).unwrap();
        yac.execute_batch(
            "CREATE TABLE comic(id INTEGER PRIMARY KEY, comicInfoId INTEGER, path TEXT);
             CREATE TABLE comic_info(id INTEGER PRIMARY KEY, title TEXT, number TEXT, volume TEXT, date TEXT,
               writer TEXT, publisher TEXT, synopsis TEXT, comicVineID TEXT, hash TEXT, numPages INTEGER,
               currentPage INTEGER DEFAULT 1, read BOOLEAN DEFAULT 0, lastTimeOpened INTEGER, added INTEGER);",
        )
        .unwrap();
        for i in 1..=3 {
            std::fs::write(series.join(format!("Test Series v{i:02}.cbz")), b"PK").unwrap();
            yac.execute(
                "INSERT INTO comic_info(id, read) VALUES(?1, ?2)",
                params![i, i < 3],
            )
            .unwrap();
            yac.execute("INSERT INTO comic VALUES(?1, ?1, ?2)", params![i, format!("/Manga/Test Series/Test Series v{i:02}.cbz")]).unwrap();
        }
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        let read = |c: &Connection, i: i64| -> (bool, Option<i64>) {
            c.query_row(
                "SELECT read, last_read_at FROM comics WHERE file_name=?1",
                [format!("Test Series v{i:02}.cbz")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        scan(&mut conn, &root).unwrap();
        // Read, without a time: they don't jump to the top of what you've been reading.
        assert_eq!((read(&conn, 1), read(&conn, 2), read(&conn, 3)), ((true, None), (true, None), (false, None)));

        let id: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Test Series v01.cbz'", [], |r| r.get(0)).unwrap();
        crate::db::set_read(&conn, id, false, 500).unwrap();
        scan(&mut conn, &root).unwrap();
        assert_eq!(read(&conn, 1), (false, None), "marked unread here, it stays unread");

        yac.execute("UPDATE comic_info SET read=1 WHERE id=3", []).unwrap();
        scan(&mut conn, &root).unwrap();
        assert!(read(&conn, 3).0, "marked read in YACReader since");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A scan of your real library into a copy of your database: lists every book whose reading
    /// it changes. Nothing real is written. `cargo test --lib real_rescan -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_rescan() {
        let real = Path::new(&std::env::var("APPDATA").unwrap()).join("com.aion.comics").join("library.db");
        let copy = std::env::temp_dir().join("aion-real-rescan.db");
        let _ = std::fs::remove_file(&copy);
        Connection::open_with_flags(&real, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .execute("VACUUM INTO ?1", [copy.to_string_lossy()])
            .unwrap();
        let mut conn = crate::db::open(&copy).unwrap();
        let reading = |c: &Connection| -> HashMap<String, (bool, i64, Option<i64>)> {
            let mut st = c.prepare("SELECT rel_path, read, current_page, last_read_at FROM comics").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))).unwrap().map(Result::unwrap).collect()
        };
        let before = reading(&conn);
        scan(&mut conn, Path::new(r"H:\Comics")).unwrap();
        let after = reading(&conn);
        let mut changed: Vec<_> = after.iter().filter(|(p, v)| before.get(*p) != Some(v)).collect();
        changed.sort();
        for (p, v) in &changed {
            println!("{p}: {:?} -> {v:?}", before.get(*p));
        }
        println!("{} books' reading changed", changed.len());
        // A second scan changes nothing more.
        scan(&mut conn, Path::new(r"H:\Comics")).unwrap();
        assert_eq!(reading(&conn), after);
    }
}

#[cfg(test)]
mod layouts {
    use super::*;

    fn sorted(paths: &[&str], layout: Layout) -> Vec<(String, String, String, String)> {
        let raw = paths
            .iter()
            .map(|p| Raw { parts: p.split('/').map(str::to_string).collect(), size: 1, mtime: 1 })
            .collect();
        lay_out(raw, "Comics", layout).into_iter().map(|f| (f.collection, f.series, f.series_rel, f.section)).collect()
    }
    fn row(c: &str, s: &str, r: &str, x: &str) -> (String, String, String, String) {
        (c.into(), s.into(), r.into(), x.into())
    }

    #[test]
    fn libraries_as_always() {
        let got = sorted(&["DC/Absolute Batman/Absolute Batman 001.cbz", "DC/Saga/Vol 1/Saga 001.cbz"], Layout::Auto);
        assert_eq!(got[0], row("DC", "Absolute Batman", "DC/Absolute Batman", ""));
        assert_eq!(got[1], row("DC", "Saga", "DC/Saga", "Vol 1"));
    }

    #[test]
    fn a_folder_per_series() {
        let books = ["Saga/Saga 001.cbz", "Saga/Saga 002.cbz", "Vinland Saga/v01.cbz", "Batman/Batman - Year One.cbz", "Hulk (2008)/Hulk 001.cbz", "Bone/Vol 1/01.cbz", "Bone/Vol 2/01.cbz"];
        let got = sorted(&books, Layout::Auto);
        assert_eq!(got[0], row("Comics", "Saga", "Saga", ""));
        assert_eq!(got[1], row("Comics", "Saga", "Saga", ""));
        assert_eq!(got[2], row("Comics", "Vinland Saga", "Vinland Saga", ""));
        assert_eq!(got[3], row("Comics", "Batman", "Batman", ""));
        assert_eq!(got[4], row("Comics", "Hulk (2008)", "Hulk (2008)", ""));
        assert_eq!(got[5], row("Comics", "Bone", "Bone", "Vol 1"));
    }

    #[test]
    fn loose_in_the_comics_folder() {
        let got = sorted(&["Saga 001.cbz", "Saga 002.cbz", "Maus.cbz", "Bone/Bone 001.cbz", "Bone 002.cbz"], Layout::Auto);
        assert_eq!(got[0], row("Comics", "Saga", "Saga", ""));
        assert_eq!(got[1], row("Comics", "Saga", "Saga", ""));
        assert_eq!(got[2], row("Comics", "Maus", "Maus.cbz", ""));
        assert_eq!(got[3], row("Comics", "Bone", "Bone", ""));
        assert_eq!(got[4], row("Comics", "Bone", "Bone", ""), "joins the series folder beside it");
    }

    #[test]
    fn loose_in_a_library() {
        let got = sorted(&["Marvel/Saga 001.cbz", "Marvel/Saga 002.cbz", "Marvel/Hulk 001.cbz", "Marvel/Thor/Thor 001.cbz", "Marvel/Thor 002.cbz"], Layout::Auto);
        assert_eq!(got[0], row("Marvel", "Saga", "Marvel/Saga", ""));
        assert_eq!(got[1], row("Marvel", "Saga", "Marvel/Saga", ""));
        assert_eq!(got[2], row("Marvel", "Hulk 001", "Marvel/Hulk 001.cbz", ""));
        assert_eq!(got[4], row("Marvel", "Thor", "Marvel/Thor", ""));
    }

    #[test]
    fn your_call_beats_the_guess() {
        let books = ["Saga/Saga 001.cbz", "Marvel/Thor/Thor 001.cbz"];
        assert_eq!(sorted(&books, Layout::Libraries)[0], row("Saga", "Saga 001", "Saga/Saga 001.cbz", ""));
        assert_eq!(sorted(&books, Layout::Series)[1], row("Comics", "Marvel", "Marvel", "Thor"));
    }

    #[test]
    fn mixed_and_named_like_the_folder() {
        let got = sorted(&["Saga/Saga 001.cbz", "Marvel/Thor/Thor 001.cbz", "Comics/Thor/Thor 001.cbz"], Layout::Auto);
        assert_eq!(got[0], row("Comics (main folder)", "Saga", "Saga", ""));
        assert_eq!(got[1], row("Marvel", "Thor", "Marvel/Thor", ""));
        assert_eq!(got[2], row("Comics", "Thor", "Comics/Thor", ""));
    }

    #[test]
    fn a_library_remembers_how_it_reads() {
        let base = std::env::temp_dir().join(format!("aion-remembers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Comics");
        std::fs::create_dir_all(root.join("Shonen/Bleach")).unwrap();
        std::fs::write(root.join("Shonen/Bleach/Bleach 001.cbz"), b"PK").unwrap();
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        scan(&mut conn, &root).unwrap();
        let reads = |c: &Connection, file: &str| {
            let (id, manga): (i64, bool) = c
                .query_row(
                    "SELECT c.id, COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga) FROM comics c
                     JOIN series s ON s.id=c.series_id JOIN collections col ON col.id=s.collection_id WHERE c.file_name=?1",
                    [file],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            let f = crate::db::reading_facts(c, id).unwrap();
            crate::db::reads_as(f.said.as_deref(), f.yac_type, false, manga)
        };
        let label = |c: &Connection, file: &str| -> String {
            c.query_row("SELECT label FROM comics WHERE file_name=?1", [file], |r| r.get(0)).unwrap()
        };
        assert_eq!(reads(&conn, "Bleach 001.cbz"), (false, false));
        assert_eq!(label(&conn, "Bleach 001.cbz"), "#1");

        let shonen: i64 = conn.query_row("SELECT id FROM collections WHERE name='Shonen'", [], |r| r.get(0)).unwrap();
        crate::db::set_library_reading(&conn, shonen, Some("manga")).unwrap();
        scan(&mut conn, &root).unwrap();
        assert_eq!(reads(&conn, "Bleach 001.cbz"), (true, false));

        // A series added afterward follows it, labels too, and it survives more scans.
        std::fs::create_dir_all(root.join("Shonen/Naruto")).unwrap();
        std::fs::write(root.join("Shonen/Naruto/Naruto v01.cbz"), b"PK").unwrap();
        scan(&mut conn, &root).unwrap();
        scan(&mut conn, &root).unwrap();
        assert_eq!(reads(&conn, "Naruto v01.cbz"), (true, false), "added later");
        assert!(label(&conn, "Naruto v01.cbz").starts_with("Vol"), "{}", label(&conn, "Naruto v01.cbz"));

        // One series can still differ, until the library is told again.
        let naruto: i64 = conn.query_row("SELECT id FROM series WHERE rel_path='Shonen/Naruto'", [], |r| r.get(0)).unwrap();
        crate::db::set_series_reading(&conn, naruto, Some("webtoon")).unwrap();
        assert_eq!(reads(&conn, "Naruto v01.cbz"), (false, true));
        assert_eq!(reads(&conn, "Bleach 001.cbz"), (true, false));
        crate::db::set_library_reading(&conn, shonen, None).unwrap();
        assert_eq!(reads(&conn, "Naruto v01.cbz"), (false, false));
        assert_eq!(reads(&conn, "Bleach 001.cbz"), (false, false));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn empty_folders_are_libraries_waiting() {
        let base = std::env::temp_dir().join(format!("aion-empties-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Comics");
        std::fs::create_dir_all(root.join("Favorites")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        std::fs::create_dir_all(root.join("Marvel/Thor")).unwrap();
        std::fs::write(root.join("Marvel/Thor/Thor 001.cbz"), b"PK").unwrap();
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        let names = |c: &Connection| -> Vec<String> {
            c.prepare("SELECT name FROM collections ORDER BY name").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        };
        scan(&mut conn, &root).unwrap();
        assert_eq!(names(&conn), ["Favorites", "Marvel"]);
        std::fs::remove_dir_all(root.join("Favorites")).unwrap();
        scan(&mut conn, &root).unwrap();
        assert_eq!(names(&conn), ["Marvel"], "and go when the folder does");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Your real library, scanned into a copy of your database by these rules: every library, series,
    /// book and reading position must come out as it went in. Nothing real is written.
    /// `cargo test --lib real_layout -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_layout_changes_nothing() {
        let real = Path::new(&std::env::var("APPDATA").unwrap()).join("com.aion.comics").join("library.db");
        let copy = std::env::temp_dir().join("aion-real-layout.db");
        let _ = std::fs::remove_file(&copy);
        Connection::open_with_flags(&real, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .execute("VACUUM INTO ?1", [copy.to_string_lossy()])
            .unwrap();
        let mut conn = crate::db::open(&copy).unwrap();
        let rows = |c: &Connection, sql: &str| -> Vec<String> {
            let mut st = c.prepare(sql).unwrap();
            let n = st.column_count();
            let mut got: Vec<String> = st
                .query_map([], |r| {
                    let cell = |i| match r.get_ref(i).unwrap() {
                        rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
                        other => format!("{other:?}"),
                    };
                    Ok((0..n).map(cell).collect::<Vec<_>>().join(" | "))
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            got.sort();
            got
        };
        // PDFs and EPUBs read now: those may be new. Everything that was here must still be.
        let newly_read = |r: &&String| r.contains(".pdf") || r.contains(".epub") || r.contains("Zelda Encyclopedia") || r.contains("Field Guide");
        const SQL: [&str; 3] = [
            "SELECT id, name, is_manga FROM collections",
            "SELECT id, collection_id, trim(name), rel_path FROM series",
            "SELECT id, series_id, section, rel_path, file_name, label, read, current_page, last_read_at FROM comics",
        ];
        let before: Vec<_> = SQL.iter().map(|q| rows(&conn, q)).collect();
        let stats = scan(&mut conn, Path::new(r"H:\Comics")).unwrap();
        println!("{} libraries, {} series, {} books", stats.collections, stats.series, stats.comics);
        for (q, was) in SQL.iter().zip(&before) {
            let now = rows(&conn, q);
            let gone: Vec<_> = was.iter().filter(|r| !now.contains(r)).take(20).collect();
            let new: Vec<_> = now.iter().filter(|r| !was.contains(r) && !newly_read(r)).take(20).collect();
            println!("{q}\n  {} rows; {} gone, {} new", now.len(), gone.len(), new.len());
            assert!(gone.is_empty() && new.is_empty(), "gone: {gone:#?}\nnew: {new:#?}");
        }
        let at_top: i64 = conn.query_row("SELECT COUNT(*) FROM collections WHERE at_top<>0", [], |r| r.get(0)).unwrap();
        assert_eq!(at_top, 0);
    }
}

#[cfg(test)]
mod found_by_hunting {
    use super::*;

    fn fresh(name: &str) -> (std::path::PathBuf, std::path::PathBuf, Connection) {
        let base = std::env::temp_dir().join(format!("aion-hunt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Comics");
        std::fs::create_dir_all(&root).unwrap();
        let conn = crate::db::open(&base.join("t.db")).unwrap();
        (base, root, conn)
    }
    fn put(root: &Path, rel: &str, bytes: usize) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, vec![b'P'; bytes]).unwrap();
    }
    fn count(c: &Connection, sql: &str) -> i64 {
        c.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn a_new_book_never_takes_a_gone_books_number() {
        let (base, root, mut conn) = fresh("number");
        put(&root, "DC/Saga/Saga 001.cbz", 10);
        put(&root, "DC/Saga/Saga 002.cbz", 20);
        scan(&mut conn, &root).unwrap();
        let old: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Saga 002.cbz'", [], |r| r.get(0)).unwrap();
        crate::db::toggle_bookmark(&conn, old, 3, 1).unwrap();
        // The newest book goes, then a different one arrives.
        std::fs::remove_file(root.join("DC/Saga/Saga 002.cbz")).unwrap();
        scan(&mut conn, &root).unwrap();
        put(&root, "DC/Maus/Maus.cbz", 30);
        scan(&mut conn, &root).unwrap();
        let new: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Maus.cbz'", [], |r| r.get(0)).unwrap();
        assert_ne!(new, old, "the new book would get the old one's cover and details");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM bookmarks"), 0, "nothing kept about a book that's gone");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn moving_or_renaming_keeps_your_place() {
        let (base, root, mut conn) = fresh("moved");
        put(&root, "DC/Saga/Saga 001.cbz", 2000);
        put(&root, "DC/Other/Book.cbz", 3 << 20);
        put(&root, "DC/Same/Twin.cbz", 5000);
        put(&root, "DC/Same2/Twin.cbz", 5000);
        scan(&mut conn, &root).unwrap();
        for (file, page) in [("Saga 001.cbz", 7), ("Book.cbz", 9)] {
            conn.execute("UPDATE comics SET current_page=?2, last_read_at=100 WHERE file_name=?1", params![file, page]).unwrap();
        }
        let saga: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Saga 001.cbz'", [], |r| r.get(0)).unwrap();
        crate::db::toggle_bookmark(&conn, saga, 3, 1).unwrap();

        // A folder renamed in Explorer.
        std::fs::rename(root.join("DC/Saga"), root.join("DC/Saga (2012)")).unwrap();
        // A big file renamed, keeping its size and date.
        std::fs::rename(root.join("DC/Other/Book.cbz"), root.join("DC/Other/Better Name.cbz")).unwrap();
        // Two look-alikes, one moved: can't tell which, so nothing is assumed.
        std::fs::rename(root.join("DC/Same2/Twin.cbz"), root.join("DC/Same/Twin 2.cbz")).unwrap();
        scan(&mut conn, &root).unwrap();

        assert_eq!(count(&conn, "SELECT current_page FROM comics WHERE rel_path='DC/Saga (2012)/Saga 001.cbz'"), 7);
        assert_eq!(count(&conn, "SELECT id FROM comics WHERE rel_path='DC/Saga (2012)/Saga 001.cbz'"), saga, "the same book");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM bookmarks"), 1);
        assert_eq!(count(&conn, "SELECT current_page FROM comics WHERE file_name='Better Name.cbz'"), 9);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM series WHERE rel_path='DC/Saga'"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics"), 4);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_small_look_alike_is_not_a_rename() {
        let (base, root, mut conn) = fresh("alike");
        put(&root, "DC/A/One.cbz", 500);
        scan(&mut conn, &root).unwrap();
        conn.execute("UPDATE comics SET current_page=4 WHERE file_name='One.cbz'", []).unwrap();
        std::fs::remove_file(root.join("DC/A/One.cbz")).unwrap();
        put(&root, "DC/A/Two.cbz", 500);
        scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT current_page FROM comics WHERE file_name='Two.cbz'"), 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_recycle_bin_is_not_comics() {
        let (base, root, mut conn) = fresh("bin");
        put(&root, "DC/Saga/Saga 001.cbz", 10);
        put(&root, "$RECYCLE.BIN/S-1-5-21/$R12345.cbz", 10);
        put(&root, "System Volume Information/x.cbz", 10);
        scan(&mut conn, &root).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics"), 1);
        let names: Vec<String> = conn.prepare("SELECT name FROM collections").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(names, ["DC"]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn changing_the_comics_folder_really_changes_it() {
        let (base, root, mut conn) = fresh("change");
        for i in 0..40 {
            put(&root, &format!("DC/Saga/Saga {i:03}.cbz"), 100 + i);
        }
        scan(&mut conn, &root).unwrap();
        // Another folder with other books: the old ones aren't "missing", and don't linger.
        let other = base.join("Other");
        put(&other, "Indie/Maus/Maus.cbz", 7);
        crate::db::note_new_root(&conn).unwrap();
        let stats = scan(&mut conn, &other).unwrap();
        assert_eq!(stats.kept_missing, 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics"), 1);
        // And an empty one is empty, once you've chosen it.
        let empty = base.join("Empty");
        std::fs::create_dir_all(&empty).unwrap();
        crate::db::note_new_root(&conn).unwrap();
        scan(&mut conn, &empty).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics"), 0);
        // The guard is still there for a folder that wasn't changed.
        put(&empty, "DC/A/A.cbz", 9);
        scan(&mut conn, &empty).unwrap();
        std::fs::remove_file(empty.join("DC/A/A.cbz")).unwrap();
        assert!(scan(&mut conn, &empty).is_err(), "a folder gone empty by itself is still refused");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn hiding_every_book_in_a_folder_makes_no_library() {
        let (base, root, mut conn) = fresh("hideall");
        put(&root, "Saga/Saga 001.cbz", 10);
        put(&root, "Marvel/Thor/Thor 001.cbz", 10);
        scan(&mut conn, &root).unwrap();
        let book: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Saga 001.cbz'", [], |r| r.get(0)).unwrap();
        let found = crate::removal::find(&conn, &root, &crate::removal::Target::Books(vec![book])).unwrap();
        crate::removal::hide(&conn, &found, "Saga #1", 100).unwrap();
        scan(&mut conn, &root).unwrap();
        let names: Vec<String> = conn.prepare("SELECT name FROM collections").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(names, ["Marvel"], "the folder isn't an empty library");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn deleting_a_series_last_book_makes_no_library() {
        let (base, root, mut conn) = fresh("lastbook");
        put(&root, "Saga/Saga 001.cbz", 10);
        put(&root, "Marvel/Thor/Thor 001.cbz", 10);
        scan(&mut conn, &root).unwrap();
        let book: i64 = conn.query_row("SELECT id FROM comics WHERE file_name='Saga 001.cbz'", [], |r| r.get(0)).unwrap();
        let found = crate::removal::find(&conn, &root, &crate::removal::Target::Books(vec![book])).unwrap();
        crate::removal::delete(&root, &found, &mut |p| Ok(std::fs::remove_file(p)?)).unwrap();
        crate::removal::forget(&conn, &crate::removal::gone(&conn, &root, &found).unwrap()).unwrap();
        scan(&mut conn, &root).unwrap();
        let names: Vec<String> = conn.prepare("SELECT name FROM collections").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(names, ["Marvel"], "its empty folder isn't a new library");
        assert!(!root.join("Saga").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn names_lose_stray_spaces_but_not_their_folders() {
        let (base, root, mut conn) = fresh("spaces");
        // A folder name ending in a space can only be made with a slash after it, on Windows.
        std::fs::create_dir_all(format!("{}/DC/Arrow /", root.display())).unwrap();
        std::fs::write(format!("{}/DC/Arrow /Arrow 001.cbz", root.display()), b"PK").unwrap();
        scan(&mut conn, &root).unwrap();
        let (name, rel): (String, String) = conn.query_row("SELECT name, rel_path FROM series", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((name.as_str(), rel.as_str()), ("Arrow", "DC/Arrow "));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_big_removal_is_quick() {
        let (base, root, mut conn) = fresh("big");
        for i in 0..1500 {
            put(&root, &format!("DC/Saga/Saga {i:04}.cbz"), 10);
        }
        scan(&mut conn, &root).unwrap();
        let dc: i64 = conn.query_row("SELECT id FROM collections WHERE name='DC'", [], |r| r.get(0)).unwrap();
        let found = crate::removal::find(&conn, &root, &crate::removal::Target::Library(dc)).unwrap();
        let started = std::time::Instant::now();
        crate::removal::hide(&conn, &found, "DC", 1).unwrap();
        println!("hid 1500 books in {:?}", started.elapsed());
        assert!(started.elapsed().as_secs_f64() < 3.0, "took {:?}, with the whole app waiting", started.elapsed());
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM comics"), 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod missing_drive {
    use super::*;

    fn library(dir: &Path, n: usize) {
        let series = dir.join("Manga").join("Test Series");
        std::fs::create_dir_all(&series).unwrap();
        for i in 1..=n {
            std::fs::write(series.join(format!("Test Series v{i:02}.cbz")), b"PK").unwrap();
        }
    }

    #[test]
    fn never_empties_the_library() {
        let base = std::env::temp_dir().join(format!("aion-scan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("Comics");
        library(&root, 40);
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        assert_eq!(scan(&mut conn, &root).unwrap().comics, 40);

        // The drive is gone: the scan refuses and nothing changes.
        std::fs::rename(&root, base.join("Unplugged")).unwrap();
        assert!(scan(&mut conn, &root).is_err());
        let count = |c: &Connection| c.query_row("SELECT COUNT(*) FROM comics", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(count(&conn), 40);

        // The folder is there but empty (drive not mounted yet): also refused.
        std::fs::create_dir_all(&root).unwrap();
        assert!(scan(&mut conn, &root).is_err());
        assert_eq!(count(&conn), 40);

        // Half the books vanish at once: kept, and reported.
        std::fs::remove_dir_all(&root).unwrap();
        library(&root, 20);
        let stats = scan(&mut conn, &root).unwrap();
        assert_eq!((stats.removed, stats.kept_missing, count(&conn)), (0, 20, 40));

        // A few really deleted: removed as usual.
        std::fs::remove_dir_all(&root).unwrap();
        library(&root, 38);
        let stats = scan(&mut conn, &root).unwrap();
        assert_eq!((stats.removed, count(&conn)), (2, 38));
        let _ = std::fs::remove_dir_all(&base);
    }
}
