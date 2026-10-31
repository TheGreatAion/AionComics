//! Library scanning: walk the comics folder, mirror it into the database,
//! and pull metadata + reading progress from an existing YACReader library.
//!
//! Folder convention: `Collection / Series / [sections…] / file`.
//! Files sitting directly inside a collection become one-book series.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;
use walkdir::WalkDir;

const EXTENSIONS: [&str; 4] = ["cbz", "cbr", "zip", "rar"];

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

struct Found {
    collection: String,
    series: String,
    series_rel: String,
    section: String,
    rel: String,
    file_name: String,
    size: i64,
    mtime: i64,
}

fn walk(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let walker = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'));
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
        if parts.len() < 2 {
            continue; // loose files at the library root
        }
        let file_name = parts[parts.len() - 1].clone();
        let (series, series_rel, section) = if parts.len() == 2 {
            let stem = Path::new(&file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| file_name.clone());
            (clean_name(&stem), parts.join("/"), String::new())
        } else {
            (
                parts[1].clone(),
                format!("{}/{}", parts[0], parts[1]),
                parts[2..parts.len() - 1].join(" / "),
            )
        };
        let meta = e.metadata().ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Found {
            collection: parts[0].clone(),
            series,
            series_rel,
            section,
            rel: parts.join("/"),
            file_name,
            size: meta.map(|m| m.len() as i64).unwrap_or(0),
            mtime,
        });
    }
    out
}

// ---------- YACReader import ----------

struct Yac {
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
    current_page: Option<i64>,
    read: bool,
    last_opened: Option<i64>,
    added: Option<i64>,
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

fn read_yac(root: &Path) -> HashMap<String, Yac> {
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
    let sql = format!(
        "SELECT c.path, ci.title, ci.number, ci.volume, ci.date, ci.writer, ci.publisher, ci.synopsis,
           ci.comicVineID, ci.hash, ci.numPages, ci.currentPage, ci.read, ci.lastTimeOpened, ci.added{extra_cols}
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
                extra: extras
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (_, field))| r.get::<_, rusqlite::types::Value>(15 + i).ok().and_then(as_text).map(|v| (*field, v)))
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
    let found = walk(root);
    let known: i64 = conn.query_row("SELECT COUNT(*) FROM comics", [], |r| r.get(0))?;
    if found.is_empty() && known > 0 {
        anyhow::bail!("No comics were found in {}. Nothing was changed.", root.display());
    }
    let yac = read_yac(root);
    let ts = now();
    let mut stats = ScanStats::default();

    let tx = conn.transaction()?;
    tx.execute("UPDATE comics SET seen=0", [])?;
    {
        let mut ins_col = tx.prepare(
            "INSERT INTO collections(name, is_manga) VALUES(?1, ?2)
             ON CONFLICT(name) DO UPDATE SET is_manga=excluded.is_manga RETURNING id",
        )?;
        let mut ins_series = tx.prepare(
            "INSERT INTO series(collection_id, name, rel_path) VALUES(?1, ?2, ?3)
             ON CONFLICT(rel_path) DO UPDATE SET name=excluded.name, collection_id=excluded.collection_id RETURNING id",
        )?;
        let mut existing = tx.prepare(
            "SELECT id, current_page, read, last_read_at FROM comics WHERE rel_path=?1",
        )?;
        let mut insert = tx.prepare(
            "INSERT INTO comics(series_id, section, rel_path, file_name, size, mtime, added_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
        )?;
        let mut update_file = tx.prepare(
            "UPDATE comics SET series_id=?2, section=?3, file_name=?4, size=?5, mtime=?6, seen=1 WHERE id=?1",
        )?;
        let mut update_meta = tx.prepare(
            "UPDATE comics SET label=?2, title=?3, number=?4, volume=?5, year=?6, writer=?7, publisher=?8,
               synopsis=?9, cv_id=?10, yac_hash=?11, num_pages=COALESCE(num_pages, ?12), meta_base=?13,
               date=?14, penciller=?15, inker=?16, colorist=?17, letterer=?18, cover_artist=?19,
               characters=?20, teams=?21, locations=?22, story_arc=?23, arc_number=?24, arc_count=?25,
               genre=?26, cv_volume_id=NULL WHERE id=?1",
        )?;
        let mut update_progress = tx.prepare(
            "UPDATE comics SET current_page=?2, read=?3, last_read_at=?4 WHERE id=?1",
        )?;

        let mut col_ids: HashMap<String, (i64, bool)> = HashMap::new();
        let mut series_ids: HashMap<String, i64> = HashMap::new();

        for f in &found {
            let (col_id, is_manga) = match col_ids.get(&f.collection) {
                Some(v) => *v,
                None => {
                    let manga = f.collection.eq_ignore_ascii_case("manga");
                    let id: i64 = ins_col.query_row(params![f.collection, manga], |r| r.get(0))?;
                    col_ids.insert(f.collection.clone(), (id, manga));
                    (id, manga)
                }
            };
            let series_id = match series_ids.get(&f.series_rel) {
                Some(id) => *id,
                None => {
                    let id: i64 =
                        ins_series.query_row(params![col_id, f.series, f.series_rel], |r| r.get(0))?;
                    series_ids.insert(f.series_rel.clone(), id);
                    id
                }
            };
            let y = yac.get(&f.rel);
            let prior: Option<(i64, i64, bool, Option<i64>)> = existing
                .query_row([&f.rel], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .optional()?;
            let (id, ours_last_read) = match prior {
                Some((id, _, _, last)) => {
                    update_file.execute(params![id, series_id, f.section, f.file_name, f.size, f.mtime])?;
                    (id, last)
                }
                None => {
                    let added = y.and_then(|y| y.added).filter(|a| *a > 0).unwrap_or(f.mtime.max(1));
                    let id: i64 = insert.query_row(
                        params![series_id, f.section, f.rel, f.file_name, f.size, f.mtime, added],
                        |r| r.get(0),
                    )?;
                    stats.added += 1;
                    (id, None)
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
            ])?;
            if let Some(y) = y {
                stats.yac_matched += 1;
                // Take YACReader's progress when it's newer than ours.
                let yac_opened = y.last_opened.unwrap_or(0);
                let has_progress = y.read || y.current_page.unwrap_or(1) > 1;
                if has_progress && yac_opened > ours_last_read.unwrap_or(0) {
                    let page = (y.current_page.unwrap_or(1) - 1).max(0);
                    let last = if yac_opened > 0 { yac_opened } else { ts };
                    update_progress.execute(params![id, page, y.read, last])?;
                }
            }
        }
    }
    // Losing more than a third of the library in one scan means a folder or drive dropped out, not
    // that you deleted that much. Keep those books until they're really gone.
    let missing: i64 = tx.query_row("SELECT COUNT(*) FROM comics WHERE seen=0", [], |r| r.get(0))?;
    if missing > 0 && known > 30 && missing * 3 > known {
        stats.kept_missing = missing;
    } else {
        stats.removed = tx.execute("DELETE FROM comics WHERE seen=0", [])? as i64;
    }
    tx.execute("DELETE FROM series WHERE id NOT IN (SELECT DISTINCT series_id FROM comics)", [])?;
    tx.execute(
        "DELETE FROM collections WHERE id NOT IN (SELECT DISTINCT collection_id FROM series)",
        [],
    )?;
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
