//! Comic Vine: finding a series, matching its books to issues, and filling in their metadata.
//!
//! Comic Vine asks for no more than about 200 requests per resource per hour and no bursts, so every
//! request goes through one throttle (at least a second apart, and under the hourly allowance) and
//! answers are cached on disk. Matching works at series level: one search, one issue list, and the
//! books are paired with issues by number and year. Credits, characters and story arcs need one
//! request per issue, so a background queue fills those in slowly and survives restarts.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::db::{self, Meta};

const BASE: &str = "https://comicvine.gamespot.com/api";
const AGENT: &str = concat!("AionComics/", env!("CARGO_PKG_VERSION"));
const MIN_GAP: Duration = Duration::from_millis(1100);
const PER_HOUR: usize = 190;

pub struct Client {
    key: Mutex<Option<String>>,
    cache_dir: PathBuf,
    last: Mutex<Option<Instant>>,
    /// Recent request times per resource, for the hourly allowance.
    used: Mutex<HashMap<String, VecDeque<Instant>>>,
    /// Wakes the background queue when there's new work or a new key.
    pub wake: Condvar,
    pub wake_lock: Mutex<()>,
}

impl Client {
    pub fn new(cache_dir: &Path, key: Option<String>) -> Self {
        let cache_dir = cache_dir.join("comicvine");
        let _ = std::fs::create_dir_all(&cache_dir);
        Self {
            key: Mutex::new(key.filter(|k| !k.trim().is_empty())),
            cache_dir,
            last: Mutex::new(None),
            used: Mutex::new(HashMap::new()),
            wake: Condvar::new(),
            wake_lock: Mutex::new(()),
        }
    }

    pub fn has_key(&self) -> bool {
        self.key.lock().unwrap().is_some()
    }

    pub fn set_key(&self, key: Option<String>) {
        *self.key.lock().unwrap() = key.filter(|k| !k.trim().is_empty());
        self.wake.notify_all();
    }

    /// How long until `resource` may be asked again (zero if now).
    fn wait_for(&self, resource: &str) -> Duration {
        let now = Instant::now();
        let mut used = self.used.lock().unwrap();
        let q = used.entry(resource.to_string()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(3600)) {
            q.pop_front();
        }
        let hourly = if q.len() >= PER_HOUR {
            Duration::from_secs(3600).saturating_sub(now.duration_since(*q.front().unwrap())) + Duration::from_secs(1)
        } else {
            Duration::ZERO
        };
        let gap = self.last.lock().unwrap().map_or(Duration::ZERO, |l| MIN_GAP.saturating_sub(now.duration_since(l)));
        hourly.max(gap)
    }

    /// GET a Comic Vine endpoint. `patient` requests (the background queue) wait out the hourly
    /// allowance; interactive ones fail fast with a clear message instead.
    pub fn get(&self, path: &str, query: &[(&str, String)], ttl_days: u64, patient: bool) -> Result<Value> {
        let resource = path.split('/').next().unwrap_or(path).to_string();
        let cache = self.cache_path(path, query);
        if let Ok(meta) = std::fs::metadata(&cache) {
            let fresh = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age.as_secs() < ttl_days * 86400);
            if fresh {
                if let Ok(v) = serde_json::from_slice::<Value>(&std::fs::read(&cache)?) {
                    return Ok(v);
                }
            }
        }
        let key = self.key.lock().unwrap().clone().ok_or_else(|| anyhow!("No API key yet"))?;
        loop {
            let wait = self.wait_for(&resource);
            if wait.is_zero() {
                break;
            }
            if !patient && wait > Duration::from_secs(5) {
                bail!("Comic Vine's hourly limit for {resource} is used up. Try again in {} minutes.", wait.as_secs() / 60 + 1);
            }
            std::thread::sleep(wait.min(Duration::from_secs(30)));
        }
        *self.last.lock().unwrap() = Some(Instant::now());
        self.used.lock().unwrap().entry(resource.clone()).or_default().push_back(Instant::now());

        let mut req = ureq::get(&format!("{BASE}/{path}/"))
            .timeout(Duration::from_secs(30))
            .set("User-Agent", AGENT)
            .query("api_key", &key)
            .query("format", "json");
        for (k, v) in query {
            req = req.query(k, v);
        }
        let body: Value = match req.call() {
            Ok(r) => serde_json::from_str(&r.into_string()?)?,
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                match code {
                    401 => bail!("Comic Vine didn't accept the API key."),
                    420 | 429 => bail!("Comic Vine says we're asking too often. It will continue later."),
                    _ => bail!("Comic Vine answered {code}: {}", text.chars().take(160).collect::<String>()),
                }
            }
            Err(e) => bail!("Couldn't reach Comic Vine: {e}"),
        };
        match body.get("status_code").and_then(|v| v.as_i64()) {
            Some(1) => {}
            Some(100) => bail!("Comic Vine didn't accept the API key."),
            Some(107) => bail!("Comic Vine's rate limit was reached. It will continue later."),
            _ => bail!("Comic Vine: {}", body.get("error").and_then(|v| v.as_str()).unwrap_or("unexpected answer")),
        }
        let _ = std::fs::write(&cache, serde_json::to_vec(&body)?);
        Ok(body)
    }

    fn cache_path(&self, path: &str, query: &[(&str, String)]) -> PathBuf {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut h);
        query.hash(&mut h);
        self.cache_dir.join(format!("{:016x}.json", h.finish()))
    }
}

// ---------- shapes sent to the interface ----------

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub id: String,
    pub name: String,
    pub start_year: Option<String>,
    pub publisher: Option<String>,
    pub count: i64,
    pub image: Option<String>,
    pub deck: Option<String>,
    /// How well it fits the series (0–1+); results come best first.
    pub score: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub number: Option<String>,
    pub name: Option<String>,
    pub cover_date: Option<String>,
    pub image: Option<String>,
    #[serde(skip)]
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub comic_id: i64,
    pub issue_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub comic_id: i64,
    pub label: String,
    pub section: String,
    pub issue_id: Option<String>,
    /// "sure" (number and year agree), "check" (number only), or "none".
    pub confidence: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuePlan {
    pub issues: Vec<Issue>,
    pub proposals: Vec<Proposal>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| match x {
        Value::String(t) if !t.trim().is_empty() => Some(t.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn image(v: &Value) -> Option<String> {
    let img = v.get("image")?;
    let url = s(img, "small_url").or_else(|| s(img, "thumb_url")).or_else(|| s(img, "medium_url"))?;
    // Comic Vine's "no cover" placeholder is a picture of the word "blank"; show nothing instead.
    (!url.ends_with("-blank.png")).then_some(url)
}

// ---------- names, numbers and ranking ----------

fn norm_name(n: &str) -> String {
    let lower = n.to_lowercase().replace('&', " and ");
    let cleaned: String = lower.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let words = if words.first() == Some(&"the") { &words[1..] } else { &words[..] };
    words.join(" ")
}

fn name_score(a: &str, b: &str) -> f64 {
    let (a, b) = (norm_name(a), norm_name(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let ta: std::collections::HashSet<&str> = a.split(' ').collect();
    let tb: std::collections::HashSet<&str> = b.split(' ').collect();
    let jaccard = ta.intersection(&tb).count() as f64 / ta.union(&tb).count() as f64;
    let contains = if a.contains(&b) || b.contains(&a) { 0.8 } else { 0.0 };
    (jaccard * 0.9).max(contains)
}

/// "012", "12.0" and "12" are the same issue number.
pub fn norm_number(n: &str) -> String {
    let t = n.trim().trim_start_matches('#').to_lowercase();
    let t = t.strip_suffix(".0").unwrap_or(&t).to_string();
    let trimmed = t.trim_start_matches('0');
    if trimmed.is_empty() || trimmed.starts_with('.') {
        if t.is_empty() { t } else { format!("0{trimmed}") }
    } else {
        trimmed.to_string()
    }
}

/// A book's number: what the metadata says, else the number in its label ("Vol. 12", "#3").
fn book_number(number: Option<String>, label: &str) -> Option<String> {
    number.filter(|n| !n.trim().is_empty()).or_else(|| {
        let digits: String = label
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        (!digits.is_empty()).then_some(digits)
    }).map(|n| norm_number(&n))
}

struct Local {
    name: String,
    books: i64,
    year: Option<i64>,
    publisher: Option<String>,
}

fn local_series(conn: &Connection, series_id: i64) -> Result<Local> {
    let name: String = conn.query_row("SELECT name FROM series WHERE id=?1", [series_id], |r| r.get(0))?;
    let books: i64 = conn.query_row("SELECT COUNT(*) FROM comics WHERE series_id=?1 AND section=''", [series_id], |r| r.get(0))?;
    let year: Option<i64> = conn.query_row(
        "SELECT MIN(CAST(substr(year,1,4) AS INTEGER)) FROM comics WHERE series_id=?1 AND year GLOB '[12][0-9][0-9][0-9]*'",
        [series_id],
        |r| r.get(0),
    )?;
    let publisher: Option<String> = conn
        .query_row(
            "SELECT publisher FROM comics WHERE series_id=?1 AND publisher IS NOT NULL AND publisher!=''
             GROUP BY publisher ORDER BY COUNT(*) DESC LIMIT 1",
            [series_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(Local { name, books, year, publisher })
}

/// The search term to start from: the series folder name without scene tags or a trailing year.
pub fn default_query(conn: &Connection, series_id: i64) -> Result<String> {
    let name: String = conn.query_row("SELECT name FROM series WHERE id=?1", [series_id], |r| r.get(0))?;
    let re = regex::Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").unwrap();
    Ok(re.replace_all(&name, "").trim().to_string())
}

const VOLUME_FIELDS: &str = "id,name,start_year,publisher,count_of_issues,image,deck";
/// How many search hits get their full record fetched.
const ENRICH: usize = 20;

fn volume(v: &Value) -> Option<Volume> {
    Some(Volume {
        id: s(v, "id")?,
        name: s(v, "name")?,
        start_year: s(v, "start_year"),
        publisher: v.get("publisher").and_then(|p| s(p, "name")),
        count: v.get("count_of_issues").and_then(|c| c.as_i64()).unwrap_or(0),
        image: image(v),
        deck: s(v, "deck"),
        score: 0.0,
    })
}

fn results(body: &Value) -> impl Iterator<Item = &Value> {
    body.get("results").and_then(|r| r.as_array()).into_iter().flatten()
}

/// Name first; then a start year near the books', enough issues to hold them, and the same publisher.
fn score(v: &Volume, local: &Local, query: &str) -> f64 {
    let mut score = name_score(&v.name, &local.name).max(name_score(&v.name, query));
    if let (Some(y), Some(ly)) = (v.start_year.as_deref().and_then(|y| y.parse::<i64>().ok()), local.year) {
        score += match (y - ly).abs() {
            0 => 0.3,
            1 => 0.2,
            2..=3 => 0.1,
            _ => 0.0,
        };
    }
    // A count of 0 means Comic Vine didn't say, not an empty series.
    if local.books > 0 && v.count > 0 {
        score += if v.count >= local.books { 0.15 } else if v.count * 2 < local.books { -0.2 } else { 0.0 };
    }
    if let (Some(p), Some(lp)) = (&v.publisher, &local.publisher) {
        if norm_name(lp).contains(&norm_name(p)) || norm_name(p).contains(&norm_name(lp)) {
            score += 0.15;
        }
    }
    score
}

fn rank(list: &mut [Volume], local: &Local, query: &str) {
    for v in list.iter_mut() {
        v.score = score(v, local, query);
    }
    list.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
}

/// Search Comic Vine for series, ranked by how well each fits this one.
pub fn search(client: &Client, conn: &Connection, series_id: i64, query: &str) -> Result<Vec<Volume>> {
    let local = local_series(conn, series_id)?;
    let body = client.get(
        "search",
        &[
            ("resources", "volume".into()),
            ("query", query.into()),
            ("limit", "50".into()),
            ("field_list", VOLUME_FIELDS.into()),
        ],
        1,
        false,
    )?;
    let mut out: Vec<Volume> = results(&body).filter_map(volume).collect();
    rank(&mut out, &local, query);

    // Search hits come back as stubs (placeholder cover, no issue count or publisher), so fetch the
    // full records of the likely ones in one go and rank again with them. Stubs are kept if that fails.
    let ids: Vec<&str> = out.iter().take(ENRICH).map(|v| v.id.as_str()).collect();
    if !ids.is_empty() {
        let full = client.get(
            "volumes",
            &[("filter", format!("id:{}", ids.join("|"))), ("field_list", VOLUME_FIELDS.into()), ("limit", "100".into())],
            1,
            false,
        );
        if let Ok(body) = full {
            let full: std::collections::HashMap<String, Volume> =
                results(&body).filter_map(volume).map(|v| (v.id.clone(), v)).collect();
            for v in out.iter_mut() {
                if let Some(f) = full.get(&v.id) {
                    *v = f.clone();
                }
            }
            rank(&mut out, &local, query);
        }
    }
    Ok(out)
}

/// Every issue of a Comic Vine series, in order, from a cached listing up to `ttl_days` old.
fn issues(client: &Client, volume_id: &str, ttl_days: u64) -> Result<Vec<Issue>> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let body = client.get(
            "issues",
            &[
                ("filter", format!("volume:{volume_id}")),
                ("field_list", "id,name,issue_number,cover_date,image,description,deck".into()),
                ("limit", "100".into()),
                ("offset", offset.to_string()),
                ("sort", "cover_date:asc".into()),
            ],
            ttl_days,
            false,
        )?;
        let page: Vec<Issue> = body
            .get("results")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| {
                Some(Issue {
                    id: s(v, "id")?,
                    number: s(v, "issue_number"),
                    name: s(v, "name"),
                    cover_date: s(v, "cover_date"),
                    image: image(v),
                    description: s(v, "description").or_else(|| s(v, "deck")),
                })
            })
            .collect();
        let total = body.get("number_of_total_results").and_then(|n| n.as_i64()).unwrap_or(0);
        offset += page.len() as i64;
        let empty = page.is_empty();
        out.extend(page);
        if empty || offset >= total {
            break;
        }
    }
    // Natural order by number (Comic Vine sorts numbers as text: 1, 10, 2…).
    out.sort_by(|a, b| {
        let key = |i: &Issue| i.number.as_deref().and_then(|n| n.parse::<f64>().ok()).unwrap_or(f64::MAX);
        key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(out)
}

/// A series' issues on Comic Vine, and a proposed issue for each book.
/// Books in sections ("Annuals", "Compendium") start unmatched: their numbers mean something else.
pub fn plan(client: &Client, conn: &Connection, series_id: i64, volume_id: &str) -> Result<IssuePlan> {
    let issues = issues(client, volume_id, 7)?;
    let books = series_books(conn, series_id, "")?;
    let mut taken = HashSet::new();
    let proposals = books
        .into_iter()
        .map(|b| {
            let (issue_id, confidence) = pair(&b, &issues, &mut taken);
            Proposal { comic_id: b.id, label: b.label, section: b.section, issue_id, confidence }
        })
        .collect();
    Ok(IssuePlan { issues, proposals })
}

struct Book {
    id: i64,
    label: String,
    section: String,
    number: Option<String>,
    year: Option<String>,
}

/// A series' books in order (sections last), optionally narrowed by a condition on `c`.
fn series_books(conn: &Connection, series_id: i64, and: &str) -> Result<Vec<Book>> {
    let mut st = conn.prepare(&format!(
        "SELECT c.id, c.label, c.section, c.number, c.year FROM comics c WHERE c.series_id=?1 {and}
         ORDER BY c.section!='', c.sort_key"
    ))?;
    let books = st
        .query_map([series_id], |r| Ok(Book { id: r.get(0)?, label: r.get(1)?, section: r.get(2)?, number: r.get(3)?, year: r.get(4)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(books)
}

/// The issue a book most likely is, by number (and by year when a number repeats), with how sure
/// that is: "sure" (number and year agree), "check" (number only), or "none". Each issue goes to
/// one book. Books in sections ("Annuals", "Compendium") stay unmatched: their numbers mean something else.
fn pair(b: &Book, issues: &[Issue], taken: &mut HashSet<String>) -> (Option<String>, &'static str) {
    if !b.section.is_empty() {
        return (None, "none");
    }
    let Some(n) = book_number(b.number.clone(), &b.label) else { return (None, "none") };
    let year = b.year.as_deref().and_then(|y| y.get(..4)).and_then(|y| y.parse::<i64>().ok());
    let same: Vec<&Issue> = issues
        .iter()
        .filter(|i| i.number.as_deref().map(norm_number).as_deref() == Some(n.as_str()) && !taken.contains(&i.id))
        .collect();
    let issue_year = |i: &Issue| i.cover_date.as_deref().and_then(|d| d.get(..4)).and_then(|y| y.parse::<i64>().ok());
    // Several issues with this number (reprints, a relaunch in the same series): take the nearest year.
    let best = same.iter().min_by_key(|i| match (issue_year(i), year) {
        (Some(a), Some(b)) => (a - b).abs(),
        _ => 0,
    });
    let Some(i) = best else { return (None, "none") };
    let close = match (issue_year(i), year) {
        (Some(a), Some(b)) => (a - b).abs() <= 1,
        _ => true,
    };
    taken.insert(i.id.clone());
    (Some(i.id.clone()), if close && same.len() == 1 { "sure" } else { "check" })
}

/// After the matcher: books you left unpaired stay that way, and automatic pairing leaves them alone.
pub fn remember_choices(conn: &Connection, pairs: &[Pair], now: i64) -> Result<()> {
    for p in pairs {
        if p.issue_id.is_none() {
            conn.execute(
                "INSERT OR REPLACE INTO cv_auto(comic_id, tried_at, settled) VALUES(?1, ?2, 1)",
                params![p.comic_id, now],
            )?;
        } else {
            conn.execute("DELETE FROM cv_auto WHERE comic_id=?1", [p.comic_id])?;
        }
    }
    Ok(())
}

/// A book with Comic Vine details (an issue id in its Comic Vine layer), as a SQL condition on `c`.
const HAS_CV: &str =
    "EXISTS (SELECT 1 FROM meta_layer v WHERE v.comic_id=c.id AND v.source='cv' AND json_extract(v.data, '$.cv_id') IS NOT NULL)";

/// How long a book that found no issue waits before trying again: Comic Vine adds new issues
/// around their release, so a book that's newer than the listing usually turns up within a day.
const RETRY_SECS: i64 = 86_400;

/// Comic Vine without being asked. Books whose ComicInfo.xml names their Comic Vine issue go
/// straight into the details queue; books new to a series that's already matched are paired by
/// number against the series' issues, but only when the pairing is certain. Returns how many
/// books were queued.
pub fn auto_match(client: &Client, conn: &Connection, now: i64) -> Result<usize> {
    if !client.has_key() {
        return Ok(0);
    }
    // The file says which issue it is.
    let mut n = conn.execute(
        &format!(
            "INSERT OR IGNORE INTO cv_queue(comic_id, issue_id, added_at)
             SELECT c.id, json_extract(f.data, '$.cv_id'), ?1 FROM meta_layer f JOIN comics c ON c.id=f.comic_id
             WHERE f.source='file' AND json_extract(f.data, '$.cv_id') GLOB '[0-9]*' AND NOT {HAS_CV}"
        ),
        [now],
    )?;
    // New books in matched series.
    let mut st = conn.prepare("SELECT series_id, volume_id FROM cv_series")?;
    let matched = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (series_id, volume_id) in matched {
        let fresh = series_books(
            conn,
            series_id,
            &format!(
                "AND c.section='' AND NOT {HAS_CV}
                 AND NOT EXISTS (SELECT 1 FROM cv_queue q WHERE q.comic_id=c.id)
                 AND NOT EXISTS (SELECT 1 FROM cv_auto a WHERE a.comic_id=c.id AND (a.settled=1 OR a.tried_at>{}))",
                now - RETRY_SECS
            ),
        )?;
        if fresh.is_empty() {
            continue;
        }
        // A day-old listing at most: the books are new, so their issues may be too.
        let issues = issues(client, &volume_id, 1)?;
        // Issues other books in the series already have aren't on offer.
        let mut taken: HashSet<String> = {
            let mut st = conn.prepare(&format!(
                "SELECT json_extract(v.data, '$.cv_id') FROM comics c JOIN meta_layer v ON v.comic_id=c.id AND v.source='cv'
                 WHERE c.series_id=?1 AND {HAS_CV}"
            ))?;
            let ids = st.query_map([series_id], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
            ids
        };
        let pairs: Vec<Pair> = fresh
            .iter()
            .filter_map(|b| match pair(b, &issues, &mut taken) {
                (Some(id), "sure") => Some(Pair { comic_id: b.id, issue_id: Some(id) }),
                _ => None,
            })
            .collect();
        if !pairs.is_empty() {
            n += apply(client, conn, series_id, &volume_id, &pairs, now)?;
        }
        for b in &fresh {
            conn.execute("INSERT OR REPLACE INTO cv_auto(comic_id, tried_at, settled) VALUES(?1, ?2, 0)", params![b.id, now])?;
        }
    }
    Ok(n)
}

/// Save a series match: the series' details, each book's issue details from the issue list, and a
/// queue of books whose credits, characters and story arcs are fetched in the background.
pub fn apply(client: &Client, conn: &Connection, series_id: i64, volume_id: &str, pairs: &[Pair], now: i64) -> Result<usize> {
    let vol = client.get(
        &format!("volume/4050-{volume_id}"),
        &[("field_list", "id,name,start_year,publisher,count_of_issues,image,description,deck".into())],
        7,
        false,
    )?;
    let v = vol.get("results").cloned().unwrap_or(Value::Null);
    let series = json!({
        "name": s(&v, "name"),
        "startYear": s(&v, "start_year"),
        "publisher": v.get("publisher").and_then(|p| s(p, "name")),
        "count": v.get("count_of_issues").and_then(|c| c.as_i64()),
        "description": s(&v, "description").or_else(|| s(&v, "deck")),
        "image": image(&v),
    });
    conn.execute(
        "INSERT OR REPLACE INTO cv_series(series_id, volume_id, data, updated_at) VALUES(?1, ?2, ?3, ?4)",
        params![series_id, volume_id, series.to_string(), now],
    )?;
    let by_id: HashMap<String, Issue> = issues(client, volume_id, 7)?.into_iter().map(|i| (i.id.clone(), i)).collect();
    let single = by_id.len() < 2;
    let mut n = 0;
    for p in pairs {
        let Some(issue) = p.issue_id.as_ref().and_then(|id| by_id.get(id)) else { continue };
        let mut m = Meta::new();
        let mut put = |k: &str, v: Option<String>| {
            if let Some(v) = v {
                m.insert(k.into(), Value::String(v));
            }
        };
        put("title", issue.name.clone().filter(|t| !crate::comicinfo::placeholder_title(t)));
        put("number", issue.number.clone());
        put("date", issue.cover_date.clone());
        put("year", issue.cover_date.as_deref().and_then(|d| d.get(..4)).map(str::to_string));
        // A one-shot often has its story only on the series page.
        put("synopsis", issue.description.clone().or_else(|| if single { series["description"].as_str().map(str::to_string) } else { None }));
        put("publisher", series["publisher"].as_str().map(str::to_string));
        put("volume", series["name"].as_str().map(str::to_string));
        put("cv_id", Some(issue.id.clone()));
        put("cv_volume_id", Some(volume_id.to_string()));
        db::merge_cv(conn, p.comic_id, &m, now)?;
        conn.execute(
            "INSERT OR REPLACE INTO cv_queue(comic_id, issue_id, added_at) VALUES(?1, ?2, ?3)",
            params![p.comic_id, issue.id, now],
        )?;
        n += 1;
    }
    client.wake.notify_all();
    Ok(n)
}

// ---------- background details ----------

fn names(v: &Value, k: &str) -> Option<String> {
    let list: Vec<String> = v.get(k)?.as_array()?.iter().filter_map(|x| s(x, "name")).collect();
    (!list.is_empty()).then(|| list.join("\n"))
}

/// One issue's credits, characters, teams, locations and story arc.
fn details(client: &Client, issue_id: &str) -> Result<Meta> {
    let body = client.get(
        &format!("issue/4000-{issue_id}"),
        &[(
            "field_list",
            "id,name,issue_number,cover_date,volume,description,deck,person_credits,character_credits,team_credits,location_credits,story_arc_credits"
                .into(),
        )],
        30,
        true,
    )?;
    let r = body.get("results").cloned().unwrap_or(Value::Null);
    // The issue itself, so a book known only by its issue (a link in its ComicInfo.xml) gets the
    // basics too. For a book matched with its series these are the same values again.
    if s(&r, "id").is_none() {
        bail!("Comic Vine has no issue {issue_id}");
    }
    let mut m = Meta::new();
    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            m.insert(k.into(), Value::String(v));
        }
    };
    put("title", s(&r, "name").filter(|t| !crate::comicinfo::placeholder_title(t)));
    put("number", s(&r, "issue_number"));
    put("date", s(&r, "cover_date"));
    put("year", s(&r, "cover_date").and_then(|d| d.get(..4).map(str::to_string)));
    put("cv_id", Some(issue_id.to_string()));
    if let Some(v) = r.get("volume") {
        put("volume", s(v, "name"));
        put("cv_volume_id", s(v, "id"));
    }
    let mut roles: HashMap<&str, Vec<String>> = HashMap::new();
    for p in r.get("person_credits").and_then(|x| x.as_array()).into_iter().flatten() {
        let Some(name) = s(p, "name") else { continue };
        for role in s(p, "role").unwrap_or_default().split(',') {
            let field = match role.trim() {
                "writer" => "writer",
                "penciler" | "penciller" | "artist" => "penciller",
                "inker" => "inker",
                "colorist" => "colorist",
                "letterer" => "letterer",
                "cover" => "cover_artist",
                _ => continue,
            };
            let list = roles.entry(field).or_default();
            if !list.contains(&name) {
                list.push(name.clone());
            }
        }
    }
    for (k, v) in roles {
        m.insert(k.into(), Value::String(v.join("\n")));
    }
    for (field, key) in [("characters", "character_credits"), ("teams", "team_credits"), ("locations", "location_credits")] {
        if let Some(v) = names(&r, key) {
            m.insert(field.into(), Value::String(v));
        }
    }
    if let Some(d) = s(&r, "description").or_else(|| s(&r, "deck")) {
        m.insert("synopsis".into(), Value::String(d));
    }
    // The first story arc, and where this issue falls in it ("part 3 of 7").
    if let Some(arc) = r.get("story_arc_credits").and_then(|a| a.as_array()).and_then(|a| a.first()) {
        if let (Some(arc_id), Some(arc_name)) = (s(arc, "id"), s(arc, "name")) {
            m.insert("story_arc".into(), Value::String(arc_name));
            if let Ok(a) = client.get(&format!("story_arc/4045-{arc_id}"), &[("field_list", "id,issues".into())], 30, true) {
                if let Some(list) = a.get("results").and_then(|x| x.get("issues")).and_then(|x| x.as_array()) {
                    if let Some(pos) = list.iter().position(|i| s(i, "id").as_deref() == Some(issue_id)) {
                        m.insert("arc_number".into(), Value::String((pos + 1).to_string()));
                        m.insert("arc_count".into(), Value::String(list.len().to_string()));
                    }
                }
            }
        }
    }
    Ok(m)
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub has_key: bool,
    pub queued: i64,
    pub last_error: Option<String>,
}

pub fn status(client: &Client, conn: &Connection) -> Result<Status> {
    Ok(Status {
        has_key: client.has_key(),
        queued: conn.query_row("SELECT COUNT(*) FROM cv_queue", [], |r| r.get(0))?,
        last_error: conn
            .query_row("SELECT last_error FROM cv_queue WHERE last_error IS NOT NULL ORDER BY added_at DESC LIMIT 1", [], |r| r.get(0))
            .optional()?,
    })
}

/// Work through the queue forever: one book at a time, as fast as Comic Vine allows.
/// `changed` is told which book just got its details.
pub fn run_queue(client: &Client, db_path: &Path, changed: impl Fn(i64, i64)) {
    let Ok(conn) = db::open(db_path) else { return };
    loop {
        let next: Option<(i64, String, i64)> = if client.has_key() {
            conn.query_row(
                "SELECT comic_id, issue_id, tries FROM cv_queue WHERE tries < 3 ORDER BY tries, added_at LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .unwrap_or(None)
        } else {
            None
        };
        let Some((comic_id, issue_id, tries)) = next else {
            // Nothing queued: look for books that can be matched without asking.
            if client.has_key() {
                match auto_match(client, &conn, crate::scan::now()) {
                    Ok(n) if n > 0 => {
                        changed(0, n as i64);
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("comic vine auto match: {e}"),
                }
            }
            let g = client.wake_lock.lock().unwrap();
            let _ = client.wake.wait_timeout(g, Duration::from_secs(60));
            continue;
        };
        let now = crate::scan::now();
        match details(client, &issue_id) {
            Ok(m) => {
                let _ = db::merge_cv(&conn, comic_id, &m, now);
                let _ = conn.execute("DELETE FROM cv_queue WHERE comic_id=?1", [comic_id]);
                let left: i64 = conn.query_row("SELECT COUNT(*) FROM cv_queue WHERE tries < 3", [], |r| r.get(0)).unwrap_or(0);
                changed(comic_id, left);
            }
            Err(e) => {
                let _ = conn.execute(
                    "UPDATE cv_queue SET tries=?2, last_error=?3 WHERE comic_id=?1",
                    params![comic_id, tries + 1, e.to_string()],
                );
                // A key or network problem affects everything: pause before trying again.
                std::thread::sleep(Duration::from_secs(if tries == 0 { 10 } else { 60 }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_stubs() {
        // What /search returns today for "Absolute Batman": every hit a stub.
        let stub = serde_json::json!({
            "id": 160294, "name": "Absolute Batman", "start_year": "2024", "count_of_issues": 0, "publisher": null,
            "image": { "small_url": "https://comicvine.gamespot.com/a/uploads/scale_small/11122/111222211/6373148-blank.png" }
        });
        let v = volume(&stub).unwrap();
        assert_eq!(v.image, None);
        let local = Local { name: "Absolute Batman".into(), books: 17, year: Some(2024), publisher: Some("DC Comics".into()) };
        // A missing count costs nothing; the full record's count and publisher then lift it.
        assert!((score(&v, &local, "Absolute Batman") - 1.3).abs() < 1e-9);
        let full = Volume { count: 20, publisher: Some("DC Comics".into()), ..v };
        assert!((score(&full, &local, "Absolute Batman") - 1.6).abs() < 1e-9);
    }

    fn library(name: &str) -> (Connection, Client) {
        let dir = std::env::temp_dir().join(format!("aion-cv-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let conn = db::open(&dir.join("t.db")).unwrap();
        conn.execute_batch(
            "INSERT INTO collections(id, name, is_manga) VALUES(1, 'DC', 0);
             INSERT INTO series(id, collection_id, name, rel_path) VALUES(1, 1, 'Absolute Batman', 'DC/Absolute Batman'),
                                                                     (2, 1, 'Loose', 'DC/Loose');
             INSERT INTO cv_series(series_id, volume_id, data, updated_at) VALUES(1, '777', '{}', 0);
             INSERT INTO comics(id, series_id, section, rel_path, file_name, label, year) VALUES
               (1, 1, '', 'DC/Absolute Batman/1.cbz', '1.cbz', '#1', '2024'),
               (2, 1, '', 'DC/Absolute Batman/2.cbz', '2.cbz', '#2', '2024'),
               (3, 1, '', 'DC/Absolute Batman/9.cbz', '9.cbz', '#9', '2025'),
               (4, 1, 'Annuals', 'DC/Absolute Batman/Annuals/1.cbz', 'a1.cbz', '#1', '2025'),
               (5, 2, '', 'DC/Loose/x.cbz', 'x.cbz', 'x', NULL);
             INSERT INTO meta_layer(comic_id, source, data, updated_at) VALUES
               (1, 'cv', '{\"cv_id\":\"1001\"}', 0),
               (5, 'file', '{\"cv_id\":\"5555\",\"title\":\"From the file\"}', 0);",
        )
        .unwrap();
        let client = Client::new(&dir, Some("test".into()));
        // What Comic Vine would answer, already in the cache: no network in tests.
        let put = |path: &str, query: &[(&str, String)], body: Value| {
            std::fs::write(client.cache_path(path, query), body.to_string()).unwrap();
        };
        put(
            "issues",
            &[
                ("filter", "volume:777".into()),
                ("field_list", "id,name,issue_number,cover_date,image,description,deck".into()),
                ("limit", "100".into()),
                ("offset", "0".into()),
                ("sort", "cover_date:asc".into()),
            ],
            json!({ "number_of_total_results": 2, "results": [
                { "id": 1001, "issue_number": "1", "cover_date": "2024-12-01", "name": "The Zoo" },
                { "id": 1002, "issue_number": "2", "cover_date": "2024-12-20", "name": "The Zoo, Part Two" },
            ]}),
        );
        put(
            "volume/4050-777",
            &[("field_list", "id,name,start_year,publisher,count_of_issues,image,description,deck".into())],
            json!({ "results": { "id": 777, "name": "Absolute Batman", "publisher": { "name": "DC Comics" } } }),
        );
        (conn, client)
    }

    fn queued(conn: &Connection) -> Vec<(i64, String)> {
        let mut st = conn.prepare("SELECT comic_id, issue_id FROM cv_queue ORDER BY comic_id").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    #[test]
    fn matches_new_books_by_itself() {
        let (conn, client) = library("auto");
        let n = auto_match(&client, &conn, 1_000_000).unwrap();
        // #2 pairs with its issue; the file's own link queues book 5; #9 has no issue yet, and the
        // annual's number means something else.
        assert_eq!(queued(&conn), vec![(2, "1002".into()), (5, "5555".into())]);
        assert_eq!(n, 2);
        let (label, title): (String, Option<String>) =
            conn.query_row("SELECT label, title FROM comics WHERE id=2", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((label.as_str(), title.as_deref()), ("#2", Some("The Zoo, Part Two")));
        // Nothing twice; #9 waits a day before looking again.
        assert_eq!(auto_match(&client, &conn, 1_000_100).unwrap(), 0);
        let tried: i64 = conn.query_row("SELECT COUNT(*) FROM cv_auto WHERE comic_id=3 AND settled=0", [], |r| r.get(0)).unwrap();
        assert_eq!(tried, 1);
        assert_eq!(queued(&conn).len(), 2);
    }

    #[test]
    fn collected_editions_keep_their_style() {
        let (conn, _) = library("trades");
        conn.execute("UPDATE comics SET label='Vol. 1' WHERE id=5", []).unwrap();
        let mut m = Meta::new();
        m.insert("number".into(), json!("1"));
        db::merge_cv(&conn, 5, &m, 0).unwrap();
        db::merge_cv(&conn, 2, &m, 0).unwrap();
        let label = |id: i64| conn.query_row("SELECT label FROM comics WHERE id=?1", [id], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!((label(5).as_str(), label(2).as_str()), ("Vol. 1", "#1"));
    }

    #[test]
    fn leaves_your_choices_alone() {
        let (conn, client) = library("choices");
        // In the matcher you left #2 unpaired.
        remember_choices(&conn, &[Pair { comic_id: 2, issue_id: None }], 0).unwrap();
        auto_match(&client, &conn, 1_000_000).unwrap();
        assert_eq!(queued(&conn), vec![(5, "5555".into())]);
        // Books already unpaired in a matched series when this arrived count as your call too.
        conn.execute_batch("DROP TABLE cv_auto; DELETE FROM cv_queue;").unwrap();
        crate::db::migrate_for_tests(&conn).unwrap();
        let settled: Vec<i64> = {
            let mut st = conn.prepare("SELECT comic_id FROM cv_auto WHERE settled=1 ORDER BY comic_id").unwrap();
            st.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(settled, vec![2, 3, 4]);
    }

    #[test]
    fn numbers_and_names() {
        assert_eq!(norm_number("012"), "12");
        assert_eq!(norm_number("12.0"), "12");
        assert_eq!(norm_number("0"), "0");
        assert_eq!(norm_number("0.5"), "0.5");
        assert_eq!(book_number(None, "Vol. 12"), Some("12".into()));
        assert_eq!(book_number(Some("003".into()), "#3"), Some("3".into()));
        assert_eq!(book_number(None, "Compendium"), None);
        assert_eq!(name_score("The Black Beetle", "Black Beetle"), 1.0);
        assert!(name_score("Vinland Saga", "Vinland Saga Omnibus") >= 0.8);
        assert!(name_score("Saga", "Swamp Thing") < 0.3);
    }
}
