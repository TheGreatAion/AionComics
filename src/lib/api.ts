import { invoke } from "@tauri-apps/api/core";
import { gate } from "./gate.svelte";

export { gate };

/** Opened in a web browser on another device (see remote.rs), not in AionComics' own window. */
export const remote = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);

/** Ask AionComics for something: through the window, or from another device over the network. */
async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!remote) return invoke<T>(cmd, args);
  const res = await fetch(`/api/${cmd}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args ?? {}),
  }).catch(() => {
    throw "Can't reach AionComics. Is the PC on, and AionComics open?";
  });
  if (res.status === 401) gate.locked = true;
  const body = await res.json().catch(() => null);
  if (!res.ok) throw typeof body === "string" ? body : `Something went wrong (${res.status})`;
  return body as T;
}

/** On another device: whether it's in yet, and typing the code to get in. */
export const door = {
  check: async () => {
    const res = await fetch("/api/hello", { cache: "no-store" });
    gate.locked = res.status === 401;
  },
  enter: async (code: string) => {
    const res = await fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ code }),
    });
    if (!res.ok) throw (await res.json().catch(() => null)) ?? "That didn't work.";
    gate.locked = false;
  },
};

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
  /** On the series page: what an annual or special follows in the story ("#12"). */
  after?: string;
}

export type EditionKind = "volume" | "issue" | "chapter" | "collection" | "deluxe" | "omnibus" | "annual" | "special";

/** A book that wouldn't open: what's known about it without opening it. */
export interface BookTrouble {
  title: string;
  fileName: string;
  kind: "drive" | "missing" | "damaged";
  canShow: boolean;
  next: ComicCard | null;
}

/** A name a search found: a creator, a character or team, or a story arc. */
export interface NameHit {
  name: string;
  books: number;
  coverId: number;
  /** How closely it matches, the closest lowest. */
  close: number;
}

export interface SearchHits {
  series: SeriesSummary[];
  books: ComicCard[];
  creators: NameHit[];
  characters: NameHit[];
  arcs: NameHit[];
  /** The book you're partway through in the first series, if you are. */
  topReading: ComicCard | null;
}

export type PeopleKind = "creator" | "character" | "arc";

/** One format of a series ("Volumes", "Issues", "Annuals"…), its books in their subfolders; or, as
 *  "order", all of them together in the order the story runs. */
/** A book's reading state before a change, so the change can be undone. */
/** A book's format as set by hand (null: sorted automatically), for Undo. */
export interface EditionState {
  id: number;
  kind: EditionKind | null;
}

export interface ReadState {
  id: number;
  read: boolean;
  currentPage: number;
  lastReadAt: number | null;
  hiddenAt: number | null;
}

export interface Edition {
  kind: EditionKind | "order";
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
  /** How you've said its series read, if you have. */
  reading: "comic" | "manga" | "webtoon" | null;
  /** Covers for the fanned stack on Home: the three most recently read series. */
  covers: number[];
}

export interface NavInfo {
  collections: CollectionSummary[];
  /** How many things are on Want to read. */
  wanted: number;
}

/** Your Want to read list, the last put on first; `seriesAt` and `booksAt` say when each went on. */
export interface WantList {
  series: SeriesSummary[];
  seriesAt: number[];
  books: ComicCard[];
  booksAt: number[];
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
  /** Whether that cover's print (for behind the collection's name) is made yet. */
  printed: boolean;
  /** How you've said this series reads, if you have. */
  reading: "comic" | "manga" | "webtoon" | null;
  /** On your Want to read list (and not started since). */
  wanted: boolean;
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
  /** Its books whose cover is already printed: those show at once behind its name. */
  printed: number[];
  /** How you've said this series reads, if you have. */
  reading: "comic" | "manga" | "webtoon" | null;
  writer: string | null;
  /** Penciller or artist, when known. */
  artist: string | null;
  publisher: string | null;
  years: string | null;
  synopsis: string | null;
  sections: { name: string; comics: ComicCard[] }[];
  editions: Edition[];
  /** The order you've put its formats in, if you have. The first one is what the series opens on. */
  formatOrder: string[] | null;
  backfill: { beforeId: number; beforeLabel: string; count: number } | null;
  /** The Comic Vine series this one is matched to. */
  cvVolumeId: string | null;
  /** On your Want to read list, and which of its books are on it by themselves. */
  wanted: boolean;
  wantedBooks: number[];
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

/** Everything by one creator, with one character or team in it, or in one story arc, by series. */
export interface PeopleView {
  name: string;
  kind: PeopleKind;
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
  /** Books matched to Comic Vine, of all `books`. */
  matched: number;
  books: number;
  lastError: string | null;
}

/** A panel in Guided View: the `panel`th stop on `page`, and how far down it if it's read in parts. */
export interface GuidedSpot {
  page: number;
  panel: number;
  part: number;
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
  /** Reads right to left: manga, unless YACReader says it's left-to-right manga or a webtoon. */
  rtl: boolean;
  /** A webtoon, as YACReader says: one long strip, read scrolling. */
  webtoon: boolean;
  /** The panel the book was left on in Guided View. */
  guidedSpot: GuidedSpot | null;
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
  options: Options;
  /** Where the library database and backups are kept. */
  dataDir: string;
  /** The YACReader library in the comics folder, if there is one. */
  yacreader: YacStatus | null;
  /** How the folders at the top are read. */
  layout: "auto" | "libraries" | "series";
}

/** Sending your reading to YACReader. */
export interface YacStatus {
  /** Books whose reading hasn't reached YACReader yet. */
  pending: number;
  /** Held back because YACReader is open. */
  waiting: boolean;
  lastSent: { at: number; books: number } | null;
}

/** Choices on the Settings page that the engine acts on. */
export interface Options {
  watchFolder: boolean;
  readFileDetails: boolean;
  /** Send your reading to YACReader's library too. */
  yacreaderSync: boolean;
  cacheDays: number;
  backupsKeep: number;
}

/** Bytes AionComics' cache takes, by kind. */
export interface Storage {
  pages: number;
  prints: number;
  lists: number;
}

/** Where a comic dropped on the window would be copied. */
export type Placement = {
  source: string;
  fileName: string;
  /** Folder inside the comics folder, e.g. "DC/Absolute Batman". */
  folder: string | null;
  status: "add" | "exists" | "name-taken" | "new-series" | "no-place" | "not-comic";
  /** Copied as it is, unpacked from a .zip or .rar of several books, or packed from a folder of pages. */
  how: "copy" | "unpack" | "pack";
  size: number;
};

/** How far copying dropped comics has got ("add-progress"). */
export type AddProgress = { done: number; total: number; bytes: number; totalBytes: number };

/** The kinds of file that are comics, for file pickers. */
export const COMIC_FILES = [{ name: "Comics", extensions: ["cbz", "cbr", "cb7", "pdf", "epub", "zip", "rar", "7z"] }];

export const api = {
  settings: () => call<Settings>("get_settings"),
  setOptions: (options: Options) => call<void>("set_options", { options }),
  storage: () => call<Storage>("storage"),
  clearPages: () => call<void>("clear_pages"),
  openDataFolder: () => call<void>("open_data_folder"),
  setLibraryRoot: (path: string) => call<void>("set_library_root", { path }),
  rescan: () => call<void>("rescan"),
  /** A Comics folder in Documents becomes the comics folder. Returns where it is. */
  startFresh: () => call<string>("start_fresh"),
  setLayout: (layout: string) => call<void>("set_layout", { layout }),
  setSeriesReading: (id: number, reading: string | null) => call<void>("set_series_reading", { id, reading }),
  /** Returns how they were, for Undo. */
  setFormatOrder: (id: number, order: string[] | null) => call<string[] | null>("set_format_order", { id, order }),
  setLibraryReading: (id: number, reading: string | null) => call<void>("set_library_reading", { id, reading }),
  /** Hide books, a series or a library; with `del`, move their files to the Recycle Bin. */
  removeBooks: (t: { books?: number[]; series?: number; library?: number; title: string }, del: boolean) =>
    call<void>("remove_books", { books: t.books ?? null, series: t.series ?? null, library: t.library ?? null, label: t.title, delete: del }),
  hiddenList: () => call<{ label: string; at: number }[]>("hidden_list"),
  putBack: (label: string, at: number) => call<void>("put_back", { label, at }),
  /** A new, empty library: a folder at the top of the comics folder. Returns its id. */
  newLibrary: (name: string) => call<number>("new_library", { name }),
  backups: () => call<Backup[]>("backups"),
  backupNow: () => call<Backup>("backup_now"),
  /** Backs up today's state, then restarts AionComics with the chosen backup in place. */
  restoreBackup: (name: string) => call<void>("restore_backup", { name }),
  home: () => call<Home>("home"),
  nav: () => call<NavInfo>("nav"),
  search: (q: string) => call<SearchHits>("search", { q }),
  collection: (id: number) => call<CollectionView>("collection", { id }),
  series: (id: number) => call<SeriesView>("series", { id }),
  people: (kind: PeopleKind, name: string) => call<PeopleView>("people", { kind, name }),
  bookTrouble: (id: number) => call<BookTrouble>("book_trouble", { id }),
  bookStats: (id: number) => call<BookStats>("book_stats", { id }),
  duplicates: () => call<DupGroup[]>("duplicates"),
  ignoreDuplicate: (key: string) => call<void>("ignore_duplicate", { key }),
  readingStats: () => call<ReadingStats>("reading_stats", { tzMinutes: -new Date().getTimezoneOffset() }),
  openComic: (id: number) => call<ReaderInfo>("open_comic", { id, screen: screenHeight }),
  pagePanels: (id: number, page: number, rtl: boolean) => call<Panel[]>("page_panels", { id, page, rtl }),
  /** Guided View: work out the book's panels in the background, from page `from` on. */
  panelsAhead: (id: number, from: number, rtl: boolean) => call<void>("panels_ahead", { id, from, rtl }),
  comicMeta: (id: number) => call<ComicMeta>("comic_meta", { id }),
  /** Who's in each of these books: [teams, characters]. */
  casts: (ids: number[]) => call<[string | null, string | null][]>("casts", { ids }),
  editComicMeta: (id: number, changes: Partial<Record<MetaField, string | null>>) =>
    call<ComicMeta>("edit_comic_meta", { id, changes }),
  cvStatus: () => call<CvStatus>("cv_status"),
  cvOpenSite: () => call<void>("cv_open_site"),
  setEdition: (id: number, kind: EditionKind | null) => call<void>("set_edition", { id, kind }),
  /** Several books at once; returns how they were, for Undo (restoreEditions). */
  setEditions: (ids: number[], kind: EditionKind | null) => call<EditionState[]>("set_editions", { ids, kind }),
  restoreEditions: (states: EditionState[]) => call<void>("restore_editions", { states }),
  cvSetKey: (key: string) => call<void>("cv_set_key", { key }),
  cvSearch: (seriesId: number, query?: string) =>
    call<{ query: string; volumes: CvVolume[] }>("cv_search", { seriesId, query: query ?? null }),
  cvPlan: (seriesId: number, volumeId: string) =>
    call<{ issues: CvIssue[]; proposals: CvProposal[] }>("cv_plan", { seriesId, volumeId }),
  cvApply: (seriesId: number, volumeId: string, pairs: { comicId: number; issueId: string | null }[]) =>
    call<number>("cv_apply", { seriesId, volumeId, pairs }),
  cvApplyBook: (comicId: number, volumeId: string, issueId: string | null) =>
    call<number>("cv_apply_book", { comicId, volumeId, issueId }),
  setUserPanels: (id: number, page: number, panels: Panel[] | null) =>
    call<void>("set_user_panels", { id, page, panels }),
  toggleBookmark: (id: number, page: number) => call<boolean>("toggle_bookmark", { id, page }),
  openPath: (path: string) => call<number>("open_path", { path }),
  takeOpenRequest: () => call<string | null>("take_open_request"),
  /** Where these would go. `name` puts everything for a collection into the one series of that name. */
  planAdd: (paths: string[], seriesId: number | null, collectionId: number | null, name: string | null = null) =>
    call<Placement[]>("plan_add", { paths, seriesId, collectionId, name }),
  addComics: (paths: string[], seriesId: number | null, collectionId: number | null, name: string | null = null) =>
    call<number>("add_comics", { paths, seriesId, collectionId, name }),
  savePage: (id: number, page: number, dest: string) => call<void>("save_page", { id, page, dest }),
  extractPages: (id: number, from: number, to: number, dir: string) =>
    call<number>("extract_pages", { id, from, to, dir }),
  saveProgress: (id: number, page: number, pageCount: number) =>
    call<void>("save_progress", { id, page, pageCount }),
  saveGuidedSpot: (id: number, spot: GuidedSpot | null) => call<void>("save_guided_spot", { id, spot }),
  // These four return how the books were before, for Undo (restoreReading).
  setRead: (id: number, read: boolean) => call<ReadState[]>("set_read", { id, read }),
  /** Off Continue reading, with the page you're on kept. Reading it again puts it back. */
  wantList: () => call<WantList>("want_list"),
  /** Put a series or one book on Want to read, or take it off. */
  setWanted: (t: { series?: number; book?: number }, on: boolean) => call<void>("set_wanted", { series: t.series ?? null, book: t.book ?? null, on }),
  hideFromReading: (id: number) => call<ReadState[]>("hide_from_reading", { id }),
  markReadBefore: (id: number) => call<ReadState[]>("mark_read_before", { id }),
  setSeriesRead: (seriesId: number, read: boolean) => call<ReadState[]>("set_series_read", { seriesId, read }),
  restoreReading: (states: ReadState[]) => call<void>("restore_reading", { states }),
  /** Books taken off Continue reading and not read since. */
  takenOffReading: () => call<ComicCard[]>("taken_off_reading"),
  unhideFromReading: (id: number) => call<void>("unhide_from_reading", { id }),
  dismissBackfill: (seriesId: number) => call<void>("dismiss_backfill", { seriesId }),
  reveal: (id: number) => call<void>("reveal", { id }),
  remoteStatus: () => call<RemoteStatus>("remote_status"),
  setRemote: (on: boolean) => call<RemoteStatus>("set_remote", { on }),
  newRemoteCode: () => call<RemoteStatus>("new_remote_code"),
};

/** Reading from another device, as Settings shows it. */
export interface RemoteStatus {
  on: boolean;
  /** "http://192.168.1.20:8642": on the same Wi-Fi. */
  home: string | null;
  /** Through Tailscale: works away from home too. */
  away: string | null;
  /** Square codes for a phone's camera (SVG). */
  homeQr: string | null;
  awayQr: string | null;
  code: string;
  devices: number;
  problem: string | null;
}

// Custom protocol: Windows' WebView2 exposes it as http://aion.localhost. On another device, the
// same pictures come from AionComics' web server.
const base = remote ? "/img" : navigator.userAgent.includes("Windows") ? "http://aion.localhost" : "aion://localhost";
export const coverUrl = (id: number) => `${base}/cover/${id}`;
export const pageUrl = (id: number, n: number) => `${base}/page/${id}/${n}`;
export const thumbUrl = (id: number, n: number) => `${base}/thumb/${id}/${n}`;
export const printUrl = (id: number) => `${base}/print/${id}`;
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

/** One chapter in a volume's list: "Chapter 301: Memory" is 301 and "Memory". */
export interface Chapter {
  n: string | null;
  title: string;
}
const CHAPTER_ITEM =
  /^(?:chapter|ch\.?|episode|ep\.?|file|act|lesson|part|night|round|story|stage|level|quest|#)\s*:?\s*(\d+(?:\.\d+)?)\s*(?:[:.\-–—]\s*)?(.*)$/i;

/** The chapters a volume holds (or the stories and issues a comic collects), from the list under
 *  its description's "Chapter Titles" (or "Contents", "Collects", "Chapter List"…) heading, as
 *  Comic Vine writes them. Read from the book's details each time they're shown, so a book added
 *  later shows its list as soon as its details arrive. */
export function chapterList(html: string | null): Chapter[] {
  if (!html) return [];
  const doc = new DOMParser().parseFromString(html, "text/html");
  const head = [...doc.querySelectorAll("h1, h2, h3, h4, h5, h6")].find((h) =>
    /chapter|contents|story titles|episode|collects|collected/i.test(h.textContent ?? ""),
  );
  const list = head?.nextElementSibling;
  if (!list || !/^(UL|OL)$/.test(list.tagName)) return [];
  return [...list.querySelectorAll(":scope > li")]
    .map((li) => {
      const t = (li.textContent ?? "").replace(/\s+/g, " ").trim();
      const m = CHAPTER_ITEM.exec(t);
      return m ? { n: m[1], title: m[2] || t } : { n: null, title: t };
    })
    .filter((c) => c.title);
}

/** Lists stored one per line read as "Thorfinn, Askeladd". */
export const listText = (v: string | null | undefined) =>
  (v ?? "")
    .split("\n")
    .map((x) => x.trim())
    .filter(Boolean)
    .join(", ");
