/** A cover's hue in degrees and how much color it has (0 for gray, 1 for pure), or null for no color. */
export function hue(color: string | null | undefined): { h: number; chroma: number } | null {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color ?? "");
  if (!m) return null;
  const [r, g, b] = [m[1], m[2], m[3]].map((h) => parseInt(h, 16) / 255);
  const hi = Math.max(r, g, b);
  const d = hi - Math.min(r, g, b);
  let h = 0;
  if (d) h = hi === r ? ((g - b) / d + 6) % 6 : hi === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return { h: Math.round(h * 60) % 360, chroma: d };
}

/** An HSL color as red, green and blue from 0 to 1 (saturation and lightness from 0 to 1). */
export function rgb(h: number, s: number, l: number): [number, number, number] {
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
  };
  return [f(0), f(8), f(4)];
}

/**
 * A cover's color at an even brightness, as CSS: its hue and (capped) saturation kept, its
 * lightness held between `min` and `max`. A near-black cover would otherwise tint nothing and a
 * bright yellow one would shout. Gray covers stay gray. Null for no color.
 */
export function tone(color: string | null | undefined, min = 0.42, max = 0.55): string | null {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color ?? "");
  if (!m) return null;
  const [r, g, b] = [m[1], m[2], m[3]].map((h) => parseInt(h, 16) / 255);
  const hi = Math.max(r, g, b);
  const lo = Math.min(r, g, b);
  const d = hi - lo;
  let h = 0;
  if (d) h = hi === r ? ((g - b) / d + 6) % 6 : hi === g ? (b - r) / d + 2 : (r - g) / d + 4;
  const l = (hi + lo) / 2;
  const s = d ? d / (1 - Math.abs(2 * l - 1)) : 0;
  return `hsl(${Math.round(h * 60)} ${Math.round(Math.min(s, 0.75) * 100)}% ${Math.round(Math.min(Math.max(l, min), max) * 100)}%)`;
}
