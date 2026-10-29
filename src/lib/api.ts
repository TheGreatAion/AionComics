import { invoke } from "@tauri-apps/api/core";

export interface ComicCard {
  id: number;
  label: string;
  title: string | null;
  year: string | null;
  seriesId: number;
  seriesName: string;
  currentPage: number;
  numPages: number | null;
  read: boolean;
  isManga: boolean;
  /** Cover height / width. */
  coverRatio: number | null;
  lastReadAt: number | null;
  /** The color the cover reads as, "#rrggbb", once worked out. */
  coverColor: string | null;
  /** On the series page: the book's format, and the issues a collection holds ("#13–18"). */
  edition?: EditionKind;
  collects?: string;
}

export type EditionKind = "volume" | "issue" | "chapter" | "collection" | "deluxe" | "omnibus" | "annual" | "special";

/** One format of a series ("Volumes", "Issues", "Annuals"…), its books in their subfolders. */
export interface Edition {
  kind: EditionKind;
  name: string;
  sections: { name: string; comics: ComicCard[] }[];
  /** Issue numbers missing from the run, when only a few are. */
  missing: number[];
}

export interface Home {
  continueReading: ComicCard[];
  pickUpAgain: ComicCard[];
}

/** A copy of the library database: reading progress, bookmarks, panel edits and metadata. */
export interface Backup {
  name: string;
  /** Unix seconds. */
  madeAt: number;
  size: number;
}

export interface CollectionSummary {
  id: number;
  name: string;
  isManga: boolean;
  seriesCount: number;
  comicCount: number;
  /** Covers for the fanned stack on Home: the three most recently read series. */
  covers: number[];
}

export interface NavInfo {
  collections: CollectionSummary[];
}

export interface SeriesSummary {
  id: number;
  name: string;
  isManga: boolean;
  count: number;
  readCount: number;
  inProgress: boolean;
  coverId: number | null;
  coverRatio: number | null;
  lastReadAt: number | null;
  lastAddedAt: number | null;
  /** The first book's cover color, "#rrggbb". */
  coverColor: string | null;
}

export interface CollectionView {
  id: number;
  name: string;
  isManga: boolean;
  series: SeriesSummary[];
}

export interface SeriesView {
  id: number;
  name: string;
  collectionId: number;
  collectionName: string;
  isManga: boolean;
  writer: string | null;
  /** Penciller or artist, when known. */
  artist: string | null;
  publisher: string | null;
  years: string | null;
  synopsis: string | null;
  sections: { name: string; comics: ComicCard[] }[];
  editions: Edition[];
  backfill: { beforeId: number; beforeLabel: string; count: number } | null;
  /** The Comic Vine series this one is matched to. */
  cvVolumeId: string | null;
}

/** Books that look like copies of one another, and why. Nothing is ever deleted for you. */
export interface DupGroup {
  key: string;
  reasons: string[];
  books: { comic: ComicCard; relPath: string; size: number | null }[];
}

/** One book's reading so far: every sitting with it. */
export interface BookStats {
  pages: number;
  sittings: number;
  seconds: number;
  firstAt: number | null;
  lastAt: number | null;
}

export interface ReadingPeriod {
  pages: number;
  seconds: number;
  sittings: number;
  finished: number;
}

export interface Ranked {
  name: string;
  pages: number;
  seriesId: number | null;
  coverId: number | null;
}

/** What your reading adds up to, recorded from the first sitting on. */
export interface ReadingStats {
  since: number | null;
  week: ReadingPeriod;
  month: ReadingPeriod;
  year: ReadingPeriod;
  all: ReadingPeriod;
  /** The last 12 weeks, oldest first: [Monday as days since 1970, pages]. */
  weeks: [number, number][];
  streak: number;
  bestStreak: number;
  series: Ranked[];
  creators: Ranked[];
  characters: Ranked[];
  finished: ComicCard[];
}

/** "2 h 14 min", "35 min", "under a minute". */
export function duration(seconds: number): string {
  const m = Math.round(seconds / 60);
  if (m < 1) return "under a minute";
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}

/** Everything by one creator, or with one character or team in it, grouped by series. */
export interface PeopleView {
  name: string;
  kind: "creator" | "character";
  /** What they did and in how many books, most first: ["pencils", 30]. */
  roles: [string, number][];
  books: number;
  series: { id: number; name: string; collectionName: string; comics: ComicCard[] }[];
}

/** Metadata fields a book can carry (names match the database columns). */
export type MetaField =
  | "title" | "number" | "volume" | "year" | "date" | "writer" | "penciller" | "inker" | "colorist"
  | "letterer" | "cover_artist" | "publisher" | "synopsis" | "characters" | "teams" | "locations"
  | "story_arc" | "arc_number" | "arc_count" | "genre" | "cv_id" | "cv_volume_id";

export interface ComicMeta {
  id: number;
  seriesId: number;
  seriesName: string;
  label: string;
  isManga: boolean;
  fileName: string;
  fields: Record<MetaField, string | null>;
  /** Fields you've edited by hand. */
  edited: MetaField[];
  /** Fields that came from AionComics' Comic Vine lookup. */
  fromCv: MetaField[];
}

export interface CvVolume {
  id: string;
  name: string;
  startYear: string | null;
  publisher: string | null;
  count: number;
  image: string | null;
  deck: string | null;
  score: number;
}

export interface CvIssue {
  id: string;
  number: string | null;
  name: string | null;
  coverDate: string | null;
  image: string | null;
}

export interface CvProposal {
  comicId: number;
  label: string;
  section: string;
  issueId: string | null;
  confidence: "sure" | "check" | "none";
}

export interface CvStatus {
  hasKey: boolean;
  queued: number;
  lastError: string | null;
}

export interface ReaderInfo {
  comic: ComicCard;
  pageCount: number;
  startPage: number;
  prevId: number | null;
  next: ComicCard | null;
  /** Bookmarked pages, 0-based. */
  bookmarks: number[];
  /** Opened from outside the library: progress, bookmarks and panel edits aren't kept. */
  external: boolean;
}

/** Normalized (0–1) panel rectangle, in reading order. */
export interface Panel {
  x: number;
  y: number;
  w: number;
  h: number;
  /** The panel's real outline when it isn't a plain rectangle (slanted gutters), clockwise, 0–1. */
  poly?: [number, number][];
}

export interface ScanStats {
  comics: number;
  series: number;
  collections: number;
  added: number;
  removed: number;
  yacMatched: number;
  seconds: number;
  finishedAt: number;
  /** Books that seemed missing but were kept, because too many vanished at once. */
  keptMissing?: number;
}

export interface Settings {
  libraryRoot: string | null;
  lastScan: ScanStats | null;
  scanning: boolean;
}

/** Where a comic file dropped on the window would be copied. */
export type Placement = {
  source: string;
  fileName: string;
  /** Folder inside the comics folder, e.g. "DC/Absolute Batman". */
  folder: string | null;
  status: "add" | "exists" | "name-taken" | "new-series" | "no-place" | "not-comic";
};

export const api = {
  settings: () => invoke<Settings>("get_settings"),
  setLibraryRoot: (path: string) => invoke<void>("set_library_root", { path }),
  rescan: () => invoke<void>("rescan"),
  backups: () => invoke<Backup[]>("backups"),
  backupNow: () => invoke<Backup>("backup_now"),
  /** Backs up today's state, then restarts AionComics with the chosen backup in place. */
  restoreBackup: (name: string) => invoke<void>("restore_backup", { name }),
  home: () => invoke<Home>("home"),
  nav: () => invoke<NavInfo>("nav"),
  search: (q: string) => invoke<SeriesSummary[]>("search", { q }),
  collection: (id: number) => invoke<CollectionView>("collection", { id }),
  series: (id: number) => invoke<SeriesView>("series", { id }),
  people: (kind: "creator" | "character", name: string) => invoke<PeopleView>("people", { kind, name }),
  bookStats: (id: number) => invoke<BookStats>("book_stats", { id }),
  duplicates: () => invoke<DupGroup[]>("duplicates"),
  ignoreDuplicate: (key: string) => invoke<void>("ignore_duplicate", { key }),
  readingStats: () => invoke<ReadingStats>("reading_stats", { tzMinutes: -new Date().getTimezoneOffset() }),
  openComic: (id: number) => invoke<ReaderInfo>("open_comic", { id, screen: screenHeight }),
  pagePanels: (id: number, page: number, rtl: boolean) => invoke<Panel[]>("page_panels", { id, page, rtl }),
  /** Guided View: work out the book's panels in the background, from page `from` on. */
  panelsAhead: (id: number, from: number, rtl: boolean) => invoke<void>("panels_ahead", { id, from, rtl }),
  comicMeta: (id: number) => invoke<ComicMeta>("comic_meta", { id }),
  editComicMeta: (id: number, changes: Partial<Record<MetaField, string | null>>) =>
    invoke<ComicMeta>("edit_comic_meta", { id, changes }),
  cvStatus: () => invoke<CvStatus>("cv_status"),
  cvOpenSite: () => invoke<void>("cv_open_site"),
  setEdition: (id: number, kind: EditionKind | null) => invoke<void>("set_edition", { id, kind }),
  cvSetKey: (key: string) => invoke<void>("cv_set_key", { key }),
  cvSearch: (seriesId: number, query?: string) =>
    invoke<{ query: string; volumes: CvVolume[] }>("cv_search", { seriesId, query: query ?? null }),
  cvPlan: (seriesId: number, volumeId: string) =>
    invoke<{ issues: CvIssue[]; proposals: CvProposal[] }>("cv_plan", { seriesId, volumeId }),
  cvApply: (seriesId: number, volumeId: string, pairs: { comicId: number; issueId: string | null }[]) =>
    invoke<number>("cv_apply", { seriesId, volumeId, pairs }),
  cvApplyBook: (comicId: number, volumeId: string, issueId: string | null) =>
    invoke<number>("cv_apply_book", { comicId, volumeId, issueId }),
  setUserPanels: (id: number, page: number, panels: Panel[] | null) =>
    invoke<void>("set_user_panels", { id, page, panels }),
  toggleBookmark: (id: number, page: number) => invoke<boolean>("toggle_bookmark", { id, page }),
  openPath: (path: string) => invoke<number>("open_path", { path }),
  takeOpenRequest: () => invoke<string | null>("take_open_request"),
  planAdd: (paths: string[], seriesId: number | null, collectionId: number | null) =>
    invoke<Placement[]>("plan_add", { paths, seriesId, collectionId }),
  addComics: (paths: string[], seriesId: number | null, collectionId: number | null) =>
    invoke<number>("add_comics", { paths, seriesId, collectionId }),
  savePage: (id: number, page: number, dest: string) => invoke<void>("save_page", { id, page, dest }),
  extractPages: (id: number, from: number, to: number, dir: string) =>
    invoke<number>("extract_pages", { id, from, to, dir }),
  saveProgress: (id: number, page: number, pageCount: number) =>
    invoke<void>("save_progress", { id, page, pageCount }),
  setRead: (id: number, read: boolean) => invoke<void>("set_read", { id, read }),
  markReadBefore: (id: number) => invoke<void>("mark_read_before", { id }),
  dismissBackfill: (seriesId: number) => invoke<void>("dismiss_backfill", { seriesId }),
  setSeriesRead: (seriesId: number, read: boolean) => invoke<void>("set_series_read", { seriesId, read }),
  reveal: (id: number) => invoke<void>("reveal", { id }),
};

// Custom protocol: Windows' WebView2 exposes it as http://aion.localhost.
const base = navigator.userAgent.includes("Windows") ? "http://aion.localhost" : "aion://localhost";
export const coverUrl = (id: number) => `${base}/cover/${id}`;
export const pageUrl = (id: number, n: number) => `${base}/page/${id}/${n}`;
export const thumbUrl = (id: number, n: number) => `${base}/thumb/${id}/${n}`;
/** The screen's height in pixels, rounded up to a multiple of 120: big pages get copies this tall. */
export const screenHeight = Math.ceil((screen.height * devicePixelRatio) / 120) * 120;
/** A page to show whole: its copy at screen height if one's been made, else the page itself. */
export const screenUrl = (id: number, n: number) => `${base}/screen/${id}/${n}/${screenHeight}`;
/** A page's small preview, only if it's been made already (404 otherwise; see warm.rs). */
export const previewUrl = (id: number, n: number) => `${base}/preview/${id}/${n}`;

export const unitWord = (isManga: boolean, n: number) =>
  isManga ? (n === 1 ? "volume" : "volumes") : n === 1 ? "issue" : "issues";

export const progressOf = (c: ComicCard) =>
  c.read ? 1 : c.numPages && c.currentPage > 0 ? (c.currentPage + 1) / c.numPages : 0;

/** The story title, unless it just repeats the label ("Vol. 1" / "Vol. 1"). */
/** Titles that only restate the number or the format ("Book 1", "Volume One", "TPB"); the same rule
 *  the library uses when it reads ComicInfo.xml and Comic Vine. */
const PLACEHOLDER_TITLE =
  /^(?:(?:v|vol\.?|volume|book|issue|no\.?|#|chapter|ch\.?|part)?\s*(?:\d+(?:\.\d+)?|one|two|three|four|five|six|seven|eight|nine|ten)|tpb|hc|hardcover|trade paperback|one[- ]shot|omnibus|o?gn)$/i;

export function storyTitle(c: ComicCard): string | null {
  const t = c.title?.trim();
  if (!t || PLACEHOLDER_TITLE.test(t)) return null;
  const norm = (x: string) => x.toLowerCase().replace(/[^a-z0-9]/g, "").replace(/^(vol|volume|issue|no)/, "");
  return norm(t) === norm(c.label) ? null : t;
}

export const percent = (c: ComicCard) => Math.max(1, Math.round(progressOf(c) * 100));

/** "Today", "3 days ago", "2 months ago". */
export function ago(t: number | null): string {
  if (!t) return "";
  const d = Math.floor((Date.now() / 1000 - t) / 86400);
  if (d < 1) return "Today";
  if (d < 2) return "Yesterday";
  if (d < 7) return `${d} days ago`;
  if (d < 30) return `${Math.round(d / 7)} week${d < 11 ? "" : "s"} ago`;
  if (d < 365) return `${Math.round(d / 30)} month${d < 45 ? "" : "s"} ago`;
  return `${Math.round(d / 365)} year${d < 548 ? "" : "s"} ago`;
}

/** Comic Vine synopses arrive as HTML; we only want the prose. */
export function plainText(html: string | null): string {
  if (!html) return "";
  // Chapter lists etc. follow the blurb; keep only the opening paragraphs.
  // Many open with a bold tagline paragraph ("FOR HONOR AND VENGEANCE") that reads as a run-on; drop it.
  const cut = html.split(/<h[1-6][^>]*>/i)[0].replace(/^\s*<p>\s*<(strong|b)>[\s\S]*?<\/\1>\s*<\/p>/i, "");
  const spaced = cut.replace(/<(br|\/p|\/div|\/li|\/h\d)[^>]*>/gi, "$& ");
  const doc = new DOMParser().parseFromString(spaced, "text/html");
  return (doc.body.textContent ?? "").replace(/\s+/g, " ").trim();
}

/** Lists stored one per line read as "Thorfinn, Askeladd". */
export const listText = (v: string | null | undefined) =>
  (v ?? "")
    .split("\n")
    .map((x) => x.trim())
    .filter(Boolean)
    .join(", ");
