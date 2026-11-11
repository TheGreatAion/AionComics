//! AionComics's own library database. Lives in the app data folder; the comics
//! folder itself is never written to.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use crate::editions;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS collections(
  id INTEGER PRIMARY KEY,
  name TEXT UNIQUE NOT NULL,
  is_manga INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS series(
  id INTEGER PRIMARY KEY,
  collection_id INTEGER NOT NULL REFERENCES collections(id),
  name TEXT NOT NULL,
  rel_path TEXT UNIQUE NOT NULL
);
CREATE TABLE IF NOT EXISTS comics(
  id INTEGER PRIMARY KEY,
  series_id INTEGER NOT NULL REFERENCES series(id),
  section TEXT NOT NULL DEFAULT '',
  rel_path TEXT UNIQUE NOT NULL,
  file_name TEXT NOT NULL,
  sort_key INTEGER NOT NULL DEFAULT 0,
  label TEXT NOT NULL DEFAULT '',
  size INTEGER, mtime INTEGER, added_at INTEGER,
  title TEXT, number TEXT, volume TEXT, year TEXT, writer TEXT, publisher TEXT, synopsis TEXT,
  cv_id TEXT, yac_hash TEXT,
  num_pages INTEGER,
  current_page INTEGER NOT NULL DEFAULT 0,
  read INTEGER NOT NULL DEFAULT 0,
  last_read_at INTEGER,
  seen INTEGER NOT NULL DEFAULT 1,
  cover_ratio REAL
);
CREATE INDEX IF NOT EXISTS comics_series ON comics(series_id, sort_key);
CREATE INDEX IF NOT EXISTS comics_last_read ON comics(last_read_at);
CREATE INDEX IF NOT EXISTS comics_added ON comics(added_at);
CREATE INDEX IF NOT EXISTS series_collection ON series(collection_id);
CREATE TABLE IF NOT EXISTS panels(
  comic_id INTEGER NOT NULL,
  page INTEGER NOT NULL,
  rtl INTEGER NOT NULL,
  version INTEGER NOT NULL,
  data TEXT NOT NULL,
  PRIMARY KEY(comic_id, page, rtl)
);
"#;

/// Books opened within this window count as "Continue Reading"; older
/// unfinished ones move to "Pick Up Again".
pub const RECENT_SECS: i64 = 30 * 86_400;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(10))?;
    conn.execute_batch(SCHEMA)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Columns added after the first release of the schema.
fn migrate(conn: &Connection) -> Result<()> {
    // How a series reads when you've said ("comic", "manga", "webtoon"); otherwise it's worked out.
    if !conn.prepare("SELECT 1 FROM pragma_table_info('series') WHERE name='reading'")?.exists([])? {
        conn.execute("ALTER TABLE series ADD COLUMN reading TEXT", [])?;
    }
    // The order you've dragged a series' formats into, comma-separated. The first is what it opens on.
    if !conn.prepare("SELECT 1 FROM pragma_table_info('series') WHERE name='formats'")?.exists([])? {
        conn.execute("ALTER TABLE series ADD COLUMN formats TEXT", [])?;
    }
    // How a library's series read when you've said, those it has and those added later.
    if !conn.prepare("SELECT 1 FROM pragma_table_info('collections') WHERE name='reading'")?.exists([])? {
        conn.execute("ALTER TABLE collections ADD COLUMN reading TEXT", [])?;
    }
    // Whether a book's pages turned out to be the long kind webtoons are made of, once looked at.
    if !conn.prepare("SELECT 1 FROM pragma_table_info('comics') WHERE name='tall'")?.exists([])? {
        conn.execute("ALTER TABLE comics ADD COLUMN tall INTEGER", [])?;
    }
    // What you've hidden (see removal.rs): files or folders the scan passes over, and where you'd
    // read to in each book, for putting it back.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS hidden(rel_path TEXT PRIMARY KEY, label TEXT NOT NULL, at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS hidden_reading(rel_path TEXT PRIMARY KEY, read INTEGER NOT NULL, current_page INTEGER NOT NULL, last_read_at INTEGER);",
    )?;
    // The library for series that sit straight in the comics folder, with no folder of its own.
    if !conn.prepare("SELECT 1 FROM pragma_table_info('collections') WHERE name='at_top'")?.exists([])? {
        conn.execute("ALTER TABLE collections ADD COLUMN at_top INTEGER NOT NULL DEFAULT 0", [])?;
    }
    let has = |col: &str| -> Result<bool> {
        let mut st = conn.prepare("SELECT 1 FROM pragma_table_info('comics') WHERE name=?1")?;
        Ok(st.exists([col])?)
    };
    if !has("cover_ratio")? {
        conn.execute("ALTER TABLE comics ADD COLUMN cover_ratio REAL", [])?;
    }
    // The color a cover reads as ("#rrggbb"), for the wash behind a book.
    if !has("cover_color")? {
        conn.execute("ALTER TABLE comics ADD COLUMN cover_color TEXT", [])?;
    }
    // Metadata beyond the basics, plus a snapshot of what the scan imported (the base layer edits sit on).
    for col in META_FIELDS.iter().chain(["meta_base"].iter()) {
        if !has(col)? {
            conn.execute(&format!("ALTER TABLE comics ADD COLUMN {col} TEXT"), [])?;
        }
    }
    conn.execute_batch(
        // Comic Vine data ('cv') and hand edits ('user') for a book: a JSON object of field -> value.
        // A hand edit of "" means the field was cleared on purpose.
        "CREATE TABLE IF NOT EXISTS meta_layer(
           comic_id INTEGER NOT NULL,
           source TEXT NOT NULL,
           data TEXT NOT NULL,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(comic_id, source)
         );
         CREATE TABLE IF NOT EXISTS cv_series(
           series_id INTEGER PRIMARY KEY,
           volume_id TEXT NOT NULL,
           data TEXT NOT NULL,
           updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS cv_queue(
           comic_id INTEGER PRIMARY KEY,
           issue_id TEXT NOT NULL,
           added_at INTEGER NOT NULL,
           tries INTEGER NOT NULL DEFAULT 0,
           last_error TEXT
         );",
    )?;
    // No foreign keys: rescans delete and re-add comics, and these should simply go stale.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS bookmarks(
           comic_id INTEGER NOT NULL,
           page INTEGER NOT NULL,
           created_at INTEGER NOT NULL,
           PRIMARY KEY(comic_id, page)
         );
         CREATE TABLE IF NOT EXISTS user_panels(
           comic_id INTEGER NOT NULL,
           page INTEGER NOT NULL,
           data TEXT NOT NULL,
           PRIMARY KEY(comic_id, page)
         );
         CREATE TABLE IF NOT EXISTS guided_spots(
           comic_id INTEGER PRIMARY KEY,
           page INTEGER NOT NULL,
           panel INTEGER NOT NULL,
           part INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS comicinfo_seen(
           comic_id INTEGER PRIMARY KEY,
           size INTEGER NOT NULL,
           mtime INTEGER NOT NULL
         );",
    )?;
    crate::stats::migrate(conn)?;
    // Groups of books you've said aren't duplicates, by their sorted ids.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS duplicate_ignored(key TEXT PRIMARY KEY);")?;
    // Formats you've given books by hand, over the ones worked out from their names.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS edition_override(comic_id INTEGER PRIMARY KEY, kind TEXT NOT NULL);")?;
    // How YACReader says a book reads (0 comic, 1 manga, 2 left-to-right manga, 3 webtoon).
    if !has("yac_type")? {
        conn.execute("ALTER TABLE comics ADD COLUMN yac_type INTEGER", [])?;
    }
    // Whether YACReader called a book read at the last scan, so a read mark there is taken once,
    // when it's new.
    if !has("yac_read")? {
        conn.execute("ALTER TABLE comics ADD COLUMN yac_read INTEGER", [])?;
    }
    // Books whose page or read state changed, and when, waiting to be sent to YACReader (see
    // yacsync.rs). A trigger, so every way of changing reading is caught.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS yac_outbox(comic_id INTEGER PRIMARY KEY, at INTEGER NOT NULL);
         CREATE TRIGGER IF NOT EXISTS yac_outbox_reading AFTER UPDATE OF read, current_page ON comics
         WHEN OLD.read IS NOT NEW.read OR OLD.current_page IS NOT NEW.current_page
         BEGIN
           INSERT INTO yac_outbox(comic_id, at) VALUES(NEW.id, CAST(strftime('%s', 'now') AS INTEGER))
           ON CONFLICT(comic_id) DO UPDATE SET at=excluded.at;
         END;",
    )?;
    // Books you took off Continue reading, and when. Reading one again puts it back.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS reading_hidden(comic_id INTEGER PRIMARY KEY, at INTEGER NOT NULL);")?;
    // Your Want to read list: whole series, and single books (an omnibus, one volume), and when each
    // went on it.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS want_series(series_id INTEGER PRIMARY KEY, at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS want_books(comic_id INTEGER PRIMARY KEY, at INTEGER NOT NULL);",
    )?;
    // Issues out on Comic Vine past the last one you have, for series you're reading or want to
    // (see cv::check_new): their numbers as a JSON list, and when that was last looked at.
    conn.execute_batch("CREATE TABLE IF NOT EXISTS cv_new(series_id INTEGER PRIMARY KEY, numbers TEXT NOT NULL, checked_at INTEGER NOT NULL);")?;
    // Books in a matched series that were tried for an automatic Comic Vine pairing. Settled ones
    // are left alone for good: you left them unpaired in the matcher.
    let fresh: bool = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='cv_auto'", [], |r| r.get::<_, i64>(0))? == 0;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS cv_auto(
           comic_id INTEGER PRIMARY KEY,
           tried_at INTEGER NOT NULL,
           settled INTEGER NOT NULL DEFAULT 0
         );",
    )?;
    if fresh {
        // Books already sitting unpaired in a matched series were your call in the matcher.
        conn.execute(
            "INSERT OR IGNORE INTO cv_auto(comic_id, tried_at, settled)
             SELECT c.id, 0, 1 FROM comics c JOIN cv_series s ON s.series_id=c.series_id
             WHERE NOT EXISTS (SELECT 1 FROM meta_layer v WHERE v.comic_id=c.id AND v.source='cv'
                               AND json_extract(v.data, '$.cv_id') IS NOT NULL)",
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
pub fn migrate_for_tests(conn: &Connection) -> Result<()> {
    migrate(conn)
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------- view models ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ComicCard {
    pub id: i64,
    pub label: String,
    pub title: Option<String>,
    pub year: Option<String>,
    pub series_id: i64,
    pub series_name: String,
    pub current_page: i64,
    pub num_pages: Option<i64>,
    pub read: bool,
    pub is_manga: bool,
    /// Height / width of the cover, so the grid can reserve the right shape.
    pub cover_ratio: Option<f64>,
    pub last_read_at: Option<i64>,
    /// The color the cover reads as, "#rrggbb", once it's been worked out.
    pub cover_color: Option<String>,
    /// On the series page: the book's format (see editions.rs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// On the series page: the issues a collection holds, "#13–18", when its description says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collects: Option<String>,
    /// On the series page: what an annual or special follows in the story, "#12".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

const CARD_COLS: &str = "c.id, c.label, c.title, c.year, s.id, s.name, c.current_page, c.num_pages, c.read,
  COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga), c.cover_ratio, c.last_read_at, c.cover_color";
const CARD_JOIN: &str = "JOIN series s ON s.id=c.series_id JOIN collections col ON col.id=s.collection_id";

fn card(r: &Row) -> rusqlite::Result<ComicCard> {
    Ok(ComicCard {
        id: r.get(0)?,
        label: r.get(1)?,
        title: r.get(2)?,
        year: r.get(3)?,
        series_id: r.get(4)?,
        series_name: r.get(5)?,
        current_page: r.get(6)?,
        num_pages: r.get(7)?,
        read: r.get(8)?,
        is_manga: r.get(9)?,
        cover_ratio: r.get(10)?,
        last_read_at: r.get(11)?,
        cover_color: r.get(12)?,
        edition: None,
        collects: None,
        after: None,
    })
}

fn cards(conn: &Connection, tail: &str, p: impl rusqlite::Params) -> Result<Vec<ComicCard>> {
    let mut st = conn.prepare(&format!("SELECT {CARD_COLS} FROM comics c {CARD_JOIN} {tail}"))?;
    let v = st.query_map(p, card)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

/// Leaves out books you took off Continue reading and haven't read since.
const NOT_HIDDEN: &str =
    "NOT EXISTS (SELECT 1 FROM reading_hidden h WHERE h.comic_id=c.id AND h.at>=COALESCE(c.last_read_at, 0))";

pub fn continue_reading(conn: &Connection, now: i64, limit: i64) -> Result<Vec<ComicCard>> {
    cards(
        conn,
        &format!("WHERE c.read=0 AND c.current_page>0 AND c.last_read_at>=?1 AND {NOT_HIDDEN} ORDER BY c.last_read_at DESC LIMIT ?2"),
        params![now - RECENT_SECS, limit],
    )
}

pub fn pick_up_again(conn: &Connection, now: i64, limit: i64) -> Result<Vec<ComicCard>> {
    cards(
        conn,
        &format!(
            "WHERE c.read=0 AND c.current_page>0 AND (c.last_read_at<?1 OR c.last_read_at IS NULL) AND {NOT_HIDDEN}
             ORDER BY c.last_read_at DESC NULLS LAST LIMIT ?2"
        ),
        params![now - RECENT_SECS, limit],
    )
}

/// Take a book off Continue reading (and Pick up again). Its place in the book is kept. Returns how
/// it was, for Undo.
pub fn hide_from_reading(conn: &Connection, id: i64, now: i64) -> Result<Vec<ReadState>> {
    let before = read_states(conn, "c.id=?1", [id])?;
    conn.execute(
        "INSERT INTO reading_hidden(comic_id, at) VALUES(?1, ?2) ON CONFLICT(comic_id) DO UPDATE SET at=excluded.at",
        params![id, now],
    )?;
    Ok(before)
}

/// Put a book taken off Continue reading back on it.
pub fn unhide_from_reading(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM reading_hidden WHERE comic_id=?1", [id])?;
    Ok(())
}

/// Books taken off Continue reading and not read since, the last one taken off first.
pub fn taken_off_reading(conn: &Connection) -> Result<Vec<ComicCard>> {
    cards(
        conn,
        "JOIN reading_hidden h ON h.comic_id=c.id
         WHERE c.read=0 AND c.current_page>0 AND h.at>=COALESCE(c.last_read_at, 0) ORDER BY h.at DESC",
        [],
    )
}

/// A book's reading state before a change (marking it read, taking it off Continue reading), so the
/// change can be undone.
#[derive(Serialize, serde::Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReadState {
    pub id: i64,
    pub read: bool,
    pub current_page: i64,
    pub last_read_at: Option<i64>,
    /// When it was taken off Continue reading, if it was.
    pub hidden_at: Option<i64>,
}

/// The reading state of the books `filter` (a condition on comics `c`) picks out.
fn read_states(conn: &Connection, filter: &str, p: impl rusqlite::Params) -> Result<Vec<ReadState>> {
    let mut st = conn.prepare(&format!(
        "SELECT c.id, c.read, c.current_page, c.last_read_at, h.at FROM comics c
         LEFT JOIN reading_hidden h ON h.comic_id=c.id WHERE {filter}"
    ))?;
    let v = st
        .query_map(p, |r| {
            Ok(ReadState { id: r.get(0)?, read: r.get(1)?, current_page: r.get(2)?, last_read_at: r.get(3)?, hidden_at: r.get(4)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

/// Undo: put books back the way `read_states` found them.
pub fn restore_reading(conn: &Connection, states: &[ReadState]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for s in states {
        tx.execute(
            "UPDATE comics SET read=?2, current_page=?3, last_read_at=?4 WHERE id=?1",
            params![s.id, s.read, s.current_page, s.last_read_at],
        )?;
        match s.hidden_at {
            Some(at) => tx.execute(
                "INSERT INTO reading_hidden(comic_id, at) VALUES(?1, ?2) ON CONFLICT(comic_id) DO UPDATE SET at=excluded.at",
                params![s.id, at],
            )?,
            None => tx.execute("DELETE FROM reading_hidden WHERE comic_id=?1", [s.id])?,
        };
    }
    tx.commit()?;
    Ok(())
}

/// The next untouched book after the one most recently read in each series.
pub fn up_next(conn: &Connection, limit: i64) -> Result<Vec<ComicCard>> {
    let sql = format!(
        "WITH latest AS (
           SELECT series_id, sort_key, last_read_at FROM (
             SELECT series_id, sort_key, last_read_at,
                    ROW_NUMBER() OVER (PARTITION BY series_id ORDER BY last_read_at DESC) AS rn
             FROM comics WHERE last_read_at IS NOT NULL)
           WHERE rn=1)
         SELECT {CARD_COLS} FROM latest
         JOIN comics c ON c.id=(SELECT n.id FROM comics n
                                WHERE n.series_id=latest.series_id AND n.sort_key>latest.sort_key
                                  AND n.read=0 AND n.current_page=0
                                ORDER BY n.sort_key LIMIT 1)
         {CARD_JOIN}
         ORDER BY latest.last_read_at DESC LIMIT ?1"
    );
    let mut st = conn.prepare(&sql)?;
    let v = st.query_map([limit], card)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: i64,
    pub name: String,
    pub is_manga: bool,
    pub series_count: i64,
    pub comic_count: i64,
    /// How you've said its series read, if you have.
    pub reading: Option<String>,
    /// First-book covers of the three most recently read series, for the fanned stack on Home.
    pub covers: Vec<i64>,
}

pub fn collections(conn: &Connection) -> Result<Vec<CollectionSummary>> {
    let mut covers = conn.prepare(
        "SELECT (SELECT c2.id FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1)
         FROM series s JOIN comics c ON c.series_id=s.id
         WHERE s.collection_id=?1
         GROUP BY s.id
         ORDER BY MAX(COALESCE(c.last_read_at,0)) DESC, MAX(c.added_at) DESC
         LIMIT 3",
    )?;
    let mut st = conn.prepare(
        "SELECT col.id, col.name, COALESCE(col.reading='manga', col.is_manga),
           (SELECT COUNT(*) FROM series s WHERE s.collection_id=col.id),
           (SELECT COUNT(*) FROM comics c JOIN series s ON s.id=c.series_id WHERE s.collection_id=col.id),
           col.reading
         FROM collections col ORDER BY col.name COLLATE NOCASE",
    )?;
    let mut v = st
        .query_map([], |r| {
            Ok(CollectionSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                is_manga: r.get(2)?,
                series_count: r.get(3)?,
                comic_count: r.get(4)?,
                reading: r.get(5)?,
                covers: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for c in &mut v {
        c.covers = covers.query_map([c.id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    }
    Ok(v)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesSummary {
    pub id: i64,
    pub name: String,
    pub is_manga: bool,
    pub count: i64,
    pub read_count: i64,
    pub in_progress: bool,
    pub cover_id: Option<i64>,
    pub cover_ratio: Option<f64>,
    pub last_read_at: Option<i64>,
    pub last_added_at: Option<i64>,
    /// The first book's cover color, "#rrggbb".
    pub cover_color: Option<String>,
    /// Whether that cover's print is made (see `print` in lib.rs); filled in there.
    pub printed: bool,
    /// How you've said this series reads, if you have.
    pub reading: Option<String>,
    /// On your Want to read list (and not started since).
    pub wanted: bool,
}

/// A series on Want to read stays there until you start it (read a page of any of its books after
/// putting it there) or every book in it is read: then it's gone on to Continue reading. Taken on
/// `w`, a row of want_series.
macro_rules! series_wanted {
    () => {
        "w.at > COALESCE((SELECT MAX(c3.last_read_at) FROM comics c3
                          WHERE c3.series_id=w.series_id AND (c3.read=1 OR c3.current_page>0)), -1)
         AND EXISTS (SELECT 1 FROM comics c3 WHERE c3.series_id=w.series_id AND c3.read=0)"
    };
}
/// The same for one book `c`, put on the list at `w.at`.
const BOOK_WANTED: &str = "c.read=0 AND (c.current_page=0 OR c.last_read_at IS NULL OR c.last_read_at < w.at)";

const SERIES_SELECT: &str = concat!(
    "SELECT s.id, s.name, COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga), COUNT(c.id), COALESCE(SUM(c.read),0),
       COALESCE(MAX(c.read=0 AND c.current_page>0),0),
       (SELECT c2.id FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       (SELECT c2.cover_ratio FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       MAX(c.last_read_at), MAX(c.added_at),
       (SELECT c2.cover_color FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       s.reading,
       EXISTS (SELECT 1 FROM want_series w WHERE w.series_id=s.id AND ",
    series_wanted!(),
    ")
     FROM series s JOIN collections col ON col.id=s.collection_id LEFT JOIN comics c ON c.series_id=s.id"
);

fn series_summary(r: &Row) -> rusqlite::Result<SeriesSummary> {
    Ok(SeriesSummary {
        id: r.get(0)?,
        name: r.get(1)?,
        is_manga: r.get(2)?,
        count: r.get(3)?,
        read_count: r.get(4)?,
        in_progress: r.get(5)?,
        cover_id: r.get(6)?,
        cover_ratio: r.get(7)?,
        last_read_at: r.get(8)?,
        last_added_at: r.get(9)?,
        cover_color: r.get(10)?,
        printed: false,
        reading: r.get(11)?,
        wanted: r.get(12)?,
    })
}

/// The covers pages will ask for a print of, for making those ahead of time. `printable` is the same
/// test as the pages' (Series.svelte, Collection.svelte): some color, and never teal.
pub fn prints_wanted(conn: &Connection) -> Result<Vec<i64>> {
    // What you're partway through shows on Home whatever its color, so it comes first.
    let mut wanted: Vec<i64> = conn
        .prepare("SELECT id FROM comics WHERE read=0 AND current_page>0 ORDER BY last_read_at DESC LIMIT 50")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    // Each series' first cover that prints in a color of its own, as its page and its collection's
    // pick it, or its first of all, to print in gray.
    let mut stmt = conn.prepare("SELECT series_id, id, cover_color FROM comics ORDER BY series_id, section!='', sort_key")?;
    let rows: Vec<(i64, i64, Option<String>)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?;
    for books in rows.chunk_by(|a, b| a.0 == b.0) {
        let pick = books.iter().find(|b| b.2.as_deref().is_some_and(printable)).unwrap_or(&books[0]);
        if !wanted.contains(&pick.1) {
            wanted.push(pick.1);
        }
    }
    Ok(wanted)
}

fn printable(hex: &str) -> bool {
    let Ok(n) = u32::from_str_radix(hex.trim_start_matches('#'), 16) else { return false };
    let [r, g, b] = [n >> 16, (n >> 8) & 255, n & 255].map(|v| v as f64 / 255.0);
    let (hi, d) = (r.max(g).max(b), r.max(g).max(b) - r.min(g).min(b));
    if d <= 0.12 {
        return false;
    }
    let h = 60.0 * if hi == r { ((g - b) / d).rem_euclid(6.0) } else if hi == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    !(150.0..=200.0).contains(&h.round())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionView {
    pub id: i64,
    pub name: String,
    pub is_manga: bool,
    pub series: Vec<SeriesSummary>,
}

pub fn collection_view(conn: &Connection, id: i64) -> Result<CollectionView> {
    let (name, is_manga): (String, bool) = conn.query_row(
        "SELECT name, COALESCE(reading='manga', is_manga) FROM collections WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut st = conn.prepare(&format!(
        "{SERIES_SELECT} WHERE s.collection_id=?1 GROUP BY s.id ORDER BY s.name COLLATE NOCASE"
    ))?;
    let series = st.query_map([id], series_summary)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(CollectionView { id, name, is_manga, series })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub name: String,
    pub comics: Vec<ComicCard>,
}

/// One format of a series ("Volumes", "Issues", "Annuals"…), its books in their subfolders.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edition {
    pub kind: String,
    pub name: String,
    pub sections: Vec<Section>,
    /// Issue numbers missing from the run, when only a few are.
    pub missing: Vec<i64>,
}

/// "You're on Vol. 12 — mark the 11 before it as read?"
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backfill {
    pub before_id: i64,
    pub before_label: String,
    pub count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesView {
    pub id: i64,
    pub name: String,
    pub collection_id: i64,
    pub collection_name: String,
    pub is_manga: bool,
    /// How you've said this series reads ("comic", "manga", "webtoon"), if you have.
    pub reading: Option<String>,
    pub writer: Option<String>,
    /// Penciller (or artist), when known.
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub years: Option<String>,
    pub synopsis: Option<String>,
    /// Every book, format by format (see `editions`), in the order the page lists them.
    pub sections: Vec<Section>,
    pub editions: Vec<Edition>,
    /// The order you've put its formats in, if you have. The first one is what the series opens on.
    pub format_order: Option<Vec<String>>,
    pub backfill: Option<Backfill>,
    /// Its books whose cover is already printed (see `print` in lib.rs), filled in there.
    pub printed: Vec<i64>,
    /// The Comic Vine series this one is matched to, if any.
    pub cv_volume_id: Option<String>,
    /// On your Want to read list, and which of its books are on it by themselves.
    pub wanted: bool,
    pub wanted_books: Vec<i64>,
    /// Issues (or manga volumes) out on Comic Vine past the last one here: "19", "20".
    pub new_issues: Vec<String>,
}

/// A book that may be the same as another, with where its file is.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupBook {
    pub comic: ComicCard,
    pub rel_path: String,
    pub size: Option<i64>,
}

/// Books that look like copies of one another, and why.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    /// Stable for the same set of books, so "not duplicates" can be remembered.
    pub key: String,
    pub reasons: Vec<String>,
    pub books: Vec<DupBook>,
}

/// Books that are probably the same: files of exactly the same size and length, files matched to
/// the same Comic Vine issue, or the same number in the same series with the same page count
/// (a CBR and a CBZ of one issue). Groups you've marked as not duplicates are left out.
pub fn duplicates(conn: &Connection) -> Result<Vec<DupGroup>> {
    let signals: [(&str, &str); 3] = [
        ("Same file size", "SELECT GROUP_CONCAT(id) FROM comics WHERE size > 0 GROUP BY size, num_pages HAVING COUNT(*) > 1"),
        ("Same Comic Vine issue", "SELECT GROUP_CONCAT(id) FROM comics WHERE cv_id IS NOT NULL AND cv_id != '' GROUP BY cv_id HAVING COUNT(*) > 1"),
        (
            "Same number and length",
            "SELECT GROUP_CONCAT(id) FROM comics WHERE num_pages > 0 GROUP BY series_id, section, label, num_pages HAVING COUNT(*) > 1",
        ),
    ];
    // Books joined by any signal end up in one group (union-find over book ids).
    let mut parent: HashMap<i64, i64> = HashMap::new();
    fn root(p: &mut HashMap<i64, i64>, x: i64) -> i64 {
        let up = *p.entry(x).or_insert(x);
        if up == x {
            return x;
        }
        let r = root(p, up);
        p.insert(x, r);
        r
    }
    let mut why: Vec<(Vec<i64>, &str)> = Vec::new();
    for (reason, sql) in signals {
        let mut st = conn.prepare(sql)?;
        let sets: Vec<String> = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        for set in sets {
            let ids: Vec<i64> = set.split(',').filter_map(|x| x.parse().ok()).collect();
            for w in ids.windows(2) {
                let (a, b) = (root(&mut parent, w[0]), root(&mut parent, w[1]));
                if a != b {
                    parent.insert(a, b);
                }
            }
            why.push((ids, reason));
        }
    }
    let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
    let ids: Vec<i64> = parent.keys().copied().collect();
    for id in ids {
        let r = root(&mut parent, id);
        groups.entry(r).or_default().push(id);
    }
    let ignored: std::collections::HashSet<String> = conn
        .prepare("SELECT key FROM duplicate_ignored")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::new();
    for (r, mut members) in groups {
        members.sort_unstable();
        let key = members.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
        if ignored.contains(&key) {
            continue;
        }
        let mut reasons: Vec<String> = why
            .iter()
            .filter(|(ids, _)| ids.first().is_some_and(|i| root(&mut parent, *i) == r))
            .map(|(_, reason)| reason.to_string())
            .collect();
        reasons.sort();
        reasons.dedup();
        let mut books = Vec::new();
        for id in &members {
            let (rel_path, size): (String, Option<i64>) =
                conn.query_row("SELECT rel_path, size FROM comics WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            books.push(DupBook { comic: card_by_id(conn, *id)?, rel_path, size });
        }
        out.push(DupGroup { key, reasons, books });
    }
    out.sort_by(|a, b| {
        let name = |g: &DupGroup| g.books[0].comic.series_name.to_lowercase();
        name(a).cmp(&name(b)).then_with(|| a.key.cmp(&b.key))
    });
    Ok(out)
}

pub fn ignore_duplicate(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO duplicate_ignored(key) VALUES(?1)", [key])?;
    Ok(())
}

/// One series' worth of books that someone made or appears in.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeopleSeries {
    pub id: i64,
    pub name: String,
    pub collection_name: String,
    pub comics: Vec<ComicCard>,
}

/// Everything in the library by one creator, or with one character or team in it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeopleView {
    pub name: String,
    pub kind: String,
    /// What they did and in how many books ("pencils", 30), most first.
    pub roles: Vec<(String, i64)>,
    pub books: i64,
    /// Biggest share of their books first.
    pub series: Vec<PeopleSeries>,
}

/// The credit fields a creator is looked up in, and the role each one means.
const CREATOR_FIELDS: [(&str, &str); 6] = [
    ("writer", "writer"),
    ("penciller", "pencils"),
    ("inker", "inks"),
    ("colorist", "colors"),
    ("letterer", "letters"),
    ("cover_artist", "cover"),
];
const CHARACTER_FIELDS: [(&str, &str); 2] = [("characters", "character"), ("teams", "team")];

/// Books by a creator (`kind` "creator") or with a character or team in them (`kind` "character").
/// Names are whole lines of those fields, so "Stan Lee" doesn't also find "Stan Lees".
pub fn people_view(conn: &Connection, kind: &str, name: &str) -> Result<PeopleView> {
    let fields: &[(&str, &str)] = match kind {
        "character" => &CHARACTER_FIELDS,
        "arc" => &[("story_arc", "arc")],
        _ => &CREATOR_FIELDS,
    };
    let has = |f: &str| format!("instr(char(10) || COALESCE(c.{f}, '') || char(10), char(10) || ?1 || char(10)) > 0");
    let any = fields.iter().map(|(f, _)| has(f)).collect::<Vec<_>>().join(" OR ");
    let flags = fields.iter().map(|(f, _)| format!("({})", has(f))).collect::<Vec<_>>().join(", ");
    let sql = format!(
        "SELECT {CARD_COLS}, col.name, {flags} FROM comics c {CARD_JOIN} WHERE {any}
         ORDER BY s.name COLLATE NOCASE, c.section != '', c.sort_key"
    );
    let mut st = conn.prepare(&sql)?;
    let mut series: Vec<PeopleSeries> = Vec::new();
    let mut roles: Vec<(String, i64)> = fields.iter().map(|(_, r)| (r.to_string(), 0)).collect();
    let mut books = 0;
    let mut rows = st.query([name.trim()])?;
    while let Some(r) = rows.next()? {
        let c = card(r)?;
        let collection: String = r.get(13)?;
        for (i, role) in roles.iter_mut().enumerate() {
            if r.get::<_, bool>(14 + i)? {
                role.1 += 1;
            }
        }
        books += 1;
        match series.last_mut() {
            Some(g) if g.id == c.series_id => g.comics.push(c),
            _ => series.push(PeopleSeries { id: c.series_id, name: c.series_name.clone(), collection_name: collection, comics: vec![c] }),
        }
    }
    series.sort_by(|a, b| b.comics.len().cmp(&a.comics.len()));
    roles.retain(|r| r.1 > 0);
    roles.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(PeopleView { name: name.trim().to_string(), kind: kind.to_string(), roles, books, series })
}

pub fn series_view(conn: &Connection, id: i64) -> Result<SeriesView> {
    let (name, collection_id, collection_name, is_manga, reading, formats): (String, i64, String, bool, Option<String>, Option<String>) = conn.query_row(
        "SELECT s.name, col.id, col.name, COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga), s.reading, s.formats FROM series s JOIN collections col ON col.id=s.collection_id WHERE s.id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    let format_order = formats.map(|f| split_formats(&f));
    let mut editions = editions_of(conn, id, is_manga)?;
    if let Some(order) = &format_order {
        // A format you haven't placed (it may have turned up since) keeps its usual place, after those you have.
        editions.sort_by_key(|e| order.iter().position(|k| *k == e.kind).unwrap_or(usize::MAX));
    }
    let sections = editions
        .iter()
        .filter(|e| e.kind != STORY)
        .flat_map(|e| e.sections.iter())
        .map(|s| Section { name: s.name.clone(), comics: s.comics.clone() })
        .collect();
    let meta = |col: &str| -> Result<Option<String>> {
        Ok(conn
            .query_row(
                &format!("SELECT {col} FROM comics WHERE series_id=?1 AND {col} IS NOT NULL AND {col}!='' ORDER BY sort_key LIMIT 1"),
                [id],
                |r| r.get(0),
            )
            .optional()?)
    };
    let years: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT MIN(year), MAX(year) FROM comics WHERE series_id=?1 AND year IS NOT NULL",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let years = match years {
        Some((Some(a), Some(b))) if a == b => Some(a),
        Some((Some(a), Some(b))) => Some(format!("{a}–{b}")),
        _ => None,
    };
    let cv: Option<(String, serde_json::Value)> = conn
        .query_row("SELECT volume_id, data FROM cv_series WHERE series_id=?1", [id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .optional()?
        .map(|(v, d)| (v, serde_json::from_str(&d).unwrap_or_default()));
    Ok(SeriesView {
        id,
        name,
        collection_id,
        collection_name,
        is_manga,
        reading,
        writer: meta("writer")?,
        artist: meta("penciller")?,
        publisher: meta("publisher")?,
        years,
        // Books' own synopses first; a matched series' description otherwise.
        synopsis: match meta("synopsis")? {
            Some(s) => Some(s),
            None => cv.as_ref().and_then(|(_, d)| d.get("description").and_then(|v| v.as_str()).map(str::to_string)),
        },
        sections,
        editions,
        format_order,
        backfill: backfill(conn, id)?,
        printed: Vec::new(),
        cv_volume_id: cv.map(|(v, _)| v),
        wanted: conn.query_row(
            concat!("SELECT EXISTS (SELECT 1 FROM want_series w WHERE w.series_id=?1 AND ", series_wanted!(), ")"),
            [id],
            |r| r.get(0),
        )?,
        wanted_books: conn
            .prepare(&format!("SELECT c.id FROM want_books w JOIN comics c ON c.id=w.comic_id WHERE c.series_id=?1 AND {BOOK_WANTED}"))?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?,
        new_issues: new_issues(conn, id)?,
    })
}

/// Everything on your Want to read list, the last put on first, with when each went on.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WantList {
    pub series: Vec<SeriesSummary>,
    pub series_at: Vec<i64>,
    pub books: Vec<ComicCard>,
    pub books_at: Vec<i64>,
}

pub fn want_list(conn: &Connection) -> Result<WantList> {
    let series = conn
        .prepare(&format!(
            "{SERIES_SELECT} JOIN want_series w ON w.series_id=s.id WHERE {} GROUP BY s.id ORDER BY w.at DESC",
            series_wanted!()
        ))?
        .query_map([], series_summary)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let books = cards(conn, &format!("JOIN want_books w ON w.comic_id=c.id WHERE {BOOK_WANTED} ORDER BY w.at DESC"), [])?;
    let at = |table: &str, key: &str| -> Result<HashMap<i64, i64>> {
        Ok(conn
            .prepare(&format!("SELECT {key}, at FROM {table}"))?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?)
    };
    let (sa, ba) = (at("want_series", "series_id")?, at("want_books", "comic_id")?);
    Ok(WantList {
        series_at: series.iter().map(|s| sa[&s.id]).collect(),
        series,
        books_at: books.iter().map(|b| ba[&b.id]).collect(),
        books,
    })
}

/// How many things are on the list, for the side panel.
pub fn want_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        &format!(
            "SELECT (SELECT COUNT(*) FROM want_series w WHERE {})
                  + (SELECT COUNT(*) FROM want_books w JOIN comics c ON c.id=w.comic_id WHERE {BOOK_WANTED})",
            series_wanted!()
        ),
        [],
        |r| r.get(0),
    )?)
}

/// Put a series or a book on Want to read (again: it counts from now), or take it off.
pub fn set_wanted(conn: &Connection, series: Option<i64>, book: Option<i64>, on: bool, now: i64) -> Result<()> {
    for (table, key, id) in [("want_series", "series_id", series), ("want_books", "comic_id", book)] {
        let Some(id) = id else { continue };
        if on {
            conn.execute(&format!("INSERT INTO {table}({key}, at) VALUES(?1, ?2) ON CONFLICT({key}) DO UPDATE SET at=excluded.at"), params![id, now])?;
        } else {
            conn.execute(&format!("DELETE FROM {table} WHERE {key}=?1"), [id])?;
        }
    }
    Ok(())
}

/// The tab on a series page with every format's books in the order the story runs.
const STORY: &str = "order";

fn split_formats(stored: &str) -> Vec<String> {
    stored.split(',').map(str::to_string).collect()
}

/// Put a series' formats in this order, or None to go back to the usual one. Returns how they were, for Undo.
pub fn set_format_order(conn: &Connection, id: i64, order: Option<&[String]>) -> Result<Option<Vec<String>>> {
    let was: Option<String> = conn.query_row("SELECT formats FROM series WHERE id=?1", [id], |r| r.get(0))?;
    let known = |k: &&String| k.as_str() == STORY || editions::KINDS.contains(&k.as_str());
    let now = order.map(|o| o.iter().filter(known).cloned().collect::<Vec<_>>().join(",")).filter(|s| !s.is_empty());
    conn.execute("UPDATE series SET formats=?2 WHERE id=?1", params![id, now])?;
    Ok(was.map(|f| split_formats(&f)))
}

/// One book of a series, as its page lists it.
struct SeriesBook {
    kind: &'static str,
    section: String,
    number: Option<f64>,
    /// What part of the story it holds, when that's known for certain.
    span: Option<editions::Span>,
    /// When it came out: its date, or just its year.
    when: Option<String>,
    /// Its file name starts with a place in a reading order.
    in_reading_order: bool,
    card: ComicCard,
}

/// A series' books, format by format, labeled and in order within each.
fn series_books(conn: &Connection, id: i64, manga: bool) -> Result<Vec<SeriesBook>> {
    let mut st = conn.prepare(
        "SELECT c.section, c.file_name, COALESCE(c.num_pages, 0), COALESCE(c.synopsis, ''), o.kind, c.number,
           (SELECT json_extract(m.data, '$.number') FROM meta_layer m WHERE m.comic_id=c.id AND m.source='user'),
           (SELECT json_extract(m.data, '$.number') FROM meta_layer m WHERE m.comic_id=c.id AND m.source='cv'),
           COALESCE(NULLIF(c.date, ''), NULLIF(c.year, ''))
         FROM comics c LEFT JOIN edition_override o ON o.comic_id=c.id WHERE c.series_id=?1 ORDER BY c.sort_key",
    )?;
    type Extra = (String, String, i64, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>);
    let extra: Vec<Extra> = st
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let series: String = conn.query_row("SELECT name FROM series WHERE id=?1", [id], |r| r.get(0))?;
    let rows = cards(conn, "WHERE c.series_id=?1 ORDER BY c.sort_key", [id])?;
    let mut books: Vec<SeriesBook> = Vec::new();
    for (mut c, (sec, file, pages, synopsis, own, stored, typed, cv, when)) in rows.into_iter().zip(extra) {
        let kind = own
            .as_deref()
            .and_then(|k| editions::KINDS.iter().copied().find(|x| *x == k))
            .unwrap_or_else(|| editions::kind(manga, &sec, &file, pages));
        // A number you typed in wins; then a reading order's place at the front of the file name;
        // then Comic Vine's; then the file name's; then whatever the book's own details say.
        let parse = |v: Option<String>| v.and_then(|s| s.trim().trim_start_matches('#').parse::<f64>().ok());
        let place = editions::order_number(&file, &series);
        let n = parse(typed).or(place).or_else(|| parse(cv)).or_else(|| editions::number(kind, &file, &series)).or_else(|| parse(stored));
        c.label = editions::label(kind, n, &c.label);
        if matches!(kind, "volume" | "collection" | "deluxe" | "omnibus") {
            c.collects = editions::collects(&synopsis).or_else(|| editions::chapters(&synopsis));
        }
        c.edition = Some(kind.to_string());
        // A place in a reading order isn't an issue number.
        let span = if place.is_some() { None } else { editions::span(kind, n, &synopsis, &series) };
        books.push(SeriesBook { kind, section: sec, number: n, span, when, in_reading_order: place.is_some(), card: c });
    }
    // Format by format; subfolders in the order they first come; numbered books in number order.
    let format_place = |k: &str| editions::KINDS.iter().position(|x| *x == k);
    let mut folders: Vec<(&str, String)> = Vec::new();
    for b in &books {
        if !folders.iter().any(|f| f.0 == b.kind && f.1 == b.section) {
            folders.push((b.kind, b.section.clone()));
        }
    }
    let folder_place = |b: &SeriesBook| folders.iter().position(|f| f.0 == b.kind && f.1 == b.section);
    let mut placed: Vec<(usize, SeriesBook)> = books.into_iter().enumerate().collect();
    placed.sort_by(|(ia, a), (ib, b)| {
        format_place(a.kind).cmp(&format_place(b.kind)).then(folder_place(a).cmp(&folder_place(b))).then_with(|| match (a.number, b.number) {
            (Some(x), Some(y)) => x.total_cmp(&y).then(ia.cmp(ib)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => ia.cmp(ib),
        })
    });
    let mut books: Vec<SeriesBook> = placed.into_iter().map(|(_, b)| b).collect();
    // A collection that says it holds earlier issues than the one before it is a mistake in its
    // description, or numbered some other way ("Season 2, chapters 1-3"): don't go by it.
    let mut reached: Vec<(&str, f64)> = Vec::new();
    for b in &mut books {
        if !matches!(b.kind, "volume" | "collection" | "deluxe" | "omnibus") {
            continue;
        }
        let Some(s) = b.span else { continue };
        match reached.iter_mut().find(|r| r.0 == b.kind) {
            Some(r) if s.from < r.1 => b.span = None,
            Some(r) => r.1 = s.from,
            None => reached.push((b.kind, s.from)),
        }
    }
    Ok(books)
}

/// The series' story order, as places in `books`, when there is one to speak of. A series named as a
/// reading order ("019 Siege - Storming Asgard") is already in the order it should be read.
fn story_of(books: &[SeriesBook]) -> Option<Vec<editions::Stop>> {
    if books.iter().any(|b| b.in_reading_order) {
        return None;
    }
    let list: Vec<editions::StoryBook> =
        books.iter().map(|b| editions::StoryBook { kind: b.kind, span: b.span, when: b.when.clone() }).collect();
    editions::story_order(&list)
}

/// A series' books sorted into their formats, then, when the story runs across them, all together
/// in the order to read them.
fn editions_of(conn: &Connection, id: i64, manga: bool) -> Result<Vec<Edition>> {
    let mut books = series_books(conn, id, manga)?;
    let story = story_of(&books);
    if let Some(story) = &story {
        for stop in story {
            if let Some(a) = stop.after {
                books[stop.book].card.after = Some(books[a].card.label.clone());
            }
        }
    }
    let mut out = Vec::new();
    for kind in editions::KINDS {
        let mine: Vec<&SeriesBook> = books.iter().filter(|b| b.kind == kind).collect();
        if mine.is_empty() {
            continue;
        }
        let missing = if kind == "issue" { editions::gaps(&mine.iter().filter_map(|b| b.number).collect::<Vec<_>>()) } else { vec![] };
        // A subfolder named for the format itself ("Issues", "TPB") goes without saying.
        let shown = |f: &str| {
            let last = f.rsplit('/').next().unwrap_or("").trim().to_lowercase();
            let plain = ["", "issues", "chapters", "volumes", "tpb", "tpbs", "trades", "omnibus", "annuals", "specials"];
            if plain.contains(&last.as_str()) { String::new() } else { f.to_string() }
        };
        let mut sections: Vec<Section> = Vec::new();
        for b in mine {
            let name = shown(&b.section);
            match sections.last_mut() {
                Some(s) if s.name == name => s.comics.push(b.card.clone()),
                _ => sections.push(Section { name, comics: vec![b.card.clone()] }),
            }
        }
        out.push(Edition { kind: kind.to_string(), name: editions::name(kind).to_string(), sections, missing });
    }
    if let Some(story) = story {
        let comics = story.iter().map(|s| books[s.book].card.clone()).collect();
        out.push(Edition { kind: STORY.to_string(), name: "In order".to_string(), sections: vec![Section { name: String::new(), comics }], missing: vec![] });
    }
    Ok(out)
}

/// Whether a series reads as manga.
fn series_is_manga(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga) FROM series s JOIN collections col ON col.id=s.collection_id WHERE s.id=?1",
        [id],
        |r| r.get(0),
    )?)
}

/// These books have just been finished. So has anything they hold (the issues in a collection, the
/// chapters in a volume), and any collection you've now read all of. Only ever from unread to read,
/// and only where the book says for certain what it holds, in this series. Returns how the books it
/// changed were, for Undo.
fn finish_what_they_hold(conn: &Connection, series: i64, finished: &[i64], now: i64) -> Result<Vec<ReadState>> {
    let books = series_books(conn, series, series_is_manga(conn, series)?)?;
    let spans: Vec<editions::Span> = books.iter().filter(|b| finished.contains(&b.card.id)).filter_map(|b| b.span).collect();
    if spans.is_empty() {
        return Ok(vec![]);
    }
    let read_now = |b: &SeriesBook| b.card.read || finished.contains(&b.card.id);
    let mut more: Vec<i64> = books
        .iter()
        .filter(|b| !read_now(b) && b.span.is_some_and(|s| spans.iter().any(|f| f.holds(&s))))
        .map(|b| b.card.id)
        .collect();
    // What's been read of the story, newly marked books included: does it now fill any collection
    // these books are part of?
    let covered: Vec<editions::Span> =
        books.iter().filter(|b| read_now(b) || more.contains(&b.card.id)).filter_map(|b| b.span).collect();
    for b in &books {
        let Some(s) = b.span.filter(|s| s.to > s.from) else { continue };
        if read_now(b) || more.contains(&b.card.id) || !spans.iter().any(|f| f.meets(&s)) {
            continue;
        }
        let whole = |n: f64| editions::Span { unit: s.unit, from: n, to: n };
        // Far too long to be a real run, and the description's numbers mean something else.
        let filled = s.to - s.from <= 5000.0
            && (s.from as i64..=s.to as i64).all(|n| covered.iter().any(|c| c.holds(&whole(n as f64))));
        if filled {
            more.push(b.card.id);
        }
    }
    let mut before = Vec::with_capacity(more.len());
    for id in more {
        before.extend(read_states(conn, "c.id=?1", [id])?);
        conn.execute("UPDATE comics SET read=1, current_page=0, last_read_at=COALESCE(last_read_at, ?2) WHERE id=?1", params![id, now])?;
    }
    Ok(before)
}

/// What a search finds that isn't a series or a book: a name, and how many books it's on.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NameHit {
    pub name: String,
    pub books: i64,
    /// A book of theirs, for its cover.
    pub cover_id: i64,
    /// How closely it matches (see `closeness`), to sort different kinds of name together.
    pub close: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHits {
    pub series: Vec<SeriesSummary>,
    pub books: Vec<ComicCard>,
    pub creators: Vec<NameHit>,
    pub characters: Vec<NameHit>,
    pub arcs: Vec<NameHit>,
    /// The book you're partway through in the first series, if you are.
    pub top_reading: Option<ComicCard>,
}

/// How closely a name matches `q` (in lower case), the closest lowest: all of it, its start, the
/// start of a word in it ("Tom King"), or only inside a word ("Walking").
fn closeness(name: &str, q: &str) -> u8 {
    let name = name.trim().to_lowercase();
    if name == q {
        return 0;
    }
    if name.starts_with(q) {
        return 1;
    }
    let at_word = name.match_indices(q).any(|(i, _)| !name[..i].chars().next_back().is_some_and(char::is_alphanumeric));
    if at_word {
        2
    } else {
        3
    }
}

/// The names on one-per-line fields that have `q` in them, the closest matches first, then those on
/// the most books.
fn names_matching(conn: &Connection, fields: &[&str], like: &str, q: &str) -> Result<Vec<NameHit>> {
    let any = fields.iter().map(|f| format!("{f} LIKE ?1 ESCAPE '\\'")).collect::<Vec<_>>().join(" OR ");
    let mut st = conn.prepare(&format!("SELECT id, {} FROM comics WHERE {any}", fields.join(", ")))?;
    let mut rows = st.query([like])?;
    let q = q.trim().to_lowercase();
    let mut found: HashMap<String, (i64, i64)> = HashMap::new();
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let mut names: Vec<String> = Vec::new();
        for i in 0..fields.len() {
            let v: Option<String> = r.get(i + 1)?;
            names.extend(v.unwrap_or_default().lines().map(|n| n.trim().to_string()).filter(|n| n.to_lowercase().contains(&q)));
        }
        names.sort();
        names.dedup();
        for n in names {
            found.entry(n).or_insert((0, id)).0 += 1;
        }
    }
    let mut hits: Vec<NameHit> = found
        .into_iter()
        .map(|(name, (books, cover_id))| NameHit { close: closeness(&name, &q), name, books, cover_id })
        .collect();
    hits.sort_by(|a, b| a.close.cmp(&b.close).then(b.books.cmp(&a.books)).then_with(|| a.name.cmp(&b.name)));
    Ok(hits)
}

/// Everything in the library that matches: series, books by their title or their file's name (where
/// the series itself didn't match, or it'd be every book in it), creators, characters and teams, and
/// story arcs. The closest matches come first, and among those whatever you read last.
pub fn search_all(conn: &Connection, q: &str) -> Result<SearchHits> {
    let like = format!("%{}%", q.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let lower = q.trim().to_lowercase();
    let mut series = {
        let mut st = conn.prepare(&format!(
            "{SERIES_SELECT} WHERE s.name LIKE ?1 ESCAPE '\\'
             GROUP BY s.id ORDER BY MAX(c.last_read_at) DESC NULLS LAST, s.name COLLATE NOCASE"
        ))?;
        let v = st.query_map([&like], series_summary)?.collect::<rusqlite::Result<Vec<_>>>()?;
        v
    };
    series.sort_by_key(|s| closeness(&s.name, &lower));
    let mut books = cards(
        conn,
        "WHERE (c.title LIKE ?1 ESCAPE '\\' OR c.file_name LIKE ?1 ESCAPE '\\') AND s.name NOT LIKE ?1 ESCAPE '\\'
         ORDER BY c.title LIKE ?1 ESCAPE '\\' DESC, c.last_read_at DESC NULLS LAST, s.name COLLATE NOCASE, c.sort_key",
        [&like],
    )?;
    // A match only in the file's name comes after any in a title.
    books.sort_by_key(|b| match b.title.as_deref() {
        Some(t) if t.to_lowercase().contains(&lower) => closeness(t, &lower),
        _ => 4,
    });
    let creators = names_matching(conn, &CREATOR_FIELDS.map(|f| f.0), &like, q)?;
    let characters = names_matching(conn, &CHARACTER_FIELDS.map(|f| f.0), &like, q)?;
    let arcs = names_matching(conn, &["story_arc"], &like, q)?;
    let top_reading = match series.first() {
        Some(s) => cards(
            conn,
            "WHERE c.series_id=?1 AND c.read=0 AND c.current_page>0 ORDER BY c.last_read_at DESC LIMIT 1",
            [s.id],
        )?
        .pop(),
        None => None,
    };
    Ok(SearchHits { series, books, creators, characters, arcs, top_reading })
}

/// Give a book a format by hand (None: back to the one worked out from its name).
pub fn set_edition(conn: &Connection, comic: i64, kind: Option<&str>) -> Result<()> {
    match kind {
        Some(k) => conn.execute("INSERT OR REPLACE INTO edition_override(comic_id, kind) VALUES(?1, ?2)", params![comic, k])?,
        None => conn.execute("DELETE FROM edition_override WHERE comic_id=?1", [comic])?,
    };
    Ok(())
}

/// A book's format as it was set by hand (None: sorted automatically), for Undo.
#[derive(Serialize, serde::Deserialize, Debug, PartialEq)]
pub struct EditionState {
    pub id: i64,
    pub kind: Option<String>,
}

/// Set the format of several books at once. Returns how they were, for Undo.
pub fn set_editions(conn: &Connection, ids: &[i64], kind: Option<&str>) -> Result<Vec<EditionState>> {
    let tx = conn.unchecked_transaction()?;
    let mut before = Vec::with_capacity(ids.len());
    for &id in ids {
        let was = tx
            .query_row("SELECT kind FROM edition_override WHERE comic_id=?1", [id], |r| r.get(0))
            .optional()?;
        before.push(EditionState { id, kind: was });
        set_edition(&tx, id, kind)?;
    }
    tx.commit()?;
    Ok(before)
}

/// Undo: put formats back the way `set_editions` found them.
pub fn restore_editions(conn: &Connection, states: &[EditionState]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for s in states {
        set_edition(&tx, s.id, s.kind.as_deref())?;
    }
    tx.commit()?;
    Ok(())
}

fn backfill(conn: &Connection, series: i64) -> Result<Option<Backfill>> {
    if get_setting(conn, &format!("backfill_dismissed:{series}"))?.is_some() {
        return Ok(None);
    }
    // The furthest book you've started or finished in this series.
    let furthest: Option<(i64, i64, String)> = conn
        .query_row(
            "SELECT id, sort_key, label FROM comics WHERE series_id=?1 AND (read=1 OR current_page>0)
             ORDER BY sort_key DESC LIMIT 1",
            [series],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((before_id, key, before_label)) = furthest else { return Ok(None) };
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM comics WHERE series_id=?1 AND sort_key<?2 AND read=0",
        params![series, key],
        |r| r.get(0),
    )?;
    Ok((count > 0).then_some(Backfill { before_id, before_label, count }))
}

pub fn mark_read_before(conn: &Connection, id: i64, now: i64) -> Result<Vec<ReadState>> {
    let before = read_states(
        conn,
        "c.series_id=(SELECT series_id FROM comics WHERE id=?1)
         AND c.sort_key<(SELECT sort_key FROM comics WHERE id=?1) AND c.read=0",
        [id],
    )?;
    conn.execute(
        "UPDATE comics SET read=1, current_page=0, last_read_at=COALESCE(last_read_at, ?2)
         WHERE series_id=(SELECT series_id FROM comics WHERE id=?1)
           AND sort_key<(SELECT sort_key FROM comics WHERE id=?1) AND read=0",
        params![id, now],
    )?;
    let series: i64 = conn.query_row("SELECT series_id FROM comics WHERE id=?1", [id], |r| r.get(0))?;
    let marked: Vec<i64> = before.iter().map(|s| s.id).collect();
    let mut before = before;
    before.extend(finish_what_they_hold(conn, series, &marked, now)?);
    Ok(before)
}

pub fn set_series_read(conn: &Connection, series: i64, read: bool, now: i64) -> Result<Vec<ReadState>> {
    let before = read_states(conn, "c.series_id=?1", [series])?;
    if read {
        conn.execute(
            "UPDATE comics SET read=1, current_page=0, last_read_at=COALESCE(last_read_at, ?2) WHERE series_id=?1",
            params![series, now],
        )?;
    } else {
        conn.execute("UPDATE comics SET read=0, current_page=0, last_read_at=NULL WHERE series_id=?1", [series])?;
    }
    Ok(before)
}

pub struct ComicFile {
    pub rel_path: String,
    pub yac_hash: Option<String>,
}

pub fn comic_file(conn: &Connection, id: i64) -> Result<ComicFile> {
    Ok(conn.query_row(
        "SELECT rel_path, yac_hash FROM comics WHERE id=?1",
        [id],
        |r| Ok(ComicFile { rel_path: r.get(0)?, yac_hash: r.get(1)? }),
    )?)
}

pub fn comic_id_by_path(conn: &Connection, rel_path: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT id FROM comics WHERE rel_path=?1 COLLATE NOCASE", [rel_path], |r| r.get(0))
        .optional()?)
}

/// The books before and after this one, for "next issue" in the reader: in the story's order, where
/// the story runs across formats and this book is part of it; otherwise the next in the series.
pub fn neighbors(conn: &Connection, id: i64) -> Result<(Option<i64>, Option<i64>)> {
    let (series, key): (i64, i64) =
        conn.query_row("SELECT series_id, sort_key FROM comics WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
    let books = series_books(conn, series, series_is_manga(conn, series)?)?;
    if let Some(story) = story_of(&books) {
        if let Some(at) = story.iter().position(|s| books[s.book].card.id == id) {
            let near = |k: Option<usize>| k.and_then(|k| story.get(k)).map(|s| books[s.book].card.id);
            return Ok((near(at.checked_sub(1)), near(Some(at + 1))));
        }
    }
    let prev = conn
        .query_row(
            "SELECT id FROM comics WHERE series_id=?1 AND sort_key<?2 ORDER BY sort_key DESC LIMIT 1",
            params![series, key],
            |r| r.get(0),
        )
        .optional()?;
    let next = conn
        .query_row(
            "SELECT id FROM comics WHERE series_id=?1 AND sort_key>?2 ORDER BY sort_key LIMIT 1",
            params![series, key],
            |r| r.get(0),
        )
        .optional()?;
    Ok((prev, next))
}

/// The issues Comic Vine had past the last one here when it was last looked at, less any that
/// have come into the library since.
fn new_issues(conn: &Connection, series_id: i64) -> Result<Vec<String>> {
    let Some(json) = conn.query_row("SELECT numbers FROM cv_new WHERE series_id=?1", [series_id], |r| r.get::<_, String>(0)).optional()? else {
        return Ok(Vec::new());
    };
    let numbers: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
    let have: Vec<f64> = conn
        .prepare("SELECT number FROM comics WHERE series_id=?1 AND section='' AND number IS NOT NULL")?
        .query_map([series_id], |r| r.get::<_, String>(0))?
        .filter_map(|n| n.ok()?.parse::<f64>().ok())
        .collect();
    Ok(numbers.into_iter().filter(|n| n.parse::<f64>().map_or(true, |v| !have.contains(&v))).collect())
}

/// Every book in the library and its file, in folder order.
pub fn all_book_paths(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let mut st = conn.prepare("SELECT id, rel_path FROM comics ORDER BY rel_path")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn card_by_id(conn: &Connection, id: i64) -> Result<ComicCard> {
    Ok(conn.query_row(&format!("SELECT {CARD_COLS} FROM comics c {CARD_JOIN} WHERE c.id=?1"), [id], card)?)
}

/// Reaching the last page finishes a book. Turning past the first page of a finished one starts a
/// re-read, so it shows as in progress again; only opening it to look leaves it finished.
pub fn set_progress(conn: &Connection, id: i64, page: i64, pages: i64, now: i64) -> Result<()> {
    let read = pages > 0 && page >= pages - 1;
    let (series, was_read): (i64, bool) = conn.query_row("SELECT series_id, read FROM comics WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    conn.execute(
        "UPDATE comics SET current_page=?2, num_pages=?3, last_read_at=?4,
           read=CASE WHEN ?5 THEN 1 WHEN ?2>0 THEN 0 ELSE read END WHERE id=?1",
        params![id, page, pages, now, read],
    )?;
    if read && !was_read {
        finish_what_they_hold(conn, series, &[id], now)?;
    }
    Ok(())
}

/// Marking a book read also marks what it holds (see `finish_what_they_hold`). Marking one unread
/// leaves the others as they are: you may well have read those on their own.
pub fn set_read(conn: &Connection, id: i64, read: bool, now: i64) -> Result<Vec<ReadState>> {
    let mut before = read_states(conn, "c.id=?1", [id])?;
    if read {
        let (series, was_read): (i64, bool) = conn.query_row("SELECT series_id, read FROM comics WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        conn.execute("UPDATE comics SET read=1, last_read_at=?2 WHERE id=?1", params![id, now])?;
        if !was_read {
            before.extend(finish_what_they_hold(conn, series, &[id], now)?);
        }
    } else {
        conn.execute("UPDATE comics SET read=0, current_page=0, last_read_at=NULL WHERE id=?1", params![id])?;
    }
    Ok(before)
}

/// How YACReader says a book reads, if it says.
/// What's known about how a book reads: your say for its series, YACReader's mark, whether its pages
/// are long (when looked at), and its cover's shape.
pub struct ReadingFacts {
    pub said: Option<String>,
    pub yac_type: Option<i64>,
    pub tall: Option<bool>,
    pub cover_ratio: Option<f64>,
}

pub fn reading_facts(conn: &Connection, id: i64) -> Result<ReadingFacts> {
    Ok(conn.query_row(
        "SELECT COALESCE(s.reading, col.reading), c.yac_type, c.tall, c.cover_ratio FROM comics c
         JOIN series s ON s.id=c.series_id JOIN collections col ON col.id=s.collection_id WHERE c.id=?1",
        [id],
        |r| Ok(ReadingFacts { said: r.get(0)?, yac_type: r.get(1)?, tall: r.get(2)?, cover_ratio: r.get(3)? }),
    )?)
}

pub fn set_tall(conn: &Connection, id: i64, tall: bool) -> Result<()> {
    conn.execute("UPDATE comics SET tall=?2 WHERE id=?1", params![id, tall])?;
    Ok(())
}

/// How a book reads: (right to left, a webtoon). Your say comes first, for its series or else its
/// library, then YACReader's mark, then long pages, then the library's name.
pub fn reads_as(said: Option<&str>, yac_type: Option<i64>, tall: bool, in_manga: bool) -> (bool, bool) {
    match said {
        Some("comic") => (false, false),
        Some("manga") => (true, false),
        Some("webtoon") => (false, true),
        _ => match yac_type {
            Some(1) => (true, false),
            Some(2) => (false, false),
            Some(3) => (false, true),
            _ if tall => (false, true),
            _ => (in_manga, false),
        },
    }
}

/// The comics folder has just been changed, on purpose: the next scan takes what's there as the
/// library, with no holding on to books as if a drive had dropped out (see `scan`).
pub fn note_new_root(conn: &Connection) -> Result<()> {
    set_setting(conn, crate::scan::NEW_ROOT, "1")
}

/// Say how a series reads, or None to have it worked out.
pub fn set_series_reading(conn: &Connection, id: i64, reading: Option<&str>) -> Result<()> {
    conn.execute("UPDATE series SET reading=?2 WHERE id=?1", params![id, reading])?;
    Ok(())
}

/// Say how a library's series read, or None to have them worked out: every one it has, whatever was
/// said for them before, and any added later. A series can still be told otherwise afterward.
pub fn set_library_reading(conn: &Connection, id: i64, reading: Option<&str>) -> Result<()> {
    conn.execute("UPDATE collections SET reading=?2 WHERE id=?1", params![id, reading])?;
    conn.execute("UPDATE series SET reading=NULL WHERE collection_id=?1", [id])?;
    Ok(())
}

pub fn set_cover_color(conn: &Connection, id: i64, color: &str) -> Result<()> {
    conn.execute("UPDATE comics SET cover_color=?2 WHERE id=?1", params![id, color])?;
    Ok(())
}

pub fn set_cover_ratio(conn: &Connection, id: i64, ratio: f64) -> Result<()> {
    conn.execute("UPDATE comics SET cover_ratio=?2 WHERE id=?1", params![id, ratio])?;
    Ok(())
}

pub fn cached_panels(conn: &Connection, id: i64, page: i64, rtl: bool, version: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT data FROM panels WHERE comic_id=?1 AND page=?2 AND rtl=?3 AND version=?4",
            params![id, page, rtl, version],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn store_panels(conn: &Connection, id: i64, page: i64, rtl: bool, version: i64, data: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO panels(comic_id, page, rtl, version, data) VALUES(?1, ?2, ?3, ?4, ?5)",
        params![id, page, rtl, version, data],
    )?;
    Ok(())
}

// ---------- bookmarks ----------

pub fn bookmarks(conn: &Connection, id: i64) -> Result<Vec<i64>> {
    let mut st = conn.prepare("SELECT page FROM bookmarks WHERE comic_id=?1 ORDER BY page")?;
    let v = st.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

/// Add the bookmark, or remove it if it's already there. Returns whether the page is now bookmarked.
pub fn toggle_bookmark(conn: &Connection, id: i64, page: i64, now: i64) -> Result<bool> {
    let removed = conn.execute("DELETE FROM bookmarks WHERE comic_id=?1 AND page=?2", params![id, page])?;
    if removed > 0 {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO bookmarks(comic_id, page, created_at) VALUES(?1, ?2, ?3)",
        params![id, page, now],
    )?;
    Ok(true)
}

// ---------- the panel a book was left on in Guided View ----------

/// The `panel`th stop on `page`, and how far down it for one read in parts.
#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GuidedSpot {
    pub page: i64,
    pub panel: i64,
    pub part: i64,
}

pub fn guided_spot(conn: &Connection, id: i64) -> Result<Option<GuidedSpot>> {
    Ok(conn
        .query_row("SELECT page, panel, part FROM guided_spots WHERE comic_id=?1", [id], |r| {
            Ok(GuidedSpot { page: r.get(0)?, panel: r.get(1)?, part: r.get(2)? })
        })
        .optional()?)
}

/// Remember the spot, or with none (a whole page, or out of Guided View) forget the one there was.
pub fn set_guided_spot(conn: &Connection, id: i64, spot: Option<&GuidedSpot>) -> Result<()> {
    match spot {
        Some(s) => conn.execute(
            "INSERT OR REPLACE INTO guided_spots(comic_id, page, panel, part) VALUES(?1, ?2, ?3, ?4)",
            params![id, s.page, s.panel, s.part],
        )?,
        None => conn.execute("DELETE FROM guided_spots WHERE comic_id=?1", [id])?,
    };
    Ok(())
}

// ---------- hand-edited panels (always win over detection) ----------

pub fn user_panels(conn: &Connection, id: i64, page: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT data FROM user_panels WHERE comic_id=?1 AND page=?2", params![id, page], |r| r.get(0))
        .optional()?)
}

/// `None` forgets the edit, so detection takes over again.
pub fn set_user_panels(conn: &Connection, id: i64, page: i64, data: Option<&str>) -> Result<()> {
    match data {
        Some(d) => conn.execute(
            "INSERT OR REPLACE INTO user_panels(comic_id, page, data) VALUES(?1, ?2, ?3)",
            params![id, page, d],
        )?,
        None => conn.execute("DELETE FROM user_panels WHERE comic_id=?1 AND page=?2", params![id, page])?,
    };
    Ok(())
}

// ---------- metadata layers ----------

/// Metadata fields a book can carry. Each is a `comics` column holding the value in effect.
pub const META_FIELDS: [&str; 22] = [
    "title", "number", "volume", "year", "date", "writer", "penciller", "inker", "colorist", "letterer",
    "cover_artist", "publisher", "synopsis", "characters", "teams", "locations", "story_arc", "arc_number",
    "arc_count", "genre", "cv_id", "cv_volume_id",
];

pub type Meta = serde_json::Map<String, serde_json::Value>;

pub fn read_layer(conn: &Connection, id: i64, source: &str) -> Result<Meta> {
    let data: Option<String> = conn
        .query_row("SELECT data FROM meta_layer WHERE comic_id=?1 AND source=?2", params![id, source], |r| r.get(0))
        .optional()?;
    Ok(data.and_then(|d| serde_json::from_str(&d).ok()).unwrap_or_default())
}

pub fn write_layer(conn: &Connection, id: i64, source: &str, data: &Meta, now: i64) -> Result<()> {
    if data.is_empty() {
        conn.execute("DELETE FROM meta_layer WHERE comic_id=?1 AND source=?2", params![id, source])?;
    } else {
        conn.execute(
            "INSERT OR REPLACE INTO meta_layer(comic_id, source, data, updated_at) VALUES(?1, ?2, ?3, ?4)",
            params![id, source, serde_json::to_string(data)?, now],
        )?;
    }
    Ok(())
}

/// Recompute the metadata in effect for some books (or every book that has a layer):
/// the file's own ComicInfo.xml, what the scan imported over it, then Comic Vine, then hand edits.
pub fn apply_meta(conn: &Connection, only: Option<&[i64]>) -> Result<usize> {
    let ids: Vec<i64> = match only {
        Some(ids) => ids.to_vec(),
        None => {
            let mut st = conn.prepare("SELECT DISTINCT comic_id FROM meta_layer")?;
            let v = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            v
        }
    };
    let sql = format!(
        "UPDATE comics SET label=?2, {} WHERE id=?1",
        META_FIELDS.iter().enumerate().map(|(i, f)| format!("{f}=?{}", i + 3)).collect::<Vec<_>>().join(", ")
    );
    let mut up = conn.prepare(&sql)?;
    let mut n = 0;
    for id in ids {
        type Found = (Option<String>, String, bool, String, String, i64, Option<String>);
        let Some((base, label, manga, section, file, pages, own)): Option<Found> = conn
            .query_row(
                "SELECT c.meta_base, c.label, COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga), c.section, c.file_name, COALESCE(c.num_pages, 0), o.kind
                 FROM comics c JOIN series s ON s.id=c.series_id JOIN collections col ON col.id=s.collection_id
                 LEFT JOIN edition_override o ON o.comic_id=c.id WHERE c.id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .optional()?
        else {
            continue;
        };
        let kind = own
            .as_deref()
            .and_then(|k| editions::KINDS.iter().copied().find(|x| *x == k))
            .unwrap_or_else(|| editions::kind(manga, &section, &file, pages));
        // No scan has recorded this book's imported values yet (e.g. the drive is offline):
        // take today's values as that base, so applying a layer never blanks the rest.
        let base = match base {
            Some(b) => b,
            None => {
                let snap = snapshot_base(conn, id, &label)?;
                conn.execute("UPDATE comics SET meta_base=?2 WHERE id=?1", params![id, snap])?;
                snap
            }
        };
        // The details inside the file fill whatever the import left empty.
        let mut m = read_layer(conn, id, "file")?;
        let imported: Meta = serde_json::from_str(&base).unwrap_or_default();
        for (k, v) in imported {
            if v.as_str().is_none_or(|s| !s.trim().is_empty()) {
                m.insert(k, v);
            }
        }
        let base_label = m.get("label").and_then(|v| v.as_str()).map(str::to_string).unwrap_or(label);
        let cv = read_layer(conn, id, "cv")?;
        let user = read_layer(conn, id, "user")?;
        let numbered = cv.contains_key("number") || user.contains_key("number");
        for layer in [cv, user] {
            for (k, v) in layer {
                m.insert(k, v);
            }
        }
        let text = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        // A number from Comic Vine or a hand edit relabels the book in the style of its format
        // ("#5", "Ch. 5", "Vol. 5"). A book already called a volume stays one.
        let collected = matches!(kind, "volume" | "collection" | "deluxe" | "omnibus");
        let label = match (numbered, text("number")) {
            (true, Some(n)) if kind == "chapter" => format!("Ch. {n}"),
            (true, Some(n)) if manga || collected || base_label.starts_with("Vol.") => format!("Vol. {n}"),
            (true, Some(n)) => format!("#{n}"),
            _ => base_label,
        };
        let values: Vec<Option<String>> = META_FIELDS.iter().map(|f| text(f)).collect();
        let mut args: Vec<&dyn rusqlite::ToSql> = vec![&id, &label];
        for v in &values {
            args.push(v);
        }
        n += up.execute(args.as_slice())?;
    }
    Ok(n)
}

fn snapshot_base(conn: &Connection, id: i64, label: &str) -> Result<String> {
    let cols = META_FIELDS.join(", ");
    let mut m = Meta::new();
    m.insert("label".into(), serde_json::Value::String(label.to_string()));
    conn.query_row(&format!("SELECT {cols} FROM comics WHERE id=?1"), [id], |r| {
        for (i, f) in META_FIELDS.iter().enumerate() {
            if let Some(v) = r.get::<_, Option<String>>(i)? {
                m.insert(f.to_string(), serde_json::Value::String(v));
            }
        }
        Ok(())
    })?;
    Ok(serde_json::to_string(&m)?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComicMeta {
    pub id: i64,
    pub series_id: i64,
    pub series_name: String,
    pub label: String,
    pub is_manga: bool,
    pub file_name: String,
    /// Values in effect.
    pub fields: Meta,
    /// Fields you've edited by hand.
    pub edited: Vec<String>,
    /// Fields that came from AionComics' own Comic Vine lookup.
    pub from_cv: Vec<String>,
}

pub fn comic_meta(conn: &Connection, id: i64) -> Result<ComicMeta> {
    let cols = META_FIELDS.iter().map(|f| format!("c.{f}")).collect::<Vec<_>>().join(", ");
    let (series_id, series_name, label, is_manga, file_name, fields) = conn.query_row(
        &format!(
            "SELECT s.id, s.name, c.label, COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga), c.file_name, {cols} FROM comics c
             JOIN series s ON s.id=c.series_id JOIN collections col ON col.id=s.collection_id WHERE c.id=?1"
        ),
        [id],
        |r| {
            let mut m = Meta::new();
            for (i, f) in META_FIELDS.iter().enumerate() {
                let v: Option<String> = r.get(5 + i)?;
                m.insert(f.to_string(), v.map(serde_json::Value::String).unwrap_or(serde_json::Value::Null));
            }
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, m))
        },
    )?;
    Ok(ComicMeta {
        id,
        series_id,
        series_name,
        label,
        is_manga,
        file_name,
        fields,
        edited: read_layer(conn, id, "user")?.keys().cloned().collect(),
        from_cv: read_layer(conn, id, "cv")?.keys().cloned().collect(),
    })
}

/// Who's in each of these books (teams, characters), in one go for the series page's cast order.
pub fn casts(conn: &Connection, ids: &[i64]) -> Result<Vec<(Option<String>, Option<String>)>> {
    let mut st = conn.prepare_cached("SELECT teams, characters FROM comics WHERE id=?1")?;
    let mut out = Vec::with_capacity(ids.len());
    for &id in ids {
        if let Some(row) = st.query_row([id], |r| Ok((r.get(0)?, r.get(1)?))).optional()? {
            out.push(row);
        }
    }
    Ok(out)
}

/// Save hand edits: each field given replaces any earlier edit; `null` drops the edit so the
/// fetched or imported value shows again.
pub fn edit_meta(conn: &Connection, id: i64, changes: &Meta, now: i64) -> Result<()> {
    let mut user = read_layer(conn, id, "user")?;
    for (k, v) in changes {
        if !META_FIELDS.contains(&k.as_str()) {
            continue;
        }
        if v.is_null() {
            user.remove(k);
        } else {
            user.insert(k.clone(), v.clone());
        }
    }
    write_layer(conn, id, "user", &user, now)?;
    apply_meta(conn, Some(&[id]))?;
    Ok(())
}

/// Merge Comic Vine fields into a book's Comic Vine layer and apply it.
pub fn merge_cv(conn: &Connection, id: i64, fields: &Meta, now: i64) -> Result<()> {
    let mut cv = read_layer(conn, id, "cv")?;
    for (k, v) in fields {
        if META_FIELDS.contains(&k.as_str()) && !v.is_null() {
            cv.insert(k.clone(), v.clone());
        }
    }
    write_layer(conn, id, "cv", &cv, now)?;
    apply_meta(conn, Some(&[id]))?;
    Ok(())
}

#[cfg(test)]
mod meta_tests {
    use super::*;
    use serde_json::json;

    fn book() -> (Connection, i64) {
        let path = std::env::temp_dir().join(format!("aion-meta-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open(&path).unwrap();
        conn.execute("INSERT INTO collections(id, name, is_manga) VALUES(1, 'Manga', 1)", []).unwrap();
        conn.execute("INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Vinland Saga', 'Manga/Vinland Saga')", []).unwrap();
        conn.execute(
            "INSERT INTO comics(id, series_id, rel_path, file_name, label, title, writer, meta_base)
             VALUES(1, 1, 'Manga/Vinland Saga/v01.cbz', 'v01.cbz', 'Vol. 1', 'Imported', 'Yukimura',
                    '{\"label\":\"Vol. 1\",\"title\":\"Imported\",\"writer\":\"Yukimura\"}')",
            [],
        )
        .unwrap();
        (conn, 1)
    }

    fn get(conn: &Connection, col: &str) -> Option<String> {
        conn.query_row(&format!("SELECT {col} FROM comics WHERE id=1"), [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn rereading_a_finished_book() {
        let path = std::env::temp_dir().join(format!("aion-progress-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open(&path).unwrap();
        conn.execute("INSERT INTO collections(id, name, is_manga) VALUES(1, 'DC', 0)", []).unwrap();
        conn.execute("INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Absolute Batman', 'DC/AB')", []).unwrap();
        conn.execute("INSERT INTO comics(id, series_id, rel_path, file_name, label) VALUES(1, 1, 'DC/AB/15.cbz', '15.cbz', '#15')", []).unwrap();
        let read = |c: &Connection| c.query_row("SELECT read FROM comics WHERE id=1", [], |r| r.get::<_, bool>(0)).unwrap();

        set_progress(&conn, 1, 37, 38, 1).unwrap();
        assert!(read(&conn), "last page finishes it");
        set_progress(&conn, 1, 0, 38, 2).unwrap();
        assert!(read(&conn), "opening it to look keeps it finished");
        set_progress(&conn, 1, 2, 38, 3).unwrap();
        assert!(!read(&conn), "turning pages starts a re-read");
        set_progress(&conn, 1, 37, 38, 4).unwrap();
        assert!(read(&conn));
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn taking_a_book_off_continue_reading() {
        let path = std::env::temp_dir().join(format!("aion-hidden-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open(&path).unwrap();
        conn.execute("INSERT INTO collections(id, name, is_manga) VALUES(1, 'DC', 0)", []).unwrap();
        conn.execute("INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Absolute Batman', 'DC/AB')", []).unwrap();
        conn.execute("INSERT INTO comics(id, series_id, rel_path, file_name, label) VALUES(1, 1, 'DC/AB/15.cbz', '15.cbz', '#15')", []).unwrap();
        let day = 86_400;
        let shown = |c: &Connection, now| continue_reading(c, now, 10).unwrap().len() + pick_up_again(c, now, 10).unwrap().len();
        let page = |c: &Connection| c.query_row("SELECT current_page FROM comics WHERE id=1", [], |r| r.get::<_, i64>(0)).unwrap();

        set_progress(&conn, 1, 12, 38, day).unwrap();
        assert_eq!(shown(&conn, day), 1);
        hide_from_reading(&conn, 1, day).unwrap();
        assert_eq!(shown(&conn, day), 0, "gone from Continue reading");
        assert_eq!(shown(&conn, day + RECENT_SECS + 1), 0, "and from Pick up again");
        assert_eq!(page(&conn), 12, "its place is kept");
        set_progress(&conn, 1, 13, 38, 2 * day).unwrap();
        assert_eq!(shown(&conn, 2 * day), 1, "reading it again brings it back");

        let before = hide_from_reading(&conn, 1, 3 * day).unwrap();
        assert_eq!(taken_off_reading(&conn).unwrap().len(), 1, "listed as taken off");
        restore_reading(&conn, &before).unwrap();
        assert_eq!(shown(&conn, 3 * day), 1, "undone");
        assert!(taken_off_reading(&conn).unwrap().is_empty());
        hide_from_reading(&conn, 1, 3 * day).unwrap();
        unhide_from_reading(&conn, 1).unwrap();
        assert_eq!(shown(&conn, 3 * day), 1, "put back by hand");

        let before = set_read(&conn, 1, true, 4 * day).unwrap();
        assert_eq!(shown(&conn, 4 * day), 0, "finished");
        restore_reading(&conn, &before).unwrap();
        assert_eq!((page(&conn), shown(&conn, 4 * day)), (13, 1), "marking it read undone, its place kept");
        let before = set_series_read(&conn, 1, false, 5 * day).unwrap();
        assert_eq!(page(&conn), 0);
        restore_reading(&conn, &before).unwrap();
        assert_eq!(page(&conn), 13, "marking the series unread undone");
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn changing_the_format_of_several_books() {
        let path = std::env::temp_dir().join(format!("aion-editions-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open(&path).unwrap();
        conn.execute("INSERT INTO collections(id, name, is_manga) VALUES(1, 'DC', 0)", []).unwrap();
        conn.execute("INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Absolute Batman', 'DC/AB')", []).unwrap();
        for i in 1..=3 {
            conn.execute(
                "INSERT INTO comics(id, series_id, rel_path, file_name, label) VALUES(?1, 1, ?2, ?3, ?4)",
                params![i, format!("DC/AB/{i}.cbz"), format!("{i}.cbz"), format!("#{i}")],
            )
            .unwrap();
        }
        let kinds = |c: &Connection| -> Vec<Option<String>> {
            (1..=3)
                .map(|i| c.query_row("SELECT kind FROM edition_override WHERE comic_id=?1", [i], |r| r.get(0)).optional().unwrap())
                .collect()
        };
        set_edition(&conn, 1, Some("annual")).unwrap();
        let before = set_editions(&conn, &[1, 2], Some("collection")).unwrap();
        assert_eq!(kinds(&conn), [Some("collection".into()), Some("collection".into()), None]);
        restore_editions(&conn, &before).unwrap();
        assert_eq!(kinds(&conn), [Some("annual".into()), None, None], "undone, the hand-set one kept");
        let before = set_editions(&conn, &[1, 2, 3], None).unwrap();
        assert_eq!(kinds(&conn), [None, None, None]);
        restore_editions(&conn, &before).unwrap();
        assert_eq!(kinds(&conn)[0], Some("annual".into()));
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    /// Every book in a copy of a real library whose series-page number disagrees with the number
    /// Comic Vine or a hand edit gave it. `AION_TEST_DB` names the copy.
    #[test]
    #[ignore]
    fn real_numbers() {
        let path = std::env::var("AION_TEST_DB").expect("AION_TEST_DB");
        let conn = Connection::open(path).unwrap();
        let series: Vec<(i64, String, bool)> = {
            let mut st = conn
                .prepare("SELECT s.id, s.name, col.is_manga FROM series s JOIN collections col ON col.id=s.collection_id ORDER BY s.name")
                .unwrap();
            let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            v
        };
        let mut wrong = 0;
        for (id, name, manga) in series {
            for e in editions_of(&conn, id, manga).unwrap() {
                for c in e.sections.iter().flat_map(|s| &s.comics) {
                    let saved: Option<String> = conn
                        .query_row(
                            "SELECT c.number FROM comics c WHERE c.id=?1 AND EXISTS (SELECT 1 FROM meta_layer m WHERE m.comic_id=c.id
                             AND m.source IN ('cv','user') AND json_extract(m.data, '$.number') IS NOT NULL)",
                            [c.id],
                            |r| r.get(0),
                        )
                        .optional()
                        .unwrap()
                        .flatten();
                    let Some(n) = saved.and_then(|s| s.trim().trim_start_matches('#').parse::<f64>().ok()) else { continue };
                    let shown = c.label.trim_start_matches(|ch: char| !ch.is_ascii_digit()).parse::<f64>().ok();
                    if shown != Some(n) {
                        wrong += 1;
                        println!("{name}: shows {:?}, saved {n} ({})", c.label, e.kind);
                    }
                }
            }
        }
        println!("{wrong} books show the wrong number");
    }

    #[test]
    fn edits_win_and_revert() {
        let (conn, id) = book();
        let m = |v: serde_json::Value| v.as_object().unwrap().clone();
        merge_cv(&conn, id, &m(json!({"title": "Book One", "number": "1", "penciller": "Yukimura"})), 1).unwrap();
        assert_eq!(get(&conn, "title").as_deref(), Some("Book One"));
        assert_eq!(get(&conn, "writer").as_deref(), Some("Yukimura"), "imported field kept");
        assert_eq!(get(&conn, "label").as_deref(), Some("Vol. 1"));

        edit_meta(&conn, id, &m(json!({"title": "My title", "number": "2", "genre": "Seinen"})), 2).unwrap();
        assert_eq!(get(&conn, "title").as_deref(), Some("My title"), "hand edit beats Comic Vine");
        assert_eq!(get(&conn, "label").as_deref(), Some("Vol. 2"), "number edit relabels");
        assert_eq!(get(&conn, "genre").as_deref(), Some("Seinen"));

        edit_meta(&conn, id, &m(json!({"title": null, "genre": ""})), 3).unwrap();
        assert_eq!(get(&conn, "title").as_deref(), Some("Book One"), "dropping the edit shows Comic Vine again");
        assert_eq!(get(&conn, "genre"), None, "an emptied field stays empty");

        // A rescan rewrites the columns from the import; reapplying puts the layers back.
        conn.execute("UPDATE comics SET title='Imported', label='Vol. 1', genre=NULL WHERE id=1", []).unwrap();
        apply_meta(&conn, None).unwrap();
        assert_eq!(get(&conn, "title").as_deref(), Some("Book One"));
        assert_eq!(get(&conn, "label").as_deref(), Some("Vol. 2"));
        let meta = comic_meta(&conn, id).unwrap();
        assert!(meta.edited.contains(&"number".to_string()) && meta.from_cv.contains(&"penciller".to_string()));
    }
}


#[cfg(test)]
mod print_tests {
    use super::*;

    #[test]
    fn prints_wanted_covers_every_series() {
        let path = std::env::temp_dir().join(format!("aion-prints-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open(&path).unwrap();
        conn.execute_batch(
            "INSERT INTO collections(id, name, is_manga) VALUES(1, 'DC', 0);
             INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Red first', 'DC/1'), (2, 1, 'Teal then red', 'DC/2'), (3, 1, 'All gray', 'DC/3');
             INSERT INTO comics(id, series_id, rel_path, file_name, label, sort_key, cover_color, current_page, read, last_read_at) VALUES
               (10, 1, 'DC/1/a', 'a', '1', 1, '#c02020', 0, 0, NULL),
               (11, 1, 'DC/1/b', 'b', '2', 2, '#2030c0', 0, 0, NULL),
               (20, 2, 'DC/2/a', 'a', '1', 1, '#59bab3', 0, 0, NULL),
               (21, 2, 'DC/2/b', 'b', '2', 2, '#c02020', 0, 0, NULL),
               (30, 3, 'DC/3/a', 'a', '1', 1, '#808080', 0, 0, NULL),
               (31, 3, 'DC/3/b', 'b', '2', 2, '#59bab3', 5, 0, 100);",
        )
        .unwrap();
        assert_eq!(prints_wanted(&conn).unwrap(), [31, 10, 21, 30], "being read first, then each series' pick");
    }
}

#[cfg(test)]
mod story_tests {
    use super::*;

    /// Saga: issues #13–19, Vol. 3 (which collects #13–18), and an annual from after #14.
    fn saga(name: &str, collects: &str) -> Connection {
        let path = std::env::temp_dir().join(format!("aion-story-test-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = open(&path).unwrap();
        conn.execute("INSERT INTO collections(id, name, is_manga) VALUES(1, 'Image', 0)", []).unwrap();
        conn.execute("INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Saga', 'Image/Saga')", []).unwrap();
        for n in 13..=19 {
            conn.execute(
                "INSERT INTO comics(id, series_id, rel_path, file_name, label, sort_key, num_pages, date, added_at)
                 VALUES(?1, 1, ?2, ?3, ?4, ?1, 24, ?5, 100)",
                params![n, format!("Image/Saga/{n}.cbz"), format!("Saga 0{n}.cbz"), format!("#{n}"), format!("2014-{:02}-01", n - 12)],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO comics(id, series_id, rel_path, file_name, label, sort_key, num_pages, synopsis, added_at)
             VALUES(103, 1, 'Image/Saga/v3.cbz', 'Saga Vol. 03.cbz', 'Vol. 3', 103, 150, ?1, 100)",
            [collects],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO comics(id, series_id, rel_path, file_name, label, sort_key, num_pages, date, added_at)
             VALUES(201, 1, 'Image/Saga/a1.cbz', 'Saga Annual 001.cbz', 'Annual', 201, 40, '2014-02-15', 100)",
            [],
        )
        .unwrap();
        conn
    }

    fn read(conn: &Connection, ids: &[i64]) -> Vec<bool> {
        ids.iter().map(|id| conn.query_row("SELECT read FROM comics WHERE id=?1", [id], |r| r.get(0)).unwrap()).collect()
    }

    const RUN: [i64; 6] = [13, 14, 15, 16, 17, 18];

    #[test]
    fn want_to_read_lasts_until_you_start() {
        let conn = saga("want", "Collects SAGA #13-18.");
        // Read a little of #19 long ago; putting the series on the list afterward still counts.
        conn.execute("UPDATE comics SET current_page=5, last_read_at=500 WHERE id=19", []).unwrap();
        set_wanted(&conn, Some(1), None, true, 1000).unwrap();
        set_wanted(&conn, None, Some(103), true, 1000).unwrap();
        assert_eq!(want_count(&conn).unwrap(), 2);
        let v = series_view(&conn, 1).unwrap();
        assert!(v.wanted);
        assert_eq!(v.wanted_books, [103]);
        assert!(collection_view(&conn, 1).unwrap().series[0].wanted);
        let list = want_list(&conn).unwrap();
        assert_eq!((list.series.len(), list.books.len(), list.series_at[0], list.books_at[0]), (1, 1, 1000, 1000));

        // Starting any book in it takes the series off; the book on its own stays.
        conn.execute("UPDATE comics SET current_page=3, last_read_at=1100 WHERE id=13", []).unwrap();
        assert!(!series_view(&conn, 1).unwrap().wanted);
        assert_eq!(want_count(&conn).unwrap(), 1);
        // Put back on, it counts from then.
        set_wanted(&conn, Some(1), None, true, 1200).unwrap();
        assert!(series_view(&conn, 1).unwrap().wanted);
        // Reading the volume takes it off, and it took its issues (and so the series) with it.
        set_read(&conn, 103, true, 1300).unwrap();
        assert_eq!(want_count(&conn).unwrap(), 0);
        assert!(want_list(&conn).unwrap().books.is_empty());

        // Taken off by hand, and gone with the books.
        set_wanted(&conn, Some(1), None, true, 1400).unwrap();
        set_wanted(&conn, None, Some(19), true, 1400).unwrap();
        set_wanted(&conn, None, Some(19), false, 1500).unwrap();
        assert_eq!(want_count(&conn).unwrap(), 1);
        conn.execute("DELETE FROM comics WHERE series_id=1", []).unwrap();
        crate::removal::clear_leftovers(&conn).unwrap();
        let left: i64 = conn.query_row("SELECT (SELECT COUNT(*) FROM want_series) + (SELECT COUNT(*) FROM want_books)", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn finishing_a_collection_finishes_its_issues() {
        let conn = saga("down", "Collects SAGA #13-18.");
        let before = set_read(&conn, 103, true, 500).unwrap();
        assert_eq!(read(&conn, &RUN), [true; 6]);
        assert_eq!(read(&conn, &[19, 201]), [false, false], "nothing outside it");
        // Undo puts back every book it changed.
        assert_eq!(before.len(), 7);
        restore_reading(&conn, &before).unwrap();
        assert_eq!(read(&conn, &RUN), [false; 6]);
        assert_eq!(read(&conn, &[103]), [false]);
    }

    #[test]
    fn reaching_the_last_page_counts_too() {
        let conn = saga("page", "Collects SAGA #13-18.");
        set_progress(&conn, 103, 149, 150, 500).unwrap();
        assert_eq!(read(&conn, &RUN), [true; 6]);
        // Your own reading stats and page in an issue aren't invented.
        let at: i64 = conn.query_row("SELECT current_page FROM comics WHERE id=13", [], |r| r.get(0)).unwrap();
        assert_eq!(at, 0);
    }

    #[test]
    fn finishing_every_issue_finishes_the_collection() {
        let conn = saga("up", "Collects SAGA #13-18.");
        for id in &RUN[..5] {
            set_progress(&conn, *id, 23, 24, 500).unwrap();
            assert_eq!(read(&conn, &[103]), [false], "not until the last");
        }
        set_progress(&conn, 18, 23, 24, 500).unwrap();
        assert_eq!(read(&conn, &[103]), [true]);
    }

    #[test]
    fn unread_stays_with_the_one_book() {
        let conn = saga("unread", "Collects SAGA #13-18.");
        set_read(&conn, 103, true, 500).unwrap();
        set_read(&conn, 103, false, 600).unwrap();
        assert_eq!(read(&conn, &RUN), [true; 6]);
        assert_eq!(read(&conn, &[103]), [false]);
        // An issue you've put back unread isn't marked again by finishing a different one.
        set_read(&conn, 14, false, 700).unwrap();
        set_progress(&conn, 15, 23, 24, 800).unwrap();
        assert_eq!(read(&conn, &[14, 103]), [false, false]);
    }

    #[test]
    fn another_series_numbers_mean_nothing_here() {
        let conn = saga("other", "Collects Superman #13-18.");
        set_read(&conn, 103, true, 500).unwrap();
        assert_eq!(read(&conn, &RUN), [false; 6]);
    }

    #[test]
    fn annuals_are_read_where_they_came_out() {
        let conn = saga("order", "Collects SAGA #13-18.");
        let v = series_view(&conn, 1).unwrap();
        let story = v.editions.iter().find(|e| e.kind == STORY).expect("an in-order tab");
        let labels: Vec<&str> = story.sections[0].comics.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["#13", "#14", "Annual #1", "#15", "#16", "#17", "#18", "#19"]);
        // It's a view of the books, not more books.
        assert_eq!(v.sections.iter().map(|s| s.comics.len()).sum::<usize>(), 9);
        let annuals = v.editions.iter().find(|e| e.kind == "annual").unwrap();
        assert_eq!(annuals.sections[0].comics[0].after.as_deref(), Some("#14"));
        // And the reader offers them in that order.
        assert_eq!(neighbors(&conn, 14).unwrap(), (Some(13), Some(201)));
        assert_eq!(neighbors(&conn, 201).unwrap(), (Some(14), Some(15)));
        assert_eq!(neighbors(&conn, 19).unwrap(), (Some(18), None));
    }

    #[test]
    fn a_series_keeps_the_format_order_you_gave_it() {
        let conn = saga("formats", "Collects SAGA #13-18.");
        let kinds = |c: &Connection| series_view(c, 1).unwrap().editions.into_iter().map(|e| e.kind).collect::<Vec<_>>();
        let usual = kinds(&conn);
        assert!(usual.len() >= 3, "{usual:?}");
        assert_eq!(series_view(&conn, 1).unwrap().format_order, None);

        let mut order = usual.clone();
        order.reverse();
        assert_eq!(set_format_order(&conn, 1, Some(&order)).unwrap(), None);
        assert_eq!(kinds(&conn), order);
        let v = series_view(&conn, 1).unwrap();
        assert_eq!(v.format_order.as_deref(), Some(&order[..]));
        // The whole-series list follows, and still holds each book once.
        assert_eq!(v.sections.iter().map(|s| s.comics.len()).sum::<usize>(), 9);

        // A format that isn't in the order, or no longer in the series, doesn't get in the way.
        let last = usual.last().unwrap().clone();
        let part = ["omnibus".to_string(), last.clone(), "nonsense".to_string()];
        let before = set_format_order(&conn, 1, Some(&part)).unwrap();
        assert_eq!(before.as_deref(), Some(&order[..]));
        let now = kinds(&conn);
        assert_eq!(now[0], last);
        assert_eq!(now.len(), usual.len());
        assert_eq!(series_view(&conn, 1).unwrap().format_order.unwrap(), ["omnibus".to_string(), last]);

        // Undo puts it back, all the way to never having been set.
        set_format_order(&conn, 1, None).unwrap();
        assert_eq!(kinds(&conn), usual);
        assert_eq!(series_view(&conn, 1).unwrap().format_order, None);
    }

    #[test]
    fn search_finds_more_than_series() {
        let conn = saga("search", "");
        conn.execute("UPDATE comics SET title='The Will', penciller='Fiona Staples', characters='Marko\nAlana', story_arc='Chapter Three' WHERE id=15", []).unwrap();
        conn.execute("UPDATE comics SET writer='Brian K. Vaughan\nFiona Staples' WHERE id=16", []).unwrap();
        let names = |v: &[NameHit]| v.iter().map(|h| (h.name.clone(), h.books)).collect::<Vec<_>>();
        assert_eq!(search_all(&conn, "saga").unwrap().series.len(), 1);
        // Every book's file has "Saga" in it, and so does the series: those aren't book hits.
        assert!(search_all(&conn, "saga").unwrap().books.is_empty());
        assert_eq!(search_all(&conn, "the will").unwrap().books[0].id, 15);
        assert_eq!(names(&search_all(&conn, "staples").unwrap().creators), [("Fiona Staples".to_string(), 2)]);
        assert_eq!(names(&search_all(&conn, "alana").unwrap().characters), [("Alana".to_string(), 1)]);
        assert_eq!(names(&search_all(&conn, "chapter").unwrap().arcs), [("Chapter Three".to_string(), 1)]);
        assert_eq!(people_view(&conn, "arc", "Chapter Three").unwrap().books, 1);
        assert!(search_all(&conn, "100%").unwrap().series.is_empty());
    }

    #[test]
    fn search_puts_the_closest_matches_first() {
        assert_eq!(closeness("King", "king"), 0);
        assert_eq!(closeness("Kingdom", "king"), 1);
        assert_eq!(closeness("Tom King", "king"), 2);
        assert_eq!(closeness("Batman – Kings of Fear", "king"), 2);
        assert_eq!(closeness("The Walking Dead", "king"), 3);

        let conn = saga("closest", "");
        let col: i64 = conn.query_row("SELECT collection_id FROM series LIMIT 1", [], |r| r.get(0)).unwrap();
        for (id, name) in [(50, "The Walking Sagas"), (51, "Sagan"), (52, "Space Saga")] {
            conn.execute("INSERT INTO series (id, collection_id, name, rel_path) VALUES (?1, ?2, ?3, ?3)", rusqlite::params![id, col, name]).unwrap();
        }
        conn.execute("UPDATE comics SET last_read_at=9 WHERE series_id=(SELECT id FROM series WHERE name='Saga')", []).unwrap();
        let found = search_all(&conn, "saga").unwrap();
        let names: Vec<&str> = found.series.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Saga", "Sagan", "Space Saga", "The Walking Sagas"]);
        // Not partway through anything yet, then partway through one book.
        assert!(found.top_reading.is_none());
        conn.execute("UPDATE comics SET current_page=5, last_read_at=20 WHERE id=15", []).unwrap();
        assert_eq!(search_all(&conn, "saga").unwrap().top_reading.unwrap().id, 15);
    }
}

#[cfg(test)]
mod real_stories {
    //! `AION_TEST_DB=<copy of library.db> cargo test --lib real_stories -- --ignored --nocapture`
    use super::*;

    #[test]
    #[ignore]
    fn what_the_real_library_would_do() {
        let conn = open(Path::new(&std::env::var("AION_TEST_DB").unwrap())).unwrap();
        let ids: Vec<i64> = conn.prepare("SELECT id FROM series").unwrap().query_map([], |r| r.get(0)).unwrap().map(|x| x.unwrap()).collect();
        let t = std::time::Instant::now();
        let (mut stories, mut linked) = (0, 0);
        for id in ids {
            let books = series_books(&conn, id, series_is_manga(&conn, id).unwrap()).unwrap();
            let name = conn.query_row("SELECT name FROM series WHERE id=?1", [id], |r| r.get::<_, String>(0)).unwrap();
            let held: Vec<String> = books
                .iter()
                .filter(|b| matches!(b.kind, "volume" | "collection" | "deluxe" | "omnibus"))
                .filter_map(|b| b.span.map(|s| format!("{} {:?} {}-{}", b.card.label, s.unit, s.from, s.to)))
                .collect();
            let singles = books.iter().filter(|b| matches!(b.kind, "issue" | "chapter") && b.span.is_some()).count();
            if !held.is_empty() && singles > 0 {
                linked += 1;
                println!("LINKS {name}: {singles} singles; {}", held.join(", "));
            }
            if let Some(story) = story_of(&books) {
                stories += 1;
                let line: Vec<String> = story
                    .iter()
                    .map(|s| {
                        let b = &books[s.book];
                        if matches!(b.kind, "annual" | "special") { format!("[{}]", b.card.label) } else { b.card.label.clone() }
                    })
                    .collect();
                let shown = if line.len() > 24 { format!("{} … {}", line[..14].join(" "), line[line.len() - 8..].join(" ")) } else { line.join(" ") };
                println!("STORY {name} ({}): {shown}", books.len());
            }
        }
        println!("{stories} stories, {linked} series with links, in {:?}", t.elapsed());
        for q in ["batman", "bendis", "thorfinn", "year one"] {
            let t = std::time::Instant::now();
            let h = search_all(&conn, q).unwrap();
            println!("search {q:?} in {:?}: series {:?} books {:?} creators {:?} characters {:?} arcs {:?}", t.elapsed(),
                h.series.iter().map(|s| &s.name).collect::<Vec<_>>(),
                h.books.iter().map(|b| format!("{} {}", b.series_name, b.label)).collect::<Vec<_>>(),
                h.creators.iter().map(|n| (&n.name, n.books)).collect::<Vec<_>>(),
                h.characters.iter().map(|n| (&n.name, n.books)).collect::<Vec<_>>(),
                h.arcs.iter().map(|n| (&n.name, n.books)).collect::<Vec<_>>());
        }
    }
}
