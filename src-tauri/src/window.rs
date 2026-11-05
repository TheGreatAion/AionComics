//! The window comes back the size and place you left it, or maximized if it was. Kept in a small
//! file in the app's data folder.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow, Window};

/// In screen pixels: the size and place as an ordinary window, even while maximized.
#[derive(Serialize, Deserialize, Clone, Copy, Default)]
struct Place {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
}

/// How much of the title strip must be on a screen for a saved place to be any use.
const STILL_SEEN: i32 = 80;
/// Maximizing moves the window a moment before it says it's maximized. A new size or place counts
/// once this long has gone by without that happening.
const SETTLE: Duration = Duration::from_millis(300);

#[derive(Default)]
struct Tracker {
    place: Place,
    known: bool,
    /// The latest size and place, until it's sure it isn't just the start of maximizing.
    fresh: Option<(Instant, i32, i32, u32, u32)>,
}

impl Tracker {
    fn settle(&mut self) {
        if let Some((_, x, y, width, height)) = self.fresh.take() {
            (self.place.x, self.place.y, self.place.width, self.place.height) = (x, y, width, height);
            self.known = true;
        }
    }
}

static NOW: Mutex<Option<Tracker>> = Mutex::new(None);

fn file(data_dir: &Path) -> PathBuf {
    data_dir.join("window.json")
}

/// Put the window back where it was, unless that screen has gone.
pub fn restore(window: &WebviewWindow, data_dir: &Path) {
    let saved = std::fs::read(file(data_dir)).ok().and_then(|b| serde_json::from_slice::<Place>(&b).ok());
    if let Some(p) = saved.filter(|p| p.width > 0 && p.height > 0) {
        let seen = window.available_monitors().unwrap_or_default().iter().any(|m| {
            let (at, size) = (m.position(), m.size());
            p.x + p.width as i32 - STILL_SEEN > at.x
                && p.x + STILL_SEEN < at.x + size.width as i32
                && p.y >= at.y - STILL_SEEN
                && p.y + STILL_SEEN < at.y + size.height as i32
        });
        if seen {
            // A window with no title bar of its own still has a hidden frame, which setting a size
            // adds on, and not by the same amount each time. Ask, look, and ask again.
            let (mut w, mut h) = (p.width as i64, p.height as i64);
            for _ in 0..4 {
                let _ = window.set_size(PhysicalSize::new(w.max(1) as u32, h.max(1) as u32));
                let Ok(got) = window.outer_size() else { break };
                let (off_w, off_h) = (got.width as i64 - p.width as i64, got.height as i64 - p.height as i64);
                if off_w == 0 && off_h == 0 {
                    break;
                }
                (w, h) = (w - off_w, h - off_h);
            }
            let _ = window.set_position(PhysicalPosition::new(p.x, p.y));
            *NOW.lock().unwrap() = Some(Tracker { place: p, known: true, fresh: None });
            if p.maximized {
                let _ = window.maximize();
            }
        }
    }
    let _ = window.show();
}

/// The window moved or changed size. Full screen and minimized aren't places to come back to.
pub fn note(window: &Window) {
    if window.is_fullscreen().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        return;
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let mut now = NOW.lock().unwrap();
    let t = now.get_or_insert_with(Tracker::default);
    t.place.maximized = maximized;
    if maximized {
        t.fresh = None;
        t.known = true;
        return;
    }
    if t.fresh.is_some_and(|f| f.0.elapsed() >= SETTLE) {
        t.settle();
    }
    if let (Ok(at), Ok(size)) = (window.outer_position(), window.outer_size()) {
        // Windows parks a minimized window far off the screen.
        if at.x > -30000 && size.width > 0 && size.height > 0 {
            t.fresh = Some((Instant::now(), at.x, at.y, size.width, size.height));
        }
    }
}

/// Keep the place for next time.
pub fn save(data_dir: &Path) {
    let mut now = NOW.lock().unwrap();
    let Some(t) = now.as_mut() else { return };
    t.settle();
    if !t.known {
        return;
    }
    if let Ok(json) = serde_json::to_vec(&t.place) {
        let _ = std::fs::write(file(data_dir), json);
    }
}
