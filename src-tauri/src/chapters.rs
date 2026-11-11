//! Where each chapter of a book starts, read from how its pages are named. Most digital manga
//! volumes say it: each page's file name carries its chapter ("Radiant - c013 (v03) - p000.jpg",
//! "Gantz_ch276_p020.png"), or each chapter is a folder of its own ("Chapter 421 - Herculopolis
//! High/002.jpg"). A book that marks fewer than two chapters has none: nothing is guessed from the
//! art.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Chapter {
    /// The page it starts on, counted from 0.
    pub page: usize,
    /// As the book writes it, without leading zeros: "13", "105.5".
    pub number: String,
    /// When its folder names it: "Herculopolis High".
    pub title: Option<String>,
}

/// A chapter in a page's file name: "c013", "ch276", "Ch. 12", "chapter 4", set off from what's
/// around it ("c041#2" is the second part of chapter 41); or a bare number just before the volume, "Berserk - 012 (v04) - p118".
static IN_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s_\-.\[])(?:chapter|chap\.?|ch\.?|c)\s?0*(\d{1,4}(?:\.\d+)?)(?:#\d+)?(?:[\s_\-(\[.]|$)|\s-\s0*(\d{1,4}(?:\.\d+)?)\s\(v\d+\)").unwrap()
});

/// A folder that is a chapter: "Chapter 421 - Herculopolis High", "Ch.0155 - Are you dead yet (gb)
/// [Group]", "Haikyu Ch 345".
static IN_FOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s_\-])(?:chapter|chap\.?|ch\.?)\s*0*(\d{1,4}(?:\.\d+)?)(?:\s*[-:–]\s*([^\[(]+?))?\s*(?:[\[(].*)?$").unwrap()
});

fn number_in_name(name: &str) -> Option<String> {
    let c = IN_NAME.captures(name)?;
    Some(c.get(1).or(c.get(2))?.as_str().to_string())
}

fn chapter_folder(path: &str) -> Option<(String, Option<String>)> {
    let folder = path.rsplit_once(['/', '\\'])?.0;
    let last = folder.rsplit(['/', '\\']).next()?;
    let c = IN_FOLDER.captures(last.trim())?;
    // A colon can't go in a folder name, so "Stage One: Focus" is saved as "Stage One_ Focus".
    let title = c.get(2).map(|t| t.as_str().trim().replace("_ ", ": ")).filter(|t| !t.is_empty());
    Some((c[1].to_string(), title))
}

/// Where chapters start, given the book's pages in reading order.
pub fn find(pages: &[String]) -> Vec<Chapter> {
    let by_name: Vec<Option<(String, Option<String>)>> = pages
        .iter()
        .map(|p| number_in_name(p.rsplit(['/', '\\']).next().unwrap_or(p)).map(|n| (n, None)))
        .collect();
    let by_folder: Vec<Option<(String, Option<String>)>> = pages.iter().map(|p| chapter_folder(p)).collect();
    let from_names = starts(&by_name);
    let from_folders = starts(&by_folder);
    // Folders name their chapters, so they win when both say the same number of chapters.
    let mut best = if from_folders.len() >= from_names.len() { from_folders } else { from_names };
    if best.len() < 2 {
        best.clear();
    }
    best
}

/// Each run of pages with the same chapter, its first page. Numbers have to go up: a book whose
/// marks jump back and forth isn't marked in a way to trust.
fn starts(marks: &[Option<(String, Option<String>)>]) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = Vec::new();
    for (page, mark) in marks.iter().enumerate() {
        let Some((number, title)) = mark else { continue };
        if out.last().is_some_and(|c| &c.number == number) {
            continue;
        }
        let value = |n: &str| n.parse::<f64>().unwrap_or(f64::NAN);
        if out.last().is_some_and(|c| !(value(number) > value(&c.number))) {
            return Vec::new();
        }
        out.push(Chapter { page, number: number.clone(), title: title.clone() });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn chapters_from_page_names() {
        let pages = names(&[
            "Radiant - c013 (v03) - p000 [VIZ Media] [Digital] [1r0n].jpg",
            "Radiant - c013 (v03) - p001 [VIZ Media] [Digital] [1r0n].jpg",
            "Radiant - c014 (v03) - p030 [VIZ Media] [Digital] [1r0n].jpg",
            "Radiant - c014 (v03) - p031 [VIZ Media] [Digital] [1r0n].jpg",
            "Radiant - c015.5 (v03) - p060 [VIZ Media] [Digital] [1r0n].jpg",
        ]);
        let found = find(&pages);
        assert_eq!(found.iter().map(|c| (c.page, c.number.as_str())).collect::<Vec<_>>(), [(0, "13"), (2, "14"), (4, "15.5")]);
    }

    #[test]
    fn the_ways_chapters_are_written() {
        for (name, want) in [
            ("Gantz_ch276_p020[Whatever].png", "276"),
            ("Kingdom---c0432-v040---p100-Fan-Scans-Turnip-Famers.png", "432"),
            ("Hajime no Ippo v56 c508 - 087.png", "508"),
            ("Berserk - 012 (v04) - p118 [Digital] [danke-repack].png", "12"),
            ("Solo Leveling - c089 (NA) - p016 [web] [Tappytoon] [LuCaZ].jpg", "89"),
            ("Choujin X - c041#2 (v08) - p106 [VIZ Media] [Digital] [1r0n].png", "41"),
        ] {
            assert_eq!(number_in_name(name).as_deref(), Some(want), "{name}");
        }
        for name in ["Case Closed v42-099.jpg", "jjl_v24_089.jpg", "Slam Dunk V21, Pg 0019.jpg", "Batman 001 (c2c).jpg", "0101_0001.jpg", "Claymore v5-101.jpg"] {
            assert_eq!(number_in_name(name), None, "{name}");
        }
    }

    #[test]
    fn chapters_from_folders_keep_their_titles() {
        let pages = names(&[
            "Dragon Ball v36/Chapter 421 - Herculopolis High/001.jpg",
            "Dragon Ball v36/Chapter 421 - Herculopolis High/002.jpg",
            "Dragon Ball v36/Chapter 422 - Let the Training Begin!/001.jpg",
            "Kingdom v68/Ch.0736 - An Important Castle (en) [Sense Scans]/000.jpg",
        ]);
        let found = find(&pages[..3]);
        assert_eq!(found[0], Chapter { page: 0, number: "421".into(), title: Some("Herculopolis High".into()) });
        assert_eq!(found[1].title.as_deref(), Some("Let the Training Begin!"));
        assert_eq!(chapter_folder(&pages[3]), Some(("736".into(), Some("An Important Castle".into()))));
        assert_eq!(chapter_folder("Haikyu v39/Haikyu Ch 345/010.jpg"), Some(("345".into(), None)));
        assert_eq!(chapter_folder("Chainsaw Man v01/001.jpg"), None);
    }

    /// Chapters found across the manga in a real library, and a few of them to check by eye:
    /// `AION_TEST_ROOT=H:\Comics cargo test --lib real_chapters -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_chapters() {
        let root = std::path::PathBuf::from(std::env::var("AION_TEST_ROOT").unwrap()).join("Manga");
        let books: Vec<_> = walkdir::WalkDir::new(&root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("cbz") || x.eq_ignore_ascii_case("cbr")))
            .collect();
        let (mut with, mut seen) = (0, 0);
        for (k, e) in books.iter().enumerate().filter(|(k, _)| k % 7 == 0) {
            let Ok(listing) = crate::archive::list(e.path()) else { continue };
            seen += 1;
            let found = find(&listing.pages);
            if !found.is_empty() {
                with += 1;
            }
            if k % 140 == 0 {
                let name = e.file_name().to_string_lossy();
                let list: Vec<_> = found.iter().map(|c| format!("{}@{}{}", c.number, c.page, c.title.as_ref().map(|t| format!(" {t}")).unwrap_or_default())).collect();
                println!("{name}: {} pages, {}", listing.pages.len(), if list.is_empty() { "none".into() } else { list.join(", ") });
            }
        }
        println!("{with} of {seen} books have chapters");
    }

    #[test]
    fn one_chapter_or_muddled_marks_are_none() {
        assert!(find(&names(&["X - c001 - p01.jpg", "X - c001 - p02.jpg"])).is_empty());
        assert!(find(&names(&["X - c002 - p01.jpg", "X - c001 - p02.jpg", "X - c003 - p03.jpg"])).is_empty());
        assert!(find(&names(&["001.jpg", "002.jpg"])).is_empty());
    }
}
