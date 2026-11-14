<script lang="ts">
  import { tick, untrack } from "svelte";
  import { slide } from "svelte/transition";
  import { addMenu } from "$lib/add.svelte";
  import CvKeyBox from "$lib/components/CvKeyBox.svelte";
  import { viewport } from "$lib/components/SidePanel.svelte";
  import { readAsItems, removing, want, wantItem } from "$lib/manage.svelte";
  import { api, chapterList, listText, plainText, remote, storyTitle, unitWord, type ComicCard, type ComicMeta, type CvBookExtras, type CvVolume, type Edition, type EditionKind, type MetaField, type SeriesView } from "$lib/api";
  import CoverViewer from "$lib/components/CoverViewer.svelte";
  import { lib, nav, openReader, reader, search, sheets, showMenu, undoable, undoableWith, type MenuItem } from "$lib/state.svelte";
  import { bareEnter, nearest } from "$lib/keynav";
  import { motionMs } from "$lib/look.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import LetterHeadline from "$lib/components/LetterHeadline.svelte";
  import InkPrint from "$lib/components/InkPrint.svelte";
  import { hue } from "$lib/color";
  import { look } from "$lib/look.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let { id, book: startBook }: { id: number; book?: number } = $props();
  let data = $state<SeriesView | null>(null);
  let synEl = $state<HTMLParagraphElement>();
  let expanded = $state(false);
  let clamped = $state(false);

  $effect(() => {
    void lib.version;
    const sid = id;
    api.series(sid).then((d) => {
      if (sid !== id) return;
      data = d;
      nav.restore();
    });
  });

  const all = $derived(data?.sections.flatMap((s) => s.comics) ?? []);
  const first = $derived(all[0]);
  // Where to start in a list of books: what's in progress, else the first unread, else the first.
  const startOf = (books: ComicCard[]) =>
    books.filter((c) => !c.read && c.currentPage > 0).sort((a, b) => (b.lastReadAt ?? 0) - (a.lastReadAt ?? 0))[0] ??
    books.find((c) => !c.read) ??
    books[0];
  const count = (e: { sections: { comics: ComicCard[] }[] }) => e.sections.reduce((n, s) => n + s.comics.length, 0);
  // The first tab, which you can change by dragging: what this series opens on.
  const front = $derived(data?.editions[0]);
  // The book shown first: where to start that format.
  const target = $derived(startOf(front?.sections.flatMap((s) => s.comics) ?? all));
  // "Makoto Yukimura · Kodansha Comics USA · 2013–2026", with the artist when it isn't the writer.
  // A long run names a dozen artists: the first few, then how many more.
  const few = (v: string | null | undefined, n: number) => {
    const names = listText(v).split(", ").filter(Boolean);
    return names.length > n ? `${names.slice(0, n).join(", ")} +${names.length - n}` : names.join(", ");
  };
  const by = $derived.by(() => {
    if (!data) return "";
    const writer = few(data.writer, 2);
    const artist = few(data.artist, 3);
    const art = artist && artist !== writer ? `art by ${artist}` : "";
    return [writer, art, data.publisher, data.years].filter(Boolean).join(" · ");
  });
  // A name that says who made it ("Daredevil by Frank Miller", "New Mutants by Zeb Wells - The
  // Complete Collection") sets the title in the letters and "by Frank Miller" in a caption box on
  // the panel's corner. Only when the "by" names someone who made the series, where that's known,
  // so "Gotham by Gaslight" and "Step by Bloody Step" stay whole.
  const headline = $derived.by(() => {
    const name = data?.name.trim() ?? "";
    const m = /^(.+?)\s+by\s+(.+?)(\s+[-–—]\s+.+)?$/i.exec(name);
    if (!m) return { title: name, by: "" };
    const made = `${listText(data?.writer)} ${listText(data?.artist)}`.toLowerCase();
    const words = m[2].match(/[a-z][a-z.']{2,}/gi) ?? [];
    if (made.trim() && !words.some((w) => made.includes(w.toLowerCase()))) return { title: name, by: "" };
    return { title: `${m[1]}${m[3] ?? ""}`, by: `by ${m[2]}` };
  });
  const findOnCv = () => data && (sheets.matching = { seriesId: data.id, isManga: data.isManga, name: data.name });

  // A series without details yet: whether Comic Vine has it, looked up quietly a moment after the
  // page opens (kept a day, so coming back costs nothing). Only a close match is offered, beside
  // the Comic Vine button, so the offer means there really is something to add. With no key to
  // look with, "Add details?" opens a box that takes one, for the whole library.
  let offer = $state<CvVolume | null>(null);
  let noKey = $state(false);
  let keyBox = $state(false);
  let keyAdded = $state(0);
  $effect(() => {
    const id = data?.id;
    const matched = !!data?.cvVolumeId;
    void keyAdded;
    offer = null;
    if (!id || matched || remote) return;
    const t = setTimeout(async () => {
      try {
        noKey = !(await api.cvStatus()).hasKey;
        if (noKey) return;
        const top = (await api.cvSearch(id)).volumes[0];
        if (top && top.score >= 0.95 && data?.id === id && !data.cvVolumeId) offer = top;
      } catch {
        // No offer; the button still searches.
      }
    }, 1200);
    return () => clearTimeout(t);
  });
  // The name is set as on collection pages: its letters cut from the covers of the format in view,
  // picked at random each visit, over one more cover printed in a single ink of its own color. That one is
  // the first cover whose color will do, preferring one printed already (each series' is, ahead of
  // time), never a teal one; with none, its first cover prints in gray. Only ever a cover, as a page
  // could give the story away. The pick holds until the format changes, even as details and prints arrive.
  const seed = Math.floor(Math.random() * 2 ** 32);
  const shuffled = (n: number) => Math.imul(n ^ seed, 2654435761) >>> 0;
  const inky = (c: ComicCard) => {
    const h = hue(c.coverColor);
    return !!h && h.chroma > 0.12 && !(h.h >= 150 && h.h <= 200);
  };
  const ink = (c: ComicCard) => (inky(c) ? hue(c.coverColor)!.h : null);
  let backdrop = $state<{ id: number; hue: number | null; at: number } | null>(null);
  $effect(() => {
    const books = inView;
    const kept = untrack(() => backdrop);
    const still = kept && books.find((c) => c.id === kept.id);
    if (still) {
      if (ink(still) !== kept.hue) backdrop = { ...kept, hue: ink(still) };
      return;
    }
    const colored = books.filter(inky);
    const c = colored.find((c) => data?.printed.includes(c.id)) ?? colored[0] ?? books[0];
    if (c) backdrop = { id: c.id, hue: ink(c), at: 20 + (shuffled(c.id) % 61) };
  });
  const letterCovers = $derived.by(() => {
    const letters = [...headline.title].filter((ch) => ch !== " ").length;
    const others = inView.filter((c) => c.id !== backdrop?.id);
    return [...(others.length ? others : inView)]
      .sort((a, b) => shuffled(a.id) - shuffled(b.id))
      .slice(0, letters)
      .map((c) => c.id);
  });
  const seriesAbout = $derived(plainText(data?.synopsis ?? null));
  const cvLabel = $derived(data?.cvVolumeId ? "Get details again from Comic Vine" : "Find details on Comic Vine");

  /* ---- The chosen book: picked in the strip, or the one you're on. */
  let picked = $state<number | null>(null);
  $effect(() => {
    void id;
    picked = startBook ?? null;
  });
  const book = $derived<ComicCard | undefined>(all.find((c) => c.id === picked) ?? target);

  /* ---- Formats (issues, volumes, omnibus, annuals…), one at a time in the strip. */
  let chosen = $state<Edition["kind"] | null>(null);
  $effect(() => {
    void id;
    chosen = null;
  });
  // Until a format is picked, the strip shows the first tab; or, when you've come here for a particular
  // book, the one that book is in.
  const edition = $derived(data?.editions.find((e) => e.kind === (chosen ?? (startBook == null ? front?.kind : null) ?? book?.edition)) ?? data?.editions[0]);
  // What's out on Comic Vine past the last one here, on the tab of the kind it counts (single
  // issues, or a manga's volumes): "#19 is out", "#19 and #20 are out", "6 new, #19 to #24".
  const newHere = $derived(
    data && data.newIssues.length && (data.editions.length <= 1 || edition?.kind === (data.isManga ? "volume" : "issue")) ? data.newIssues : [],
  );
  const newLine = $derived.by(() => {
    const mark = (n: string) => (data?.isManga ? `Vol. ${n}` : `#${n}`);
    const n = newHere.length;
    if (!n) return "";
    if (n === 1) return `${mark(newHere[0])} is out`;
    if (n <= 3) return `${newHere.slice(0, -1).map(mark).join(", ")} and ${mark(newHere[n - 1])} are out`;
    return `${n} new, ${mark(newHere[0])} to ${mark(newHere[n - 1])}`;
  });
  const inView = $derived(edition?.sections.flatMap((s) => s.comics) ?? all);
  // Only what's still unread, when asked; remembered from one series to the next.
  let unreadOnly = $state((() => { try { return localStorage.getItem("aion.unreadOnly") === "1"; } catch { return false; } })());
  function toggleUnread() {
    unreadOnly = !unreadOnly;
    many = [];
    try {
      localStorage.setItem("aion.unreadOnly", unreadOnly ? "1" : "0");
    } catch {}
  }
  const showing = $derived(
    (edition?.sections ?? data?.sections ?? [])
      .map((s) => (unreadOnly ? { name: s.name, comics: s.comics.filter((c) => !c.read) } : s))
      .filter((s) => s.comics.length),
  );
  const onShow = $derived(showing.flatMap((s) => s.comics));
  function choose(kind: Edition["kind"]) {
    const e = data?.editions.find((x) => x.kind === kind);
    if (!e) return;
    chosen = kind;
    many = [];
    const start = startOf(e.sections.flatMap((s) => s.comics));
    if (start) pick(start);
  }
  /* ---- Dragging a format along the tabs. They stay where they are in the page while they slide over
     with transforms, and take their new places only on the drop, so nothing is laid out again midway. */
  const DRAG_START = 5;
  let drag = $state<{ kind: string; at: Record<string, number>; landing: boolean } | null>(null);
  let tabsEl = $state<HTMLElement>();
  let justDragged = false;
  function grab(e: PointerEvent, kind: string) {
    if (e.button !== 0 || !tabsEl || !data || drag) return;
    const tab = e.currentTarget as HTMLElement;
    const kinds = data.editions.map((x) => x.kind as string);
    const box = Object.fromEntries(kinds.map((k, i) => [k, tabsEl!.children[i].getBoundingClientRect()]));
    const gap = box[kinds[1]].left - box[kinds[0]].right;
    const base = box[kinds[0]].left;
    const far = box[kinds[kinds.length - 1]].right - box[kind].width;
    // Where each tab sits with the tabs in this order.
    const lefts = (order: string[]) => {
      const out: Record<string, number> = {};
      let x = base;
      for (const k of order) {
        out[k] = x;
        x += box[k].width + gap;
      }
      return out;
    };
    const offsets = (left: Record<string, number>) => Object.fromEntries(kinds.map((k) => [k, left[k] - box[k].left]));
    const startX = e.clientX;
    let started = false;
    let order = kinds;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - startX;
      if (!started) {
        if (Math.abs(dx) < DRAG_START) return;
        started = true;
        tab.setPointerCapture(ev.pointerId);
      }
      const wanted = Math.max(base, Math.min(far, box[kind].left + dx));
      // The order that puts this tab's slot closest to where it's being held.
      const others = kinds.filter((k) => k !== kind);
      let best = Infinity;
      for (let i = 0; i <= others.length; i++) {
        const trial = [...others.slice(0, i), kind, ...others.slice(i)];
        const d = Math.abs(lefts(trial)[kind] - wanted);
        if (d < best) [best, order] = [d, trial];
      }
      drag = { kind, at: offsets({ ...lefts(order), [kind]: wanted }), landing: false };
    };
    const finish = (drop: boolean) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("keydown", esc, true);
      if (!started) return;
      // The click that ends a drag isn't a click on the tab.
      justDragged = true;
      setTimeout(() => (justDragged = false));
      const final = drop ? order : kinds;
      drag = { kind, at: offsets(lefts(final)), landing: true };
      setTimeout(() => {
        drag = null;
        if (final.join() !== kinds.join()) reorder(final);
      }, motionMs(130, 200));
    };
    const up = () => finish(true);
    const cancel = () => finish(false);
    const esc = (ev: KeyboardEvent) => {
      if (ev.key !== "Escape") return;
      ev.preventDefault();
      ev.stopPropagation();
      finish(false);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", esc, true);
  }
  function reorder(order: string[]) {
    if (!data) return;
    const sid = data.id;
    const moved = order[0] !== data.editions[0].kind;
    const editions = order.map((k) => data!.editions.find((x) => x.kind === k)!);
    data.editions = editions;
    data.formatOrder = order;
    undoableWith(
      moved ? `${data.name.trim()} now opens on ${editions[0].name}` : "Formats reordered",
      () => api.setFormatOrder(sid, order),
      async (before) => void (await api.setFormatOrder(sid, before)),
      () => {
        chosen = null;
        picked = null;
      },
    );
    if (moved) choose(editions[0].kind);
  }
  // The count by the series name is the format in view.
  const mainRead = $derived(inView.filter((c) => c.read).length);
  const mainCount = $derived(inView.length);
  let meta = $state<ComicMeta | null>(null);
  $effect(() => {
    void lib.version;
    const bid = book?.id;
    if (bid == null) return;
    api.comicMeta(bid).then((m) => bid === book?.id && (meta = m));
  });
  // Only this book's details: until they arrive, the previous book's aren't shown under its name.
  const f = $derived(meta && meta.id === book?.id ? meta.fields : null);

  const lines = (v: string | null | undefined) =>
    (v ?? "")
      .split("\n")
      .map((x) => x.trim())
      .filter(Boolean);
  const synopsis = $derived(plainText(f?.synopsis ?? null));
  // The chapters a manga volume holds, or what a comic collects, listed after its blurb.
  const chapters = $derived(chapterList(f?.synopsis ?? null));
  const numbered = $derived(chapters.some((c) => c.n));
  const CHAPTERS = 8;
  let allChapters = $state(false);
  const title = $derived(book ? storyTitle(book) : null);
  // Who made it, each person once with everything they did: "Jack Kirby  pencils, cover".
  const ROLES: [MetaField, string][] = [
    ["writer", "writer"],
    ["penciller", "pencils"],
    ["inker", "inks"],
    ["colorist", "colors"],
    ["letterer", "letters"],
    ["cover_artist", "cover"],
  ];
  const credits = $derived.by(() => {
    if (!f) return [];
    const people = new Map<string, string[]>();
    for (const [field, role] of ROLES) for (const name of lines(f[field])) people.set(name, [...(people.get(name) ?? []), role]);
    return [...people].map(([name, roles]) => ({ name, roles: roles.join(", ") }));
  });
  // Shown before a "+N": a big collection credits dozens of people and a hundred characters.
  const PEOPLE = 6;
  const CHIPS = 8;
  let allChips = $state(false);
  let allPeople = $state(false);
  // Comic Vine lists who's in a book alphabetically, so a big collection's first names are usually
  // walk-ons. The regulars come first instead: whoever turns up in the most books of the series
  // (and a team the series is named after before anyone).
  let cast = $state(new Map<string, number>());
  $effect(() => {
    void lib.version;
    const sid = id;
    const ids = all.slice(0, 200).map((c) => c.id);
    api.casts(ids).then((list) => {
      if (sid !== id) return;
      const seen = new Map<string, number>();
      for (const [teams, characters] of list)
        for (const n of new Set([...lines(teams), ...lines(characters)])) seen.set(n, (seen.get(n) ?? 0) + 1);
      cast = seen;
    }, () => {});
  });
  const characters = $derived.by(() => {
    const name = data?.name.toLowerCase() ?? "";
    return [...lines(f?.teams), ...lines(f?.characters)]
      .map((n, i) => ({ n, i, score: (name.includes(n.toLowerCase()) ? 1e6 : 0) + (cast.get(n) ?? 0) }))
      .sort((a, b) => b.score - a.score || a.i - b.i)
      .map((x) => x.n);
  });
  const arc = $derived.by(() => {
    if (!f?.story_arc) return "";
    const part = f.arc_number && f.arc_count ? `, part ${f.arc_number} of ${f.arc_count}` : "";
    return `${f.story_arc}${part}`;
  });
  const facts = $derived.by(() => {
    if (!book) return "";
    const year = f?.date?.slice(0, 4) ?? book.year;
    const pages = book.numPages ? `${book.numPages.toLocaleString()} pages` : "";
    return [year, pages].filter(Boolean).join(" · ");
  });
  const reading = $derived(!!book && !book.read && book.currentPage > 0);
  const action = $derived(!book ? "" : reading ? "Resume" : book.read ? "Read again" : "Read");

  // A new book starts with its blurb folded and its cast cut short.
  $effect(() => {
    void book?.id;
    expanded = false;
    allChapters = false;
    allChips = false;
    allPeople = false;
  });
  // Only offer "More" when the blurb is actually cut off.
  $effect(() => {
    void synopsis;
    if (synEl && !expanded) clamped = synEl.scrollHeight > synEl.clientHeight + 2;
  });

  async function act(fn: () => Promise<void>) {
    await fn();
    lib.version++;
  }

  function open() {
    if (!book) return;
    openReader(book.id, gallery?.querySelector<HTMLImageElement>(`[data-book="${book.id}"] img`) ?? null);
  }

  function seriesMenu(e: MouseEvent) {
    if (!data) return;
    const sid = data.id;
    const name = data.name.trim();
    const allBooks = data.sections.reduce((n, sec) => n + sec.comics.length, 0);
    showMenu(e, [
      ...(all.some((b) => !b.read) || data.wanted ? [wantItem({ series: sid }, data.wanted, name), "separator" as const] : []),
      { label: "Mark series as read", action: () => undoable(`${name} marked as read`, () => api.setSeriesRead(sid, true)) },
      { label: "Mark series as unread", action: () => undoable(`${name} marked as unread`, () => api.setSeriesRead(sid, false)) },
      "separator",
      { label: "Read as", items: readAsItems(data.reading, (r) => api.setSeriesReading(sid, r)) },
      "separator",
      { label: data.cvVolumeId ? "Match again on Comic Vine…" : "Find on Comic Vine…", action: findOnCv },
      { label: "Show in Explorer", action: () => first && api.reveal(first.id) },
      "separator",
      {
        label: "Remove…",
        action: () =>
          (removing.target = {
            series: sid,
            title: name,
            detail: `${allBooks} ${unitWord(data!.isManga, allBooks)}`,
            what: "its files",
            leaves: true,
          }),
      },
    ]);
  }

  function bookMenu(e: MouseEvent, c: ComicCard) {
    // Right-click on one of several picked books: the menu is for all of them. On any other book,
    // the picks go and it's for that one.
    if (many.includes(c.id) && many.length > 1) return manyMenu(e);
    many = [];
    const name = `${c.seriesName.trim()} ${c.label}`;
    showMenu(e, [
      { label: c.read ? "Read again" : c.currentPage > 0 ? "Resume" : "Read", action: () => openReader(c.id) },
      "separator",
      c.read
        ? { label: "Mark as unread", action: () => undoable(`${name} marked as unread`, () => api.setRead(c.id, false)) }
        : { label: "Mark as read", action: () => undoable(`${name} marked as read`, () => api.setRead(c.id, true)) },
      { label: "Mark earlier ones as read", action: () => undoable(`Books before ${c.label} marked as read`, () => api.markReadBefore(c.id)) },
      ...(!c.read && c.currentPage > 0 ? [{ label: "Remove from Continue reading", action: () => undoable(`${name} taken off`, () => api.hideFromReading(c.id)) }] : []),
      ...(c.read ? [] : ["separator" as const, data?.wantedBooks.includes(c.id) ? wantItem({ book: c.id }, true, name) : { label: "Want to read this book", action: () => want({ book: c.id }, true, name) }]),
      "separator",
      { label: "Edit details…", action: () => (sheets.editing = c.id) },
      { label: "Find on Comic Vine…", action: () => data && (sheets.matching = { seriesId: data.id, isManga: data.isManga, name: data.name, comicId: c.id, label: c.label, count: data.editions.find((x) => x.kind === c.edition)?.sections.reduce((n, s) => n + s.comics.length, 0) }) },
      { label: "Show in Explorer", action: () => api.reveal(c.id) },
      { label: "Remove…", action: () => askRemove([c], name) },
      "separator",
      // Its format, when its name gave the wrong idea. Kept in AionComics; the file isn't touched.
      formatMenu([c], name),
    ]);
  }

  // The format list, for one book or several: "Format › Volume, Issue, … Sort automatically".
  function formatMenu(books: ComicCard[], who: string): MenuItem {
    const ids = books.map((b) => b.id);
    const now = books.every((b) => b.edition === books[0].edition) ? books[0].edition : undefined;
    const set = (kind: EditionKind | null) => {
      many = [];
      undoableWith(kind ? `${who} set to ${KIND_NAME[kind]}` : `${who} sorted automatically`, () => api.setEditions(ids, kind), api.restoreEditions);
    };
    return {
      label: "Format",
      hint: now ? KIND_NAME[now] : books.length > 1 ? "Mixed" : undefined,
      items: [...KINDS.map((k) => ({ label: KIND_NAME[k], checked: now === k, action: () => set(k) })), "separator", { label: "Sort automatically", action: () => set(null) }],
    };
  }

  /* ---- Several books picked in the strip (Ctrl+click, Shift+click), changed together. */
  let many = $state<number[]>([]);
  let anchor: number | null = null;
  $effect(() => {
    void id;
    many = [];
    anchor = null;
  });
  function manyMenu(e: MouseEvent) {
    const books = all.filter((b) => many.includes(b.id));
    const who = `${books.length} books`;
    const each = (list: ComicCard[], fn: (id: number) => ReturnType<typeof api.hideFromReading>) => async () => {
      many = [];
      return (await Promise.all(list.map((b) => fn(b.id)))).flat();
    };
    const unread = books.filter((b) => !b.read);
    const read = books.filter((b) => b.read);
    const going = books.filter((b) => !b.read && b.currentPage > 0);
    showMenu(e, [
      { label: `${who} picked`, disabled: true },
      "separator",
      ...(unread.length ? [{ label: "Mark as read", action: () => undoable(`${who} marked as read`, each(unread, (id) => api.setRead(id, true))) }] : []),
      ...(read.length ? [{ label: "Mark as unread", action: () => undoable(`${who} marked as unread`, each(read, (id) => api.setRead(id, false))) }] : []),
      ...(going.length ? [{ label: "Remove from Continue reading", action: () => undoable(`${going.length} books taken off`, each(going, api.hideFromReading)) }] : []),
      { label: "Remove…", action: () => askRemove(books, who) },
      "separator",
      formatMenu(books, who),
    ]);
  }
  // The Remove box for these books. Taking every book there is takes the page with it.
  function askRemove(books: ComicCard[], title: string) {
    if (!data) return;
    const all = data.sections.reduce((n, sec) => n + sec.comics.length, 0);
    const one = books.length === 1;
    many = [];
    removing.target = {
      books: books.map((b) => b.id),
      title,
      detail: `${books.length} ${unitWord(data.isManga, books.length)}`,
      what: one ? "the file" : "the files",
      leaves: books.length >= all,
    };
  }
  function clickPick(e: MouseEvent, c: ComicCard) {
    if (e.ctrlKey || e.metaKey) {
      // Starting from the book on show, as Explorer starts from the one selected.
      const base = many.length ? many : book ? [book.id] : [];
      many = base.includes(c.id) ? base.filter((x) => x !== c.id) : [...base, c.id];
      anchor = c.id;
    } else if (e.shiftKey) {
      const ids = onShow.map((x) => x.id);
      const from = ids.indexOf(anchor ?? book?.id ?? c.id);
      const to = ids.indexOf(c.id);
      many = from < 0 ? [c.id] : ids.slice(Math.min(from, to), Math.max(from, to) + 1);
    } else {
      many = [];
      // The book whose details are open closes them; any other opens its own.
      if (c.id === book?.id && !shut) shut = true;
      else {
        shut = false;
        pick(c);
      }
    }
  }
  const KINDS: EditionKind[] = ["volume", "issue", "chapter", "collection", "deluxe", "omnibus", "annual", "special"];
  const KIND_NAME: Record<Edition["kind"], string> = {
    order: "In order",
    volume: "Volume",
    issue: "Issue",
    chapter: "Chapter",
    collection: "Collection",
    deluxe: "Deluxe edition",
    omnibus: "Omnibus",
    annual: "Annual",
    special: "Special",
  };

  /* ---- Every book as a big cover, a row at a time; the chosen one's details open under its row. */
  let gallery = $state<HTMLElement>();
  let shut = $state(false);
  $effect(() => {
    void id;
    shut = false;
  });
  const drawer = $derived(!!book && !shut);

  // From Comic Vine, for a book matched to an issue: its other covers, and the characters who
  // first appear in it. Asked for once the details have been open a moment; the first time for a
  // book it can take a few seconds (one question per character), and nothing shows until then.
  let extras = $state<CvBookExtras | null>(null);
  let viewing = $state<number | null>(null);
  $effect(() => {
    const id = drawer ? book?.id : undefined;
    extras = null;
    viewing = null;
    if (!id) return;
    const t = setTimeout(() => {
      api
        .cvBookExtras(id)
        .then((x) => {
          if (book?.id === id) extras = x;
        })
        .catch(() => {});
    }, 300);
    return () => clearTimeout(t);
  });
  const COVERS = 9;
  async function pick(c: ComicCard) {
    picked = c.id;
    anchor = c.id;
    await tick();
    const card = gallery?.querySelector<HTMLElement>(`[data-book="${c.id}"]`);
    card?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  // How big the covers are: Ctrl + the mouse wheel, remembered.
  const SIZE_KEY = "aion.cover-size";
  // A phone fits three covers across, closer together.
  const GAP = $derived(viewport.phone ? 14 : 26);
  let size = $state(Number((() => { try { return localStorage.getItem(SIZE_KEY); } catch { return null; } })()) || 190);
  let galleryW = $state(0);
  const cols = $derived(Math.max(1, Math.floor((galleryW + GAP) / ((viewport.phone ? 100 : size) + GAP))));
  const trackW = $derived((galleryW - GAP * (cols - 1)) / cols);
  function onwheel(e: WheelEvent) {
    if (!e.ctrlKey || reader.id !== null) return;
    e.preventDefault();
    size = Math.max(130, Math.min(340, size - Math.sign(e.deltaY) * 20));
    try {
      localStorage.setItem(SIZE_KEY, String(size));
    } catch {}
  }
  $effect(() => {
    window.addEventListener("wheel", onwheel, { passive: false });
    return () => window.removeEventListener("wheel", onwheel);
  });

  // A book's place in a list. By id: a format's lists and the whole series' are separate copies.
  const at = (books: ComicCard[], c: ComicCard | undefined) => (c ? books.findIndex((b) => b.id === c.id) : -1);
  // Where the details go in a section: after the last cover in the chosen book's row.
  const rowEnd = (books: ComicCard[]) => {
    const i = at(books, book);
    return i < 0 ? -1 : Math.min(Math.floor(i / cols) * cols + cols - 1, books.length - 1);
  };
  const notch = (books: ComicCard[]) => (Math.max(0, at(books, book)) % cols) * (trackW + GAP) + trackW / 2;

  // A fresh visit (not Back) to a long series scrolls the book you're on into view.
  let landed = $state<number | null>(null);
  $effect(() => {
    if (!data || !book || landed === id || !gallery) return;
    landed = id;
    const fresh = !nav.stack[nav.stack.length - 1].scroll;
    tick().then(() => {
      const card = gallery?.querySelector<HTMLElement>(`[data-book="${book!.id}"]`);
      if (fresh && card && card.getBoundingClientRect().bottom > window.innerHeight) card.scrollIntoView({ block: "center" });
    });
  });

  // ← → step through the books, ↑ ↓ a row, Enter opens the one picked, Esc closes its details.
  function onkey(e: KeyboardEvent) {
    if (reader.id !== null || search.open || sheets.editing !== null || sheets.matching || e.defaultPrevented) return;
    if (e.key === "Escape" && (many.length || drawer)) {
      e.preventDefault();
      if (many.length) many = [];
      else shut = true;
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    if ((e.target as HTMLElement)?.closest?.("input, textarea, select, [contenteditable]")) return;
    // Enter with nothing picked, or on a cover (after clicking it), reads the book on show.
    if (book && (bareEnter(e) || (e.key === "Enter" && (e.target as HTMLElement)?.closest?.(".card")))) {
      e.preventDefault();
      return open();
    }
    if (!book) return;
    let to: ComicCard | undefined;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") to = onShow[at(onShow, book) + (e.key === "ArrowRight" ? 1 : -1)];
    else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      const cards = [...(gallery?.querySelectorAll<HTMLElement>("[data-book]") ?? [])];
      const from = cards.find((el) => el.dataset.book === String(book!.id));
      const el = from && nearest(cards, from, e.key === "ArrowDown" ? "down" : "up");
      to = el ? all.find((c) => String(c.id) === el.dataset.book) : undefined;
    } else return;
    e.preventDefault();
    if (to) pick(to);
  }
</script>

<svelte:window onkeydown={onkey} />

{#if data}
  <div class="screen">
    <!-- The name as a comic panel, as on collection pages: inked frame, how many and how far in a
         caption box on its corner, a cover printed behind the letters. -->
    <div class="top" class:inked={look.backdrop && backdrop}>
      <div class="caption count">
        {mainCount.toLocaleString()} {!edition ? unitWord(data.isManga, mainCount) : mainCount === 1 ? KIND_NAME[edition.kind] : edition.name} · {mainRead === mainCount && mainCount ? "all read" : `${mainRead.toLocaleString()} read`}
      </div>
      {#if look.backdrop && backdrop}
        <InkPrint id={backdrop.id} hue={backdrop.hue} at={backdrop.at} />
      {/if}
      <LetterHeadline text={headline.title} covers={letterCovers} maxLines={4} />
      {#if headline.by}<div class="caption byline">{headline.by}</div>{/if}
    </div>
    <!-- Where it lives and who made it; fetching its details, and more. -->
    <header class="series">
      <div class="names">
        <button class="crumb link" onclick={() => nav.go({ name: "collection", id: data!.collectionId })}>{data.collectionName}</button>
        {#if by}<span class="by">· {by}</span>{/if}
      </div>
      <div class="tools">
        <!-- On your Want to read list, or not: filled in when it is. -->
        <button
          class="ico"
          class:on={data.wanted}
          title={data.wanted ? "On your Want to read list. Press to take it off" : all.every((b) => b.read) ? "You've read it all" : "Want to read"}
          aria-label="Want to read"
          aria-pressed={data.wanted}
          disabled={!data.wanted && all.every((b) => b.read)}
          onclick={() => want({ series: data!.id }, !data!.wanted, data!.name.trim())}
        >
          <Icon name={data.wanted ? "bookmark" : "bookmarkOutline"} size={18} />
        </button>
        <button class="ico" class:on={unreadOnly} title="Only what's still unread" aria-label="Only what's still unread" aria-pressed={unreadOnly} onclick={toggleUnread}>
          <Icon name="unread" size={18} />
        </button>
        <button
          class="ico pc-only"
          title="Add {unitWord(data.isManga, 2)}"
          aria-label="Add {unitWord(data.isManga, 2)}"
          onclick={(e) => addMenu(e, { series: data!.id, title: data!.name })}
        >
          <Icon name="plus" size={18} />
        </button>
        <!-- Always here, so a series can fetch its details again (a new issue, a wrong match). -->
        <span class="cvspot">
        {#if noKey && !data.cvVolumeId}
          <button
            class="offer cvkey-toggle pc-only"
            class:open={keyBox}
            title="Comic Vine can fill in this series' details"
            aria-expanded={keyBox}
            onclick={() => (keyBox = !keyBox)}
          >
            Add details?
          </button>
        {:else if offer}
          <button
            class="offer pc-only"
            title="Comic Vine has {offer.name}{offer.startYear ? ` (${offer.startYear}${offer.publisher ? `, ${offer.publisher}` : ''})` : ''}"
            onclick={findOnCv}
          >
            Want to add details?
          </button>
        {/if}
        <button class="ico pc-only" title={cvLabel} aria-label={cvLabel} onclick={findOnCv}>
          <Icon name="fetch" size={18} />
        </button>
        {#if keyBox}
          <CvKeyBox
            name={data.name}
            cover={inView[0]?.id ?? all[0]?.id}
            onclose={() => {
              keyBox = false;
              keyAdded++;
            }}
            onfind={() => {
              keyBox = false;
              keyAdded++;
              findOnCv();
            }}
          />
        {/if}
        </span>
        <button class="ico pc-only" title="More" aria-label="More" onclick={seriesMenu} oncontextmenu={seriesMenu}><Icon name="more" size={18} /></button>
      </div>
    </header>


    {#if data.backfill}
      {@const b = data.backfill}
      <p class="backfill pc-only" title="You're on {b.beforeLabel}">
        {b.count} earlier {unitWord(data.isManga, b.count)} read?
        <button class="ico" title="Mark them read" aria-label="Mark them read" onclick={() => undoable(`${b.count} earlier ${unitWord(data!.isManga, b.count)} marked as read`, () => api.markReadBefore(b.beforeId))}>
          <Icon name="check" size={16} />
        </button>
        <button class="ico" title="Not now" aria-label="Not now" onclick={() => act(() => api.dismissBackfill(data!.id))}>
          <Icon name="close" size={15} />
        </button>
      </p>
    {/if}

    <!-- One format at a time; what's picked with Ctrl or Shift; what's missing. -->
    {#if data.editions.length > 1 || many.length || edition?.missing.length || newHere.length}
      <div class="bar">
        {#if data.editions.length > 1}
          <div class="tabs" class:dragging={drag && !drag.landing} class:landing={drag?.landing} role="tablist" aria-label="Formats" bind:this={tabsEl}>
            {#each data.editions as e (e.kind)}
              <button
                class="tab"
                class:on={e.kind === edition?.kind}
                class:held={drag?.kind === e.kind}
                role="tab"
                aria-selected={e.kind === edition?.kind}
                title="Drag to reorder. This series opens on the first one."
                style:transform={drag ? `translateX(${drag.at[e.kind]}px)` : undefined}
                onpointerdown={(ev) => grab(ev, e.kind)}
                onclick={() => !justDragged && choose(e.kind)}
              >
                {e.name}<span class="n fine">{count(e)}</span>
              </button>
            {/each}
          </div>
        {/if}
        {#if many.length}
          <span class="picked fine" role="status">
            {many.length} picked · right-click to change them
            <button class="ico" title="Clear (Esc)" aria-label="Clear the picks" onclick={() => (many = [])}><Icon name="close" size={13} /></button>
          </span>
        {:else if edition?.missing.length || newHere.length}
          <span class="missing fine">
            {#if newHere.length}<span class="new" title="On Comic Vine, after the last one you have">{newLine}</span>{/if}
            {#if newHere.length && edition?.missing.length}<span class="sep">·</span>{/if}
            {#if edition?.missing.length}<span title="Not in your library">{edition.missing.map((n) => `#${n}`).join(", ")} missing</span>{/if}
          </span>
        {/if}
      </div>
    {/if}

    <!-- Every book as a cover. Click one for its details under its row (again to close them),
         double-click to read; Ctrl and Shift pick several. -->
    <nav class="gallery" class:multi={many.length > 0} bind:this={gallery} bind:clientWidth={galleryW} aria-label="Books in this series">
      {#if !showing.length && unreadOnly && all.length}
        <p class="all-read">
          You've read all of these.
          <button class="link" onclick={toggleUnread}>Show them</button>
        </p>
      {/if}
      {#each showing as s, si (si)}
        {@const end = drawer ? rowEnd(s.comics) : -1}
        {@const titled = s.comics.some((c) => storyTitle(c))}
        {#if s.name}<div class="gname">{s.name}</div>{/if}
        <div class="grid" style:grid-template-columns="repeat({cols}, minmax(0, 1fr))" style:column-gap="{GAP}px">
          {#each s.comics as c, j (c.id)}
            <button
              class="card"
              class:on={c.id === book?.id}
              class:open={c.id === book?.id && drawer}
              class:done={c.read}
              class:sel={many.includes(c.id)}
              data-book={c.id}
              title="{c.label}{storyTitle(c) ? ` · ${storyTitle(c)}` : ''}"
              aria-pressed={many.length ? many.includes(c.id) : undefined}
              aria-expanded={c.id === book?.id ? drawer : undefined}
              onclick={(e) => clickPick(e, c)}
              ondblclick={() => openReader(c.id, gallery?.querySelector<HTMLImageElement>(`[data-book="${c.id}"] img`) ?? null)}
              oncontextmenu={(e) => bookMenu(e, c)}
            >
              <span class="art">
                <Cover id={c.id} ratio={c.coverRatio} />
                {#if c.read}<span class="tick"><Icon name="check" size={12} stroke={2.5} /></span>{/if}
                {#if many.includes(c.id)}<span class="mark"><Icon name="check" size={12} stroke={2.5} /></span>{/if}
                {#if !c.read && c.currentPage > 0 && c.numPages}
                  <span class="line"><i style:width="{((c.currentPage + 1) / c.numPages) * 100}%"></i></span>
                {/if}
              </span>
              <span class="cap fine">{c.label}{#if c.collects}<span class="holds">{c.collects}</span>{:else if c.after}<span class="holds" title="Comes after {c.after} in the story">after {c.after}</span>{/if}</span>
              <!-- Where any book in a section has a title, each keeps two lines for one, so covers stay in line. -->
              {#if titled}<span class="name">{storyTitle(c) ?? ""}</span>{/if}
            </button>
            {#if j === end && book}
              <section class="drawer" style:--notch="{notch(s.comics)}px" in:slide={{ duration: motionMs(160, 260) }}>
                <button class="ico close" title="Close (Esc)" aria-label="Close the details" onclick={() => (shut = true)}><Icon name="close" size={16} /></button>
                <div class="about" class:dim={!f}>
                  <!-- A book with its own title (from Comic Vine or its ComicInfo.xml) is headed by it, its number above. -->
                  {#if title}
                    <div class="label small">{book.label}</div>
                    <h2 class="title" class:long={title.length > 40}>{title}</h2>
                  {:else}
                    <h2 class="label">{book.label}</h2>
                  {/if}
                  <div class="facts fine">
                    {facts}{#if arc}<span class="arc">{arc}</span>{/if}
                  </div>
                  {#if synopsis}
                    <p class="syn" class:open={expanded} bind:this={synEl}>{synopsis}</p>
                    {#if clamped || expanded}
                      <button class="more link" onclick={() => (expanded = !expanded)}>{expanded ? "Less" : "More"}</button>
                    {/if}
                  {/if}
                  {#if chapters.length}
                    <ol class="chapters">
                      {#each allChapters || chapters.length <= CHAPTERS + 2 ? chapters : chapters.slice(0, CHAPTERS) as ch, k (k)}
                        <li>{#if numbered}<span class="n fine">{ch.n ?? ""}</span>{/if}<span class="t">{ch.title}</span></li>
                      {/each}
                    </ol>
                    {#if chapters.length > CHAPTERS + 2 && !allChapters}
                      <button class="link more" onclick={() => (allChapters = true)}>+{chapters.length - CHAPTERS}</button>
                    {/if}
                  {/if}
                  {#if credits.length}
                    <div class="credits">
                      {#each allPeople ? credits : credits.slice(0, PEOPLE) as p (p.name)}
                        <button class="person" title="Everything by {p.name}" onclick={() => nav.go({ name: "people", kind: "creator", value: p.name })}>
                          {p.name} <em>{p.roles}</em>
                        </button>
                      {/each}
                      {#if credits.length > PEOPLE && !allPeople}
                        <button class="link more-people" onclick={() => (allPeople = true)}>+{credits.length - PEOPLE}</button>
                      {/if}
                    </div>
                  {/if}
                  {#if characters.length}
                    <div class="chips">
                      {#each allChips ? characters : characters.slice(0, CHIPS) as ch (ch)}
                        <button class="chip" title="Everything with {ch}" onclick={() => nav.go({ name: "people", kind: "character", value: ch })}>{ch}</button>
                      {/each}
                      {#if characters.length > CHIPS && !allChips}
                        <button class="chip more-chips" onclick={() => (allChips = true)}>+{characters.length - CHIPS}</button>
                      {/if}
                    </div>
                  {/if}
                  {#if extras?.firsts.length}
                    <div class="extra">
                      <div class="xlabel fine">First appearance of</div>
                      <div class="faces">
                        {#each extras.firsts as f (f.name)}
                          <button class="face" title="Everything with {f.name}" onclick={() => nav.go({ name: "people", kind: "character", value: f.name })}>
                            {#if f.image}<img src={f.image} alt="" loading="lazy" />{:else}<span class="noface"></span>{/if}
                            <span>{f.name}</span>
                          </button>
                        {/each}
                      </div>
                    </div>
                  {/if}
                  {#if extras?.covers.length}
                    <div class="extra">
                      <div class="xlabel fine">Covers <span>{extras.covers.length}</span></div>
                      <div class="covers">
                        {#each extras.covers.slice(0, COVERS) as c, i (c.small)}
                          <button title="Cover {i + 1}" onclick={() => (viewing = i)}><img src={c.small} alt="" loading="lazy" /></button>
                        {/each}
                        {#if extras.covers.length > COVERS}
                          <button class="link more-covers" onclick={() => (viewing = COVERS)}>+{extras.covers.length - COVERS}</button>
                        {/if}
                      </div>
                    </div>
                  {/if}
                  <div class="actions">
                    <button class="btn play" onclick={open}><Icon name="play" size={14} />{action}</button>
                    {#if reading && book.numPages}
                      <span class="where fine">
                        <span class="prog"><i style:width="{((book.currentPage + 1) / book.numPages) * 100}%"></i></span>
                        {book.currentPage + 1} / {book.numPages}
                      </span>
                    {:else if book.read}
                      <span class="where fine"><Icon name="check" size={13} />Read</span>
                    {/if}
                    <button class="ico pc-only" title="More" aria-label="More for this book" onclick={(e) => bookMenu(e, book!)}><Icon name="more" size={18} /></button>
                  </div>
                </div>
              </section>
            {/if}
          {/each}
        </div>
      {/each}
    </nav>
  </div>
{/if}


{#if viewing !== null && extras?.covers.length && book && data}
  <CoverViewer covers={extras.covers} at={viewing} title="{data.name.trim()} {book.label}" onclose={() => (viewing = null)} />
{/if}

<style>
  .screen {
    position: relative;
    isolation: isolate;
    min-height: 100vh;
    padding-bottom: 80px;
  }
  /* The headline's panel (see LetterHeadline and InkPrint), as on collection pages. A long name
     sets small, so the panel keeps a height of its own and the name sits in the middle of it. */
  .top {
    position: relative;
    margin: 64px var(--pad) 0;
    padding: 44px 24px 40px;
    min-height: 380px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    border: 4px solid var(--ink);
    background: var(--raised);
  }
  .top > :global(.headline) {
    position: relative;
    z-index: 1;
  }
  .caption.count {
    position: absolute;
    left: -4px;
    top: -4px;
    z-index: 2;
  }
  /* Who made it, from the series' name, on the opposite corner. */
  .caption.byline {
    position: absolute;
    right: -4px;
    bottom: -4px;
    z-index: 2;
  }
  .series {
    margin-top: 18px;
    padding: 0 var(--pad);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 32px;
  }
  .names {
    min-width: 0;
    font-size: 14px;
  }
  .crumb {
    font-size: 14px;
  }
  .by {
    color: var(--text-2);
  }
  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .offer {
    margin-right: -2px;
    padding: 4px 4px 4px 8px;
    font-size: 13px;
    color: var(--text-2);
    white-space: nowrap;
    animation: offer-in 0.4s var(--ease);
  }
  .cvspot {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .offer:hover,
  .offer.open {
    color: var(--text);
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  @keyframes offer-in {
    from {
      opacity: 0;
    }
  }
  .tools .ico.on {
    background: rgb(var(--fg) / 0.09);
    color: var(--text);
  }
  .all-read {
    color: var(--text-2);
    padding: 40px 0;
  }
  .backfill {
    margin: 16px var(--pad) 0;
    color: var(--text-2);
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* Tabs for the formats ("Volumes 12  Omnibus 4"), and what's picked or missing on the right. */
  .bar {
    margin: 26px var(--pad) 0;
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 34px;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .tab {
    display: inline-flex;
    align-items: baseline;
    gap: 8px;
    padding: 6px 14px;
    border-radius: 4px;
    color: var(--text-2);
    font-size: 13px;
    transition:
      background 0.15s,
      color 0.15s;
  }
  .tab:hover {
    color: var(--text);
  }
  /* While one's being dragged, the others slide aside for it, and it settles into its slot when let go. */
  .tabs.dragging,
  .tabs.dragging .tab {
    cursor: grabbing;
  }
  .tabs.dragging .tab:not(.held),
  .tabs.landing .tab {
    transition:
      transform 0.13s var(--ease),
      background 0.15s,
      color 0.15s;
  }
  .tab.held {
    position: relative;
    z-index: 1;
    background: rgb(var(--fg) / 0.14);
    color: var(--text);
  }
  .tab.on {
    background: rgb(var(--fg) / 0.09);
    color: var(--text);
  }
  .tab .n {
    font-size: 12px;
    color: var(--text-3);
  }
  .tab.on .n {
    color: var(--text-2);
  }
  .missing,
  .picked {
    margin-left: auto;
    font-size: 12px;
    color: var(--text-2);
  }
  .missing .new {
    color: var(--text);
  }
  .missing .sep {
    margin: 0 8px;
    color: var(--text-3);
  }
  .picked {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .gallery {
    display: block;
    margin: 26px var(--pad) 0;
  }
  .gname {
    margin: 22px 0 14px;
    font-size: 13px;
    color: var(--text-2);
  }
  .gname:first-child {
    margin-top: 0;
  }
  .grid {
    display: grid;
    row-gap: 34px;
    align-items: end;
  }
  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    min-width: 0;
    text-align: left;
  }
  .art {
    position: relative;
    display: block;
    transition: transform 0.4s var(--ease);
  }
  .art :global(.cover) {
    box-shadow: 0 18px 40px -18px rgba(0, 0, 0, 0.9);
    transition:
      box-shadow 0.25s,
      filter 0.25s;
  }
  .card.done .art :global(.cover) {
    filter: brightness(0.86);
  }
  .card:hover .art {
    transform: translateY(-4px);
  }
  .card.on .art {
    transform: translateY(-8px);
  }
  /* The book on show (or picked with Ctrl or Shift) is ringed. While some are picked, the one on
     show is only lifted. */
  .card.on .art :global(.cover),
  .card.sel .art :global(.cover) {
    filter: none;
    box-shadow:
      0 0 0 3px var(--text),
      0 24px 50px -18px rgba(0, 0, 0, 0.95);
  }
  .gallery.multi .card.on:not(.sel) .art :global(.cover) {
    box-shadow: 0 24px 50px -18px rgba(0, 0, 0, 0.95);
  }
  /* Read: a tick in the corner, the art only a shade darker. */
  .tick {
    position: absolute;
    right: 8px;
    bottom: 8px;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: rgba(20, 19, 18, 0.85);
    color: #eaeaea;
  }
  .mark {
    position: absolute;
    top: -9px;
    right: -9px;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--text);
    color: var(--bg);
  }
  .line {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 4px;
    background: rgba(0, 0, 0, 0.55);
  }
  .line i {
    position: absolute;
    inset: 0 auto 0 0;
    background: #eaeaea;
  }
  .cap {
    margin-top: 10px;
    display: flex;
    gap: 10px;
    align-items: baseline;
    font-size: 12px;
    color: var(--text-2);
  }
  .card.on .cap {
    color: var(--text);
  }
  /* The book's own title, under its number: two lines at most. */
  .name {
    margin-top: 4px;
    font-size: 13px;
    line-height: 1.35;
    min-height: 2.7em;
    color: var(--text-2);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  .card.on .name {
    color: var(--text);
  }
  /* What a collection holds: "#13–18". */
  .holds {
    font-size: 12px;
    color: var(--text-3);
  }

  /* The chosen book's details, the width of the page under its row, a notch pointing up at it. */
  .drawer {
    grid-column: 1 / -1;
    position: relative;
    margin-top: -8px;
    padding: 30px 36px 32px;
    background: rgba(0, 0, 0, 0.3);
    border-top: 1px solid rgb(var(--fg) / 0.08);
  }
  .drawer::before {
    content: "";
    position: absolute;
    top: -12px;
    left: calc(var(--notch) - 12px);
    border: 12px solid transparent;
    border-top: 0;
    border-bottom-color: rgba(0, 0, 0, 0.3);
  }
  :global(:root[data-theme="light"]) .drawer {
    background: rgb(var(--fg) / 0.05);
  }
  :global(:root[data-theme="light"]) .drawer::before {
    border-bottom-color: rgb(var(--fg) / 0.05);
  }
  .close {
    position: absolute;
    top: 14px;
    right: 14px;
  }
  .about {
    min-width: 0;
    max-width: 80ch;
    transition: opacity 0.25s;
  }
  .about.dim {
    opacity: 0.6;
  }
  .label {
    font-family: var(--display);
    font-weight: 400;
    font-size: 52px;
    line-height: 0.9;
    letter-spacing: 0.005em;
    /* A file name with no spaces would otherwise run off the page. */
    overflow-wrap: anywhere;
  }
  /* Over a title, the number is a quiet line in the text face. */
  .label.small {
    font-family: var(--font);
    font-weight: 400;
    font-size: 20px;
    line-height: 1;
    letter-spacing: -0.01em;
    color: var(--text-2);
  }
  .title {
    margin-top: 8px;
    font-family: var(--display);
    font-weight: 400;
    font-size: 52px;
    line-height: 0.92;
    letter-spacing: 0.005em;
    color: var(--text);
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .title.long {
    font-size: 40px;
  }
  .facts {
    margin-top: 12px;
    color: var(--text-2);
    display: flex;
    flex-wrap: wrap;
    gap: 4px 18px;
  }
  .arc {
    color: var(--text-2);
  }
  .syn {
    margin-top: 16px;
    color: var(--text-2);
    font-size: 15px;
    line-height: 1.65;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    user-select: text;
  }
  .syn.open {
    display: block;
  }
  .more {
    margin-top: 6px;
    font-size: 13px;
  }
  /* A volume's chapters: number, then title, in two columns. */
  .chapters {
    margin: 16px 0 0;
    padding: 0;
    list-style: none;
    columns: 2;
    column-gap: 32px;
    font-size: 14px;
    user-select: text;
  }
  .chapters li {
    display: flex;
    gap: 12px;
    padding: 3px 0;
    break-inside: avoid;
    min-width: 0;
  }
  .chapters .n {
    flex: none;
    min-width: 3ch;
    text-align: right;
    color: var(--text-3);
    font-size: 12px;
    padding-top: 2px;
  }
  .chapters .t {
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .credits {
    margin-top: 16px;
    display: flex;
    flex-wrap: wrap;
    gap: 6px 22px;
    font-size: 14px;
    color: var(--text);
  }
  .more-people {
    font-size: 13px;
  }
  .person {
    color: inherit;
    font: inherit;
    transition: color 0.15s;
  }
  .person:hover {
    color: var(--strong);
    text-decoration: underline;
    text-underline-offset: 4px;
    text-decoration-color: var(--text-3);
  }
  .credits em {
    font-style: normal;
    color: var(--text-3);
    margin-left: 4px;
  }
  /* From Comic Vine: who first appears here, and the issue's covers. */
  .extra {
    margin-top: 18px;
  }
  .xlabel {
    margin-bottom: 8px;
    color: var(--text-3);
  }
  .xlabel span {
    margin-left: 6px;
  }
  .faces {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
  }
  .face {
    width: 72px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: 12px;
    text-align: center;
  }
  .face:hover {
    color: var(--text);
  }
  .face img,
  .noface {
    width: 56px;
    height: 56px;
    border-radius: 50%;
    object-fit: cover;
    object-position: top;
    box-shadow: 0 0 0 1px var(--line);
    background: var(--menu);
  }
  .covers {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .covers img {
    height: 96px;
    width: auto;
    /* A wraparound cover is cropped to a cover's width here; the viewer shows it whole. */
    max-width: 66px;
    object-fit: cover;
    border-radius: 2px;
    display: block;
    box-shadow: 0 0 0 1px var(--line);
    transition: transform 0.15s var(--ease);
  }
  .covers button:hover img {
    transform: translateY(-2px);
  }
  .more-covers {
    font-size: 13px;
    padding: 0 6px;
  }
  .chips {
    margin-top: 12px;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    padding: 3px 11px;
    border: 1px solid rgb(var(--fg) / 0.14);
    border-radius: 999px;
    color: var(--text-2);
    font-size: 12px;
  }
  .chip:hover,
  .more-chips:hover {
    color: var(--text);
    border-color: rgb(var(--fg) / 0.32);
  }
  .actions {
    margin-top: 22px;
    display: flex;
    align-items: center;
    gap: 22px;
  }
  .play {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }
  .where {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    color: var(--text-2);
  }
  .where .prog {
    width: 120px;
  }
</style>
