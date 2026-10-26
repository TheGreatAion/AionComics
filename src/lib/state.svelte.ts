import type { CollectionSummary } from "./api";

export type Route =
  | { name: "home" }
  | { name: "collection"; id: number }
  /** `book`: the book to show first, instead of the one you're on. */
  | { name: "series"; id: number; book?: number }
  /** Everything by a creator, or with a character or team in it. */
  | { name: "people"; kind: "creator" | "character"; value: string }
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
  collections: [] as CollectionSummary[],
  /** Why the last scan changed nothing (drive missing), or what it held back. Shown in the top bar. */
  notice: "",
  /** A few words for the top bar; the full notice is its tooltip and appears in Settings. */
  noticeLabel: "",
});

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
  matching: null as { seriesId: number; isManga: boolean; name: string } | null,
});

export type MenuItem =
  | {
      label: string;
      action: () => void;
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
