//! How comics get into the library: watching the comics folder for files added outside the app,
//! and copying comic files dropped onto the window into the right folder.
//!
//! Dropping is the one time AionComics writes to the comics folder, and only ever adds: files are
//! copied under their own names (the originals stay where they were), and nothing there is
//! overwritten, renamed, moved or deleted. How a book is called in the app comes from its
//! metadata, never from changing the file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};

use anyhow::{bail, Result};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

const EXTENSIONS: [&str; 4] = ["cbz", "cbr", "zip", "rar"];

fn is_comic(p: &Path) -> bool {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).is_some_and(|e| EXTENSIONS.contains(&e.as_str()))
}

// ---------- watching ----------

/// When something in the comics folder last changed (ms since the epoch), 0 when nothing has.
pub struct Changes(AtomicI64);

impl Changes {
    pub fn new() -> Arc<Self> {
        Arc::new(Changes(AtomicI64::new(0)))
    }
    pub fn touch(&self) {
        self.0.store(now_ms(), Ordering::SeqCst);
    }
    /// True once, when changes have been quiet for `quiet_ms` (a copy in progress keeps changing).
    pub fn settled(&self, quiet_ms: i64) -> bool {
        let t = self.0.load(Ordering::SeqCst);
        t > 0 && now_ms() - t >= quiet_ms && self.0.compare_exchange(t, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Watch `root` for comics (and folders) appearing, disappearing or being renamed. Hidden folders
/// such as YACReader's `.yacreaderlibrary` are ignored. Dropping the returned watcher stops it.
pub fn watch(root: &Path, changes: Arc<Changes>) -> Result<notify::RecommendedWatcher> {
    use notify::{EventKind, RecursiveMode, Watcher};
    let base = root.to_path_buf();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, EventKind::Access(_)) {
            return;
        }
        let relevant = ev.paths.iter().any(|p| {
            let hidden = p.strip_prefix(&base).ok().is_some_and(|r| r.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')));
            // A folder has no extension; comic files have theirs.
            !hidden && (is_comic(p) || p.extension().is_none())
        });
        if relevant {
            changes.touch();
        }
    })?;
    w.watch(root, RecursiveMode::Recursive)?;
    Ok(w)
}

// ---------- dropping files in ----------

/// Where a dropped file will go, worked out before anything is copied.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub source: String,
    pub file_name: String,
    /// Folder inside the comics folder ("DC/Absolute Batman"), when one could be chosen.
    pub folder: Option<String>,
    /// "add", "exists" (already there), "name-taken" (a different file has that name there; left
    /// alone rather than renaming either), "new-series" (a folder will be made), "no-place" (open
    /// a collection first), or "not-comic".
    pub status: &'static str,
}

static NUMBER_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\s_.-]+(?:v(?:ol(?:ume)?)?\.?\s*\d+(?:\.\d+)?|#?\d{1,4}(?:\.\d+)?)(?:[\s_.-]+.*)?$").unwrap()
});

/// The series a file name belongs to: "Absolute Batman 018 (2026) (Digital).cbz" → "Absolute Batman".
pub fn series_guess(file_name: &str) -> String {
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let clean = crate::scan::clean_name(&stem.replace('_', " "));
    let cut = NUMBER_TAIL.replace(&clean, "").trim().to_string();
    if cut.is_empty() { clean } else { cut }
}

fn norm(s: &str) -> String {
    let l = crate::scan::clean_name(s).to_lowercase().replace('&', " and ");
    let w: String = l.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let words: Vec<&str> = w.split_whitespace().collect();
    let words = if words.first() == Some(&"the") { &words[1..] } else { &words[..] };
    words.join(" ")
}

/// A folder name Windows accepts.
fn safe_folder(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim_end_matches(['.', ' ']).to_string()
}

/// Folder of a series: its own, or the collection's for a single book sitting there.
fn series_folder(rel: &str, root: &Path) -> String {
    if is_comic(Path::new(rel)) || root.join(rel).is_file() {
        rel.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default()
    } else {
        rel.to_string()
    }
}

/// Where each dropped path goes. `series` is the series page you dropped on, `collection` the
/// collection page; with neither, a file goes to the series of the same name anywhere.
pub fn plan(conn: &Connection, root: &Path, paths: &[String], series: Option<i64>, collection: Option<i64>) -> Result<Vec<Placement>> {
    let target_series: Option<String> = match series {
        Some(id) => conn.query_row("SELECT rel_path FROM series WHERE id=?1", [id], |r| r.get(0)).optional()?,
        None => None,
    };
    let collection_name: Option<String> = match collection {
        Some(id) => conn.query_row("SELECT name FROM collections WHERE id=?1", [id], |r| r.get(0)).optional()?,
        None => None,
    };
    // Existing series: (collection, normalised name, folder).
    let mut st = conn.prepare("SELECT col.name, s.name, s.rel_path FROM series s JOIN collections col ON col.id=s.collection_id")?;
    let known: Vec<(String, String, String)> = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(c, n, rel)| (c, norm(&n), series_folder(&rel, root)))
        .collect();
    let mut out = Vec::new();
    for src in paths {
        let p = PathBuf::from(src);
        let file_name = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let place = |folder: Option<String>, status| Placement { source: src.clone(), file_name: file_name.clone(), folder, status };
        if !p.is_file() || !is_comic(&p) {
            out.push(place(None, "not-comic"));
            continue;
        }
        let guess = series_guess(&file_name);
        let same = |c: &Option<String>| {
            known.iter().find(|(col, n, _)| *n == norm(&guess) && c.as_ref().is_none_or(|c| c == col)).map(|k| k.2.clone())
        };
        let (folder, new) = if let Some(rel) = &target_series {
            (Some(series_folder(rel, root)), false)
        } else if let Some(col) = &collection_name {
            match same(&collection_name) {
                Some(f) => (Some(f), false),
                None => (Some(format!("{col}/{}", safe_folder(&guess))), true),
            }
        } else {
            (same(&None), false)
        };
        let Some(folder) = folder.filter(|f| !f.is_empty()) else {
            out.push(place(None, "no-place"));
            continue;
        };
        let dest = root.join(&folder).join(&file_name);
        let status = if dest.exists() {
            if dest.metadata().map(|m| m.len()).ok() == p.metadata().map(|m| m.len()).ok() { "exists" } else { "name-taken" }
        } else if new && !root.join(&folder).exists() {
            "new-series"
        } else {
            "add"
        };
        out.push(place(Some(folder), status));
    }
    Ok(out)
}

/// Copy the placed files in. Returns how many were added.
pub fn copy_in(root: &Path, placements: &[Placement]) -> Result<usize> {
    let root_real = std::fs::canonicalize(root)?;
    let mut added = 0;
    for p in placements {
        let ("add" | "new-series", Some(folder)) = (p.status, &p.folder) else { continue };
        if folder.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            bail!("Refusing an odd destination: {folder}");
        }
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir)?;
        // Belt and braces: the folder really is inside the comics folder.
        if !std::fs::canonicalize(&dir)?.starts_with(&root_real) {
            bail!("Refusing to copy outside the comics folder");
        }
        let dest = dir.join(&p.file_name);
        // Something got there since the plan was made: leave it be.
        if dest.exists() {
            continue;
        }
        // Copy under a temporary name, so a half-copied file is never scanned as a comic.
        let part = dest.with_extension("aion-part");
        std::fs::copy(&p.source, &part)?;
        std::fs::rename(&part, &dest)?;
        added += 1;
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_series() {
        assert_eq!(series_guess("Absolute Batman 018 (2026) (Digital) (Pyrate-DCP).cbz"), "Absolute Batman");
        assert_eq!(series_guess("Vinland Saga v12 (2021) (Digital).cbz"), "Vinland Saga");
        assert_eq!(series_guess("Saga 054 - The War for Phang.cbr"), "Saga");
        assert_eq!(series_guess("Batman - The Long Halloween.cbz"), "Batman - The Long Halloween");
        assert_eq!(norm("The Walking Dead"), norm("Walking Dead"));
        assert_eq!(safe_folder("What If? Spider: Man."), "What If Spider Man");
    }

    #[test]
    fn notices_new_comics() {
        let root = std::env::temp_dir().join(format!("aion-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".yacreaderlibrary")).unwrap();
        let changes = Changes::new();
        let _w = watch(&root, changes.clone()).unwrap();
        let wait = || std::thread::sleep(std::time::Duration::from_millis(600));
        std::fs::write(root.join(".yacreaderlibrary/library.ydb"), b"x").unwrap();
        std::fs::write(root.join("notes.txt"), b"x").unwrap();
        wait();
        assert!(!changes.settled(100), "hidden folders and other files don't count");
        std::fs::write(root.join("Saga 001.cbz"), b"x").unwrap();
        wait();
        assert!(changes.settled(100));
        assert!(!changes.settled(100), "reported once");
        drop(_w);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copies_without_overwriting() {
        let base = std::env::temp_dir().join(format!("aion-intake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, src) = (base.join("Comics"), base.join("Downloads"));
        std::fs::create_dir_all(root.join("DC/Absolute Batman")).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(root.join("DC/Absolute Batman/Absolute Batman 018.cbz"), b"old").unwrap();
        std::fs::write(src.join("Absolute Batman 018.cbz"), b"new one").unwrap();
        std::fs::write(src.join("Absolute Batman 019.cbz"), b"nineteen").unwrap();
        let place = |name: &str, status| Placement {
            source: src.join(name).to_string_lossy().into_owned(),
            file_name: name.into(),
            folder: Some("DC/Absolute Batman".into()),
            status,
        };
        // A different file with the same name: planned as taken, and never copied or renamed.
        let dest = root.join("DC/Absolute Batman/Absolute Batman 018.cbz");
        assert!(dest.exists());
        assert_eq!(copy_in(&root, &[place("Absolute Batman 018.cbz", "add")]).unwrap(), 0, "never overwrites");
        assert_eq!(std::fs::read(&dest).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(root.join("DC/Absolute Batman")).unwrap().count(), 1, "nothing renamed or added");
        // A new one goes in under its own name; the original stays.
        assert_eq!(copy_in(&root, &[place("Absolute Batman 019.cbz", "add")]).unwrap(), 1);
        assert_eq!(std::fs::read(root.join("DC/Absolute Batman/Absolute Batman 019.cbz")).unwrap(), b"nineteen");
        assert!(src.join("Absolute Batman 019.cbz").exists(), "the original stays");
        assert_eq!(copy_in(&root, &[place("Absolute Batman 018.cbz", "name-taken")]).unwrap(), 0);
        let mut bad = place("Absolute Batman 019.cbz", "add");
        bad.folder = Some("../Downloads".into());
        assert!(copy_in(&root, &[bad]).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
