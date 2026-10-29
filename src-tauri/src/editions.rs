//! Which format a book is (issue, chapter, volume, collection, deluxe edition, omnibus, annual or
//! special), worked out from what's already known about it: its subfolder, its file name and its
//! page count. No file is opened, so a whole series sorts in no time. A book you've moved by hand
//! keeps the format you gave it (see `db::set_edition`).

use regex::Regex;
use std::sync::LazyLock;

/// The formats, in the order the series page lists them.
pub const KINDS: [&str; 8] = ["volume", "issue", "chapter", "collection", "deluxe", "omnibus", "annual", "special"];

/// What a format is called on the series page.
pub fn name(kind: &str) -> &'static str {
    match kind {
        "issue" => "Issues",
        "chapter" => "Chapters",
        "volume" => "Volumes",
        "collection" => "Collections",
        "deluxe" => "Deluxe editions",
        "omnibus" => "Omnibus",
        "annual" => "Annuals",
        _ => "Specials",
    }
}

fn re(p: &str) -> Regex {
    Regex::new(&format!("(?i){p}")).unwrap()
}

// Words that name a format outright, strongest first; looked for in the subfolder, then the file name.
static WORDS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    vec![
        ("annual", re(r"\bannuals?\b")),
        ("special", re(r"\bspecials?\b|one[- ]?shot|\bfcbd\b|free comic book day|director'?s cut|giant[- ]size|king[- ]size|\bpreview\b")),
        ("omnibus", re(r"omnibus|compendium|\bcomplete\b|perfect edition|master edition|big book")),
        ("deluxe", re(r"deluxe|hardcover|\bhc\b|absolute edition|library edition|oversized|treasury|artist'?s edition|anniversary edition")),
        ("collection", re(r"epic collection|ultimate collection|masterworks|essential|showcase presents")),
        ("chapter", re(r"\bch(?:apter)?s?\b\.?\s?\d|\bchapters\b")),
        ("volume", re(r"\btpb\b|\btrades?\b|(?:\b|_)vol(?:ume)?s?(?:\b|_)\.?\s?\d|(?:\b|_)v\d{1,3}(?:\b|_)|\bbook (?:one|two|three|four|five|\d)|\bvolumes\b")),
        ("issue", re(r"\bissues?\b")),
    ]
});
static BARE_NUMBER: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|[\s#_-])\d{2,4}(?:\.\d)?(?:[\s(._-]|$)"));

/// The format of one book. `section` is its subfolder within the series ("" for none).
pub fn kind(manga: bool, section: &str, file_name: &str, pages: i64) -> &'static str {
    let folder = section.rsplit('/').next().unwrap_or("").trim().to_lowercase();
    match folder.as_str() {
        "chapters" => return "chapter",
        "issues" => return "issue",
        "volumes" | "tpb" | "tpbs" | "trades" => return "volume",
        "omnibus" | "omnibuses" => return "omnibus",
        "annuals" => return "annual",
        "specials" | "one-shots" | "one shots" => return "special",
        _ => {}
    }
    for (k, p) in WORDS.iter() {
        if p.is_match(&folder) {
            return k;
        }
    }
    let file = file_name.to_lowercase();
    for (k, p) in WORDS.iter() {
        if p.is_match(&file) {
            return k;
        }
    }
    // A bare number: an issue or a chapter, unless it's far too long to be one.
    if BARE_NUMBER.is_match(&file) {
        return match (pages >= 120, manga) {
            (true, true) => "volume",
            (true, false) => "collection",
            (false, true) => "chapter",
            (false, false) => "issue",
        };
    }
    // No number at all: a book of its own, told by its size.
    if pages >= 600 {
        "omnibus"
    } else if pages >= 90 {
        if manga { "volume" } else { "collection" }
    } else {
        "special"
    }
}

static YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"\((?:19|20)\d\d\)"));
static VOL_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"(?:\b|_)(?:vol(?:ume)?|v|book|part|omnibus|tpb)[\s._]*0*(\d{1,3})(?:\b|_)"));
static CH_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"\bch(?:apter)?\.?\s*0*(\d{1,4}(?:\.\d)?)"));
static AFTER_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:annual|special|issue)[\s#._-]*0*(\d{1,3})\b"));
static ANY_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|[\s#_-])0*(\d{1,4}(?:\.\d)?)(?:[\s(._-]|$)"));
static WORD_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"\bbook (one|two|three|four|five|six|seven|eight|nine|ten)\b"));

/// The book's number within its format, from its file name: "Saga Vol. 3" is 3 as a volume,
/// "Titans Annual 002" is 2 as an annual.
pub fn number(kind: &str, file_name: &str) -> Option<f64> {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    // Years in brackets would otherwise pass for numbers: "Titans 019 (2018)".
    let stem = YEAR.replace_all(stem, " ");
    let grab = |r: &Regex| r.captures(&stem).and_then(|c| c[1].parse::<f64>().ok());
    let words = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
    match kind {
        "chapter" => grab(&CH_NUM).or_else(|| grab(&ANY_NUM)),
        "annual" | "special" => grab(&AFTER_WORD).or_else(|| grab(&VOL_NUM)),
        "issue" => grab(&AFTER_WORD).or_else(|| grab(&ANY_NUM)),
        _ => grab(&VOL_NUM)
            .or_else(|| WORD_NUM.captures(&stem).and_then(|c| words.iter().position(|w| c[1].eq_ignore_ascii_case(w))).map(|i| i as f64 + 1.0))
            .or_else(|| grab(&ANY_NUM)),
    }
}

fn show(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
}

/// How a book is labeled on the series page: "#20", "Ch. 110", "Vol. 3", "Annual #2". Books
/// without a number keep their own label.
pub fn label(kind: &str, n: Option<f64>, own: &str) -> String {
    let Some(n) = n else { return own.to_string() };
    match kind {
        "issue" => format!("#{}", show(n)),
        "chapter" => format!("Ch. {}", show(n)),
        "annual" => format!("Annual #{}", show(n)),
        "special" => format!("Special #{}", show(n)),
        _ => format!("Vol. {}", show(n)),
    }
}

static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"<[^>]+>|[\r\n]+"));
static COLLECTS: LazyLock<Regex> =
    LazyLock::new(|| re(r"collect(?:s|ing|ed)?\b[^.#]{0,90}?#\s?(\d{1,4})(?:\s?(?:-|–|—|to|through)\s?#?(\d{1,4}))?"));

/// The issues a collection says it holds, from its description: "Collects SAGA #13-18" → "#13–18".
pub fn collects(synopsis: &str) -> Option<String> {
    // Descriptions from Comic Vine are HTML.
    let text = TAG.replace_all(synopsis, " ").replace("&nbsp;", " ").replace("&#8211;", "–");
    let c = COLLECTS.captures(&text)?;
    Some(match c.get(2) {
        Some(to) => format!("#{}–{}", &c[1], to.as_str()),
        None => format!("#{}", &c[1]),
    })
}

/// The whole numbers missing between a run's first and last issue, when there are only a few.
pub fn gaps(numbers: &[f64]) -> Vec<i64> {
    let mut whole: Vec<i64> = numbers.iter().filter(|n| n.fract() == 0.0 && **n >= 1.0).map(|n| *n as i64).collect();
    whole.sort_unstable();
    whole.dedup();
    let (Some(&lo), Some(&hi)) = (whole.first(), whole.last()) else { return vec![] };
    let missing: Vec<i64> = (lo..=hi).filter(|n| whole.binary_search(n).is_err()).collect();
    if missing.len() <= 6 { missing } else { vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind(false, "", "Saga Vol. 3.cbz", 152), "volume");
        assert_eq!(kind(false, "Issues", "Saga 019 (2014) (Digital).cbr", 32), "issue");
        assert_eq!(kind(false, "Compendium", "Saga - Compendium v01 (2019).cbz", 1322), "omnibus");
        assert_eq!(kind(false, "", "Titans Annual 002 (2018) (digital).cbz", 40), "annual");
        assert_eq!(kind(false, "", "Titans Special 01 (2018) (digital).cbz", 40), "special");
        assert_eq!(kind(false, "", "Titans 019 (2018) (2 covers) (digital).cbz", 30), "issue");
        assert_eq!(kind(true, "Chapters", "Solo Leveling 085 (2020) (Digital).cbz", 33), "chapter");
        assert_eq!(kind(true, "", "Erased_v01_(2017)_(Digital).cbz", 193), "volume");
        assert_eq!(kind(false, "", "All-Star Superman - The Deluxe Edition (2021).cbz", 307), "deluxe");
        assert_eq!(kind(false, "", "Batman - Earth One (2012) (digital).cbr", 142), "collection");
    }

    #[test]
    fn numbers_and_labels() {
        assert_eq!(number("volume", "Saga Vol. 11.cbz"), Some(11.0));
        assert_eq!(number("issue", "Titans 019 (2018) (2 covers) (digital).cbz"), Some(19.0));
        assert_eq!(number("issue", "Titans 013 (2017) GetComics.INFO (1).cbz"), Some(13.0));
        assert_eq!(number("annual", "Titans Annual 002 (2018) (digital).cbz"), Some(2.0));
        assert_eq!(number("annual", "Absolute Batman 2025 Annual 001 (2025).cbz"), Some(1.0));
        assert_eq!(number("chapter", "Solo Leveling 000 - Prologue (2020).cbz"), Some(0.0));
        assert_eq!(number("volume", "Teen Titans by Geoff Johns Book 02 (2018).cbz"), Some(2.0));
        assert_eq!(number("volume", "Kokou no Hito - Volume_04.cbz"), Some(4.0));
        assert_eq!(label("chapter", Some(110.0), "Vol. 9"), "Ch. 110");
        assert_eq!(label("special", None, "Heaven and Earth"), "Heaven and Earth");
    }

    #[test]
    fn collected_issues() {
        assert_eq!(collects("Collects SAGA #13-18. The Eisner…").as_deref(), Some("#13–18"));
        assert_eq!(collects("collecting issues #6-11 of the hit").as_deref(), Some("#6–11"));
        assert_eq!(collects("collects All-Star Superman#1-12").as_deref(), Some("#1–12"));
        assert_eq!(collects("A new series begins!"), None);
        assert_eq!(collects("<p><em>Collects</em> issues <b>#1-5</b>.</p>").as_deref(), Some("#1–5"));
        assert_eq!(gaps(&[19.0, 20.0, 22.0, 23.0]), vec![21]);
    }
}
