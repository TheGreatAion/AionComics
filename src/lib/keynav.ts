// Arrow keys on library pages: move between covers (anything marked data-nav) and press Enter to
// open the one picked. ← and → go through them in reading order; ↑ and ↓ to the nearest one in the
// row above or below.

const DIRS = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" } as const;
type Dir = (typeof DIRS)[keyof typeof DIRS];

/** Handle an arrow key; true if it moved (or kept) the pick. With `sideways` false, ← and → are
 *  left to the page until a cover is picked (Home flips its pile with them). */
export function arrowNav(e: KeyboardEvent, sideways = true): boolean {
  const dir = DIRS[e.key as keyof typeof DIRS];
  if (!dir) return false;
  const items = [...document.querySelectorAll<HTMLElement>("main [data-nav]")].filter((el) => el.getClientRects().length);
  if (!items.length) return false;
  const cur = (document.activeElement as HTMLElement | null)?.closest<HTMLElement>("[data-nav]");
  const at = cur ? items.indexOf(cur) : -1;

  let to: HTMLElement | null | undefined;
  if (at < 0) {
    // Nothing picked yet: start from the first cover in view.
    if (dir === "up" || (!sideways && dir !== "down")) return false;
    to = items.find((el) => el.getBoundingClientRect().bottom > 80) ?? items[0];
  } else if (dir === "left" || dir === "right") {
    to = items[at + (dir === "right" ? 1 : -1)];
    if (!to) return true;
  } else {
    to = nearest(items, items[at], dir);
    if (!to) {
      // Up from the top row lets go, back to the top of the page.
      if (dir === "up") {
        items[at].blur();
        window.scrollTo({ top: 0, behavior: "smooth" });
      }
      return true;
    }
  }
  to.focus({ preventScroll: true });
  to.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
  return true;
}

/** In the nearest row above or below, the one most nearly in line. */
export function nearest(items: HTMLElement[], from: HTMLElement, dir: Dir) {
  const a = from.getBoundingClientRect();
  const ax = a.left + a.width / 2;
  const sign = dir === "down" ? 1 : -1;
  const rows = items
    .filter((el) => el !== from)
    .map((el) => ({ el, r: el.getBoundingClientRect() }))
    .filter(({ r }) => (r.top - a.top) * sign > a.height / 3);
  if (!rows.length) return null;
  const row = Math.min(...rows.map(({ r }) => (r.top - a.top) * sign));
  return rows
    .filter(({ r }) => (r.top - a.top) * sign < row + a.height / 2)
    .sort((x, y) => Math.abs(x.r.left + x.r.width / 2 - ax) - Math.abs(y.r.left + y.r.width / 2 - ax))[0].el;
}

/** Enter with nothing on the page picked (no cover, no button), as opposed to Enter on a button. */
export function bareEnter(e: KeyboardEvent) {
  if (e.key !== "Enter" || e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return false;
  const el = document.activeElement;
  return !el || el === document.body || !el.closest("main");
}
