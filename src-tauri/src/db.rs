//! AionComics's own library database. Lives in the app data folder; the comics
//! folder itself is never written to.

use std::path::Path;

use anyhow::Result;
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
    let has = |col: &str| -> Result<bool> {
        let mut st = conn.prepare("SELECT 1 FROM pragma_table_info('comics') WHERE name=?1")?;
        Ok(st.exists([col])?)
    };
    if !has("cover_ratio")? {
        conn.execute("ALTER TABLE comics ADD COLUMN cover_ratio REAL", [])?;
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
         );",
    )?;
    Ok(())
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
}

const CARD_COLS: &str = "c.id, c.label, c.title, c.year, s.id, s.name, c.current_page, c.num_pages, c.read,
  col.is_manga, c.cover_ratio, c.last_read_at";
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
    })
}

fn cards(conn: &Connection, tail: &str, p: impl rusqlite::Params) -> Result<Vec<ComicCard>> {
    let mut st = conn.prepare(&format!("SELECT {CARD_COLS} FROM comics c {CARD_JOIN} {tail}"))?;
    let v = st.query_map(p, card)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

pub fn continue_reading(conn: &Connection, now: i64, limit: i64) -> Result<Vec<ComicCard>> {
    cards(
        conn,
        "WHERE c.read=0 AND c.current_page>0 AND c.last_read_at>=?1 ORDER BY c.last_read_at DESC LIMIT ?2",
        params![now - RECENT_SECS, limit],
    )
}

pub fn pick_up_again(conn: &Connection, now: i64, limit: i64) -> Result<Vec<ComicCard>> {
    cards(
        conn,
        "WHERE c.read=0 AND c.current_page>0 AND (c.last_read_at<?1 OR c.last_read_at IS NULL)
         ORDER BY c.last_read_at DESC NULLS LAST LIMIT ?2",
        params![now - RECENT_SECS, limit],
    )
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

pub fn recently_added(conn: &Connection, limit: i64) -> Result<Vec<ComicCard>> {
    cards(conn, "ORDER BY c.added_at DESC, c.sort_key DESC LIMIT ?1", [limit])
}

pub fn finished(conn: &Connection, limit: i64) -> Result<Vec<ComicCard>> {
    cards(conn, "WHERE c.read=1 ORDER BY c.last_read_at DESC NULLS LAST LIMIT ?1", [limit])
}

/// Recently added, folded per series so 25 new issues show as one stack.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentGroup {
    pub series_id: i64,
    pub series_name: String,
    pub is_manga: bool,
    pub count: i64,
    pub cover: ComicCard,
}

pub fn recent_groups(conn: &Connection, limit: i64) -> Result<Vec<RecentGroup>> {
    // A "batch" is everything added within two weeks of the series' newest book.
    let mut st = conn.prepare(
        "WITH g AS (SELECT series_id, MAX(added_at) AS newest FROM comics GROUP BY series_id
                    ORDER BY newest DESC LIMIT ?1)
         SELECT s.id, s.name, col.is_manga, g.newest,
           (SELECT COUNT(*) FROM comics b WHERE b.series_id=g.series_id AND b.added_at>=g.newest-1209600),
           (SELECT n.id FROM comics n WHERE n.series_id=g.series_id ORDER BY n.added_at DESC, n.sort_key DESC LIMIT 1)
         FROM g JOIN series s ON s.id=g.series_id JOIN collections col ON col.id=s.collection_id
         ORDER BY g.newest DESC",
    )?;
    let rows: Vec<(i64, String, bool, i64, i64)> = st
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(4)?, r.get(5)?)))?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|(series_id, series_name, is_manga, count, cover_id)| {
            Ok(RecentGroup { series_id, series_name, is_manga, count, cover: card_by_id(conn, cover_id)? })
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: i64,
    pub name: String,
    pub is_manga: bool,
    pub series_count: i64,
    pub comic_count: i64,
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
        "SELECT col.id, col.name, col.is_manga,
           (SELECT COUNT(*) FROM series s WHERE s.collection_id=col.id),
           (SELECT COUNT(*) FROM comics c JOIN series s ON s.id=c.series_id WHERE s.collection_id=col.id)
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
}

const SERIES_SELECT: &str = "SELECT s.id, s.name, col.is_manga, COUNT(c.id), COALESCE(SUM(c.read),0),
       COALESCE(MAX(c.read=0 AND c.current_page>0),0),
       (SELECT c2.id FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       (SELECT c2.cover_ratio FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       MAX(c.last_read_at), MAX(c.added_at)
     FROM series s JOIN collections col ON col.id=s.collection_id LEFT JOIN comics c ON c.series_id=s.id";

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
    })
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
        "SELECT name, is_manga FROM collections WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut st = conn.prepare(&format!(
        "{SERIES_SELECT} WHERE s.collection_id=?1 GROUP BY s.id ORDER BY s.name COLLATE NOCASE"
    ))?;
    let series = st.query_map([id], series_summary)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(CollectionView { id, name, is_manga, series })
}

/// Series whose name or writer matches.
pub fn search(conn: &Connection, q: &str) -> Result<Vec<SeriesSummary>> {
    let like = format!("%{}%", q.trim().replace('%', "\\%").replace('_', "\\_"));
    let mut st = conn.prepare(&format!(
        "{SERIES_SELECT} WHERE s.name LIKE ?1 ESCAPE '\\'
           OR EXISTS (SELECT 1 FROM comics w WHERE w.series_id=s.id AND w.writer LIKE ?1 ESCAPE '\\')
         GROUP BY s.id ORDER BY (s.name LIKE ?1 ESCAPE '\\') DESC, MAX(c.last_read_at) DESC NULLS LAST, s.name LIMIT 60"
    ))?;
    let v = st.query_map([like], series_summary)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub name: String,
    pub comics: Vec<ComicCard>,
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
    pub writer: Option<String>,
    /// Penciller (or artist), when known.
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub years: Option<String>,
    pub synopsis: Option<String>,
    pub sections: Vec<Section>,
    pub backfill: Option<Backfill>,
    /// The Comic Vine series this one is matched to, if any.
    pub cv_volume_id: Option<String>,
}

pub fn series_view(conn: &Connection, id: i64) -> Result<SeriesView> {
    let (name, collection_id, collection_name, is_manga): (String, i64, String, bool) = conn.query_row(
        "SELECT s.name, col.id, col.name, col.is_manga FROM series s JOIN collections col ON col.id=s.collection_id WHERE s.id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let mut sec_names = conn.prepare("SELECT section FROM comics WHERE series_id=?1 ORDER BY sort_key")?;
    let names: Vec<String> = sec_names.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let rows = cards(conn, "WHERE c.series_id=?1 ORDER BY c.sort_key", [id])?;
    let mut sections: Vec<Section> = Vec::new();
    for (sec, c) in names.into_iter().zip(rows) {
        match sections.last_mut() {
            Some(s) if s.name == sec => s.comics.push(c),
            _ => sections.push(Section { name: sec, comics: vec![c] }),
        }
    }
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
        backfill: backfill(conn, id)?,
        cv_volume_id: cv.map(|(v, _)| v),
    })
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

pub fn mark_read_before(conn: &Connection, id: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE comics SET read=1, current_page=0, last_read_at=COALESCE(last_read_at, ?2)
         WHERE series_id=(SELECT series_id FROM comics WHERE id=?1)
           AND sort_key<(SELECT sort_key FROM comics WHERE id=?1) AND read=0",
        params![id, now],
    )?;
    Ok(())
}

pub fn set_series_read(conn: &Connection, series: i64, read: bool, now: i64) -> Result<()> {
    if read {
        conn.execute(
            "UPDATE comics SET read=1, current_page=0, last_read_at=COALESCE(last_read_at, ?2) WHERE series_id=?1",
            params![series, now],
        )?;
    } else {
        conn.execute("UPDATE comics SET read=0, current_page=0, last_read_at=NULL WHERE series_id=?1", [series])?;
    }
    Ok(())
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

/// Neighbouring comics in the same series, for "next issue" in the reader.
pub fn neighbours(conn: &Connection, id: i64) -> Result<(Option<i64>, Option<i64>)> {
    let (series, key): (i64, i64) =
        conn.query_row("SELECT series_id, sort_key FROM comics WHERE id=?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?;
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

pub fn card_by_id(conn: &Connection, id: i64) -> Result<ComicCard> {
    Ok(conn.query_row(&format!("SELECT {CARD_COLS} FROM comics c {CARD_JOIN} WHERE c.id=?1"), [id], card)?)
}

/// Reaching the last page finishes a book. Turning past the first page of a finished one starts a
/// re-read, so it shows as in progress again; only opening it to look leaves it finished.
pub fn set_progress(conn: &Connection, id: i64, page: i64, pages: i64, now: i64) -> Result<()> {
    let read = pages > 0 && page >= pages - 1;
    conn.execute(
        "UPDATE comics SET current_page=?2, num_pages=?3, last_read_at=?4,
           read=CASE WHEN ?5 THEN 1 WHEN ?2>0 THEN 0 ELSE read END WHERE id=?1",
        params![id, page, pages, now, read],
    )?;
    Ok(())
}

pub fn set_read(conn: &Connection, id: i64, read: bool, now: i64) -> Result<()> {
    if read {
        conn.execute("UPDATE comics SET read=1, last_read_at=?2 WHERE id=?1", params![id, now])?;
    } else {
        conn.execute("UPDATE comics SET read=0, current_page=0, last_read_at=NULL WHERE id=?1", params![id])?;
    }
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

fn read_layer(conn: &Connection, id: i64, source: &str) -> Result<Meta> {
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
/// what the scan imported, then Comic Vine on top, then hand edits on top of that.
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
        let Some((base, label, manga)): Option<(Option<String>, String, bool)> = conn
            .query_row(
                "SELECT c.meta_base, c.label, col.is_manga FROM comics c JOIN series s ON s.id=c.series_id
                 JOIN collections col ON col.id=s.collection_id WHERE c.id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
        else {
            continue;
        };
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
        let mut m: Meta = serde_json::from_str(&base).unwrap_or_default();
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
        // A number from Comic Vine or a hand edit relabels the book ("#5", "Vol. 5").
        let label = match (numbered, text("number")) {
            (true, Some(n)) => if manga { format!("Vol. {n}") } else { format!("#{n}") },
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
            "SELECT s.id, s.name, c.label, col.is_manga, c.file_name, {cols} FROM comics c
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
