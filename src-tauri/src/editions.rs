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
        ("chapter", re(r"\bch(?:apter)?s?\b\.?\s?\d|\bch\d{1,4}\b|\bchapters\b")),
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

static YEAR: LazyLock<Regex> = LazyLock::new(|| re(r"\((?:19|20)\d\d\)|\b(?:19|20)\d\d-\d\d-\d\d\b"));
static TAGS: LazyLock<Regex> = LazyLock::new(|| re(r"\s*[(\[][^)\]]*[)\]]"));
static NON_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"[^\p{L}\p{N}]+"));
static WORD: LazyLock<Regex> = LazyLock::new(|| re(r"[\p{L}\p{N}]+"));
static LEADING: LazyLock<Regex> = LazyLock::new(|| re(r"^0*(\d{1,4})\s+\D"));
static VOL_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"(?:\b|_)(?:vol(?:ume)?|v|book|part|omnibus|tpb)[\s._]*0*(\d{1,3})(?:\b|_)"));
static CH_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"\bch(?:apter)?\.?\s*0*(\d{1,4}(?:\.\d)?)"));
static AFTER_WORD: LazyLock<Regex> = LazyLock::new(|| re(r"\b(?:annual|special|issue)[\s#._-]*0*(\d{1,3})\b"));
static ANY_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|[\s#_-])0*(\d{1,4}(?:\.\d)?)(?:[\s(._-]|$)"));
static WORD_NUM: LazyLock<Regex> = LazyLock::new(|| re(r"\bbook (one|two|three|four|five|six|seven|eight|nine|ten)\b"));

/// The file name with the series' own name taken off the front, so a number in the name isn't
/// taken for the book's: "Ben 10 001" is " 001" in the series "Ben 10". Names are compared by
/// their letters and digits only ("Spider-Man" is "Spider Man"), without tags like "(2024)".
pub(crate) fn without_series<'a>(stem: &'a str, series: &str) -> &'a str {
    let words = |s: &str| NON_WORD.split(&s.to_lowercase()).filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let want = words(&TAGS.replace_all(series, ""));
    if want.is_empty() {
        return stem;
    }
    // Walk the stem word by word until the series' words are used up.
    for (m, w) in WORD.find_iter(stem).zip(&want) {
        if m.as_str().to_lowercase() != *w {
            return stem;
        }
        if std::ptr::eq(w, want.last().unwrap()) {
            return &stem[m.end()..];
        }
    }
    stem
}

/// A number the file name starts with is its place in a reading order ("019 Siege - Storming
/// Asgard"), which matters more than the issue's own number. Not when it's the start of the
/// series' own name ("100 Bullets - Book 01").
pub fn order_number(file_name: &str, series: &str) -> Option<f64> {
    if without_series(file_name, series).len() != file_name.len() {
        return None;
    }
    LEADING.captures(file_name).and_then(|c| c[1].parse().ok())
}

/// The book's number within its format, from its file name: "Saga Vol. 3" is 3 as a volume,
/// "Titans Annual 002" is 2 as an annual. `series` is the series' name, skipped where the file
/// name starts with it.
pub fn number(kind: &str, file_name: &str, series: &str) -> Option<f64> {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let stem = without_series(stem, series);
    // Years in brackets and dates would otherwise pass for numbers: "Titans 019 (2018)".
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
    LazyLock::new(|| re(r"(?:collect|reprint)(?:s|ing|ed)?\b([^.#]{0,90}?)#\s?(\d{1,4})(?:\s?(?:-|–|—|to|through)\s?#?(\d{1,4}))?"));
// Without a "#", only a range counts ("Collecting Justice League 0-17"): a lone number is too
// often a year or a count ("the 2005 event", "six issues").
static COLLECTS_RANGE: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?:collect|reprint)(?:s|ing|ed)?\b([^.#]{0,90}?)\b(\d{1,3})\s?(?:-|–|—|to|through)\s?(\d{1,3})\b"));

/// What a description says is collected: the words before the numbers, and the numbers.
fn collected(synopsis: &str) -> Option<(String, u32, u32)> {
    // Descriptions from Comic Vine are HTML.
    let text = TAG.replace_all(synopsis, " ").replace("&nbsp;", " ").replace("&#8211;", "–");
    if let Some(c) = COLLECTS.captures(&text) {
        let from = c[2].parse().ok()?;
        let to = c.get(3).map_or(Some(from), |t| t.as_str().parse().ok())?;
        return Some((c[1].to_string(), from, to));
    }
    let c = COLLECTS_RANGE.captures(&text)?;
    let (from, to) = (c[2].parse().ok()?, c[3].parse().ok()?);
    (from < to).then(|| (c[1].to_string(), from, to))
}

/// The issues a collection says it holds, from its description: "Collects SAGA #13-18" → "#13–18",
/// "Collecting Justice League 0-17" → "#0–17".
pub fn collects(synopsis: &str) -> Option<String> {
    let (_, from, to) = collected(synopsis)?;
    Some(if from == to { format!("#{from}") } else { format!("#{from}–{to}") })
}

static CHAPTER_HEAD: LazyLock<Regex> =
    LazyLock::new(|| re(r"<h\d[^>]*>[^<]*(?:chapter|contents|story titles|episode)[^<]*</h\d>\s*<[uo]l[^>]*>([\s\S]*?)</[uo]l>"));
static CHAPTER_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    re(r"^\s*(?:chapter|ch\.?|episode|ep\.?|file|act|lesson|part|night|round|story|stage|level|quest|#)\s*:?\s*(\d{1,4})\b")
});

/// The first and last chapter in a manga volume's chapter list.
fn chapter_numbers(synopsis: &str) -> Option<(String, String)> {
    let list = CHAPTER_HEAD.captures(synopsis)?;
    let numbers: Vec<String> = list[1]
        .split("<li")
        .skip(1)
        .filter_map(|li| {
            let text = TAG.replace_all(li.split_once('>').map_or("", |(_, t)| t), " ");
            CHAPTER_NUMBER.captures(&text).map(|c| c[1].to_string())
        })
        .collect();
    Some((numbers.first()?.clone(), numbers.last()?.clone()))
}

/// The chapters a manga volume holds, from the chapter list in its description: "Ch. 301–310".
pub fn chapters(synopsis: &str) -> Option<String> {
    let (first, last) = chapter_numbers(synopsis)?;
    Some(if first == last { format!("Ch. {first}") } else { format!("Ch. {first}–{last}") })
}

/// Which numbers a span counts in: a series' issues and its chapters are numbered separately.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Unit {
    Issue,
    Chapter,
}

/// The part of the story a book holds: one issue or chapter, or the run a collection says it holds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Span {
    pub unit: Unit,
    pub from: f64,
    pub to: f64,
}

impl Span {
    pub fn holds(&self, other: &Span) -> bool {
        self.unit == other.unit && self.from <= other.from && other.to <= self.to
    }
    pub fn meets(&self, other: &Span) -> bool {
        self.unit == other.unit && self.from <= other.to && other.from <= self.to
    }
}

/// Whether the words before a collection's numbers are about this series. "Collects #1-5" and
/// "collects issues 1-5 of the hit" are; "Collects Superman #1-5" in Action Comics isn't, and those
/// numbers mustn't be taken for this series' own.
fn names_series(words: &str, series: &str) -> bool {
    const PLAIN: [&str; 16] =
        ["the", "and", "issue", "issues", "comic", "comics", "series", "hit", "new", "original", "stories", "story", "first", "all", "from", "miniseries"];
    let split = |s: &str| NON_WORD.split(&s.to_lowercase()).filter(|w| w.len() > 2).map(str::to_string).collect::<Vec<_>>();
    let own = split(series);
    let named: Vec<String> = split(words).into_iter().filter(|w| !PLAIN.contains(&w.as_str())).collect();
    named.is_empty() || named.iter().any(|w| own.contains(w))
}

/// The part of the story a book holds, when that's known for certain. `n` is its number; annuals,
/// specials and collections that don't say have none.
pub fn span(kind: &str, n: Option<f64>, synopsis: &str, series: &str) -> Option<Span> {
    match kind {
        "issue" => n.map(|n| Span { unit: Unit::Issue, from: n, to: n }),
        "chapter" => n.map(|n| Span { unit: Unit::Chapter, from: n, to: n }),
        "volume" | "collection" | "deluxe" | "omnibus" => {
            if let Some((words, from, to)) = collected(synopsis) {
                return (from <= to && names_series(&words, series)).then_some(Span { unit: Unit::Issue, from: from as f64, to: to as f64 });
            }
            let (first, last) = chapter_numbers(synopsis)?;
            let (from, to) = (first.parse::<f64>().ok()?, last.parse::<f64>().ok()?);
            (from <= to).then_some(Span { unit: Unit::Chapter, from, to })
        }
        _ => None,
    }
}

/// What the story order needs to know about a book.
pub struct StoryBook {
    pub kind: &'static str,
    pub span: Option<Span>,
    /// When it came out: "2018-03-01", or just "2018".
    pub when: Option<String>,
}

/// One book's place in the story order: its index in the list given, and for an annual or special,
/// the index of the book it follows (None: it comes first, or nothing says where it goes).
#[derive(Debug, PartialEq)]
pub struct Stop {
    pub book: usize,
    pub after: Option<usize>,
}

/// The whole story in the order to read it, when that takes more than one format: the collections
/// and then the issues or chapters that carry on past them, with annuals and specials placed where
/// they came out. `books` is in the order the series page lists them. None when one format already
/// tells it all (or reading it means nothing more than going down that format's list).
///
/// A run of issues or chapters that starts no later than the collections and goes at least as far
/// tells the story by itself, so the collections stay out of it; so do collections that don't say
/// what they hold, unless the issues or chapters start past #1, and so carry on from them.
pub fn story_order(books: &[StoryBook]) -> Option<Vec<Stop>> {
    let of = |kinds: &[&str]| -> Vec<usize> { (0..books.len()).filter(|i| kinds.contains(&books[*i].kind)).collect() };
    let longer = |a: Vec<usize>, b: Vec<usize>| if b.len() > a.len() { b } else { a };
    let singles = longer(of(&["issue"]), of(&["chapter"]));
    // Deluxe editions and omnibuses are the same story on better paper: only the fullest format counts.
    let shelf = ["volume", "collection", "deluxe", "omnibus"].iter().fold(Vec::new(), |best, k| longer(best, of(&[k])));
    let unit = singles.first().and_then(|i| books[*i].span).map(|s| s.unit);
    let run = |list: &[usize], unit: Option<Unit>| -> Vec<Span> { list.iter().filter_map(|i| books[*i].span).filter(|s| unit.is_none_or(|u| s.unit == u)).collect() };
    let low = |spans: &[Span]| spans.iter().map(|s| s.from).fold(f64::INFINITY, f64::min);
    let high = |spans: &[Span]| spans.iter().map(|s| s.to).fold(f64::NEG_INFINITY, f64::max);
    let main: Vec<usize> = if shelf.is_empty() {
        singles.clone()
    } else if singles.is_empty() {
        shelf.clone()
    } else {
        let (told, loose) = (run(&shelf, unit), run(&singles, unit));
        if told.is_empty() {
            if low(&loose) > 1.0 { shelf.iter().chain(&singles).copied().collect() } else { singles.clone() }
        } else if low(&loose) <= low(&told) && high(&loose) >= high(&told) {
            singles.clone()
        } else {
            let past = high(&told);
            let after = singles.iter().copied().filter(|i| books[*i].span.is_some_and(|s| s.from > past));
            shelf.iter().copied().chain(after).collect()
        }
    };
    let extras = of(&["annual", "special"]);
    let mixed = !shelf.is_empty() && !singles.is_empty() && main.iter().any(|i| shelf.contains(i)) && main.iter().any(|i| singles.contains(i));
    // Dates compare as text. A bare year counts as that year's start among the main run, and its end
    // for an annual, which so comes after everything else that year.
    let at_start = |i: usize| books[i].when.as_deref().map(|w| if w.len() > 4 { w.to_string() } else { format!("{w}-00") });
    let at_end = |i: usize| books[i].when.as_deref().map(|w| if w.len() > 4 { w.to_string() } else { format!("{w}-13") });
    let dated = main.iter().any(|i| books[*i].when.is_some());
    // Worth saying only if formats join up, or there's a run long enough and a date to place something
    // in it. A special with no date only goes on the end, which is where it already is.
    let placeable = main.len() >= 3 && dated && extras.iter().any(|x| books[*x].when.is_some());
    if main.is_empty() || !(mixed || placeable) {
        return None;
    }
    // For each annual: the place in the main run it follows. usize::MAX is before the first.
    const BEFORE: usize = usize::MAX;
    let mut placed: Vec<(usize, usize)> = Vec::new();
    let mut last: Vec<usize> = Vec::new();
    for &x in &extras {
        match at_end(x).filter(|_| dated) {
            Some(w) => {
                let spot = main.iter().rposition(|i| at_start(*i).is_some_and(|m| m <= w)).unwrap_or(BEFORE);
                placed.push((spot, x));
            }
            None => last.push(x),
        }
    }
    placed.sort_by_key(|(spot, x)| (spot.wrapping_add(1), at_end(*x), *x));
    let mut out = Vec::with_capacity(main.len() + extras.len());
    let mut waiting = placed.iter().peekable();
    while let Some(&(_, x)) = waiting.next_if(|(spot, _)| *spot == BEFORE) {
        out.push(Stop { book: x, after: None });
    }
    for (k, &m) in main.iter().enumerate() {
        out.push(Stop { book: m, after: None });
        while let Some(&(_, x)) = waiting.next_if(|(spot, _)| *spot == k) {
            out.push(Stop { book: x, after: Some(m) });
        }
    }
    out.extend(last.into_iter().map(|x| Stop { book: x, after: None }));
    Some(out)
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

    fn issue(n: f64, when: Option<&str>) -> StoryBook {
        StoryBook { kind: "issue", span: span("issue", Some(n), "", ""), when: when.map(str::to_string) }
    }
    fn chapter(n: f64) -> StoryBook {
        StoryBook { kind: "chapter", span: span("chapter", Some(n), "", ""), when: None }
    }
    fn annual(when: Option<&str>) -> StoryBook {
        StoryBook { kind: "annual", span: None, when: when.map(str::to_string) }
    }
    fn shelf(kind: &'static str, unit: Unit, from: f64, to: f64) -> StoryBook {
        StoryBook { kind, span: Some(Span { unit, from, to }), when: None }
    }
    fn order(books: &[StoryBook]) -> Option<Vec<usize>> {
        story_order(books).map(|v| v.into_iter().map(|s| s.book).collect())
    }

    #[test]
    fn annuals_go_where_they_came_out() {
        let books = [issue(1.0, Some("2018-01-01")), issue(2.0, Some("2018-06-01")), issue(3.0, Some("2019-02-01")), annual(Some("2018-08-01"))];
        let got = story_order(&books).unwrap();
        assert_eq!(got.iter().map(|s| s.book).collect::<Vec<_>>(), [0, 1, 3, 2]);
        assert_eq!(got[2].after, Some(1));
        // With only the year known, after everything from that year.
        let books = [issue(1.0, Some("2018")), issue(2.0, Some("2018")), issue(3.0, Some("2019")), annual(Some("2018"))];
        assert_eq!(order(&books).unwrap(), [0, 1, 3, 2]);
        // Older than everything: first. Nothing to go on: last.
        let books = [issue(1.0, Some("2018-01-01")), issue(2.0, Some("2018-02-01")), issue(3.0, Some("2018-03-01")), annual(Some("2017-01-01")), annual(None)];
        assert_eq!(order(&books).unwrap(), [3, 0, 1, 2, 4]);
        // Nothing dated to place: nothing to say.
        let books = [issue(1.0, Some("2018-01-01")), issue(2.0, None), issue(3.0, None), annual(None)];
        assert_eq!(story_order(&books), None);
        // Too few books to call a run.
        assert_eq!(story_order(&[issue(1.0, Some("2018")), annual(Some("2017"))]), None);
    }

    #[test]
    fn a_bare_chapter_number_is_a_chapter() {
        assert_eq!(kind(true, "", "ch001.cbz", 20), "chapter");
        assert_eq!(number("chapter", "ch001.cbz", "Modulo"), Some(1.0));
    }

    #[test]
    fn chapters_carry_on_past_the_volumes() {
        let books = [shelf("volume", Unit::Chapter, 1.0, 10.0), shelf("volume", Unit::Chapter, 11.0, 20.0), chapter(5.0), chapter(21.0), chapter(22.0)];
        assert_eq!(order(&books).unwrap(), [0, 1, 3, 4]);
    }

    #[test]
    fn a_full_run_needs_nothing_else() {
        // Every issue, and trades of some: the issues alone tell it, in one format.
        let books = [shelf("collection", Unit::Issue, 1.0, 6.0), issue(1.0, None), issue(2.0, None), issue(3.0, None), issue(6.0, None), issue(7.0, None)];
        assert_eq!(story_order(&books), None);
        // One format and nothing to place in it.
        assert_eq!(story_order(&[issue(1.0, None), issue(2.0, None)]), None);
        // Only annuals: no run to place them in.
        assert_eq!(story_order(&[annual(None)]), None);
    }

    #[test]
    fn volumes_that_dont_say_are_followed_only_by_later_books() {
        let vol = |_| StoryBook { kind: "volume", span: None, when: None };
        // The chapters start past #1, so they carry on from the volumes.
        let books = [vol(1), vol(2), chapter(40.0), chapter(41.0)];
        assert_eq!(order(&books).unwrap(), [0, 1, 2, 3]);
        // They start at #1: the same story twice, so the chapters alone tell it.
        let books = [vol(1), vol(2), chapter(1.0), chapter(2.0)];
        assert_eq!(story_order(&books), None);
    }

    #[test]
    fn spans_trust_only_what_names_the_series() {
        let got = |syn: &str, series: &str| span("collection", None, syn, series);
        let run = Some(Span { unit: Unit::Issue, from: 13.0, to: 18.0 });
        assert_eq!(got("Collects SAGA #13-18.", "Saga"), run);
        assert_eq!(got("collecting issues #13-18 of the hit", "Saga"), run);
        assert_eq!(got("Collects Superman #13-18", "Action Comics"), None);
        assert_eq!(got("A new series begins!", "Saga"), None);
        let list = "<h2>Chapter Titles</h2><ul><li>Chapter 301: Memory</li><li>Chapter 310: End</li></ul>";
        assert_eq!(span("volume", None, list, "Bleach"), Some(Span { unit: Unit::Chapter, from: 301.0, to: 310.0 }));
        assert_eq!(span("annual", Some(1.0), "Collects #1-2", "Saga"), None);
    }

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
        assert_eq!(number("volume", "Saga Vol. 11.cbz", "Saga"), Some(11.0));
        assert_eq!(number("issue", "Titans 019 (2018) (2 covers) (digital).cbz", "Titans"), Some(19.0));
        assert_eq!(number("issue", "Titans 013 (2017) GetComics.INFO (1).cbz", "Titans"), Some(13.0));
        assert_eq!(number("annual", "Titans Annual 002 (2018) (digital).cbz", "Titans"), Some(2.0));
        assert_eq!(number("annual", "Absolute Batman 2025 Annual 001 (2025).cbz", "Absolute Batman"), Some(1.0));
        assert_eq!(number("chapter", "Solo Leveling 000 - Prologue (2020).cbz", "Solo Leveling"), Some(0.0));
        assert_eq!(number("volume", "Teen Titans by Geoff Johns Book 02 (2018).cbz", "Teen Titans"), Some(2.0));
        assert_eq!(number("volume", "Kokou no Hito - Volume_04.cbz", "Kokou no Hito"), Some(4.0));
        // A number in the series' own name isn't the book's.
        assert_eq!(number("issue", "Ben 10 003 (2026) (Series) (Dynamite Entertainment) (Digital).cbz", "Ben 10"), Some(3.0));
        assert_eq!(number("issue", "Doom 2099 001 (2020) (Digital) (Zone-Empire).cbz", "Doom 2099"), Some(1.0));
        assert_eq!(number("issue", "Spider-Man 2099 004 (2015).cbz", "Spider Man 2099 (2015)"), Some(4.0));
        assert_eq!(number("issue", "Injustice 2 012 (2017).cbr", "Injustice 2"), Some(12.0));
        assert_eq!(number("issue", "Marvel 1602 (2010) (Digital).cbz", "Marvel 1602"), None);
        // Nor is a date.
        assert_eq!(number("volume", "Justice League by Alex Ross 2018-12-19 (TPB) (digital).cbz", "Justice League"), None);
        // A reading order's place comes first in the name.
        assert_eq!(order_number("019 Siege - Storming Asgard (2010).cbz", "Siege"), Some(19.0));
        assert_eq!(order_number("Siege 001 (2010).cbr", "Siege"), None);
        assert_eq!(order_number("100 Bullets - Book 01 (2014).cbr", "100 Bullets"), None);
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
        assert_eq!(collects("Paperback collecting Justice League 0-17, and more").as_deref(), Some("#0–17"));
        assert_eq!(collects("Collects Concrete 6-10; stories from Dark Horse Presents").as_deref(), Some("#6–10"));
        assert_eq!(collects("reprints issues #25-30 of the series").as_deref(), Some("#25–30"));
        assert_eq!(collects("collects the many titles from the 2005 event"), None);
        assert_eq!(collects("Collecting the complete four-issue miniseries"), None);
        assert_eq!(gaps(&[19.0, 20.0, 22.0, 23.0]), vec![21]);
        let hxh = "<p>Blurb.</p><h2>Chapter Titles</h2><ul><li>Chapter 301: Memory</li><li>Chapter 310: Action</li></ul>";
        assert_eq!(chapters(hxh).as_deref(), Some("Ch. 301–310"));
        let aot = "<h2>Contents</h2><ul><li>Episode 79: Perfect Game</li><li>Episode 82: Hero</li></ul><h4>Covers</h4>";
        assert_eq!(chapters(aot).as_deref(), Some("Ch. 79–82"));
        assert_eq!(chapters("<h2>Chapter List</h2><ul><li>第三百一訓 侠の鎖</li></ul>"), None);
        assert_eq!(chapters("<h2>Back Blurb</h2><ul><li>Chapter 1</li></ul>"), None);
    }
}
