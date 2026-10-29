//! Panels found by their drawn frames: the third way of reading a page, for the layouts the gutter
//! passes can't cut apart. Panels that overlap (one laid across the tops of the ones below it),
//! and framed panels set over art that fills the page, have no gutter running between them, but
//! every one of them has a border drawn around what shows of it.
//!
//! 1. Mark border evidence: pixels on a thin line darker than what's beside it, or on the light
//!    edge where a white gutter or rule meets art.
//! 2. Find long straight runs of it, leaning up to about 9°, across and down the page.
//! 3. Pair them into four-sided frames whose sides are drawn most of the way round. Two borders
//!    ending level may be closed by an open side, where art laid over the panel hides its edge.
//! 4. Take frames smallest first, so a box around a whole row or the page gives way to the panels
//!    inside it. A frame partly hidden under one already taken counts the hidden part as drawn:
//!    that's how a panel overlapped by another is still found. Frames with a gutter running
//!    through them, or one flat tone inside, are shapes in the art and never taken.
//! 5. Art left around the frames (the page behind inset strips) becomes panels of its own.
//!
//! Black borders are looked for first, dark colored ones only when black shows next to nothing.
//! It only runs when the gutter passes found nothing, and it only answers when it finds a few
//! frames that cover a fair part of the page, lie along its paper (or are drawn cleanly all
//! round), and barely cross each other, so splash pages, covers and busy art stay whole.

use image::GrayImage;

use crate::panels::Panel;

/// Border evidence is read off the page smoothed along the line's direction only: texture and
/// hatching average out, while the band of a border across it stays sharp.
const ALONG: usize = 5;
/// A border is a solid dark band: every pixel across it at most this bright (smoothed)... Black
/// first; failing that, dark colors too (a navy border reads about 70). Taking dark colors from
/// the start breaks borders apart where dark art runs into them.
const BANDS: [i16; 2] = [48, 80];
/// Dark colors also turn up borders in lettering, logos and shaded art, so frames found only with
/// them must cover at least this share of the page, every side this dense.
const FALLBACK_FRAMED: f32 = 0.3;
const FALLBACK_DENSE: f32 = 0.8;
/// And they are only looked for when black ones show at most this many lines.
const FEW_LINES: usize = 20;
/// Fewer lines than this at work size (both directions together), and borders are looked for again
/// on the page at twice the size.
const FINE_LINES: usize = 200;
/// Solid black: art, background or a border too dark to tell apart.
const BLACK: i16 = 48;
/// ...and this many pixels thick. Thinner is an inked line in the art (a building, hatching);
/// thicker is a dark mass (a figure, a shadow).
const MIN_BAND: usize = 3;
const MAX_BAND: usize = 10;
/// Beside the band, one side clearly brighter and the other at least a little (a few pixels out,
/// past any soft edge).
const STRONG: i16 = 30;
const WEAK: i16 = 6;
const BESIDE: usize = 2;
/// Steepest lean of a line, as a share of its length (tan 9°).
const MAX_LEAN: f32 = 0.16;
/// Gaps a line may jump (balloons, art crossing it), in work pixels. Generous: lines only propose
/// frames, and every frame is measured against the evidence itself.
const GAP: usize = 24;
/// Shortest line kept, as a share of the page side it runs along.
const MIN_LINE: f32 = 0.06;
/// Lines kept per direction, longest first.
const MAX_LINES: usize = 160;
/// More possible frames than this and the lines are busy art (hatching, buildings, painted
/// texture), not borders: no page with frames shows near this many, and judging them all takes
/// seconds.
const MAX_CANDIDATES: usize = 10_000;
/// A side is drawn where evidence lies within this many pixels of it.
const NEAR: i32 = 2;
/// A side needs real evidence along at least this share of it: the rest may run through solid black
/// (a border between black art and black background shows nowhere, and proves nothing either way).
const SEEN: f32 = 0.4;
/// Near white across the whole width of a border: a balloon laid over it.
const BALLOON: i16 = 228;
/// Share of every side that must be drawn (or hidden under a frame already taken), out of the
/// stretches that say anything: evidence, or bright art crossing where the border would be.
const DRAWN: f32 = 0.78;
/// Before measuring a side properly, the line found for it must cover at least this much of it.
const ROUGH: f32 = 0.35;
/// More frames than this, or frames adding up to more than this share of the page, is art seen
/// as frames (buildings, hatching): the page gets none.
const MAX_FRAMES: usize = 14;
/// Or more pairs of frames lying across each other than this (see `crossings`).
const MAX_CROSSINGS: usize = 2;
/// A frame has at least this many sides within this lean of straight: panels are drawn slanted
/// on one or two sides at most, while boxes formed by chance among strokes of art lean every way.
const STRAIGHT: f32 = 0.04;
const STRAIGHT_SIDES: usize = 2;
/// And at least two sides on lines this dense (see `Line::dense`).
const DENSE: f32 = 0.75;
const DENSE_SIDES: usize = 2;
/// A frame with more than this share of its inside in one tone is a shape in the art.
const FLAT: f32 = 0.7;
/// Frames with no paper beside them count only if every side is at least this dense.
const CLEAN: f32 = 0.85;
const MAX_FRAMED: f32 = 1.25;
/// Just past a real panel's side is gutter, page background, or the border of the panel next to
/// it; past a straight line in the art (a door, a card, a figure's edge) the same art carries on.
/// Looked for this far out, and needed past every side (`OUT_SIDES`) for at least `OUT_SHARE` of
/// their length.
const OUT_FROM: f32 = 6.0;
const OUT_TO: f32 = 12.0;
const OUT_SIDES: usize = 4;
const OUT_SHARE: f32 = 0.5;
/// Gutter-like ground: near white, or as dark as a border.
const GROUND_LIGHT: i16 = 212;
/// Or a different picture: framed art laid over the page's art differs from it across the frame,
/// by at least this much in average brightness either side of the line.
const CHANGE: f32 = 22.0;

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
    /// Not drawn, but taken as drawn: a frame's side hidden behind art laid over it, closing the
    /// frame across the level ends of two borders (see `closings`).
    open: bool,
    /// Samples along it that landed on evidence: how well it follows a border, gaps aside.
    hits: u32,
    /// Its length as found, before running it on through black (see `Evidence::lines`).
    span: f32,
}

impl Line {
    fn at(&self, t: f32) -> f32 {
        self.a + self.s * t
    }
    /// Share of its length as found that lies on evidence: near 1 for a drawn border, well under
    /// for a path picked through the strokes of busy art.
    fn dense(&self) -> f32 {
        self.hits as f32 / (self.span + 1.0)
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
    px: Vec<u8>,
    across: Vec<bool>,
    down: Vec<bool>,
}

impl Evidence {
    fn new(img: &GrayImage, band: i16) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let px = img.as_raw();
        Evidence { w, h, px: px.to_vec(), across: marks(px, w, h, band, true, 1), down: marks(px, w, h, band, false, 1) }
    }

    /// The same, with the evidence read off `fine`, the page at about twice the size. Shrinking a
    /// page blurs a border only a pixel or two wide into the art around it; at twice the size it's
    /// still a dark line. The marks are then laid back onto the work-size grid.
    fn new_fine(img: &GrayImage, fine: &GrayImage, band: i16) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let (fw, fh) = (fine.width() as usize, fine.height() as usize);
        let pool = |m: Vec<bool>| {
            let mut out = vec![false; w * h];
            for y in 0..fh {
                let sy = (y * h / fh).min(h - 1);
                for x in 0..fw {
                    if m[y * fw + x] {
                        out[sy * w + (x * w / fw).min(w - 1)] = true;
                    }
                }
            }
            out
        };
        let fpx = fine.as_raw();
        Evidence { w, h, px: img.as_raw().to_vec(), across: pool(marks(fpx, fw, fh, band, true, 2)), down: pool(marks(fpx, fw, fh, band, false, 2)) }
    }
}

/// Border evidence for lines across (or down): pixels on every dark band of border thickness with
/// brighter surroundings. `k`: how many times the work size the pixels are. Band sizes scale with
/// it, except the thinnest band taken, which is what reading at twice the size is for.
fn marks(px: &[u8], w: usize, h: usize, band: i16, across: bool, scale: usize) -> Vec<bool> {
    let (along, beside, max_band) = (ALONG * scale - (scale - 1), BESIDE * scale, MAX_BAND * scale);
    // Smooth along the line: a running mean over ALONG pixels across x (lines across) or y.
    let mut sm = vec![0i16; w * h];
    let r = along / 2;
    if across {
        for y in 0..h {
            let row = &px[y * w..(y + 1) * w];
            for x in 0..w {
                let (lo, hi) = (x.saturating_sub(r), (x + r).min(w - 1));
                let sum: u32 = row[lo..=hi].iter().map(|&v| v as u32).sum();
                sm[y * w + x] = (sum / (hi - lo + 1) as u32) as i16;
            }
        }
    } else {
        for x in 0..w {
            for y in 0..h {
                let (lo, hi) = (y.saturating_sub(r), (y + r).min(h - 1));
                let sum: u32 = (lo..=hi).map(|yy| px[yy * w + x] as u32).sum();
                sm[y * w + x] = (sum / (hi - lo + 1) as u32) as i16;
            }
        }
    }
    // Walk across the line direction (down each column for lines across) and mark every dark band
    // of border thickness with brighter surroundings.
    let mut out = vec![false; w * h];
    let (n_lines, n_cross) = if across { (w, h) } else { (h, w) };
    let idx = |line: usize, k: usize| if across { k * w + line } else { line * w + k };
    for line in 0..n_lines {
        let mut k = 0;
        while k < n_cross {
            if sm[idx(line, k)] > band {
                k += 1;
                continue;
            }
            let start = k;
            while k < n_cross && sm[idx(line, k)] <= band {
                k += 1;
            }
            let (end, thick) = (k, k - start);
            if thick < MIN_BAND {
                continue;
            }
            let level = |from: usize, to: usize| (from..to).map(|j| sm[idx(line, j)]).max().unwrap_or(0);
            let before = (start > beside).then(|| sm[idx(line, start - 1 - beside)]);
            let after = (end + beside < n_cross).then(|| sm[idx(line, end + beside)]);
            if thick <= max_band {
                // A border band on its own: brighter both sides.
                let (Some(b), Some(a)) = (before, after) else { continue };
                let band = level(start, end);
                if b.max(a) - band >= STRONG && b.min(a) - band >= WEAK {
                    for j in start..end {
                        out[idx(line, j)] = true;
                    }
                }
            } else {
                // A border running into dark art beside it merges with it: keep the edge where a
                // panel's brighter inside meets the black.
                if before.is_some_and(|b| b - level(start, start + MIN_BAND) >= STRONG) {
                    for j in start..start + MIN_BAND {
                        out[idx(line, j)] = true;
                    }
                }
                if after.is_some_and(|a| a - level(end - MIN_BAND, end) >= STRONG) {
                    for j in end - MIN_BAND..end {
                        out[idx(line, j)] = true;
                    }
                }
            }
        }
    }
    out
}

impl Evidence {

    /// Evidence near (x, y) for a line running across (or down).
    fn near(&self, across: bool, x: f32, y: f32) -> bool {
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        let map = if across { &self.across } else { &self.down };
        (-NEAR..=NEAR).any(|d| {
            let (xx, yy) = if across { (xi, yi + d) } else { (xi + d, yi) };
            xx >= 0 && yy >= 0 && (xx as usize) < self.w && (yy as usize) < self.h && map[yy as usize * self.w + xx as usize]
        })
    }

    /// Where a border may run unseen, so the line says nothing either way: solid black within a
    /// couple of pixels across it (black art against black background), or near white all across
    /// (a balloon laid over the border).
    fn unseen(&self, across: bool, x: f32, y: f32) -> bool {
        let (xi, yi) = (x.round() as i32, y.round() as i32);
        let at = |d: i32| {
            let (xx, yy) = if across { (xi, yi + d) } else { (xi + d, yi) };
            (xx >= 0 && yy >= 0 && (xx as usize) < self.w && (yy as usize) < self.h).then(|| self.px[yy as usize * self.w + xx as usize] as i16)
        };
        (-NEAR..=NEAR).any(|d| at(d).is_some_and(|v| v <= BLACK)) || (-NEAR..=NEAR).all(|d| at(d).is_none_or(|v| v >= BALLOON))
    }

    /// Straight runs of evidence across the page (or down it), longest first, near-duplicates dropped.
    fn lines(&self, across: bool) -> Vec<Line> {
        let (len, depth) = if across { (self.w, self.h) } else { (self.h, self.w) };
        let max_lean = (len as f32 * MAX_LEAN) as i32;
        let min_len = (len as f32 * MIN_LINE) as usize;
        let map = if across { &self.across } else { &self.down };
        // Evidence within two pixels across, laid out along the line (row s, position t), so each
        // step along a line is one look.
        let mut near = vec![false; len * depth];
        for s in 0..depth {
            for t in 0..len {
                let at = |ss: usize| map[if across { ss * self.w + t } else { t * self.w + ss }];
                near[s * len + t] = (s.saturating_sub(2)..=(s + 2).min(depth - 1)).any(at);
            }
        }
        let hit = |t: usize, s: i32| -> bool { s >= 0 && (s as usize) < depth && near[s as usize * len + t] };
        // Every line through the page at every lean, the leans shared out between threads. Each
        // thread's runs come back in order of lean, so the result is the same as one thread's.
        let leans: Vec<i32> = (-max_lean..=max_lean).step_by(2).collect();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
        let chunk = leans.len().div_ceil(threads).max(1);
        let runs = |leans: &[i32]| -> Vec<Line> {
            let mut found: Vec<Line> = Vec::new();
            for &lean in leans {
                let slope = lean as f32 / len as f32;
                for a in -max_lean..depth as i32 + max_lean {
                    let (mut start, mut last, mut gap, mut hits) = (None::<usize>, 0usize, 0usize, 0u32);
                    for t in 0..len {
                        let s = (a as f32 + slope * t as f32).round() as i32;
                        if hit(t, s) {
                            if start.is_none() {
                                start = Some(t);
                                hits = 0;
                            }
                            last = t;
                            gap = 0;
                            hits += 1;
                        } else if start.is_some() {
                            gap += 1;
                            if gap > GAP {
                                let s0 = start.take().unwrap();
                                if last - s0 >= min_len {
                                    found.push(Line { a: a as f32, s: slope, from: s0 as f32, to: last as f32, open: false, hits, span: (last - s0) as f32 });
                                }
                            }
                        }
                    }
                    if let Some(s0) = start.filter(|&s0| last - s0 >= min_len) {
                        found.push(Line { a: a as f32, s: slope, from: s0 as f32, to: last as f32, open: false, hits, span: (last - s0) as f32 });
                    }
                }
            }
            found
        };
        let mut found: Vec<Line> = std::thread::scope(|sc| {
            let jobs: Vec<_> = leans.chunks(chunk).map(|part| sc.spawn(move || runs(part))).collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap()).collect()
        });
        // The same border shows up at neighboring offsets and leans: keep the one that follows it
        // most closely (most samples on evidence).
        // A few pixels of slack across the line let slightly slanted lines through the same
        // evidence tie with the true one: borders are far more often straight, so ties go to the
        // least slanted.
        found.sort_by(|p, q| q.hits.cmp(&p.hits).then_with(|| p.s.abs().total_cmp(&q.s.abs())));
        let mut kept: Vec<Line> = Vec::new();
        for l in found {
            let mid = (l.from + l.to) / 2.0;
            let same = kept.iter().any(|k| {
                let shared = k.to.min(l.to) - k.from.max(l.from);
                shared > 0.5 * (l.to - l.from) && (k.at(mid) - l.at(mid)).abs() <= 5.0
            });
            if same {
                continue;
            }
            // A border interrupted for a long stretch (a figure running into it) is found as
            // pieces lying exactly in line: join them across the gap.
            if let Some(k) = kept.iter_mut().find(|k| {
                let apart = l.from.max(k.from) - l.to.min(k.to);
                apart > 0.0 && apart < 0.5 * len as f32 && (k.at(l.from) - l.at(l.from)).abs() <= 3.0 && (k.at(l.to) - l.at(l.to)).abs() <= 3.0
            }) {
                k.from = k.from.min(l.from);
                k.to = k.to.max(l.to);
                k.hits += l.hits;
                k.span = k.to - k.from;
                continue;
            }
            kept.push(l);
            if kept.len() >= MAX_LINES {
                break;
            }
        }
        // A border running on into black (black art against black background) can't be seen, but
        // carries on: extend each line through solid black at both ends.
        for k in kept.iter_mut() {
            let (ka, ks) = (k.a, k.s);
            let dark = |t: f32| -> bool {
                let s = (ka + ks * t).round() as i32;
                let t = t.round() as i32;
                if t < 0 || t >= len as i32 || s < 0 || s >= depth as i32 {
                    return false;
                }
                let i = if across { s as usize * self.w + t as usize } else { t as usize * self.w + s as usize };
                self.px[i] as i16 <= BLACK
            };
            // Not far, though: on a dark page any stroke would run on across it.
            let (from, to) = (k.from - k.span / 2.0, k.to + k.span / 2.0);
            while k.from > from.max(0.0) && dark(k.from - 1.0) {
                k.from -= 1.0;
            }
            while k.to < to.min(len as f32 - 1.0) && dark(k.to + 1.0) {
                k.to += 1.0;
            }
        }
        // The page's own edges aren't offered as sides: a free side lets a frame run past a real
        // border a few pixels in, and art bleeding off the page around the frames is picked up as
        // panels of its own afterward (see `rest`).
        let _ = depth;
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
    /// Its place among the candidates, to tell frames apart.
    id: usize,
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
        let edges = sides.iter().filter(|s| s.open).count();
        Frame { corners, sides, area, edges, box_, id: 0 }
    }

    /// An open side (see `Line::open`) must run between where the borders beside it end.
    fn closed_at_ends(&self) -> bool {
        (0..4).filter(|&i| self.sides[i].open).all(|i| {
            let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
            let ends = |l: &Line, c: (f32, f32)| {
                // Beside a side across the page run borders down it, whose ends are y values.
                let t = if i % 2 == 0 { c.1 } else { c.0 };
                (l.from - t).abs() <= END + 2.0 || (l.to - t).abs() <= END + 2.0
            };
            ends(&self.sides[(i + 3) % 4], p) && ends(&self.sides[(i + 1) % 4], q)
        })
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

    /// Whether most of the inside is one flat tone (a black mass, a colored shape): a panel holds
    /// a picture.
    fn flat(&self, ev: &Evidence) -> bool {
        let (x0, y0, x1, y1) = self.box_;
        let mut bins = [0usize; 16];
        let mut all = 0;
        for i in 0..20 {
            for j in 0..20 {
                let pt = (x0 + (x1 - x0) * (i as f32 + 0.5) / 20.0, y0 + (y1 - y0) * (j as f32 + 0.5) / 20.0);
                if self.contains(pt) && (pt.0 as usize) < ev.w && (pt.1 as usize) < ev.h {
                    bins[ev.px[pt.1 as usize * ev.w + pt.0 as usize] as usize / 16] += 1;
                    all += 1;
                }
            }
        }
        // Two neighboring bins: a tone near a bin's edge splits between them.
        let most = (0..15).map(|b| bins[b] + bins[b + 1]).max().unwrap_or(0);
        most as f32 > FLAT * all as f32
    }

    /// The least dense of its drawn sides (see `Line::dense`).
    fn weakest(&self) -> f32 {
        self.sides.iter().filter(|l| !l.open).map(|l| l.dense()).fold(1.0, f32::min)
    }

    /// How many sides have the page's light paper (tone `bg`, or white) just outside along most
    /// of them.
    fn paper_sides(&self, ev: &Evidence, bg: i16) -> usize {
        (0..4)
            .filter(|&i| {
                if self.sides[i].open {
                    return false;
                }
                let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let (nx, ny) = (dy / len, -dx / len);
                let n = (len / 3.0).max(1.0) as usize;
                let paper = (1..n)
                    .filter(|&k| {
                        let f = k as f32 / n as f32;
                        let (x, y) = (p.0 + dx * f + nx * (OUT_FROM + OUT_TO) / 2.0, p.1 + dy * f + ny * (OUT_FROM + OUT_TO) / 2.0);
                        x >= 0.0 && y >= 0.0 && (x as usize) < ev.w && (y as usize) < ev.h && {
                            let v = ev.px[y as usize * ev.w + x as usize] as i16;
                            v >= 230 || bg > 0 && (v - bg).abs() <= GUTTER_TONE
                        }
                    })
                    .count();
                paper * 10 >= 6 * n.saturating_sub(1).max(1)
            })
            .count()
    }

    /// Whether a gutter of the page's light gutter color `bg` runs right across this frame: a box
    /// round several panels, or picked out of art laid over them, never a panel itself.
    fn crosses_gutter(&self, ev: &Evidence, bg: i16) -> bool {
        if bg < LIGHT_GUTTER {
            return false;
        }
        let (x0, y0, x1, y1) = self.box_;
        let like = |x: f32, y: f32| {
            let (x, y) = (x as usize, y as usize);
            x < ev.w && y < ev.h && (ev.px[y * ev.w + x] as i16 - bg).abs() <= GUTTER_TONE
        };
        // Rows across (and columns down), kept a few pixels inside the sides, from end to end.
        for across in [true, false] {
            let (lo, hi) = if across { (y0, y1) } else { (x0, x1) };
            let mut u = lo + 8.0;
            while u < hi - 8.0 {
                let (a, b) = if across {
                    (self.sides[3].at(u) + 6.0, self.sides[1].at(u) - 6.0)
                } else {
                    (self.sides[0].at(u) + 6.0, self.sides[2].at(u) - 6.0)
                };
                if b - a >= 30.0 {
                    let n = ((b - a) / 2.0) as usize;
                    let hits = (0..=n).filter(|&k| {
                        let t = a + (b - a) * k as f32 / n as f32;
                        if across { like(t, u) } else { like(u, t) }
                    });
                    if hits.count() * 100 >= 85 * (n + 1) {
                        return true;
                    }
                }
                u += 3.0;
            }
        }
        false
    }

    /// Share of this frame lying in `o`, by the frames' own shapes (their boxes overlap more
    /// when slanted).
    fn share_in(&self, o: &Frame) -> f32 {
        let (x0, y0, x1, y1) = self.box_;
        let (mut mine, mut both) = (0, 0);
        for i in 0..12 {
            for j in 0..12 {
                let pt = (x0 + (x1 - x0) * (i as f32 + 0.5) / 12.0, y0 + (y1 - y0) * (j as f32 + 0.5) / 12.0);
                if self.contains(pt) {
                    mine += 1;
                    both += o.contains(pt) as usize;
                }
            }
        }
        both as f32 / mine.max(1) as f32
    }

    fn inside(&self, o: &Frame, slack: f32) -> bool {
        let (a, b) = (self.box_, o.box_);
        a.0 >= b.0 - slack && a.1 >= b.1 - slack && a.2 <= b.2 + slack && a.3 <= b.3 + slack
    }

    /// The least-drawn side's drawn share, counting what lies under `over` (frames already taken,
    /// which may be laid across this one) as drawn.
    fn drawn(&self, ev: &Evidence, over: &[&Frame]) -> f32 {
        // One side may be wholly unseen (black art, a black border and black background in a row,
        // the way a strip's far side goes over a dark page), as long as nothing crosses it; the
        // other three must be drawn.
        let sides = self.side_scores(ev, over);
        let unseen = self.unseen_sides(ev);
        let mut worst = 1.0f32;
        let mut spared = false;
        for i in 0..4 {
            if sides[i] < DRAWN && unseen[i] && !spared {
                spared = true;
                continue;
            }
            worst = worst.min(sides[i]);
        }
        worst
    }

    /// Sides almost entirely unseen (see `Evidence::unseen`) with next to nothing crossing them.
    fn unseen_sides(&self, ev: &Evidence) -> [bool; 4] {
        [0, 1, 2, 3].map(|i| {
            if self.sides[i].open {
                return false;
            }
            let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
            let across = i % 2 == 0;
            let n = (((q.0 - p.0).abs().max((q.1 - p.1).abs())) / 2.0).max(1.0) as usize;
            let (mut dark, mut all) = (0usize, 0usize);
            for k in 0..=n {
                let f = k as f32 / n as f32;
                let pt = (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f);
                all += 1;
                dark += (ev.near(across, pt.0, pt.1) || ev.unseen(across, pt.0, pt.1)) as usize;
            }
            dark as f32 >= 0.9 * all as f32
        })
    }

    /// Each side's drawn share (see `drawn`), clockwise from the top.
    fn side_scores(&self, ev: &Evidence, over: &[&Frame]) -> [f32; 4] {
        [0, 1, 2, 3].map(|i| {
                if self.sides[i].open {
                    return 1.0;
                }
                let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
                let across = i % 2 == 0;
                let n = (((q.0 - p.0).abs().max((q.1 - p.1).abs())) / 2.0).max(1.0) as usize;
                // The ends of a side meet the neighboring sides' borders: skip a few pixels there.
                let (skip, mut on, mut off, mut all) = (3.min(n / 4), 0usize, 0usize, 0usize);
                for k in skip..=n - skip {
                    let f = k as f32 / n as f32;
                    let pt = (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f);
                    all += 1;
                    if ev.near(across, pt.0, pt.1) || over.iter().any(|o| o.contains(pt)) {
                        on += 1;
                    } else if !ev.unseen(across, pt.0, pt.1) {
                        off += 1;
                    }
                }
                if (on as f32) < SEEN * all.max(1) as f32 {
                    return on as f32 / all.max(1) as f32;
                }
                on as f32 / (on + off).max(1) as f32
        })
    }

    /// How many sides have gutter-like ground just outside them (see `OUT_FROM`).
    fn sides_on_ground(&self, ev: &Evidence, over: &[&Frame]) -> usize {
        // A frame mostly bounded by the page edge (art bleeding off two sides or more) has little
        // to show it's a panel: its other sides need real gutter past them, not just different art.
        let strict = self.edges >= 2;
        self.ground(ev, over, strict).iter().filter(|&&g| g >= OUT_SHARE).count()
    }

    /// Each side's share with gutter-like ground (or, unless `strict`, a different picture) just
    /// outside it, clockwise from the top.
    fn ground(&self, ev: &Evidence, over: &[&Frame], strict: bool) -> [f32; 4] {
        [0, 1, 2, 3].map(|i| {
                if self.sides[i].open {
                    return 1.0;
                }
                let (p, q) = (self.corners[i], self.corners[(i + 1) % 4]);
                let (dx, dy) = (q.0 - p.0, q.1 - p.1);
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                // Outward: corners run clockwise on screen, so out is to the left of p→q.
                let (nx, ny) = (dy / len, -dx / len);
                let n = (len / 3.0).max(1.0) as usize;
                let (mut good, mut all) = (0usize, 0usize);
                for k in 1..n {
                    let f = k as f32 / n as f32;
                    let (x, y) = (p.0 + dx * f, p.1 + dy * f);
                    all += 1;
                    let outs = [OUT_FROM, (OUT_FROM + OUT_TO) / 2.0, OUT_TO];
                    let at = |d: f32| -> Option<i16> {
                        let (ox, oy) = (x + nx * d, y + ny * d);
                        (ox >= 0.0 && oy >= 0.0 && ox < ev.w as f32 && oy < ev.h as f32).then(|| ev.px[oy as usize * ev.w + ox as usize] as i16)
                    };
                    let hidden = outs.iter().any(|&d| over.iter().any(|o| o.contains((x + nx * d, y + ny * d))));
                    let ground = hidden
                        || outs.iter().all(|&d| at(d).is_none_or(|v| v >= GROUND_LIGHT || v <= BLACK))
                        || !strict && {
                            let mean = |sign: f32| {
                                let v: Vec<i16> = outs.iter().filter_map(|&d| at(sign * d)).collect();
                                (!v.is_empty()).then(|| v.iter().map(|&x| x as f32).sum::<f32>() / v.len() as f32)
                            };
                            matches!((mean(1.0), mean(-1.0)), (Some(o), Some(i)) if (o - i).abs() >= CHANGE)
                        };
                    good += ground as usize;
                }
                good as f32 / all.max(1) as f32
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

/// Pairs of frames lying across each other, neither inside the other: panels sit side by side or
/// one inside another, and may lie under one other, while boxes picked out of art cross every way.
fn crossings(frames: &[Frame]) -> usize {
    let mut n = 0;
    for (i, a) in frames.iter().enumerate() {
        for b in &frames[i + 1..] {
            let (small, big) = if a.area < b.area { (a, b) } else { (b, a) };
            n += (small.share_in(big) > 0.1 && small.share_in(big) < 0.9) as usize;
        }
    }
    n
}

/// Whether `inner` are pieces of `outer`: each spans it across (or down) to within a few pixels, and
/// side by side they touch or overlap, with no gutter between.
fn slices(inner: &[&Frame], outer: &Frame) -> bool {
    const SLACK: f32 = 20.0;
    const GUTTER: f32 = 4.0;
    let o = outer.box_;
    let tall = inner.iter().all(|t| (t.box_.1 - o.1).abs() <= SLACK && (t.box_.3 - o.3).abs() <= SLACK);
    let wide = inner.iter().all(|t| (t.box_.0 - o.0).abs() <= SLACK && (t.box_.2 - o.2).abs() <= SLACK);
    if !tall && !wide {
        return false;
    }
    // Where each piece starts and ends, measured on its sides at both ends of the cut and in the
    // middle (the boxes of slanted pieces overlap even with a gutter between them, and two
    // borders a gutter apart may still cross somewhere).
    let (lo, hi) = if tall { (o.1, o.3) } else { (o.0, o.2) };
    let at = [lo + SLACK, (lo + hi) / 2.0, hi - SLACK];
    let mut pieces: Vec<&Frame> = inner.to_vec();
    let start = |t: &Frame, u: f32| if tall { t.sides[3].at(u) } else { t.sides[0].at(u) };
    let end = |t: &Frame, u: f32| if tall { t.sides[1].at(u) } else { t.sides[2].at(u) };
    pieces.sort_by(|a, b| start(a, at[1]).total_cmp(&start(b, at[1])));
    pieces.windows(2).all(|p| at.iter().all(|&u| start(p[1], u) - end(p[0], u) <= GUTTER))
}

/// Frames whose four sides lie on found lines and are drawn at least roughly, from the lines'
/// extents alone (cheap); `drawn` measures them properly later.
fn candidates(across: &[Line], down: &[Line], w: f32, h: f32) -> Vec<Frame> {
    let page = w * h;
    let mut out = Vec::new();
    for (i, t) in across.iter().enumerate() {
        for b in &across[i + 1..] {
            let (top, bottom) = if t.at(w / 2.0) < b.at(w / 2.0) { (*t, *b) } else { (*b, *t) };
            if bottom.at(w / 2.0) - top.at(w / 2.0) < 0.05 * h || (top.open && bottom.open) {
                continue;
            }
            let (ox0, ox1) = (top.from.max(bottom.from), top.to.min(bottom.to));
            if ox1 - ox0 < 0.08 * w && !(top.open || bottom.open) {
                continue;
            }
            let (y_lo, y_hi) = (top.at(w / 2.0), bottom.at(w / 2.0));
            let sides: Vec<&Line> = down
                .iter()
                .filter(|v| v.covers(y_lo, y_hi) >= ROUGH)
                .collect();
            for (j, l) in sides.iter().enumerate() {
                for r in &sides[j + 1..] {
                    let (left, right) = if l.at(y_lo) < r.at(y_lo) { (**l, **r) } else { (**r, **l) };
                    if [top, right, bottom, left].iter().filter(|l| l.open).count() > 1 {
                        continue;
                    }
                    let (x_lo, x_hi) = (left.at((y_lo + y_hi) / 2.0), right.at((y_lo + y_hi) / 2.0));
                    if x_hi - x_lo < 0.08 * w {
                        continue;
                    }
                    if top.covers(x_lo, x_hi) < ROUGH || bottom.covers(x_lo, x_hi) < ROUGH {
                        continue;
                    }
                    let four = [top, right, bottom, left];
                    if four.iter().filter(|l| l.s.abs() <= STRAIGHT).count() < STRAIGHT_SIDES
                        || four.iter().filter(|l| l.open || l.dense() >= DENSE).count() < if four.iter().any(|l| l.open) { 4 } else { DENSE_SIDES }
                    {
                        continue;
                    }
                    let f = Frame::new(top, right, bottom, left);
                    if f.edges == 1 && !f.closed_at_ends() {
                        continue;
                    }
                    if (MIN_FRAME..=MAX_FRAME).contains(&(f.area / page)) {
                        out.push(f);
                    }
                }
            }
        }
    }
    out
}

/// Gutters at least this light are told from art inside a frame (see `Frame::crosses_gutter`),
/// by being within this much of the gutter's tone.
const LIGHT_GUTTER: i16 = 150;
const GUTTER_TONE: i16 = 14;

/// The page's most common light tone, taken for its gutters' (a scan's dark rim around the page
/// may hide them from the page edge), or 0 when there's too little of any to be paper.
fn light_tone(img: &GrayImage) -> i16 {
    let mut count = [0usize; 256];
    for &v in img.as_raw() {
        count[v as usize] += 1;
    }
    // In bands a few tones wide: paper is near even, not exactly.
    let band = |v: usize| count[v.saturating_sub(3)..=(v + 3).min(255)].iter().sum::<usize>();
    let best = (LIGHT_GUTTER as usize..=255).max_by_key(|&v| band(v)).unwrap();
    if band(best) * 100 >= 3 * img.as_raw().len() { best as i16 } else { 0 }
}

/// Two borders closed by an open side may end this far apart (in work pixels): the art hiding the
/// side may hide the end of one of them too.
const END: f32 = 30.0;

/// Open sides (see `Line::open`) closing pairs of straight, dense borders that end near level, as lines
/// running the other way: across the page for `down` borders, down it for `across` ones. `len` is
/// the page side the borders run along, `other` the one the closings run along.
fn closings(lines: &[Line], len: f32, other: f32) -> Vec<Line> {
    let solid: Vec<&Line> = lines.iter().filter(|l| !l.open && l.s.abs() <= STRAIGHT && l.dense() >= DENSE).collect();
    let mut out = Vec::new();
    for (i, l) in solid.iter().enumerate() {
        for r in &solid[i + 1..] {
            for (el, er, last) in [(l.from, r.from, false), (l.to, r.to, true)] {
                // A border ending at the page edge just runs off it.
                if (el - er).abs() > END || el.min(er) < 0.01 * len || el.max(er) > 0.99 * len {
                    continue;
                }
                // Closed where the longer one ends, square to them.
                let e = if last { el.max(er) } else { el.min(er) };
                let (pl, pr) = ((l.at(e), e), (r.at(e), e));
                let (p, q) = if pl.0 < pr.0 { (pl, pr) } else { (pr, pl) };
                if q.0 - p.0 < 0.08 * other {
                    continue;
                }
                let s = (q.1 - p.1) / (q.0 - p.0);
                out.push(Line { a: p.1 - s * p.0, s, from: p.0, to: q.0, open: true, hits: 0, span: q.0 - p.0 });
            }
        }
    }
    out.sort_by(|p, q| (q.to - q.from).total_cmp(&(p.to - p.from)));
    out.truncate(MAX_LINES / 4);
    out
}

/// Lines across and down the page, with the open sides closing each direction's borders added to
/// the other.
fn all_lines(ev: &Evidence, w: f32, h: f32) -> (Vec<Line>, Vec<Line>) {
    let (mut across, mut down) = (ev.lines(true), ev.lines(false));
    let (shut_down, shut_across) = (closings(&down, h, w), closings(&across, w, h));
    across.extend(shut_down);
    down.extend(shut_across);
    (across, down)
}

/// The frames on a page and the art around them, as panels (normalized, unordered); empty when
/// the page doesn't look framed.
///
/// `fine`: the same page at about twice the size, when the scan has the pixels for it. Borders a
/// pixel or two wide don't survive shrinking the page, so when the page at work size shows few
/// lines, its borders are read off this instead (see `Evidence::new_fine`). Not otherwise: on a
/// page full of lines already, the thin ones it adds are hatching, and only slow the search.
pub fn find(img: &GrayImage, fine: &dyn Fn() -> Option<GrayImage>, log: &mut dyn FnMut(String)) -> Vec<Panel> {
    let bg = light_tone(img);
    let mut first = usize::MAX;
    for (k, band) in BANDS.into_iter().enumerate() {
        let (least, sure) = if k == 0 { (MIN_FRAMED, 0.0) } else { (FALLBACK_FRAMED, FALLBACK_DENSE) };
        let (found, lines) = find_at(img, &Evidence::new(img, band), bg, least, sure, log);
        if !found.is_empty() {
            log(format!("frames: answered with {} panels", found.len()));
            return found;
        }
        first = first.min(lines);
        // Dark colored borders leave black ones hardly any lines to show; plenty of lines that
        // made no frames are art, and dark colors would only add to it.
        if lines > FEW_LINES {
            break;
        }
    }
    let Some(fine) = (first < FINE_LINES).then(fine).flatten() else { return Vec::new() };
    log("frames: twice the size".into());
    let (found, _) = find_at(img, &Evidence::new_fine(img, &fine, BANDS[0]), bg, MIN_FRAMED, 0.0, log);
    if !found.is_empty() {
        log(format!("frames: answered with {} panels", found.len()));
    }
    found
}

/// `least`: the share of the page the frames must cover; `sure`: how dense every frame's sides
/// must be (see `Frame::weakest`).
fn find_at(img: &GrayImage, ev: &Evidence, bg: i16, least: f32, sure: f32, log: &mut dyn FnMut(String)) -> (Vec<Panel>, usize) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let (across, down) = all_lines(ev, w, h);
    let lines = across.len() + down.len();
    let mut cands = candidates(&across, &down, w, h);
    for (k, f) in cands.iter_mut().enumerate() {
        f.id = k;
    }
    log(format!("frames: {} across, {} down, {} candidates", across.len(), down.len(), cands.len()));
    if cands.len() > MAX_CANDIDATES {
        return (Vec::new(), lines);
    }
    // Smallest first: a box around a whole row (or the page) gives way to the panels in it.
    cands.sort_by(|a, b| a.area.total_cmp(&b.area));
    // What can be told of each frame on its own, worked out once and on every core: a gutter or
    // one flat tone inside rules it out; and how much of it is drawn with nothing laid over it.
    let alone: Vec<(Option<&str>, f32)> = {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
        let chunk = cands.len().div_ceil(threads).max(1);
        let judge = |f: &Frame| {
            let out = if f.crosses_gutter(ev, bg) {
                Some("gutter inside")
            } else if f.flat(ev) {
                Some("flat inside")
            } else {
                None
            };
            (out, if out.is_none() { f.drawn(ev, &[]) } else { 0.0 })
        };
        std::thread::scope(|sc| {
            let jobs: Vec<_> = cands.chunks(chunk).map(|part| sc.spawn(move || part.iter().map(judge).collect::<Vec<_>>())).collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap()).collect()
        })
    };
    let mut taken: Vec<Frame> = Vec::new();
    // Twice: frames drawn all round first, then ones partly hidden under those.
    // AION_TRACE=x,y (fractions, tests): say why frames with their top-left corner there are left out.
    let trace: Option<(f32, f32)> = std::env::var("AION_TRACE").ok().filter(|_| cfg!(test)).and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some((x.parse().ok()?, y.parse().ok()?))
    });
    for hidden in [false, true] {
        for (f, &(ruled_out, drawn_alone)) in cands.iter().zip(&alone) {
            let traced = trace.is_some_and(|(x, y)| (f.box_.0 / w - x).abs() < 0.015 && (f.box_.1 / h - y).abs() < 0.015);
            let mut why = |m: &str| {
                if traced {
                    log(format!("trace {:.2},{:.2} {:.2}x{:.2} e{} hidden {hidden}: {m}", f.box_.0 / w, f.box_.1 / h, (f.box_.2 - f.box_.0) / w, (f.box_.3 - f.box_.1) / h, f.edges));
                }
            };
            if taken.iter().any(|t| t.inside(f, 8.0) && f.inside(t, 8.0)) {
                why("same as one taken");
                continue; // the same frame again
            }
            if let Some(m) = ruled_out {
                why(m);
                continue;
            }
            let drawn = if hidden { f.drawn(ev, &taken.iter().collect::<Vec<_>>()) } else { drawn_alone };
            if drawn < DRAWN {
                why("not drawn");
                continue;
            }
            let others: Vec<&Frame> = taken.iter().collect();
            if f.sides_on_ground(ev, &others) < OUT_SIDES {
                why("no ground");
                continue;
            }
            // Frames inside this one: none (a panel), or one set clear of its edges (an inset).
            let inner: Vec<&Frame> = taken.iter().filter(|t| t.inside(f, 6.0)).collect();
            let clear = |t: &Frame| {
                let (a, b) = (t.box_, f.box_);
                [a.0 - b.0, b.2 - a.2, a.1 - b.1, b.3 - a.3].iter().all(|d| *d > 12.0)
            };
            // Several frames inside that meet with no gutter between them, together spanning this
            // one, are pieces of it cut along straight lines in its art (a door, a figure's edge):
            // this one replaces them. Panels side by side always have a gutter between.
            let pieces = inner.len() > 1 && slices(&inner, f);
            if pieces {
                let gone: Vec<usize> = inner.iter().map(|t| t.id).collect();
                taken.retain(|t| !gone.contains(&t.id));
            } else if inner.len() > 1 || inner.first().is_some_and(|t| !clear(t)) {
                why("frames inside");
                continue;
            }
            // Otherwise frames may only overlap a little; except that, the second time round, a
            // panel may lie under one other (and most of it must still show).
            let big: Vec<f32> = taken
                .iter()
                .filter(|t| !t.inside(f, 6.0))
                .map(|t| t.overlap(f))
                .filter(|&o| o > 0.0)
                .zip(taken.iter().filter(|t| !t.inside(f, 6.0)).filter(|t| t.overlap(f) > 0.0))
                .filter(|(o, t)| *o > MAX_OVERLAP * t.area.min(f.area))
                .map(|(o, _)| o)
                .collect();
            let clash = match big.as_slice() {
                [] => false,
                [o] => !hidden || *o >= 0.5 * f.area,
                _ => true,
            };
            if !clash {
                why(if pieces { "taken, replacing pieces" } else { "taken" });
                taken.push(f.clone());
            } else {
                why("clash");
            }
        }
    }
    let page = w * h;
    let framed: f32 = taken.iter().map(|f| f.area).sum::<f32>() / page;
    let drawn_frames = taken.iter().filter(|f| f.edges == 0).count();
    let crossings = crossings(&taken);
    log(format!(
        "frames: {} taken ({} drawn all round, {} crossing, paper {:?}), {:.2} of the page: {:?}",
        taken.len(),
        drawn_frames,
        crossings,
        taken.iter().map(|f| (f.paper_sides(ev, bg), (f.weakest() * 100.0) as u32)).collect::<Vec<_>>(),
        framed,
        taken.iter().map(|f| format!("{:.2},{:.2} {:.2}x{:.2} e{}", f.box_.0 / w, f.box_.1 / h, (f.box_.2 - f.box_.0) / w, (f.box_.3 - f.box_.1) / h, f.edges)).collect::<Vec<_>>()
    ));
    // Art seen as frames comes anywhere; real ones are laid out on the page, some along its paper.
    // Or, laid over full-bleed art, they are drawn cleanly all round.
    let on_paper = taken.iter().any(|f| f.paper_sides(ev, bg) >= 2) || taken.iter().all(|f| f.weakest() >= CLEAN);
    if !on_paper || taken.iter().any(|f| f.weakest() < sure) || taken.len() < 2 || taken.len() > MAX_FRAMES || crossings > MAX_CROSSINGS || drawn_frames == 0 || !(least..=MAX_FRAMED).contains(&framed) {
        return (Vec::new(), lines);
    }
    let mut out: Vec<Panel> = taken.iter().map(|f| f.panel(w, h)).collect();
    out.extend(rest(img, &taken));
    (out, lines)
}

/// For tuning: the page dimmed, border evidence across in red and down in green, the lines found
/// in blue, and the best few candidates' scores printed.
#[cfg(test)]
pub fn debug(img: &GrayImage) -> (image::RgbImage, Vec<String>) {
    let bg = light_tone(img);
    let (w, h) = (img.width() as f32, img.height() as f32);
    // AION_BAND=n: read the evidence at that darkness (default the first of `BANDS`).
    let band = std::env::var("AION_BAND").ok().and_then(|v| v.parse().ok()).unwrap_or(BANDS[0]);
    let ev = Evidence::new(img, band);
    let mut out = image::RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let i = (y * img.width() + x) as usize;
        let v = img.as_raw()[i] / 3;
        image::Rgb([if ev.across[i] { 255 } else { v }, if ev.down[i] { 255 } else { v }, v])
    });
    // AION_COL=x: which rows of that column (±2) hold evidence for lines running down.
    if let Some(cx) = std::env::var("AION_COL").ok().and_then(|v| v.parse::<usize>().ok()) {
        let rows: String = (0..ev.h).map(|y| if (cx.saturating_sub(2)..=(cx + 2).min(ev.w - 1)).any(|x| ev.down[y * ev.w + x]) { '#' } else { '.' }).collect();
        eprintln!("col {cx} (w {}): {}", ev.w, rows);
    }
    let (across, down) = all_lines(&ev, w, h);
    for (lines, is_across) in [(&across, true), (&down, false)] {
        for l in lines.iter().filter(|l| !l.open) {
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
    // AION_COLRAW=x,y0,y1 (fractions): the raw column of brightness there.
    if let Some(v) = std::env::var("AION_COLRAW").ok() {
        let f: Vec<f32> = v.split(',').filter_map(|t| t.parse().ok()).collect();
        let (x, y0, y1) = ((f[0] * w) as usize, (f[1] * h) as usize, (f[2] * h) as usize);
        let col: Vec<u8> = (y0..y1).map(|y| img.as_raw()[y * img.width() as usize + x]).collect();
        eprintln!("column x={x} from y={y0}: {col:?}");
    }
    // AION_PROFILE=y-fraction: the raw row of brightness there, from the left edge in.
    if let Some(fy) = std::env::var("AION_PROFILE").ok().and_then(|v| v.parse::<f32>().ok()) {
        let y = (fy * h) as usize;
        let iw = img.width() as usize;
        let row: Vec<u8> = (0..60).map(|x| img.as_raw()[y * iw + x]).collect();
        let tail: Vec<u8> = (iw - 60..iw).map(|x| img.as_raw()[y * iw + x]).collect();
        eprintln!("profile y={y}: {row:?}
  right end: {tail:?}");
    }
    if std::env::var("AION_LINES").is_ok() {
        for (lines, name, len) in [(&across, "across", w), (&down, "down", h)] {
            for l in lines.iter().filter(|l| !l.open) {
                let mid = (l.from + l.to) / 2.0;
                eprintln!("{name} at {:.3} lean {:.3} from {:.2} to {:.2}", l.at(mid) / if name == "across" { h } else { w }, l.s, l.from / len, l.to / len);
            }
        }
    }
    let mut cands = candidates(&across, &down, w, h);
    let mut scored: Vec<(f32, String)> = cands
        .drain(..)
        .map(|f| {
            let sides = f.side_scores(&ev, &[]);
            let gr = f.ground(&ev, &[], f.edges >= 2);
            let s = if f.crosses_gutter(&ev, bg) { 0.0 } else { sides.iter().copied().fold(1.0, f32::min) };
            (s, format!("{s:.2} {:.2?} g{:.2?} {:.2},{:.2} {:.2}x{:.2} e{} lean {:.3?} dens {:.2?}", sides, gr, f.box_.0 / w, f.box_.1 / h, (f.box_.2 - f.box_.0) / w, (f.box_.3 - f.box_.1) / h, f.edges, f.sides.map(|l| l.s), f.sides.map(|l| l.dense())))
        })
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    // AION_WIDE=1: only wide, short candidates (strips).
    if std::env::var("AION_WIDE").is_ok() {
        scored.retain(|s| std::env::var("AION_WIDE").unwrap() == "0" || s.1.split(" ").nth(10).and_then(|wh| wh.split('x').next()?.parse::<f32>().ok()).is_some_and(|w| w > 0.7));
    }
    let keep = if std::env::var("AION_WIDE").is_ok() { 400 } else { 12 };
    (out, scored.into_iter().take(keep).map(|s| s.1).collect())
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
                    img.put_pixel(x, y, if edge { Luma([10]) } else { Luma([art(x, y)[0] + 80]) });
                }
            }
        };
        frame(30, 300, 570, 430);
        frame(30, 470, 570, 600);
        let found = find(&img, &|| None, &mut |_| {});
        let strips = found.iter().filter(|p| p.w > 0.8 && p.h < 0.2).count();
        assert_eq!(strips, 2, "{found:?}");
        assert!(found.iter().any(|p| p.y < 0.05 && p.h > 0.2), "the page above: {found:?}");
        assert!(found.iter().any(|p| p.y > 0.6 && p.h > 0.2), "the page below: {found:?}");
    }

    /// A slanted panel laid across the tops of two panels below it, on white paper.
    #[test]
    fn finds_overlapping_panels() {
        let mut img = GrayImage::from_pixel(600, 900, Luma([250]));
        // Each panel its own picture: a lighter or darker version of the same art.
        let border = |img: &mut GrayImage, x0: u32, y0: u32, x1: u32, y1: u32, lift: u8| {
            for y in y0..y1 {
                for x in x0..x1 {
                    let edge = x < x0 + 3 || x >= x1 - 3 || y < y0 + 3 || y >= y1 - 3;
                    img.put_pixel(x, y, if edge { Luma([15]) } else { Luma([art(x, y)[0] + lift]) });
                }
            }
        };
        border(&mut img, 30, 30, 570, 300, 0); // top panel
        border(&mut img, 30, 380, 290, 870, 0); // bottom left
        border(&mut img, 310, 380, 570, 870, 0); // bottom right
        border(&mut img, 80, 320, 520, 520, 50); // laid across the tops of both
        let found = find(&img, &|| None, &mut |_| {});
        assert!(found.len() >= 4, "{found:?}");
        let near = |x: f32, y: f32| found.iter().any(|p| (p.x - x).abs() < 0.04 && (p.y - y).abs() < 0.04);
        assert!(near(0.05, 0.033) && near(0.133, 0.355), "{found:?}");
        assert!(near(0.05, 0.42) || near(0.05, 0.58), "bottom left, partly hidden: {found:?}");
    }
}
