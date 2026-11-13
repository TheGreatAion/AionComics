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

    /// Drop a cached answer, so the next ask goes to Comic Vine.
    pub fn forget(&self, path: &str, query: &[(&str, String)]) {
        let _ = std::fs::remove_file(self.cache_path(path, query));
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
    /// The full description, as Comic Vine writes it (HTML).
    pub description: Option<String>,
    /// The language of an edition outside English ("Spanish"), or "UK edition" for a British
    /// reprint, when it can be told (see `cvextra::language`); None otherwise.
    pub language: Option<String>,
    /// Its first and last issues: "1 · Episode One: The Black Hand".
    pub first_issue: Option<String>,
    pub last_issue: Option<String>,
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
    /// The day it went on sale (the cover date runs a couple of months ahead).
    pub store_date: Option<String>,
    pub image: Option<String>,
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

/// How close a Comic Vine series' name (`a`) is to yours (`b`), 0 to 1. Mostly how much of your
/// name it has (so plain "X-Men" isn't a near miss for "X-Men: The Dark Phoenix Saga"), a little
/// how much it adds, a word off by one letter still counting ("Johnathan"). A subtitle of yours
/// on its own counts too, a little less ("The Sworn Sword" out of "The Hedge Knight II - The
/// Sworn Sword").
fn name_score(a: &str, b: &str) -> f64 {
    let whole = closeness(a, b);
    let parts: Vec<&str> = b.split([':', '-', '–']).map(str::trim).filter(|p| p.split_whitespace().count() >= 2).collect();
    if parts.len() < 2 {
        return whole;
    }
    parts.iter().map(|p| closeness(a, p) * 0.9).fold(whole, f64::max)
}

fn closeness(a: &str, b: &str) -> f64 {
    let (a, b) = (norm_name(a), norm_name(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    let (ta, tb): (Vec<&str>, Vec<&str>) = (a.split(' ').collect(), b.split(' ').collect());
    let near = |x: &str, y: &str| x == y || (x.len() >= 5 && y.len() >= 5 && one_off(x, y));
    // Words for a format or edition say little about which series it is.
    let weight = |w: &str| if FORMAT_WORDS.contains(&w) { 0.3 } else { 1.0 };
    let share = |from: &[&str], into: &[&str]| {
        let total: f64 = from.iter().map(|w| weight(w)).sum();
        let found: f64 = from.iter().filter(|w| into.iter().any(|x| near(x, w))).map(|w| weight(w)).sum();
        found / total
    };
    0.68 * share(&tb, &ta) + 0.27 * share(&ta, &tb)
}

const FORMAT_WORDS: &[&str] = &[
    "the", "a", "of", "and", "by", "vol", "volume", "book", "omnibus", "deluxe", "edition", "compendium", "collection", "complete",
    "tpb", "hc", "hardcover", "paperback", "library", "digital", "colored", "color", "epic",
];

/// Two words one letter apart: one added, dropped or changed.
fn one_off(x: &str, y: &str) -> bool {
    let (x, y): (Vec<char>, Vec<char>) = (x.chars().collect(), y.chars().collect());
    let (short, long) = if x.len() <= y.len() { (&x, &y) } else { (&y, &x) };
    match long.len() - short.len() {
        0 => short.iter().zip(long.iter()).filter(|(a, b)| a != b).count() == 1,
        1 => {
            let i = short.iter().zip(long.iter()).take_while(|(a, b)| a == b).count();
            short[i..] == long[i + 1..]
        }
        _ => false,
    }
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
    /// Comics only: whether the books here are collected editions (true) or single issues (false),
    /// when most of them are one or the other.
    collected: Option<bool>,
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
    let kinds: Vec<&str> = series_books(conn, series_id, "AND c.section=''")?.iter().filter(|b| !b.manga).map(|b| b.kind).collect();
    let trades = kinds.iter().filter(|k| matches!(**k, "volume" | "collection" | "deluxe" | "omnibus")).count();
    let issues = kinds.iter().filter(|k| **k == "issue").count();
    let collected = if trades * 3 >= kinds.len() * 2 && trades > 0 {
        Some(true)
    } else if issues * 3 >= kinds.len() * 2 && issues > 0 {
        Some(false)
    } else {
        None
    };
    Ok(Local { name, books, year, publisher, collected })
}

/// Words in a Comic Vine series' name, or its first issue's, that say it's collected editions.
static COLLECTED_SERIES: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)\b(tpb|hc|trade paperback|hardcover|omnibus|compendium|deluxe|collection|library edition|absolute edition|epic collection|vol(ume)?\.? ?\d|book (one|two|three|\d))\b").unwrap()
});

/// Whether a Comic Vine series looks like collected editions rather than single issues.
fn looks_collected(v: &Volume) -> bool {
    COLLECTED_SERIES.is_match(&v.name) || v.first_issue.as_deref().is_some_and(|f| COLLECTED_SERIES.is_match(f) || COLLECTED_NAME.is_match(f.split(" · ").nth(1).unwrap_or("")))
}

/// The search term to start from: the series folder name without scene tags or a trailing year.
pub fn default_query(conn: &Connection, series_id: i64) -> Result<String> {
    let name: String = conn.query_row("SELECT name FROM series WHERE id=?1", [series_id], |r| r.get(0))?;
    let re = regex::Regex::new(r"\s*[\(\[][^\)\]]*[\)\]]").unwrap();
    Ok(re.replace_all(&name, "").trim().to_string())
}

const VOLUME_FIELDS: &str = "id,name,start_year,publisher,count_of_issues,image,deck,description,first_issue,last_issue";
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
        language: crate::cvextra::language(v.get("publisher").and_then(|p| s(p, "name")).as_deref(), s(v, "description").as_deref()),
        description: s(v, "description"),
        first_issue: v.get("first_issue").and_then(issue_line),
        last_issue: v.get("last_issue").and_then(issue_line),
        score: 0.0,
    })
}

/// A first or last issue as "1 · Episode One: The Black Hand" (just "1" when it has no title).
fn issue_line(i: &Value) -> Option<String> {
    let n = s(i, "issue_number")?;
    Some(match s(i, "name") {
        Some(t) => format!("{} · {t}", norm_number(&n)),
        None => norm_number(&n),
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
    // Your books are the English editions; one in another language is rarely the one you want.
    if v.language.is_some() {
        score -= 0.6;
    }
    // Trades for trades, single issues for single issues.
    if let Some(collected) = local.collected {
        score += if looks_collected(v) == collected { 0.12 } else { -0.12 };
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
                ("field_list", "id,name,issue_number,cover_date,store_date,image,description,deck".into()),
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
                    store_date: s(v, "store_date"),
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

/// A Comic Vine series' issues, for showing it while you choose: from the listing matching uses
/// too (kept a week), so looking first costs nothing later.
pub fn volume_issues(client: &Client, volume_id: &str) -> Result<Vec<Issue>> {
    issues(client, volume_id, 7)
}

/// A series' issues on Comic Vine, and a proposed issue for each book. `fresh` asks for the
/// listing afresh, so issues out since the last look are there. Books already paired with this Comic Vine
/// series keep their issue, and books you left unpaired stay that way. Books in sections
/// ("Annuals", "Compendium") and books of another format start unmatched.
pub fn plan(client: &Client, conn: &Connection, series_id: i64, volume_id: &str, fresh: bool) -> Result<IssuePlan> {
    let issues = issues(client, volume_id, if fresh { 0 } else { 7 })?;
    let books = series_books(conn, series_id, "")?;
    let holds = holds(&books, &issues);
    let listed: HashSet<&str> = issues.iter().map(|i| i.id.as_str()).collect();
    let kept = |b: &Book| {
        b.cv_id
            .as_deref()
            .filter(|id| b.cv_volume_id.as_deref() == Some(volume_id) && listed.contains(id) && holds(b) == Some("sure"))
            .map(str::to_string)
    };
    let mut taken: HashSet<String> = books.iter().filter_map(kept).collect();
    let proposals = books
        .iter()
        .map(|b| {
            let (issue_id, confidence) = match (kept(b), holds(b)) {
                (Some(id), _) => (Some(id), "sure"),
                (None, None) => (None, "none"),
                (None, _) if b.settled => (None, "none"),
                (None, Some(cap)) => match pair(b, &issues, &mut taken) {
                    (Some(id), "sure") => (Some(id), cap),
                    other => other,
                },
            };
            Proposal { comic_id: b.id, label: b.label.clone(), section: b.section.clone(), issue_id, confidence }
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
    /// Its format (see `editions`): issue, volume, annual…
    kind: &'static str,
    manga: bool,
    /// The issue and series its Comic Vine details are from, if it has any.
    cv_id: Option<String>,
    cv_volume_id: Option<String>,
    /// You left it unpaired in the matcher.
    settled: bool,
}

/// A series' books in order (sections last), optionally narrowed by a condition on `c`.
fn series_books(conn: &Connection, series_id: i64, and: &str) -> Result<Vec<Book>> {
    let mut st = conn.prepare(&format!(
        "SELECT c.id, c.label, c.section, c.number, c.year, c.file_name, COALESCE(c.num_pages, 0), o.kind, COALESCE(COALESCE(s.reading, col.reading)='manga', col.is_manga),
           json_extract(v.data, '$.cv_id'), json_extract(v.data, '$.cv_volume_id'),
           EXISTS (SELECT 1 FROM cv_auto a WHERE a.comic_id=c.id AND a.settled=1)
         FROM comics c JOIN series s ON s.id=c.series_id JOIN collections col ON col.id=s.collection_id
         LEFT JOIN edition_override o ON o.comic_id=c.id
         LEFT JOIN meta_layer v ON v.comic_id=c.id AND v.source='cv'
         WHERE c.series_id=?1 {and}
         ORDER BY c.section!='', c.sort_key"
    ))?;
    let books = st
        .query_map([series_id], |r| {
            let (section, file, pages, own, manga): (String, String, i64, Option<String>, bool) =
                (r.get(2)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?);
            let kind = own
                .as_deref()
                .and_then(|k| crate::editions::KINDS.iter().copied().find(|x| *x == k))
                .unwrap_or_else(|| crate::editions::kind(manga, &section, &file, pages));
            Ok(Book {
                id: r.get(0)?,
                label: r.get(1)?,
                section,
                number: r.get(3)?,
                year: r.get(4)?,
                kind,
                manga,
                cv_id: r.get(9)?,
                cv_volume_id: r.get(10)?,
                settled: r.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(books)
}

/// Which books a Comic Vine series can hold, and how sure that is: `None` for a book it can't,
/// "check" for one it may. A manga's issues on Comic Vine are its volumes. A comic's are single
/// issues, unless that Comic Vine series is itself collected editions (most of its issues called
/// "Volume 1: …"). So "Vol. 1" of a trade doesn't get the details of issue #1, nor "Annual 2"
/// those of #2. When the books here are nothing but collected editions, they may be what the
/// Comic Vine series holds, or it may be the single issues they collect: worth a check.
fn holds(books: &[Book], issues: &[Issue]) -> impl Fn(&Book) -> Option<&'static str> + use<> {
    let collected_kind = |k: &str| matches!(k, "volume" | "collection" | "deluxe" | "omnibus");
    let named = issues
        .iter()
        .filter(|i| i.name.as_deref().is_some_and(|n| COLLECTED_NAME.is_match(n)))
        .count();
    let main: Vec<&Book> = books.iter().filter(|b| b.section.is_empty()).collect();
    let collected = named * 2 > issues.len();
    let all_collected = !main.is_empty() && main.iter().all(|b| collected_kind(b.kind));
    move |b: &Book| match (collected_kind(b.kind), b.kind == "issue") {
        (true, _) if b.manga || collected => Some("sure"),
        (true, _) if all_collected => Some("check"),
        (_, true) if !b.manga && !collected => Some("sure"),
        _ => None,
    }
}

static COLLECTED_NAME: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^\s*(?:vol(?:ume)?\.?|book|tpb|part)\s*(?:\d|(?:one|two|three|four|five|six|seven|eight|nine|ten)\b)").unwrap()
    });

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
        let holds = holds(&series_books(conn, series_id, "")?, &issues);
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
            .filter(|b| holds(b) == Some("sure"))
            .filter_map(|b| match pair(b, &issues, &mut taken) {
                (Some(id), "sure") => Some(Pair { comic_id: b.id, issue_id: Some(id) }),
                _ => None,
            })
            .collect();
        if !pairs.is_empty() {
            n += apply(client, conn, series_id, &volume_id, &pairs, true, false, now)?;
        }
        for b in &fresh {
            conn.execute("INSERT OR REPLACE INTO cv_auto(comic_id, tried_at, settled) VALUES(?1, ?2, 0)", params![b.id, now])?;
        }
    }
    Ok(n)
}

/// How often a series is looked at for new issues.
const NEW_EVERY_SECS: i64 = 3 * 86_400;
/// Series read in this long, or on Want to read, are the ones looked at.
const FOLLOWED_SECS: i64 = 365 * 86_400;

/// Look at the next series due for new issues on Comic Vine: one you've read in the last year or
/// want to read, matched to a Comic Vine series. Returns whether what's new for it changed, or
/// None when no series is due.
pub fn check_new(client: &Client, conn: &Connection, now: i64) -> Result<Option<bool>> {
    let due: Option<(i64, String)> = conn
        .query_row(
            "SELECT s.series_id, s.volume_id FROM cv_series s LEFT JOIN cv_new n ON n.series_id=s.series_id
             WHERE (n.checked_at IS NULL OR n.checked_at<?1 OR n.checked_at<s.updated_at)
               AND (EXISTS (SELECT 1 FROM comics c WHERE c.series_id=s.series_id AND (c.read=1 OR c.current_page>0) AND c.last_read_at>?2)
                    OR EXISTS (SELECT 1 FROM want_series w WHERE w.series_id=s.series_id))
             ORDER BY n.checked_at IS NOT NULL, n.checked_at LIMIT 1",
            params![now - NEW_EVERY_SECS, now - FOLLOWED_SECS],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((series_id, volume_id)) = due else { return Ok(None) };
    let found = issues(client, &volume_id, 1)?;
    let numbers = new_numbers(&series_books(conn, series_id, "")?, &found, &date_after(now, 60));
    let json = serde_json::to_string(&numbers)?;
    let before: Option<String> = conn.query_row("SELECT numbers FROM cv_new WHERE series_id=?1", [series_id], |r| r.get(0)).optional()?;
    conn.execute("INSERT OR REPLACE INTO cv_new(series_id, numbers, checked_at) VALUES(?1, ?2, ?3)", params![series_id, json, now])?;
    Ok(Some(before.as_deref() != Some(json.as_str())))
}

/// A date as Comic Vine writes one ("2026-11-11"), `days` after `now` (seconds since the epoch).
fn date_after(now: i64, days: i64) -> String {
    // Days since 1970-01-01 to a calendar date (the civil_from_days method).
    let z = now.div_euclid(86_400) + days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The numbers of the issues past the last one here that are out by `until`. Only books of the
/// kind the Comic Vine series holds count (single issues, or a manga's volumes), and an issue
/// already paired with a book here isn't new. A cover date runs about two months ahead of the day
/// an issue goes on sale, so `until` is two months from today.
fn new_numbers(books: &[Book], issues: &[Issue], until: &str) -> Vec<String> {
    let holds = holds(books, issues);
    let held: Vec<&Book> = books.iter().filter(|b| b.section.is_empty() && holds(b) == Some("sure")).collect();
    let value = |b: &Book| book_number(b.number.clone(), &b.label).and_then(|n| n.parse::<f64>().ok());
    let Some(last) = held.iter().filter_map(|b| value(b)).reduce(f64::max) else { return Vec::new() };
    let paired: HashSet<&str> = held.iter().filter_map(|b| b.cv_id.as_deref()).collect();
    issues
        .iter()
        .filter(|i| !paired.contains(i.id.as_str()))
        .filter(|i| i.cover_date.as_deref().is_none_or(|d| d <= until))
        .filter_map(|i| {
            let n = norm_number(i.number.as_deref()?);
            (n.parse::<f64>().ok()? > last).then_some(n)
        })
        .collect()
}

/// Save matched books: the series' details, each book's issue details from the issue list, and a
/// queue of books whose credits, characters and story arcs are fetched in the background.
/// `link_series` also makes `volume_id` the series' own Comic Vine series; a single book matched on
/// its own (a trade among issues) leaves that alone. `fresh` (you asked) gets everything from
/// Comic Vine again rather than from what's been kept.
///
/// A book paired with a different issue than before starts over, so nothing of the old issue (its
/// story arc, say) stays behind. A book given no issue loses the details it had from this Comic
/// Vine series, or from the one the series was matched to before.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    client: &Client,
    conn: &Connection,
    series_id: i64,
    volume_id: &str,
    pairs: &[Pair],
    link_series: bool,
    fresh: bool,
    now: i64,
) -> Result<usize> {
    let vol = client.get(
        &format!("volume/4050-{volume_id}"),
        &[("field_list", "id,name,start_year,publisher,count_of_issues,image,description,deck".into())],
        if fresh { 0 } else { 7 },
        false,
    )?;
    let before: Option<String> =
        conn.query_row("SELECT volume_id FROM cv_series WHERE series_id=?1", [series_id], |r| r.get(0)).optional()?;
    let v = vol.get("results").cloned().unwrap_or(Value::Null);
    let series = json!({
        "name": s(&v, "name"),
        "startYear": s(&v, "start_year"),
        "publisher": v.get("publisher").and_then(|p| s(p, "name")),
        "count": v.get("count_of_issues").and_then(|c| c.as_i64()),
        "description": s(&v, "description").or_else(|| s(&v, "deck")),
        "image": image(&v),
    });
    if link_series {
        conn.execute(
            "INSERT OR REPLACE INTO cv_series(series_id, volume_id, data, updated_at) VALUES(?1, ?2, ?3, ?4)",
            params![series_id, volume_id, series.to_string(), now],
        )?;
    }
    let by_id: HashMap<String, Issue> = issues(client, volume_id, 7)?.into_iter().map(|i| (i.id.clone(), i)).collect();
    let single = by_id.len() < 2;
    let mut n = 0;
    for p in pairs {
        let had = db::read_layer(conn, p.comic_id, "cv")?;
        let had_id = had.get("cv_id").and_then(|v| v.as_str()).map(str::to_string);
        let had_volume = had.get("cv_volume_id").and_then(|v| v.as_str());
        let Some(issue) = p.issue_id.as_ref().and_then(|id| by_id.get(id)) else {
            if p.issue_id.is_none() && had_volume.is_some_and(|v| v == volume_id || Some(v) == before.as_deref()) {
                db::write_layer(conn, p.comic_id, "cv", &Meta::new(), now)?;
                conn.execute("DELETE FROM cv_queue WHERE comic_id=?1", [p.comic_id])?;
                db::apply_meta(conn, Some(&[p.comic_id]))?;
            }
            continue;
        };
        if had_id.as_deref() != Some(issue.id.as_str()) {
            db::write_layer(conn, p.comic_id, "cv", &Meta::new(), now)?;
        }
        if fresh {
            client.forget(&format!("issue/4000-{}", issue.id), &details_query());
        }
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
fn details_query() -> [(&'static str, String); 1] {
    [(
        "field_list",
        "id,name,issue_number,cover_date,volume,description,deck,person_credits,character_credits,team_credits,location_credits,story_arc_credits"
            .into(),
    )]
}

fn details(client: &Client, issue_id: &str) -> Result<Meta> {
    let body = client.get(&format!("issue/4000-{issue_id}"), &details_query(), 30, true)?;
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
    /// Books matched to Comic Vine, of all the books there are.
    pub matched: i64,
    pub books: i64,
    pub last_error: Option<String>,
}

pub fn status(client: &Client, conn: &Connection) -> Result<Status> {
    Ok(Status {
        has_key: client.has_key(),
        queued: conn.query_row("SELECT COUNT(*) FROM cv_queue", [], |r| r.get(0))?,
        matched: conn.query_row("SELECT COUNT(*) FROM comics WHERE cv_id IS NOT NULL", [], |r| r.get(0))?,
        books: conn.query_row("SELECT COUNT(*) FROM comics", [], |r| r.get(0))?,
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
                // Then, one series at a time, whether new issues are out.
                match check_new(client, &conn, crate::scan::now()) {
                    // Only a change is worth redrawing the screen for.
                    Ok(Some(news)) => {
                        if news {
                            changed(0, 0);
                        }
                        continue;
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("comic vine new issues: {e}"),
                }
            }
            let g = client.wake_lock.lock().unwrap();
            let _ = client.wake.wait_timeout(g, Duration::from_secs(60));
            continue;
        };
        let now = crate::scan::now();
        match details(client, &issue_id) {
            Ok(m) => {
                // Matched again to another issue (or unmatched) while this was on its way: drop it.
                let still: Option<String> =
                    conn.query_row("SELECT issue_id FROM cv_queue WHERE comic_id=?1", [comic_id], |r| r.get(0)).optional().unwrap_or(None);
                if still.as_deref() != Some(issue_id.as_str()) {
                    continue;
                }
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
        let local = Local { name: "Absolute Batman".into(), books: 17, year: Some(2024), publisher: Some("DC Comics".into()), collected: None };
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
             INSERT INTO comics(id, series_id, section, rel_path, file_name, label, year, num_pages) VALUES
               (1, 1, '', 'DC/Absolute Batman/1.cbz', 'Absolute Batman 001 (2024).cbz', '#1', '2024', 30),
               (2, 1, '', 'DC/Absolute Batman/2.cbz', 'Absolute Batman 002 (2024).cbz', '#2', '2024', 30),
               (3, 1, '', 'DC/Absolute Batman/9.cbz', 'Absolute Batman 009 (2025).cbz', '#9', '2025', 30),
               (4, 1, 'Annuals', 'DC/Absolute Batman/Annuals/1.cbz', 'Absolute Batman Annual 001 (2025).cbz', '#1', '2025', 40),
               (5, 2, '', 'DC/Loose/x.cbz', 'x.cbz', 'x', NULL, NULL);
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
                ("field_list", "id,name,issue_number,cover_date,store_date,image,description,deck".into()),
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

    /// How well searching finds the right Comic Vine series, on a copy of a real library
    /// (`AION_TEST_DB`, with its key; answers kept in `AION_TEST_CACHE`). The right series is known
    /// from an issue id a book already carries (YACReader's import, a ComicInfo.xml or a match).
    /// `AION_EVAL_N` series are tried (default 60), from `AION_EVAL_OFFSET` on. Prints where the right
    /// one ranked, then totals.
    #[test]
    #[ignore]
    fn search_quality() {
        let conn = db::open(std::path::Path::new(&std::env::var("AION_TEST_DB").unwrap())).unwrap();
        let key: Option<String> = conn.query_row("SELECT value FROM settings WHERE key='cv_api_key'", [], |r| r.get(0)).optional().unwrap();
        let client = Client::new(std::path::Path::new(&std::env::var("AION_TEST_CACHE").unwrap()), key);
        let n: usize = std::env::var("AION_EVAL_N").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
        // One book with an issue id per series, spread across the library.
        let picks: Vec<(i64, String, String)> = {
            let mut st = conn
                .prepare(
                    "SELECT c.series_id, s.name, MIN(c.cv_id) FROM comics c JOIN series s ON s.id=c.series_id
                     WHERE c.cv_id GLOB '[0-9]*' AND c.section='' GROUP BY c.series_id ORDER BY c.series_id",
                )
                .unwrap();
            let all: Vec<(i64, String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            let step = (all.len() / n).max(1);
            // AION_EVAL_OFFSET picks a different spread of series, to check on ones not tuned on.
            let offset: usize = std::env::var("AION_EVAL_OFFSET").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            all.into_iter().skip(offset).step_by(step).take(n).collect()
        };
        let (mut first, mut top3, mut top10, mut missed, mut tried) = (0, 0, 0, 0, 0);
        for (sid, name, issue) in picks {
            let truth = match client.get(&format!("issue/4000-{issue}"), &[("field_list", "volume".into())], 60, true) {
                Ok(b) => b.get("results").and_then(|r| r.get("volume")).and_then(|v| s(v, "id")),
                Err(e) => {
                    println!("{name}: {e}");
                    continue;
                }
            };
            let Some(truth) = truth else { continue };
            let query = default_query(&conn, sid).unwrap();
            let found = match search(&client, &conn, sid, &query) {
                Ok(f) => f,
                Err(e) => {
                    println!("{name}: {e}");
                    continue;
                }
            };
            tried += 1;
            let at = found.iter().position(|v| v.id == truth);
            match at {
                Some(0) => first += 1,
                Some(1..=2) => top3 += 1,
                Some(3..=9) => top10 += 1,
                _ => missed += 1,
            }
            let top = found.first().map(|v| format!("{} ({}, {}, {})", v.name, v.start_year.as_deref().unwrap_or("?"), v.publisher.as_deref().unwrap_or("?"), v.count)).unwrap_or_default();
            let right = found.iter().find(|v| v.id == truth).map(|v| format!("{} ({}, {})", v.name, v.start_year.as_deref().unwrap_or("?"), v.count)).unwrap_or_else(|| format!("volume {truth}"));
            println!("{:>4} | {name} | searched {query:?} | top: {top} | right: {right}", at.map_or("-".into(), |a| (a + 1).to_string()));
        }
        println!("{tried} series: first {first}, in top 3 {}, in top 10 {}, not found {missed}", first + top3, first + top3 + top10);
    }

    /// What the matcher would now offer for each matched series in a copy of a real library,
    /// from Comic Vine answers already kept (`AION_TEST_DB`, `AION_TEST_CACHE`). Asks nothing new.
    #[test]
    #[ignore]
    fn real_plans() {
        let conn = db::open(std::path::Path::new(&std::env::var("AION_TEST_DB").unwrap())).unwrap();
        let client = Client::new(std::path::Path::new(&std::env::var("AION_TEST_CACHE").unwrap()), None);
        let matched: Vec<(i64, String, String)> = {
            let mut st = conn.prepare("SELECT v.series_id, s.name, v.volume_id FROM cv_series v JOIN series s ON s.id=v.series_id").unwrap();
            let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
            v
        };
        for (sid, name, vid) in matched {
            match plan(&client, &conn, sid, &vid, false) {
                Ok(p) => {
                    for b in p.proposals {
                        let now: Option<String> = conn
                            .query_row("SELECT json_extract(data, '$.cv_id') FROM meta_layer WHERE comic_id=?1 AND source='cv'", [b.comic_id], |r| r.get(0))
                            .optional()
                            .unwrap()
                            .flatten();
                        let mark = if now == b.issue_id { " " } else { "*" };
                        println!("{mark} {name} | {} | now {:?} -> offered {:?} ({})", b.label, now, b.issue_id, b.confidence);
                    }
                }
                Err(e) => println!("{name}: {e}"),
            }
        }
    }

    #[test]
    fn trades_and_annuals_arent_issues() {
        let (conn, client) = library("formats");
        // A trade and an annual sitting among the issues, numbered 1 and 2 like issues are.
        conn.execute_batch(
            "INSERT INTO comics(id, series_id, section, rel_path, file_name, label, year, num_pages) VALUES
               (6, 1, '', 'DC/Absolute Batman/v1.cbz', 'Absolute Batman v01 - The Zoo (2025).cbz', 'Vol. 1', '2024', 160),
               (7, 1, '', 'DC/Absolute Batman/a2.cbz', 'Absolute Batman Annual 002 (2024).cbz', '#2', '2024', 40);",
        )
        .unwrap();
        let plan = plan(&client, &conn, 1, "777", false).unwrap();
        let got = |id: i64| plan.proposals.iter().find(|p| p.comic_id == id).unwrap().issue_id.clone();
        assert_eq!((got(1).as_deref(), got(2).as_deref()), (Some("1001"), Some("1002")));
        assert_eq!((got(6), got(7)), (None, None));
    }

    #[test]
    fn which_books_a_series_holds() {
        let book = |kind: &'static str, section: &str, manga: bool| Book {
            id: 0,
            label: String::new(),
            section: section.into(),
            number: None,
            year: None,
            kind,
            manga,
            cv_id: None,
            cv_volume_id: None,
            settled: false,
        };
        let issue = |name: Option<&str>| Issue { id: "1".into(), number: Some("1".into()), name: name.map(Into::into), cover_date: None, store_date: None, image: None, description: None };
        let singles = [issue(None), issue(Some("The Paper"))];
        let trades = [issue(Some("Volume 1: The Trench")), issue(Some("Vol. 2")), issue(Some("Book Three"))];
        // Issues and a trade side by side, matched to a series of single issues.
        let mixed = [book("issue", "", false), book("volume", "", false), book("annual", "", false)];
        let h = holds(&mixed, &singles);
        assert_eq!((h(&mixed[0]), h(&mixed[1]), h(&mixed[2])), (Some("sure"), None, None));
        // Trades, matched to a series of trades.
        let shelf = [book("volume", "", false), book("issue", "Incursion", false)];
        assert_eq!(holds(&shelf, &trades)(&shelf[0]), Some("sure"));
        // Trades, matched to a series that doesn't say: worth a check.
        assert_eq!(holds(&shelf, &singles)(&shelf[0]), Some("check"));
        // A manga's volumes are what Comic Vine lists; its chapters aren't.
        let manga = [book("volume", "", true), book("chapter", "", true)];
        let h = holds(&manga, &singles);
        assert_eq!((h(&manga[0]), h(&manga[1])), (Some("sure"), None));
    }

    #[test]
    fn matching_again_starts_clean() {
        let (conn, client) = library("again");
        let story = |id: i64| conn.query_row("SELECT story_arc FROM comics WHERE id=?1", [id], |r| r.get::<_, Option<String>>(0)).unwrap();
        // Book 1 was paired with #1 and got its story arc; book 2 with #2.
        let mut m = Meta::new();
        m.insert("story_arc".into(), json!("The Zoo"));
        m.insert("cv_volume_id".into(), json!("777"));
        db::merge_cv(&conn, 1, &m, 0).unwrap();
        apply(&client, &conn, 1, "777", &[Pair { comic_id: 2, issue_id: Some("1002".into()) }], true, false, 0).unwrap();
        // Matching again: book 1 is really #2, and book 2 is none of these.
        let pairs = [Pair { comic_id: 1, issue_id: Some("1002".into()) }, Pair { comic_id: 2, issue_id: None }];
        apply(&client, &conn, 1, "777", &pairs, true, false, 0).unwrap();
        assert_eq!(story(1), None, "nothing of the old issue stays");
        let title = |id: i64| conn.query_row("SELECT title, cv_id FROM comics WHERE id=?1", [id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))).unwrap();
        assert_eq!(title(1), (Some("The Zoo, Part Two".into()), Some("1002".into())));
        assert_eq!(title(2), (None, None), "a book left unpaired loses its old details");
        assert_eq!(queued(&conn), vec![(1, "1002".into())]);
        // A match kept from before is offered again as it was.
        let plan = plan(&client, &conn, 1, "777", false).unwrap();
        assert_eq!(plan.proposals.iter().find(|p| p.comic_id == 1).unwrap().issue_id.as_deref(), Some("1002"));
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
    fn new_issues_past_the_last_one_here() {
        assert_eq!(date_after(0, 0), "1970-01-01");
        assert_eq!(date_after(1_794_355_200, 60), "2027-01-10");
        let book = |id: i64, n: &str, cv: Option<&str>| Book {
            id,
            label: format!("#{n}"),
            section: String::new(),
            number: Some(n.into()),
            year: None,
            kind: "issue",
            manga: false,
            cv_id: cv.map(Into::into),
            cv_volume_id: None,
            settled: false,
        };
        let issue = |id: &str, n: &str, date: &str| Issue {
            id: id.into(),
            number: Some(n.into()),
            name: None,
            cover_date: Some(date.into()),
            store_date: None,
            image: None,
            description: None,
        };
        let books = [book(1, "1", Some("a")), book(2, "2", Some("b")), book(3, "4", None)];
        let issues = [
            issue("a", "1", "2026-01-01"),
            issue("b", "2", "2026-02-01"),
            issue("c", "3", "2026-03-01"),
            issue("d", "4", "2026-04-01"),
            issue("e", "5", "2026-05-01"),
            issue("f", "6", "2026-12-01"),
            issue("g", "7", "2027-03-01"),
        ];
        // #3 is a gap, not new; #5 and #6 are out by mid-January; #7 isn't yet.
        assert_eq!(new_numbers(&books, &issues, "2027-01-10"), ["5", "6"]);
        // Nothing here of the kind it holds: nothing to compare with.
        let trades = [Book { kind: "volume", ..book(9, "1", None) }];
        assert!(new_numbers(&trades, &issues, "2027-01-10").is_empty());
    }

    #[test]
    fn names_that_cover_yours() {
        // The whole name beats a shorter one inside it.
        assert!(name_score("X-Men: Dark Phoenix Saga Complete Collection", "X-Men The Dark Phoenix Saga") > name_score("X-Men", "X-Men The Dark Phoenix Saga"));
        // A one-letter slip still matches.
        assert!(name_score("Fantastic Four By Jonathan Hickman", "Fantastic Four By Johnathan Hickman") > name_score("Fantastic Four", "Fantastic Four By Johnathan Hickman"));
        // A subtitle on its own.
        assert!(name_score("The Sworn Sword: The Graphic Novel", "The Hedge Knight II - The Sworn Sword") > name_score("Knight", "The Hedge Knight II - The Sworn Sword"));
        assert_eq!(name_score("Saga", "Saga"), 1.0);
        assert!(one_off("johnathan", "jonathan") && one_off("colour", "color") && !one_off("batman", "superman"));
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
