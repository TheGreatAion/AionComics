// Reader preferences that persist between books and launches.

export type Fit = "height" | "width" | "page" | "full";

const DEFAULTS = {
  fit: "page" as Fit,
  /** Continuous vertical scroll (also the webtoon mode). */
  scroll: false,
  double: false,
  /** Guided View is remembered separately for manga and comics. */
  guidedManga: false,
  guidedComics: false,
  /** Page count and clock pinned top-right. */
  info: false,
  background: "black" as "black" | "grey",
  guidedSpeed: "normal" as "slow" | "normal" | "fast",
  guidedDim: "strong" as "light" | "strong",
  /** Guided View shows each page whole before its first panel. */
  guidedOverview: true,
};

export type Prefs = typeof DEFAULTS;

function load(): Prefs {
  try {
    const saved = JSON.parse(localStorage.getItem("aion.reader") ?? "{}");
    // Carry over the settings from the earlier reader.
    if (saved.fit === undefined && localStorage.getItem("aion.fit") === "width") saved.fit = "width";
    return { ...DEFAULTS, ...saved };
  } catch {
    return { ...DEFAULTS };
  }
}

export const prefs = $state<Prefs>(load());

export function savePrefs() {
  try {
    localStorage.setItem("aion.reader", JSON.stringify(prefs));
  } catch {}
}

export const GUIDED_MS = { slow: 900, normal: 650, fast: 400 } as const;
export const GUIDED_DIM = { light: 0.75, strong: 0.94 } as const;
