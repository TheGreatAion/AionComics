//! Reading sessions, and what they add up to. Every page turn the reader saves is logged against
//! its book; turns no more than `GAP_SECS` apart are one sitting. A sitting knows when it started
//! and ended, how many pages were read forward in it, and whether it reached the last page.
//! Nothing is guessed for the time before this was recorded.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::db::{self, ComicCard};

/// The longest pause that still counts as the same sitting.
pub const GAP_SECS: i64 = 600;
/// Time on the last page of a sitting, which no later turn measures.
const LAST_PAGE_SECS: i64 = 20;
/// A jump (the page box, the scrubber) isn't reading: one save counts for a spread at most.
const MAX_STEP: i64 = 2;

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS reading_sessions(
           id INTEGER PRIMARY KEY,
           comic_id INTEGER NOT NULL,
           started_at INTEGER NOT NULL,
           ended_at INTEGER NOT NULL,
           first_page INTEGER NOT NULL,
           last_page INTEGER NOT NULL,
           pages INTEGER NOT NULL,
           finished INTEGER NOT NULL DEFAULT 0
         );
         CREATE INDEX IF NOT EXISTS sessions_comic ON reading_sessions(comic_id, ended_at);
         CREATE INDEX IF NOT EXISTS sessions_ended ON reading_sessions(ended_at);",
    )?;
    Ok(())
}

/// Log a page the reader saved: carry on the book's sitting, or start a new one.
pub fn record(conn: &Connection, comic_id: i64, page: i64, page_count: i64, now: i64) -> Result<()> {
    let finished = page_count > 0 && page >= page_count - 1;
    let last: Option<(i64, i64, i64)> = conn
        .query_row(
            "SELECT id, ended_at, last_page FROM reading_sessions WHERE comic_id=?1 ORDER BY ended_at DESC LIMIT 1",
            [comic_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match last {
        Some((sid, ended, last_page)) if now - ended <= GAP_SECS => {
            let step = (page - last_page).clamp(0, MAX_STEP);
            conn.execute(
                "UPDATE reading_sessions SET ended_at=?2, last_page=?3, pages=pages+?4, finished=finished OR ?5 WHERE id=?1",
                params![sid, now, page, step, finished],
            )?;
        }
        _ => {
            conn.execute(
                "INSERT INTO reading_sessions(comic_id, started_at, ended_at, first_page, last_page, pages, finished)
                 VALUES(?1, ?2, ?2, ?3, ?3, 1, ?4)",
                params![comic_id, now, page, finished],
            )?;
        }
    }
    Ok(())
}

/// One book's reading, for the end card.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BookStats {
    pub pages: i64,
    pub sittings: i64,
    pub seconds: i64,
    pub first_at: Option<i64>,
    pub last_at: Option<i64>,
}

pub fn book(conn: &Connection, comic_id: i64) -> Result<BookStats> {
    Ok(conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(pages), 0), COUNT(*), COALESCE(SUM(ended_at - started_at + {LAST_PAGE_SECS}), 0),
                    MIN(started_at), MAX(ended_at)
             FROM reading_sessions WHERE comic_id=?1"
        ),
        [comic_id],
        |r| Ok(BookStats { pages: r.get(0)?, sittings: r.get(1)?, seconds: r.get(2)?, first_at: r.get(3)?, last_at: r.get(4)? }),
    )?)
}

#[derive(Serialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Period {
    pub pages: i64,
    pub seconds: i64,
    pub sittings: i64,
    pub finished: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    pub name: String,
    pub pages: i64,
    /// For a series: its id, and a book to show its cover.
    pub series_id: Option<i64>,
    pub cover_id: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    /// When the first sitting was recorded.
    pub since: Option<i64>,
    pub week: Period,
    pub month: Period,
    pub year: Period,
    pub all: Period,
    /// Pages read in each of the last 12 weeks, oldest first: (Monday as local days since 1970, pages).
    pub weeks: Vec<(i64, i64)>,
    /// Days in a row with some reading, up to today (or yesterday, before today's reading).
    pub streak: i64,
    pub best_streak: i64,
    pub series: Vec<Ranked>,
    pub creators: Vec<Ranked>,
    pub characters: Vec<Ranked>,
    pub finished: Vec<ComicCard>,
}

// Calendar days in the reader's time zone, from Howard Hinnant's civil-date algorithms.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Everything the stats page shows. `tz_secs` is the reader's offset from UTC, so "today" and
/// "this week" are their own.
pub fn stats(conn: &Connection, now: i64, tz_secs: i64) -> Result<Stats> {
    let day = |t: i64| (t + tz_secs).div_euclid(86_400);
    let start_of = |d: i64| d * 86_400 - tz_secs;
    let today = day(now);
    let (y, m, _) = civil_from_days(today);
    // Weeks start on Monday; 1970-01-01 was a Thursday.
    let monday = today - (today + 3).rem_euclid(7);
    let bounds = [start_of(monday), start_of(days_from_civil(y, m, 1)), start_of(days_from_civil(y, 1, 1)), i64::MIN];

    struct Row {
        comic: i64,
        started: i64,
        ended: i64,
        pages: i64,
        finished: bool,
    }
    let mut st = conn.prepare("SELECT comic_id, started_at, ended_at, pages, finished FROM reading_sessions ORDER BY ended_at")?;
    let rows: Vec<Row> = st
        .query_map([], |r| Ok(Row { comic: r.get(0)?, started: r.get(1)?, ended: r.get(2)?, pages: r.get(3)?, finished: r.get(4)? }))?
        .collect::<rusqlite::Result<_>>()?;

    let mut periods = vec![Period::default(); 4];
    let mut done: Vec<HashSet<i64>> = vec![HashSet::new(); 4];
    let mut days = HashSet::new();
    let first_week = monday - 7 * 11;
    let mut weeks: Vec<(i64, i64)> = (0..12).map(|k| (first_week + 7 * k, 0)).collect();
    let mut by_book: HashMap<i64, i64> = HashMap::new();
    for r in &rows {
        for (k, from) in bounds.iter().enumerate() {
            if r.ended >= *from {
                let p = &mut periods[k];
                p.pages += r.pages;
                p.seconds += r.ended - r.started + LAST_PAGE_SECS;
                p.sittings += 1;
                if r.finished {
                    done[k].insert(r.comic);
                }
            }
        }
        let d = day(r.started);
        days.insert(d);
        if d >= first_week {
            weeks[((d - first_week) / 7) as usize].1 += r.pages;
        }
        *by_book.entry(r.comic).or_default() += r.pages;
    }
    for (p, set) in periods.iter_mut().zip(&done) {
        p.finished = set.len() as i64;
    }

    // Streaks of days with reading.
    let mut sorted: Vec<i64> = days.iter().copied().collect();
    sorted.sort_unstable();
    let (mut best, mut run, mut prev) = (0, 0, i64::MIN);
    for d in &sorted {
        run = if *d == prev + 1 { run + 1 } else { 1 };
        best = best.max(run);
        prev = *d;
    }
    let mut streak = 0;
    let mut d = if days.contains(&today) { today } else { today - 1 };
    while days.contains(&d) {
        streak += 1;
        d -= 1;
    }

    // Who and what you've read the most of, weighted by pages.
    let mut series: HashMap<i64, (String, i64, i64)> = HashMap::new();
    let mut creators: HashMap<String, i64> = HashMap::new();
    let mut characters: HashMap<String, i64> = HashMap::new();
    let lines = |v: Option<String>| -> Vec<String> {
        v.unwrap_or_default().lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
    };
    let mut q = conn.prepare(
        "SELECT c.series_id, s.name, c.writer, c.penciller, c.characters FROM comics c JOIN series s ON s.id=c.series_id WHERE c.id=?1",
    )?;
    for (&comic, &pages) in &by_book {
        let Some((sid, sname, writer, penciller, chars)) = q
            .query_row([comic], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?))
            })
            .optional()?
        else {
            continue;
        };
        let e = series.entry(sid).or_insert((sname, 0, comic));
        e.1 += pages;
        let mut people: HashSet<String> = lines(writer).into_iter().collect();
        people.extend(lines(penciller));
        for p in people {
            *creators.entry(p).or_default() += pages;
        }
        for c in lines(chars) {
            *characters.entry(c).or_default() += pages;
        }
    }
    let top = |m: HashMap<String, i64>| -> Vec<Ranked> {
        let mut v: Vec<Ranked> = m.into_iter().map(|(name, pages)| Ranked { name, pages, series_id: None, cover_id: None }).collect();
        v.sort_by(|a, b| b.pages.cmp(&a.pages).then_with(|| a.name.cmp(&b.name)));
        v.truncate(8);
        v
    };
    let mut series: Vec<Ranked> = series
        .into_iter()
        .map(|(id, (name, pages, cover))| Ranked { name, pages, series_id: Some(id), cover_id: Some(cover) })
        .collect();
    series.sort_by(|a, b| b.pages.cmp(&a.pages).then_with(|| a.name.cmp(&b.name)));
    series.truncate(8);

    // Books finished, newest first.
    let mut fin = conn.prepare(
        "SELECT comic_id FROM reading_sessions WHERE finished=1 GROUP BY comic_id ORDER BY MAX(ended_at) DESC LIMIT 12",
    )?;
    let ids: Vec<i64> = fin.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    let finished = ids.into_iter().filter_map(|id| db::card_by_id(conn, id).ok()).collect();

    Ok(Stats {
        since: rows.first().map(|r| r.started),
        week: periods[0].clone(),
        month: periods[1].clone(),
        year: periods[2].clone(),
        all: periods[3].clone(),
        weeks,
        streak,
        best_streak: best,
        series,
        creators: top(creators),
        characters: top(characters),
        finished,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        for (y, m, d) in [(1970, 1, 1), (2000, 2, 29), (2026, 10, 26), (2027, 1, 1)] {
            assert_eq!(civil_from_days(days_from_civil(y, m, d)), (y, m, d));
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn sittings_and_totals() {
        let path = std::env::temp_dir().join(format!("aion-stats-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let conn = db::open(&path).unwrap();
        conn.execute_batch(
            "INSERT INTO collections(id, name, is_manga) VALUES(1, 'Marvel', 0);
             INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Fantastic Four', 'Marvel/FF');
             INSERT INTO comics(id, series_id, rel_path, file_name, label, writer, penciller, characters)
               VALUES(1, 1, 'Marvel/FF/1.cbz', '1.cbz', '#1', 'Stan Lee', 'Jack Kirby', 'The Thing
Doctor Doom');",
        )
        .unwrap();
        let t0 = days_from_civil(2026, 10, 26) * 86_400 + 9 * 3600; // a Monday, 9:00 UTC
        // One sitting: pages 0..=10 a minute apart.
        for p in 0..=10 {
            record(&conn, 1, p, 20, t0 + p * 60).unwrap();
        }
        // A jump with the page box isn't eleven pages of reading.
        record(&conn, 1, 18, 20, t0 + 11 * 60).unwrap();
        // The next day, a second sitting that reaches the end.
        record(&conn, 1, 19, 20, t0 + 86_400).unwrap();
        let b = book(&conn, 1).unwrap();
        assert_eq!(b.sittings, 2);
        assert_eq!(b.pages, 11 + 2 + 1);
        assert_eq!(b.seconds, 11 * 60 + LAST_PAGE_SECS + LAST_PAGE_SECS);
        let s = stats(&conn, t0 + 86_400 + 60, 0).unwrap();
        assert_eq!(s.week.sittings, 2);
        assert_eq!(s.week.finished, 1);
        assert_eq!((s.streak, s.best_streak), (2, 2));
        assert_eq!(s.weeks.last().unwrap().1, 14);
        // Nothing read the day before yesterday and not yet today: yesterday's run still counts.
        assert_eq!(stats(&conn, t0 + 2 * 86_400 + 3600, 0).unwrap().streak, 2);
        assert_eq!(stats(&conn, t0 + 3 * 86_400 + 3600, 0).unwrap().streak, 0);
        assert_eq!(s.series[0].name, "Fantastic Four");
        assert_eq!(s.creators.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Jack Kirby", "Stan Lee"]);
        assert_eq!(s.characters.len(), 2);
        assert_eq!(s.finished.len(), 1);
    }
}
