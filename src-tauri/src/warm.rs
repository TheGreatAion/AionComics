//! Small previews of every page of the book being read, made in the background: a jump shows one
//! at once while the page itself loads, and the page ticks and page strip show them instantly.
//! Pages nearest to where you are go first, and the pass pauses whenever a page is being turned to.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::{archive, now_ms, Shared};

/// The pass waits this long after the reader last asked for a page.
const YIELD_MS: i64 = 300;

/// Start previewing book `id` from page `from`, stopping the pass for any other book.
pub fn start(state: &Shared, id: i64, count: usize, from: usize) {
    // Outside books get new ids each session, so their previews aren't kept.
    if id < 0 {
        return;
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(old) = state.warming.lock().unwrap().replace((id, stop.clone())) {
        old.1.store(true, Ordering::Relaxed);
    }
    let state = state.clone();
    std::thread::spawn(move || {
        if let Err(e) = run(&state, id, count, from, &stop) {
            eprintln!("page previews for {id}: {e}");
        }
    });
}

/// Pages in order of distance from `from`, ahead before behind.
fn order(count: usize, from: usize) -> impl Iterator<Item = usize> {
    (0..count).flat_map(move |d| [from.checked_add(d), from.checked_sub(d + 1)]).flatten().filter(move |&n| n < count)
}

fn run(state: &Shared, id: i64, count: usize, from: usize, stop: &AtomicBool) -> anyhow::Result<()> {
    let (path, _) = state.comic_path(id)?;
    // A CBR's pages turn up as it's unpacked: go round again for the ones that weren't there yet.
    for _ in 0..60 {
        let mut missing = 0;
        for n in order(count, from) {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let file = state.page_thumb_path(id, n);
            if file.exists() {
                continue;
            }
            while now_ms() - state.last_page.load(Ordering::SeqCst) < YIELD_MS {
                std::thread::sleep(Duration::from_millis(50));
                if stop.load(Ordering::Relaxed) {
                    return Ok(());
                }
            }
            let Some(bytes) = state.archives.page_if_ready(id, &path, n)? else {
                missing += 1;
                continue;
            };
            let thumb = archive::make_page_thumb(&bytes)?;
            let _ = std::fs::create_dir_all(file.parent().unwrap());
            std::fs::write(&file, thumb)?;
        }
        if missing == 0 {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
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
