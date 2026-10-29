<script lang="ts">
  import { tick } from "svelte";
  import { api, listText, plainText, storyTitle, unitWord, type ComicCard, type ComicMeta, type EditionKind, type MetaField, type SeriesView } from "$lib/api";
  import { lib, nav, openReader, reader, search, sheets, showMenu } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
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
  const by = $derived.by(() => {
    if (!data) return "";
    const writer = listText(data.writer);
    const artist = listText(data.artist);
    const art = artist && artist !== writer ? `art by ${artist}` : "";
    return [writer, art, data.publisher, data.years].filter(Boolean).join(" · ");
  });
  const findOnCv = () => data && (sheets.matching = { seriesId: data.id, isManga: data.isManga, name: data.name });
  const seriesAbout = $derived(plainText(data?.synopsis ?? null));

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
    Promise.all(ids.map((i) => api.comicMeta(i).catch(() => null))).then((list) => {
      if (sid !== id) return;
      const seen = new Map<string, number>();
      for (const m of list) {
        if (!m) continue;
        for (const n of new Set([...lines(m.fields.teams), ...lines(m.fields.characters)])) seen.set(n, (seen.get(n) ?? 0) + 1);
      }
      cast = seen;
    });
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

  let coverEl = $state<HTMLElement>();
  function open() {
    if (!book) return;
    openReader(book.id, coverEl?.querySelector("img") ?? null);
  }

  function seriesMenu(e: MouseEvent) {
    if (!data) return;
    const sid = data.id;
    showMenu(e, [
      { label: "Mark series as read", action: () => act(() => api.setSeriesRead(sid, true)) },
      { label: "Mark series as unread", action: () => act(() => api.setSeriesRead(sid, false)) },
      "separator",
      { label: data.cvVolumeId ? "Match again on Comic Vine…" : "Find on Comic Vine…", action: findOnCv },
      { label: "Show in Explorer", action: () => first && api.reveal(first.id) },
    ]);
  }

  function bookMenu(e: MouseEvent, c: ComicCard) {
    showMenu(e, [
      { label: c.read ? "Read again" : c.currentPage > 0 ? "Resume" : "Read", action: () => openReader(c.id) },
      "separator",
      c.read
        ? { label: "Mark as unread", action: () => act(() => api.setRead(c.id, false)) }
        : { label: "Mark as read", action: () => act(() => api.setRead(c.id, true)) },
      { label: "Mark earlier ones as read", action: () => act(() => api.markReadBefore(c.id)) },
      "separator",
      { label: "Edit details…", action: () => (sheets.editing = c.id) },
      { label: "Show in Explorer", action: () => api.reveal(c.id) },
      "separator",
      // Its format, when its name gave the wrong idea. Kept in AionComics; the file isn't touched.
      ...KINDS.map((k) => ({ label: KIND_NAME[k], checked: c.edition === k, action: () => act(() => api.setEdition(c.id, k)) })),
      { label: "Sort automatically", action: () => act(() => api.setEdition(c.id, null)) },
    ]);
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

  /* ---- The strip of covers along the bottom. */
  let strip = $state<HTMLElement>();
  async function pick(c: ComicCard) {
    picked = c.id;
    await tick();
    strip?.querySelector<HTMLElement>(`[data-book="${c.id}"]`)?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "smooth" });
  }
  // The wheel scrolls the strip sideways when it's longer than the window.
  $effect(() => {
    const el = strip;
    if (!el) return;
    const onwheel = (e: WheelEvent) => {
      if (el.scrollWidth <= el.clientWidth || Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    };
    el.addEventListener("wheel", onwheel, { passive: false });
    return () => el.removeEventListener("wheel", onwheel);
  });
  // ← and → step through the books.
  function onkey(e: KeyboardEvent) {
    if (reader.id !== null || search.open || sheets.editing !== null || sheets.matching) return;
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    if ((e.target as HTMLElement)?.closest?.("input, textarea, select, [contenteditable]")) return;
    if (!book || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    const i = inView.indexOf(book) + (e.key === "ArrowRight" ? 1 : -1);
    if (i < 0 || i >= inView.length) return;
    e.preventDefault();
    pick(inView[i]);
  }

  const shape = (c: ComicCard | undefined) => (c?.coverRatio && c.coverRatio > 0.9 && c.coverRatio < 2.1 ? c.coverRatio : 1.5);
  // The cover takes the height the window leaves between the series line and the strip, within
  // reason; a book with a lot to say may still push the strip below the fold.
  let vh = $state(0);
  let headH = $state(0);
  let stripH = $state(0);
  let noteH = $state(0);
  const note = $derived(data?.backfill ? noteH + 16 : 0);
  const coverW = $derived(Math.round(Math.min(Math.max(vh - 92 - headH - note - stripH - 66, 260), 500) / shape(book)));
</script>

<svelte:window onkeydown={onkey} bind:innerHeight={vh} />

{#if data}
  <div class="screen">
    <!-- The chosen book tints the page in its cover's color, changing with it. -->
    <!-- The series: its name, who made it, how far you are. -->
    <header class="series" bind:clientHeight={headH}>
      <div class="names">
        <button class="crumb link" onclick={() => nav.go({ name: "collection", id: data!.collectionId })}>{data.collectionName}</button>
        <h1>{data.name}</h1>
        {#if by}<div class="by">{by}</div>{/if}
      </div>
      <div class="tools">
        <span class="mono count" title="{mainRead} of {mainCount} {unitWord(data.isManga, mainCount)} read">
          <Icon name="check" size={13} />{mainRead} / {mainCount}
        </span>
        {#if !data.cvVolumeId && !seriesAbout}
          <button class="ico" title="Find details on Comic Vine" aria-label="Find details on Comic Vine" onclick={findOnCv}>
            <Icon name="fetch" size={18} />
          </button>
        {/if}
        <button class="ico" title="More" aria-label="More" onclick={seriesMenu} oncontextmenu={seriesMenu}><Icon name="more" size={18} /></button>
      </div>
    </header>

    {#if data.backfill}
      {@const b = data.backfill}
      <p class="backfill" title="You're on {b.beforeLabel}" bind:clientHeight={noteH}>
        {b.count} earlier {unitWord(data.isManga, b.count)} read?
        <button class="ico" title="Mark them read" aria-label="Mark them read" onclick={() => act(() => api.markReadBefore(b.beforeId))}>
          <Icon name="check" size={16} />
        </button>
        <button class="ico" title="Not now" aria-label="Not now" onclick={() => act(() => api.dismissBackfill(data!.id))}>
          <Icon name="close" size={15} />
        </button>
      </p>
    {/if}

    <!-- The chosen book in full: its cover, story, who made it and who's in it. -->
    {#if book}
      <section class="book">
        <button class="cover" bind:this={coverEl} onclick={open} oncontextmenu={(e) => bookMenu(e, book!)} title="{action} {book.label}">
          <Cover id={book.id} ratio={shape(book)} width={coverW} pages={book.numPages} flip={book.id === first?.id ? `s${data.id}` : undefined} />
        </button>
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

    <!-- One format's books at a time; click one to see it above, ← → to step through. -->
    <div class="shelf" bind:offsetHeight={stripH}>
      {#if data.editions.length > 1}
        <div class="tabs" role="tablist" aria-label="Formats">
          {#each data.editions as e (e.kind)}
            <button class="tab" class:on={e.kind === edition?.kind} role="tab" aria-selected={e.kind === edition?.kind} onclick={() => choose(e.kind)}>
              {e.name}<span class="n mono">{count(e)}</span>
            </button>
          {/each}
          {#if edition?.missing.length}
            <span class="missing mono" title="Not in your library">{edition.missing.map((n) => `#${n}`).join(", ")} missing</span>
          {/if}
        </div>
      {/if}
    <nav class="strip" bind:this={strip} aria-label="Books in this series">
      {#each edition?.sections ?? data.sections as s, i (i)}
        <div class="group">
          {#if s.name}<div class="gname">{s.name}</div>{/if}
          <div class="row">
            {#each s.comics as c (c.id)}
              <button
                class="pick"
                class:on={c.id === book?.id}
                class:done={c.read}
                data-book={c.id}
                title="{c.label}{storyTitle(c) ? ` · ${storyTitle(c)}` : ''}"
                onclick={() => pick(c)}
                ondblclick={() => openReader(c.id)}
                oncontextmenu={(e) => bookMenu(e, c)}
              >
                <Cover id={c.id} ratio={c.coverRatio} width={64} pages={c.numPages} />
                <span class="cap mono">{c.label}</span>
                {#if c.collects}<span class="holds mono">{c.collects}</span>{/if}
                {#if !c.read && c.currentPage > 0 && c.numPages}
                  <span class="line"><i style:width="{((c.currentPage + 1) / c.numPages) * 100}%"></i></span>
                {/if}
              </button>
            {/each}
          </div>
        </div>
      {/each}
    </nav>
    </div>
  </div>

{/if}

<style>
  /* The first screen: the series line, the chosen book, and the strip of every book under it. */
  .screen {
    position: relative;
    isolation: isolate;
    min-height: 100vh;
    display: flex;
    flex-direction: column;
    padding-top: 92px;
  }
  .series {
    flex: none;
    padding: 0 var(--pad);
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 32px;
  }
  .crumb {
    font-size: 13px;
  }
  h1 {
    margin-top: 10px;
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(40px, 4.2vw, 60px);
    line-height: 0.92;
    letter-spacing: 0.005em;
    overflow-wrap: anywhere;
  }
  .by {
    margin-top: 8px;
    color: var(--text-2);
    font-size: 14px;
  }
  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .count {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-right: 10px;
    color: var(--text-3);
  }
  .backfill {
    margin: 16px var(--pad) 0;
    color: var(--text-2);
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .book {
    flex: 1 0 auto;
    padding: 34px var(--pad) 30px;
    display: flex;
    gap: 48px;
    align-items: flex-start;
  }
  .cover {
    flex: none;
    display: block;
  }
  .cover :global(.cover) {
    box-shadow: 0 26px 60px -24px rgba(0, 0, 0, 0.95);
    transition: transform 0.6s var(--ease);
  }
  .cover:hover :global(.cover) {
    transform: translateY(-4px);
  }
  .about {
    min-width: 0;
    max-width: 72ch;
    transition: opacity 0.25s;
  }
  .about.dim {
    opacity: 0.6;
  }
  .label {
    font-family: var(--display);
    font-weight: 400;
    font-size: 60px;
    line-height: 0.9;
    letter-spacing: 0.005em;
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
    margin-top: 10px;
    font-family: var(--display);
    font-weight: 400;
    font-size: 60px;
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
    font-size: 44px;
  }
  .facts {
    margin-top: 12px;
    color: var(--text-3);
    display: flex;
    flex-wrap: wrap;
    gap: 4px 18px;
  }
  .arc {
    color: var(--text-2);
  }
  .syn {
    margin-top: 18px;
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
  .credits {
    margin-top: 18px;
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
  .chip:hover {
    color: var(--text);
    border-color: rgb(var(--fg) / 0.32);
  }
  .credits em {
    font-style: normal;
    color: var(--text-3);
    margin-left: 4px;
  }
  .chips {
    margin-top: 14px;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    padding: 3px 11px;
    border: 1px solid rgb(var(--fg) / 0.12);
    border-radius: 999px;
    color: var(--text-2);
    font-size: 12px;
  }
  .more-chips:hover {
    color: var(--text);
    border-color: rgb(var(--fg) / 0.3);
  }
  .actions {
    margin-top: 24px;
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
    color: var(--text-3);
  }
  .where .prog {
    width: 120px;
  }

  /* A band along the bottom, scrolling sideways in a long series. */
  .shelf {
    flex: none;
    background: rgb(var(--fg) / 0.025);
    border-top: 1px solid rgb(var(--fg) / 0.05);
  }
  /* One tab a format: "Volumes 12  Issues 72  Omnibus 1". */
  .tabs {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 14px var(--pad) 0;
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
    background: rgb(var(--fg) / 0.07);
    color: var(--text);
  }
  .tab .n {
    font-size: 11px;
    color: var(--text-3);
  }
  .tab.on .n {
    color: var(--text-2);
  }
  .missing {
    margin-left: auto;
    font-size: 11px;
    color: var(--text-3);
  }
  .strip {
    display: flex;
    gap: 36px;
    overflow-x: auto;
    padding: 18px var(--pad) 16px;
  }
  .group {
    flex: none;
  }
  .gname {
    font-size: 12px;
    color: var(--text-3);
    margin-bottom: 10px;
  }
  .row {
    display: flex;
    gap: 14px;
    align-items: flex-end;
  }
  .pick {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding-top: 10px;
  }
  .pick :global(.cover) {
    opacity: 0.55;
    transition:
      opacity 0.25s,
      transform 0.45s var(--ease),
      box-shadow 0.25s;
  }
  .pick.done :global(.cover) {
    opacity: 0.35;
  }
  .pick:hover :global(.cover) {
    opacity: 0.9;
  }
  .pick.on :global(.cover) {
    opacity: 1;
    transform: translateY(-8px);
    box-shadow: 0 0 0 2px #eaeaea;
  }
  .cap {
    margin-top: 8px;
    font-size: 11px;
    color: var(--text-3);
  }
  .pick.on .cap {
    color: var(--text);
  }
  /* What a collection holds: "#13–18". */
  .holds {
    font-size: 10.5px;
    color: var(--text-3);
    opacity: 0.8;
  }
  .line {
    position: absolute;
    left: 0;
    right: 0;
    bottom: -6px;
    height: 2px;
    background: rgb(var(--fg) / 0.1);
  }
  .line i {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--text);
  }

</style>
