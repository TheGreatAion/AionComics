// Reader preferences that persist between books and launches: the reader's own changes included.

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
  background: "black" as "black" | "gray",
  guidedSpeed: "normal" as "slow" | "normal" | "fast",
  /** Around the current panel: dimmed, darkened, or hidden altogether. */
  guidedDim: "strong" as "light" | "strong" | "hidden",
  /** Guided View shows each page whole before its first panel. */
  guidedOverview: true,
  /** A panel much taller than the screen is read in parts, top to bottom, closer up. */
  guidedParts: true,
};

export type Prefs = typeof DEFAULTS;

function load(): Prefs {
  try {
    const saved = JSON.parse(localStorage.getItem("aion.reader") ?? "{}");
    // Carry over the settings from the earlier reader.
    if (saved.fit === undefined && localStorage.getItem("aion.fit") === "width") saved.fit = "width";
    // The gray background was once saved under its British spelling.
    if (saved.background === "grey") saved.background = "gray";
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

/** How a book is read: page fit, scrolling, double page (and which way its spreads go) and
 *  Guided View. */
export type View = { fit: Fit; scroll: boolean; double: boolean; pairRtl: boolean; guided: boolean };

/** The view a book opens in: the way you last read, whatever the book (Guided View apart for
 *  manga and comics). A webtoon starts scrolling, the way it's drawn. */
export function viewFor(manga: boolean, webtoon = false): View {
  if (webtoon) return { fit: "width", scroll: true, double: false, pairRtl: false, guided: false };
  return { fit: prefs.fit, scroll: prefs.scroll, double: prefs.double, pairRtl: manga, guided: manga ? prefs.guidedManga : prefs.guidedComics };
}

/** Change the fit, scrolling, double page or Guided View in a book and every book opens that way
 *  from then on, until you change it again. */
export function keepView(v: View, manga: boolean) {
  prefs.fit = v.fit;
  prefs.scroll = v.scroll;
  prefs.double = v.double;
  if (manga) prefs.guidedManga = v.guided;
  else prefs.guidedComics = v.guided;
  savePrefs();
}

// Each series once kept a view of its own; one way of reading for every book replaced that.
try {
  localStorage.removeItem("aion.series-view");
} catch {}

export const GUIDED_MS = { slow: 900, normal: 650, fast: 400 } as const;
export const GUIDED_DIM = { light: 0.75, strong: 0.94, hidden: 1 } as const;
/** How long a click waits to see whether it's a double-click (zoom into a panel). */
export const DOUBLE_CLICK_MS = 230;
