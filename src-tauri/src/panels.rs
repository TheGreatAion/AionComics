//! Panel detection for Guided View.
//!
//! 1. Estimate the gutter color from the page border (white or black,
//!    whichever dominates; bleeding art is ignored).
//! 2. Flood-fill gutter-colored pixels inward from the page edge: that's
//!    the "outside". Everything enclosed is panel material — this copes with
//!    slanted gutters, which straight cuts can't.
//! 3. Each connected blob of non-outside pixels is a candidate panel. A blob
//!    can really be several panels bridged by a balloon or by art crossing a
//!    gutter, so it's split recursively along straight (possibly slanted)
//!    lines that mostly run over paper and are flanked by panel borders.
//! 4. Reading order comes from an XY-cut over the boxes: bands top to bottom,
//!    within a band left to right (right to left for manga).
//!
//! Borderless art can still fragment or merge. Both degrade to "show a
//! different area", which is fine for reading.

use std::collections::VecDeque;

use anyhow::Result;
use image::{imageops::FilterType, GrayImage};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Panel {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// The panel's real outline when it isn't its box (slanted layouts):
    /// normalized page coordinates, clockwise on screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poly: Option<Vec<[f32; 2]>>,
}

impl Panel {
    fn area(&self) -> f32 {
        self.w * self.h
    }
    fn contains(&self, o: &Panel, slack: f32) -> bool {
        o.x >= self.x - slack
            && o.y >= self.y - slack
            && o.x + o.w <= self.x + self.w + slack
            && o.y + o.h <= self.y + self.h + slack
    }
    fn overlap(&self, o: &Panel) -> f32 {
        let w = (self.x + self.w).min(o.x + o.w) - self.x.max(o.x);
        let h = (self.y + self.h).min(o.y + o.h) - self.y.max(o.y);
        w.max(0.0) * h.max(0.0)
    }
    fn big_enough(&self) -> bool {
        self.area() >= MIN_PANEL_AREA && self.w > 0.05 && self.h > 0.04
    }
}

const WORK_SIZE: u32 = 900; // long side of the analysis image
const GUTTER_DELTA: i16 = 40; // how far from the gutter color still counts as gutter
const MIN_PANEL_AREA: f32 = 0.012; // drop specks: page numbers, stray captions

// Splitting blobs that are really several panels.
const SPLIT_DEPTH: u32 = 4; // at most 2^4 panels out of one blob
const MAX_LEAN: f32 = 0.14; // tan 8°: how far a cut may slant
const CUT_PAPER: f32 = 0.75; // share of a cut that must run over paper (balloons interrupt the rest)
const DARK_MARGIN: i16 = 60; // gutter level at or below which the page's margins are dark
const CLEAR_DELTA: i16 = 20; // "paper" for a cut: stricter than GUTTER_DELTA, so screentone isn't
const CUT_FLANK: f32 = 0.5; // share of a cut with straight borders close by on both sides
const FLANK_DIST: u8 = 18; // "close by", in work pixels (gutters up to ~36px)
const END_SLACK: usize = 3; // samples at each end of a cut where it may graze a panel corner
const FLANK_WOBBLE: u8 = 3; // how much the distance to a border may vary along a run
const FLANK_RUN: usize = 12; // samples a border must keep running alongside to count
const CUT_MARGIN: f32 = 0.04; // a cut keeps at least this much of the page on each side
const MIN_PIECE_AREA: f32 = 0.02; // pieces cut off a blob must look like real panels
const MIN_PIECE_SIDE: f32 = 0.1; // not a sliver between parallel strokes
const DARK_INK: u8 = 150; // borders around light gutters
const LIGHT_INK: u8 = 110; // borders around dark gutters
const STEP: usize = 2; // sampling stride of the cut search
const SIDE_LEAN: f32 = 0.25; // steepest panel side a polygon follows (~14°)
const SIDE_TOLERANCE: f32 = 3.0; // work pixels off its line a border point may be
const RULED: f32 = 0.75; // share of a side's outline on one straight line for a ruled border
const MIN_RULED_AREA: f32 = 0.3; // page share in ruled panels below which a page is art, not panels
const BAND_SLACK: f32 = 0.02; // reading order: overlap (page fraction) that still separates rows
const BLEED_MARGIN: f32 = 0.15; // bleed layouts: most of a cut's length that may be page margin at one end

const MARGIN_FLAT: i16 = 14; // bleed layouts: how much a page margin's level may vary
const BLEED_END_RUN: f32 = 0.12; // bleed layouts: most of a cut's length that may be art at one end
const BLEED_ROW_FIRST: f32 = 0.8; // bleed layouts: a cut across wins unless one down scores this much better
const MAX_SHARED: f32 = 0.5; // a box half inside a bigger one is a fragment of it (neighbors share slivers)
const BLEED_CUT_PAPER: f32 = 0.6; // bleed layouts: share of a cut over paper
const BLEED_CLEAN_FLANK: f32 = 0.25; // bleed layouts: flanking needed by a cut that's ≥95% paper
const BLEED_LIGHT_LINE: u8 = 235; // bleed layouts: a light cut's median at least this
const BLEED_DARK_LINE: u8 = 75; // bleed layouts: a dark cut's median at most this
const BLEED_LINE_DELTA: i16 = 28; // bleed layouts: how far from the cut's median still counts as on it
const BLEED_EDGE: i16 = 50; // bleed layouts: art beside a line is at least this far off its level
/// Line levels the bleed edge maps are made for: (light line, the band's lowest light / highest dark level).
const EDGE_BANDS: [(bool, u8); 6] = [(true, 240), (true, 220), (true, 200), (false, 25), (false, 50), (false, 75)];

/// Which of `EDGE_BANDS` a line at level `m` uses: the one that's strictest for it.
fn edge_band(m: u8) -> usize {
    if m >= BLEED_LIGHT_LINE {
        EDGE_BANDS.iter().position(|&(light, l)| light && m >= l).unwrap_or(2)
    } else {
        3 + EDGE_BANDS[3..].iter().position(|&(_, l)| m <= l).unwrap_or(2)
    }
}
const BLEED_GUTTER_MAX: i32 = 45; // bleed layouts: work pixels past which a gutter has ended on both sides

/// Gutter luminance: the dominant extreme (light or dark) around the border.
fn gutter_color(img: &GrayImage) -> i16 {
    let (w, h) = (img.width(), img.height());
    let band = (w.min(h) / 60).max(2);
    let (mut light, mut dark, mut all) = (Vec::new(), Vec::new(), Vec::new());
    for y in 0..h {
        for x in 0..w {
            if x < band || y < band || x >= w - band || y >= h - band {
                let v = img.get_pixel(x, y)[0];
                all.push(v);
                if v >= 200 {
                    light.push(v);
                } else if v <= 60 {
                    dark.push(v);
                }
            }
        }
    }
    let pick = if light.len() >= dark.len() { &mut light } else { &mut dark };
    let pool = if pick.len() * 4 >= all.len() { pick } else { &mut all };
    pool.sort_unstable();
    pool.get(pool.len() / 2).copied().unwrap_or(255) as i16
}

/// Pixel lists of the connected blobs left after flooding the outside.
fn blobs(img: &GrayImage, bg: i16) -> Vec<Vec<u32>> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let px = img.as_raw();
    let gutter = |i: usize| (px[i] as i16 - bg).abs() <= GUTTER_DELTA;

    // Flood the outside from every gutter pixel on the border.
    let mut outside = vec![false; w * h];
    let mut q = VecDeque::new();
    let seed = |i: usize, outside: &mut Vec<bool>, q: &mut VecDeque<usize>| {
        if !outside[i] && gutter(i) {
            outside[i] = true;
            q.push_back(i);
        }
    };
    for x in 0..w {
        seed(x, &mut outside, &mut q);
        seed((h - 1) * w + x, &mut outside, &mut q);
    }
    for y in 0..h {
        seed(y * w, &mut outside, &mut q);
        seed(y * w + w - 1, &mut outside, &mut q);
    }
    while let Some(i) = q.pop_front() {
        let (x, y) = (i % w, i / w);
        if x > 0 {
            seed(i - 1, &mut outside, &mut q);
        }
        if x + 1 < w {
            seed(i + 1, &mut outside, &mut q);
        }
        if y > 0 {
            seed(i - w, &mut outside, &mut q);
        }
        if y + 1 < h {
            seed(i + w, &mut outside, &mut q);
        }
    }

    // Connected components of everything else.
    let mut seen = outside;
    let mut out = Vec::new();
    for start in 0..w * h {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        q.push_back(start);
        let mut blob = Vec::new();
        while let Some(i) = q.pop_front() {
            blob.push(i as u32);
            let (x, y) = (i % w, i / w);
            for j in [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ]
            .into_iter()
            .flatten()
            {
                if !seen[j] {
                    seen[j] = true;
                    q.push_back(j);
                }
            }
        }
        out.push(blob);
    }
    out
}

/// A detected panel plus how many of its sides are ruled borders: panels
/// have them, the ragged edges of free-standing art don't.
struct Piece {
    panel: Panel,
    ruled_sides: usize,
    /// Sides not on the page edge.
    inner_sides: usize,
}

impl Piece {
    /// Looks like a panel: two ruled sides, or (in bleed layouts, where a panel runs off the page)
    /// every side that isn't the page edge ruled.
    fn ruled(&self, bleed: bool) -> bool {
        self.ruled_sides >= 2 || bleed && self.inner_sides >= 1 && self.ruled_sides == self.inner_sides
    }
}

/// One side of a piece in box coordinates: `s = a + b t`, where t runs along
/// the side (x for top/bottom, y for left/right).
struct Side {
    a: f32,
    b: f32,
    /// Share of the side's extremes lying on the line.
    straight: f32,
}

impl Side {
    /// Median-of-slopes fit of the outermost ink along one side. Balloons and
    /// art poking out past a border are outliers. A side that's barely
    /// slanted, or not straight at all, falls back to the box edge `far`.
    fn fit(pts: &[(f32, f32)], len: f32, far: f32, outward: f32) -> Side {
        let flat = Side { a: far, b: 0.0, straight: 0.0 };
        if pts.len() < 8 {
            return flat;
        }
        let stride = (pts.len() / 40).max(1);
        let sample: Vec<_> = pts.iter().step_by(stride).collect();
        let mut slopes = Vec::new();
        for (i, p) in sample.iter().enumerate() {
            for q in &sample[i + 1..] {
                if q.0 - p.0 >= len * 0.1 {
                    slopes.push((q.1 - p.1) / (q.0 - p.0));
                }
            }
        }
        if slopes.is_empty() {
            return flat;
        }
        let median = |v: &mut Vec<f32>| {
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        let b = median(&mut slopes).clamp(-SIDE_LEAN, SIDE_LEAN);
        let a = median(&mut pts.iter().map(|p| p.1 - b * p.0).collect());
        let on = pts.iter().filter(|p| (p.1 - a - b * p.0).abs() <= SIDE_TOLERANCE).count();
        let straight = on as f32 / len;
        if straight < 0.5 || (b * len).abs() < SIDE_TOLERANCE * 2.0 {
            return Side { a: far, b: 0.0, straight };
        }
        // Sit on the outermost point the line passes through, so the polygon
        // never clips the panel's own border.
        let shift = pts
            .iter()
            .map(|p| (p.1 - a - b * p.0) * outward)
            .filter(|d| *d <= SIDE_TOLERANCE)
            .fold(0.0f32, f32::max);
        Side { a: a + shift * outward, b, straight }
    }
}

/// Clip a polygon (clockwise, box coordinates) to the box `0..w, 0..h`.
fn clip_to_box(poly: &[(f32, f32)], w: f32, h: f32) -> Vec<(f32, f32)> {
    // Signed distance inside each box edge.
    let edges: [fn(&(f32, f32), f32, f32) -> f32; 4] =
        [|p, _, _| p.0, |p, w, _| w - p.0, |p, _, _| p.1, |p, _, h| h - p.1];
    let mut pts = poly.to_vec();
    for inside in edges {
        let src = std::mem::take(&mut pts);
        for k in 0..src.len() {
            let (p, q) = (src[k], src[(k + 1) % src.len()]);
            let (dp, dq) = (inside(&p, w, h), inside(&q, w, h));
            if dp >= 0.0 {
                pts.push(p);
            }
            if (dp >= 0.0) != (dq >= 0.0) {
                let f = dp / (dp - dq);
                pts.push((p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f));
            }
        }
    }
    pts
}

/// Inclusive pixel rectangle.
#[derive(Clone, Copy)]
struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

/// A straight line across a blob. It runs along `t` (x for a horizontal cut,
/// y for a vertical one) and sits at `s = a + lean * (t - t0) / len`.
#[derive(Clone, Copy)]
struct Cut {
    vertical: bool,
    t0: f32,
    len: f32,
    a: f32,
    lean: f32,
}

impl Cut {
    fn beyond(&self, i: u32, w: usize) -> bool {
        let (x, y) = ((i as usize % w) as f32, (i as usize / w) as f32);
        let (t, s) = if self.vertical { (y, x) } else { (x, y) };
        s > self.a + self.lean * (t - self.t0) / self.len
    }
}

const UP: usize = 0;
const DOWN: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;

/// Distance (capped at 255) from each pixel to the nearest ink pixel looking
/// up, down, left and right.
fn ink_distances(px: &[u8], w: usize, h: usize, ink: impl Fn(u8) -> bool) -> [Vec<u8>; 4] {
    let mut d: [Vec<u8>; 4] = std::array::from_fn(|_| vec![255; w * h]);
    let mut walk = |dir: usize, idx: &mut dyn Iterator<Item = usize>| {
        let mut run = 255u8;
        for i in idx {
            run = if ink(px[i]) { 0 } else { run.saturating_add(1) };
            d[dir][i] = run;
        }
    };
    for x in 0..w {
        walk(UP, &mut (0..h).map(|y| y * w + x));
        walk(DOWN, &mut (0..h).rev().map(|y| y * w + x));
    }
    for y in 0..h {
        walk(LEFT, &mut (0..w).map(|x| y * w + x));
        walk(RIGHT, &mut (0..w).rev().map(|x| y * w + x));
    }
    d
}

/// Analysis state for splitting blobs.
struct Splitter {
    w: usize,
    h: usize,
    px: Vec<u8>,
    /// Gutter-colored, or plain white paper (balloons, light gutters on a dark page).
    paper: Vec<bool>,
    /// Stricter `paper` for cut lines, so light screentone doesn't pass for gutter.
    clear: Vec<bool>,
    /// Ink distances around light paper (dark ink) and dark paper (light ink).
    near: [[Vec<u8>; 4]; 2],
    /// Bleed mode: distances to art beside a line, per band of line level (see `edge_band`).
    edges: Vec<[Vec<u8>; 4]>,
    /// `mark[i] == stamp` while `i` belongs to the blob being split.
    mark: Vec<u32>,
    stamp: u32,
    /// Bleed mode: the whole page is one blob (panels run off the edges, so there is no outside to
    /// flood). Any edge of art flanks a gutter, not just border ink, and a cut may open onto
    /// paper anywhere, since gutters inside the blob aren't flooded.
    bleed: bool,
    /// Gutter level (see `gutter_color`).
    bg: i16,
}

impl Splitter {
    fn new(img: &GrayImage, bg: i16, bleed: bool) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.as_raw().clone();
        let is_paper = |v: u8| (v as i16 - bg).abs() <= GUTTER_DELTA || (!bleed || bg > 128) && v >= 215;
        let paper: Vec<bool> = px.iter().map(|&v| is_paper(v)).collect();
        let clear = px
            .iter()
            .map(|&v| (v as i16 - bg).abs() <= CLEAR_DELTA || (!bleed || bg > 128) && v >= 255 - CLEAR_DELTA as u8)
            .collect();
        let near = [ink_distances(&px, w, h, |v| v <= DARK_INK), ink_distances(&px, w, h, |v| v >= LIGHT_INK)];
        // Art beside a line is whatever is clearly off its level: darker than a light line by
        // BLEED_EDGE, or lighter than a dark one. One map per band of line levels.
        let edges = if bleed {
            EDGE_BANDS
                .iter()
                .map(|&(light, level)| {
                    if light {
                        ink_distances(&px, w, h, |v| (v as i16) < level as i16 - BLEED_EDGE)
                    } else {
                        ink_distances(&px, w, h, |v| v as i16 > level as i16 + BLEED_EDGE)
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        Splitter { w, h, px, paper, clear, near, edges, mark: vec![0; w * h], stamp: 0, bleed, bg }
    }

    /// Bounding box of a blob's non-paper pixels.
    fn tight(&self, pixels: &[u32]) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        for &i in pixels {
            let i = i as usize;
            if self.paper[i] {
                continue;
            }
            let (x, y) = (i % self.w, i / self.w);
            r = Some(match r {
                None => Rect { x0: x, y0: y, x1: x, y1: y },
                Some(r) => Rect { x0: r.x0.min(x), y0: r.y0.min(y), x1: r.x1.max(x), y1: r.y1.max(y) },
            });
        }
        r
    }

    fn panel(&self, r: Rect) -> Panel {
        let (w, h) = (self.w as f32, self.h as f32);
        Panel {
            x: r.x0 as f32 / w,
            y: r.y0 as f32 / h,
            w: (r.x1 + 1 - r.x0) as f32 / w,
            h: (r.y1 + 1 - r.y0) as f32 / h,
            poly: None,
        }
    }

    /// Emit one panel per piece of the blob, cutting it along gutters.
    fn split(&mut self, pixels: Vec<u32>, depth: u32, out: &mut Vec<Piece>) {
        let Some(r) = self.tight(&pixels) else { return };
        let panel = self.panel(r);
        if depth < SPLIT_DEPTH && panel.area() >= 2.0 * MIN_PANEL_AREA {
            self.stamp += 1;
            for &i in &pixels {
                self.mark[i as usize] = self.stamp;
            }
            let (across, down) = (self.best_cut(r, false), self.best_cut(r, true));
            let best = match (across, down) {
                // Pages are built in rows. Where art may cross a gutter (bleed layouts), a cut down
                // the whole page can slice through a row; a decent cut across goes first.
                (Some(h), Some(v)) if self.bleed && h.0 >= v.0 * BLEED_ROW_FIRST => Some(h),
                (h, v) => [h, v].into_iter().flatten().max_by(|a, b| a.0.total_cmp(&b.0)),
            };
            if let Some((_, cut)) = best {
                let w = self.w;
                let (a, b): (Vec<u32>, Vec<u32>) = pixels.iter().partition(|&&i| !cut.beyond(i, w));
                let (a, b) = (self.main_parts(a), self.main_parts(b));
                let fits = |s: &[u32]| {
                    self.tight(s).map(|r| self.panel(r)).is_some_and(|p| {
                        p.big_enough() && p.area() >= MIN_PIECE_AREA && p.w.min(p.h) >= MIN_PIECE_SIDE
                    })
                };
                if fits(&a) && fits(&b) {
                    self.split(a, depth + 1, out);
                    self.split(b, depth + 1, out);
                    return;
                }
            }
        }
        out.push(self.shape(&pixels, r));
    }

    /// Fit the piece's outline: a robust straight line along each side,
    /// ignoring balloons and art that stick out past the border.
    fn shape(&self, pixels: &[u32], r: Rect) -> Piece {
        let (bw, bh) = (r.x1 + 1 - r.x0, r.y1 + 1 - r.y0);
        // Outermost ink per row (left, right) and per column (top, bottom).
        let (mut left, mut right) = (vec![usize::MAX; bh], vec![0; bh]);
        let (mut top, mut bottom) = (vec![usize::MAX; bw], vec![0; bw]);
        for &i in pixels {
            let i = i as usize;
            if self.paper[i] {
                continue;
            }
            let (x, y) = (i % self.w - r.x0, i / self.w - r.y0);
            left[y] = left[y].min(x);
            right[y] = right[y].max(x + 1);
            top[x] = top[x].min(y);
            bottom[x] = bottom[x].max(y + 1);
        }
        let side = |ext: &[usize], far: usize, outward: f32| -> Side {
            let pts: Vec<(f32, f32)> = ext
                .iter()
                .enumerate()
                .filter(|(_, e)| **e != usize::MAX)
                .map(|(t, &e)| (t as f32 + 0.5, e as f32))
                .collect();
            Side::fit(&pts, ext.len() as f32, far as f32, outward)
        };
        let sides = [
            side(&top, 0, -1.0),
            side(&right, bw, 1.0),
            side(&bottom, bh, 1.0),
            side(&left, 0, -1.0),
        ];
        // Sides along the page edge are straight for free (bleeding art), so
        // only inner sides count as ruled.
        let edge = [r.y0 <= 1, r.x1 + 2 >= self.w, r.y1 + 2 >= self.h, r.x0 <= 1];
        let ruled_sides = sides.iter().zip(edge).filter(|(s, e)| !e && s.straight >= RULED).count();
        let inner_sides = edge.iter().filter(|e| !**e).count();
        // Corners where neighboring sides meet (clockwise from top-left),
        // clipped to the box.
        let corner = |h: &Side, v: &Side| {
            // h: y = a + b x, v: x = a + b y.
            let y = (h.a + h.b * v.a) / (1.0 - h.b * v.b);
            (v.a + v.b * y, y)
        };
        let quad = [
            corner(&sides[0], &sides[3]),
            corner(&sides[0], &sides[1]),
            corner(&sides[2], &sides[1]),
            corner(&sides[2], &sides[3]),
        ];
        let mut panel = self.panel(r);
        if sides.iter().any(|s| s.b != 0.0) {
            let poly = clip_to_box(&quad, bw as f32, bh as f32);
            let (w, h) = (self.w as f32, self.h as f32);
            if poly.len() >= 3 {
                panel.poly = Some(poly.iter().map(|&(x, y)| [(x + r.x0 as f32) / w, (y + r.y0 as f32) / h]).collect());
            }
        }
        Piece { panel, ruled_sides, inner_sides }
    }

    /// Drop small fragments a straight cut sheared off a neighboring panel
    /// (a slanted corner, half a balloon), so they don't inflate the box.
    fn main_parts(&mut self, pixels: Vec<u32>) -> Vec<u32> {
        self.stamp += 1;
        let stamp = self.stamp;
        for &i in &pixels {
            self.mark[i as usize] = stamp;
        }
        let w = self.w;
        let mut parts: Vec<Vec<u32>> = Vec::new();
        let mut q = VecDeque::new();
        for &start in &pixels {
            if self.mark[start as usize] != stamp {
                continue;
            }
            self.mark[start as usize] = 0;
            q.push_back(start as usize);
            let mut part = Vec::new();
            while let Some(i) = q.pop_front() {
                part.push(i as u32);
                let (x, y) = (i % w, i / w);
                for j in [
                    (x > 0).then(|| i - 1),
                    (x + 1 < w).then(|| i + 1),
                    (y > 0).then(|| i - w),
                    (y + 1 < self.h).then(|| i + w),
                ]
                .into_iter()
                .flatten()
                {
                    if self.mark[j] == stamp {
                        self.mark[j] = 0;
                        q.push_back(j);
                    }
                }
            }
            parts.push(part);
        }
        let largest = parts.iter().map(Vec::len).max().unwrap_or(0);
        parts.into_iter().filter(|p| p.len() * 6 >= largest).flatten().collect()
    }

    /// The best-scoring gutter line across `r` (horizontal, or vertical when
    /// `vertical`), if any qualifies. Pixels outside the current blob count as
    /// paper, so slanted gutters already flooded as "outside" still line up.
    fn best_cut(&self, r: Rect, vertical: bool) -> Option<(f32, Cut)> {
        let (t0, t1, s0, s1) = if vertical { (r.y0, r.y1, r.x0, r.x1) } else { (r.x0, r.x1, r.y0, r.y1) };
        let margin = (CUT_MARGIN * if vertical { self.w } else { self.h } as f32) as usize;
        if s1 < s0 + 2 * margin || t1 <= t0 {
            return None;
        }
        let (lo, hi) = ((s0 + margin) as i32, (s1 - margin) as i32);
        let len = (t1 - t0) as f32;
        let max_lean = (len * MAX_LEAN) as i32;
        let (na, nb) = if vertical { (LEFT, RIGHT) } else { (UP, DOWN) };
        let t_dim = if vertical { self.h } else { self.w };
        let samples = (t1 - t0) / STEP + 1;
        // Modern full-bleed art spills over gutters more (hair, limbs, effects), so allow more of it.
        let paper_share = if self.bleed { BLEED_CUT_PAPER } else { CUT_PAPER };
        let max_bad = ((1.0 - paper_share) * samples as f32) as usize;
        let mut best: Option<(f32, Cut)> = None;
        let at = |a: i32, lean: i32, t: usize| {
            let s = (a as f32 + lean as f32 * (t - t0) as f32 / len).round() as usize;
            if vertical { t * self.w + s } else { s * self.w + t }
        };
        let mut levels = Vec::with_capacity(samples);
        for lean in (-max_lean..=max_lean).step_by(STEP) {
            for a in (lo..=hi).step_by(STEP) {
                if !(lo..=hi).contains(&(a + lean)) {
                    continue;
                }
                // Bleed layouts: the line is judged by its own level, not a page-wide gutter color.
                // Most of it must sit near its median, and that must be gutter-like: near white, or
                // near black (a dark gutter, or the border rule panels butt against).
                let line = if self.bleed {
                    // Most lines cross mid-tone art: a quick look at every fourth sample rules them
                    // out before the median.
                    let (mut light, mut dark, mut n) = (0usize, 0usize, 0usize);
                    for t in (t0..=t1).step_by(STEP * 4) {
                        let v = self.px[at(a, lean, t)] as i16;
                        light += (v >= BLEED_LIGHT_LINE as i16 - BLEED_LINE_DELTA) as usize;
                        dark += (v <= BLEED_DARK_LINE as i16 + BLEED_LINE_DELTA) as usize;
                        n += 1;
                    }
                    if (light.max(dark) as f32) < n as f32 * (paper_share - 0.1) {
                        continue;
                    }
                    levels.clear();
                    levels.extend((t0..=t1).step_by(STEP).map(|t| self.px[at(a, lean, t)]));
                    let mid = levels.len() / 2;
                    let m = *levels.select_nth_unstable(mid).1;
                    // Most lines cross art: count how much of the line is off its level before walking it.
                    let off = levels.iter().filter(|&&v| (v as i16 - m as i16).abs() > BLEED_LINE_DELTA).count();
                    if m > BLEED_DARK_LINE && m < BLEED_LIGHT_LINE || off > max_bad {
                        continue;
                    }
                    Some(m)
                } else {
                    None
                };
                // A border runs alongside a gutter as a straight line, so the
                // distance to ink on each side stays nearly constant along the
                // cut. White gaps between strokes, or beside a balloon's curve,
                // drift instead, so only long steady runs count as flanked.
                let (mut bad, mut run, mut flanked) = (0, 0, 0);
                // First and last samples on the line's level (bleed layouts; see below).
                let (mut first_on, mut last_on) = (None, 0);
                // Distance range [lo, hi] to ink on each side over the current run.
                let mut band: Option<[(u8, u8); 2]> = None;
                let mut blocks: Option<(usize, usize)> = None;
                let mut open = [false; 2];
                let mut span: Option<(usize, usize)> = None;
                let mut count = |run: usize, end: usize, flanked: &mut usize| {
                    if run >= FLANK_RUN {
                        *flanked += run;
                        span = Some((span.map_or(end - run, |s| s.0), end));
                    }
                };
                // Bleed layouts: a dark rule ending at the page margin (plain paper, of whatever
                // color) runs out of the art there, so leave the margin off the line and count that
                // end as open.
                let (lead, trail) = match line {
                    Some(m) if m <= BLEED_DARK_LINE => {
                        let cap = (samples as f32 * BLEED_MARGIN) as usize;
                        let v = |k: usize| self.px[at(a, lean, t0 + k * STEP)] as i16;
                        let margin = |from: usize, k: usize| (v(k) - v(from)).abs() <= MARGIN_FLAT && v(k) > m as i16 + 2 * BLEED_LINE_DELTA;
                        let lead = (0..samples).take(cap).take_while(|&k| margin(0, k)).count();
                        let trail = (0..samples).rev().take(cap).take_while(|&k| margin(samples - 1, k)).count();
                        (lead, trail)
                    }
                    _ => (0, 0),
                };
                open = [lead > 0, trail > 0];
                for (k, t) in (t0..=t1).step_by(STEP).enumerate() {
                    if k < lead || k + trail >= samples {
                        continue;
                    }
                    let i = at(a, lean, t);
                    let near = match line {
                        Some(m) => &self.edges[edge_band(m)],
                        None => &self.near[(self.px[i] < 128) as usize],
                    };
                    let (da, db) = (near[na][i], near[nb][i]);
                    let member = self.mark[i] == self.stamp;
                    let blocked = member
                        && match line {
                            Some(m) => (self.px[i] as i16 - m as i16).abs() > BLEED_LINE_DELTA,
                            None => !self.clear[i],
                        };
                    // An end is open when the line leaves the blob there (into
                    // the flooded gutter) or runs off the page on clean paper.
                    // Near the ends it may graze a panel corner.
                    let end = (k < END_SLACK, k + END_SLACK >= samples);
                    if end.0 || end.1 {
                        let edge = if end.0 { t0 <= 1 } else { t1 + 2 >= t_dim };
                        open[end.1 as usize] |= !blocked && (!member || edge || self.bleed);
                    }
                    // Borders flank a gutter: flooded, or the gutter's own color where the flood
                    // didn't reach. A light sky inside a panel passes for paper too, and figures
                    // standing in it can look like borders, so it isn't flanked. (Pages with dark
                    // margins often have white gutters between the panels, though.)
                    let sky = member && !self.bleed && self.bg > DARK_MARGIN && (self.px[i] as i16 - self.bg).abs() > CLEAR_DELTA;
                    let widen = |(lo, hi): (u8, u8), d: u8| (lo.min(d), hi.max(d));
                    let next = band.map(|[ra, rb]| [widen(ra, da), widen(rb, db)]);
                    let steady = next.is_none_or(|b| b.iter().all(|(lo, hi)| hi - lo <= FLANK_WOBBLE));
                    if blocked || sky || da > FLANK_DIST || db > FLANK_DIST || !steady {
                        count(run, k, &mut flanked);
                        run = 0;
                        band = None;
                    }
                    if !blocked {
                        first_on.get_or_insert(k);
                        last_on = k;
                    }
                    if blocked {
                        bad += 1;
                        if k >= END_SLACK && k + END_SLACK < samples {
                            blocks = Some((blocks.map_or(k, |b| b.0), k));
                        }
                        if bad > max_bad {
                            break;
                        }
                    } else if !sky && da <= FLANK_DIST && db <= FLANK_DIST {
                        band = Some(band.map_or([(da, da), (db, db)], |[ra, rb]| [widen(ra, da), widen(rb, db)]));
                        run += 1;
                    }
                }
                count(run, samples, &mut flanked);
                // A gutter runs edge to edge, interrupted at most in the middle (hair, a balloon). A
                // line that ends in a long stretch of art is a gutter that stopped (where a row
                // ends), carried on through the panels below it.
                if self.bleed {
                    let tail = (samples - 1 - trail).saturating_sub(last_on);
                    if first_on.is_none_or(|f| (f - lead).max(tail) as f32 > samples as f32 * BLEED_END_RUN) {
                        continue;
                    }
                }
                if bad > max_bad {
                    continue;
                }
                // A real gutter connects to the page's gutters: from an open
                // end the line reaches the flanked stretch without crossing
                // ink. Gaps between strokes inside a panel are walled off by
                // the panel's own border.
                let Some((first, last)) = span else {
                    continue;
                };
                let connected = (open[0] && blocks.is_none_or(|b| b.0 >= first))
                    || (open[1] && blocks.is_none_or(|b| b.1 < last));
                if !connected {
                    continue;
                }
                // Measured over the stretch where borders run (slanted rows
                // make the box taller than the gutter), but at least half the cut.
                let span = (last - first).max(samples / 2);
                let flank = flanked as f32 / span as f32;
                // In bleed layouts art beside a gutter can be as dark (or light) as the gutter, hiding
                // the edge; a line that's almost all gutter all the way across needs less of it.
                let clean = self.bleed && bad * 20 <= samples;
                if flank < if clean { BLEED_CLEAN_FLANK } else { CUT_FLANK } {
                    continue;
                }
                // A gutter is narrow. Open paper or a plain sky goes on past it on both sides.
                if let Some(m) = line {
                    let blank = |off: i32| {
                        let b = a + off;
                        let room = if vertical { self.w } else { self.h } as i32;
                        if b < 0 || b + lean < 0 || b >= room || b + lean >= room {
                            return false;
                        }
                        let on = (t0..=t1)
                            .step_by(STEP * 2)
                            .filter(|&t| (self.px[at(b, lean, t)] as i16 - m as i16).abs() <= BLEED_LINE_DELTA)
                            .count();
                        on * 2 * STEP > (t1 - t0) * 3 / 4
                    };
                    if blank(-BLEED_GUTTER_MAX) && blank(BLEED_GUTTER_MAX) {
                        continue;
                    }
                }
                // Rank by flanked length overall, so a full-length gutter beats
                // one that only runs for half the cut.
                let score = (samples - bad + flanked) as f32 / samples as f32;
                if best.is_none_or(|(b, _)| score > b) {
                    let cut = Cut { vertical, t0: t0 as f32, len, a: a as f32, lean: lean as f32 };
                    best = Some((score, cut));
                }
            }
        }
        best
    }
}

/// Order boxes by recursive XY-cut over their (slightly shrunk) extents, so
/// slanted panels whose boxes overlap a little still separate into bands. A
/// band only absorbs a box that overlaps it by more than `BAND_SLACK`, so
/// rows tilted by a slanted gutter don't chain into one.
fn order(mut boxes: Vec<Panel>, rtl: bool, horizontal: bool, tries: u32, out: &mut Vec<Panel>) {
    if boxes.len() <= 1 || tries >= 2 {
        // Neither axis separates these: top-to-bottom, then reading direction.
        boxes.sort_by(|a, b| {
            let ka = (a.y, if rtl { -a.x } else { a.x });
            let kb = (b.y, if rtl { -b.x } else { b.x });
            ka.partial_cmp(&kb).unwrap()
        });
        out.extend(boxes);
        return;
    }
    let span = |p: &Panel| -> (f32, f32) {
        let (a, len) = if horizontal { (p.y, p.h) } else { (p.x, p.w) };
        let shrink = len * 0.12;
        (a + shrink, a + len - shrink)
    };
    boxes.sort_by(|a, b| span(a).0.partial_cmp(&span(b).0).unwrap());
    let mut groups: Vec<Vec<Panel>> = Vec::new();
    let mut end = f32::MIN;
    for b in boxes {
        let (s, e) = span(&b);
        match groups.last_mut() {
            Some(g) if s < end - BAND_SLACK => {
                g.push(b);
                end = end.max(e);
            }
            _ => {
                groups.push(vec![b]);
                end = e;
            }
        }
    }
    if groups.len() == 1 {
        // This axis didn't split anything; try the other one.
        order(groups.pop().unwrap(), rtl, !horizontal, tries + 1, out);
        return;
    }
    if !horizontal && rtl {
        groups.reverse();
    }
    for g in groups {
        order(g, rtl, !horizontal, 0, out);
    }
}

pub fn detect(bytes: &[u8], rtl: bool) -> Result<Vec<Panel>> {
    detect_with(bytes, rtl, &mut |_| {})
}

/// `detect`, reporting each stage's findings to `log` (for tuning).
fn detect_with(bytes: &[u8], rtl: bool, log: &mut dyn FnMut(String)) -> Result<Vec<Panel>> {
    // Only a WORK_SIZE copy is analyzed, so decode no bigger than needed.
    let img = crate::archive::decode_at_least(bytes, WORK_SIZE)?;
    let scale = WORK_SIZE as f32 / img.width().max(img.height()) as f32;
    let small = if scale < 1.0 {
        img.resize(
            (img.width() as f32 * scale) as u32,
            (img.height() as f32 * scale) as u32,
            FilterType::Triangle,
        )
    } else {
        img
    }
    .to_luma8();
    // A scan's dark rim around the paper would pass for the gutter color and stop the flood from
    // reaching the real gutters: analyze the paper inside it, then put the panels back in place.
    // The page at twice the work size, for reading thin borders (see `frames::find`): only made
    // if the last pass gets that far, and only when the scan is that big.
    let fine = |crop: Option<(u32, u32, u32, u32)>| -> Option<GrayImage> {
        #[cfg(test)]
        if std::env::var("AION_NOFINE").is_ok() {
            return None;
        }
        let big = crate::archive::decode_at_least(bytes, WORK_SIZE * 2).ok()?;
        if big.width().max(big.height()) < WORK_SIZE * 3 / 2 {
            return None;
        }
        let big = big.resize_exact(small.width() * 2, small.height() * 2, FilterType::Triangle).to_luma8();
        Some(match crop {
            Some((x0, y0, x1, y1)) => image::imageops::crop_imm(&big, x0 * 2, y0 * 2, (x1 - x0) * 2, (y1 - y0) * 2).to_image(),
            None => big,
        })
    };
    let Some((x0, y0, x1, y1)) = rim(&small) else {
        return detect_in(small.clone(), &|| fine(None), rtl, log);
    };
    let (w, h) = (small.width() as f32, small.height() as f32);
    log(format!("rim cut: {x0},{y0} to {x1},{y1}"));
    let inner = image::imageops::crop_imm(&small, x0, y0, x1 - x0, y1 - y0).to_image();
    let (sx, sy, ox, oy) = ((x1 - x0) as f32 / w, (y1 - y0) as f32 / h, x0 as f32 / w, y0 as f32 / h);
    let mut panels = detect_in(inner, &|| fine(Some((x0, y0, x1, y1))), rtl, log)?;
    for p in &mut panels {
        (p.x, p.y, p.w, p.h) = (ox + p.x * sx, oy + p.y * sy, p.w * sx, p.h * sy);
        for v in p.poly.iter_mut().flatten() {
            *v = [ox + v[0] * sx, oy + v[1] * sy];
        }
    }
    Ok(panels)
}

/// The paper inside a scan's dark rim, as a crop box (x0, y0, x1, y1), when there is a rim: a few
/// rows or columns of near black along at least three edges, with light paper just inside along
/// at least one (art may run into the rim on the others, or lettering cross the margin).
fn rim(img: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (img.width(), img.height());
    let px = |x: u32, y: u32| img.get_pixel(x, y)[0];
    // Thickness of the rim along one edge (lines from it that are near all dark), and whether the
    // band just inside is mostly light.
    let side = |n: u32, len: u32, at: &dyn Fn(u32, u32) -> u8| -> (u32, bool) {
        let share = |k: u32, f: &dyn Fn(u8) -> bool| (0..len).filter(|&t| f(at(k, t))).count() as f32 / len as f32;
        let mut k = 0;
        while k < n / 12 && share(k, &|v| v <= 60) >= 0.9 {
            k += 1;
        }
        (k, (k + 2..k + 8).all(|j| share(j, &|v| v >= 150) >= 0.6))
    };
    let top = side(h, w, &|k, t| px(t, k));
    let bottom = side(h, w, &|k, t| px(t, h - 1 - k));
    let left = side(w, h, &|k, t| px(k, t));
    let right = side(w, h, &|k, t| px(w - 1 - k, t));
    let edges = [top, bottom, left, right];
    if edges.iter().filter(|e| e.0 >= 3).count() < 3 || !edges.iter().any(|e| e.0 >= 3 && e.1) {
        return None;
    }
    // Past the rim and the paper's own edge (a soft line where the scan meets it).
    let cut = |e: (u32, bool)| if e.0 > 0 { e.0 + 2 } else { 0 };
    Some((cut(left), cut(top), w - cut(right), h - cut(bottom)))
}

fn detect_in(small: GrayImage, fine: &dyn Fn() -> Option<GrayImage>, rtl: bool, log: &mut dyn FnMut(String)) -> Result<Vec<Panel>> {
    // Gutters around the panels: flood them from the page edge, then split what's enclosed.
    let bg = gutter_color(&small);
    let mut splitter = Splitter::new(&small, bg, false);
    let mut pieces = Vec::new();
    let found = blobs(&small, bg);
    let area = (small.width() * small.height()) as f32;
    let mut sizes: Vec<f32> = found.iter().map(|b| b.len() as f32 / area).filter(|a| *a > 0.005).collect();
    sizes.sort_by(|a, b| b.total_cmp(a));
    log(format!("bg {bg}, {} blobs, biggest {:.2?}", found.len(), &sizes[..sizes.len().min(6)]));
    for blob in found {
        splitter.split(blob, 0, &mut pieces);
    }
    if let Ok((kept, _)) = judge(pieces, false, log) {
        return Ok(ordered(kept, rtl));
    }
    #[cfg(test)]
    if std::env::var("AION_PANELS_NOBLEED").is_ok() {
        return Ok(vec![]);
    }

    // Art running off the page edges leaves no gutter around the panels to flood from, and dark
    // bleeding art passes for dark gutters. So cut the whole page along straight lines instead:
    // light or dark gutters, and the black rules panels butt against.
    let mut splitter = Splitter::new(&small, 255, true);
    let mut pieces = Vec::new();
    splitter.split((0..small.width() * small.height()).collect(), 0, &mut pieces);
    log("bleed:".into());
    if let Ok((kept, _)) = judge(pieces, true, log) {
        return Ok(ordered(kept, rtl));
    }

    // No gutter runs between panels that overlap, or between framed panels laid over art that
    // fills the page; but each of them has a border drawn round what shows of it.
    // AION_NOFRAMES=1 (tests): leave this pass out, to compare.
    let framed = if cfg!(test) && std::env::var("AION_NOFRAMES").is_ok() { Vec::new() } else { crate::frames::find(&small, fine, log) };
    let mut out = Vec::new();
    order(framed, rtl, true, 0, &mut out);
    Ok(out)
}

/// Keep the pieces that make a paneled page, with their ruled share of the page; Err when the page
/// is a splash or free-standing art and gets no guided steps.
fn judge(mut pieces: Vec<Piece>, bleed: bool, log: &mut dyn FnMut(String)) -> Result<(Vec<Piece>, f32), ()> {
    pieces.retain(|p| p.panel.big_enough());
    log(format!(
        "pieces {:?}",
        pieces
            .iter()
            .map(|p| format!("{:.2}x{:.2}@{:.2},{:.2} r{}/{}", p.panel.w, p.panel.h, p.panel.x, p.panel.y, p.ruled_sides, p.inner_sides))
            .collect::<Vec<_>>()
    ));
    // Drop boxes nested in (or mostly covered by) bigger ones: insets, art
    // fragments, a cover's masthead.
    pieces.sort_by(|a, b| b.panel.area().total_cmp(&a.panel.area()));
    let mut kept: Vec<Piece> = Vec::new();
    for p in pieces {
        let b = &p.panel;
        // Small and half inside a much bigger box: a fragment of it. (Two panels of similar size
        // can overlap that much when one box straddles the other.)
        let fragment = |k: &Piece| k.panel.overlap(b) > MAX_SHARED * b.area() && b.area() * 3.0 <= k.panel.area();
        if !kept.iter().any(|k| k.panel.contains(b, 0.01) || k.panel.overlap(b) > 0.7 * b.area() || fragment(k)) {
            kept.push(p);
        }
    }
    // One region covering most of the page is a splash: no guided steps.
    if kept.len() <= 1 && kept.first().is_none_or(|p| p.panel.area() > 0.6) {
        log(format!("splash: {} kept", kept.len()));
        return Err(());
    }
    // Covers, title pages and free-standing art break up into blobs with
    // ragged edges; a paneled page has a fair share of it in panels with
    // ruled sides.
    let ruled: f32 = kept.iter().filter(|p| p.ruled(bleed)).map(|p| p.panel.area()).sum();
    if ruled < MIN_RULED_AREA {
        log(format!("art: ruled area {ruled:.2} of {} kept", kept.len()));
        return Err(());
    }
    Ok((kept, ruled))
}

fn ordered(kept: Vec<Piece>, rtl: bool) -> Vec<Panel> {
    let mut out = Vec::new();
    order(kept.into_iter().map(|p| p.panel).collect(), rtl, true, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    /// White page, black-bordered panels: one wide strip, then two side by side.
    fn page() -> Vec<u8> {
        let mut img = GrayImage::from_pixel(600, 900, Luma([255]));
        let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32| {
            for y in y0..y1 {
                for x in x0..x1 {
                    img.put_pixel(x, y, Luma([40]));
                }
            }
        };
        fill(30, 30, 570, 300);
        fill(30, 330, 290, 870);
        fill(310, 330, 570, 870);
        png(img)
    }

    fn png(img: GrayImage) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    /// White page with panels given by `inside(x, y)`: a 3px black border
    /// around a gray interior.
    fn panels(inside: impl Fn(i32, i32) -> bool) -> GrayImage {
        GrayImage::from_fn(600, 900, |x, y| {
            let (x, y) = (x as i32, y as i32);
            if !inside(x, y) {
                Luma([255])
            } else if [(-3, 0), (3, 0), (0, -3), (0, 3)].iter().all(|(dx, dy)| inside(x + dx, y + dy)) {
                Luma([170])
            } else {
                Luma([20])
            }
        })
    }

    #[test]
    fn reading_order() {
        let ltr = detect(&page(), false).unwrap();
        assert_eq!(ltr.len(), 3);
        assert!(ltr[0].w > 0.85);
        assert!(ltr[1].x < ltr[2].x);
        let rtl = detect(&page(), true).unwrap();
        assert!(rtl[1].x > rtl[2].x);
        assert!(ltr.iter().all(|p| p.poly.is_none()), "rectangles need no polygon");
    }

    /// The same page scanned with a black rim round the paper: the rim is cut away, and the panels
    /// come back where they are on the whole page.
    #[test]
    fn scan_rim() {
        let inner = image::load_from_memory(&page()).unwrap().to_luma8();
        let mut img = GrayImage::from_pixel(640, 960, Luma([22]));
        image::imageops::overlay(&mut img, &inner, 20, 30);
        let plain = detect(&page(), false).unwrap();
        let rimmed = detect(&png(img), false).unwrap();
        assert_eq!(rimmed.len(), 3, "{rimmed:?}");
        let left = 20.0 / 640.0 + plain[1].x * 600.0 / 640.0;
        assert!((rimmed[1].x - left).abs() < 0.01, "{rimmed:?}");
        assert!(rim(&inner).is_none(), "no rim, no cut");
    }

    /// Two panels side by side, split by a gutter leaning ~6°, with art
    /// crossing the gutter so the flood fill alone sees one blob.
    #[test]
    fn slanted_gutter() {
        let gutter = |y: i32| 300 + (y - 450) / 10;
        let mut img = panels(|x, y| {
            (30..870).contains(&y) && (30..570).contains(&x) && (x - gutter(y)).abs() > 8
        });
        for y in 420..470 {
            for x in 250..350 {
                img.put_pixel(x, y, Luma([30]));
            }
        }
        let bytes = png(img);
        let ltr = detect(&bytes, false).unwrap();
        assert_eq!(ltr.len(), 2, "{ltr:?}");
        assert!(ltr[0].x < ltr[1].x);
        assert!(ltr.iter().all(|p| p.w < 0.55 && p.h > 0.9), "{ltr:?}");
        let rtl = detect(&bytes, true).unwrap();
        assert!(rtl[0].x > rtl[1].x);

        // Both panels get a clockwise polygon whose shared side leans with
        // the gutter: x = 300 ± 9 + (y - 450) / 10 in page pixels.
        let edge = |p: &Panel, near_top: bool, right: bool| -> f32 {
            let poly = p.poly.as_ref().expect("slanted panel has a polygon");
            let xs = poly.iter().filter(|v| if near_top { v[1] < 0.1 } else { v[1] > 0.9 }).map(|v| v[0]);
            if right { xs.fold(f32::MIN, f32::max) } else { xs.fold(f32::MAX, f32::min) }
        };
        for p in &ltr {
            let poly = p.poly.as_ref().unwrap();
            assert!((3..=8).contains(&poly.len()), "{poly:?}");
            let twice_area: f32 = (0..poly.len())
                .map(|k| {
                    let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
                    a[0] * b[1] - b[0] * a[1]
                })
                .sum();
            assert!(twice_area > 0.0, "clockwise on screen: {poly:?}");
        }
        let near = |got: f32, px: f32| (got - px / 600.0).abs() < 0.015;
        assert!(near(edge(&ltr[0], true, true), 249.0), "{:?}", ltr[0].poly);
        assert!(near(edge(&ltr[0], false, true), 333.0), "{:?}", ltr[0].poly);
        assert!(near(edge(&ltr[1], true, false), 267.0), "{:?}", ltr[1].poly);
        assert!(near(edge(&ltr[1], false, false), 351.0), "{:?}", ltr[1].poly);
    }

    /// Full-bleed page: art runs off every edge, so there's no gutter to flood. A black rule splits
    /// it into a top and bottom panel, and a white gutter with borders splits the bottom one. The
    /// same art with no lines at all is a splash.
    #[test]
    fn bleed_layout() {
        let art = |x: u32, y: u32| Luma([120 + ((x * 7 + y * 13) % 40) as u8]);
        let mut img = GrayImage::from_fn(600, 900, art);
        for y in 398..404 {
            for x in 0..600 {
                img.put_pixel(x, y, Luma([12]));
            }
        }
        for y in 404..900 {
            for x in 290..310 {
                let border = x < 293 || x >= 307;
                img.put_pixel(x, y, Luma([if border { 12 } else { 252 }]));
            }
        }
        let found = detect(&png(img), false).unwrap();
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(found[0].y < 0.02 && found[0].h > 0.4 && found[0].w > 0.95, "{found:?}");
        assert!(found[1].x < found[2].x && found[1].y > 0.43 && found[2].y > 0.43, "{found:?}");

        let splash = detect(&png(GrayImage::from_fn(600, 900, art)), false).unwrap();
        assert!(splash.is_empty(), "{splash:?}");
    }

    /// Two stacked panels with a lettered balloon bridging the gutter. A
    /// white band of sky inside the lower panel must not be cut.
    #[test]
    fn balloon_bridge() {
        let mut img = panels(|x, y| (30..570).contains(&x) && ((30..440).contains(&y) || (460..870).contains(&y)));
        for y in 600..640 {
            for x in 33..567 {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        for y in 400..500 {
            for x in 200..400 {
                let e = ((x as f32 - 300.0) / 90.0).powi(2) + ((y as f32 - 450.0) / 45.0).powi(2);
                if e <= 1.0 {
                    img.put_pixel(x, y, Luma([if e > 0.88 { 0 } else { 255 }]));
                }
            }
        }
        for y in (438..444).chain(456..462) {
            for x in (265..335).filter(|x| x % 9 < 6) {
                img.put_pixel(x, y, Luma([0]));
            }
        }
        let found = detect(&png(img), false).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].y < 0.05 && found[0].h < 0.5, "{found:?}");
        assert!(found[1].y > 0.48 && found[1].h > 0.4, "{found:?}");
    }
}

#[cfg(test)]
mod real_pages {
    //! `AION_TEST_ROOT=H:\Comics cargo test --lib real_panels -- --ignored --nocapture`
    //! Writes overlays to %TEMP%/aion-panels for eyeballing; `AION_PANELS_ONLY`
    //! limits the run to cases whose name contains it.
    use super::*;
    use crate::archive::Archives;
    use image::{Rgb, RgbImage};

    fn line(img: &mut RgbImage, (x0, y0): (f32, f32), (x1, y1): (f32, f32), width: i32, color: Rgb<u8>) {
        let steps = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as i32;
        for k in 0..=steps {
            let f = k as f32 / steps as f32;
            let (cx, cy) = ((x0 + (x1 - x0) * f) as i32, (y0 + (y1 - y0) * f) as i32);
            for dy in -width / 2..=width / 2 {
                for dx in -width / 2..=width / 2 {
                    let (x, y) = (cx + dx, cy + dy);
                    if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
                        img.put_pixel(x as u32, y as u32, color);
                    }
                }
            }
        }
    }

    /// The polygon when there is one (thick), else the box; boxes of
    /// polygon panels are drawn thin for reference.
    fn outline(img: &mut RgbImage, p: &Panel, color: Rgb<u8>) {
        let (w, h) = (img.width() as f32, img.height() as f32);
        let corners = [[p.x, p.y], [p.x + p.w, p.y], [p.x + p.w, p.y + p.h], [p.x, p.y + p.h]];
        let (shape, box_width) = match &p.poly {
            Some(poly) => (poly.clone(), 2),
            None => (corners.to_vec(), 0),
        };
        for (pts, width) in [(corners.to_vec(), box_width), (shape, 8)] {
            for k in 0..pts.len() {
                let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                if width > 0 {
                    line(img, (a[0] * w, a[1] * h), (b[0] * w, b[1] * h), width, color);
                }
            }
        }
    }

    /// Every page of one book: `AION_PANELS_FILE=<cbz/cbr> [AION_PANELS_RTL=1] cargo test --lib
    /// book_panels -- --ignored --nocapture`. Overlays go to %TEMP%/aion-panels-book.
    #[test]
    #[ignore]
    fn book_panels() {
        let file = std::path::PathBuf::from(std::env::var("AION_PANELS_FILE").unwrap());
        let rtl = std::env::var("AION_PANELS_RTL").is_ok_and(|v| v == "1");
        let out = std::env::temp_dir().join("aion-panels-book");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let archives = Archives::new(&out);
        let colors = [Rgb([230, 40, 40]), Rgb([40, 160, 240]), Rgb([40, 200, 90]), Rgb([240, 170, 30]), Rgb([200, 60, 220]), Rgb([30, 220, 220]), Rgb([255, 120, 0]), Rgb([120, 120, 255])];
        // Contact sheets of 12 pages each (6 across), numbered by their position.
        let (tw, th, across) = (300u32, 461u32, 6u32);
        let mut sheet = RgbImage::from_pixel(tw * across, th * 2, Rgb([128, 128, 128]));
        // AION_PANELS_PAGES=21-44 (1-based, inclusive) limits the run.
        let range: Option<(usize, usize)> = std::env::var("AION_PANELS_PAGES").ok().and_then(|r| {
            let (a, b) = r.split_once('-')?;
            Some((a.parse::<usize>().ok()? - 1, b.parse::<usize>().ok()? - 1))
        });
        let mut page = range.map_or(0, |r| r.0);
        let first_page = page;
        let flush = |sheet: &RgbImage, first: usize| sheet.save(out.join(format!("sheet-{:02}.png", first + 1))).unwrap();
        while let Ok((bytes, _)) = archives.page(1, &file, page) {
            if range.is_some_and(|r| page > r.1) {
                break;
            }
            let t = std::time::Instant::now();
            let mut notes = Vec::new();
            let panels = detect_with(&bytes, rtl, &mut |s| notes.push(s)).unwrap();
            println!("p{:02}: {} panels in {:?}", page + 1, panels.len(), t.elapsed());
            for n in notes {
                println!("     {n}");
            }
            // AION_PANELS_MASK=1: also save what the gutter flood leaves, biggest blob black,
            // the rest gray, the flooded gutters white.
            if std::env::var("AION_PANELS_MASK").is_ok() {
                let img = crate::archive::decode_at_least(&bytes, WORK_SIZE).unwrap();
                let s = WORK_SIZE as f32 / img.width().max(img.height()) as f32;
                let small = img.resize((img.width() as f32 * s) as u32, (img.height() as f32 * s) as u32, FilterType::Triangle).to_luma8();
                let found = blobs(&small, gutter_color(&small));
                let mut mask = GrayImage::from_pixel(small.width(), small.height(), image::Luma([255]));
                let biggest = found.iter().map(|b| b.len()).max().unwrap_or(0);
                for b in &found {
                    let v = if b.len() == biggest { 0 } else { 140 };
                    for &i in b {
                        mask.as_mut()[i as usize] = v;
                    }
                }
                mask.save(out.join(format!("mask{:02}.png", page + 1))).unwrap();
                let (lines, best) = crate::frames::debug(&small);
                lines.save(out.join(format!("lines{:02}.png", page + 1))).unwrap();
                for b in best {
                    println!("     cand {b}");
                }
            }
            let mut img = image::load_from_memory(&bytes).unwrap().to_rgb8();
            for (k, p) in panels.iter().enumerate() {
                outline(&mut img, p, colors[k % colors.len()]);
            }
            let small = image::imageops::resize(&img, 400, 400 * img.height() / img.width(), FilterType::Triangle);
            small.save(out.join(format!("p{:02}.png", page + 1))).unwrap();
            let slot = ((page - first_page) % 12) as u32;
            if slot == 0 && page > first_page {
                flush(&sheet, page - 12);
                sheet = RgbImage::from_pixel(tw * across, th * 2, Rgb([128, 128, 128]));
            }
            let thumb = image::imageops::resize(&img, tw - 6, th - 6, FilterType::Triangle);
            image::imageops::overlay(&mut sheet, &thumb, ((slot % across) * tw + 3) as i64, ((slot / across) * th + 3) as i64);
            page += 1;
        }
        flush(&sheet, first_page + (page - first_page).saturating_sub(1) / 12 * 12);
    }

    /// Pages listed in a file, one `path<TAB>rtl (0/1)<TAB>page (0-based)` per line:
    /// `AION_PANELS_LIST=<file> cargo test --lib sample_panels -- --ignored --nocapture`. Contact
    /// sheets of 8 go to %TEMP%/aion-panels-sample; each panel's place in the reading order is
    /// marked by that many squares in its corner.
    #[test]
    #[ignore]
    fn sample_panels() {
        let list = std::fs::read_to_string(std::env::var("AION_PANELS_LIST").unwrap()).unwrap();
        let out = std::env::temp_dir().join("aion-panels-sample");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let archives = Archives::new(&out);
        let colors = [Rgb([230, 40, 40]), Rgb([40, 160, 240]), Rgb([40, 200, 90]), Rgb([240, 170, 30]), Rgb([200, 60, 220]), Rgb([30, 220, 220]), Rgb([255, 120, 0]), Rgb([120, 120, 255])];
        let (tw, th, across) = (450u32, 692u32, 4u32);
        let mut sheet = RgbImage::from_pixel(tw * across, th * 2, Rgb([128, 128, 128]));
        let mut n = 0u32;
        for (k, row) in list.lines().filter(|l| !l.trim().is_empty()).enumerate() {
            let mut parts = row.split('\t');
            let (path, rtl, page) = (parts.next().unwrap(), parts.next().unwrap() == "1", parts.next().unwrap().trim().parse::<usize>().unwrap());
            // One id per book, so a RAR is unpacked once however many of its pages are listed.
            let id = list.lines().position(|l| l.split('\t').next() == Some(path)).unwrap() as i64;
            let Ok((bytes, _)) = archives.page(id, std::path::Path::new(path), page) else {
                println!("{k:02}: can't read page {page} of {path}");
                continue;
            };
            let t = std::time::Instant::now();
            let mut notes = Vec::new();
            let panels = detect_with(&bytes, rtl, &mut |s| notes.push(s)).unwrap();
            println!("{k:02}: {} p{page}: {} panels in {:?}", path.rsplit('/').next().unwrap(), panels.len(), t.elapsed());
            println!("     = {:?}", panels.iter().map(|p| format!("{:.2},{:.2} {:.2}x{:.2}", p.x, p.y, p.w, p.h)).collect::<Vec<_>>());
            for note in notes {
                println!("     {note}");
            }
            let full = image::load_from_memory(&bytes).unwrap().to_rgb8();
            let mut img = image::imageops::resize(&full, 900, 900 * full.height() / full.width(), FilterType::Triangle);
            for (j, p) in panels.iter().enumerate() {
                let c = colors[j % colors.len()];
                let (w, h) = (img.width() as f32, img.height() as f32);
                let corners = [[p.x, p.y], [p.x + p.w, p.y], [p.x + p.w, p.y + p.h], [p.x, p.y + p.h]];
                let shape = p.poly.clone().unwrap_or(corners.to_vec());
                for q in 0..shape.len() {
                    let (a, b) = (shape[q], shape[(q + 1) % shape.len()]);
                    line(&mut img, (a[0] * w, a[1] * h), (b[0] * w, b[1] * h), 5, c);
                }
                for d in 0..=j as u32 {
                    let (x0, y0) = ((p.x * w) as u32 + 10 + d * 18, (p.y * h) as u32 + 10);
                    for y in y0..(y0 + 13).min(img.height()) {
                        for x in x0..(x0 + 13).min(img.width()) {
                            img.put_pixel(x, y, c);
                        }
                    }
                }
            }
            let thumb = image::imageops::resize(&img, tw - 6, (th - 6).min((tw - 6) * img.height() / img.width()), FilterType::Triangle);
            let slot = n % 8;
            image::imageops::overlay(&mut sheet, &thumb, ((slot % across) * tw + 3) as i64, ((slot / across) * th + 3) as i64);
            n += 1;
            if n % 8 == 0 {
                sheet.save(out.join(format!("sheet-{:02}.png", n / 8))).unwrap();
                sheet = RgbImage::from_pixel(tw * across, th * 2, Rgb([128, 128, 128]));
            }
        }
        if n % 8 != 0 {
            sheet.save(out.join(format!("sheet-{:02}.png", n / 8 + 1))).unwrap();
        }
    }

    #[test]
    #[ignore]
    fn real_panels() {
        let root = std::path::PathBuf::from(std::env::var("AION_TEST_ROOT").unwrap());
        let out = std::env::temp_dir().join("aion-panels");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let archives = Archives::new(&out);
        // (series folder, volume file name contains ("" = first volume), rtl, page)
        let cases = [
            ("Manga/Vinland Saga", "", true, 40usize),
            ("Manga/Vinland Saga", "", true, 60),
            ("Manga/Vinland Saga", "", true, 90),
            ("Manga/Vinland Saga", "", true, 120),
            ("Manga/AKIRA", "", true, 30),
            ("Manga/AKIRA", "", true, 55),
            ("Manga/Vagabond", "", true, 30),
            ("Manga/Vagabond", "", true, 60),
            ("Manga/The Darwin Incident", "", true, 20),
            ("Manga/The Darwin Incident", "", true, 45),
            ("Manga/The Darwin Incident", "v02", true, 90),
            ("Indie/Saga", "", false, 8),
            ("Indie/Saga", "", false, 15),
            ("DC/Batman - The Long Halloween", "", false, 12),
            ("DC/Batman - The Long Halloween", "", false, 25),
            ("Indie/The Black Beetle", "", false, 10),
            ("Indie/The Black Beetle", "", false, 20),
            ("Marvel/Daredevil by Frank Miller", "", false, 20),
            ("Marvel/Daredevil by Frank Miller", "", false, 35),
            ("DC/Saga of the Swamp Thing by Alan Moore", "vol_04", false, 50),
            ("DC/Saga of the Swamp Thing by Alan Moore", "vol_06", false, 32),
            ("Indie/The Usagi Yojimbo Saga", "Book 03", false, 23),
            ("Manga/The Darwin Incident", "v03", true, 41),
            ("Manga/Vagabond", "v37", true, 50),
            ("Marvel/Daredevil by Frank Miller", "v02", false, 32),
            // Covers, title and chapter pages: no guided steps.
            ("Marvel/Daredevil by Frank Miller", "", false, 0),
            ("Marvel/Daredevil by Frank Miller", "", false, 2),
            ("Indie/The Black Beetle", "", false, 0),
            ("Indie/The Black Beetle", "", false, 8),
            ("Indie/Saga", "", false, 0),
            ("Indie/Saga", "", false, 2),
            ("Indie/Saga", "", false, 4),
            ("Manga/Vinland Saga", "", true, 0),
            ("Manga/AKIRA", "", true, 0),
            ("Manga/Vagabond", "", true, 2),
            ("Manga/The Darwin Incident", "", true, 0),
            ("Manga/The Darwin Incident", "", true, 11),
            ("DC/Batman - The Long Halloween", "", false, 0),
            ("DC/Batman - The Long Halloween", "", false, 6),
            ("DC/Batman - The Long Halloween", "", false, 7),
        ];
        let colors = [Rgb([230, 40, 40]), Rgb([40, 160, 240]), Rgb([40, 200, 90]), Rgb([240, 170, 30]), Rgb([200, 60, 220]), Rgb([30, 220, 220]), Rgb([255, 120, 0]), Rgb([120, 120, 255])];
        let only = std::env::var("AION_PANELS_ONLY").unwrap_or_default();
        for (i, (dir, volume, rtl, page)) in cases.iter().enumerate() {
            let name = format!("{dir}{}{volume} p{page}", if volume.is_empty() { "" } else { " " });
            if !name.contains(&only) {
                continue;
            }
            let file = walkdir::WalkDir::new(root.join(dir)).into_iter().filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file() && e.path().extension().map(|x| x == "cbz" || x == "cbr").unwrap_or(false))
                .map(|e| e.into_path())
                .filter(|p| p.file_name().unwrap().to_string_lossy().contains(volume))
                .min_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy())).unwrap();
            let (bytes, _) = archives.page(i as i64, &file, *page).unwrap();
            let t = std::time::Instant::now();
            let panels = detect(&bytes, *rtl).unwrap();
            println!("{name}: {} panels in {:?}", panels.len(), t.elapsed());
            let mut img = image::load_from_memory(&bytes).unwrap().to_rgb8();
            for (k, p) in panels.iter().enumerate() {
                outline(&mut img, p, colors[k % colors.len()]);
                println!("   {k}: x{:.2} y{:.2} w{:.2} h{:.2}{}", p.x, p.y, p.w, p.h, if p.poly.is_some() { " poly" } else { "" });
            }
            let small = image::imageops::resize(&img, 600, 600 * img.height() / img.width(), FilterType::Triangle);
            let file_name = name.rsplit('/').next().unwrap().replace(' ', "_");
            small.save(out.join(format!("{i:02}-{file_name}.png"))).unwrap();
        }
    }
}
