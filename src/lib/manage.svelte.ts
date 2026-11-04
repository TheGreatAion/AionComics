import { api } from "./api";
import { lib, showMenu, type MenuItem } from "./state.svelte";

/** What the Remove box is asking about: some books, a series or a library. */
export type RemoveTarget = {
  books?: number[];
  series?: number;
  library?: number;
  title: string;
  /** How many books that is, in words. */
  detail: string;
  /** What "delete" takes: "the file", "the folder"... */
  what: string;
  /** The page you're on won't be there afterward. */
  leaves?: boolean;
};

export const removing = $state({ target: null as RemoveTarget | null });

export type Reading = "comic" | "manga" | "webtoon" | null;

/** How a series reads, or a whole library's series: worked out, or what you say. */
export function readAsItems(current: Reading | undefined, pick: (r: Reading) => Promise<unknown>): MenuItem[] {
  const choose = (r: Reading) => () => pick(r).then(() => lib.version++);
  return [
    { label: "Work it out", checked: current === null, action: choose(null) },
    "separator",
    { label: "Comic: left to right", checked: current === "comic", action: choose("comic") },
    { label: "Manga: right to left", checked: current === "manga", action: choose("manga") },
    { label: "Webtoon: scrolls down", checked: current === "webtoon", action: choose("webtoon") },
  ];
}

/** A library's menu, wherever it's right-clicked or its ⋯ pressed. */
export function libraryMenu(e: MouseEvent, c: { id: number; name: string; seriesCount: number; comicCount: number }, here: boolean) {
  const series = c.seriesCount === 1 ? "1 series" : `${c.seriesCount.toLocaleString()} series`;
  const books = c.comicCount === 1 ? "1 book" : `${c.comicCount.toLocaleString()} books`;
  showMenu(e, [
    { label: "Read everything here as", items: readAsItems(undefined, (r) => api.setLibraryReading(c.id, r)), disabled: !c.seriesCount },
    "separator",
    { label: "Remove…", action: () => (removing.target = { library: c.id, title: c.name, detail: `${series}, ${books}`, what: "everything in it", leaves: here }) },
  ]);
}
