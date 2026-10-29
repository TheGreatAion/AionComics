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
}

const CARD_COLS: &str = "c.id, c.label, c.title, c.year, s.id, s.name, c.current_page, c.num_pages, c.read,
  col.is_manga, c.cover_ratio, c.last_read_at, c.cover_color";
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
    /// The first book's cover color, "#rrggbb".
    pub cover_color: Option<String>,
}

const SERIES_SELECT: &str = "SELECT s.id, s.name, col.is_manga, COUNT(c.id), COALESCE(SUM(c.read),0),
       COALESCE(MAX(c.read=0 AND c.current_page>0),0),
       (SELECT c2.id FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       (SELECT c2.cover_ratio FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1),
       MAX(c.last_read_at), MAX(c.added_at),
       (SELECT c2.cover_color FROM comics c2 WHERE c2.series_id=s.id ORDER BY c2.sort_key LIMIT 1)
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
        cover_color: r.get(10)?,
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
    pub writer: Option<String>,
    /// Penciller (or artist), when known.
    pub artist: Option<String>,
    pub publisher: Option<String>,
    pub years: Option<String>,
    pub synopsis: Option<String>,
    /// Every book, format by format (see `editions`), in the order the page lists them.
    pub sections: Vec<Section>,
    pub editions: Vec<Edition>,
    pub backfill: Option<Backfill>,
    /// The Comic Vine series this one is matched to, if any.
    pub cv_volume_id: Option<String>,
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
    let fields: &[(&str, &str)] = if kind == "character" { &CHARACTER_FIELDS } else { &CREATOR_FIELDS };
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
    let (name, collection_id, collection_name, is_manga): (String, i64, String, bool) = conn.query_row(
        "SELECT s.name, col.id, col.name, col.is_manga FROM series s JOIN collections col ON col.id=s.collection_id WHERE s.id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let editions = editions_of(conn, id, is_manga)?;
    let sections = editions.iter().flat_map(|e| e.sections.iter()).map(|s| Section { name: s.name.clone(), comics: s.comics.clone() }).collect();
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
        editions,
        backfill: backfill(conn, id)?,
        cv_volume_id: cv.map(|(v, _)| v),
    })
}

/// A series' books sorted into their formats, each labeled and ordered within it.
fn editions_of(conn: &Connection, id: i64, manga: bool) -> Result<Vec<Edition>> {
    let mut st = conn.prepare(
        "SELECT c.section, c.file_name, COALESCE(c.num_pages, 0), COALESCE(c.synopsis, ''), o.kind
         FROM comics c LEFT JOIN edition_override o ON o.comic_id=c.id WHERE c.series_id=?1 ORDER BY c.sort_key",
    )?;
    let extra: Vec<(String, String, i64, String, Option<String>)> =
        st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<rusqlite::Result<_>>()?;
    let rows = cards(conn, "WHERE c.series_id=?1 ORDER BY c.sort_key", [id])?;
    // (format, subfolder, number, place in the old order, book)
    let mut books: Vec<(&'static str, String, Option<f64>, usize, ComicCard)> = Vec::new();
    for (i, (mut c, (sec, file, pages, synopsis, own))) in rows.into_iter().zip(extra).enumerate() {
        let kind = own
            .as_deref()
            .and_then(|k| editions::KINDS.iter().copied().find(|x| *x == k))
            .unwrap_or_else(|| editions::kind(manga, &sec, &file, pages));
        let n = editions::number(kind, &file);
        c.label = editions::label(kind, n, &c.label);
        if matches!(kind, "volume" | "collection" | "deluxe" | "omnibus") {
            c.collects = editions::collects(&synopsis);
        }
        c.edition = Some(kind.to_string());
        books.push((kind, sec, n, i, c));
    }
    let mut out = Vec::new();
    for kind in editions::KINDS {
        let mut mine: Vec<_> = books.iter().filter(|b| b.0 == kind).collect();
        if mine.is_empty() {
            continue;
        }
        let missing = if kind == "issue" { editions::gaps(&mine.iter().filter_map(|b| b.2).collect::<Vec<_>>()) } else { vec![] };
        // Subfolders in the order they first come; numbered books in number order within each.
        let mut folders: Vec<&str> = Vec::new();
        for b in &mine {
            if !folders.contains(&b.1.as_str()) {
                folders.push(&b.1);
            }
        }
        mine.sort_by(|a, b| {
            let fa = folders.iter().position(|f| *f == a.1).unwrap_or(0);
            let fb = folders.iter().position(|f| *f == b.1).unwrap_or(0);
            fa.cmp(&fb).then_with(|| match (a.2, b.2) {
                (Some(x), Some(y)) => x.total_cmp(&y).then(a.3.cmp(&b.3)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.3.cmp(&b.3),
            })
        });
        // A subfolder named for the format itself ("Issues", "TPB") goes without saying.
        let shown = |f: &str| {
            let last = f.rsplit('/').next().unwrap_or("").trim().to_lowercase();
            let plain = ["", "issues", "chapters", "volumes", "tpb", "tpbs", "trades", "omnibus", "annuals", "specials"];
            if plain.contains(&last.as_str()) { String::new() } else { f.to_string() }
        };
        let mut sections: Vec<Section> = Vec::new();
        for b in mine {
            let name = shown(&b.1);
            match sections.last_mut() {
                Some(s) if s.name == name => s.comics.push(b.4.clone()),
                _ => sections.push(Section { name, comics: vec![b.4.clone()] }),
            }
        }
        out.push(Edition { kind: kind.to_string(), name: editions::name(kind).to_string(), sections, missing });
    }
    Ok(out)
}

/// Give a book a format by hand (None: back to the one worked out from its name).
pub fn set_edition(conn: &Connection, comic: i64, kind: Option<&str>) -> Result<()> {
    match kind {
        Some(k) => conn.execute("INSERT OR REPLACE INTO edition_override(comic_id, kind) VALUES(?1, ?2)", params![comic, k])?,
        None => conn.execute("DELETE FROM edition_override WHERE comic_id=?1", [comic])?,
    };
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

/// Neighboring comics in the same series, for "next issue" in the reader.
pub fn neighbors(conn: &Connection, id: i64) -> Result<(Option<i64>, Option<i64>)> {
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
        // A number from Comic Vine or a hand edit relabels the book ("#5", "Vol. 5"). A book already
        // called a volume (a collected edition, say) stays one.
        let label = match (numbered, text("number")) {
            (true, Some(n)) if manga || base_label.starts_with("Vol.") => format!("Vol. {n}"),
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
