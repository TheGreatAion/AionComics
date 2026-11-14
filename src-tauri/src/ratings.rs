//! Your star ratings (1 to 5) for books and series, what they add up to, and the series on your
//! shelves they point to: ones you haven't started, by the people behind books you rated highly.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ratings(comic_id INTEGER PRIMARY KEY, stars INTEGER NOT NULL, at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS series_ratings(series_id INTEGER PRIMARY KEY, stars INTEGER NOT NULL, at INTEGER NOT NULL);
         CREATE TABLE IF NOT EXISTS suggest_skipped(series_id INTEGER PRIMARY KEY, at INTEGER NOT NULL);",
    )?;
    Ok(())
}

fn clamp(stars: Option<i64>) -> Option<i64> {
    stars.filter(|s| (1..=5).contains(s))
}

/// Rate a book, or with None take its rating away.
pub fn set_book(conn: &Connection, comic_id: i64, stars: Option<i64>, now: i64) -> Result<()> {
    match clamp(stars) {
        Some(s) => conn.execute(
            "INSERT INTO ratings(comic_id, stars, at) VALUES(?1, ?2, ?3) ON CONFLICT(comic_id) DO UPDATE SET stars=excluded.stars, at=excluded.at",
            params![comic_id, s, now],
        )?,
        None => conn.execute("DELETE FROM ratings WHERE comic_id=?1", [comic_id])?,
    };
    Ok(())
}

/// Rate a series as a whole, or with None go back to the average of its books.
pub fn set_series(conn: &Connection, series_id: i64, stars: Option<i64>, now: i64) -> Result<()> {
    match clamp(stars) {
        Some(s) => conn.execute(
            "INSERT INTO series_ratings(series_id, stars, at) VALUES(?1, ?2, ?3) ON CONFLICT(series_id) DO UPDATE SET stars=excluded.stars, at=excluded.at",
            params![series_id, s, now],
        )?,
        None => conn.execute("DELETE FROM series_ratings WHERE series_id=?1", [series_id])?,
    };
    Ok(())
}

/// How a series stands: your own rating of it, and the average of its books you rated.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SeriesStars {
    pub own: Option<i64>,
    pub average: Option<f64>,
    pub rated: i64,
}

pub fn series_stars(conn: &Connection, series_id: i64) -> Result<SeriesStars> {
    let own = conn.query_row("SELECT stars FROM series_ratings WHERE series_id=?1", [series_id], |r| r.get(0)).optional()?;
    let (rated, avg): (i64, Option<f64>) = conn.query_row(
        "SELECT COUNT(*), AVG(r.stars) FROM ratings r JOIN comics c ON c.id=r.comic_id WHERE c.series_id=?1",
        [series_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let books: i64 = conn.query_row("SELECT COUNT(*) FROM comics WHERE series_id=?1", [series_id], |r| r.get(0))?;
    Ok(SeriesStars { own, average: avg.filter(|_| rated >= 2.min(books.max(1))), rated })
}

fn people(writer: Option<String>, artist: Option<String>) -> HashSet<String> {
    [writer, artist]
        .into_iter()
        .flatten()
        .flat_map(|v| v.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect::<Vec<_>>())
        .collect()
}

/// Everything you rated, book by book: a series rating counts for each of its books you haven't
/// rated yourself. (comic, series, series name, stars, writer, artist)
type Rated = (i64, i64, String, i64, Option<String>, Option<String>);

fn rated_books(conn: &Connection) -> Result<Vec<Rated>> {
    let mut st = conn.prepare(
        "SELECT c.id, s.id, s.name, COALESCE(r.stars, sr.stars), c.writer, c.penciller
         FROM comics c JOIN series s ON s.id=c.series_id
         LEFT JOIN ratings r ON r.comic_id=c.id
         LEFT JOIN series_ratings sr ON sr.series_id=s.id
         WHERE r.stars IS NOT NULL OR (sr.stars IS NOT NULL AND (c.read=1 OR c.current_page>0))",
    )?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Favorite {
    pub name: String,
    pub average: f64,
    pub rated: i64,
}

/// The writers and artists you rate highest, from at least three rated books each.
pub fn favorite_creators(conn: &Connection) -> Result<Vec<Favorite>> {
    let mut by: HashMap<String, (i64, i64)> = HashMap::new();
    for (_, _, _, stars, writer, artist) in rated_books(conn)? {
        for p in people(writer, artist) {
            let e = by.entry(p).or_default();
            e.0 += stars;
            e.1 += 1;
        }
    }
    let mut v: Vec<Favorite> = by
        .into_iter()
        .filter(|(_, (_, n))| *n >= 3)
        .map(|(name, (sum, n))| Favorite { name, average: sum as f64 / n as f64, rated: n })
        .collect();
    v.sort_by(|a, b| b.average.total_cmp(&a.average).then(b.rated.cmp(&a.rated)).then_with(|| a.name.cmp(&b.name)));
    v.truncate(8);
    Ok(v)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub series_id: i64,
    pub name: String,
    pub books: i64,
    pub is_manga: bool,
    /// Its first books, for the covers.
    pub covers: Vec<i64>,
    /// Who it shares with what you liked, and that series.
    pub who: String,
    pub because: String,
}

/// Lowercase letters and digits only, for telling a series' other formats by name.
fn plain(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Series on your shelves you haven't started, by the people behind books you rated 4 or 5 stars:
/// up to `most`, changing with the day. None until three books (or series) are rated that highly.
pub fn suggestions(conn: &Connection, day: i64, most: usize) -> Result<Vec<Suggestion>> {
    // Each liked creator, with the series of theirs you liked best (and how much).
    let rated = rated_books(conn)?;
    let liked: Vec<&Rated> = rated.iter().filter(|r| r.3 >= 4).collect();
    let liked_series: HashSet<i64> = liked.iter().map(|r| r.1).collect();
    if liked.len() < 3 {
        return Ok(Vec::new());
    }
    let mut fans: HashMap<String, (f64, i64, String)> = HashMap::new(); // creator -> (weight, best stars, series)
    for (_, _, sname, stars, writer, artist) in &liked {
        for p in people(writer.clone(), artist.clone()) {
            let e = fans.entry(p).or_insert((0.0, 0, String::new()));
            e.0 += if *stars == 5 { 1.5 } else { 1.0 };
            if *stars > e.1 {
                e.1 = *stars;
                e.2 = sname.clone();
            }
        }
    }
    let liked_names: Vec<String> = liked.iter().map(|r| plain(&r.2)).filter(|n| n.len() >= 4).collect();

    // Series nobody's opened: no book read or begun, not passed over, not on Want to read.
    let mut st = conn.prepare(
        "SELECT s.id, s.name, COUNT(c.id), COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga),
                group_concat(COALESCE(c.writer,''), char(10)), group_concat(COALESCE(c.penciller,''), char(10))
         FROM series s JOIN collections col ON col.id=s.collection_id JOIN comics c ON c.series_id=s.id
         WHERE NOT EXISTS (SELECT 1 FROM suggest_skipped k WHERE k.series_id=s.id)
           AND NOT EXISTS (SELECT 1 FROM want_series w WHERE w.series_id=s.id)
         GROUP BY s.id
         HAVING MAX(c.read)=0 AND MAX(c.current_page)=0",
    )?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?))
    })?;
    let mut picks: Vec<(f64, Suggestion)> = Vec::new();
    for row in rows {
        let (id, name, books, is_manga, writer, artist) = row?;
        if liked_series.contains(&id) {
            continue;
        }
        // Another format of something you rated ("Chainsaw Man Deluxe") isn't news.
        let p = plain(&name);
        if liked_names.iter().any(|n| p.contains(n.as_str()) || n.contains(p.as_str())) {
            continue;
        }
        let mut best: Option<(&String, &(f64, i64, String))> = None;
        let mut score = 0.0;
        for person in people(writer, artist) {
            if let Some((k, v)) = fans.get_key_value(&person) {
                score += v.0;
                if best.is_none_or(|b| (v.1, v.0) > (b.1 .1, b.1 .0)) {
                    best = Some((k, v));
                }
            }
        }
        let Some((who, (_, _, because))) = best else { continue };
        picks.push((score, Suggestion { series_id: id, name, books, is_manga, covers: Vec::new(), who: who.clone(), because: because.clone() }));
    }
    // The best handful, a different few of them each day.
    picks.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
    picks.truncate(most * 3);
    let mix = |id: i64| ((id ^ day).wrapping_mul(0x9E37_79B9_7F4A_7C15_u64 as i64)).rotate_left(17);
    picks.sort_by_key(|(score, s)| (-(score.min(6.0) * 2.0).round() as i64, mix(s.series_id)));
    let mut out: Vec<Suggestion> = picks.into_iter().take(most).map(|(_, s)| s).collect();
    let mut covers = conn.prepare("SELECT id FROM comics WHERE series_id=?1 ORDER BY section!='', sort_key LIMIT 3")?;
    for s in &mut out {
        s.covers = covers.query_map([s.series_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    }
    Ok(out)
}

/// "Not interested": that series isn't suggested again.
pub fn skip(conn: &Connection, series_id: i64, now: i64) -> Result<()> {
    conn.execute("INSERT OR REPLACE INTO suggest_skipped(series_id, at) VALUES(?1, ?2)", params![series_id, now])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Connection {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        conn.execute_batch(
            "INSERT INTO collections(id, name) VALUES (1, 'Manga');
             INSERT INTO series(id, collection_id, name, rel_path) VALUES
               (1, 1, 'Chainsaw Man', 'a'), (2, 1, 'Fire Punch', 'b'), (3, 1, 'Look Back', 'c'),
               (4, 1, 'Vinland Saga', 'd'), (5, 1, 'Planetes', 'e'), (6, 1, 'Chainsaw Man Deluxe', 'f'),
               (7, 1, 'Something Else', 'g');",
        )
        .unwrap();
        let book = |id: i64, series: i64, writer: &str, read: i64| {
            conn.execute(
                "INSERT INTO comics(id, series_id, section, rel_path, file_name, sort_key, label, size, mtime, added_at, writer, read, current_page)
                 VALUES(?1, ?2, '', ?3, ?3, ?3, ?3, 0, 0, 0, ?4, ?5, 0)",
                params![id, series, format!("{id}.cbz"), writer, read],
            )
            .unwrap();
        };
        book(1, 1, "Tatsuki Fujimoto", 1);
        book(2, 1, "Tatsuki Fujimoto", 1);
        book(3, 1, "Tatsuki Fujimoto", 1);
        book(4, 2, "Tatsuki Fujimoto", 0);
        book(5, 3, "Tatsuki Fujimoto", 0);
        book(6, 4, "Makoto Yukimura", 1);
        book(7, 5, "Makoto Yukimura", 0);
        book(8, 6, "Tatsuki Fujimoto", 0);
        book(9, 7, "Nobody", 0);
        conn
    }

    #[test]
    fn stars_and_averages() {
        let conn = library();
        set_book(&conn, 1, Some(5), 1).unwrap();
        assert!(series_stars(&conn, 1).unwrap().average.is_none(), "one of three books isn't an average yet");
        set_book(&conn, 2, Some(4), 1).unwrap();
        let s = series_stars(&conn, 1).unwrap();
        assert_eq!((s.own, s.average, s.rated), (None, Some(4.5), 2));
        set_series(&conn, 1, Some(3), 1).unwrap();
        assert_eq!(series_stars(&conn, 1).unwrap().own, Some(3));
        set_book(&conn, 2, None, 1).unwrap();
        set_book(&conn, 2, Some(9), 1).unwrap();
        assert_eq!(series_stars(&conn, 1).unwrap().rated, 1, "out of range takes it away");
        // A one-book series averages from its one book.
        set_book(&conn, 6, Some(5), 1).unwrap();
        assert_eq!(series_stars(&conn, 4).unwrap().average, Some(5.0));
    }

    #[test]
    fn suggestions_from_what_you_liked() {
        let conn = library();
        set_book(&conn, 1, Some(5), 1).unwrap();
        set_book(&conn, 2, Some(5), 1).unwrap();
        assert!(suggestions(&conn, 0, 4).unwrap().is_empty(), "not until three are liked");
        set_book(&conn, 6, Some(4), 1).unwrap();
        let names = |v: Vec<Suggestion>| -> Vec<String> {
            let mut n: Vec<String> = v.into_iter().map(|s| s.name).collect();
            n.sort();
            n
        };
        let got = suggestions(&conn, 0, 4).unwrap();
        let fp = got.iter().find(|s| s.name == "Fire Punch").unwrap();
        assert_eq!((fp.who.as_str(), fp.because.as_str()), ("Tatsuki Fujimoto", "Chainsaw Man"));
        // Not the deluxe edition of what you rated, not an unrelated series.
        assert_eq!(names(got), ["Fire Punch", "Look Back", "Planetes"]);
        skip(&conn, 2, 1).unwrap();
        crate::db::set_wanted(&conn, Some(3), None, true, 1).unwrap();
        assert_eq!(names(suggestions(&conn, 0, 4).unwrap()), ["Planetes"]);
        // Favorite creators need three rated books.
        let f = favorite_creators(&conn).unwrap();
        assert!(f.is_empty());
        set_book(&conn, 3, Some(4), 1).unwrap();
        let f = favorite_creators(&conn).unwrap();
        assert_eq!((f[0].name.as_str(), f[0].rated), ("Tatsuki Fujimoto", 3));
    }
}
