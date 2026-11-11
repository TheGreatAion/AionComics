//! A check of every book in the library, started from Settings: is the file there, can it be read
//! as a comic, does it have pages, and does its cover read as a picture. It finds the books that
//! would fail when opened, before you open them. Nothing is changed or fixed: each one found can
//! be shown in Explorer to deal with there.
//!
//! It runs on a thread of its own, giving way to reading (as the other background passes do),
//! and can be stopped. The last results are kept in the database, so they're there after a restart.

use crate::{archive, db, Shared};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

/// What's wrong with a book.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// The file isn't where the library says it is.
    Missing,
    /// It's there, but can't be read as a comic.
    Damaged,
    /// It opens, but there are no pages in it.
    Empty,
    /// Its first page doesn't read as a picture.
    Cover,
}

#[derive(Serialize, Deserialize, Clone)]
struct Found {
    id: i64,
    kind: Kind,
}

#[derive(Default)]
pub struct Health {
    running: bool,
    stop: bool,
    done: usize,
    total: usize,
    found: Vec<Found>,
    /// When the last full check finished (seconds since the epoch).
    finished_at: Option<i64>,
    /// The last results have been read back from the database.
    loaded: bool,
}

pub type Check = Mutex<Health>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    comic: db::ComicCard,
    rel_path: String,
    kind: Kind,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    running: bool,
    done: usize,
    total: usize,
    finished_at: Option<i64>,
    problems: Vec<Problem>,
}

const SAVED: &str = "health_last";

/// What one book's check found, if anything.
pub fn check_book(archives: &archive::Archives, id: i64, path: &std::path::Path) -> Option<Kind> {
    if !path.exists() {
        return Some(Kind::Missing);
    }
    // The page list kept from opening the book before, if the file hasn't changed since; else it's
    // listed now (and kept, which makes the book quicker to open later too).
    let listing = match archives.listing(id, path) {
        Ok(l) if l.pages.is_empty() => return Some(Kind::Empty),
        Ok(l) => l,
        // It reads, but holds no pictures.
        Err(e) if e.to_string().starts_with("No pages found") => return Some(Kind::Empty),
        Err(_) => return Some(Kind::Damaged),
    };
    // A PDF's first page would have to be drawn to tell; that it lists its pages is enough.
    if listing.kind == archive::Kind::Pdf {
        return None;
    }
    let Ok(bytes) = archives.first_page(id, path) else { return Some(Kind::Cover) };
    let size = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok());
    match size {
        Some((w, h)) if w > 0 && h > 0 => None,
        _ => Some(Kind::Cover),
    }
}

/// Start a check of the whole library, unless one is running.
pub fn start(state: &Shared) -> Result<()> {
    {
        let mut h = state.health.lock().unwrap();
        if h.running {
            return Ok(());
        }
        h.running = true;
        h.stop = false;
        h.done = 0;
        h.total = 0;
        h.found.clear();
        h.loaded = true;
    }
    let state = state.clone();
    std::thread::spawn(move || {
        let finished = run(&state).unwrap_or(false);
        let mut h = state.health.lock().unwrap();
        h.running = false;
        if finished {
            h.finished_at = Some(crate::now_ms() / 1000);
            let saved = serde_json::json!({ "at": h.finished_at, "found": h.found });
            let _ = db::set_setting(&state.conn.lock().unwrap(), SAVED, &saved.to_string());
        }
    });
    Ok(())
}

/// Go through every book. False when stopped (or the comics folder isn't there) before the end.
fn run(state: &Shared) -> Result<bool> {
    let Some(root) = state.root() else { return Ok(false) };
    if !root.exists() {
        return Ok(false);
    }
    let books = db::all_book_paths(&state.conn.lock().unwrap())?;
    state.health.lock().unwrap().total = books.len();
    for (id, rel) in books {
        // Reading comes first, and a scan has the drive to itself.
        while crate::now_ms() - state.last_page.load(Ordering::SeqCst) < 5000 || state.scanning.load(Ordering::SeqCst) {
            if state.health.lock().unwrap().stop {
                return Ok(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        if state.health.lock().unwrap().stop {
            return Ok(false);
        }
        let kind = check_book(&state.archives, id, &root.join(&rel));
        let mut h = state.health.lock().unwrap();
        h.done += 1;
        if let Some(kind) = kind {
            h.found.push(Found { id, kind });
        }
    }
    Ok(true)
}

pub fn stop(state: &Shared) {
    state.health.lock().unwrap().stop = true;
}

/// Where the check is, and what it has found (or found last time).
pub fn status(state: &Shared) -> Result<Status> {
    let conn = state.conn.lock().unwrap();
    let mut h = state.health.lock().unwrap();
    if !h.loaded {
        h.loaded = true;
        if let Some(saved) = db::get_setting(&conn, SAVED)?.and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) {
            h.finished_at = saved["at"].as_i64();
            h.found = serde_json::from_value(saved["found"].clone()).unwrap_or_default();
        }
    }
    // Books that have gone from the library since (removed, or fixed and rescanned under a new id)
    // drop out of the list.
    let problems = h
        .found
        .iter()
        .filter_map(|f| {
            let comic = db::card_by_id(&conn, f.id).ok()?;
            let rel_path = db::comic_file(&conn, f.id).ok()?.rel_path;
            Some(Problem { comic, rel_path, kind: f.kind })
        })
        .collect();
    Ok(Status { running: h.running, done: h.done, total: h.total, finished_at: h.finished_at, problems })
}

/// Check one book again (after fixing it in Explorer): it leaves the list if it's fine now.
pub fn recheck(state: &Shared, id: i64) -> Result<Option<Kind>> {
    let (path, _) = state.comic_path(id)?;
    // Whatever was kept of the old file is stale now.
    state.archives.forget_book(id);
    let kind = check_book(&state.archives, id, &path);
    let mut h = state.health.lock().unwrap();
    match kind {
        Some(k) => {
            if let Some(f) = h.found.iter_mut().find(|f| f.id == id) {
                f.kind = k;
            }
        }
        None => h.found.retain(|f| f.id != id),
    }
    let saved = serde_json::json!({ "at": h.finished_at, "found": h.found });
    drop(h);
    let _ = db::set_setting(&state.conn.lock().unwrap(), SAVED, &saved.to_string());
    Ok(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each step of the check on real files, timed, with what went wrong:
    /// `AION_HEALTH_FILES="a.cbz|b.cbr" cargo test --release --lib real_health -- --ignored --nocapture`,
    /// or `AION_TEST_ROOT=H:\Comics` for the first books of each library.
    #[test]
    #[ignore]
    fn real_health() {
        let files: Vec<std::path::PathBuf> = match std::env::var("AION_HEALTH_FILES") {
            Ok(list) => list.split('|').map(Into::into).collect(),
            Err(_) => {
                let root = std::path::PathBuf::from(std::env::var("AION_TEST_ROOT").unwrap());
                walkdir::WalkDir::new(&root)
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| ["cbz", "cbr"].contains(&x.to_string_lossy().to_lowercase().as_str())))
                    .step_by(97)
                    .take(40)
                    .map(|e| e.into_path())
                    .collect()
            }
        };
        let dir = std::env::temp_dir().join(format!("aion-health-real-{}", std::process::id()));
        let archives = archive::Archives::new(&dir);
        let (mut t_list, mut t_first) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        for (i, path) in files.iter().enumerate() {
            let t = std::time::Instant::now();
            let listed = archive::list(path);
            let dl = t.elapsed();
            let t = std::time::Instant::now();
            let first = archives.first_page(i as i64 + 1, path);
            let df = t.elapsed();
            t_list += dl;
            t_first += df;
            let name = path.file_name().unwrap().to_string_lossy();
            let first_says = match &first {
                Ok(b) => format!("{} bytes {:?}", b.len(), &b[..4.min(b.len())]),
                Err(e) => format!("ERR {e}"),
            };
            let listed_says = match &listed {
                Ok(l) => format!("{} pages, first {}", l.pages.len(), l.pages.first().map(|p| p.as_str()).unwrap_or("")),
                Err(e) => format!("ERR {e}"),
            };
            println!("{dl:>9.1?} {df:>9.1?}  {}  | {listed_says} | {first_says} | {:?}", &name[..name.len().min(50)], check_book(&archives, i as i64 + 1000, path));
        }
        println!("listing {t_list:.1?}, first pages {t_first:.1?}, over {} books", files.len());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_what_would_fail_to_open() {
        let dir = std::env::temp_dir().join(format!("aion-health-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archives = archive::Archives::new(&dir.join("cache"));
        // Gone.
        assert_eq!(check_book(&archives, 1, &dir.join("gone.cbz")), Some(Kind::Missing));
        // Not a comic at all.
        let junk = dir.join("junk.cbz");
        std::fs::write(&junk, b"this is not a zip file").unwrap();
        assert_eq!(check_book(&archives, 2, &junk), Some(Kind::Damaged));
        // A zip with no pictures in it.
        let empty = dir.join("empty.cbz");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&empty).unwrap());
            z.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, b"hello").unwrap();
            z.finish().unwrap();
        }
        assert_eq!(check_book(&archives, 3, &empty), Some(Kind::Empty));
        // A first page that's named like a picture but isn't one.
        let bad = dir.join("bad.cbz");
        {
            let mut z = zip::ZipWriter::new(std::fs::File::create(&bad).unwrap());
            z.start_file("001.jpg", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, b"not a jpeg").unwrap();
            z.finish().unwrap();
        }
        assert_eq!(check_book(&archives, 4, &bad), Some(Kind::Cover));
        // A good one.
        let good = dir.join("good.cbz");
        {
            let mut png = Vec::new();
            image::RgbImage::new(4, 6).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
            let mut z = zip::ZipWriter::new(std::fs::File::create(&good).unwrap());
            z.start_file("001.png", zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, &png).unwrap();
            z.finish().unwrap();
        }
        assert_eq!(check_book(&archives, 5, &good), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
