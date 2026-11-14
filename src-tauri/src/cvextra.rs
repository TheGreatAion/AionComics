//! More from Comic Vine, fetched when you look rather than in the background: the language of an
//! edition from another country, a character's or creator's picture and story, and a book's other
//! covers and the characters who first appear in it.
//!
//! Every answer goes through the client's disk cache (kept two months), so a page looked at once
//! costs nothing the next time, and nothing is fetched for what you never open.

use crate::cv::Client;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::sync::LazyLock;

/// How long answers about characters, creators and covers are kept.
const KEEP_DAYS: u64 = 60;

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| match x {
        Value::String(t) if !t.trim().is_empty() => Some(t.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

// ---------- the language of an edition ----------

/// "Spanish publication.", "French translation of…", "English translation of the Japanese manga":
/// how Comic Vine's editors open the description of an edition, naming its language. Only the
/// opening counts: "For the German translation, see…" further on points at another series.
static SAID: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"(?i)^\W*(?:the\s+|an?\s+)?(english|spanish|french|german|italian|portuguese|brazilian|dutch|polish|turkish|japanese|korean|chinese|swedish|danish|norwegian|finnish|greek|czech|hungarian|russian|indonesian|thai|vietnamese|argentin(?:e|ian)|mexican|catalan|croatian|serbian|slovenian|romanian|hebrew|arabic|filipino|tagalog|bulgarian|ukrainian)\s+(?:language\s+)?(?:publication|edition|translation|reprint|version)",
    )
    .unwrap()
});

static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]*>").unwrap());

/// Publishers that put out other countries' editions of English comics.
const PUBLISHERS: &[(&str, &str)] = &[
    ("panini españa", "Spanish"),
    ("panini verlag", "German"),
    ("panini france", "French"),
    ("panini brasil", "Portuguese"),
    ("panini comics méxico", "Spanish"),
    ("ecc ediciones", "Spanish"),
    ("planeta deagostini", "Spanish"),
    ("planeta cómic", "Spanish"),
    ("norma editorial", "Spanish"),
    ("editorial televisa", "Spanish"),
    ("ovni press", "Spanish"),
    ("urban comics", "French"),
    ("glénat", "French"),
    ("kana", "French"),
    ("dargaud", "French"),
    ("dupuis", "French"),
    ("delcourt", "French"),
    ("soleil", "French"),
    ("semic", "French"),
    ("carlsen", "German"),
    ("egmont ehapa", "German"),
    ("cross cult", "German"),
    ("star comics", "Italian"),
    ("rw edizioni", "Italian"),
    ("rw lion", "Italian"),
    ("edizioni bd", "Italian"),
    ("j-pop", "Italian"),
    ("planet manga", "Italian"),
    ("mondadori", "Italian"),
    ("sergio bonelli", "Italian"),
    ("marmara çizgi", "Turkish"),
    ("jbc", "Portuguese"),
    ("mythos editora", "Portuguese"),
    ("egmont polska", "Polish"),
    ("mucha comics", "Polish"),
];

/// Japanese and Korean publishers of the original editions, and British reprints, by their exact
/// name on Comic Vine (English arms of Japanese publishers, like "Kodansha Comics", are named
/// differently).
const ORIGINALS: &[(&str, &str)] = &[
    ("shueisha", "Japanese"),
    ("kodansha", "Japanese"),
    ("shogakukan", "Japanese"),
    ("hakusensha", "Japanese"),
    ("akita shoten", "Japanese"),
    ("square enix", "Japanese"),
    ("kadokawa", "Japanese"),
    ("kadokawa shoten", "Japanese"),
    ("ascii media works", "Japanese"),
    ("media factory", "Japanese"),
    ("futabasha", "Japanese"),
    ("shonen gahosha", "Japanese"),
    ("mag garden", "Japanese"),
    ("ichijinsha", "Japanese"),
    ("houbunsha", "Japanese"),
    ("shinchosha", "Japanese"),
    ("takeshobo", "Japanese"),
    ("enterbrain", "Japanese"),
    ("tokuma shoten", "Japanese"),
    ("gentosha", "Japanese"),
    ("coamix", "Japanese"),
    ("nihon bungeisha", "Japanese"),
    ("leed publishing", "Japanese"),
    // Reprints for Britain: English, but not the edition you have.
    ("marvel uk/panini uk", "UK edition"),
    ("panini uk", "UK edition"),
    ("titan comics uk", "UK edition"),
    ("daewon c.i.", "Korean"),
    ("haksan", "Korean"),
    ("seoul cultural publishers", "Korean"),
];

/// Words in a publisher's name that say where it's from.
const PLACES: &[(&str, &str)] = &[
    ("españa", "Spanish"),
    ("espana", "Spanish"),
    ("méxico", "Spanish"),
    ("mexico", "Spanish"),
    ("argentina", "Spanish"),
    ("france", "French"),
    ("italia", "Italian"),
    ("deutschland", "German"),
    ("verlag", "German"),
    ("gmbh", "German"),
    ("editionen", "German"),
    ("ediciones", "Spanish"),
    ("edizioni", "Italian"),
    ("éditions", "French"),
    ("editora", "Portuguese"),
    ("uitgeverij", "Dutch"),
    ("wydawnictwo", "Polish"),
    ("brasil", "Portuguese"),
    ("polska", "Polish"),
    ("türkiye", "Turkish"),
    ("nederland", "Dutch"),
    ("sverige", "Swedish"),
];

/// The language of a series in another language than English, when its description or its
/// publisher says. None for English, and when nothing says (no guessing).
pub fn language(publisher: Option<&str>, description: Option<&str>) -> Option<String> {
    let opening = description.map(|d| TAGS.replace_all(d, " ").replace("&nbsp;", " "));
    if let Some(m) = opening.as_deref().and_then(|d| SAID.captures(d.trim_start())) {
        let word = m[1].to_lowercase();
        return Some(
            match word.as_str() {
                // Said outright: whatever the publisher, it's the English edition.
                "english" => return None,
                "brazilian" => "Portuguese",
                "mexican" | "argentine" | "argentinian" => "Spanish",
                "tagalog" => "Filipino",
                w => return Some(capitalize(w)),
            }
            .to_string(),
        );
    }
    let p = publisher?.to_lowercase();
    let plain = romaji(&p);
    if let Some((_, lang)) = ORIGINALS.iter().find(|(name, _)| plain == *name) {
        return Some(lang.to_string());
    }
    PUBLISHERS
        .iter()
        .find(|(name, _)| p == *name || p.starts_with(&format!("{name} ")))
        .or_else(|| PLACES.iter().find(|(word, _)| p.split(|c: char| !c.is_alphanumeric()).any(|w| w == *word)))
        .map(|(_, lang)| lang.to_string())
}

/// Long vowels written out plainly: "shônen gahôsha" and "shōnen gahōsha" as "shonen gahosha".
fn romaji(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'â' | 'ā' => 'a',
            'î' | 'ī' => 'i',
            'û' | 'ū' => 'u',
            'ê' | 'ē' => 'e',
            'ô' | 'ō' => 'o',
            c => c,
        })
        .collect()
}

fn capitalize(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

// ---------- characters and creators ----------

/// The issue someone first appeared in: "Saga #1", its cover date, and your book of it, if any.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FirstIn {
    pub label: String,
    pub date: Option<String>,
    pub comic_id: Option<i64>,
    pub series_id: Option<i64>,
}

/// What Comic Vine says about a character or a creator.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub name: String,
    pub image: Option<String>,
    /// One line about them.
    pub deck: Option<String>,
    /// The full story, as Comic Vine writes it (HTML).
    pub description: Option<String>,
    pub real_name: Option<String>,
    pub aliases: Vec<String>,
    /// For a character: alien, mutant, human…
    pub origin: Option<String>,
    pub publisher: Option<String>,
    pub issues: Option<i64>,
    pub first: Option<FirstIn>,
    pub creators: Vec<String>,
    /// For a creator.
    pub country: Option<String>,
    pub hometown: Option<String>,
    pub born: Option<String>,
    pub died: Option<String>,
    pub site: Option<String>,
}

const CHARACTER_FIELDS: &str =
    "id,name,real_name,aliases,deck,description,image,first_appeared_in_issue,count_of_issue_appearances,publisher,origin,creators,site_detail_url";
const PERSON_FIELDS: &str = "id,name,aliases,deck,description,image,country,hometown,birth,death,site_detail_url";

fn picture(v: &Value) -> Option<String> {
    let img = v.get("image")?;
    let url = s(img, "medium_url").or_else(|| s(img, "small_url"))?;
    (!url.contains("blank") && !url.contains("default")).then_some(url)
}

/// The publishers of the books a character is in, most books first: which of several characters
/// with the same name is meant.
fn publishers_of(conn: &Connection, character: &str) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT LOWER(c.publisher) FROM comics c
         WHERE c.publisher IS NOT NULL AND c.publisher != ''
           AND instr(char(10) || COALESCE(c.characters, '') || char(10), char(10) || ?1 || char(10)) > 0
         GROUP BY LOWER(c.publisher) ORDER BY COUNT(*) DESC LIMIT 3",
    )?;
    let v = st.query_map([character], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

/// The one record of several with a name: the same name, from the publisher of your books, then
/// in the most issues.
fn best<'a>(results: &'a [Value], name: &str, publishers: &[String]) -> Option<&'a Value> {
    let same = |v: &&Value| s(v, "name").is_some_and(|n| n.eq_ignore_ascii_case(name.trim()));
    let mut pool: Vec<&Value> = results.iter().filter(same).collect();
    if pool.is_empty() {
        pool = results.iter().collect();
    }
    let rank = |v: &Value| {
        let p = v.get("publisher").and_then(|p| s(p, "name")).map(|p| p.to_lowercase()).unwrap_or_default();
        let from = publishers.iter().position(|x| !p.is_empty() && (x.contains(&p) || p.contains(x.as_str()))).map_or(0, |i| 3 - i as i64);
        (from, v.get("count_of_issue_appearances").and_then(|c| c.as_i64()).unwrap_or(0))
    };
    pool.into_iter().max_by_key(|v| rank(v))
}

/// Your copy of a Comic Vine issue (the book and its series), if you have one matched to it.
fn your_copy(conn: &Connection, issue_id: &str) -> Option<(i64, i64)> {
    conn.query_row(
        "SELECT c.id, c.series_id FROM meta_layer m JOIN comics c ON c.id=m.comic_id
         WHERE m.source='cv' AND json_extract(m.data, '$.cv_id')=?1 LIMIT 1",
        [issue_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .ok()
    .flatten()
}

/// "Saga #1" and its cover date, for an issue Comic Vine names only by number and id.
fn first_in(client: &Client, conn: &Connection, first: &Value) -> Option<FirstIn> {
    let id = s(first, "id")?;
    let issue = client
        .get(&format!("issue/4000-{id}"), &[("field_list", "id,issue_number,name,cover_date,volume".into())], KEEP_DAYS, false)
        .ok()?;
    let r = issue.get("results")?;
    let series = r.get("volume").and_then(|v| s(v, "name")).unwrap_or_default();
    let n = s(r, "issue_number").or_else(|| s(first, "issue_number"));
    let label = match n {
        Some(n) if !series.is_empty() => format!("{series} #{}", crate::cv::norm_number(&n)),
        Some(n) => format!("#{}", crate::cv::norm_number(&n)),
        None => series,
    };
    let mine = your_copy(conn, &id);
    Some(FirstIn { label, date: s(r, "cover_date"), comic_id: mine.map(|m| m.0), series_id: mine.map(|m| m.1) })
}

fn character_record(client: &Client, id: &str) -> Result<Value> {
    let body = client.get(&format!("character/4005-{id}"), &[("field_list", CHARACTER_FIELDS.into())], KEEP_DAYS, false)?;
    Ok(body.get("results").cloned().unwrap_or(Value::Null))
}

/// What Comic Vine has on a character or creator ("character" or "creator"), by the name your books
/// use. None when Comic Vine has no one by that name.
pub fn about(client: &Client, conn: &Connection, kind: &str, name: &str) -> Result<Option<About>> {
    let list = |v: &Value| -> Vec<String> {
        s(v, "aliases").map(|a| a.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()).unwrap_or_default()
    };
    if kind == "character" {
        let found = client.get(
            "characters",
            &[("filter", format!("name:{name}")), ("field_list", "id,name,publisher,count_of_issue_appearances".into()), ("limit", "50".into())],
            KEEP_DAYS,
            false,
        )?;
        let results: Vec<Value> = found.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
        let Some(id) = best(&results, name, &publishers_of(conn, name)?).and_then(|v| s(v, "id")) else { return Ok(None) };
        let r = character_record(client, &id)?;
        let first = r.get("first_appeared_in_issue").filter(|f| !f.is_null()).and_then(|f| first_in(client, conn, f));
        return Ok(Some(About {
            name: s(&r, "name").unwrap_or_else(|| name.to_string()),
            image: picture(&r),
            deck: s(&r, "deck"),
            description: s(&r, "description"),
            real_name: s(&r, "real_name").filter(|n| !n.eq_ignore_ascii_case(name)),
            aliases: list(&r),
            origin: r.get("origin").and_then(|o| s(o, "name")),
            publisher: r.get("publisher").and_then(|p| s(p, "name")),
            issues: r.get("count_of_issue_appearances").and_then(|c| c.as_i64()),
            first,
            creators: r.get("creators").and_then(|c| c.as_array()).into_iter().flatten().filter_map(|c| s(c, "name")).collect(),
            site: s(&r, "site_detail_url"),
            ..Default::default()
        }));
    }
    let found = client.get(
        "people",
        &[("filter", format!("name:{name}")), ("field_list", PERSON_FIELDS.into()), ("limit", "20".into())],
        KEEP_DAYS,
        false,
    )?;
    let results: Vec<Value> = found.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
    let Some(r) = best(&results, name, &[]) else { return Ok(None) };
    let date = |k: &str| s(r, k).map(|d| d.chars().take(10).collect::<String>());
    Ok(Some(About {
        name: s(r, "name").unwrap_or_else(|| name.to_string()),
        image: picture(r),
        deck: s(r, "deck"),
        description: s(r, "description"),
        aliases: list(r),
        country: s(r, "country"),
        hometown: s(r, "hometown"),
        born: date("birth"),
        died: date("death"),
        site: s(r, "site_detail_url"),
        ..Default::default()
    }))
}

// ---------- a book's covers and first appearances ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cover {
    /// For the row of small covers.
    pub small: String,
    /// For looking at one.
    pub large: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Face {
    pub name: String,
    pub image: Option<String>,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BookExtras {
    /// The issue's covers, its main one first; empty unless there are others too.
    pub covers: Vec<Cover>,
    /// Characters whose first appearance this issue is.
    pub firsts: Vec<Face>,
}

/// One of Comic Vine's picture addresses at another of its sizes ("scale_medium", "original").
fn sized(url: &str, size: &str) -> String {
    let re = regex::Regex::new(r"/uploads/[a-z_]+/").unwrap();
    re.replace(url, format!("/uploads/{size}/").as_str()).into_owned()
}

/// How many of an issue's characters are looked up for first appearances.
const MOST_CHARACTERS: usize = 24;

/// A book's other covers and who first appears in it, for a book matched to a Comic Vine issue.
/// Looking for first appearances takes one request per character the first time (each kept two
/// months, and shared with character pages); it stops early, keeping what it has, if Comic Vine's
/// hourly allowance runs low.
pub fn book_extras(client: &Client, conn: &Connection, comic_id: i64) -> Result<BookExtras> {
    let issue_id: Option<String> = conn
        .query_row("SELECT json_extract(data, '$.cv_id') FROM meta_layer WHERE comic_id=?1 AND source='cv'", [comic_id], |r| r.get(0))
        .optional()?
        .flatten();
    let Some(issue_id) = issue_id else { return Ok(BookExtras::default()) };
    let body = client.get(
        &format!("issue/4000-{issue_id}"),
        &[("field_list", "id,image,associated_images,character_credits".into())],
        KEEP_DAYS,
        false,
    )?;
    let r = body.get("results").cloned().unwrap_or(Value::Null);
    let mut urls: Vec<String> = Vec::new();
    if let Some(u) = r.get("image").and_then(|i| s(i, "original_url")) {
        urls.push(u);
    }
    for a in r.get("associated_images").and_then(|a| a.as_array()).into_iter().flatten() {
        if let Some(u) = s(a, "original_url") {
            if !urls.contains(&u) {
                urls.push(u);
            }
        }
    }
    let covers = if urls.len() > 1 {
        urls.iter().map(|u| Cover { small: sized(u, "scale_medium"), large: sized(u, "scale_large") }).collect()
    } else {
        Vec::new()
    };
    let mut firsts = Vec::new();
    for c in r.get("character_credits").and_then(|c| c.as_array()).into_iter().flatten().take(MOST_CHARACTERS) {
        let Some(id) = s(c, "id") else { continue };
        let Ok(ch) = character_record(client, &id) else { break };
        let first = ch.get("first_appeared_in_issue").and_then(|f| s(f, "id"));
        if first.as_deref() == Some(issue_id.as_str()) {
            firsts.push(Face { name: s(&ch, "name").unwrap_or_default(), image: picture(&ch) });
        }
    }
    Ok(BookExtras { covers, firsts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_from_descriptions_and_publishers() {
        assert_eq!(language(Some("Urban Comics"), Some("<p>French publication.</p>")).as_deref(), Some("French"));
        assert_eq!(language(Some("Some House"), Some("Brazilian edition of the series")).as_deref(), Some("Portuguese"));
        assert_eq!(language(Some("Panini España"), None).as_deref(), Some("Spanish"));
        assert_eq!(language(Some("ECC Ediciones"), Some("")).as_deref(), Some("Spanish"));
        assert_eq!(language(Some("Panini Verlag"), None).as_deref(), Some("German"));
        assert_eq!(language(Some("Editora Brasil Comics"), None).as_deref(), Some("Portuguese"));
        // The Japanese original, not its English edition.
        assert_eq!(language(Some("Shueisha"), None).as_deref(), Some("Japanese"));
        assert_eq!(language(Some("Kodansha"), None).as_deref(), Some("Japanese"));
        assert_eq!(language(Some("Shônen Gahôsha"), None).as_deref(), Some("Japanese"));
        assert_eq!(language(Some("Shōnen Gahōsha"), None).as_deref(), Some("Japanese"));
        assert_eq!(language(Some("Kodansha Comics"), None), None);
        assert_eq!(language(Some("VIZ"), None), None);
        assert_eq!(language(Some("Marvel UK/Panini UK"), None).as_deref(), Some("UK edition"));
        // Only the opening counts; a pointer to another edition further on doesn't.
        assert_eq!(language(Some("Viz"), Some("<p>English translation of the Japanese manga Claymore.</p><p>For the German translation, see Claymore</p>")), None);
        assert_eq!(language(Some("Tokyopop GmbH"), Some("<p>For the original Japanese version, see Claymore</p>")).as_deref(), Some("German"));
        assert_eq!(language(Some("Editores de Tebeos"), Some("<p>Spanish translation of the Japanese manga Claymore.</p>")).as_deref(), Some("Spanish"));
        assert_eq!(language(Some("Kult Editionen"), None).as_deref(), Some("German"));
        // English, and anything that doesn't say, has no tag.
        assert_eq!(language(Some("DC Comics"), Some("The Absolute Universe begins.")), None);
        assert_eq!(language(Some("Image"), Some("A French artist draws this one.")), None);
        assert_eq!(language(None, None), None);
    }

    #[test]
    fn picture_sizes() {
        assert_eq!(
            sized("https://comicvine.gamespot.com/a/uploads/original/6/65423/2897180-saga5th.jpg", "scale_medium"),
            "https://comicvine.gamespot.com/a/uploads/scale_medium/6/65423/2897180-saga5th.jpg"
        );
    }
}
