//! Background work for the book being read, nearest pages to where you are first, pausing whenever
//! a page is being turned to:
//! - small previews of every page: a jump shows one at once while the page itself loads, and the
//!   page ticks and page strip show them instantly;
//! - copies of big pages at screen height, which decode several times faster than the page;
//! - in Guided View, every page's panels, so stepping on never waits for them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use crate::{archive, db, now_ms, panels, Shared, PANELS_VERSION};

/// The pass waits this long after the reader last asked for a page.
const YIELD_MS: i64 = 300;

/// Start previewing book `id` from page `from` (and making copies for a screen `screen` pixels
/// tall), stopping the pass for any other book.
pub fn start(state: &Shared, id: i64, count: usize, from: usize, screen: Option<u32>) {
    // Outside books get new ids each session, so their previews aren't kept.
    if id < 0 {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(old) = state.warming.lock().unwrap().replace((id, stop.clone())) {
        old.1.store(true, Ordering::Relaxed);
    }
    // Previews and screen copies side by side: copies cost more, and a big or progressive scan
    // needs one as soon as previews do.
    let (s, halt) = (state.clone(), stop.clone());
    std::thread::spawn(move || {
        if let Err(e) = run(&s, id, count, from, &halt) {
            eprintln!("page previews for {id}: {e}");
        }
    });
    if let Some(h) = screen {
        let state = state.clone();
        std::thread::spawn(move || {
            if let Err(e) = copies(&state, id, count, from, h, &stop) {
                eprintln!("screen copies for {id}: {e}");
            }
            state.archives.prune_now();
        });
    }
}

/// Pages in order of distance from `from`, ahead before behind.
fn order(count: usize, from: usize) -> impl Iterator<Item = usize> {
    (0..count).flat_map(move |d| [from.checked_add(d), from.checked_sub(d + 1)]).flatten().filter(move |&n| n < count)
}

fn run(state: &Shared, id: i64, count: usize, from: usize, stop: &AtomicBool) -> anyhow::Result<()> {
    let (path, _) = state.comic_path(id)?;
    follow(state, id, count, from, stop, |n| state.page_thumb_path(id, n).exists(), |n| {
        // A CBR's page may not be there yet (still being unpacked): tried again later.
        let Some(bytes) = state.archives.page_if_ready(id, &path, n)? else { return Ok(false) };
        let file = state.page_thumb_path(id, n);
        let _ = std::fs::create_dir_all(file.parent().unwrap());
        std::fs::write(&file, archive::make_page_thumb(&bytes)?)?;
        Ok(true)
    })
}

/// Work through a book's pages, always next the one nearest to where the reader is now (the page
/// asked for last, or `from`), pausing whenever a page is being turned to. `have(n)`: page n is
/// done already; `make(n)`: do it, false if it can't be yet (tried again after a while).
fn follow(
    state: &Shared,
    id: i64,
    count: usize,
    from: usize,
    stop: &AtomicBool,
    have: impl Fn(usize) -> bool,
    mut make: impl FnMut(usize) -> anyhow::Result<bool>,
) -> anyhow::Result<()> {
    let mut done = vec![false; count];
    for _ in 0..60 {
        let mut later = vec![false; count];
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let at = match *state.reading.lock().unwrap() {
                Some((book, page)) if book == id => page.min(count.saturating_sub(1)),
                _ => from,
            };
            let Some(n) = order(count, at).find(|&n| !done[n] && !later[n]) else { break };
            if have(n) {
                done[n] = true;
                continue;
            }
            while now_ms() - state.last_page.load(Ordering::SeqCst) < YIELD_MS {
                std::thread::sleep(Duration::from_millis(50));
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
            }
            if make(n)? {
                done[n] = true;
            } else {
                later[n] = true;
            }
        }
        if !later.contains(&true) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Ok(())
}

/// Screen copies of the book's big pages (see `archive::make_screen_copy`). A page that needs none
/// gets an empty file, so it isn't looked at again.
fn copies(state: &Shared, id: i64, count: usize, from: usize, h: u32, stop: &AtomicBool) -> anyhow::Result<()> {
    let (path, _) = state.comic_path(id)?;
    let file = |n: usize| state.archives.screen_copy_path(id, &path, n, h);
    follow(state, id, count, from, stop, |n| file(n).is_ok_and(|f| f.exists()), |n| {
        let Some(bytes) = state.archives.page_if_ready(id, &path, n)? else { return Ok(false) };
        let copy = archive::make_screen_copy(&bytes, h)?.unwrap_or_default();
        let (file, tmp) = (file(n)?, file(n)?.with_extension("part"));
        std::fs::write(&tmp, copy)?;
        std::fs::rename(&tmp, &file)?;
        Ok(true)
    })
}

/// What the panels pass should work on: book, reading direction, and the page to start from.
static AHEAD: OnceLock<(Mutex<Option<(i64, bool, usize)>>, Condvar)> = OnceLock::new();

/// Work out the panels of book `id` (as read in direction `rtl`) from page `from` on, in place of
/// whatever the pass was doing. One pass for the whole app, started the first time.
pub fn panels_ahead(state: &Shared, id: i64, rtl: bool, from: usize) {
    let mut first = false;
    let (want, cv) = AHEAD.get_or_init(|| {
        first = true;
        (Mutex::new(None), Condvar::new())
    });
    *want.lock().unwrap() = Some((id, rtl, from));
    cv.notify_all();
    if first {
        let state = state.clone();
        std::thread::spawn(move || panels_pass(&state));
    }
}

fn panels_pass(state: &Shared) {
    let (want, cv) = AHEAD.get().unwrap();
    loop {
        let job = {
            let mut w = want.lock().unwrap();
            while w.is_none() {
                w = cv.wait(w).unwrap();
            }
            w.unwrap()
        };
        let changed = || *want.lock().unwrap() != Some(job);
        if let Err(e) = panels_book(state, job, &changed) {
            eprintln!("panels for {}: {e}", job.0);
        }
        // Done with this book (or it was replaced): wait for the next request.
        let mut w = want.lock().unwrap();
        if *w == Some(job) {
            *w = None;
        }
    }
}

fn panels_book(state: &Shared, (id, rtl, from): (i64, bool, usize), changed: &dyn Fn() -> bool) -> anyhow::Result<()> {
    // Outside books get new ids each session, so there's no keeping their panels.
    if id < 0 {
        return Ok(());
    }
    let (path, _) = state.comic_path(id)?;
    let count = state.archives.listing(id, &path)?.pages.len();
    // Covers are shown whole: no panels to find.
    for n in order(count, from).filter(|&n| n > 0) {
        if changed() {
            return Ok(());
        }
        let page = n as i64;
        let known = {
            let conn = state.conn.lock().unwrap();
            db::user_panels(&conn, id, page)?.is_some() || db::cached_panels(&conn, id, page, rtl, PANELS_VERSION)?.is_some()
        };
        if known {
            continue;
        }
        while now_ms() - state.last_page.load(Ordering::SeqCst) < YIELD_MS {
            std::thread::sleep(Duration::from_millis(50));
            if changed() {
                return Ok(());
            }
        }
        let Some(bytes) = state.archives.page_if_ready(id, &path, n)? else { continue };
        let found = panels::detect(&bytes, rtl)?;
        db::store_panels(&state.conn.lock().unwrap(), id, page, rtl, PANELS_VERSION, &serde_json::to_string(&found)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn nearest_first() {
        assert_eq!(super::order(6, 2).collect::<Vec<_>>(), [2, 1, 3, 0, 4, 5]);
        assert_eq!(super::order(3, 0).collect::<Vec<_>>(), [0, 1, 2]);
    }
}
