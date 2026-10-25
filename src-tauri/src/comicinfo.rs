//! Details stored inside comic files: the `ComicInfo.xml` that ComicRack, ComicTagger, Mylar and
//! most digital releases put in the archive. Read once per file (again only if the file changes)
//! into the `file` metadata layer, which fills in what the YACReader import doesn't have; Comic Vine
//! and hand edits still go on top. Files are only ever read.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use anyhow::Result;
use regex::Regex;
use rusqlite::{params, Connection};

use crate::archive::{self, Kind};
use crate::db::{self, Meta};

/// Anything bigger isn't a ComicInfo.xml worth reading.
const MAX_SIZE: u64 = 1 << 20;

fn is_info(name: &str) -> bool {
    let n = name.replace('\\', "/").to_ascii_lowercase();
    n.rsplit('/').next() == Some("comicinfo.xml") && !n.contains("__macosx")
}

/// The raw ComicInfo.xml of a comic file or folder, if it has one.
pub fn read(path: &Path) -> Result<Option<String>> {
    let bytes = match archive::detect(path)? {
        Kind::Zip => {
            let mut z = archive::open_zip(path)?;
            let Some(name) = z.file_names().filter_map(|n| n.ok()).find(|n| is_info(n)).map(|n| n.into_owned()) else {
                return Ok(None);
            };
            let mut f = z.by_name(&name)?;
            if f.size() > MAX_SIZE {
                return Ok(None);
            }
            let mut b = Vec::new();
            f.read_to_end(&mut b)?;
            b
        }
        Kind::Rar => {
            // Headers only first: most RARs don't have one, and finding that out is cheap.
            let mut found = None;
            for e in unrar::Archive::new(path).open_for_listing()? {
                let e = e?;
                if e.is_file() && is_info(&e.filename.to_string_lossy()) && e.unpacked_size <= MAX_SIZE {
                    found = Some(e.filename.to_string_lossy().replace('\\', "/"));
                    break;
                }
            }
            let Some(name) = found else { return Ok(None) };
            archive::rar_read_one(path, &name)?
        }
        Kind::Folder => {
            let Some(e) = fs::read_dir(path)?.filter_map(|e| e.ok()).find(|e| is_info(&e.file_name().to_string_lossy())) else {
                return Ok(None);
            };
            if e.metadata()?.len() > MAX_SIZE {
                return Ok(None);
            }
            fs::read(e.path())?
        }
    };
    Ok(Some(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string()))
}

/// Titles that only restate the number ("v03", "Book 01", "#5", "Volume One") or the format
/// ("TPB", "One-Shot") aren't titles. Comic Vine names most collected editions this way too.
static PLACEHOLDER_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:v|vol\.?|volume|book|issue|no\.?|#|chapter|ch\.?|part)?\s*(?:\d+(?:\.\d+)?|one|two|three|four|five|six|seven|eight|nine|ten)|tpb|hc|hardcover|trade paperback|one[- ]shot|omnibus|o?gn)$",
    )
    .unwrap()
});

pub(crate) fn placeholder_title(t: &str) -> bool {
    PLACEHOLDER_TITLE.is_match(t.trim())
}
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<(\w+)>(.*?)</(\w+)>").unwrap());
static CV_ISSUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"comicvine\.gamespot\.com/[^\s]*?4000-(\d+)").unwrap());

fn unescape(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("<![CDATA[").and_then(|s| s.strip_suffix("]]>")).unwrap_or(s);
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// ComicInfo lists are comma separated; AionComics keeps one name per line.
fn lines(v: &str) -> String {
    v.split([',', ';', '\n']).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

/// AionComics fields from a ComicInfo.xml.
pub fn parse(xml: &str) -> Meta {
    let mut tags: std::collections::HashMap<String, String> = Default::default();
    for c in TAG.captures_iter(xml) {
        if c[1] == c[3] {
            let v = unescape(&c[2]);
            if !v.is_empty() {
                tags.entry(c[1].to_ascii_lowercase()).or_insert(v);
            }
        }
    }
    let mut m = Meta::new();
    let mut put = |k: &str, v: String| {
        if !v.is_empty() {
            m.insert(k.into(), serde_json::Value::String(v));
        }
    };
    let get = |k: &str| tags.get(k).cloned();
    for (field, tag, list) in [
        ("title", "title", false),
        ("volume", "series", false),
        ("number", "number", false),
        ("writer", "writer", true),
        ("penciller", "penciller", true),
        ("inker", "inker", true),
        ("colorist", "colorist", true),
        ("letterer", "letterer", true),
        ("cover_artist", "coverartist", true),
        ("publisher", "publisher", false),
        ("synopsis", "summary", false),
        ("characters", "characters", true),
        ("teams", "teams", true),
        ("locations", "locations", true),
        ("story_arc", "storyarc", false),
        ("arc_number", "storyarcnumber", false),
        ("genre", "genre", false),
    ] {
        if let Some(v) = get(tag) {
            put(field, if list { lines(&v) } else { v });
        }
    }
    if let Some(y) = get("year").filter(|y| y.len() == 4 && y.chars().all(|c| c.is_ascii_digit()) && y != "0000") {
        let num = |k: &str| get(k).and_then(|v| v.parse::<u32>().ok());
        match num("month").filter(|m| (1..=12).contains(m)) {
            Some(mo) => put("date", format!("{y}-{mo:02}-{:02}", num("day").filter(|d| (1..=31).contains(d)).unwrap_or(1))),
            None => {}
        }
        put("year", y);
    }
    if let Some(id) = get("web").and_then(|w| CV_ISSUE.captures(&w).map(|c| c[1].to_string())) {
        put("cv_id", id);
    }
    if m.get("title").and_then(|t| t.as_str()).is_some_and(placeholder_title) {
        m.remove("title");
    }
    m
}

/// Books whose file hasn't been checked for a ComicInfo.xml since it last changed.
fn pending(conn: &Connection, limit: i64) -> Result<Vec<(i64, String, i64, i64)>> {
    let mut st = conn.prepare(
        "SELECT c.id, c.rel_path, COALESCE(c.size, 0), COALESCE(c.mtime, 0) FROM comics c
         LEFT JOIN comicinfo_seen s ON s.comic_id=c.id
         WHERE s.comic_id IS NULL OR s.size!=COALESCE(c.size, 0) OR s.mtime!=COALESCE(c.mtime, 0)
         -- ZIPs answer in a couple of reads; big RARs take a lot of seeking, so they go last.
         ORDER BY lower(c.rel_path) LIKE '%.cbr' OR lower(c.rel_path) LIKE '%.rar', c.id
         LIMIT ?1",
    )?;
    let rows = st.query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Check up to `batch` books; returns the ids whose details changed, or None when all are done.
/// The file reading happens without the database lock held.
pub fn step(conn: &std::sync::Mutex<Connection>, root: &Path, batch: i64) -> Result<Option<Vec<i64>>> {
    let todo = pending(&conn.lock().unwrap(), batch)?;
    if todo.is_empty() {
        return Ok(None);
    }
    let found: Vec<(i64, i64, i64, Option<Meta>)> = todo
        .into_iter()
        .map(|(id, rel, size, mtime)| {
            // Unreadable or odd files count as checked; a change to the file retries them.
            let meta = read(&root.join(&rel)).ok().flatten().map(|x| parse(&x)).filter(|m| !m.is_empty());
            (id, size, mtime, meta)
        })
        .collect();
    let c = conn.lock().unwrap();
    let now = crate::scan::now();
    let mut changed = Vec::new();
    for (id, size, mtime, meta) in found {
        let had: bool = c.query_row("SELECT COUNT(*) FROM meta_layer WHERE comic_id=?1 AND source='file'", [id], |r| r.get::<_, i64>(0))? > 0;
        if meta.is_some() || had {
            db::write_layer(&c, id, "file", &meta.unwrap_or_default(), now)?;
            changed.push(id);
        }
        c.execute(
            "INSERT OR REPLACE INTO comicinfo_seen(comic_id, size, mtime) VALUES(?1, ?2, ?3)",
            params![id, size, mtime],
        )?;
    }
    if !changed.is_empty() {
        db::apply_meta(&c, Some(&changed))?;
    }
    Ok(Some(changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comicinfo() {
        let xml = r#"<?xml version="1.0"?>
<ComicInfo xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Title>The Seventh Kingdom, Part One</Title>
  <Series>Absolute Batman</Series>
  <Number>17</Number>
  <Year>2026</Year><Month>3</Month>
  <Writer>Scott Snyder</Writer>
  <Penciller>Eric Canete, Nick Dragotta</Penciller>
  <Publisher>DC Comics</Publisher>
  <Summary>Bruce &amp; Alfred face the &quot;Seventh Kingdom&quot; &#8212; at last.</Summary>
  <Characters>Batman, Poison Ivy</Characters>
  <Web>https://comicvine.gamespot.com/absolute-batman-17/4000-1123456/</Web>
  <Notes></Notes>
</ComicInfo>"#;
        let m = parse(xml);
        let s = |k: &str| m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        assert_eq!(s("title"), "The Seventh Kingdom, Part One");
        assert_eq!(s("volume"), "Absolute Batman");
        assert_eq!(s("number"), "17");
        assert_eq!(s("year"), "2026");
        assert_eq!(s("date"), "2026-03-01");
        assert_eq!(s("penciller"), "Eric Canete\nNick Dragotta");
        assert_eq!(s("synopsis"), "Bruce & Alfred face the \"Seventh Kingdom\" — at last.");
        assert_eq!(s("characters"), "Batman\nPoison Ivy");
        assert_eq!(s("cv_id"), "1123456");
        assert!(!m.contains_key("notes"));
        for junk in ["v03", "Book 01", "#5", "Vol. 12", "TPB", "Volume One", "One-Shot", "Book One"] {
            let m = parse(&format!("<ComicInfo><Title>{junk}</Title><Number>3</Number></ComicInfo>"));
            assert!(!m.contains_key("title"), "{junk}");
        }
    }
}
