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

/** How a series is read: page fit, scrolling, double page (and which way its spreads go) and
 *  Guided View. Each series keeps the way it was last read; one not read yet starts from the
 *  settings above, which only Settings changes. */
export type View = { fit: Fit; scroll: boolean; double: boolean; pairRtl: boolean; guided: boolean };

const VIEWS_KEY = "aion.series-view";
function loadViews(): Record<number, View> {
  try {
    return JSON.parse(localStorage.getItem(VIEWS_KEY) ?? "{}");
  } catch {
    return {};
  }
}
/** Series that keep a view of their own. */
export const views = $state<Record<number, View>>(loadViews());

/** The view a book opens in: its series' own, else the settings. Books from outside the library
 *  (no series) always start from the settings. */
export function viewFor(seriesId: number | null, manga: boolean): View {
  const kept = seriesId ? views[seriesId] : undefined;
  if (kept) return { ...kept };
  return { fit: prefs.fit, scroll: prefs.scroll, double: prefs.double, pairRtl: manga, guided: manga ? prefs.guidedManga : prefs.guidedComics };
}

export function keepView(seriesId: number, v: View) {
  views[seriesId] = { ...v };
  saveViews();
}

/** Every series back to the settings. */
export function forgetViews() {
  for (const k of Object.keys(views)) delete views[+k];
  saveViews();
}

function saveViews() {
  try {
    localStorage.setItem(VIEWS_KEY, JSON.stringify(views));
  } catch {}
}

export const GUIDED_MS = { slow: 900, normal: 650, fast: 400 } as const;
export const GUIDED_DIM = { light: 0.75, strong: 0.94, hidden: 1 } as const;
/** How long a click waits to see whether it's a double-click (zoom into a panel). */
export const DOUBLE_CLICK_MS = 230;
