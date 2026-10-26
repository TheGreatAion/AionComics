//! Reading pages out of comic archives (CBZ/ZIP and CBR/RAR) and plain folders of images.
//!
//! ZIP archives support random access, so pages are read on demand from a
//! small pool of open archives. RAR archives can only be read front to back,
//! so a background thread unpacks a whole CBR into a cache folder, from the
//! page you're on to the end and then the start; a page it hasn't reached yet
//! is read on its own meanwhile. Unpacked pages are kept between sessions,
//! up to PAGES_CAP, and page lists are remembered on disk, because listing a
//! big CBR means reading through it.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
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
        bail!("7-Zip archives aren't supported yet")
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

pub fn list(path: &Path) -> Result<Listing> {
    let kind = detect(path)?;
    let mut pages = Vec::new();
    let mut solid = false;
    let (mut parts, mut head) = (Vec::new(), 0);
    match kind {
        Kind::Zip => {
            let z = open_zip(path)?;
            for n in z.file_names() {
                let n = n?;
                if is_image(&n) {
                    pages.push(n.into_owned());
                }
            }
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
    sort_pages(&mut pages);
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
            Kind::Rar => self.rar_read(id, path, &listing, index)?,
            Kind::Folder => fs::read(path.join(name))?,
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
            Kind::Rar => match fs::read(self.book_dir(id, path)?.join(index.to_string())) {
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
        }
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
            let mut f = z.by_name(name)?;
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
            let res = extract_rar(&j, &path, &listing, from);
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
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&copy)?;
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
