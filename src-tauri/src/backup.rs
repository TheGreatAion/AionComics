//! Copies of the library database, so reading progress, bookmarks, panel edits, Comic Vine matches
//! and hand edits survive a bad day. One a day while AionComics runs (and at launch when the last
//! is older than that), the newest few kept. A copy is a standalone database made by SQLite itself
//! (`VACUUM INTO`), consistent even while the app is writing.
//!
//! Restoring can't swap the database out from under the open app, so it's done on the next launch:
//! the choice is written down, the app restarts, and the copy goes in before anything opens it.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use rusqlite::Connection;
use serde::Serialize;

/// How many copies are kept; older ones are deleted as new ones are made.
pub const KEEP: usize = 10;
/// How old the newest copy may get before another is made.
pub const EVERY_SECS: i64 = 86_400;
const PENDING: &str = "restore-pending";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub name: String,
    /// Unix seconds.
    pub made_at: i64,
    pub size: u64,
}

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// The copies there are, newest first.
pub fn list(dir: &Path) -> Result<Vec<Backup>> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(Vec::new()) };
    let mut out: Vec<Backup> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let made_at = name.strip_prefix("library-")?.strip_suffix(".db")?.parse().ok()?;
            Some(Backup { name, made_at, size: e.metadata().ok()?.len() })
        })
        .collect();
    out.sort_by(|a, b| b.made_at.cmp(&a.made_at));
    Ok(out)
}

/// Make a copy now, then keep only the newest few.
pub fn make(conn: &Connection, dir: &Path, now: i64) -> Result<Backup> {
    std::fs::create_dir_all(dir)?;
    // Two copies in the same second (a restore right after a scheduled copy) get the next second.
    let mut at = now;
    while dir.join(format!("library-{at}.db")).exists() {
        at += 1;
    }
    let name = format!("library-{at}.db");
    let path = dir.join(&name);
    conn.execute("VACUUM INTO ?1", [path.to_string_lossy()])?;
    let size = std::fs::metadata(&path)?.len();
    for old in list(dir)?.into_iter().skip(KEEP) {
        let _ = std::fs::remove_file(dir.join(old.name));
    }
    Ok(Backup { name, made_at: at, size })
}

/// Whether the newest copy is old enough that another is due.
pub fn due(dir: &Path, now: i64) -> bool {
    list(dir).ok().and_then(|l| l.first().map(|b| now - b.made_at >= EVERY_SECS)).unwrap_or(true)
}

/// Ask for `name` to be put back on the next launch.
pub fn schedule(data_dir: &Path, name: &str) -> Result<()> {
    if !list(&dir(data_dir))?.iter().any(|b| b.name == name) {
        bail!("There's no backup called {name}");
    }
    std::fs::write(data_dir.join(PENDING), name)?;
    Ok(())
}

/// At launch, before the database is opened: put back the copy that was asked for, if any.
/// Returns its name when one went in.
pub fn apply_pending(data_dir: &Path, db_path: &Path) -> Result<Option<String>> {
    let marker = data_dir.join(PENDING);
    let Ok(name) = std::fs::read_to_string(&marker) else { return Ok(None) };
    let name = name.trim().to_string();
    let src = dir(data_dir).join(&name);
    // Done with the request either way, so a missing copy can't wedge every launch after.
    std::fs::remove_file(&marker)?;
    if !src.is_file() {
        return Ok(None);
    }
    for ext in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{ext}", db_path.display()));
        let _ = std::fs::remove_file(side);
    }
    std::fs::copy(&src, db_path)?;
    Ok(Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_keeps_and_restores() {
        let data = std::env::temp_dir().join(format!("aion-backup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let db_path = data.join("library.db");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t(v TEXT); INSERT INTO t VALUES('first');").unwrap();
        let dir = dir(&data);
        assert!(due(&dir, 1000));
        let first = make(&conn, &dir, 1000).unwrap();
        assert!(!due(&dir, 1000 + EVERY_SECS - 1) && due(&dir, 1000 + EVERY_SECS));
        // Only the newest KEEP survive.
        for k in 1..=KEEP as i64 + 2 {
            make(&conn, &dir, 1000 + k).unwrap();
        }
        let kept = list(&dir).unwrap();
        assert_eq!(kept.len(), KEEP);
        assert!(kept.iter().all(|b| b.name != first.name));
        // Change the library, then put a copy back the way a launch would.
        let copy = kept.last().unwrap().name.clone();
        conn.execute("UPDATE t SET v='changed'", []).unwrap();
        drop(conn);
        schedule(&data, &copy).unwrap();
        assert!(schedule(&data, "library-1.db").is_err());
        assert_eq!(apply_pending(&data, &db_path).unwrap(), Some(copy));
        let back: String = Connection::open(&db_path).unwrap().query_row("SELECT v FROM t", [], |r| r.get(0)).unwrap();
        assert_eq!(back, "first");
        // Nothing asked for, nothing done.
        assert_eq!(apply_pending(&data, &db_path).unwrap(), None);
    }
}
