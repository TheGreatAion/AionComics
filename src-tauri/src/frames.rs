//! Panels found by their drawn frames: the third way of reading a page, for the layouts the gutter
//! passes can't cut apart. Panels that overlap (one laid across the tops of the ones below it),
//! and framed panels set over art that fills the page, have no gutter running between them, but
//! every one of them has a border drawn around what shows of it.
//!
//! 1. Mark border evidence: pixels on a thin line darker than what's beside it, or on the light
//!    edge where a white gutter or rule meets art.
//! 2. Find long straight runs of it, leaning up to about 9°, across and down the page.
//! 3. Pair them into four-sided frames whose sides are drawn most of the way round.
//! 4. Take frames smallest first, so a box around a whole row or the page gives way to the panels
//!    inside it. A frame partly hidden under one already taken counts the hidden part as drawn:
//!    that's how a panel overlapped by another is still found.
//! 5. Art left around the frames (the page behind inset strips) becomes panels of its own.
//!
//! It only runs when the gutter passes found nothing, and it only answers when it finds a few
//! frames that cover a fair part of the page, so splash pages stay whole.

use image::GrayImage;

use crate::panels::Panel;

/// A border line pixel: a dark one is at most this bright, a light one at least this.
const DARK: u8 = 90;
const LIGHT: u8 = 228;
/// A thin dark line: one side clearly brighter, the other at least a little. (The edge of a
/// shadow is dark on one side and no brighter on the other.)
const STRONG: i16 = 60;
const WEAK: i16 = 15;
/// A light line (white gutter, or a white rule): much darker art beside it.
const LIGHT_EDGE: i16 = 80;
/// How far either side of a line to look for that contrast: borders run from hairline to thick.
const SIDES: [usize; 2] = [3, 5];
/// Steepest lean of a line, as a share of its length (tan 9°).
const MAX_LEAN: f32 = 0.16;
/// Gaps a line may jump (balloon tails, art crossing it), in work pixels.
const GAP: usize = 5;
/// Shortest line kept, as a share of the page side it runs along.
const MIN_LINE: f32 = 0.12;
/// Lines kept per direction, longest first.
const MAX_LINES: usize = 160;
/// A side is drawn where evidence lies within this many pixels of it.
const NEAR: i32 = 2;
/// Share of every side that must be drawn (or hidden under a frame already taken).
const DRAWN: f32 = 0.78;
/// Before measuring a side properly, the line found for it must cover at least this much of it.
const ROUGH: f32 = 0.35;
/// More frames than this, or frames adding up to more than this share of the page, is art seen
/// as frames (buildings, hatching): the page gets none.
const MAX_FRAMES: usize = 14;
const MAX_FRAMED: f32 = 1.25;
/// Frames smaller than this share of the page are art (windows, screens, signs).
const MIN_FRAME: f32 = 0.025;
/// Frames bigger than this are the page (or a border round it).
const MAX_FRAME: f32 = 0.8;
/// How much of the smaller of two frames may overlap the other, unless one hides the other.
const MAX_OVERLAP: f32 = 0.25;
/// A page has frames when there are at least two covering this much of it.
const MIN_FRAMED: f32 = 0.15;
/// Art left around the frames counts as a panel from this share of the page.
const MIN_REST: f32 = 0.06;

/// A straight run of border evidence. Across the page it's y = a + s·x (x from `from` to `to`);
/// down the page, x = a + s·y.
#[derive(Clone, Copy, Debug)]
struct Line {
    a: f32,
    s: f32,
    from: f32,
    to: f32,
    /// The page edge: drawn all along for free (art bleeding off it has no border).
    edge: bool,
}

impl Line {
    fn at(&self, t: f32) -> f32 {
        self.a + self.s * t
    }
    /// Share of [lo, hi] this line runs along.
    fn covers(&self, lo: f32, hi: f32) -> f32 {
        ((self.to.min(hi) - self.from.max(lo)).max(0.0)) / (hi - lo).max(1.0)
    }
}

/// Border evidence, one map for lines running across (`across`) and one for lines running down.
struct Evidence {
    w: usize,
    h: usize,
    across: Vec<bool>,
    down: Vec<bool>,
}

impl Evidence {
    fn new(img: &GrayImage) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.as_raw();
        let mark = |across: bool| {
            let mut out = vec![false; w * h];
            for y in 0..h {
                for x in 0..w {
                    let v = px[y * w + x] as i16;
                    let (dark, light) = (v <= DARK as i16, v >= LIGHT as i16);
                    if !dark && !light {
                        continue;
                    }
                    let side = |d: usize, before: bool| -> Option<i16> {
                        let (xx, yy) = if across {
                            (x as isize, y as isize + if before { -(d as isize) } else { d as isize })
                        } else {
                            (x as isize + if before { -(d as isize) } else { d as isize }, y as isize)
                        };
                        (xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h)
                            .then(|| px[yy as usize * w + xx as usize] as i16)
                    };
                    out[y * w + x] = SIDES.iter().any(|&d| {
                        let (Some(p), Some(q)) = (side(d, true), side(d, false)) else { return false };
                        if dark {
                            let (hi, lo) = (p.max(q) - v, p.min(q) - v);
                            hi >= STRONG && lo >= WEAK
                        } else {
                            v - p.min(q) >= LIGHT_EDGE
                        }
                    });
                }
            }
            out
        };
        Evidence { w, h, across: mark(true), down: mark(false) }
    }

    /// Evidence near (x, y) for a line running across (or down).
    fn near(&self, across: bool, x: f32, y: f32) -> bool {
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        let map = if across { &self.across } else { &self.down };
        (-NEAR..=NEAR).any(|d| {
            let (xx, yy) = if across { (xi, yi + d) } else { (xi + d, yi) };
            xx >= 0 && yy >= 0 && (xx as usize) < self.w && (yy as usize) < self.h && map[yy as usize * self.w + xx as usize]
        })
    }

    /// Straight runs of evidence across the page (or down it), longest first, near-duplicates dropped.
    fn lines(&self, across: bool) -> Vec<Line> {
        let (len, depth) = if across { (self.w, self.h) } else { (self.h, self.w) };
        let max_lean = (len as f32 * MAX_LEAN) as i32;
        let min_len = (len as f32 * MIN_LINE) as usize;
        let map = if across { &self.across } else { &self.down };
        let hit = |t: usize, s: i32| -> bool {
            if s < 0 || s as usize >= depth {
                return false;
            }
            let s = s as usize;
            let i = if across { s * self.w + t } else { t * self.w + s };
            map[i] || (s > 0 && map[if across { i - self.w } else { i - 1 }]) || (s + 1 < depth && map[if across { i + self.w } else { i + 1 }])
        };
        let mut found: Vec<Line> = Vec::new();
        for lean in (-max_lean..=max_lean).step_by(2) {
            let slope = lean as f32 / len as f32;
            for a in -max_lean..depth as i32 + max_lean {
                let (mut start, mut last, mut gap) = (None::<usize>, 0usize, 0usize);
                for t in 0..len {
                    let s = (a as f32 + slope * t as f32).round() as i32;
                    if hit(t, s) {
                        if start.is_none() {
                            start = Some(t);
                        }
                        last = t;
                        gap = 0;
                    } else if start.is_some() {
                        gap += 1;
                        if gap > GAP {
                            let s0 = start.take().unwrap();
                            if last - s0 >= min_len {
                                found.push(Line { a: a as f32, s: slope, from: s0 as f32, to: last as f32, edge: false });
                            }
                        }
                    }
                }
                if let Some(s0) = start.filter(|&s0| last - s0 >= min_len) {
                    found.push(Line { a: a as f32, s: slope, from: s0 as f32, to: last as f32, edge: false });
                }
            }
        }
        // The same border shows up at neighboring offsets and leans: keep the longest of each.
        found.sort_by(|p, q| (q.to - q.from).total_cmp(&(p.to - p.from)));
        let mut kept: Vec<Line> = Vec::new();
        for l in found {
            let mid = (l.from + l.to) / 2.0;
            let same = kept.iter().any(|k| {
                let shared = k.to.min(l.to) - k.from.max(l.from);
                shared > 0.5 * (l.to - l.from) && (k.at(mid) - l.at(mid)).abs() <= 5.0
            });
            if !same {
                kept.push(l);
                if kept.len() >= MAX_LINES {
                    break;
                }
            }
        }
        // The page's own edges bound panels whose art bleeds off them.
        let far = (depth - 1) as f32;
        kept.push(Line { a: 0.0, s: 0.0, from: 0.0, to: len as f32, edge: true });
        kept.push(Line { a: far, s: 0.0, from: 0.0, to: len as f32, edge: true });
        kept
    }
}

/// A four-sided frame: corners clockwise from top-left, in work pixels.
#[derive(Clone, Debug)]
struct Frame {
    corners: [(f32, f32); 4],
    sides: [Line; 4],
    area: f32,
    /// How many of its sides are the page edge.
    edges: usize,
    box_: (f32, f32, f32, f32),
}

impl Frame {
    fn new(top: Line, right: Line, bottom: Line, left: Line) -> Self {
        // top/bottom: y = a + s x; left/right: x = a + s y.
        let meet = |h: &Line, v: &Line| {
            let y = (h.a + h.s * v.a) / (1.0 - h.s * v.s);
            (v.a + v.s * y, y)
        };
        let corners = [meet(&top, &left), meet(&top, &right), meet(&bottom, &right), meet(&bottom, &left)];
        let area = 0.5
            * (0..4)
                .map(|i| {
                    let (p, q) = (corners[i], corners[(i + 1) % 4]);
                    p.0 * q.1 - q.0 * p.1
                })
                .sum::<f32>()
                .abs();
        let xs = corners.map(|c| c.0);
        let ys = corners.map(|c| c.1);
        let box_ = (
            xs.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
            ys.iter().copied().fold(f32::MIN, f32::max),
        );
        let sides = [top, right, bottom, left];
        let edges = sides.iter().filter(|s| s.edge).count();
        Frame { corners, sides, area, edges, box_ }
    }

    fn contains(&self, (x, y): (f32, f32)) -> bool {
        // Inside a convex quad clockwise on screen (y down): every edge has the point on its right.
        (0..4).all(|i| {
            let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
            (q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0) >= -1.0
        })
    }

    /// Share of the box of `o` that's also in this one's box: a cheap overlap.
    fn overlap(&self, o: &Frame) -> f32 {
        let (a, b) = (self.box_, o.box_);
        let w = a.2.min(b.2) - a.0.max(b.0);
        let h = a.3.min(b.3) - a.1.max(b.1);
        w.max(0.0) * h.max(0.0)
    }

    fn inside(&self, o: &Frame, slack: f32) -> bool {
        let (a, b) = (self.box_, o.box_);
        a.0 >= b.0 - slack && a.1 >= b.1 - slack && a.2 <= b.2 + slack && a.3 <= b.3 + slack
    }

    /// The least-drawn side's drawn share, counting what lies under `over` (frames already taken,
    /// which may be laid across this one) as drawn.
    fn drawn(&self, ev: &Evidence, over: &[&Frame]) -> f32 {
        self.side_scores(ev, over).into_iter().fold(1.0, f32::min)
    }

    /// Each side's drawn share (see `drawn`), clockwise from the top.
    fn side_scores(&self, ev: &Evidence, over: &[&Frame]) -> [f32; 4] {
        [0, 1, 2, 3].map(|i| {
                if self.sides[i].edge {
                    return 1.0;
                }
                let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
                let across = i % 2 == 0;
                let n = (((q.0 - p.0).abs().max((q.1 - p.1).abs())) / 2.0).max(1.0) as usize;
                // The ends of a side meet the neighboring sides' borders: skip a few pixels there.
                let (skip, mut on, mut all) = (3.min(n / 4), 0usize, 0usize);
                for k in skip..=n - skip {
                    let f = k as f32 / n as f32;
                    let pt = (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f);
                    all += 1;
                    if ev.near(across, pt.0, pt.1) || over.iter().any(|o| o.contains(pt)) {
                        on += 1;
                    }
                }
                on as f32 / all.max(1) as f32
        })
    }

    fn panel(&self, w: f32, h: f32) -> Panel {
        let (x0, y0, x1, y1) = self.box_;
        let (x0, y0, x1, y1) = (x0.max(0.0), y0.max(0.0), x1.min(w), y1.min(h));
        let slanted = self.sides.iter().any(|s| s.s.abs() > 0.004);
        Panel {
            x: x0 / w,
            y: y0 / h,
            w: (x1 - x0) / w,
            h: (y1 - y0) / h,
            poly: slanted.then(|| self.corners.iter().map(|&(x, y)| [(x / w).clamp(0.0, 1.0), (y / h).clamp(0.0, 1.0)]).collect()),
        }
    }
}

/// Frames whose four sides lie on found lines and are drawn at least roughly, from the lines'
/// extents alone (cheap); `drawn` measures them properly later.
fn candidates(across: &[Line], down: &[Line], w: f32, h: f32) -> Vec<Frame> {
    let page = w * h;
    let mut out = Vec::new();
    for (i, t) in across.iter().enumerate() {
        for b in &across[i + 1..] {
            let (top, bottom) = if t.at(w / 2.0) < b.at(w / 2.0) { (*t, *b) } else { (*b, *t) };
            if bottom.at(w / 2.0) - top.at(w / 2.0) < 0.05 * h || (top.edge && bottom.edge) {
                continue;
            }
            let (ox0, ox1) = (top.from.max(bottom.from), top.to.min(bottom.to));
            if ox1 - ox0 < 0.08 * w && !(top.edge || bottom.edge) {
                continue;
            }
            let (y_lo, y_hi) = (top.at(w / 2.0), bottom.at(w / 2.0));
            let sides: Vec<&Line> = down
                .iter()
                .filter(|v| v.covers(y_lo, y_hi) >= ROUGH)
                .filter(|v| {
                    let x = v.at((y_lo + y_hi) / 2.0);
                    x >= top.from.min(bottom.from) - 12.0 && x <= top.to.max(bottom.to) + 12.0
                })
                .collect();
            for (j, l) in sides.iter().enumerate() {
                for r in &sides[j + 1..] {
                    let (left, right) = if l.at(y_lo) < r.at(y_lo) { (**l, **r) } else { (**r, **l) };
                    if left.edge && right.edge && (top.edge || bottom.edge) {
                        continue; // three page edges and one line: a cut, not a frame
                    }
                    let (x_lo, x_hi) = (left.at((y_lo + y_hi) / 2.0), right.at((y_lo + y_hi) / 2.0));
                    if x_hi - x_lo < 0.08 * w {
                        continue;
                    }
                    if top.covers(x_lo, x_hi) < ROUGH || bottom.covers(x_lo, x_hi) < ROUGH {
                        continue;
                    }
                    let f = Frame::new(top, right, bottom, left);
                    if (MIN_FRAME..=MAX_FRAME).contains(&(f.area / page)) {
                        out.push(f);
                    }
                }
            }
        }
    }
    out
}

/// The frames on a page and the art around them, as panels (normalized, unordered); empty when
/// the page doesn't look framed.
pub fn find(img: &GrayImage, log: &mut dyn FnMut(String)) -> Vec<Panel> {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let ev = Evidence::new(img);
    let (across, down) = (ev.lines(true), ev.lines(false));
    let mut cands = candidates(&across, &down, w, h);
    log(format!("frames: {} across, {} down, {} candidates", across.len(), down.len(), cands.len()));
    // Smallest first: a box around a whole row (or the page) gives way to the panels in it.
    cands.sort_by(|a, b| a.area.total_cmp(&b.area));
    let mut taken: Vec<Frame> = Vec::new();
    // Twice: frames drawn all round first, then ones partly hidden under those.
    for hidden in [false, true] {
        for f in &cands {
            if taken.iter().any(|t| t.inside(f, 8.0) && f.inside(t, 8.0)) {
                continue; // the same frame again
            }
            let over: Vec<&Frame> = if hidden { taken.iter().collect() } else { Vec::new() };
            if f.drawn(&ev, &over) < DRAWN {
                continue;
            }
            // Frames inside this one: none (a panel), or one set clear of its edges (an inset).
            let inner: Vec<&Frame> = taken.iter().filter(|t| t.inside(f, 6.0)).collect();
            let clear = |t: &Frame| {
                let (a, b) = (t.box_, f.box_);
                [a.0 - b.0, b.2 - a.2, a.1 - b.1, b.3 - a.3].iter().all(|d| *d > 12.0)
            };
            if inner.len() > 1 || inner.first().is_some_and(|t| !clear(t)) {
                continue;
            }
            // Otherwise frames may only overlap a little, unless one lies across the other.
            let clash = taken.iter().any(|t| {
                if t.inside(f, 6.0) {
                    return false;
                }
                let o = t.overlap(f);
                o > MAX_OVERLAP * t.area.min(f.area) && !(hidden && o < 0.6 * f.area)
            });
            if !clash {
                taken.push(f.clone());
            }
        }
    }
    let page = w * h;
    let framed: f32 = taken.iter().map(|f| f.area).sum::<f32>() / page;
    let drawn_frames = taken.iter().filter(|f| f.edges == 0).count();
    log(format!(
        "frames: {} taken ({} drawn all round), {:.2} of the page: {:?}",
        taken.len(),
        drawn_frames,
        framed,
        taken.iter().map(|f| format!("{:.2},{:.2} {:.2}x{:.2} e{}", f.box_.0 / w, f.box_.1 / h, (f.box_.2 - f.box_.0) / w, (f.box_.3 - f.box_.1) / h, f.edges)).collect::<Vec<_>>()
    ));
    if taken.len() < 2 || taken.len() > MAX_FRAMES || drawn_frames == 0 || !(MIN_FRAMED..=MAX_FRAMED).contains(&framed) {
        return Vec::new();
    }
    let mut out: Vec<Panel> = taken.iter().map(|f| f.panel(w, h)).collect();
    out.extend(rest(img, &taken));
    out
}

/// For tuning: the page dimmed, border evidence across in red and down in green, the lines found
/// in blue, and the best few candidates' scores printed.
#[cfg(test)]
pub fn debug(img: &GrayImage) -> (image::RgbImage, Vec<String>) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let ev = Evidence::new(img);
    let mut out = image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let i = (y * img.width() + x) as usize;
        let v = img.as_raw()[i] / 3;
        image::Rgb([if ev.across[i] { 255 } else { v }, if ev.down[i] { 255 } else { v }, v])
    });
    let (across, down) = (ev.lines(true), ev.lines(false));
    for (lines, is_across) in [(&across, true), (&down, false)] {
        for l in lines.iter().filter(|l| !l.edge) {
            let mut t = l.from;
            while t <= l.to {
                let (x, y) = if is_across { (t, l.at(t)) } else { (l.at(t), t) };
                if x >= 0.0 && y >= 0.0 && x < w && y < h {
                    out.put_pixel(x as u32, y as u32, image::Rgb([60, 140, 255]));
                }
                t += 1.0;
            }
        }
    }
    let mut cands = candidates(&across, &down, w, h);
    let mut scored: Vec<(f32, String)> = cands
        .drain(..)
        .map(|f| {
            let sides = f.side_scores(&ev, &[]);
            let s = sides.iter().copied().fold(1.0, f32::min);
            (s, format!("{s:.2} {:.2?} {:.2},{:.2} {:.2}x{:.2} e{}", sides, f.box_.0 / w, f.box_.1 / h, (f.box_.2 - f.box_.0) / w, (f.box_.3 - f.box_.1) / h, f.edges))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    (out, scored.into_iter().take(12).map(|s| s.1).collect())
}

/// Art not inside any frame, in connected pieces big enough to be panels (the page showing behind
/// inset frames). Margins and gutters (mostly paper) don't count.
fn rest(img: &GrayImage, frames: &[Frame]) -> Vec<Panel> {
    const CELL: usize = 10;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let (cw, ch) = (w / CELL, h / CELL);
    let px = img.as_raw();
    let mut free = vec![false; cw * ch];
    for cy in 0..ch {
        for cx in 0..cw {
            let c = ((cx * CELL + CELL / 2) as f32, (cy * CELL + CELL / 2) as f32);
            let grown = |f: &Frame| {
                let (x0, y0, x1, y1) = f.box_;
                c.0 >= x0 - 6.0 && c.0 <= x1 + 6.0 && c.1 >= y0 - 6.0 && c.1 <= y1 + 6.0 && f.contains(c)
                    || f.contains((c.0 - 6.0, c.1))
                    || f.contains((c.0 + 6.0, c.1))
                    || f.contains((c.0, c.1 - 6.0))
                    || f.contains((c.0, c.1 + 6.0))
            };
            if frames.iter().any(grown) {
                continue;
            }
            let mut paper = 0;
            for yy in cy * CELL..(cy + 1) * CELL {
                for xx in cx * CELL..(cx + 1) * CELL {
                    let v = px[yy * w + xx];
                    paper += (v >= 235 || v <= 12) as usize;
                }
            }
            free[cy * cw + cx] = paper * 10 < CELL * CELL * 7;
        }
        // Beside a frame, a narrow strip of art (the page showing between frame and edge) mustn't
        // join the art above the frame to the art below it.
        let fy = (cy * CELL + CELL / 2) as f32;
        if frames.iter().any(|f| fy >= f.box_.1 - 6.0 && fy <= f.box_.3 + 6.0) {
            let row = &mut free[cy * cw..(cy + 1) * cw];
            let mut x = 0;
            while x < cw {
                if !row[x] {
                    x += 1;
                    continue;
                }
                let end = (x..cw).find(|&e| !row[e]).unwrap_or(cw);
                if ((end - x) as f32) < 0.2 * cw as f32 {
                    row[x..end].iter_mut().for_each(|c| *c = false);
                }
                x = end;
            }
        }
    }
    let mut seen = vec![false; cw * ch];
    let mut out = Vec::new();
    for start in 0..cw * ch {
        if !free[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (cw, ch, 0, 0, 0);
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i % cw, i / cw);
            (x0, y0, x1, y1, n) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y), n + 1);
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < cw && ny < ch {
                    let j = ny * cw + nx;
                    if free[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let share = ((x1 + 1 - x0) * (y1 + 1 - y0)) as f32 / (cw * ch) as f32;
        if n as f32 >= MIN_REST * (cw * ch) as f32 && share < 0.7 {
            out.push(Panel {
                x: x0 as f32 / cw as f32,
                y: y0 as f32 / ch as f32,
                w: (x1 + 1 - x0) as f32 / cw as f32,
                h: (y1 + 1 - y0) as f32 / ch as f32,
                poly: None,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    fn art(x: u32, y: u32) -> Luma<u8> {
        Luma([90 + ((x * 7 + y * 13) % 60) as u8])
    }

    /// Art filling the page, with two black-framed strips laid across it.
    #[test]
    fn finds_inset_strips() {
        let mut img = GrayImage::from_fn(600, 900, art);
        let mut frame = |x0: u32, y0: u32, x1: u32, y1: u32| {
            for y in y0..y1 {
                for x in x0..x1 {
                    let edge = x < x0 + 3 || x >= x1 - 3 || y < y0 + 3 || y >= y1 - 3;
                    img.put_pixel(x, y, Luma([if edge { 10 } else { 200 }]));
                }
            }
        };
        frame(30, 300, 570, 430);
        frame(30, 470, 570, 600);
        let found = find(&img, &mut |_| {});
        let strips = found.iter().filter(|p| p.w > 0.8 && p.h < 0.2).count();
        assert_eq!(strips, 2, "{found:?}");
        assert!(found.iter().any(|p| p.y < 0.05 && p.h > 0.2), "the page above: {found:?}");
        assert!(found.iter().any(|p| p.y > 0.6 && p.h > 0.2), "the page below: {found:?}");
    }

    /// A slanted panel laid across the tops of two panels below it, on white paper.
    #[test]
    fn finds_overlapping_panels() {
        let mut img = GrayImage::from_pixel(600, 900, Luma([250]));
        let border = |img: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32| {
            for y in y0..y1 {
                for x in x0..x1 {
                    let edge = x < x0 + 3 || x >= x1 - 3 || y < y0 + 3 || y >= y1 - 3;
                    img.put_pixel(x, y, if edge { Luma([15]) } else { art(x, y) });
                }
            }
        };
        border(&mut img, 30, 30, 570, 300); // top panel
        border(&mut img, 30, 380, 290, 870); // bottom left
        border(&mut img, 310, 380, 570, 870); // bottom right
        border(&mut img, 80, 320, 520, 520); // laid across the tops of both
        let found = find(&img, &mut |_| {});
        assert!(found.len() >= 4, "{found:?}");
        let near = |x: f32, y: f32| found.iter().any(|p| (p.x - x).abs() < 0.04 && (p.y - y).abs() < 0.04);
        assert!(near(0.05, 0.033) && near(0.133, 0.355), "{found:?}");
        assert!(near(0.05, 0.42) || near(0.05, 0.58), "bottom left, partly hidden: {found:?}");
    }
}
