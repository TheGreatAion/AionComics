//! Reading pages out of comic archives (CBZ/ZIP, CBR/RAR, CB7/7z, EPUB, PDF) and plain folders of images.
//!
//! ZIP archives support random access, so pages are read on demand from a
//! small pool of open archives. unrar can only read a RAR front to back, but
//! the RAR headers say where each page sits, so a page of any CBR that isn't
//! solid is unpacked by itself from its own block (see rarindex.rs). Only a
//! CBR whose pages can't be is unpacked whole into a cache folder by a
//! background thread, from the page you're on to the end and then the start,
//! and kept between sessions, up to PAGES_CAP. Page lists are remembered on
//! disk, because listing a big CBR means reading through it. A CB7 is treated like a solid CBR, an
//! EPUB is a ZIP with its pages in the order its contents say, and PDF pages are drawn by Windows
//! (see pdf.rs) and kept in the same cache folder.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

/// What's kept in books' cache folders (unpacked CBR pages, screen copies, page previews), all
/// books together; the books opened longest ago go first.
const PAGES_CAP: u64 = 5 << 30;
/// A page the unpacking is at most this many pages short of is waited for rather than read on
/// its own (which means reading through the archive up to it).
const RAR_SOON: usize = 6;
/// Page requests for one book served at once (see `Archives::page_first`).
const PAGE_READERS: usize = 2;
/// A waiting request this many pages or more from the newest one is dropped: skipped past.
const STALE: usize = 4;
/// Open copies of one ZIP kept for reading pages side by side.
const ZIP_SPARE: usize = 3;

/// The kinds of file that are comics.
pub const EXTENSIONS: [&str; 8] = ["cbz", "cbr", "cb7", "zip", "rar", "7z", "pdf", "epub"];

// unrar's C++ sources use registry/crypto APIs from advapi32. Tauri happens to
// link it already, but tests and tools built from this crate need it too.
#[cfg(windows)]
#[link(name = "advapi32")]
unsafe extern "system" {}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Kind {
    Zip,
    Rar,
    /// A directory of image files, read in natural order.
    Folder,
    SevenZ,
    Pdf,
}

/// Sniff the real archive type; plenty of `.cbz` files are RARs and vice versa.
pub fn detect(path: &Path) -> Result<Kind> {
    if path.is_dir() {
        return Ok(Kind::Folder);
    }
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut b = [0u8; 8];
    let n = f.read(&mut b)?;
    if n >= 4 && &b[..2] == b"PK" {
        Ok(Kind::Zip)
    } else if n >= 4 && &b[..4] == b"Rar!" {
        Ok(Kind::Rar)
    } else if n >= 6 && b[..6] == [b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C] {
        Ok(Kind::SevenZ)
    } else if n >= 5 && &b[..5] == b"%PDF-" {
        Ok(Kind::Pdf)
    } else {
        bail!("Unrecognized archive format")
    }
}

pub fn is_image(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let file = l.rsplit('/').next().unwrap_or("");
    let ext_ok = [".jpg", ".jpeg", ".png", ".webp", ".gif"]
        .iter()
        .any(|e| l.ends_with(e));
    ext_ok && !l.contains("__macosx") && !file.starts_with('.')
}

pub fn mime_for(name: &str) -> &'static str {
    let l = name.to_ascii_lowercase();
    if l.ends_with(".png") {
        "image/png"
    } else if l.ends_with(".webp") {
        "image/webp"
    } else if l.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

fn sort_pages(v: &mut [String]) {
    v.sort_by(|a, b| natord::compare_ignore_case(a, b));
}

fn rar_name(h: &unrar::FileHeader) -> String {
    h.filename.to_string_lossy().replace('\\', "/")
}

type Zip = zip::ZipArchive<BufReader<File>>;

/// Where an entry sits in a ZIP, by the name `file_names` gave it. Those names are decoded (an
/// older ZIP's names from the DOS character set), but a lookup by name compares the bytes stored
/// in the file, so a name with a character like a non-breaking space in it is only found by
/// going through the decoded names.
pub(crate) fn zip_index<R: std::io::Read + std::io::Seek>(z: &zip::ZipArchive<R>, name: &str) -> Option<usize> {
    z.index_for_name(name).or_else(|| z.file_names().position(|n| n.is_ok_and(|n| n == name)))
}

/// An entry of a ZIP, by the name `file_names` gave it (see `zip_index`).
pub(crate) fn zip_entry<'a, R: std::io::Read + std::io::Seek>(z: &'a mut zip::ZipArchive<R>, name: &str) -> Result<zip::read::ZipFile<'a, R>> {
    let i = zip_index(z, name).ok_or_else(|| anyhow::anyhow!("{name} not found in archive"))?;
    Ok(z.by_index(i)?)
}

/// Open a ZIP trusting it starts at byte 0. The crate's default "detect" mode
/// scans the whole file for stray signatures, which costs seconds per 50 MB.
pub(crate) fn open_zip(path: &Path) -> Result<Zip> {
    let known = zip::read::Config { archive_offset: zip::read::ArchiveOffset::Known(0) };
    match zip::ZipArchive::with_config(known, BufReader::new(File::open(path)?)) {
        Ok(z) => Ok(z),
        Err(_) => Ok(zip::ZipArchive::new(BufReader::new(File::open(path)?))?),
    }
}

#[derive(Serialize, Deserialize)]
pub struct Listing {
    pub kind: Kind,
    /// Solid RARs can only be decoded front to back; others allow seeking.
    pub solid: bool,
    /// Image entry names in reading order.
    pub pages: Vec<String>,
    /// A RAR's pages that can be got at directly, by page (see rarindex.rs).
    #[serde(default)]
    pub parts: Vec<Option<Part>>,
    /// Where a RAR's opening headers end.
    #[serde(default)]
    pub head: u64,
}

/// Where a RAR page sits in the file, so it can be read without walking the archive to it.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct Part {
    /// Its block (header and data): where it starts, and how long it is.
    block: (u64, u64),
    /// Its bytes as they are, when stored uncompressed.
    data: Option<(u64, u64)>,
    crc: u32,
}

/// An attribute of an XML tag, by name.
fn xml_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let wanted = format!("{name}=");
    let (start, _) = tag.match_indices(&wanted).find(|(i, _)| tag[..*i].ends_with(char::is_whitespace))?;
    let value = &tag[start + wanted.len()..];
    let quote = value.chars().next()?;
    let rest = &value[1..];
    Some(&rest[..rest.find(quote)?])
}

/// A path as an EPUB writes it (relative to `base`, maybe with %20s) as the name inside the ZIP.
fn epub_path(base: &str, href: &str) -> String {
    let mut bytes = Vec::new();
    let raw = href.split(['#', '?']).next().unwrap_or("").as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let hex = (raw[i] == b'%' && i + 3 <= raw.len()).then(|| std::str::from_utf8(&raw[i + 1..i + 3]).ok()).flatten();
        match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
            Some(b) => {
                bytes.push(b);
                i += 3;
            }
            None => {
                bytes.push(raw[i]);
                i += 1;
            }
        }
    }
    let href = String::from_utf8_lossy(&bytes);
    let mut parts: Vec<&str> = if href.starts_with('/') { Vec::new() } else { base.split('/').filter(|p| !p.is_empty()).collect() };
    for p in href.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// An EPUB's pictures in reading order: each page its contents list, and the pictures on it.
fn epub_pages(z: &mut Zip) -> Option<Vec<String>> {
    fn text(z: &mut Zip, name: &str) -> Option<String> {
        let mut s = String::new();
        zip_entry(z, name).ok()?.read_to_string(&mut s).ok()?;
        Some(s)
    }
    let tag = regex::Regex::new(r"<[^>]+>").ok()?;
    let picture = regex::Regex::new(r#"(?is)<(?:img|image)\b[^>]*?\s(?:src|href|xlink:href)\s*=\s*["']([^"']+)["']"#).ok()?;
    let container = text(z, "META-INF/container.xml")?;
    let rootfile = tag.find_iter(&container).map(|m| m.as_str()).find(|t| t.starts_with("<rootfile") && !t.starts_with("<rootfiles"))?;
    let opf_name = xml_attr(rootfile, "full-path")?.to_string();
    let opf = text(z, &opf_name)?;
    let base = opf_name.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    let mut items: HashMap<String, String> = HashMap::new();
    let mut spine = Vec::new();
    for t in tag.find_iter(&opf).map(|m| m.as_str()) {
        if t.starts_with("<item") && !t.starts_with("<itemref") {
            if let (Some(id), Some(href)) = (xml_attr(t, "id"), xml_attr(t, "href")) {
                items.insert(id.to_string(), epub_path(&base, href));
            }
        } else if t.starts_with("<itemref") {
            if let Some(id) = xml_attr(t, "idref") {
                spine.push(id.to_string());
            }
        }
    }
    let mut pages: Vec<String> = Vec::new();
    for id in spine {
        let Some(name) = items.get(&id) else { continue };
        if is_image(name) {
            pages.push(name.clone());
            continue;
        }
        let Some(page) = text(z, name) else { continue };
        let dir = name.rsplit_once('/').map_or("", |(d, _)| d);
        pages.extend(picture.captures_iter(&page).map(|c| epub_path(dir, &c[1])).filter(|p| is_image(p)));
    }
    let names: HashSet<String> = z.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let mut seen = HashSet::new();
    pages.retain(|p| names.contains(p) && seen.insert(p.clone()));
    (!pages.is_empty()).then_some(pages)
}

pub fn list(path: &Path) -> Result<Listing> {
    let kind = detect(path)?;
    let mut pages = Vec::new();
    let mut solid = false;
    let (mut parts, mut head) = (Vec::new(), 0);
    // An EPUB says what order its pages go in; anything else goes by their names.
    let mut in_order = false;
    match kind {
        Kind::Zip => {
            let mut z = open_zip(path)?;
            let epub = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("epub"));
            match epub.then(|| epub_pages(&mut z)).flatten() {
                Some(ordered) => {
                    pages = ordered;
                    in_order = true;
                }
                None => {
                    for n in z.file_names() {
                        let n = n?;
                        if is_image(&n) {
                            pages.push(n.into_owned());
                        }
                    }
                }
            }
        }
        Kind::SevenZ => {
            let arc = sevenz_rust2::Archive::open(path)?;
            pages = arc.files.iter().filter(|f| !f.is_directory() && is_image(f.name())).map(|f| f.name().to_string()).collect();
            solid = true;
            parts = vec![None; pages.len()];
        }
        Kind::Pdf => {
            pages = (1..=crate::pdf::page_count(path)?).map(|n| format!("{n:05}.png")).collect();
        }
        Kind::Folder => {
            for e in fs::read_dir(path)? {
                let e = e?;
                let n = e.file_name().to_string_lossy().into_owned();
                if e.file_type()?.is_file() && is_image(&n) {
                    pages.push(n);
                }
            }
        }
        Kind::Rar => {
            let arc = unrar::Archive::new(path).open_for_listing()?;
            solid = arc.is_solid();
            // Every file in archive order, to line up with the headers read for parts.
            let mut files = Vec::new();
            for e in arc {
                let e = e?;
                if e.is_file() {
                    let n = rar_name(&e);
                    if is_image(&n) {
                        pages.push(n.clone());
                    }
                    files.push((n, e.unpacked_size, e.file_crc));
                }
            }
            // Only when the headers tell the same story as unrar: as many files, the same sizes.
            let found = crate::rarindex::entries(path).ok().filter(|ix| {
                ix.files.len() == files.len() && ix.files.iter().zip(&files).all(|(e, f)| e.unpacked == f.1)
            });
            sort_pages(&mut pages);
            parts = vec![None; pages.len()];
            if let Some(ix) = found.filter(|_| !solid) {
                let at: HashMap<&str, Part> = ix
                    .files
                    .iter()
                    .zip(&files)
                    .filter(|(e, _)| e.alone)
                    .map(|(e, f)| (f.0.as_str(), Part { block: e.block, data: e.stored, crc: f.2 }))
                    .collect();
                parts = pages.iter().map(|p| at.get(p.as_str()).copied()).collect();
                head = ix.head;
            }
        }
    }
    if pages.is_empty() {
        bail!("No pages found in this archive");
    }
    if !in_order {
        sort_pages(&mut pages);
    }
    Ok(Listing { kind, solid, pages, parts, head })
}

/// Read a single named entry from a RAR by streaming until we reach it.
pub(crate) fn rar_read_one(path: &Path, want: &str) -> Result<Vec<u8>> {
    let mut arc = unrar::Archive::new(path).open_for_processing()?;
    while let Some(h) = arc.read_header()? {
        if rar_name(h.entry()) == want {
            let (data, _) = h.read()?;
            return Ok(data);
        }
        arc = h.skip()?;
    }
    bail!("{want} not found in archive")
}

/// Tiny least-recently-used map; capacities here are single digits.
struct Lru<V> {
    cap: usize,
    items: Vec<(i64, V)>,
}

impl<V: Clone> Lru<V> {
    fn new(cap: usize) -> Self {
        Self { cap, items: Vec::new() }
    }
    fn get(&mut self, k: i64) -> Option<V> {
        let i = self.items.iter().position(|(key, _)| *key == k)?;
        let item = self.items.remove(i);
        let v = item.1.clone();
        self.items.push(item);
        Some(v)
    }
    /// Inserts and returns whatever got evicted.
    fn remove(&mut self, k: i64) -> Option<V> {
        let i = self.items.iter().position(|(key, _)| *key == k)?;
        Some(self.items.remove(i).1)
    }
    fn put(&mut self, k: i64, v: V) -> Option<V> {
        self.items.retain(|(key, _)| *key != k);
        self.items.push((k, v));
        if self.items.len() > self.cap {
            Some(self.items.remove(0).1)
        } else {
            None
        }
    }
}

/// Unpacking one CBR into its cache folder, a file per page (named by its reading-order index).
struct RarJob {
    dir: PathBuf,
    state: Mutex<RarState>,
    cv: Condvar,
    cancel: AtomicBool,
}

#[derive(Default)]
struct RarState {
    done: bool,
    error: Option<String>,
    /// The page unpacked last.
    last: Option<usize>,
}

/// A remembered page list, valid while the file's size and modification time match.
#[derive(Serialize, Deserialize)]
struct SavedListing {
    size: u64,
    mtime: u64,
    listing: Listing,
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let m = fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some((m.len(), mtime))
}

/// Open copies of one ZIP, free for the taking: each read takes one, so reads run side by side.
type ZipHandle = Arc<Mutex<Vec<Zip>>>;

/// The reader's page requests for one book, newest first.
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    cv: Condvar,
}

#[derive(Default)]
struct GateState {
    next: u64,
    running: usize,
    /// (ticket, page) of requests waiting their turn; higher tickets are newer.
    waiting: Vec<(u64, usize)>,
    /// The page asked for last.
    latest: usize,
}

/// A request's turn at the archive; the next waiting one goes when it's dropped.
struct Turn<'a>(&'a Gate);

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().running -= 1;
        self.0.cv.notify_all();
    }
}

impl Gate {
    /// Wait for this request's turn: the newest waiting request goes next, and one for a page far
    /// from the newest gets none (None) — the reader has skipped past it.
    fn enter(&self, page: usize) -> Option<Turn<'_>> {
        let mut st = self.state.lock().unwrap();
        let ticket = st.next;
        st.next += 1;
        st.waiting.push((ticket, page));
        st.latest = page;
        self.cv.notify_all();
        loop {
            let newest = st.waiting.iter().map(|w| w.0).max();
            if st.running < PAGE_READERS && newest == Some(ticket) {
                st.waiting.retain(|w| w.0 != ticket);
                st.running += 1;
                return Some(Turn(self));
            }
            if page.abs_diff(st.latest) >= STALE {
                st.waiting.retain(|w| w.0 != ticket);
                self.cv.notify_all();
                return None;
            }
            st = self.cv.wait(st).unwrap();
        }
    }
}

pub struct Archives {
    pages_dir: PathBuf,
    listings_dir: PathBuf,
    listings: Mutex<Lru<Arc<Listing>>>,
    zips: Mutex<Lru<ZipHandle>>,
    rars: Mutex<Lru<Arc<RarJob>>>,
    gates: Mutex<Lru<Arc<Gate>>>,
    /// Books whose cache folder was checked against their file this session.
    checked: Mutex<HashSet<i64>>,
    /// Numbers the tiny archives single CBR pages are unpacked from (see `rar_direct`).
    scratch: std::sync::atomic::AtomicU64,
    /// CBRs whose old unpacked pages were cleared away this session (see `drop_unpacked`).
    trimmed: Mutex<HashSet<i64>>,
}

impl Archives {
    pub fn new(cache_dir: &Path) -> Self {
        let pages_dir = cache_dir.join("pages");
        // Books opened from outside the library get new ids each session: their pages are no use.
        // Nor are tiny archives left over from unpacking single pages.
        for e in fs::read_dir(&pages_dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('-') {
                let _ = fs::remove_dir_all(e.path());
            } else if name.starts_with("one-") {
                let _ = fs::remove_file(e.path());
            }
        }
        let listings_dir = cache_dir.join("listings");
        let _ = fs::create_dir_all(&listings_dir);
        Self {
            pages_dir,
            listings_dir,
            listings: Mutex::new(Lru::new(16)),
            zips: Mutex::new(Lru::new(4)),
            rars: Mutex::new(Lru::new(4)),
            gates: Mutex::new(Lru::new(8)),
            checked: Mutex::new(HashSet::new()),
            scratch: Default::default(),
            trimmed: Mutex::new(HashSet::new()),
        }
    }

    pub fn listing(&self, id: i64, path: &Path) -> Result<Arc<Listing>> {
        if let Some(l) = self.listings.lock().unwrap().get(id) {
            return Ok(l);
        }
        let l = Arc::new(match self.saved_listing(id, path) {
            Some(l) => l,
            None => {
                let l = list(path)?;
                self.save_listing(id, path, &l);
                l
            }
        });
        self.listings.lock().unwrap().put(id, l.clone());
        Ok(l)
    }

    // Books opened from outside the library get new ids every session, so only library books are remembered.
    fn saved_listing(&self, id: i64, path: &Path) -> Option<Listing> {
        if id < 0 {
            return None;
        }
        let saved: SavedListing = serde_json::from_slice(&fs::read(self.listings_dir.join(format!("{id}.json"))).ok()?).ok()?;
        // A CBR listed before pages had parts gets listed again, once.
        let current = saved.listing.kind != Kind::Rar || saved.listing.parts.len() == saved.listing.pages.len();
        (current && stamp(path)? == (saved.size, saved.mtime)).then_some(saved.listing)
    }

    fn save_listing(&self, id: i64, path: &Path, listing: &Listing) {
        let Some((size, mtime)) = stamp(path).filter(|_| id > 0) else { return };
        let saved = SavedListing {
            size,
            mtime,
            listing: Listing {
                kind: listing.kind,
                solid: listing.solid,
                pages: listing.pages.clone(),
                parts: listing.parts.clone(),
                head: listing.head,
            },
        };
        if let Ok(json) = serde_json::to_vec(&saved) {
            let _ = fs::write(self.listings_dir.join(format!("{id}.json")), json);
        }
    }

    pub fn page(&self, id: i64, path: &Path, index: usize) -> Result<(Vec<u8>, &'static str)> {
        let listing = self.listing(id, path)?;
        let name = listing
            .pages
            .get(index)
            .ok_or_else(|| anyhow!("page {index} out of range"))?;
        let data = match listing.kind {
            Kind::Zip => self.zip_read(id, path, name)?,
            Kind::Rar | Kind::SevenZ => self.rar_read(id, path, &listing, index)?,
            Kind::Folder => fs::read(path.join(name))?,
            Kind::Pdf => self.pdf_read(id, path, index)?,
        };
        Ok((data, mime_for(name)))
    }

    /// A page the reader is waiting for: the newest request for a book is served first, and ones
    /// it has skipped past (STALE pages or more from the newest) are dropped with an error.
    pub fn page_first(&self, id: i64, path: &Path, index: usize) -> Result<(Vec<u8>, &'static str)> {
        let gate = {
            let mut gates = self.gates.lock().unwrap();
            gates.get(id).unwrap_or_else(|| {
                let g = Arc::new(Gate::default());
                gates.put(id, g.clone());
                g
            })
        };
        let Some(_turn) = gate.enter(index) else { bail!("skipped") };
        self.page(id, path, index)
    }

    /// A page for background work, without starting to unpack a CBR: CBZ and folder pages are
    /// read, a CBR's once unpacked or when they can be got at directly (None otherwise).
    pub fn page_if_ready(&self, id: i64, path: &Path, index: usize) -> Result<Option<Vec<u8>>> {
        let listing = self.listing(id, path)?;
        match listing.kind {
            Kind::Rar | Kind::SevenZ => match fs::read(self.book_dir(id, path)?.join(index.to_string())) {
                Ok(data) => Ok(Some(data)),
                Err(_) => Ok(self.rar_direct(path, &listing, index)),
            },
            _ => self.page(id, path, index).map(|p| Some(p.0)),
        }
    }

    /// Where page `n`'s copy for a screen `h` pixels tall goes (see `make_screen_copy`).
    pub fn screen_copy_path(&self, id: i64, path: &Path, n: usize, h: u32) -> Result<PathBuf> {
        Ok(self.book_dir(id, path)?.join(format!("screen-{n}-{h}.jpg")))
    }

    /// Keep the cache under PAGES_CAP now (it's also kept so after each CBR is unpacked).
    pub fn prune_now(&self) {
        let busy: Vec<PathBuf> = self.rars.lock().unwrap().items.iter().map(|(_, j)| j.dir.clone()).collect();
        prune(&self.pages_dir, &busy);
    }

    /// A CBR page got at directly (see rarindex.rs): read straight from the file if it's stored
    /// as it is, else unpacked from its block alone. None if it can't be, or doesn't check out.
    fn rar_direct(&self, path: &Path, listing: &Listing, index: usize) -> Option<Vec<u8>> {
        use std::io::{Seek, SeekFrom};
        let part = listing.parts.get(index).copied().flatten()?;
        let data = match part.data {
            Some((at, len)) => {
                let mut f = File::open(path).ok()?;
                f.seek(SeekFrom::Start(at)).ok()?;
                let mut buf = vec![0u8; len as usize];
                f.read_exact(&mut buf).ok()?;
                buf
            }
            None => {
                let n = self.scratch.fetch_add(1, Ordering::Relaxed);
                let _ = fs::create_dir_all(&self.pages_dir);
                let tmp = self.pages_dir.join(format!("one-{n}.rar"));
                crate::rarindex::unpack_alone(path, listing.head, part.block, &tmp).ok()?
            }
        };
        (crc32fast::hash(&data) == part.crc).then_some(data)
    }

    /// First page without kicking off a full RAR extraction (used for covers).
    pub fn first_page(&self, id: i64, path: &Path) -> Result<Vec<u8>> {
        let listing = self.listing(id, path)?;
        match listing.kind {
            Kind::Zip => self.zip_read(id, path, &listing.pages[0]),
            Kind::Rar => match self.rar_direct(path, &listing, 0) {
                Some(data) => Ok(data),
                None => rar_read_one(path, &listing.pages[0]),
            },
            Kind::Folder => Ok(fs::read(path.join(&listing.pages[0]))?),
            Kind::SevenZ => Ok(sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())?.read_file(&listing.pages[0])?),
            Kind::Pdf => self.pdf_read(id, path, 0),
        }
    }

    /// A PDF page, drawn once and kept in the book's cache folder.
    fn pdf_read(&self, id: i64, path: &Path, index: usize) -> Result<Vec<u8>> {
        let file = self.book_dir(id, path)?.join(index.to_string());
        if let Ok(data) = fs::read(&file) {
            return Ok(data);
        }
        let data = crate::pdf::page_png(path, index)?;
        let tmp = file.with_extension(format!("{}.part", self.scratch.fetch_add(1, Ordering::Relaxed)));
        if fs::write(&tmp, &data).is_ok() && fs::rename(&tmp, &file).is_err() {
            let _ = fs::remove_file(&tmp);
        }
        Ok(data)
    }

    /// Whether a book's pages are the long kind a webtoon is made of, from a few in the middle.
    /// Asks nothing of a book that would need unpacking first, or of a PDF, which has no such pages.
    pub fn pages_look_tall(&self, id: i64, path: &Path) -> bool {
        const TALL: f64 = 2.0;
        let Ok(listing) = self.listing(id, path) else { return false };
        let n = listing.pages.len();
        if listing.kind == Kind::Pdf || n < 3 {
            return false;
        }
        let ratios: Vec<f64> = [n / 3, n / 2, n * 2 / 3]
            .iter()
            .filter_map(|&i| self.page_if_ready(id, path, i).ok().flatten())
            .filter_map(|bytes| image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok())
            .map(|(w, h)| f64::from(h) / f64::from(w.max(1)))
            .collect();
        ratios.iter().filter(|r| **r >= TALL).count() * 2 > ratios.len()
    }

    /// Let go of a book's file, so it can be deleted: open copies, unpacking and its cache folder.
    pub fn release(&self, id: i64) {
        self.zips.lock().unwrap().remove(id);
        self.listings.lock().unwrap().remove(id);
        self.gates.lock().unwrap().remove(id);
        if let Some(job) = self.rars.lock().unwrap().remove(id) {
            job.cancel.store(true, Ordering::Relaxed);
            job.cv.notify_all();
            let st = job.state.lock().unwrap();
            let _ = job.cv.wait_timeout_while(st, Duration::from_secs(5), |s| !s.done);
        }
        self.forget_book(id);
        let _ = fs::remove_file(self.listings_dir.join(format!("{id}.json")));
    }

    fn zip_read(&self, id: i64, path: &Path, name: &str) -> Result<Vec<u8>> {
        let handle = {
            let mut zips = self.zips.lock().unwrap();
            zips.get(id).unwrap_or_else(|| {
                let h: ZipHandle = Arc::default();
                zips.put(id, h.clone());
                h
            })
        };
        let free = handle.lock().unwrap().pop();
        let mut z = match free {
            Some(z) => z,
            None => open_zip(path)?,
        };
        let buf = {
            let mut f = zip_entry(&mut z, name)?;
            let mut buf = Vec::with_capacity(f.size() as usize);
            f.read_to_end(&mut buf)?;
            buf
        };
        let mut spare = handle.lock().unwrap();
        if spare.len() < ZIP_SPARE {
            spare.push(z);
        }
        Ok(buf)
    }

    fn rar_read(&self, id: i64, path: &Path, listing: &Arc<Listing>, index: usize) -> Result<Vec<u8>> {
        // Every page can be got at by itself: nothing needs unpacking whole, or keeping unpacked.
        if listing.parts.iter().all(|p| p.is_some()) {
            self.drop_unpacked(id, path);
            if let Some(data) = self.rar_direct(path, listing, index) {
                return Ok(data);
            }
            return rar_read_one(path, &listing.pages[index]);
        }
        let job = self.rar_job(id, path, listing, index)?;
        let target = job.dir.join(index.to_string());
        let mut st = job.state.lock().unwrap();
        // A solid archive can only be read front to back, so the unpacking is the only way to the
        // page. Otherwise wait only if it's about to get there, and the page can't be got at
        // directly.
        let direct = listing.parts.get(index).is_some_and(|p| p.is_some());
        let soon = |st: &RarState| !direct && st.last.is_some_and(|l| l < index && index - l <= RAR_SOON);
        let deadline = std::time::Instant::now() + Duration::from_millis(400);
        while !target.exists() && !st.done && (listing.solid || soon(&st) && std::time::Instant::now() < deadline) {
            st = job.cv.wait_timeout(st, Duration::from_millis(50)).unwrap().0;
        }
        let err = st.error.clone();
        drop(st);
        if let Ok(data) = fs::read(&target) {
            return Ok(data);
        }
        if let Some(data) = self.rar_direct(path, listing, index) {
            return Ok(data);
        }
        if !listing.solid {
            return rar_read_one(path, &listing.pages[index]);
        }
        bail!(err.unwrap_or_else(|| "page missing from archive".into()))
    }

    /// Library books with a cache folder, and when each was last opened.
    pub fn cached_books(&self) -> Vec<(i64, std::time::SystemTime)> {
        fs::read_dir(&self.pages_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let id: i64 = e.file_name().to_string_lossy().parse().ok().filter(|&id| id > 0)?;
                let used = fs::metadata(e.path().join("stamp")).and_then(|m| m.modified()).ok()?;
                Some((id, used))
            })
            .collect()
    }

    /// Clear a book's cache folder (it's made again if the book is opened again), unless its
    /// CBR is being unpacked right now.
    pub fn forget_book(&self, id: i64) {
        {
            let mut rars = self.rars.lock().unwrap();
            if rars.items.iter().any(|(k, j)| *k == id && !j.state.lock().unwrap().done) {
                return;
            }
            // Its finished unpacking goes too, or opening it again would find no pages and no job to make them.
            rars.remove(id);
        }
        let _ = fs::remove_dir_all(self.pages_dir.join(id.to_string()));
        self.checked.lock().unwrap().remove(&id);
        self.trimmed.lock().unwrap().remove(&id);
    }

    /// Clear away a CBR's pages unpacked whole, if every one of its pages can be got at by itself.
    pub fn trim_unpacked(&self, id: i64, path: &Path) {
        let Ok(listing) = self.listing(id, path) else { return };
        if listing.kind == Kind::Rar && listing.parts.iter().all(|p| p.is_some()) {
            self.drop_unpacked(id, path);
        }
    }

    /// A CBR's pages unpacked whole before its pages could be got at one by one: no use now.
    /// Looked at once a session.
    fn drop_unpacked(&self, id: i64, path: &Path) {
        if !self.trimmed.lock().unwrap().insert(id) {
            return;
        }
        let Ok(dir) = self.book_dir(id, path) else { return };
        for e in fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.split('.').next().is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())) {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    /// The book's cache folder (a CBR's unpacked pages, any book's screen copies), emptied first
    /// if what's in it came from an earlier version of the file. Checking it marks the book used,
    /// for PAGES_CAP.
    pub fn book_dir(&self, id: i64, path: &Path) -> Result<PathBuf> {
        let dir = self.pages_dir.join(id.to_string());
        if self.checked.lock().unwrap().insert(id) {
            let (size, mtime) = stamp(path).ok_or_else(|| anyhow!("can't read {}", path.display()))?;
            let mark = format!("{size} {mtime}");
            let file = dir.join("stamp");
            if fs::read_to_string(&file).ok().as_deref() != Some(mark.as_str()) {
                let _ = fs::remove_dir_all(&dir);
            }
            fs::create_dir_all(&dir)?;
            fs::write(&file, mark)?;
        }
        Ok(dir)
    }

    /// The unpacking job for this book, started (from page `from`) if it isn't running or finished.
    fn rar_job(&self, id: i64, path: &Path, listing: &Arc<Listing>, from: usize) -> Result<Arc<RarJob>> {
        let dir = self.book_dir(id, path)?;
        let mut rars = self.rars.lock().unwrap();
        if let Some(j) = rars.get(id) {
            return Ok(j);
        }
        let job = Arc::new(RarJob { dir, state: Mutex::default(), cv: Condvar::new(), cancel: AtomicBool::new(false) });
        if let Some(old) = rars.put(id, job.clone()) {
            // Another book fell out: stop unpacking it (what it unpacked stays).
            old.cancel.store(true, Ordering::Relaxed);
            old.cv.notify_all();
        }
        let busy: Vec<PathBuf> = rars.items.iter().map(|(_, j)| j.dir.clone()).collect();
        drop(rars);
        let (j, path, listing, pages_dir) = (job.clone(), path.to_path_buf(), listing.clone(), self.pages_dir.clone());
        std::thread::spawn(move || {
            let res = if listing.kind == Kind::SevenZ { extract_7z(&j, &path, &listing) } else { extract_rar(&j, &path, &listing, from) };
            let mut st = j.state.lock().unwrap();
            st.done = true;
            st.error = res.err().map(|e| e.to_string());
            drop(st);
            j.cv.notify_all();
            prune(&pages_dir, &busy);
        });
        Ok(job)
    }
}

fn extract_rar(job: &RarJob, path: &Path, listing: &Listing, from: usize) -> Result<()> {
    let index: HashMap<&str, usize> = listing.pages.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    let have = |i: usize| job.dir.join(i.to_string()).exists();
    // From the page the reader opened on to the end first (a non-solid archive skips past the
    // pages before it cheaply), then the start. A solid one has to be decoded in order.
    let from = if listing.solid { 0 } else { from };
    for first_pass in [true, false] {
        if !first_pass && from == 0 {
            break;
        }
        let mut arc = unrar::Archive::new(path).open_for_processing()?;
        while let Some(h) = arc.read_header()? {
            if job.cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            let page = index.get(rar_name(h.entry()).as_str()).copied();
            let Some(i) = page.filter(|&i| (i >= from) == first_pass && !have(i)) else {
                arc = if listing.solid { h.read()?.1 } else { h.skip()? };
                continue;
            };
            let (data, next) = h.read()?;
            let tmp = job.dir.join(format!("{i}.part"));
            fs::write(&tmp, &data)?;
            fs::rename(&tmp, job.dir.join(i.to_string()))?;
            job.state.lock().unwrap().last = Some(i);
            job.cv.notify_all();
            arc = next;
        }
    }
    Ok(())
}

/// A CB7 unpacked front to back, the only way most can be.
fn extract_7z(job: &RarJob, path: &Path, listing: &Listing) -> Result<()> {
    let index: HashMap<&str, usize> = listing.pages.iter().enumerate().map(|(i, n)| (n.as_str(), i)).collect();
    let mut arc = sevenz_rust2::ArchiveReader::open(path, sevenz_rust2::Password::empty())?;
    arc.for_each_entries(|entry, reader| {
        if job.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let page = index.get(entry.name()).copied().filter(|i| !job.dir.join(i.to_string()).exists());
        let Some(i) = page else {
            std::io::copy(reader, &mut std::io::sink())?;
            return Ok(true);
        };
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let tmp = job.dir.join(format!("{i}.part"));
        fs::write(&tmp, &data)?;
        fs::rename(&tmp, job.dir.join(i.to_string()))?;
        job.state.lock().unwrap().last = Some(i);
        job.cv.notify_all();
        Ok(true)
    })?;
    Ok(())
}

/// Keep unpacked pages under PAGES_CAP: the books opened longest ago go, except `busy` ones.
fn prune(pages_dir: &Path, busy: &[PathBuf]) {
    let size = |d: &Path| fs::read_dir(d).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum::<u64>();
    let used = |d: &Path| fs::metadata(d.join("stamp")).and_then(|m| m.modified()).ok();
    let mut books: Vec<(Option<std::time::SystemTime>, u64, PathBuf)> =
        fs::read_dir(pages_dir).into_iter().flatten().flatten().map(|e| e.path()).map(|d| (used(&d), size(&d), d)).collect();
    let mut total: u64 = books.iter().map(|b| b.1).sum();
    books.sort_by_key(|b| b.0);
    for (_, bytes, dir) in books {
        if total <= PAGES_CAP {
            break;
        }
        if !busy.contains(&dir) {
            let _ = fs::remove_dir_all(&dir);
            total -= bytes;
        }
    }
}

/// Decode an image no smaller than `long_side` on its long side. JPEGs (nearly every comic page) are
/// decoded at a reduced scale straight from their compressed data, which is several times faster
/// than decoding at full size and shrinking afterward.
pub fn decode_at_least(bytes: &[u8], long_side: u32) -> Result<image::DynamicImage> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        if let Ok(img) = decode_jpeg_scaled(bytes, long_side) {
            return Ok(img);
        }
    }
    Ok(image::load_from_memory(bytes)?)
}

fn decode_jpeg_scaled(bytes: &[u8], long_side: u32) -> Result<image::DynamicImage> {
    use jpeg_decoder::PixelFormat;
    let mut d = jpeg_decoder::Decoder::new(bytes);
    d.read_info()?;
    let info = d.info().ok_or_else(|| anyhow!("no JPEG header"))?;
    let (w, h) = (info.width as u32, info.height as u32);
    let s = (long_side as f64 / w.max(h) as f64).min(1.0);
    let want = |v: u32| ((v as f64 * s).ceil() as u16).max(1);
    let (sw, sh) = d.scale(want(w), want(h))?;
    let px = d.decode()?;
    let (sw, sh) = (sw as u32, sh as u32);
    let img = match d.info().map(|i| i.pixel_format) {
        Some(PixelFormat::RGB24) => image::RgbImage::from_raw(sw, sh, px).map(image::DynamicImage::ImageRgb8),
        Some(PixelFormat::L8) => image::GrayImage::from_raw(sw, sh, px).map(image::DynamicImage::ImageLuma8),
        _ => None,
    };
    img.ok_or_else(|| anyhow!("unsupported JPEG pixel format"))
}

/// A page this much taller than the screen, or more, gets a copy at screen height.
const SCREEN_SLACK: f64 = 1.25;

/// A copy of a page at screen height `h` (a JPEG), for showing it whole: a big scan takes several
/// times as long to decode as the screen can show of it. None when the page isn't that much
/// bigger than the screen, so the page itself does.
pub fn make_screen_copy(bytes: &[u8], h: u32) -> Result<Option<Vec<u8>>> {
    let (pw, ph) = page_size(bytes)?;
    if (ph as f64) < h as f64 * SCREEN_SLACK {
        return Ok(None);
    }
    let w = ((pw as f64 * h as f64 / ph as f64).round() as u32).max(1);
    let img = decode_at_least(bytes, w.max(h))?;
    let copy = img.resize_exact(w, h, image::imageops::FilterType::CatmullRom).to_rgb8();
    let mut out = Vec::new();
    // At screen size, quality 85 looks the same as higher and takes a fifth less room.
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&copy)?;
    Ok(Some(out))
}

/// A page's size from its header, without decoding it.
fn page_size(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut d = jpeg_decoder::Decoder::new(bytes);
        d.read_info()?;
        let info = d.info().ok_or_else(|| anyhow!("no JPEG header"))?;
        return Ok((info.width as u32, info.height as u32));
    }
    Ok(image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?.into_dimensions()?)
}

/// Downscale a page into a JPEG thumbnail for the library grid.
/// Returns the JPEG and the cover's shape (height / width).
pub fn make_thumbnail(bytes: &[u8]) -> Result<(Vec<u8>, f64)> {
    let img = decode_at_least(bytes, 900)?;
    let ratio = img.height() as f64 / img.width().max(1) as f64;
    let thumb = img.thumbnail(400, 900).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 84).encode_image(&thumb)?;
    Ok((out, ratio))
}

/// The color a cover reads as at a glance, as "#rrggbb": the strongest hue among its vivid
/// pixels (by how much of the cover it covers and how saturated it is), averaged within that hue.
/// A cover with next to no color (black and white manga, say) gets its plain average.
pub fn cover_color(bytes: &[u8]) -> Result<String> {
    let img = image::load_from_memory(bytes)?.thumbnail(48, 72).to_rgb8();
    const BINS: usize = 24;
    let mut weight = [0f64; BINS];
    let mut sums = [[0f64; 3]; BINS];
    let (mut all, mut n) = ([0f64; 3], 0f64);
    for p in img.pixels() {
        let [r, g, b] = p.0.map(|c| c as f64 / 255.0);
        for (s, c) in all.iter_mut().zip([r, g, b]) {
            *s += c;
        }
        n += 1.0;
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let (v, chroma) = (max, max - min);
        if v < 0.12 || chroma < 0.08 {
            continue;
        }
        let s = chroma / v;
        let h = if max == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        } / 6.0;
        let bin = ((h * BINS as f64) as usize).min(BINS - 1);
        let w = s * s * v.sqrt();
        weight[bin] += w;
        for (acc, c) in sums[bin].iter_mut().zip([r, g, b]) {
            *acc += c * w;
        }
    }
    // Neighboring bins count toward a hue, so a gradient across two bins isn't split in half.
    let best = (0..BINS)
        .max_by(|&a, &b| {
            let around = |i: usize| weight[i] + 0.5 * (weight[(i + 1) % BINS] + weight[(i + BINS - 1) % BINS]);
            around(a).total_cmp(&around(b))
        })
        .unwrap_or(0);
    let total: f64 = weight.iter().sum();
    let rgb = if total > n * 0.02 {
        sums[best].map(|c| c / weight[best])
    } else {
        all.map(|c| c / n.max(1.0))
    };
    let [r, g, b] = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    Ok(format!("#{r:02x}{g:02x}{b:02x}"))
}

/// A cover blown up behind a collection's name: in gray (the page inks it in the cover's color)
/// and 1100 pixels across at most, a small fraction of the full page's weight.
pub fn make_print(bytes: &[u8]) -> Result<Vec<u8>> {
    let img = decode_at_least(bytes, 1800)?;
    let print = img.thumbnail(1100, 1800).to_luma8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 72).encode_image(&print)?;
    Ok(out)
}

/// Small JPEG of a page for the reader's page strip and bookmarks.
pub fn make_page_thumb(bytes: &[u8]) -> Result<Vec<u8>> {
    let img = decode_at_least(bytes, 480)?;
    let thumb = img.thumbnail(320, 480).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80).encode_image(&thumb)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ZIP of stored files whose names are raw bytes, flagged as the old DOS character set (not
    /// UTF-8), as older tools wrote them.
    fn dos_zip(path: &Path, files: &[(&[u8], &[u8])]) {
        let (mut body, mut dir) = (Vec::new(), Vec::new());
        for (name, data) in files {
            let crc = crc32fast::hash(data);
            let at = body.len() as u32;
            let le16 = |v: usize| (v as u16).to_le_bytes();
            let mut head = Vec::new();
            head.extend(b"PK\x03\x04");
            head.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version, flags 0 (not UTF-8), stored, time, date
            head.extend(crc.to_le_bytes());
            head.extend((data.len() as u32).to_le_bytes());
            head.extend((data.len() as u32).to_le_bytes());
            head.extend(le16(name.len()));
            head.extend([0, 0]);
            head.extend(*name);
            body.extend(&head);
            body.extend(*data);
            dir.extend(b"PK\x01\x02");
            dir.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            dir.extend(crc.to_le_bytes());
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend(le16(name.len()));
            dir.extend([0; 12]); // extra, comment, disk, internal and external attributes
            dir.extend(at.to_le_bytes());
            dir.extend(*name);
        }
        let mut out = body.clone();
        out.extend(&dir);
        out.extend(b"PK\x05\x06");
        out.extend([0, 0, 0, 0]);
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((dir.len() as u32).to_le_bytes());
        out.extend((body.len() as u32).to_le_bytes());
        out.extend([0, 0]);
        fs::write(path, out).unwrap();
    }

    /// A page whose name has a character outside plain ASCII in an old ZIP (Conan Omnibus v05:
    /// 0xFF, a non-breaking space in the DOS character set) is listed and read like any other.
    #[test]
    fn pages_named_in_the_old_character_set() {
        let dir = std::env::temp_dir().join(format!("aion-dosname-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let book = dir.join("book.cbz");
        dos_zip(&book, &[(b"Conan\xffv05/001.jpg", b"first page"), (b"Conan\xffv05/002.jpg", b"second page")]);
        let listing = list(&book).unwrap();
        assert_eq!(listing.pages, ["Conan\u{a0}v05/001.jpg", "Conan\u{a0}v05/002.jpg"]);
        let archives = Archives::new(&dir.join("cache"));
        assert_eq!(archives.page(1, &book, 1).unwrap().0, b"second page");
        assert_eq!(archives.first_page(1, &book).unwrap(), b"first page");
        let _ = fs::remove_dir_all(&dir);
    }

    /// While two pages are being read, a request waits; one for a page far from a newer request
    /// is dropped, and the newest goes as soon as a reader is free.
    #[test]
    fn newest_page_first() {
        let gate = Arc::new(Gate::default());
        let (a, b) = (gate.enter(0).unwrap(), gate.enter(1).unwrap());
        let g = gate.clone();
        let passed = std::thread::spawn(move || g.enter(2).is_some());
        std::thread::sleep(Duration::from_millis(50));
        let g = gate.clone();
        let landed = std::thread::spawn(move || g.enter(9).is_some());
        assert!(!passed.join().unwrap(), "skipped past: dropped");
        std::thread::sleep(Duration::from_millis(50));
        drop(a);
        assert!(landed.join().unwrap(), "the newest gets the free reader");
        drop(b);
    }

    /// A page well over screen height gets a copy at it, the same shape; one near it gets none.
    #[test]
    fn screen_copies() {
        let page = jpeg(|x, y| [(x % 256) as u8, (y % 256) as u8, 90]);
        let copy = make_screen_copy(&page, 100).unwrap().expect("300 tall on a 100 tall screen");
        let img = image::load_from_memory(&copy).unwrap();
        assert_eq!((img.width(), img.height()), (67, 100));
        assert!(make_screen_copy(&page, 280).unwrap().is_none(), "300 tall on a 280 tall screen");
    }

    fn jpeg(paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_fn(200, 300, |x, y| image::Rgb(paint(x, y)));
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&img).unwrap();
        out
    }

    #[test]
    fn cover_colors() {
        // A red title block over a mostly black cover reads as red, not as the dark average.
        let red = cover_color(&jpeg(|_, y| if y < 90 { [200, 40, 30] } else { [10, 10, 10] })).unwrap();
        let [r, g, b] = [1, 3, 5].map(|i| u8::from_str_radix(&red[i..i + 2], 16).unwrap());
        assert!(r > 150 && g < 80 && b < 80, "{red}");
        // A black and white cover gets its gray.
        let gray = cover_color(&jpeg(|x, _| if x < 100 { [240, 240, 240] } else { [20, 20, 20] })).unwrap();
        let [r, g, b] = [1, 3, 5].map(|i| u8::from_str_radix(&gray[i..i + 2], 16).unwrap());
        assert!(r.abs_diff(g) < 6 && g.abs_diff(b) < 6 && (90..170).contains(&r), "{gray}");
    }
}


#[cfg(test)]
mod new_kinds {
    use super::*;
    use std::io::Write as _;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aion-kinds-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }
    /// A small real picture, `w` by `h`.
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbImage::new(w, h).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn cb7_pages() {
        let d = dir("cb7");
        let file = d.join("Book 001.cb7");
        let mut w = sevenz_rust2::ArchiveWriter::create(&file).unwrap();
        for name in ["pages/10.png", "pages/2.png", "pages/1.png", "notes.txt"] {
            w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(Cursor::new(name.as_bytes().to_vec()))).unwrap();
        }
        w.finish().unwrap();
        let listing = list(&file).unwrap();
        assert_eq!(listing.kind, Kind::SevenZ);
        assert_eq!(listing.pages, ["pages/1.png", "pages/2.png", "pages/10.png"]);
        let archives = Archives::new(&d.join("cache"));
        assert_eq!(archives.page(1, &file, 2).unwrap().0, b"pages/10.png");
        assert_eq!(archives.page(1, &file, 0).unwrap().0, b"pages/1.png");
        assert_eq!(archives.first_page(1, &file).unwrap(), b"pages/1.png");
        archives.release(1);
        fs::remove_file(&file).unwrap();
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_finished_book_can_be_opened_again() {
        // Finishing a book clears its unpacked pages when you open the next; going back must still work.
        let d = dir("reopen");
        let file = d.join("Book.cb7");
        let mut w = sevenz_rust2::ArchiveWriter::create(&file).unwrap();
        for name in ["1.png", "2.png"] {
            w.push_archive_entry(sevenz_rust2::ArchiveEntry::new_file(name), Some(Cursor::new(name.as_bytes().to_vec()))).unwrap();
        }
        w.finish().unwrap();
        let archives = Archives::new(&d.join("cache"));
        assert_eq!(archives.page(7, &file, 0).unwrap().0, b"1.png");
        archives.page(7, &file, 1).unwrap();
        let job = archives.rars.lock().unwrap().get(7).unwrap();
        let st = job.state.lock().unwrap();
        drop(job.cv.wait_timeout_while(st, Duration::from_secs(5), |s| !s.done).unwrap());
        archives.forget_book(7);
        assert_eq!(archives.page(7, &file, 1).unwrap().0, b"2.png");
        archives.release(7);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn epub_pages_follow_its_contents() {
        let d = dir("epub");
        let file = d.join("Book.epub");
        let mut z = zip::ZipWriter::new(File::create(&file).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        let opf = r#"<?xml version="1.0"?><package><manifest>
            <item id="b" href="text/b.xhtml" media-type="application/xhtml+xml"/>
            <item media-type="application/xhtml+xml" id="a" href="text/a.xhtml"/>
            <item id="c" href="images/z%20cover.jpg" media-type="image/jpeg"/>
          </manifest><spine><itemref idref="c"/><itemref idref="b"/><itemref idref="a"/></spine></package>"#;
        for (name, data) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="x"/></rootfiles></container>"#),
            ("OEBPS/content.opf", opf),
            ("OEBPS/text/b.xhtml", r#"<html><body><img class="p" src="../images/b.jpg"/><svg><image xlink:href="../images/b2.png"/></svg></body></html>"#),
            ("OEBPS/text/a.xhtml", r#"<html><body><img src='../images/a.jpg'/><img src="../images/missing.jpg"/></body></html>"#),
            ("OEBPS/images/z cover.jpg", "cover"),
            ("OEBPS/images/b.jpg", "b"),
            ("OEBPS/images/b2.png", "b2"),
            ("OEBPS/images/a.jpg", "a"),
            ("OEBPS/images/extra.jpg", "extra"),
        ] {
            z.start_file(name, o).unwrap();
            z.write_all(data.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        let listing = list(&file).unwrap();
        assert_eq!(listing.kind, Kind::Zip);
        assert_eq!(listing.pages, ["OEBPS/images/z cover.jpg", "OEBPS/images/b.jpg", "OEBPS/images/b2.png", "OEBPS/images/a.jpg"]);
        let archives = Archives::new(&d.join("cache"));
        assert_eq!(archives.page(2, &file, 1).unwrap().0, b"b");
        archives.release(2);

        // With no contents to go by, its pictures in name order.
        let plain = d.join("Plain.epub");
        let mut z = zip::ZipWriter::new(File::create(&plain).unwrap());
        for name in ["10.jpg", "2.jpg"] {
            z.start_file(name, o).unwrap();
            z.write_all(b"x").unwrap();
        }
        z.finish().unwrap();
        assert_eq!(list(&plain).unwrap().pages, ["2.jpg", "10.jpg"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(windows)]
    #[test]
    fn pdf_pages_are_drawn() {
        // A small real PDF: two pages, the second wider than it is tall.
        let d = dir("pdf");
        let file = d.join("Book.pdf");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 600] >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 800 400] >>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut at = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            at.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
        }
        let xref = pdf.len();
        pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
        for a in at {
            pdf.push_str(&format!("{a:010} 00000 n \n"));
        }
        pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
        fs::write(&file, pdf).unwrap();

        let listing = list(&file).unwrap();
        assert_eq!(listing.kind, Kind::Pdf);
        assert_eq!(listing.pages.len(), 2);
        let archives = Archives::new(&d.join("cache"));
        let (bytes, mime) = archives.page(3, &file, 1).unwrap();
        assert_eq!(mime, "image/png");
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (2400, 1200));
        assert!(fs::read(archives.book_dir(3, &file).unwrap().join("1")).is_ok(), "kept for next time");
        assert!(image::load_from_memory(&archives.first_page(3, &file).unwrap()).is_ok());
        archives.release(3);
        let _ = fs::remove_dir_all(&d);
    }

    /// Your real PDFs and EPUB, only read: their pages, and a first page of each drawn into a temporary
    /// cache. `cargo test --lib real_new_kinds -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_new_kinds() {
        let d = dir("real");
        let archives = Archives::new(&d.join("cache"));
        for (id, f) in [
            (11, r"H:\Comics\Indie\The Legend of Zelda Encyclopedia\The Legend of Zelda Encyclopedia (2018).pdf"),
            (12, r"H:\Comics\Manga\Fullmetal Alchemist\Fullmetal Alchemist - The Abducted Alchemist.epub"),
        ] {
            let started = std::time::Instant::now();
            let listing = list(Path::new(f)).unwrap();
            let middle = listing.pages.len() / 2;
            let (bytes, mime) = archives.page(id, Path::new(f), middle).unwrap();
            let img = image::load_from_memory(&bytes).unwrap();
            println!("{f}: {:?}, {} pages; page {middle} is {mime}, {}x{}, {} KB, in {:?}", listing.kind, listing.pages.len(), img.width(), img.height(), bytes.len() / 1024, started.elapsed());
            archives.release(id);
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn tall_pages_are_noticed() {
        let d = dir("tall");
        let book = |name: &str, w: u32, h: u32| {
            let file = d.join(name);
            let mut z = zip::ZipWriter::new(File::create(&file).unwrap());
            for p in 1..=6 {
                z.start_file(format!("{p}.png"), zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(&png(w, h)).unwrap();
            }
            z.finish().unwrap();
            file
        };
        let archives = Archives::new(&d.join("cache"));
        assert!(archives.pages_look_tall(4, &book("Webtoon.cbz", 80, 400)));
        assert!(!archives.pages_look_tall(5, &book("Comic.cbz", 200, 300)));
        assert!(!archives.pages_look_tall(6, &book("Wide.cbz", 400, 200)));
        let _ = fs::remove_dir_all(&d);
    }
}
