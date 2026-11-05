<script lang="ts">
  import { tick, untrack } from "svelte";
  import { slide } from "svelte/transition";
  import { addMenu } from "$lib/add.svelte";
  import CvNudge from "$lib/components/CvNudge.svelte";
  import { readAsItems, removing } from "$lib/manage.svelte";
  import { api, chapterList, listText, plainText, storyTitle, unitWord, type ComicCard, type ComicMeta, type EditionKind, type MetaField, type SeriesView } from "$lib/api";
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
  // The book shown first: the one you're reading, else where to start the biggest format.
  const target = $derived.by(() => {
    const going = startOf(all);
    if (going && going.currentPage > 0 && !going.read) return going;
    const biggest = [...(data?.editions ?? [])].sort((a, b) => count(b) - count(a))[0];
    return biggest ? startOf(biggest.sections.flatMap((s) => s.comics)) : going;
  });
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
  // The name is set as on collection pages: its letters cut from the series' covers, picked at
  // random each visit, over one more cover printed in a single ink of its own color. That one is
  // the first cover whose color will do, preferring one printed already (each series' is, ahead of
  // time), never a teal one; with none, its first cover prints in gray. Only ever a cover, as a page
  // could give the story away. The pick holds while you're here, even as details and prints arrive.
  const seed = Math.floor(Math.random() * 2 ** 32);
  const shuffled = (n: number) => Math.imul(n ^ seed, 2654435761) >>> 0;
  const inky = (c: ComicCard) => {
    const h = hue(c.coverColor);
    return !!h && h.chroma > 0.12 && !(h.h >= 150 && h.h <= 200);
  };
  const ink = (c: ComicCard) => (inky(c) ? hue(c.coverColor)!.h : null);
  let backdrop = $state<{ id: number; hue: number | null; at: number } | null>(null);
  $effect(() => {
    const books = all;
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
    const others = all.filter((c) => c.id !== backdrop?.id);
    return [...(others.length ? others : all)]
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
  let chosen = $state<EditionKind | null>(null);
  $effect(() => {
    void id;
    chosen = null;
  });
  // Until a format is picked, the strip shows the one the chosen book is in.
  const edition = $derived(data?.editions.find((e) => e.kind === (chosen ?? book?.edition)) ?? data?.editions[0]);
  const inView = $derived(edition?.sections.flatMap((s) => s.comics) ?? all);
  function choose(kind: EditionKind) {
    const e = data?.editions.find((x) => x.kind === kind);
    if (!e) return;
    chosen = kind;
    many = [];
    const start = startOf(e.sections.flatMap((s) => s.comics));
    if (start) pick(start);
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
      const ids = inView.map((x) => x.id);
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
  const KIND_NAME: Record<EditionKind, string> = {
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
  async function pick(c: ComicCard) {
    picked = c.id;
    anchor = c.id;
    await tick();
    const card = gallery?.querySelector<HTMLElement>(`[data-book="${c.id}"]`);
    card?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  // How big the covers are: Ctrl + the mouse wheel, remembered.
  const SIZE_KEY = "aion.cover-size";
  const GAP = 26;
  let size = $state(Number((() => { try { return localStorage.getItem(SIZE_KEY); } catch { return null; } })()) || 190);
  let galleryW = $state(0);
  const cols = $derived(Math.max(1, Math.floor((galleryW + GAP) / (size + GAP))));
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
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") to = inView[at(inView, book) + (e.key === "ArrowRight" ? 1 : -1)];
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
        <button
          class="ico"
          title="Add {unitWord(data.isManga, 2)}"
          aria-label="Add {unitWord(data.isManga, 2)}"
          onclick={(e) => addMenu(e, { series: data!.id, title: data!.name })}
        >
          <Icon name="plus" size={18} />
        </button>
        <!-- Always here, so a series can fetch its details again (a new issue, a wrong match). -->
        <button class="ico" title={cvLabel} aria-label={cvLabel} onclick={findOnCv}>
          <Icon name="fetch" size={18} />
        </button>
        <button class="ico" title="More" aria-label="More" onclick={seriesMenu} oncontextmenu={seriesMenu}><Icon name="more" size={18} /></button>
      </div>
    </header>

    {#if !data.cvVolumeId}<CvNudge onsaved={findOnCv} />{/if}

    {#if data.backfill}
      {@const b = data.backfill}
      <p class="backfill" title="You're on {b.beforeLabel}">
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
    {#if data.editions.length > 1 || many.length || edition?.missing.length}
      <div class="bar">
        {#if data.editions.length > 1}
          <div class="tabs" role="tablist" aria-label="Formats">
            {#each data.editions as e (e.kind)}
              <button class="tab" class:on={e.kind === edition?.kind} role="tab" aria-selected={e.kind === edition?.kind} onclick={() => choose(e.kind)}>
                {e.name}<span class="n mono">{count(e)}</span>
              </button>
            {/each}
          </div>
        {/if}
        {#if many.length}
          <span class="picked mono" role="status">
            {many.length} picked · right-click to change them
            <button class="ico" title="Clear (Esc)" aria-label="Clear the picks" onclick={() => (many = [])}><Icon name="close" size={13} /></button>
          </span>
        {:else if edition?.missing.length}
          <span class="missing mono" title="Not in your library">{edition.missing.map((n) => `#${n}`).join(", ")} missing</span>
        {/if}
      </div>
    {/if}

    <!-- Every book as a cover. Click one for its details under its row (again to close them),
         double-click to read; Ctrl and Shift pick several. -->
    <nav class="gallery" class:multi={many.length > 0} bind:this={gallery} bind:clientWidth={galleryW} aria-label="Books in this series">
      {#each edition?.sections ?? data.sections as s, si (si)}
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
              <span class="cap mono">{c.label}{#if c.collects}<span class="holds">{c.collects}</span>{/if}</span>
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
                  <div class="facts mono">
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
                        <li>{#if numbered}<span class="n mono">{ch.n ?? ""}</span>{/if}<span class="t">{ch.title}</span></li>
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
                  <div class="actions">
                    <button class="btn play" onclick={open}><Icon name="play" size={14} />{action}</button>
                    {#if reading && book.numPages}
                      <span class="where mono">
                        <span class="prog"><i style:width="{((book.currentPage + 1) / book.numPages) * 100}%"></i></span>
                        {book.currentPage + 1} / {book.numPages}
                      </span>
                    {:else if book.read}
                      <span class="where mono"><Icon name="check" size={13} />Read</span>
                    {/if}
                    <button class="ico" title="More" aria-label="More for this book" onclick={(e) => bookMenu(e, book!)}><Icon name="more" size={18} /></button>
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
    font-size: 13.5px;
    transition:
      background 0.15s,
      color 0.15s;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.on {
    background: rgb(var(--fg) / 0.09);
    color: var(--text);
  }
  .tab .n {
    font-size: 11px;
    color: var(--text-3);
  }
  .tab.on .n {
    color: var(--text-2);
  }
  .missing,
  .picked {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-2);
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
    font-size: 11px;
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
    font-weight: 300;
    font-size: 20px;
    line-height: 1;
    letter-spacing: -0.02em;
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
