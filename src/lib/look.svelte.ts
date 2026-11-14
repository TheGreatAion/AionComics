// How the app moves, and the cover behind collections' names: choices on the Settings page, kept
// in this window's storage. app.html applies the motion choice before the first paint.

export type Motion = "full" | "quick" | "off";

function load<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const v = localStorage.getItem(key) as T | null;
    return v && allowed.includes(v) ? v : fallback;
  } catch {
    return fallback;
  }
}

// Until you choose, Windows' own "Animation effects" setting decides: off there, off here.
const calm = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

export const look = $state({
  motion: load("aion.motion", ["full", "quick", "off"] as const, calm ? "off" : "quick"),
  backdrop: load("aion.backdrop", ["on", "off"] as const, "on") === "on",
});

function store(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

export function setMotion(m: Motion) {
  look.motion = m;
  document.documentElement.dataset.motion = m;
  store("aion.motion", m);
}

export function setBackdrop(on: boolean) {
  look.backdrop = on;
  store("aion.backdrop", on ? "on" : "off");
}

/** How long a movement takes: `quick` is its normal length, `full` the longer, slower one. */
export const motionMs = (quick: number, full: number) => (look.motion === "off" ? 0 : look.motion === "full" ? full : quick);
