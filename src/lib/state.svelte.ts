import { api, type CollectionSummary, type ReadState } from "./api";

export type Route =
  | { name: "home" }
  | { name: "collection"; id: number }
  /** `book`: the book to show first, instead of the one you're on. */
  | { name: "series"; id: number; book?: number }
  /** Everything by a creator, or with a character or team in it. */
  | { name: "people"; kind: "creator" | "character" | "arc"; value: string }
  /** Books that may be in the library twice. */
  | { name: "duplicates" }
  /** Books that wouldn't open: missing, damaged, empty, or a cover that won't load. */
  | { name: "health" }
  /** What your reading adds up to. */
  | { name: "stats" }
  /** Series and books you mean to read. */
  | { name: "wanted" }
  | { name: "settings" };

type Entry = { route: Route; scroll: number };

class Nav {
  stack = $state<Entry[]>([{ route: { name: "home" }, scroll: 0 }]);

  get current(): Route {
    return this.stack[this.stack.length - 1].route;
  }
  get canGoBack() {
    return this.stack.length > 1;
  }
  /** Drill deeper (keeps history and remembers where you were on this page). */
  go(r: Route) {
    this.stack[this.stack.length - 1].scroll = window.scrollY;
    this.stack.push({ route: r, scroll: 0 });
  }
  /** Jump from the top bar (resets history). */
  root(r: Route) {
    this.stack = [{ route: r, scroll: 0 }];
  }
  back() {
    if (this.stack.length > 1) this.stack.pop();
  }
  /** Views call this once their content is on screen, so Back lands where you left off. */
  restore() {
    const y = this.stack[this.stack.length - 1].scroll;
    if (y) requestAnimationFrame(() => window.scrollTo(0, y));
  }
}

export const nav = new Nav();

/** Bumped whenever library data may have changed, so views refetch. */
export const lib = $state({
  version: 0,
  scanning: false,
  hasRoot: true as boolean | null,
  /** Still on the welcome screen, after the folder's been chosen. */
  welcoming: false,
  collections: [] as CollectionSummary[],
  /** Whether `collections` has been fetched yet: nothing in it means no libraries only once it has. */
  collectionsLoaded: false,
  /** How many things are on Want to read. */
  wanted: 0,
  /** Why the last scan changed nothing (drive missing), or what it held back. Shown in the top bar. */
  notice: "",
  /** A few words for the top bar; the full notice is its tooltip and appears in Settings. */
  noticeLabel: "",
});

/** The last change to what's read, offered back for a few seconds: Undo, or Ctrl+Z. */
export const undo = $state({ text: "", run: null as null | (() => Promise<void>) });
let undoTimer: ReturnType<typeof setTimeout> | undefined;
const UNDO_MS = 7000;

/** Mark something read, or take it off Continue reading, and offer to take it back. `after` runs
 *  once it's been taken back. */
export function undoable(text: string, change: () => Promise<ReadState[]>, after?: () => void) {
  return undoableWith(text, change, api.restoreReading, after);
}

/** Any change that returns how things were, and `restore` to put them back. */
export async function undoableWith<T>(text: string, change: () => Promise<T>, restore: (before: T) => Promise<void>, after?: () => void) {
  const before = await change();
  lib.version++;
  undo.text = text;
  undo.run = async () => {
    await restore(before);
    lib.version++;
    after?.();
  };
  holdUndo(false);
}

export function takeBack() {
  const run = undo.run;
  dropUndo();
  return run?.();
}

export function dropUndo() {
  clearTimeout(undoTimer);
  undo.text = "";
  undo.run = null;
}

/** The pointer over the bar keeps it up. */
export function holdUndo(hold: boolean) {
  clearTimeout(undoTimer);
  if (!hold && undo.run) undoTimer = setTimeout(dropUndo, UNDO_MS);
}

export const reader = $state({
  id: null as number | null,
  /** The image the reader grew out of, so it can shrink back into it. */
  from: null as HTMLImageElement | null,
});

export function openReader(id: number, from: HTMLImageElement | null = null) {
  reader.from = from;
  reader.id = id;
}

export const search = $state({ open: false });

/** The book whose details are being edited, and the series being matched on Comic Vine. */
export const sheets = $state({
  editing: null as number | null,
  /** With `comicId`, just that one book is matched (`label` names it; `count`: how many books of its format the series has). */
  matching: null as { seriesId: number; isManga: boolean; name: string; comicId?: number; label?: string; count?: number } | null,
});

export type MenuItem =
  | {
      label: string;
      action?: () => void;
      /** A list that opens beside this item (then there's no action of its own). */
      items?: MenuItem[];
      /** Keyboard shortcut shown on the right. */
      hint?: string;
      /** Toggle state: true shows a tick, false reserves the space. */
      checked?: boolean;
      disabled?: boolean;
    }
  | "separator";

export const menu = $state({
  open: false,
  x: 0,
  y: 0,
  items: [] as MenuItem[],
});

export function showMenu(e: MouseEvent, items: MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  menu.x = e.clientX;
  menu.y = e.clientY;
  menu.items = items;
  menu.open = true;
}
