//! Sending your reading back to YACReader, when you've turned it on in Settings.
//!
//! The comics folder is otherwise never written to; this is the one exception you chose. Only
//! YACReader's record of each book's reading changes (its page, whether it's read, when it was
//! last opened): never its details, its folders or the comic files.
//!
//! Every change to a book's page or read state lands in `yac_outbox` (a trigger does it, so
//! nothing that changes reading can miss it). The outbox is sent while YACReader is closed, after
//! a one-time copy of its database is put by AionComics' own backups. A book you opened in
//! YACReader after changing it here is left alone: the next scan brings YACReader's reading in.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use crate::db;

pub const SETTING: &str = "yac_write_back";
const LAST_SENT: &str = "yac_last_sent";

pub fn enabled(conn: &Connection) -> bool {
    db::get_setting(conn, SETTING).ok().flatten().is_some_and(|v| v == "1")
}

pub fn library(root: &Path) -> PathBuf {
    root.join(".yacreaderlibrary").join("library.ydb")
}

/// Queue every book you've read in AionComics, as of when you read it; called when sending is
/// turned on, so YACReader catches up on reading done before.
pub fn queue_all(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO yac_outbox(comic_id, at) SELECT id, last_read_at FROM comics WHERE last_read_at IS NOT NULL
         ON CONFLICT(comic_id) DO NOTHING",
        [],
    )?;
    Ok(())
}

/// Books waiting to be sent (ones a rescan has since removed don't count).
pub fn pending(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM yac_outbox o JOIN comics c ON c.id=o.comic_id", [], |r| r.get(0)).unwrap_or(0)
}

/// YACReader or its library window is open. Its library keeps what it has shown in memory, so
/// AionComics waits for both to close rather than change things under it.
#[cfg(windows)]
pub fn yacreader_open() -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(out) = std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    else {
        // Can't tell: assume it's open, and try again later.
        return true;
    };
    String::from_utf8_lossy(&out.stdout).lines().any(|l| {
        let name = l.split(',').next().unwrap_or("").trim_matches('"').to_ascii_lowercase();
        name == "yacreader.exe" || name == "yacreaderlibrary.exe"
    })
}

#[cfg(not(windows))]
pub fn yacreader_open() -> bool {
    false
}

#[derive(Serialize, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub at: i64,
    pub books: i64,
}

pub fn last_sent(conn: &Connection) -> Option<Sent> {
    let v = db::get_setting(conn, LAST_SENT).ok().flatten()?;
    let (at, books) = v.split_once(',')?;
    Some(Sent { at: at.parse().ok()?, books: books.parse().ok()? })
}

/// A book's reading in AionComics, waiting to be sent.
struct Out {
    id: i64,
    at: i64,
    path: String,
    read: bool,
    page: i64,
    pages: Option<i64>,
    last_read: Option<i64>,
}

/// What to write over YACReader's record of a book, or None when it already says the same (or
/// was opened in YACReader since).
fn update_for(o: &Out, yac: (bool, i64, Option<i64>, Option<i64>)) -> Option<(bool, i64, bool, Option<i64>)> {
    let (y_read, y_page, y_pages, y_opened) = yac;
    if y_opened.unwrap_or(0) > o.at {
        return None;
    }
    // YACReader counts pages from 1; a finished book sits on its last page.
    let (read, page, opened) = if o.read {
        (true, y_pages.or(o.pages).filter(|n| *n > 0).unwrap_or(y_page.max(1)), true)
    } else if o.page > 0 {
        (false, o.page + 1, true)
    } else {
        (false, 1, false)
    };
    let same = if read { y_read } else { !y_read && y_page.max(1) == page };
    (!same).then_some((read, page, opened, o.last_read.or(y_opened)))
}

/// Send what's waiting, if sending is on and YACReader is closed. Returns how many books changed
/// in YACReader.
pub fn flush(db_path: &Path, data_dir: &Path, root: &Path) -> Result<usize> {
    flush_unless(db_path, data_dir, root, yacreader_open)
}

/// `flush`, with what says YACReader is open given (tests can't depend on what's running).
fn flush_unless(db_path: &Path, data_dir: &Path, root: &Path, open: fn() -> bool) -> Result<usize> {
    let conn = db::open(db_path)?;
    if !enabled(&conn) || pending(&conn) == 0 {
        return Ok(0);
    }
    let ydb = library(root);
    if !ydb.exists() || open() {
        return Ok(0);
    }
    conn.execute("DELETE FROM yac_outbox WHERE comic_id NOT IN (SELECT id FROM comics)", [])?;
    let outs: Vec<Out> = conn
        .prepare(
            "SELECT o.comic_id, o.at, c.rel_path, c.read, c.current_page, c.num_pages, c.last_read_at
             FROM yac_outbox o JOIN comics c ON c.id=o.comic_id",
        )?
        .query_map([], |r| {
            Ok(Out {
                id: r.get(0)?,
                at: r.get(1)?,
                path: r.get(2)?,
                read: r.get(3)?,
                page: r.get(4)?,
                pages: r.get(5)?,
                last_read: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    backup_once(&ydb, data_dir)?;
    let mut yac = Connection::open_with_flags(&ydb, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    yac.busy_timeout(std::time::Duration::from_secs(5))?;
    let tx = yac.transaction()?;
    let mut changed = 0;
    {
        let mut find = tx.prepare(
            "SELECT ci.id, ci.read, ci.currentPage, ci.numPages, ci.lastTimeOpened
             FROM comic c JOIN comic_info ci ON ci.id=c.comicInfoId WHERE c.path=?1",
        )?;
        let mut write = tx.prepare(
            "UPDATE comic_info SET read=?2, currentPage=?3, hasBeenOpened=?4, lastTimeOpened=?5 WHERE id=?1",
        )?;
        for o in &outs {
            let path = format!("/{}", o.path.replace('\\', "/"));
            let found = find
                .query_row([&path], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        (
                            r.get::<_, Option<bool>>(1)?.unwrap_or(false),
                            r.get::<_, Option<i64>>(2)?.unwrap_or(1),
                            r.get::<_, Option<i64>>(3)?,
                            r.get::<_, Option<i64>>(4)?,
                        ),
                    ))
                })
                .optional()?;
            if let Some((info, state)) = found {
                if let Some((read, page, opened, when)) = update_for(o, state) {
                    write.execute(params![info, read, page, opened, when])?;
                    changed += 1;
                }
            }
        }
    }
    tx.commit()?;
    // Only what was sent: a page turned meanwhile is queued again with a newer time.
    for o in &outs {
        conn.execute("DELETE FROM yac_outbox WHERE comic_id=?1 AND at=?2", params![o.id, o.at])?;
    }
    if changed > 0 {
        db::set_setting(&conn, LAST_SENT, &format!("{},{changed}", crate::scan::now()))?;
    }
    Ok(changed)
}

/// Where the copy of YACReader's database is kept, made before AionComics first changes it.
pub fn backup_path(data_dir: &Path) -> PathBuf {
    data_dir.join("yacreader-backup").join("library.ydb")
}

fn backup_once(ydb: &Path, data_dir: &Path) -> Result<()> {
    let to = backup_path(data_dir);
    if to.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(to.parent().unwrap())?;
    let from = Connection::open_with_flags(ydb, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    from.execute("VACUUM INTO ?1", [to.to_string_lossy()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(read: bool, page: i64, at: i64) -> Out {
        Out { id: 1, at, path: String::new(), read, page, pages: Some(20), last_read: Some(at) }
    }

    #[test]
    fn what_goes_to_yacreader() {
        // Page 5 here (0-based) is page 6 there.
        assert_eq!(update_for(&out(false, 5, 100), (false, 1, Some(20), Some(50))), Some((false, 6, true, Some(100))));
        // Already the same: nothing to write.
        assert_eq!(update_for(&out(false, 5, 100), (false, 6, Some(20), Some(50))), None);
        // Finished: read, on the last page.
        assert_eq!(update_for(&out(true, 0, 100), (false, 3, Some(20), Some(50))), Some((true, 20, true, Some(100))));
        assert_eq!(update_for(&out(true, 0, 100), (true, 20, Some(20), Some(50))), None);
        // Marked unread: back to the start, not opened.
        let mut unread = out(false, 0, 100);
        unread.last_read = None;
        assert_eq!(update_for(&unread, (true, 20, Some(20), Some(50))), Some((false, 1, false, Some(50))));
        // Opened in YACReader after the change here: YACReader's reading stands.
        assert_eq!(update_for(&out(false, 5, 100), (false, 9, Some(20), Some(150))), None);
    }

    /// A tiny YACReader library, laid out like the real one.
    fn yac_library(dir: &Path) -> PathBuf {
        let ydb = library(dir);
        std::fs::create_dir_all(ydb.parent().unwrap()).unwrap();
        let c = Connection::open(&ydb).unwrap();
        c.execute_batch(
            "CREATE TABLE comic(id INTEGER PRIMARY KEY, parentId INTEGER, comicInfoId INTEGER, fileName TEXT, path TEXT);
             CREATE TABLE comic_info(id INTEGER PRIMARY KEY, numPages INTEGER, read BOOLEAN DEFAULT 0,
               hasBeenOpened BOOLEAN DEFAULT 0, currentPage INTEGER DEFAULT 1, lastTimeOpened INTEGER, title TEXT);
             INSERT INTO comic_info(id, numPages, title) VALUES(7, 30, 'Kept as it is'), (8, 12, NULL);
             INSERT INTO comic VALUES(1, 2, 7, 'Saga 001.cbz', '/Indie/Saga/Saga 001.cbz'),
                                     (2, 2, 8, 'Saga 002.cbz', '/Indie/Saga/Saga 002.cbz');",
        )
        .unwrap();
        ydb
    }

    #[test]
    fn sends_reading_once_and_backs_up_first() {
        let dir = std::env::temp_dir().join(format!("aion-yacsync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (root, data) = (dir.join("comics"), dir.join("data"));
        std::fs::create_dir_all(&data).unwrap();
        let ydb = yac_library(&root);
        let db_path = data.join("library.db");
        {
            let c = db::open(&db_path).unwrap();
            c.execute_batch(
                "INSERT INTO collections(id, name) VALUES(1, 'Indie');
                 INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Saga', 'Indie/Saga');
                 INSERT INTO comics(id, series_id, rel_path, file_name, num_pages)
                   VALUES(1, 1, 'Indie/Saga/Saga 001.cbz', 'Saga 001.cbz', 30),
                         (2, 1, 'Indie/Saga/Saga 002.cbz', 'Saga 002.cbz', 12);",
            )
            .unwrap();
            // Reading while sending is off still queues, so nothing is lost when it's turned on.
            db::set_progress(&c, 1, 9, 30, 1000).unwrap();
            db::set_read(&c, 2, true, 1001).unwrap();
            assert_eq!(pending(&c), 2);
        }
        let (closed, open): (fn() -> bool, fn() -> bool) = (|| false, || true);
        assert_eq!(flush_unless(&db_path, &data, &root, closed).unwrap(), 0, "sending is off");
        db::set_setting(&db::open(&db_path).unwrap(), SETTING, "1").unwrap();
        assert_eq!(flush_unless(&db_path, &data, &root, open).unwrap(), 0, "YACReader is open");
        assert!(!backup_path(&data).exists());
        assert_eq!(flush_unless(&db_path, &data, &root, closed).unwrap(), 2);
        assert!(backup_path(&data).exists());

        let y = Connection::open(&ydb).unwrap();
        let row = |id: i64| -> (bool, i64, bool, Option<i64>, Option<String>) {
            y.query_row(
                "SELECT read, currentPage, hasBeenOpened, lastTimeOpened, title FROM comic_info WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap()
        };
        assert_eq!(row(7), (false, 10, true, Some(1000), Some("Kept as it is".into())));
        assert_eq!(row(8), (true, 12, true, Some(1001), None));
        // The backup is YACReader's library as it was before.
        let b = Connection::open(backup_path(&data)).unwrap();
        let before: i64 = b.query_row("SELECT currentPage FROM comic_info WHERE id=7", [], |r| r.get(0)).unwrap();
        assert_eq!(before, 1);

        // Sent, so nothing waits; and a scan bringing the same reading back in sends nothing.
        let c = db::open(&db_path).unwrap();
        assert_eq!(pending(&c), 0);
        db::set_progress(&c, 1, 9, 30, 1000).unwrap();
        assert_eq!(flush_unless(&db_path, &data, &root, closed).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Everything you've read in AionComics, sent to a copy of your real YACReader library, then
    /// read back the way a scan does: it must come back as the same reading. Nothing real is
    /// written. Run with `cargo test --lib real_round_trip -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_round_trip() {
        let appdata = std::env::var("APPDATA").unwrap();
        let real_db = Path::new(&appdata).join("com.aion.comics").join("library.db");
        let dir = std::env::temp_dir().join("aion-yac-round-trip");
        let _ = std::fs::remove_dir_all(&dir);
        let (root, data) = (dir.join("comics"), dir.join("data"));
        std::fs::create_dir_all(library(&root).parent().unwrap()).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let db_path = data.join("library.db");
        // Copies made through SQLite, so they're whole even while the app runs.
        Connection::open_with_flags(&real_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .execute("VACUUM INTO ?1", [db_path.to_string_lossy()])
            .unwrap();
        Connection::open_with_flags(library(Path::new(r"H:\Comics")), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .execute("VACUUM INTO ?1", [library(&root).to_string_lossy()])
            .unwrap();
        let c = db::open(&db_path).unwrap();
        c.execute("DELETE FROM yac_outbox", []).unwrap();
        db::set_setting(&c, SETTING, "1").unwrap();
        queue_all(&c).unwrap();
        let queued = pending(&c);
        let sent = flush_unless(&db_path, &data, &root, || false).unwrap();
        println!("{queued} books read in AionComics; {sent} changed in YACReader");
        assert_eq!(pending(&c), 0);

        let yac = crate::scan::read_yac(&root);
        let mut st = c
            .prepare("SELECT rel_path, read, current_page, last_read_at FROM comics WHERE last_read_at IS NOT NULL")
            .unwrap();
        let rows: Vec<(String, bool, i64, i64)> =
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect();
        let (mut same, mut yac_newer, mut missing) = (0, 0, 0);
        for (path, read, page, last) in rows {
            let Some(y) = yac.get(&path) else {
                missing += 1;
                continue;
            };
            if y.last_opened.unwrap_or(0) > last {
                yac_newer += 1;
                println!("  YACReader's is newer: {path}");
                continue;
            }
            assert_eq!(y.read, read, "{path}");
            if !read {
                assert_eq!(y.current_page.unwrap_or(1) - 1, page, "{path}");
            }
            same += 1;
        }
        println!("{same} read back the same; {yac_newer} newer in YACReader (left alone); {missing} not in YACReader");
    }
}
