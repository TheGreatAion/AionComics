//! How comics get into the library: watching the comics folder for files added outside the app,
//! and copying comic files dropped onto the window into the right folder.
//!
//! Dropping is the one time AionComics writes to the comics folder, and only ever adds: files are
//! copied under their own names (the originals stay where they were), and nothing there is
//! overwritten, renamed, moved or deleted. How a book is called in the app comes from its
//! metadata, never from changing the file.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};

use anyhow::{bail, Result};
use regex::Regex;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::archive::EXTENSIONS;

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

/// Where one book's bytes come from.
#[derive(Clone, Debug)]
pub enum Source {
    /// A comic file, copied as it is.
    File(PathBuf),
    /// A comic file inside a .zip of several, unpacked.
    Zipped { zip: PathBuf, entry: String },
    /// A comic file inside a .rar of several, unpacked.
    Rarred { rar: PathBuf, entry: String },
    /// A comic file inside a .7z of several, unpacked.
    SevenZipped { arc: PathBuf, entry: String },
    /// A folder of loose pages, packed into a .cbz.
    Pages(PathBuf),
}

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
    /// "copy", "unpack" (out of a .zip or .rar of several books) or "pack" (a folder of pages).
    pub how: &'static str,
    /// How many bytes it takes, when known.
    pub size: u64,
    #[serde(skip)]
    pub from: Option<Source>,
}

static NUMBER_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\s_.-]+(?:v(?:ol(?:ume)?)?\.?\s*\d+(?:\.\d+)?|#?\d{1,4}(?:\.\d+)?)(?:[\s_.-]+.*)?$").unwrap()
});

/// Two numbers in a row: the last is the issue, the first part of the name ("Spider-Man 2099 001").
static TWO_NUMBERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*\s\d{1,4})[\s_.-]+#?\d{1,4}(?:[\s_.-]+.*)?$").unwrap());

/// A folder's path in a form Windows won't trim: without a slash on the end, it quietly drops spaces
/// from the end of the last name, and so means, or makes, a different folder.
pub(crate) fn as_folder(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(std::path::MAIN_SEPARATOR_STR);
    s.into()
}

/// The series a file name belongs to: "Absolute Batman 018 (2026) (Digital).cbz" → "Absolute Batman".
pub fn series_guess(file_name: &str) -> String {
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let clean = crate::scan::clean_name(&stem.replace('_', " "));
    if let Some(c) = TWO_NUMBERS.captures(&clean) {
        return c[1].trim().to_string();
    }
    let cut = NUMBER_TAIL.replace(&clean, "").trim().to_string();
    if cut.is_empty() { clean } else { cut }
}

static NUMBER_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:v|vol|volume|ch|chap|chapter|issue|part|book|ep|episode|no|tome|digital)\.?$").unwrap());

/// A guess that names no series: "v01", "Chapter 3", "01".
pub(crate) fn says_nothing(guess: &str) -> bool {
    !guess
        .split(|c: char| !c.is_alphabetic() && c != '.')
        .any(|w| w.chars().filter(|c| c.is_alphabetic()).count() >= 2 && !NUMBER_WORD.is_match(w))
}

pub(crate) fn norm(s: &str) -> String {
    let l = crate::scan::clean_name(s).to_lowercase().replace('&', " and ");
    let w: String = l.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let words: Vec<&str> = w.split_whitespace().collect();
    let words = if words.first() == Some(&"the") { &words[1..] } else { &words[..] };
    words.join(" ")
}

/// A folder name Windows accepts.
pub(crate) fn safe_folder(name: &str) -> String {
    let s: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // A leading dot would hide the folder, and a name of only dots isn't a name.
    s.trim_matches(['.', ' ']).to_string()
}

/// Folder of a series: its own, or the collection's for a single book sitting there. A single book
/// straight in the comics folder gets a folder for its series when more arrive; it joins them there
/// by name (see `scan::lay_out`).
fn series_folder(rel: &str, root: &Path) -> String {
    if is_comic(Path::new(rel)) || root.join(rel).is_file() {
        rel.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_else(|| safe_folder(&series_guess(rel)))
    } else {
        rel.to_string()
    }
}

/// One book found in what was dropped, and the folder it sat in (the series it likely belongs
/// to when its own name doesn't say).
pub struct Found {
    from: Source,
    source: String,
    file_name: String,
    size: u64,
    hint: Option<String>,
}

fn last_part(path: &str) -> &str {
    path.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(path)
}

fn skipped_entry(name: &str) -> bool {
    name.split('/').any(|p| p == "__MACOSX" || p.starts_with('.'))
}

/// The comic files inside a .zip, .rar or .7z, when it's a bundle of books rather than a book itself.
/// A book can have a stray PDF or the like among its pages, so one with many more pictures than comic
/// files is a book.
fn bundled(p: &Path) -> Vec<Found> {
    let shown = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = p.file_stem().map(|f| f.to_string_lossy().into_owned());
    let found = |entry: String, size: u64, from: Source| {
        let (dir, file) = entry.rsplit_once('/').unwrap_or(("", &entry));
        Found {
            source: format!("{shown} › {file}"),
            file_name: file.to_string(),
            size,
            hint: if dir.is_empty() { stem.clone() } else { Some(last_part(dir).to_string()) },
            from,
        }
    };
    let mut out = Vec::new();
    let mut pictures = 0;
    match crate::archive::detect(p) {
        Ok(crate::archive::Kind::Zip) => {
            let Ok(mut z) = crate::archive::open_zip(p) else { return out };
            for i in 0..z.len() {
                let Ok(f) = z.by_index(i) else { continue };
                let Ok(name) = f.name().map(|n| n.replace('\\', "/")) else { continue };
                pictures += usize::from(crate::archive::is_image(&name));
                if !f.is_dir() && is_comic(Path::new(&name)) && !skipped_entry(&name) {
                    out.push(found(name.clone(), f.size(), Source::Zipped { zip: p.to_path_buf(), entry: name }));
                }
            }
        }
        Ok(crate::archive::Kind::Rar) => {
            let Ok(arc) = unrar::Archive::new(p).open_for_listing() else { return out };
            for e in arc.flatten() {
                let name = e.filename.to_string_lossy().replace('\\', "/");
                pictures += usize::from(crate::archive::is_image(&name));
                if e.is_file() && is_comic(Path::new(&name)) && !skipped_entry(&name) {
                    out.push(found(name.clone(), e.unpacked_size, Source::Rarred { rar: p.to_path_buf(), entry: name }));
                }
            }
        }
        Ok(crate::archive::Kind::SevenZ) => {
            let Ok(arc) = sevenz_rust2::Archive::open(p) else { return out };
            for e in arc.files.iter().filter(|e| !e.is_directory()) {
                let name = e.name().replace('\\', "/");
                pictures += usize::from(crate::archive::is_image(&name));
                if is_comic(Path::new(&name)) && !skipped_entry(&name) {
                    out.push(found(name, e.size(), Source::SevenZipped { arc: p.to_path_buf(), entry: e.name().to_string() }));
                }
            }
        }
        _ => {}
    }
    if pictures > out.len() + 5 {
        out.clear();
    }
    out
}

/// Every book in what was dropped: comic files as they are, the books inside a .zip or .rar of
/// several, and folders (at any depth) holding their comic files or a book's loose pages.
pub fn gather(src: &str) -> Vec<Found> {
    let p = PathBuf::from(src);
    let file = |p: &Path, hint: Option<String>| {
        let inner = bundled(p);
        if !inner.is_empty() {
            return inner;
        }
        vec![Found {
            from: Source::File(p.to_path_buf()),
            source: p.to_string_lossy().into_owned(),
            file_name: p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            size: p.metadata().map(|m| m.len()).unwrap_or(0),
            hint,
        }]
    };
    if p.is_file() {
        let folder = p.parent().map(|d| last_part(&d.to_string_lossy()).to_string());
        return if is_comic(&p) { file(&p, folder) } else { Vec::new() };
    }
    if !p.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let walk = walkdir::WalkDir::new(&p).max_depth(8).sort_by(|a, b| natord::compare_ignore_case(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy()));
    for e in walk.into_iter().filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.')).flatten() {
        let here = e.path();
        // The folder it sits in, as long as that's part of what was dropped.
        let parent = || (e.depth() > 0).then(|| here.parent().map(|d| last_part(&d.to_string_lossy()).to_string())).flatten();
        if e.file_type().is_file() {
            if is_comic(here) {
                out.extend(file(here, parent()));
            }
            continue;
        }
        // A folder of pages and nothing else is one book.
        let Ok(items) = std::fs::read_dir(here) else { continue };
        let (mut pages, mut size, mut comics) = (0, 0, false);
        for i in items.flatten() {
            let n = i.file_name().to_string_lossy().into_owned();
            if i.path().is_file() && crate::archive::is_image(&n) {
                pages += 1;
                size += i.metadata().map(|m| m.len()).unwrap_or(0);
            } else if is_comic(&i.path()) {
                comics = true;
            }
        }
        if pages >= 2 && !comics {
            let name = safe_folder(&e.file_name().to_string_lossy());
            out.push(Found {
                from: Source::Pages(here.to_path_buf()),
                source: here.to_string_lossy().into_owned(),
                file_name: format!("{name}.cbz"),
                size,
                hint: (e.depth() > 0).then(|| here.parent().map(|d| last_part(&d.to_string_lossy()).to_string())).flatten(),
            });
        }
    }
    out
}

/// Every dropped path with the books in it, looked for before the database is touched: a big folder
/// takes a while, and the app stays usable meanwhile.
pub fn gather_all(paths: &[String]) -> Vec<(String, Vec<Found>)> {
    paths.iter().map(|p| (p.clone(), gather(p))).collect()
}

/// Where each dropped path goes. `series` is the series page you dropped on, `collection` the
/// collection page; with neither, a file goes to the series of the same name anywhere. `name` puts
/// everything for a collection into the one series of that name, whatever the files are called.
pub fn plan(
    conn: &Connection,
    root: &Path,
    found: Vec<(String, Vec<Found>)>,
    series: Option<i64>,
    collection: Option<i64>,
    name: Option<&str>,
) -> Result<Vec<Placement>> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let target_series: Option<String> = match series {
        Some(id) => conn.query_row("SELECT rel_path FROM series WHERE id=?1", [id], |r| r.get(0)).optional()?,
        None => None,
    };
    let chosen: Option<(String, bool)> = match collection {
        Some(id) => conn.query_row("SELECT name, at_top FROM collections WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?,
        None => None,
    };
    let collection_name = chosen.as_ref().map(|c| c.0.clone());
    // A new series' folder: in its library's folder, or straight in the comics folder for the library
    // that has none of its own, or when there are no libraries yet.
    let in_library = |safe: String| match &chosen {
        Some((col, false)) => format!("{col}/{safe}"),
        _ => safe,
    };
    let no_libraries = conn.query_row("SELECT COUNT(*) FROM collections", [], |r| r.get::<_, i64>(0))? == 0;
    // Existing series: (collection, normalized name, folder).
    let mut st = conn.prepare("SELECT col.name, s.name, s.rel_path FROM series s JOIN collections col ON col.id=s.collection_id")?;
    let known: Vec<(String, String, String)> = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(c, n, rel)| (c, norm(&n), series_folder(&rel, root)))
        .collect();
    let root_real = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut planned = std::collections::HashMap::<PathBuf, u64>::new();
    let mut new_series = std::collections::HashMap::<String, String>::new();
    let mut out = Vec::new();
    for (src, books) in found {
        if books.is_empty() {
            let file_name = last_part(&src).to_string();
            out.push(Placement { source: src.clone(), file_name, folder: None, status: "not-comic", how: "copy", size: 0, from: None });
            continue;
        }
        for b in books {
            let how = match b.from {
                Source::File(_) => "copy",
                Source::Zipped { .. } | Source::Rarred { .. } | Source::SevenZipped { .. } => "unpack",
                Source::Pages(_) => "pack",
            };
            let place = |folder: Option<String>, status| Placement {
                source: b.source.clone(),
                file_name: b.file_name.clone(),
                folder,
                status,
                how,
                size: b.size,
                from: Some(b.from.clone()),
            };
            // A name like "v01.cbz" says nothing: the folder it came in names the series.
            let own = series_guess(&b.file_name);
            let guess = match &b.hint {
                Some(h) if says_nothing(&own) => crate::scan::clean_name(h),
                _ => own,
            };
            let same = |c: &Option<String>| {
                let find = |g: &str| known.iter().find(|(col, n, _)| *n == norm(g) && c.as_ref().is_none_or(|c| c == col)).map(|k| k.2.clone());
                find(&guess).or_else(|| b.hint.as_deref().and_then(find))
            };
            let (folder, new) = if let Some(rel) = &target_series {
                (Some(series_folder(rel, root)), false)
            } else if let Some(name) = name.filter(|_| chosen.is_some() || no_libraries) {
                match known.iter().find(|(c, n, _)| collection_name.as_ref().is_none_or(|col| c == col) && *n == norm(name)) {
                    Some(k) => (Some(k.2.clone()), false),
                    None => match safe_folder(name) {
                        safe if safe.is_empty() => (None, false),
                        safe => (Some(in_library(safe)), true),
                    },
                }
            } else if chosen.is_some() {
                match same(&collection_name) {
                    Some(f) => (Some(f), false),
                    // A new series this drop already made a folder for, under a name spelled a little
                    // differently ("Vanya - The Lost Warrior" and "Vanya the Lost Warrior"): the same.
                    None => {
                        let folder = new_series.entry(norm(&guess)).or_insert_with(|| in_library(safe_folder(&guess)));
                        (Some(folder.clone()), true)
                    }
                }
            } else {
                match same(&None) {
                    Some(f) => (Some(f), false),
                    None if no_libraries => (Some(new_series.entry(norm(&guess)).or_insert_with(|| safe_folder(&guess)).clone()), true),
                    None => (None, false),
                }
            };
            let Some(folder) = folder.filter(|f| !f.is_empty()) else {
                out.push(place(None, "no-place"));
                continue;
            };
            // Already in the comics folder (dragged from it): nothing to copy.
            let inside = match &b.from {
                Source::File(p) | Source::Pages(p) | Source::Zipped { zip: p, .. } | Source::Rarred { rar: p, .. } | Source::SevenZipped { arc: p, .. } => {
                    std::fs::canonicalize(p).is_ok_and(|p| p.starts_with(&root_real))
                }
            };
            let dest = root.join(&folder).join(&b.file_name);
            // Two books in this drop that would land on the same name: the same file, or the second
            // left out rather than renamed.
            let twin = planned.get(&dest).copied();
            planned.entry(dest.clone()).or_insert(b.size);
            let status = if inside || twin == Some(b.size) {
                "exists"
            } else if twin.is_some() {
                "name-taken"
            } else if dest.exists() {
                let same_size = matches!(b.from, Source::Pages(_)) || dest.metadata().map(|m| m.len()).ok() == Some(b.size);
                if same_size { "exists" } else { "name-taken" }
            } else if new && !root.join(&folder).exists() {
                "new-series"
            } else {
                "add"
            };
            out.push(place(Some(folder), status));
        }
    }
    Ok(out)
}

/// A new, empty library: a folder at the top of the comics folder. Returns its id. A folder of that
/// name that already holds something is left alone.
pub fn new_library(conn: &Connection, root: &Path, name: &str) -> Result<i64> {
    let safe = safe_folder(name);
    if safe.is_empty() {
        bail!("That name can't be a folder");
    }
    let layout = crate::db::get_setting(conn, crate::scan::Layout::SETTING)?;
    if crate::scan::Layout::parse(layout.as_deref()) == crate::scan::Layout::Series {
        bail!("Your folders are set to read as series. Change that in Settings to make libraries.");
    }
    let dir = root.join(&safe);
    if dir.exists() && (!dir.is_dir() || std::fs::read_dir(&dir)?.next().is_some()) {
        bail!("Something called {safe} is already in your comics folder");
    }
    std::fs::create_dir_all(&dir)?;
    let id = conn.query_row(
        "INSERT INTO collections(name, is_manga) VALUES(?1, ?2)
         ON CONFLICT(name) DO UPDATE SET name=excluded.name RETURNING id",
        rusqlite::params![safe, safe.eq_ignore_ascii_case("manga")],
        |r| r.get(0),
    )?;
    Ok(id)
}

/// How far a copy has got, for the note at the bottom of the window.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub bytes: u64,
    pub total_bytes: u64,
}

/// Copy `r` into `w`, telling `moved` about each stretch.
fn pour(mut r: impl Read, w: &mut impl Write, moved: &mut dyn FnMut(u64)) -> Result<()> {
    let mut buf = vec![0u8; 4 << 20];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        w.write_all(&buf[..n])?;
        moved(n as u64);
    }
}

/// Write one book to `part`.
fn write_book(from: &Source, part: &Path, moved: &mut dyn FnMut(u64)) -> Result<()> {
    match from {
        Source::File(p) => {
            let mut w = std::io::BufWriter::new(File::create(part)?);
            pour(File::open(p)?, &mut w, moved)?;
            w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
            // Keep the original's date, as a plain copy would.
            if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
                let _ = File::options().write(true).open(part).and_then(|f| f.set_modified(t));
            }
        }
        Source::Zipped { zip, entry } => {
            let mut z = crate::archive::open_zip(zip)?;
            let f = crate::archive::zip_entry(&mut z, entry)?;
            let mut w = std::io::BufWriter::new(File::create(part)?);
            pour(f, &mut w, moved)?;
            w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
        Source::Rarred { rar, entry } => {
            let mut arc = unrar::Archive::new(rar).open_for_processing()?;
            while let Some(h) = arc.read_header()? {
                if h.entry().filename.to_string_lossy().replace('\\', "/") == *entry {
                    let size = h.entry().unpacked_size;
                    h.extract_to(part)?;
                    moved(size);
                    return Ok(());
                }
                arc = h.skip()?;
            }
            bail!("{entry} wasn't in {}", rar.display());
        }
        Source::SevenZipped { arc, entry } => {
            let mut w = std::io::BufWriter::new(File::create(part)?);
            let mut found = false;
            sevenz_rust2::ArchiveReader::open(arc, sevenz_rust2::Password::empty())?.for_each_entries(|e, r| {
                if e.name() != entry {
                    std::io::copy(r, &mut std::io::sink())?;
                    return Ok(true);
                }
                found = true;
                let mut buf = vec![0u8; 4 << 20];
                loop {
                    let n = r.read(&mut buf)?;
                    if n == 0 {
                        return Ok(false);
                    }
                    w.write_all(&buf[..n])?;
                    moved(n as u64);
                }
            })?;
            if !found {
                bail!("{entry} wasn't in {}", arc.display());
            }
            w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
        Source::Pages(dir) => {
            let mut pages: Vec<PathBuf> = std::fs::read_dir(dir)?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && crate::archive::is_image(&p.file_name().unwrap_or_default().to_string_lossy()))
                .collect();
            pages.sort_by(|a, b| natord::compare_ignore_case(&a.to_string_lossy(), &b.to_string_lossy()));
            let mut z = zip::ZipWriter::new(std::io::BufWriter::new(File::create(part)?));
            // Pages are already compressed images: stored as they are, which is quick.
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored).large_file(true);
            for p in pages {
                z.start_file(p.file_name().unwrap_or_default().to_string_lossy(), opts)?;
                pour(File::open(&p)?, &mut z, moved)?;
            }
            z.finish()?.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
    }
    Ok(())
}

/// Copy the placed books in, telling `progress` as it goes. Returns how many were added.
pub fn copy_in(root: &Path, placements: &[Placement], progress: &mut dyn FnMut(&Progress)) -> Result<usize> {
    let root_real = std::fs::canonicalize(root)?;
    let adding: Vec<&Placement> = placements.iter().filter(|p| matches!(p.status, "add" | "new-series")).collect();
    let mut at = Progress { total: adding.len(), total_bytes: adding.iter().map(|p| p.size).sum(), ..Default::default() };
    let mut added = 0;
    for p in adding {
        let (Some(folder), Some(from)) = (&p.folder, &p.from) else { continue };
        if folder.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            bail!("Refusing an odd destination: {folder}");
        }
        let name = Path::new(&p.file_name);
        if name.components().count() != 1 || !matches!(name.components().next(), Some(std::path::Component::Normal(_))) {
            bail!("Refusing an odd file name: {}", p.file_name);
        }
        let dir = as_folder(&root.join(folder));
        std::fs::create_dir_all(&dir)?;
        // Belt and braces: the folder really is inside the comics folder.
        if !std::fs::canonicalize(&dir)?.starts_with(&root_real) {
            bail!("Refusing to copy outside the comics folder");
        }
        let dest = dir.join(name);
        let start = at.bytes;
        // Something got there since the plan was made: leave it be.
        if !dest.exists() {
            // Written under a temporary name, so a half-copied file is never scanned as a comic.
            let part = dest.with_extension("aion-part");
            let mut last = std::time::Instant::now();
            let written = write_book(from, &part, &mut |n| {
                at.bytes += n;
                if last.elapsed().as_millis() >= 150 {
                    last = std::time::Instant::now();
                    progress(&at);
                }
            });
            match written {
                Ok(()) => {
                    std::fs::rename(&part, &dest)?;
                    added += 1;
                }
                Err(e) => {
                    // Only ever our own half-written file.
                    let _ = std::fs::remove_file(&part);
                    return Err(e.context(format!("copying {}", p.file_name)));
                }
            }
        }
        at.bytes = start + p.size;
        at.done += 1;
        progress(&at);
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
            how: "copy",
            size: 8,
            from: Some(Source::File(src.join(name))),
        };
        let copy_in = |root: &Path, p: &[Placement]| copy_in(root, p, &mut |_| {});
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

    #[test]
    fn two_numbers_keep_the_first_in_the_name() {
        assert_eq!(series_guess("Spider-Man 2099 001 (2014) (Digital).cbz"), "Spider-Man 2099");
        assert_eq!(series_guess("Batman 66 012.cbz"), "Batman 66");
        assert_eq!(series_guess("100 Bullets 001.cbz"), "100 Bullets");
        assert_eq!(series_guess("Saga 054 - The War for Phang.cbr"), "Saga");
        assert_eq!(series_guess("Saga 054 - Book 2.cbr"), "Saga");
    }

    #[test]
    fn adding_to_a_folder_whose_name_ends_in_a_space_makes_no_stray_folder() {
        let base = std::env::temp_dir().join(format!("aion-space-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, src) = (base.join("Comics"), base.join("Downloads"));
        std::fs::create_dir_all(format!("{}/DC/Arrow /", root.display())).unwrap();
        std::fs::write(format!("{}/DC/Arrow /Arrow 001.cbz", root.display()), b"PK").unwrap();
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Arrow 002.cbz"), b"PK two").unwrap();
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM series", [], |r| r.get(0)).unwrap();
        let paths = vec![src.join("Arrow 002.cbz").to_string_lossy().into_owned()];
        let placed = plan(&conn, &root, gather_all(&paths), Some(id), None, None).unwrap();
        assert_eq!(copy_in(&root, &placed, &mut |_| {}).unwrap(), 1);
        assert!(std::path::Path::new(&format!("{}/DC/Arrow /Arrow 002.cbz", root.display())).exists());
        let folders: Vec<String> = std::fs::read_dir(root.join("DC")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(folders, ["Arrow "], "no second folder beside it");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn seven_zip_bundles_and_books_with_a_stray_comic() {
        use std::io::Write as _;
        let base = std::env::temp_dir().join(format!("aion-7z-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, src) = (base.join("Comics"), base.join("Downloads"));
        std::fs::create_dir_all(root.join("Manga/Berserk")).unwrap();
        std::fs::write(root.join("Manga/Berserk/Berserk v01.cbz"), b"PK").unwrap();
        std::fs::create_dir_all(&src).unwrap();
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let manga: i64 = conn.query_row("SELECT id FROM collections WHERE name='Manga'", [], |r| r.get(0)).unwrap();

        // A .7z of two books, with a cover picture beside them.
        let bundle = src.join("Berserk bundle.7z");
        let mut w = sevenz_rust2::ArchiveWriter::create(&bundle).unwrap();
        for (name, data) in [("Berserk/Berserk v02.cbz", "two"), ("Berserk/Berserk v03.cbz", "three"), ("cover.jpg", "c")] {
            w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(std::io::Cursor::new(data.as_bytes()))).unwrap();
        }
        w.finish().unwrap();
        let paths = vec![bundle.to_string_lossy().into_owned()];
        let placed = plan(&conn, &root, gather_all(&paths), None, Some(manga), None).unwrap();
        let got: Vec<_> = placed.iter().map(|p| (p.file_name.as_str(), p.folder.as_deref(), p.status, p.how)).collect();
        assert_eq!(got, [("Berserk v02.cbz", Some("Manga/Berserk"), "add", "unpack"), ("Berserk v03.cbz", Some("Manga/Berserk"), "add", "unpack")]);
        assert_eq!(copy_in(&root, &placed, &mut |_| {}).unwrap(), 2);
        assert_eq!(std::fs::read(root.join("Manga/Berserk/Berserk v03.cbz")).unwrap(), b"three");
        assert!(!root.join("Manga/Berserk/cover.jpg").exists());

        // A comic with a stray PDF among its pages is a comic, of each kind that can hold one.
        let book = src.join("Saga 001.cbz");
        let mut z = zip::ZipWriter::new(File::create(&book).unwrap());
        for name in ["01.jpg", "02.jpg", "03.jpg", "04.jpg", "05.jpg", "06.jpg", "07.jpg", "08.jpg", "credits.pdf"] {
            z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b"x").unwrap();
        }
        z.finish().unwrap();
        let book7 = src.join("Saga 002.cb7");
        let mut w = sevenz_rust2::ArchiveWriter::create(&book7).unwrap();
        for name in ["01.jpg", "02.jpg", "03.jpg", "04.jpg", "05.jpg", "06.jpg", "07.jpg", "08.jpg", "credits.pdf"] {
            w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(std::io::Cursor::new(b"x".as_slice()))).unwrap();
        }
        w.finish().unwrap();
        let paths = vec![book.to_string_lossy().into_owned(), book7.to_string_lossy().into_owned()];
        let placed = plan(&conn, &root, gather_all(&paths), None, Some(manga), None).unwrap();
        let got: Vec<_> = placed.iter().map(|p| (p.file_name.as_str(), p.how)).collect();
        assert_eq!(got, [("Saga 001.cbz", "copy"), ("Saga 002.cb7", "copy")]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn adding_follows_the_folders() {
        let base = std::env::temp_dir().join(format!("aion-follows-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, src) = (base.join("Comics"), base.join("Downloads"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&src).unwrap();
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let drop_in = |name: &str| {
            std::fs::write(src.join(name), b"PK").unwrap();
            vec![src.join(name).to_string_lossy().into_owned()]
        };
        let folder = |p: Vec<Placement>| p[0].folder.clone();
        // Nothing here yet: a series straight in the comics folder, under the name asked for or its own.
        let saga = drop_in("Saga 001.cbz");
        assert_eq!(folder(plan(&conn, &root, gather_all(&saga), None, None, None).unwrap()).as_deref(), Some("Saga"));
        assert_eq!(folder(plan(&conn, &root, gather_all(&saga), None, None, Some("Mine")).unwrap()).as_deref(), Some("Mine"));
        copy_in(&root, &plan(&conn, &root, gather_all(&saga), None, None, None).unwrap(), &mut |_| {}).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let (id, at_top): (i64, bool) = conn.query_row("SELECT id, at_top FROM collections", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert!(at_top);
        // In the library with no folder of its own: new series stay at the top, not in a "Comics" folder.
        let maus = drop_in("Maus.cbz");
        assert_eq!(folder(plan(&conn, &root, gather_all(&maus), None, Some(id), None).unwrap()).as_deref(), Some("Maus"));
        let more = drop_in("Saga 002.cbz");
        assert_eq!(folder(plan(&conn, &root, gather_all(&more), None, Some(id), None).unwrap()).as_deref(), Some("Saga"));
        // A loose book's series gets a folder for the next one, which it joins by name.
        std::fs::write(root.join("Bone 001.cbz"), b"PK").unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let bone: i64 = conn.query_row("SELECT id FROM series WHERE rel_path='Bone 001.cbz'", [], |r| r.get(0)).unwrap();
        let next = drop_in("Bone 002.cbz");
        let placed = plan(&conn, &root, gather_all(&next), Some(bone), None, None).unwrap();
        assert_eq!(placed[0].folder.as_deref(), Some("Bone"));
        copy_in(&root, &placed, &mut |_| {}).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let books: i64 = conn.query_row("SELECT COUNT(*) FROM comics c JOIN series s ON s.id=c.series_id WHERE s.rel_path='Bone'", [], |r| r.get(0)).unwrap();
        assert_eq!(books, 2, "one series");
        // A new library, then a series in it: an ordinary library from then on.
        let favorites = new_library(&conn, &root, "Favorites").unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        assert_eq!(new_library(&conn, &root, "Favorites").unwrap(), favorites, "still empty, so the same one");
        assert!(new_library(&conn, &root, "Saga").is_err(), "that folder holds something");
        assert!(new_library(&conn, &root, "..").is_err());
        let placed = plan(&conn, &root, gather_all(&maus), None, Some(favorites), None).unwrap();
        assert_eq!(placed[0].folder.as_deref(), Some("Favorites/Maus"));
        copy_in(&root, &placed, &mut |_| {}).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let found: i64 = conn.query_row("SELECT collection_id FROM series WHERE rel_path='Favorites/Maus'", [], |r| r.get(0)).unwrap();
        assert_eq!(found, favorites);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn one_new_series_spelled_two_ways() {
        let base = std::env::temp_dir().join(format!("aion-spelled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, src) = (base.join("Comics"), base.join("Downloads"));
        std::fs::create_dir_all(root.join("Indie/Saga")).unwrap();
        std::fs::write(root.join("Indie/Saga/Saga 001.cbz"), b"PK").unwrap();
        std::fs::create_dir_all(&src).unwrap();
        let names = ["Vanya - The Lost Warrior 008 (2025) (Digital-HD).cbz", "Vanya the Lost Warrior 009 (2025) (Digital).cbz"];
        for n in names {
            std::fs::write(src.join(n), b"PK").unwrap();
        }
        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let indie: i64 = conn.query_row("SELECT id FROM collections WHERE name='Indie'", [], |r| r.get(0)).unwrap();
        let paths: Vec<String> = names.iter().map(|n| src.join(n).to_string_lossy().into_owned()).collect();
        let placed = plan(&conn, &root, gather_all(&paths), None, Some(indie), None).unwrap();
        let folders: Vec<_> = placed.iter().map(|p| p.folder.as_deref()).collect();
        assert_eq!(folders, [Some("Indie/Vanya - The Lost Warrior"); 2]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn folders_and_bundles() {
        use std::io::Write as _;
        let base = std::env::temp_dir().join(format!("aion-bundles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, src) = (base.join("Comics"), base.join("Downloads"));
        std::fs::create_dir_all(root.join("Manga/Berserk")).unwrap();
        std::fs::write(root.join("Manga/Berserk/Berserk v01.cbz"), b"PK").unwrap();
        // A folder of a series' books with names that say nothing but the number.
        std::fs::create_dir_all(src.join("Vinland Saga")).unwrap();
        std::fs::write(src.join("Vinland Saga/v01.cbz"), b"PK one").unwrap();
        std::fs::write(src.join("Vinland Saga/v02.cbz"), b"PK two").unwrap();
        // A .zip of books, with the leftovers a Mac adds.
        let mut z = zip::ZipWriter::new(File::create(src.join("Berserk bundle.zip")).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in [("Berserk/Berserk v02.cbz", "two"), ("Berserk/Berserk v03.cbz", "three"), ("__MACOSX/Berserk/._Berserk v02.cbz", "x"), ("notes.txt", "hi")] {
            z.start_file(name, opts).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        // A book's loose pages.
        std::fs::create_dir_all(src.join("Some Book 01")).unwrap();
        for n in ["10.jpg", "2.jpg", "1.jpg"] {
            std::fs::write(src.join("Some Book 01").join(n), n).unwrap();
        }
        std::fs::write(src.join("notes.txt"), b"x").unwrap();

        let mut conn = crate::db::open(&base.join("t.db")).unwrap();
        crate::scan::scan(&mut conn, &root).unwrap();
        let manga: i64 = conn.query_row("SELECT id FROM collections WHERE name='Manga'", [], |r| r.get(0)).unwrap();
        let paths: Vec<String> = ["Vinland Saga", "Berserk bundle.zip", "Some Book 01", "notes.txt"]
            .iter()
            .map(|n| src.join(n).to_string_lossy().into_owned())
            .collect();
        let placed = plan(&conn, &root, gather_all(&paths), None, Some(manga), None).unwrap();
        let got: Vec<(&str, Option<&str>, &str, &str)> =
            placed.iter().map(|p| (p.file_name.as_str(), p.folder.as_deref(), p.status, p.how)).collect();
        assert_eq!(
            got,
            vec![
                ("v01.cbz", Some("Manga/Vinland Saga"), "new-series", "copy"),
                ("v02.cbz", Some("Manga/Vinland Saga"), "new-series", "copy"),
                ("Berserk v02.cbz", Some("Manga/Berserk"), "add", "unpack"),
                ("Berserk v03.cbz", Some("Manga/Berserk"), "add", "unpack"),
                ("Some Book 01.cbz", Some("Manga/Some Book"), "new-series", "pack"),
                ("notes.txt", None, "not-comic", "copy"),
            ]
        );
        let mut last = Progress::default();
        assert_eq!(copy_in(&root, &placed, &mut |p| last = p.clone()).unwrap(), 5);
        assert_eq!((last.done, last.total), (5, 5));
        assert_eq!(std::fs::read(root.join("Manga/Vinland Saga/v02.cbz")).unwrap(), b"PK two");
        assert_eq!(std::fs::read(root.join("Manga/Berserk/Berserk v03.cbz")).unwrap(), b"three");
        let packed = crate::archive::open_zip(&root.join("Manga/Some Book/Some Book 01.cbz")).unwrap();
        assert_eq!(packed.file_names().map(|n| n.unwrap().into_owned()).collect::<Vec<_>>(), ["1.jpg", "2.jpg", "10.jpg"]);
        assert!(src.join("Berserk bundle.zip").exists() && src.join("Some Book 01/1.jpg").exists(), "the originals stay");
        // Dropped again: all there already.
        // Named: everything goes into that one series, existing or new, and clashing names stay out.
        let into = |name| plan(&conn, &root, gather_all(&paths[..1]), None, Some(manga), Some(name)).unwrap();
        assert!(into(" BERSERK ").iter().all(|p| p.folder.as_deref() == Some("Manga/Berserk") && p.status == "add"));
        assert!(into("New: Name.").iter().all(|p| p.folder.as_deref() == Some("Manga/New Name") && p.status == "new-series"));
        assert!(into("..").iter().all(|p| p.status == "no-place"));
        std::fs::create_dir_all(src.join("Two/A")).unwrap();
        std::fs::create_dir_all(src.join("Two/B")).unwrap();
        std::fs::write(src.join("Two/A/01.cbz"), b"PK a").unwrap();
        std::fs::write(src.join("Two/B/01.cbz"), b"PK bigger").unwrap();
        let two = vec![src.join("Two").to_string_lossy().into_owned()];
        let statuses: Vec<_> = plan(&conn, &root, gather_all(&two), None, Some(manga), Some("Two")).unwrap().iter().map(|p| p.status).collect();
        assert_eq!(statuses, ["new-series", "name-taken"]);
        let again = plan(&conn, &root, gather_all(&paths[..3]), None, Some(manga), None).unwrap();
        assert!(again.iter().all(|p| p.status == "exists"), "{:?}", again.iter().map(|p| p.status).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&base);
    }
}
