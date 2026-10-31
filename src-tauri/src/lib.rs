mod archive;
mod backup;
mod comicinfo;
mod cv;
mod db;
mod editions;
mod frames;
mod intake;
mod panels;
mod rarindex;
mod scan;
mod stats;
mod warm;

/// Bump when panel detection changes so cached results are recomputed.
const PANELS_VERSION: i64 = 11;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    data_dir: PathBuf,
    db_path: PathBuf,
    conn: Mutex<Connection>,
    thumbs_dir: PathBuf,
    archives: archive::Archives,
    scanning: AtomicBool,
    /// Files and folders opened from outside the library this session. Book id `-(i + 1)` is entry `i`.
    external: Mutex<Vec<PathBuf>>,
    comicvine: cv::Client,
    /// Changes seen in the comics folder, rescanned once they settle.
    changes: Arc<intake::Changes>,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// A ComicInfo.xml pass is running.
    reading_info: AtomicBool,
    /// When a page was last served (ms since the epoch): background passes over the drive wait
    /// while you're reading, so they never slow a page turn.
    last_page: std::sync::atomic::AtomicI64,
    /// A comic Windows asked us to open at launch, until the window picks it up.
    open_request: Mutex<Option<String>>,
    /// The book and page the reader asked for last: background work goes on around it.
    reading: Mutex<Option<(i64, usize)>>,
    /// The book whose page previews are being made, and the flag that stops it (see warm.rs).
    warming: Mutex<Option<(i64, Arc<AtomicBool>)>>,
}

type Shared = Arc<AppState>;
type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl AppState {
    fn root(&self) -> Option<PathBuf> {
        let conn = self.conn.lock().unwrap();
        db::get_setting(&conn, "library_root").ok().flatten().map(PathBuf::from)
    }

    /// Where a page's small preview is kept: in the book's cache folder, under PAGES_CAP with the
    /// rest (see archive.rs). Library books only: outside ones get new ids each session.
    fn page_thumb_path(&self, id: i64, path: &Path, n: usize) -> anyhow::Result<PathBuf> {
        anyhow::ensure!(id > 0, "previews are kept for library books only");
        Ok(self.archives.book_dir(id, path)?.join(format!("preview-{n}.jpg")))
    }

    fn comic_path(&self, id: i64) -> anyhow::Result<(PathBuf, db::ComicFile)> {
        if id < 0 {
            let ext = self.external.lock().unwrap();
            let path = ext.get((-id - 1) as usize).ok_or_else(|| anyhow::anyhow!("Unknown book"))?;
            return Ok((path.clone(), db::ComicFile { rel_path: String::new(), yac_hash: None }));
        }
        let root = self.root().ok_or_else(|| anyhow::anyhow!("No library folder set"))?;
        let f = db::comic_file(&self.conn.lock().unwrap(), id)?;
        Ok((root.join(&f.rel_path), f))
    }

    /// Where `path` sits inside the library folder, as stored in `comics.rel_path` (forward slashes).
    fn library_rel(&self, path: &Path) -> Option<String> {
        let root = self.root()?;
        let norm = |p: &Path| p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
        let (root_s, path_s) = (norm(&root), norm(path));
        let rest = path_s.strip_prefix(&root_s)?.strip_prefix('/')?;
        // Keep the original casing of the remainder.
        let full = path.to_string_lossy().replace('\\', "/");
        Some(full[full.len() - rest.len()..].to_string())
    }

    /// AionComics never writes into the library folder, even when asked to save there.
    fn guard_outside_library(&self, dest: &Path) -> anyhow::Result<()> {
        let probe = if dest.exists() { dest.to_path_buf() } else { dest.parent().unwrap_or(dest).to_path_buf() };
        let real = std::fs::canonicalize(&probe).unwrap_or(probe);
        if let Some(root) = self.root() {
            let root = std::fs::canonicalize(&root).unwrap_or(root);
            let low = |p: &Path| p.to_string_lossy().to_lowercase();
            if low(&real).starts_with(&low(&root)) {
                anyhow::bail!("AionComics doesn't write into your comics folder. Pick somewhere else, like Downloads.");
            }
        }
        Ok(())
    }
}

// ---------- scanning ----------

/// How long after launch the library scan, cache tidying and backup wait, so the first screen
/// has the database to itself.
const STARTUP_QUIET: std::time::Duration = std::time::Duration::from_secs(3);

/// Choices on the Settings page that the engine acts on, kept in the database's settings.
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
struct Options {
    /// Rescan by itself when the comics folder changes.
    watch_folder: bool,
    /// Read the details stored inside the comic files (ComicInfo.xml).
    read_file_details: bool,
    /// A book not opened for this many days has its cache cleared.
    cache_days: u64,
    /// How many backups are kept.
    backups_keep: usize,
}

impl Options {
    fn load(conn: &Connection) -> Options {
        let get = |k: &str| db::get_setting(conn, k).ok().flatten();
        Options {
            watch_folder: get("watch_folder").is_none_or(|v| v != "0"),
            read_file_details: get("read_file_details").is_none_or(|v| v != "0"),
            cache_days: get("cache_days").and_then(|v| v.parse().ok()).unwrap_or(30),
            backups_keep: backup::keep(conn),
        }
    }

    fn save(&self, conn: &Connection) -> anyhow::Result<()> {
        let flag = |b: bool| if b { "1" } else { "0" };
        db::set_setting(conn, "watch_folder", flag(self.watch_folder))?;
        db::set_setting(conn, "read_file_details", flag(self.read_file_details))?;
        db::set_setting(conn, "cache_days", &self.cache_days.max(1).to_string())?;
        db::set_setting(conn, "backups_keep", &self.backups_keep.max(1).to_string())
    }
}

/// Keep the page cache to what's likely to be read again: a book you've finished, or haven't opened
/// for the days chosen in Settings, is cleared (all of it comes back if it's opened again), and a
/// CBR keeps no unpacked pages it can do without. `keep`: the book being opened, left as it is.
fn tidy_cache(state: &AppState, keep: Option<i64>) {
    let days = Options::load(&state.conn.lock().unwrap()).cache_days;
    let month = std::time::Duration::from_secs(days * 24 * 3600);
    for (id, used) in state.archives.cached_books() {
        if Some(id) == keep {
            continue;
        }
        let finished = db::card_by_id(&state.conn.lock().unwrap(), id).map(|c| c.read).unwrap_or(true);
        if finished || used.elapsed().is_ok_and(|age| age > month) {
            state.archives.forget_book(id);
        } else if let Ok((path, _)) = state.comic_path(id) {
            state.archives.trim_unpacked(id, &path);
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Watch the comics folder, so books added or removed outside AionComics show up by themselves.
fn watch_library(state: &Shared) {
    let on = Options::load(&state.conn.lock().unwrap()).watch_folder;
    let watcher = state.root().filter(|_| on).and_then(|root| intake::watch(&root, state.changes.clone()).ok());
    *state.watcher.lock().unwrap() = watcher;
}

/// Make the prints (see `print`) of series covers that don't have one yet, a cover at a time,
/// giving way to reading and to a rescan.
fn make_prints(state: &Shared, conn: &Connection) {
    let Ok(covers) = db::series_covers(conn) else { return };
    for id in covers {
        if print_path(state, id).exists() {
            continue;
        }
        while now_ms() - state.last_page.load(Ordering::SeqCst) < 5000 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        if state.scanning.load(Ordering::SeqCst) {
            return;
        }
        let _ = print(state, id);
    }
}

/// Read the ComicInfo.xml of books not checked yet, a few at a time, on a connection of its own.
fn read_comic_info(app: &AppHandle, state: &Shared, conn: Connection, root: &Path) {
    if state.reading_info.swap(true, Ordering::SeqCst) {
        return;
    }
    let conn = Mutex::new(conn);
    loop {
        // Reading comes first: wait until no page has been asked for in a few seconds.
        while now_ms() - state.last_page.load(Ordering::SeqCst) < 5000 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        let Ok(Some(changed)) = comicinfo::step(&conn, root, 10) else { break };
        if !changed.is_empty() {
            let _ = app.emit("metadata-changed", serde_json::json!({ "comicId": null, "left": 0 }));
            // A ComicInfo.xml may name the book's Comic Vine issue.
            state.comicvine.wake.notify_all();
        }
        // A rescan picks up where this leaves off; don't compete with it for the drive.
        if state.scanning.load(Ordering::SeqCst) {
            break;
        }
    }
    state.reading_info.store(false, Ordering::SeqCst);
}

fn start_scan(app: &AppHandle, state: &Shared) {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(root) = state.root() else {
        state.scanning.store(false, Ordering::SeqCst);
        return;
    };
    let (app, state) = (app.clone(), state.clone());
    let _ = app.emit("scan-started", ());
    std::thread::spawn(move || {
        // Separate connection so browsing stays responsive during a scan.
        let result = db::open(&state.db_path).and_then(|mut c| {
            let stats = scan::scan(&mut c, &root)?;
            db::set_setting(&c, "last_scan", &serde_json::to_string(&stats)?)?;
            Ok((stats, c))
        });
        state.scanning.store(false, Ordering::SeqCst);
        match result {
            Ok((stats, mut c)) => {
                let _ = app.emit("library-changed", stats);
                // New books in series already matched to Comic Vine can be matched straight away.
                state.comicvine.wake.notify_all();
                // Learn the page lists of the books you're likely to open next, so even a big CBR opens at once.
                // Wait a little first: on a spinning drive this shouldn't compete with the first book you open.
                std::thread::sleep(std::time::Duration::from_secs(10));
                let now = scan::now();
                let likely = [
                    db::continue_reading(&c, now, 30),
                    db::up_next(&c, 30),
                    db::pick_up_again(&c, now, 30),
                ];
                for card in likely.into_iter().flatten().flatten() {
                    if let Ok((path, _)) = state.comic_path(card.id) {
                        let _ = state.archives.listing(card.id, &path);
                    }
                }
                // Cover shapes are a slower second pass over the drive.
                // And their colors, from the covers already made (new covers get theirs as they're made).
                if let Ok(n) = scan::cover_colors(&mut c, &root, &state.thumbs_dir) {
                    if n > 0 {
                        let _ = app.emit("library-changed", ());
                    }
                }
                if let Ok(n) = scan::cover_ratios(&mut c, &root) {
                    if n > 0 {
                        let _ = app.emit("library-changed", ());
                    }
                }
                // Every series' cover printed for behind its collection's name, so those pages
                // never wait on one.
                make_prints(&state, &c);
                // Then the details stored inside the files themselves.
                if Options::load(&c).read_file_details {
                    read_comic_info(&app, &state, c, &root);
                }
            }
            Err(e) => {
                let _ = app.emit("scan-failed", e.to_string());
            }
        }
    });
}

// ---------- commands ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    library_root: Option<String>,
    last_scan: Option<serde_json::Value>,
    scanning: bool,
    options: Options,
    /// Where the library database and backups are kept.
    data_dir: String,
}

#[tauri::command(async)]
fn get_settings(state: State<Shared>) -> CmdResult<Settings> {
    let conn = state.conn.lock().unwrap();
    Ok(Settings {
        library_root: db::get_setting(&conn, "library_root").map_err(err)?,
        last_scan: db::get_setting(&conn, "last_scan")
            .map_err(err)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        scanning: state.scanning.load(Ordering::SeqCst),
        options: Options::load(&conn),
        data_dir: state.data_dir.display().to_string(),
    })
}

/// Save the Settings page's choices, and act on the ones that change something now.
#[tauri::command]
fn set_options(app: AppHandle, state: State<Shared>, options: Options) -> CmdResult<()> {
    let before = {
        let conn = state.conn.lock().unwrap();
        let before = Options::load(&conn);
        options.save(&conn).map_err(err)?;
        before
    };
    if options.watch_folder != before.watch_folder {
        watch_library(&state);
    }
    if options.read_file_details && !before.read_file_details {
        if let Some(root) = state.root() {
            let (app, state) = (app.clone(), state.inner().clone());
            std::thread::spawn(move || {
                if let Ok(c) = db::open(&state.db_path) {
                    read_comic_info(&app, &state, c, &root);
                }
            });
        }
    }
    if options.cache_days < before.cache_days {
        let state = state.inner().clone();
        std::thread::spawn(move || tidy_cache(&state, None));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Storage {
    /// Pages kept so books open at once: unpacked CBR pages, screen copies, page previews.
    pages: u64,
    /// The covers printed behind collections' names.
    prints: u64,
    /// Books' page lists and Comic Vine answers.
    lists: u64,
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// How much room AionComics' cache takes, by kind.
#[tauri::command]
async fn storage(state: State<'_, Shared>) -> CmdResult<Storage> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let cache = state.thumbs_dir.parent().unwrap_or(&state.thumbs_dir).to_path_buf();
        Storage {
            pages: dir_size(&cache.join("pages")),
            // Covers mostly come from YACReader's cache in the comics folder: next to nothing here.
            prints: dir_size(&state.thumbs_dir),
            lists: dir_size(&cache.join("listings")) + dir_size(&cache.join("comicvine")),
        }
    })
    .await
    .map_err(err)
}

/// Clear every book's kept pages, except the book being read. They come back as books are opened.
#[tauri::command]
async fn clear_pages(state: State<'_, Shared>) -> CmdResult<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let reading = state.reading.lock().unwrap().map(|(book, _)| book);
        for (id, _) in state.archives.cached_books() {
            if Some(id) != reading {
                state.archives.forget_book(id);
            }
        }
    })
    .await
    .map_err(err)
}

/// Show the folder with the library database and backups in Explorer.
#[tauri::command]
fn open_data_folder(state: State<Shared>) -> CmdResult<()> {
    #[cfg(windows)]
    std::process::Command::new("explorer").arg(&state.data_dir).spawn().map_err(err)?;
    #[cfg(not(windows))]
    let _ = &state;
    Ok(())
}

#[tauri::command]
fn set_library_root(app: AppHandle, state: State<Shared>, path: String) -> CmdResult<()> {
    if !Path::new(&path).is_dir() {
        return Err(format!("{path} isn't a folder"));
    }
    db::set_setting(&state.conn.lock().unwrap(), "library_root", &path).map_err(err)?;
    watch_library(&state);
    start_scan(&app, &state);
    Ok(())
}

/// Where comic files dropped on the window would go (see `intake::plan`).
#[tauri::command]
fn plan_add(state: State<Shared>, paths: Vec<String>, series_id: Option<i64>, collection_id: Option<i64>) -> CmdResult<Vec<intake::Placement>> {
    let root = state.root().ok_or("No comics folder set")?;
    intake::plan(&state.conn.lock().unwrap(), &root, &paths, series_id, collection_id).map_err(err)
}

/// Copy dropped comic files into the comics folder, then rescan. Returns how many were added.
#[tauri::command]
async fn add_comics(
    app: AppHandle,
    state: State<'_, Shared>,
    paths: Vec<String>,
    series_id: Option<i64>,
    collection_id: Option<i64>,
) -> CmdResult<usize> {
    let state = state.inner().clone();
    let st = state.clone();
    let added = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<usize> {
        let root = st.root().ok_or_else(|| anyhow::anyhow!("No comics folder set"))?;
        let placed = intake::plan(&st.conn.lock().unwrap(), &root, &paths, series_id, collection_id)?;
        intake::copy_in(&root, &placed)
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    if added > 0 {
        start_scan(&app, &state);
    }
    Ok(added)
}

/// The comic Windows asked AionComics to open when it started, if any (asked once by the window).
#[tauri::command]
fn take_open_request(state: State<Shared>) -> Option<String> {
    state.open_request.lock().unwrap().take()
}

/// A comic file among command-line arguments (how Windows passes "Open with").
fn comic_arg(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().skip(1).find(|a| {
        let p = Path::new(a);
        p.is_file() && p.extension().is_some_and(|e| ["cbz", "cbr", "zip", "rar"].contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
    })
}

#[tauri::command]
fn rescan(app: AppHandle, state: State<Shared>) {
    start_scan(&app, &state);
}

/// The database backups there are, newest first.
#[tauri::command]
fn backups(state: State<Shared>) -> CmdResult<Vec<backup::Backup>> {
    backup::list(&backup::dir(&state.data_dir)).map_err(err)
}

#[tauri::command]
fn backup_now(state: State<Shared>) -> CmdResult<backup::Backup> {
    backup::make(&state.conn.lock().unwrap(), &backup::dir(&state.data_dir), scan::now()).map_err(err)
}

/// Put a backup back: today's state is backed up first (so this can be undone), then AionComics
/// restarts and the copy goes in before the database opens.
#[tauri::command]
fn restore_backup(app: AppHandle, state: State<Shared>, name: String) -> CmdResult<()> {
    let dir = backup::dir(&state.data_dir);
    backup::make(&state.conn.lock().unwrap(), &dir, scan::now()).map_err(err)?;
    backup::schedule(&state.data_dir, &name).map_err(err)?;
    relaunch().map_err(err)?;
    app.exit(0);
    Ok(())
}

/// Start AionComics again a moment after this copy has gone (with only one copy allowed at a
/// time, a new one started straight away would hand over to this one and quit).
fn relaunch() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new("cmd");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // cmd reads its own quoting, so the line goes through as written. No console window,
        // and not tied to this process.
        cmd.raw_arg(format!("/C ping -n 3 127.0.0.1 >nul & start \"\" \"{}\"", exe.display()));
        cmd.creation_flags(0x0800_0000 | 0x0000_0008);
    }
    cmd.spawn()?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Home {
    continue_reading: Vec<db::ComicCard>,
    pick_up_again: Vec<db::ComicCard>,
}

#[tauri::command(async)]
fn home(state: State<Shared>) -> CmdResult<Home> {
    let conn = state.conn.lock().unwrap();
    let now = scan::now();
    (|| -> anyhow::Result<Home> {
        Ok(Home {
            continue_reading: db::continue_reading(&conn, now, 24)?,
            pick_up_again: db::pick_up_again(&conn, now, 24)?,
        })
    })()
    .map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Nav {
    collections: Vec<db::CollectionSummary>,
}

/// Everything the sidebar needs.
#[tauri::command(async)]
fn nav(state: State<Shared>) -> CmdResult<Nav> {
    let conn = state.conn.lock().unwrap();
    (|| -> anyhow::Result<Nav> {
        Ok(Nav {
            collections: db::collections(&conn)?,
        })
    })()
    .map_err(err)
}

#[tauri::command(async)]
fn search(state: State<Shared>, q: String) -> CmdResult<Vec<db::SeriesSummary>> {
    if q.trim().is_empty() {
        return Ok(vec![]);
    }
    db::search(&state.conn.lock().unwrap(), &q).map_err(err)
}

#[tauri::command]
fn mark_read_before(state: State<Shared>, id: i64) -> CmdResult<()> {
    db::mark_read_before(&state.conn.lock().unwrap(), id, scan::now()).map_err(err)
}

#[tauri::command]
fn dismiss_backfill(state: State<Shared>, series_id: i64) -> CmdResult<()> {
    db::set_setting(&state.conn.lock().unwrap(), &format!("backfill_dismissed:{series_id}"), "1").map_err(err)
}

/// Show a comic's file selected in Explorer.
#[tauri::command]
fn reveal(state: State<Shared>, id: i64) -> CmdResult<()> {
    let (path, _) = state.comic_path(id).map_err(err)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map_err(err)?;
    }
    #[cfg(not(windows))]
    let _ = path;
    Ok(())
}

#[tauri::command]
fn set_series_read(state: State<Shared>, series_id: i64, read: bool) -> CmdResult<()> {
    db::set_series_read(&state.conn.lock().unwrap(), series_id, read, scan::now()).map_err(err)
}

#[tauri::command(async)]
fn collection(state: State<Shared>, id: i64) -> CmdResult<db::CollectionView> {
    let mut view = db::collection_view(&state.conn.lock().unwrap(), id).map_err(err)?;
    for s in &mut view.series {
        s.printed = s.cover_id.is_some_and(|c| print_path(&state, c).exists());
    }
    Ok(view)
}

#[tauri::command(async)]
fn series(state: State<Shared>, id: i64) -> CmdResult<db::SeriesView> {
    db::series_view(&state.conn.lock().unwrap(), id).map_err(err)
}

/// Books that look like copies of one another.
#[tauri::command(async)]
fn duplicates(state: State<Shared>) -> CmdResult<Vec<db::DupGroup>> {
    db::duplicates(&state.conn.lock().unwrap()).map_err(err)
}

/// Stop showing a group: you've checked and they're different books.
#[tauri::command]
fn ignore_duplicate(state: State<Shared>, key: String) -> CmdResult<()> {
    db::ignore_duplicate(&state.conn.lock().unwrap(), &key).map_err(err)
}

/// Every book by a creator, or with a character or team in it.
#[tauri::command(async)]
fn people(state: State<Shared>, kind: String, name: String) -> CmdResult<db::PeopleView> {
    db::people_view(&state.conn.lock().unwrap(), &kind, &name).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReaderInfo {
    comic: db::ComicCard,
    page_count: usize,
    start_page: usize,
    prev_id: Option<i64>,
    next: Option<db::ComicCard>,
    bookmarks: Vec<i64>,
    /// Opened from outside the library: progress, bookmarks and panel edits aren't kept.
    external: bool,
}

/// Open a file or folder picked from disk. Books inside the library open as themselves.
#[tauri::command]
fn open_path(state: State<Shared>, path: String) -> CmdResult<i64> {
    let path = PathBuf::from(path);
    if let Some(rel) = state.library_rel(&path) {
        if let Some(id) = db::comic_id_by_path(&state.conn.lock().unwrap(), &rel).map_err(err)? {
            return Ok(id);
        }
    }
    archive::list(&path).map_err(err)?;
    let mut ext = state.external.lock().unwrap();
    ext.push(path);
    Ok(-(ext.len() as i64))
}

fn page_ext(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    }
}

/// Save one page to a file the user picked.
#[tauri::command]
async fn save_page(state: State<'_, Shared>, id: i64, page: usize, dest: String) -> CmdResult<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<()> {
        let dest = PathBuf::from(dest);
        state.guard_outside_library(&dest)?;
        let (path, _) = state.comic_path(id)?;
        let (bytes, _) = state.archives.page(id, &path, page)?;
        std::fs::write(&dest, bytes)?;
        Ok(())
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Save pages `from..=to` (0-based) into a folder. Returns how many were written.
#[tauri::command]
async fn extract_pages(state: State<'_, Shared>, id: i64, from: usize, to: usize, dir: String) -> CmdResult<usize> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<usize> {
        let dir = PathBuf::from(dir);
        state.guard_outside_library(&dir.join("page"))?;
        let (path, _) = state.comic_path(id)?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "page".into());
        let mut n = 0;
        for i in from..=to {
            let (bytes, mime) = state.archives.page(id, &path, i)?;
            std::fs::write(dir.join(format!("{stem} - {:03}.{}", i + 1, page_ext(mime))), bytes)?;
            n += 1;
        }
        Ok(n)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

// ---------- metadata ----------

/// Everything known about one book, and which fields were edited by hand or came from Comic Vine.
#[tauri::command(async)]
fn comic_meta(state: State<Shared>, id: i64) -> CmdResult<db::ComicMeta> {
    db::comic_meta(&state.conn.lock().unwrap(), id).map_err(err)
}

/// Who's in each of these books: teams, then characters.
#[tauri::command(async)]
fn casts(state: State<Shared>, ids: Vec<i64>) -> CmdResult<Vec<(Option<String>, Option<String>)>> {
    db::casts(&state.conn.lock().unwrap(), &ids).map_err(err)
}

/// Hand edits: each field given is saved; `null` drops your edit for that field.
#[tauri::command]
fn edit_comic_meta(state: State<Shared>, id: i64, changes: db::Meta) -> CmdResult<db::ComicMeta> {
    if id < 0 {
        return Err("Details are kept for books in your library.".into());
    }
    let conn = state.conn.lock().unwrap();
    db::edit_meta(&conn, id, &changes, scan::now()).map_err(err)?;
    db::comic_meta(&conn, id).map_err(err)
}

#[tauri::command]
fn cv_status(state: State<Shared>) -> CmdResult<cv::Status> {
    cv::status(&state.comicvine, &state.conn.lock().unwrap()).map_err(err)
}

/// Open Comic Vine's API page, where a free key is had, in the default browser. Only this one
/// address: the page can't ask for anything else to be opened.
#[tauri::command]
fn cv_open_site() -> CmdResult<()> {
    std::process::Command::new("explorer").arg("https://comicvine.gamespot.com/api/").spawn().map_err(err)?;
    Ok(())
}

/// Save a Comic Vine API key after checking Comic Vine accepts it. An empty key removes it.
#[tauri::command]
async fn cv_set_key(state: State<'_, Shared>, key: String) -> CmdResult<()> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<()> {
        let key = key.trim().to_string();
        if !key.is_empty() {
            let probe = cv::Client::new(&std::env::temp_dir(), Some(key.clone()));
            probe.get("search", &[("resources", "volume".into()), ("query", "batman".into()), ("limit", "1".into())], 0, false)?;
        }
        db::set_setting(&state.conn.lock().unwrap(), "cv_api_key", &key)?;
        state.comicvine.set_key(Some(key));
        Ok(())
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CvSearch {
    query: String,
    volumes: Vec<cv::Volume>,
}

/// Comic Vine series that could be this one, best first. Without a query, searches by the series name.
#[tauri::command]
async fn cv_search(state: State<'_, Shared>, series_id: i64, query: Option<String>) -> CmdResult<CvSearch> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<CvSearch> {
        // Its own connection: the library stays usable while Comic Vine answers.
        let conn = db::open(&state.db_path)?;
        let query = match query.filter(|q| !q.trim().is_empty()) {
            Some(q) => q.trim().to_string(),
            None => cv::default_query(&conn, series_id)?,
        };
        let volumes = cv::search(&state.comicvine, &conn, series_id, &query)?;
        Ok(CvSearch { query, volumes })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// A Comic Vine series' issues and the issue proposed for each of this series' books.
#[tauri::command]
async fn cv_plan(state: State<'_, Shared>, series_id: i64, volume_id: String) -> CmdResult<cv::IssuePlan> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&state.db_path)?;
        cv::plan(&state.comicvine, &conn, series_id, &volume_id, true)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Save the matches. Credits and characters follow in the background.
#[tauri::command]
async fn cv_apply(state: State<'_, Shared>, series_id: i64, volume_id: String, pairs: Vec<cv::Pair>) -> CmdResult<usize> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&state.db_path)?;
        let now = scan::now();
        cv::remember_choices(&conn, &pairs, now)?;
        cv::apply(&state.comicvine, &conn, series_id, &volume_id, &pairs, true, true, now)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Match one book to a Comic Vine issue (None: leave it unmatched), without changing what the
/// rest of its series is matched to.
#[tauri::command]
async fn cv_apply_book(state: State<'_, Shared>, comic_id: i64, volume_id: String, issue_id: Option<String>) -> CmdResult<usize> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&state.db_path)?;
        let now = scan::now();
        let series_id: i64 = conn.query_row("SELECT series_id FROM comics WHERE id=?1", [comic_id], |r| r.get(0))?;
        let pairs = [cv::Pair { comic_id, issue_id }];
        cv::remember_choices(&conn, &pairs, now)?;
        cv::apply(&state.comicvine, &conn, series_id, &volume_id, &pairs, false, true, now)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
fn toggle_bookmark(state: State<Shared>, id: i64, page: i64) -> CmdResult<bool> {
    if id < 0 {
        return Err("Bookmarks are kept for books in your library.".into());
    }
    db::toggle_bookmark(&state.conn.lock().unwrap(), id, page, scan::now()).map_err(err)
}

/// Replace a page's panels with hand-edited ones; `None` goes back to automatic detection.
#[tauri::command]
fn set_user_panels(state: State<Shared>, id: i64, page: i64, panels: Option<Vec<panels::Panel>>) -> CmdResult<()> {
    if id < 0 {
        return Err("Panel edits are kept for books in your library.".into());
    }
    let json = panels.map(|p| serde_json::to_string(&p)).transpose().map_err(err)?;
    db::set_user_panels(&state.conn.lock().unwrap(), id, page, json.as_deref()).map_err(err)
}

/// A book opened from disk: named after its file, no series, no progress.
fn external_card(path: &Path, id: i64, pages: usize) -> db::ComicCard {
    db::ComicCard {
        id,
        label: String::new(),
        title: None,
        year: None,
        series_id: 0,
        series_name: path
            .file_stem()
            .or(path.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        current_page: 0,
        num_pages: Some(pages as i64),
        read: false,
        is_manga: false,
        cover_ratio: None,
        last_read_at: None,
        cover_color: None,
        edition: None,
        collects: None,
    }
}

#[tauri::command]
async fn open_comic(state: State<'_, Shared>, id: i64, screen: Option<u32>) -> CmdResult<ReaderInfo> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<ReaderInfo> {
        let (path, _) = state.comic_path(id)?;
        let listing = state.archives.listing(id, &path)?;
        let n = listing.pages.len();
        if id < 0 {
            let comic = external_card(&path, id, n);
            return Ok(ReaderInfo {
                comic,
                page_count: n,
                start_page: 0,
                prev_id: None,
                next: None,
                bookmarks: vec![],
                external: true,
            });
        }
        let conn = state.conn.lock().unwrap();
        let comic = db::card_by_id(&conn, id)?;
        let (prev_id, next_id) = db::neighbors(&conn, id)?;
        let next = next_id.map(|n| db::card_by_id(&conn, n)).transpose()?;
        let start = if comic.read { 0 } else { (comic.current_page.max(0) as usize).min(n - 1) };
        let bookmarks = db::bookmarks(&conn, id)?;
        drop(conn);
        warm::start(&state, id, n, start, screen);
        let s = state.clone();
        std::thread::spawn(move || tidy_cache(&s, Some(id)));
        Ok(ReaderInfo { comic, page_count: n, start_page: start, prev_id, next, bookmarks, external: false })
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// In Guided View: work out the book's panels in the background, from page `from` on.
#[tauri::command]
fn panels_ahead(state: State<'_, Shared>, id: i64, from: usize, rtl: bool) {
    warm::panels_ahead(state.inner(), id, rtl, from);
}

#[tauri::command]
async fn page_panels(state: State<'_, Shared>, id: i64, page: i64, rtl: bool) -> CmdResult<Vec<panels::Panel>> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Vec<panels::Panel>> {
        // Covers are splash pages; guided view just shows them whole.
        if id > 0 {
            if let Some(json) = db::user_panels(&state.conn.lock().unwrap(), id, page)? {
                return Ok(serde_json::from_str(&json)?);
            }
        }
        if page == 0 {
            return Ok(vec![]);
        }
        if id < 0 {
            let (path, _) = state.comic_path(id)?;
            let (bytes, _) = state.archives.page(id, &path, page as usize)?;
            return panels::detect(&bytes, rtl);
        }
        let cached = db::cached_panels(&state.conn.lock().unwrap(), id, page, rtl, PANELS_VERSION)?;
        if let Some(json) = cached {
            return Ok(serde_json::from_str(&json)?);
        }
        let (path, _) = state.comic_path(id)?;
        let (bytes, _) = state.archives.page(id, &path, page as usize)?;
        let found = panels::detect(&bytes, rtl)?;
        let json = serde_json::to_string(&found)?;
        db::store_panels(&state.conn.lock().unwrap(), id, page, rtl, PANELS_VERSION, &json)?;
        Ok(found)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

#[tauri::command]
fn save_progress(state: State<Shared>, id: i64, page: i64, page_count: i64) -> CmdResult<()> {
    if id < 0 {
        return Ok(());
    }
    let conn = state.conn.lock().unwrap();
    let now = scan::now();
    db::set_progress(&conn, id, page, page_count, now).map_err(err)?;
    stats::record(&conn, id, page, page_count, now).map_err(err)
}

/// One book's reading so far, for the end card.
#[tauri::command(async)]
fn book_stats(state: State<Shared>, id: i64) -> CmdResult<stats::BookStats> {
    stats::book(&state.conn.lock().unwrap(), id).map_err(err)
}

/// Everything for the stats page. `tz_minutes` is the reader's offset from UTC.
#[tauri::command(async)]
fn reading_stats(state: State<Shared>, tz_minutes: i64) -> CmdResult<stats::Stats> {
    stats::stats(&state.conn.lock().unwrap(), scan::now(), tz_minutes * 60).map_err(err)
}

#[tauri::command]
fn set_read(state: State<Shared>, id: i64, read: bool) -> CmdResult<()> {
    db::set_read(&state.conn.lock().unwrap(), id, read, scan::now()).map_err(err)
}

/// Give a book a format by hand, or (None) let it be worked out from its name again.
#[tauri::command]
fn set_edition(state: State<Shared>, id: i64, kind: Option<String>) -> CmdResult<()> {
    db::set_edition(&state.conn.lock().unwrap(), id, kind.as_deref()).map_err(err)
}

// ---------- image protocol: aion://localhost/cover/{id}, /print/{id}, /page/{id}/{n} ----------

fn respond(status: StatusCode, mime: &str, body: Vec<u8>, cache: bool) -> Response<Vec<u8>> {
    let mut b = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*");
    if cache {
        b = b.header(header::CACHE_CONTROL, "max-age=31536000, immutable");
    }
    b.body(body).unwrap()
}

fn cover(state: &AppState, id: i64) -> anyhow::Result<Vec<u8>> {
    if id < 0 {
        // Outside books get fresh ids each session, so their covers aren't cached.
        let (path, _) = state.comic_path(id)?;
        return Ok(archive::make_thumbnail(&state.archives.first_page(id, &path)?)?.0);
    }
    let cached = state.thumbs_dir.join(format!("{id}.jpg"));
    if let Ok(b) = std::fs::read(&cached) {
        return Ok(b);
    }
    let (path, file) = state.comic_path(id)?;
    // Reuse YACReader's cover cache when it has one.
    if let (Some(hash), Some(root)) = (file.yac_hash.as_ref(), state.root()) {
        let yac = root.join(".yacreaderlibrary").join("covers").join(format!("{hash}.jpg"));
        if let Ok(b) = std::fs::read(yac) {
            return Ok(b);
        }
    }
    let (thumb, ratio) = archive::make_thumbnail(&state.archives.first_page(id, &path)?)?;
    let _ = std::fs::write(&cached, &thumb);
    let color = archive::cover_color(&thumb).ok();
    let conn = state.conn.lock().unwrap();
    let _ = db::set_cover_ratio(&conn, id, ratio);
    if let Some(c) = color {
        let _ = db::set_cover_color(&conn, id, &c);
    }
    Ok(thumb)
}

/// Where a cover's print is kept (see `archive::make_print`).
fn print_path(state: &AppState, id: i64) -> PathBuf {
    state.thumbs_dir.join("prints").join(format!("{id}.jpg"))
}

/// A cover's print for behind a collection's name, made once and kept. Made from the first page
/// alone, so a CBR isn't unpacked for it.
fn print(state: &AppState, id: i64) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(id > 0, "prints are kept for library books only");
    let cached = print_path(state, id);
    if let Ok(b) = std::fs::read(&cached) {
        return Ok(b);
    }
    let (path, _) = state.comic_path(id)?;
    let print = archive::make_print(&state.archives.first_page(id, &path)?)?;
    let _ = std::fs::create_dir_all(cached.parent().unwrap());
    let _ = std::fs::write(&cached, &print);
    Ok(print)
}

fn handle_image(state: &AppState, req: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let parts: Vec<&str> = req.uri().path().trim_matches('/').split('/').collect();
    let result = match parts.as_slice() {
        ["cover", id] => id
            .parse()
            .map_err(anyhow::Error::from)
            .and_then(|id| cover(state, id))
            .map(|b| (b, "image/jpeg")),
        ["print", id] => id
            .parse()
            .map_err(anyhow::Error::from)
            .and_then(|id| print(state, id))
            .map(|b| (b, "image/jpeg")),
        ["thumb", id, n] => (|| {
            let id: i64 = id.parse()?;
            let n: usize = n.parse()?;
            let (path, _) = state.comic_path(id)?;
            let cached = state.page_thumb_path(id, &path, n).ok();
            if let Some(b) = cached.as_ref().and_then(|c| std::fs::read(c).ok()) {
                return Ok((b, "image/jpeg"));
            }
            let thumb = archive::make_page_thumb(&state.archives.page(id, &path, n)?.0)?;
            if let Some(c) = cached {
                let _ = std::fs::write(c, &thumb);
            }
            Ok((thumb, "image/jpeg"))
        })(),
        // A preview only if it's been made already: it stands in while the page loads, so it
        // mustn't cost a page read itself.
        ["preview", id, n] => (|| {
            let (id, n): (i64, usize) = (id.parse()?, n.parse()?);
            let (path, _) = state.comic_path(id)?;
            Ok((std::fs::read(state.page_thumb_path(id, &path, n)?)?, "image/jpeg"))
        })(),
        // A page shown whole on a screen `h` pixels tall: its copy at that height if one's been
        // made (an empty one means the page itself will do), else the page.
        ["screen", id, n, h] => (|| {
            state.last_page.store(now_ms(), Ordering::SeqCst);
            let (id, n, h): (i64, usize, u32) = (id.parse()?, n.parse()?, h.parse()?);
            *state.reading.lock().unwrap() = Some((id, n));
            let (path, _) = state.comic_path(id)?;
            if id > 0 {
                if let Ok(copy) = std::fs::read(state.archives.screen_copy_path(id, &path, n, h)?) {
                    if !copy.is_empty() {
                        return Ok((copy, "image/jpeg"));
                    }
                }
            }
            state.archives.page_first(id, &path, n)
        })(),
        ["page", id, n] => (|| {
            state.last_page.store(now_ms(), Ordering::SeqCst);
            let id: i64 = id.parse()?;
            let n: usize = n.parse()?;
            *state.reading.lock().unwrap() = Some((id, n));
            let (path, _) = state.comic_path(id)?;
            state.archives.page_first(id, &path, n)
        })(),
        _ => Err(anyhow::anyhow!("not found")),
    };
    match result {
        Ok((body, mime)) => respond(StatusCode::OK, mime, body, true),
        Err(e) => respond(StatusCode::NOT_FOUND, "text/plain", e.to_string().into_bytes(), false),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();
    // One AionComics at a time: opening a comic from Explorer while it runs opens it in that window.
    // (AION_MULTI_INSTANCE lets a test copy run beside it.)
    if std::env::var_os("AION_MULTI_INSTANCE").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            if let Some(path) = comic_arg(argv) {
                let _ = app.emit("open-file", path);
            }
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("aion", |ctx, request, responder| {
            let state = ctx.app_handle().state::<Shared>().inner().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(handle_image(&state, &request));
            });
        })
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            let thumbs_dir = cache_dir.join("thumbs");
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(&thumbs_dir)?;
            // Page previews used to be kept here with no limit; they're in the page cache now.
            let _ = std::fs::remove_dir_all(thumbs_dir.join("pages"));
            let db_path = data_dir.join("library.db");
            // A backup you chose to restore goes in before anything opens the database.
            match backup::apply_pending(&data_dir, &db_path) {
                Ok(Some(name)) => eprintln!("restored backup {name}"),
                Ok(None) => {}
                Err(e) => eprintln!("couldn't restore a backup: {e}"),
            }
            let conn = db::open(&db_path)?;
            let cv_key = db::get_setting(&conn, "cv_api_key")?;
            let state: Shared = Arc::new(AppState {
                data_dir: data_dir.clone(),
                db_path,
                conn: Mutex::new(conn),
                thumbs_dir,
                archives: archive::Archives::new(&cache_dir),
                scanning: AtomicBool::new(false),
                external: Mutex::new(Vec::new()),
                comicvine: cv::Client::new(&cache_dir, cv_key),
                changes: intake::Changes::new(),
                watcher: Mutex::new(None),
                reading_info: AtomicBool::new(false),
                last_page: std::sync::atomic::AtomicI64::new(0),
                open_request: Mutex::new(comic_arg(std::env::args())),
                warming: Mutex::new(None),
                reading: Mutex::new(None),
            });
            app.manage(state.clone());
            // Rescan a few seconds after the comics folder stops changing (a copy in progress keeps
            // changing it). Changes during a scan wait for the next one.
            watch_library(&state);
            {
                let (state, app) = (state.clone(), app.handle().clone());
                std::thread::spawn(move || loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if state.changes.settled(4000) {
                        if state.scanning.load(Ordering::SeqCst) {
                            state.changes.touch();
                        } else {
                            start_scan(&app, &state);
                        }
                    }
                });
            }
            // Comic Vine details (credits, characters, story arcs) trickle in on their own thread.
            {
                let (state, app) = (state.clone(), app.handle().clone());
                std::thread::spawn(move || {
                    cv::run_queue(&state.comicvine, &state.db_path, |comic_id, left| {
                        let _ = app.emit("metadata-changed", serde_json::json!({ "comicId": comic_id, "left": left }));
                    });
                });
            }
            // A copy of the database a day, on a connection of its own.
            {
                let state = state.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(STARTUP_QUIET);
                    let dir = backup::dir(&state.data_dir);
                    if backup::due(&dir, scan::now()) {
                        match db::open(&state.db_path).and_then(|c| backup::make(&c, &dir, scan::now())) {
                            Ok(b) => eprintln!("backed up to {}", b.name),
                            Err(e) => eprintln!("backup failed: {e}"),
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(3600) - STARTUP_QUIET);
                });
            }
            // Pick up anything added or moved since last launch, and tidy the cache, once the first
            // screen is up: both work the database and the drive, and would hold it up.
            {
                let (app, state) = (app.handle().clone(), state.clone());
                std::thread::spawn(move || {
                    std::thread::sleep(STARTUP_QUIET);
                    start_scan(&app, &state);
                    tidy_cache(&state, None);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_library_root,
            rescan,
            backups,
            backup_now,
            restore_backup,
            home,
            nav,
            search,
            mark_read_before,
            dismiss_backfill,
            set_series_read,
            reveal,
            collection,
            series,
            people,
            duplicates,
            ignore_duplicate,
            book_stats,
            reading_stats,
            open_comic,
            page_panels,
            panels_ahead,
            save_progress,
            set_read,
            open_path,
            save_page,
            extract_pages,
            toggle_bookmark,
            set_user_panels,
            comic_meta,
            casts,
            set_options,
            storage,
            clear_pages,
            open_data_folder,
            edit_comic_meta,
            cv_status,
            cv_open_site,
            set_edition,
            cv_set_key,
            cv_search,
            cv_plan,
            cv_apply,
            cv_apply_book,
            plan_add,
            add_comics,
            take_open_request
        ])
        .run(tauri::generate_context!())
        .expect("error while running AionComics");
}

#[cfg(test)]
mod real_library {
    //! Run against a real library: `AION_TEST_ROOT=H:\Comics cargo test --lib real -- --ignored --nocapture`
    use super::*;

    #[test]
    #[ignore]
    fn real_scan_and_read() {
        let root = PathBuf::from(std::env::var("AION_TEST_ROOT").expect("AION_TEST_ROOT"));
        let tmp = std::env::temp_dir().join("aion-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut conn = db::open(&tmp.join("t.db")).unwrap();
        let stats = scan::scan(&mut conn, &root).unwrap();
        println!("{}", serde_json::to_string_pretty(&stats).unwrap());
        let again = scan::scan(&mut conn, &root).unwrap();
        println!("rescan: {:.1}s, added {}", again.seconds, again.added);

        let cont = db::continue_reading(&conn, scan::now(), 5).unwrap();
        for c in &cont {
            println!("continue: {} {} p{}/{:?}", c.series_name, c.label, c.current_page, c.num_pages);
        }
        for col in db::collections(&conn).unwrap() {
            println!("collection {}: {} series, {} comics", col.name, col.series_count, col.comic_count);
        }
        let mut st = conn
            .prepare("SELECT s.name, c.label, c.section FROM comics c JOIN series s ON s.id=c.series_id ORDER BY random() LIMIT 25")
            .unwrap();
        for r in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).unwrap() {
            let (s, l, sec) = r.unwrap();
            println!("  {s} | {l} | {sec}");
        }

        let archives = archive::Archives::new(&tmp);
        for ext in ["cbr", "cbz"] {
            let (id, rel): (i64, String) = conn
                .query_row(&format!("SELECT id, rel_path FROM comics WHERE rel_path LIKE '%.{ext}' ORDER BY random() LIMIT 1"), [], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap();
            let path = root.join(&rel);
            let t = std::time::Instant::now();
            let listing = archives.listing(id, &path).unwrap();
            let (p0, mime) = archives.page(id, &path, 0).unwrap();
            let t0 = t.elapsed();
            let last = listing.pages.len() - 1;
            let (pl, _) = archives.page(id, &path, last).unwrap();
            println!("{ext} {rel}: {:?} {} pages, first {} bytes {mime} in {:?}, last {} bytes in {:?}",
                listing.kind, listing.pages.len(), p0.len(), t0, pl.len(), t.elapsed());
            let (thumb, ratio) = archive::make_thumbnail(&p0).unwrap();
            println!("  thumbnail {} bytes, ratio {ratio:.2}", thumb.len());
        }
    }
}

#[cfg(test)]
mod real_queries {
    //! `AION_TEST_DB=<copy of library.db> AION_TEST_ROOT=H:\Comics cargo test --lib real_queries -- --ignored --nocapture`
    use super::*;

    #[test]
    #[ignore]
    fn home_rows() {
        let mut conn = db::open(Path::new(&std::env::var("AION_TEST_DB").unwrap())).unwrap();
        let now = scan::now();
        let show = |name: &str, v: &[db::ComicCard]| {
            println!("{name} ({}): {}", v.len(), v.iter().take(6).map(|c| format!("{} {}", c.series_name, c.label)).collect::<Vec<_>>().join(" | "));
        };
        show("continue", &db::continue_reading(&conn, now, 50).unwrap());
        show("up next", &db::up_next(&conn, 50).unwrap());
        show("pick up", &db::pick_up_again(&conn, now, 50).unwrap());
        let vin: i64 = conn.query_row("SELECT id FROM series WHERE name='Vinland Saga'", [], |r| r.get(0)).unwrap();
        let v = db::series_view(&conn, vin).unwrap();
        println!("vinland backfill: {:?}", v.backfill.map(|b| (b.before_label, b.count)));
        println!("search yukimura: {:?}", db::search(&conn, "yukimura").unwrap().iter().map(|s| s.name.clone()).collect::<Vec<_>>());
        let t = std::time::Instant::now();
        let n = scan::cover_ratios(&mut conn, Path::new(&std::env::var("AION_TEST_ROOT").unwrap())).unwrap();
        println!("cover ratios: {n} in {:?}", t.elapsed());
    }
}
