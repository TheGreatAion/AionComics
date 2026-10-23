// Keyboard shortcuts: one string per chord, e.g. "ctrl+ArrowLeft", "shift+c", "+", "F1".

/** Turn a key event into a chord string. Shift only counts for letters and named keys,
 *  so "+" and "?" match however the keyboard layout produces them. */
export function chord(e: KeyboardEvent): string {
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  const letter = /^[a-z0-9]$/.test(k);
  const mods = [
    e.ctrlKey || e.metaKey ? "ctrl" : "",
    e.altKey ? "alt" : "",
    e.shiftKey && (letter || e.key.length > 1) ? "shift" : "",
  ].filter(Boolean);
  return [...mods, k === " " ? "Space" : k].join("+");
}

const NAMES: Record<string, string> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Escape: "Esc",
  PageDown: "PgDn",
  PageUp: "PgUp",
  Backspace: "⌫",
};

/** How a chord reads in menus: "Ctrl+←", "Shift+C". */
export function pretty(c: string): string {
  return c
    .split("+")
    .filter((p, i, all) => p || i === all.length - 1)
    .map((p) => (p === "" ? "+" : NAMES[p] ?? (p.length === 1 ? p.toUpperCase() : p[0].toUpperCase() + p.slice(1))))
    .join("+");
}

/** User overrides: action id → chords. */
export function loadOverrides(): Record<string, string[]> {
  try {
    return JSON.parse(localStorage.getItem("aion.keys") ?? "{}");
  } catch {
    return {};
  }
}

export function saveOverrides(o: Record<string, string[]>) {
  try {
    localStorage.setItem("aion.keys", JSON.stringify(o));
  } catch {}
}
