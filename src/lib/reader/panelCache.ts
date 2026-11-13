// Panels the reader has already asked for, kept for the session: turning to a page whose panels
// are here goes straight to its first panel, with no wait and no glimpse of the whole page.

import { api, type Panel } from "$lib/api";

/** Pages kept; the oldest are dropped first. */
const MAX = 60;
const have = new Map<string, Panel[]>();
const loading = new Map<string, Promise<Panel[]>>();
/** Bumped by `forget`, so an answer asked for before it isn't kept after it. */
let age = 0;

const name = (book: number, page: number, rtl: boolean) => `${book}:${page}:${rtl ? 1 : 0}`;

/** A page's panels if they're already here. */
export function peek(book: number, page: number, rtl: boolean): Panel[] | undefined {
  return have.get(name(book, page, rtl));
}

/** A page's panels: from here if they are, else asked for (once, however many ask). A failure
 *  isn't kept, so the next ask tries again. */
export function panelsOf(book: number, page: number, rtl: boolean): Promise<Panel[]> {
  const k = name(book, page, rtl);
  const got = have.get(k);
  if (got) return Promise.resolve(got);
  const going = loading.get(k);
  if (going) return going;
  const born = age;
  const p = api
    .pagePanels(book, page, rtl)
    .then((list) => {
      if (born === age) {
        have.set(k, list);
        if (have.size > MAX) have.delete(have.keys().next().value!);
      }
      return list;
    })
    .finally(() => loading.get(k) === p && loading.delete(k));
  loading.set(k, p);
  return p;
}

/** Drop everything, after panels were changed by hand. */
export function forget() {
  age++;
  have.clear();
  loading.clear();
}
