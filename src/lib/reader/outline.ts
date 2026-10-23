// Guided View's spotlight: the page is dimmed everywhere except the current panel's outline.
// Outlines always have the same number of points, so CSS can morph one panel's shape into the next.

import type { Panel } from "$lib/api";

type Pt = [number, number];
const POINTS = 8;

/** Clockwise (on screen) ring of exactly POINTS points, starting nearest the top-left corner. */
function normalise(ring: Pt[]): Pt[] {
  let pts = ring.slice();
  const area = pts.reduce((s, [x, y], i) => {
    const [nx, ny] = pts[(i + 1) % pts.length];
    return s + x * ny - nx * y;
  }, 0);
  if (area < 0) pts.reverse();
  const start = pts.reduce((best, p, i) => (p[0] + p[1] < pts[best][0] + pts[best][1] ? i : best), 0);
  pts = [...pts.slice(start), ...pts.slice(0, start)];
  // Too many corners: keep evenly spaced ones. Too few: repeat corners in place.
  return Array.from({ length: POINTS }, (_, i) => pts[Math.floor((i * pts.length) / POINTS)]);
}

/** The lit area for a panel (or the whole page), grown by `pad` (0–1, per axis) so borders aren't clipped. */
export function outline(p: Panel | null, padX: number, padY: number): Pt[] {
  if (!p) return normalise([[0, 0], [1, 0], [1, 1], [0, 1]]);
  const clamp = (v: number) => Math.max(0, Math.min(1, v));
  if (p.poly && p.poly.length >= 3) {
    const cx = p.poly.reduce((s, q) => s + q[0], 0) / p.poly.length;
    const cy = p.poly.reduce((s, q) => s + q[1], 0) / p.poly.length;
    // Push each corner outward from the middle by the padding.
    return normalise(
      p.poly.map(([x, y]) => [clamp(x + Math.sign(x - cx) * padX), clamp(y + Math.sign(y - cy) * padY)] as Pt),
    );
  }
  const [x0, y0] = [clamp(p.x - padX), clamp(p.y - padY)];
  const [x1, y1] = [clamp(p.x + p.w + padX), clamp(p.y + p.h + padY)];
  return normalise([[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
}

/** A clip-path that keeps everything *outside* the outline: the page rectangle with the outline cut out. */
export function dimClip(pts: Pt[]): string {
  const pc = ([x, y]: Pt) => `${(x * 100).toFixed(3)}% ${(y * 100).toFixed(3)}%`;
  const ring = [...pts, pts[0]].map(pc).join(", ");
  return `polygon(evenodd, 0% 0%, 100% 0%, 100% 100%, 0% 100%, 0% 0%, ${ring}, 0% 0%)`;
}
