// Every reader tool, once. The menu, the shortcuts sheet and key handling all read this table.

import { remote } from "$lib/api";
import type { MenuItem } from "$lib/state.svelte";
import type { Ctl } from "./ctl.svelte";
import { pretty } from "./keys";
import { prefs, savePrefs } from "./prefs.svelte";

export type Action = {
  id: string;
  label: string;
  /** Menu section; a separator goes between sections. */
  group: string;
  keys: string[];
  run: (c: Ctl) => void;
  checked?: (c: Ctl) => boolean;
  enabled?: (c: Ctl) => boolean;
  /** Keys-only actions (arrows, Space) stay out of the menu. */
  hidden?: boolean;
};

const pagesOnly = (c: Ctl) => c.mode === "pages";
const library = (c: Ctl) => !c.info?.external;

export const ACTIONS: Action[] = [
  { id: "open", label: "Open…", group: "file", keys: ["o"], run: (c) => c.openFile() },
  { id: "openFolder", label: "Open folder…", group: "file", keys: ["ctrl+o"], run: (c) => c.openFolder() },
  { id: "save", label: "Save page…", group: "file", keys: ["ctrl+s"], run: (c) => c.savePage() },
  { id: "extract", label: "Extract pages…", group: "file", keys: ["ctrl+e"], run: (c) => c.toggle("extract") },

  {
    id: "prevComic",
    label: "Previous book",
    group: "books",
    keys: ["ctrl+ArrowLeft"],
    run: (c) => c.info?.prevId && c.hooks.open(c.info.prevId),
    enabled: (c) => !!c.info?.prevId,
  },
  {
    id: "nextComic",
    label: "Next book",
    group: "books",
    keys: ["ctrl+ArrowRight"],
    run: (c) => c.info?.next && c.hooks.open(c.info.next.id),
    enabled: (c) => !!c.info?.next,
  },

  { id: "prev", label: "Previous page", group: "pages", keys: ["PageUp", "Backspace"], run: (c) => c.prev() },
  { id: "next", label: "Next page", group: "pages", keys: ["Space", "PageDown"], run: (c) => c.forward() },
  { id: "left", label: "Turn left", group: "pages", keys: ["ArrowLeft"], run: (c) => c.left(), hidden: true },
  { id: "right", label: "Turn right", group: "pages", keys: ["ArrowRight"], run: (c) => c.right(), hidden: true },
  { id: "first", label: "First page", group: "pages", keys: ["Home"], run: (c) => c.goTo(0), hidden: true },
  { id: "last", label: "Last page", group: "pages", keys: ["End"], run: (c) => c.goTo(c.last), hidden: true },
  { id: "goto", label: "Go to page…", group: "pages", keys: ["g"], run: (c) => c.hooks.focusPage?.() },
  {
    id: "flow",
    label: "Page strip",
    group: "pages",
    keys: ["s"],
    run: (c) => c.toggle("flow"),
    checked: (c) => c.overlay === "flow",
  },

  {
    id: "fitHeight",
    label: "Fit height",
    group: "fit",
    keys: ["h"],
    run: (c) => c.setFit("height"),
    checked: (c) => c.view.fit === "height",
  },
  {
    id: "fitWidth",
    label: "Fit width",
    group: "fit",
    keys: ["f"],
    run: (c) => c.setFit("width"),
    checked: (c) => c.view.fit === "width",
  },
  {
    id: "fitPage",
    label: "Fit page",
    group: "fit",
    keys: ["p"],
    run: (c) => c.setFit("page"),
    checked: (c) => c.view.fit === "page",
  },
  {
    id: "fullSize",
    label: "Full size",
    group: "fit",
    keys: ["w"],
    run: (c) => c.setFit("full"),
    checked: (c) => c.view.fit === "full",
  },
  {
    id: "scroll",
    label: "Continuous scroll",
    group: "fit",
    keys: ["shift+c"],
    run: (c) => c.toggleScroll(),
    checked: (c) => c.view.scroll,
  },

  {
    id: "rotateLeft",
    label: "Rotate left",
    group: "layout",
    keys: ["l"],
    run: (c) => c.rotate(-90),
    enabled: pagesOnly,
  },
  {
    id: "rotateRight",
    label: "Rotate right",
    group: "layout",
    keys: ["r"],
    run: (c) => c.rotate(90),
    enabled: pagesOnly,
  },
  {
    id: "double",
    label: "Double page",
    group: "layout",
    keys: ["d"],
    run: (c) => c.toggleDouble(),
    checked: (c) => c.double,
  },
  {
    id: "doubleManga",
    label: "Double page, right to left",
    group: "layout",
    keys: ["j"],
    run: (c) => c.toggleMangaDouble(),
    checked: (c) => c.double && c.pairRtl,
  },

  {
    id: "magnifier",
    label: "Magnifying glass",
    group: "zoom",
    keys: ["z"],
    run: (c) => (c.magnifier = !c.magnifier),
    checked: (c) => c.magnifier,
    enabled: pagesOnly,
  },
  { id: "zoomIn", label: "Zoom in", group: "zoom", keys: ["+", "="], run: (c) => c.zoomBy(1.25), enabled: (c) => c.mode !== "guided" },
  { id: "zoomOut", label: "Zoom out", group: "zoom", keys: ["-"], run: (c) => c.zoomBy(0.8), enabled: (c) => c.mode !== "guided" },
  { id: "zoomReset", label: "Reset zoom", group: "zoom", keys: ["ctrl+0"], run: (c) => (c.zoom = 1) },

  {
    id: "bookmark",
    label: "Bookmark this page",
    group: "marks",
    keys: ["ctrl+m"],
    run: (c) => c.toggleBookmark(),
    checked: (c) => c.bookmarks.includes(c.page),
    enabled: library,
  },
  {
    id: "bookmarks",
    label: "Show bookmarks",
    group: "marks",
    keys: ["m"],
    run: (c) => c.toggle("bookmarks"),
    checked: (c) => c.overlay === "bookmarks",
    enabled: library,
  },

  {
    id: "guided",
    label: "Guided View",
    group: "guided",
    keys: ["v"],
    run: (c) => c.toggleGuided(),
    checked: (c) => c.guided,
  },
  {
    id: "editPanels",
    label: "Edit panels on this page",
    group: "guided",
    keys: ["e"],
    run: (c) => c.toggle("editor"),
    checked: (c) => c.overlay === "editor",
    enabled: library,
  },

  {
    id: "fullscreen",
    label: "Full screen",
    group: "view",
    keys: ["F11", "alt+Enter"],
    run: (c) => c.setFullscreen(!c.fullscreen),
    checked: (c) => c.fullscreen,
  },
  {
    id: "info",
    label: "Keep page number visible",
    group: "view",
    keys: ["i"],
    run: () => {
      prefs.info = !prefs.info;
      savePrefs();
    },
    checked: () => prefs.info,
  },
  { id: "shortcuts", label: "Keyboard shortcuts", group: "app", keys: ["?", "F1"], run: (c) => c.toggle("shortcuts") },
  { id: "options", label: "Options", group: "app", keys: ["c"], run: (c) => c.toggle("options") },
  { id: "close", label: "Back to library", group: "close", keys: ["Escape"], run: (c) => c.hooks.close() },
];

/** Keys in effect for an action: the user's override, or the default. */
export const keysFor = (a: Action, overrides: Record<string, string[]>) => overrides[a.id] ?? a.keys;

/** A finger, not a mouse and keyboard: a phone or tablet. */
const touch = () => typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;

/** Tools that need this PC's files, keyboard or mouse: left out when reading from another device. */
export const PC_ONLY = new Set(["open", "openFolder", "save", "extract", "editPanels", "shortcuts", "magnifier"]);

export function menuItems(c: Ctl, overrides: Record<string, string[]>): MenuItem[] {
  const items: MenuItem[] = [];
  let group = "";
  for (const a of ACTIONS) {
    if (a.hidden || (remote && PC_ONLY.has(a.id))) continue;
    if (remote && a.id === "fullscreen" && !document.fullscreenEnabled) continue;
    if (group && a.group !== group) items.push("separator");
    group = a.group;
    const keys = keysFor(a, overrides);
    // Next/previous show their arrow, which depends on the reading direction.
    const arrow = a.id === "next" ? (c.flipped ? "ArrowLeft" : "ArrowRight") : a.id === "prev" ? (c.flipped ? "ArrowRight" : "ArrowLeft") : null;
    items.push({
      label: a.label,
      // No keys to show on a phone or tablet.
      hint: touch() ? undefined : arrow ? pretty(arrow) : keys[0] ? pretty(keys[0]) : undefined,
      checked: a.checked ? a.checked(c) : false,
      disabled: a.enabled ? !a.enabled(c) : false,
      action: () => a.run(c),
    });
  }
  return items;
}
