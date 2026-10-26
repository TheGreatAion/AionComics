// Everything the reader knows and can do. Views render it; actions.ts maps keys and menu items onto it.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, pageUrl, type Panel, type ReaderInfo } from "$lib/api";
import { prefs, savePrefs, type Fit } from "./prefs.svelte";

export type Overlay = null | "flow" | "bookmarks" | "shortcuts" | "options" | "extract" | "editor";
export type Dims = { w: number; h: number };
/** One stop of Guided View: a panel on a page, or a whole page (a splash inside a spread). */
export type Step = { page: number; panel: Panel | null };

export const FIT_LABEL: Record<Fit, string> = {
  height: "Fit height",
  width: "Fit width",
  page: "Fit page",
  full: "Full size",
};

export class Ctl {
  info = $state<ReaderInfo | null>(null);
  error = $state<string | null>(null);
  /** First page on screen (the left page of a spread in comics, the right one in manga). */
  page = $state(0);
  /** Show this page alone in double page mode: turning back landed between a page standing alone
   *  and the spread just left, so pairing it would repeat a page. Cleared by any other move. */
  solo = $state(false);
  /** Where the book opened; progress is saved only once the page moves from here. */
  openedAt = 0;
  /** Bumped by navigation commands, so the continuous-scroll view knows to move. */
  seek = $state(0);
  /** Natural sizes of pages seen so far; spreads need to know which pages are wide. */
  dims = $state<Record<number, Dims>>({});
  zoom = $state(1);
  rotation = $state(0);
  magnifier = $state(false);
  lensZoom = $state(2.5);
  /** Spreads read right to left (J, or any manga). */
  pairRtl = $state(false);
  /** Guided View stops for the pages on screen, in reading order. */
  steps = $state<Step[] | null>(null);
  step = $state(-1); // -1 = the whole page or spread, then each stop
  fullscreen = $state(false);
  landOnLast = false; // stepping back into a page lands on its last panel
  /** A double-clicked point (page, and 0–1 across and down it) to zoom into once its panels load. */
  zoomAt: { page: number; x: number; y: number } | null = null;
  overlay = $state<Overlay>(null);
  bookmarks = $state<number[]>([]);
  endCard = $state(false);
  note = $state("");
  /** The scrolling element of the current view, for Space to scroll before turning. */
  stage: HTMLElement | null = null;
  private noteTimer: ReturnType<typeof setTimeout> | undefined;
  private measuring = new Set<number>();
  private warm: HTMLImageElement[] = [];

  constructor(public hooks: { open: (id: number) => void; close: () => void; focusPage?: () => void }) {}

  get id() {
    return this.info?.comic.id ?? 0;
  }
  get rtl() {
    return this.info?.comic.isManga ?? false;
  }
  get count() {
    return this.info?.pageCount ?? 0;
  }
  get last() {
    return Math.max(0, this.count - 1);
  }
  get guided() {
    return this.rtl ? prefs.guidedManga : prefs.guidedComics;
  }
  set guided(v: boolean) {
    if (this.rtl) prefs.guidedManga = v;
    else prefs.guidedComics = v;
    savePrefs();
  }
  get mode(): "guided" | "scroll" | "pages" {
    return this.guided ? "guided" : prefs.scroll ? "scroll" : "pages";
  }
  /** Two pages side by side, also while Guided View steps through them. */
  get double() {
    return prefs.double && this.mode !== "scroll";
  }
  /** Turning direction on screen: manga, or a spread set to read right to left. */
  get flipped() {
    return this.rtl || (this.double && this.pairRtl);
  }

  load(info: ReaderInfo) {
    // Page sizes and decoded pages belong to the previous book. Cleared here, together with the
    // switch, so the old book's pages can't measure themselves into the new one in between.
    this.dims = {};
    this.measuring.clear();
    this.warm = [];
    this.info = info;
    this.bookmarks = info.bookmarks;
    this.pairRtl = info.comic.isManga;
    this.step = -1;
    this.endCard = false;
    this.solo = false;
    this.page = this.align(info.startPage);
    this.openedAt = this.page;
    this.seek++;
  }

  // ---------- pages ----------

  wide(i: number) {
    const d = this.dims[i];
    return !!d && d.w > d.h * 1.05;
  }

  /** Load and decode a page before it's shown: its size pairs spreads correctly, and the decoded
   *  pixels make the turn instant instead of decoding a big scan the moment it appears. */
  measure(i: number) {
    if (i < 0 || i > this.last || this.measuring.has(i)) return;
    this.measuring.add(i);
    const book = this.id;
    const img = new Image();
    img.src = pageUrl(book, i);
    img
      .decode()
      .then(() => {
        if (this.id === book && !this.dims[i]) this.dims[i] = { w: img.naturalWidth, h: img.naturalHeight };
      })
      .catch(() => this.id === book && this.measuring.delete(i));
    // Hold on to the last few so their decoded pixels stay cached.
    this.warm.push(img);
    if (this.warm.length > 10) this.warm.shift();
  }

  /** In double page mode, a page that can't share the screen: the last page, a wide page, or one
   *  whose neighbor is wide. (The cover, page 0, always stands alone too.) */
  private alone(p: number) {
    return p >= this.last || this.wide(p) || this.wide(p + 1);
  }

  /** Pages on screen, in reading order. */
  get shown(): number[] {
    const p = this.page;
    if (!this.double || p === 0 || this.solo || this.alone(p)) return [p];
    return [p, p + 1];
  }

  /** The pages that going to p would put on screen, in reading order. */
  spreadAt(p: number): number[] {
    const s = this.align(p);
    if (!this.double || s === 0 || this.alone(s)) return [s];
    return [s, s + 1];
  }

  /** The first page of the spread that holds p. Spreads are laid out from the cover on: 1–2, 3–4…,
   *  and every page standing alone shifts the pairs after it, so jumping, resuming and turning back
   *  all land on the same spreads as turning forward. */
  private align(p: number) {
    p = Math.max(0, Math.min(this.last, p));
    if (!this.double || p === 0) return p;
    let s = 1;
    for (;;) {
      const next = s + (this.alone(s) ? 1 : 2);
      if (p < next) return s;
      s = next;
    }
  }

  /** The spread before the one starting at p. Normally the laid-out one; but page sizes arrive as
   *  pages load, and a wide page found late can shift the layout under the reader, so never land
   *  on a spread that repeats a page just seen. */
  private backFrom(p: number) {
    if (p <= 1) return 0;
    const s = this.align(p - 1);
    const end = s === 0 || !this.double || this.alone(s) ? s : s + 1;
    if (end === p - 1) return s;
    // Pair the two pages before, or failing that show the one before by itself (see solo).
    return p - 2 >= 1 && !this.alone(p - 2) ? p - 2 : p - 1;
  }

  goTo(p: number) {
    this.step = -1;
    this.endCard = false;
    this.solo = false;
    this.page = this.align(p);
    this.seek++;
  }

  next() {
    const info = this.info;
    if (!info) return;
    if (this.endCard) {
      if (info.next) this.hooks.open(info.next.id);
      else this.hooks.close();
      return;
    }
    if (this.mode === "guided" && this.steps && this.step < this.steps.length - 1) {
      this.step++;
      return;
    }
    const n = this.page + this.shown.length;
    if (n <= this.last) {
      this.step = -1;
      this.solo = false;
      this.page = n;
      this.seek++;
    } else this.endCard = true;
  }

  prev() {
    if (!this.info) return;
    if (this.endCard) {
      this.endCard = false;
      return;
    }
    // Without the whole-page stop, stepping back from the first panel goes to the page before.
    if (this.mode === "guided" && this.step > (prefs.guidedOverview || !this.steps?.length ? -1 : 0)) {
      this.step--;
      return;
    }
    if (this.page > 0) {
      this.step = -1;
      this.landOnLast = this.mode === "guided";
      const from = this.page;
      const to = this.backFrom(from);
      this.solo = this.double && to > 0 && to + 1 >= from && !this.alone(to);
      this.page = to;
      this.seek++;
    }
  }

  // Physical directions follow the reading direction.
  right() {
    if (this.flipped) this.prev();
    else this.next();
  }
  left() {
    if (this.flipped) this.next();
    else this.prev();
  }

  /** Space: scroll the page first if it doesn't fit, then turn. */
  forward() {
    const s = this.stage;
    if (s && this.mode !== "guided" && !this.endCard && s.scrollTop + s.clientHeight < s.scrollHeight - 4) {
      s.scrollBy({ top: s.clientHeight * 0.85, behavior: "smooth" });
      return;
    }
    if (this.mode === "scroll" && this.page >= this.last) {
      this.endCard = true;
      return;
    }
    this.next();
  }

  /** The last page in view, which is what progress should record. */
  get furthest() {
    return Math.max(...this.shown);
  }

  // ---------- view modes ----------

  setFit(f: Fit) {
    prefs.fit = f;
    this.zoom = 1;
    savePrefs();
  }

  cycleFit() {
    const order: Fit[] = ["page", "height", "width", "full"];
    this.setFit(order[(order.indexOf(prefs.fit) + 1) % order.length]);
  }

  toggleScroll() {
    prefs.scroll = !prefs.scroll;
    if (prefs.scroll) this.guided = false;
    this.solo = false;
    this.rotation = 0;
    savePrefs();
    this.seek++;
  }

  toggleDouble() {
    if (this.double) prefs.double = false;
    else {
      prefs.double = true;
      prefs.scroll = false;
      this.pairRtl = this.rtl;
    }
    savePrefs();
    this.goTo(this.page);
  }

  toggleMangaDouble() {
    if (this.double && this.pairRtl) prefs.double = false;
    else {
      prefs.double = true;
      prefs.scroll = false;
      this.pairRtl = true;
    }
    savePrefs();
    this.goTo(this.page);
  }

  /** The stop for a point on a page: the smallest panel holding it, else the nearest; -1 if none. */
  stopAt(page: number, x: number, y: number) {
    const steps = this.steps ?? [];
    let best = -1;
    let bestScore = Infinity;
    steps.forEach((s, i) => {
      if (s.page !== page || !s.panel) return;
      const p = s.panel;
      const inside = x >= p.x && x <= p.x + p.w && y >= p.y && y <= p.y + p.h;
      const dist = Math.hypot(x - (p.x + p.w / 2), y - (p.y + p.h / 2));
      const score = inside ? p.w * p.h : 10 + dist;
      if (score < bestScore) [best, bestScore] = [i, score];
    });
    return best;
  }

  /** Double-click on the page: Guided View, at the panel under the pointer. */
  zoomInto(page: number, x: number, y: number) {
    if (this.mode === "guided" && this.steps) {
      const i = this.stopAt(page, x, y);
      // On the stop already shown, a double-click goes back out to the whole page.
      this.step = i === this.step ? -1 : i;
      return;
    }
    this.zoomAt = { page, x, y };
    if (!this.guided) this.toggleGuided();
  }

  toggleGuided() {
    this.guided = !this.guided;
    this.step = -1;
    if (this.guided) this.magnifier = false;
  }

  async setFullscreen(on: boolean) {
    await getCurrentWindow().setFullscreen(on);
    this.fullscreen = on;
  }

  rotate(by: number) {
    this.rotation = (this.rotation + by + 360) % 360;
  }

  zoomBy(f: number) {
    this.zoom = Math.max(0.25, Math.min(8, +(this.zoom * f).toFixed(3)));
  }

  // ---------- bookmarks ----------

  async toggleBookmark() {
    if (!this.info) return;
    const page = this.page;
    try {
      const on = await api.toggleBookmark(this.id, page);
      this.bookmarks = on ? [...this.bookmarks, page].sort((a, b) => a - b) : this.bookmarks.filter((b) => b !== page);
      this.say(on ? `Bookmarked page ${page + 1}` : `Removed the bookmark on page ${page + 1}`);
    } catch (e) {
      this.say(String(e));
    }
  }

  // ---------- files ----------

  async openFile() {
    const path = await open({
      title: "Open a comic",
      filters: [{ name: "Comics", extensions: ["cbz", "cbr", "zip", "rar"] }],
    });
    if (typeof path === "string") this.openPath(path);
  }

  async openFolder() {
    const path = await open({ title: "Open a folder of images", directory: true });
    if (typeof path === "string") this.openPath(path);
  }

  private async openPath(path: string) {
    try {
      this.hooks.open(await api.openPath(path));
    } catch (e) {
      this.say(String(e));
    }
  }

  async savePage() {
    if (!this.info) return;
    const c = this.info.comic;
    const name = `${[c.seriesName.trim(), c.label].filter(Boolean).join(" ")} - ${String(this.page + 1).padStart(3, "0")}.jpg`;
    const dest = await save({ title: "Save page", defaultPath: name });
    if (!dest) return;
    try {
      await api.savePage(this.id, this.page, dest);
      this.say(`Saved page ${this.page + 1}`);
    } catch (e) {
      this.say(String(e));
    }
  }

  async extract(from: number, to: number) {
    const dir = await open({ title: "Choose a folder for the pages", directory: true });
    if (typeof dir !== "string") return;
    try {
      const n = await api.extractPages(this.id, from, to, dir);
      this.overlay = null;
      this.say(`Saved ${n} ${n === 1 ? "page" : "pages"}`);
    } catch (e) {
      this.say(String(e));
    }
  }

  // ---------- small things ----------

  say(text: string) {
    clearTimeout(this.noteTimer);
    this.note = text;
    this.noteTimer = setTimeout(() => (this.note = ""), 3000);
  }

  toggle(o: Exclude<Overlay, null>) {
    this.overlay = this.overlay === o ? null : o;
  }
}
