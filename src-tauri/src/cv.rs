//! Comic Vine: finding a series, matching its books to issues, and filling in their metadata.
//!
//! Comic Vine asks for no more than about 200 requests per resource per hour and no bursts, so every
//! request goes through one throttle (at least a second apart, and under the hourly allowance) and
//! answers are cached on disk. Matching works at series level: one search, one issue list, and the
//! books are paired with issues by number and year. Credits, characters and story arcs need one
//! request per issue, so a background queue fills those in slowly and survives restarts.

use std::collections::{HashMap, VecDeque};
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

/// Every issue of a Comic Vine series, in order.
fn issues(client: &Client, volume_id: &str) -> Result<Vec<Issue>> {
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
            7,
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
    let issues = issues(client, volume_id)?;
    let mut st = conn.prepare(
        "SELECT id, label, section, number, year FROM comics WHERE series_id=?1 ORDER BY section!='', sort_key",
    )?;
    let books = st
        .query_map([series_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut taken = std::collections::HashSet::new();
    let proposals = books
        .into_iter()
        .map(|(comic_id, label, section, number, year)| {
            let mut issue_id = None;
            let mut confidence = "none";
            if section.is_empty() {
                if let Some(n) = book_number(number, &label) {
                    let year = year.and_then(|y| y.get(..4).and_then(|y| y.parse::<i64>().ok()));
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
                    if let Some(i) = best {
                        let close = match (issue_year(i), year) {
                            (Some(a), Some(b)) => (a - b).abs() <= 1,
                            _ => true,
                        };
                        confidence = if close && same.len() == 1 { "sure" } else { "check" };
                        taken.insert(i.id.clone());
                        issue_id = Some(i.id.clone());
                    }
                }
            }
            Proposal { comic_id, label, section, issue_id, confidence }
        })
        .collect();
    Ok(IssuePlan { issues, proposals })
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
    let by_id: HashMap<String, Issue> = issues(client, volume_id)?.into_iter().map(|i| (i.id.clone(), i)).collect();
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
        put("title", issue.name.clone());
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
        &[("field_list", "id,description,deck,person_credits,character_credits,team_credits,location_credits,story_arc_credits".into())],
        30,
        true,
    )?;
    let r = body.get("results").cloned().unwrap_or(Value::Null);
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
    let mut m = Meta::new();
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
