<script lang="ts">
  import { tick } from "svelte";
  import { api, storyTitle, unitWord, progressOf, type ComicCard, type NameHit, type SearchHits, type SeriesSummary } from "$lib/api";
  import { nearest } from "$lib/keynav";
  import { nav, search } from "$lib/state.svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  // A page of its own, in the app's theme: the closest series large with where you are in it,
  // then more series, who and what's in the books, the books, and story arcs. The hits are numbered
  // in the order they're drawn, for the keys.
  type Hit = {
    i: number;
    key: string;
    cover: number | null;
    ratio: number | null;
    name: string;
    detail?: string;
    /** Not a result: the button that shows the rest of this list, or fewer of it. */
    toggles?: Folding;
    go: () => void;
  };
  type Folding = "who" | "arcs";

  // Names can run to the hundreds: a few to start with, and the rest if you ask.
  const FEW = 6;
  let unfolded = $state<Record<Folding, boolean>>({ who: false, arcs: false });

  let q = $state("");
  let found = $state<SearchHits | null>(null);
  let sel = $state(0);
  // Whether the arrow keys are among the results yet: until then ← and → move along what's typed.
  let browsing = $state(false);
  // Everything found is listed, but drawn a stretch at a time as you scroll: a single letter can
  // find most of the library.
  const STRETCH = 140;
  let shown = $state(STRETCH);
  let input = $state<HTMLInputElement>();
  let results = $state<HTMLDivElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (!search.open) return;
    q = "";
    found = null;
    input?.focus();
  });

  const books = (n: number) => `${n.toLocaleString()} ${n === 1 ? "book" : "books"}`;
  const howMany = (s: SeriesSummary) => `${s.count.toLocaleString()} ${unitWord(s.isManga, s.count)}`;

  $effect(() => {
    const query = q.trim();
    clearTimeout(timer);
    if (!query) {
      found = null;
      return;
    }
    timer = setTimeout(() => {
      api.search(query).then((r) => {
        if (q.trim() !== query) return;
        found = r;
        sel = 0;
        browsing = false;
        shown = STRETCH;
        unfolded = { who: false, arcs: false };
        results?.scrollTo(0, 0);
      });
    }, 120);
  });

  const lists = $derived.by(() => {
    const r = found;
    let i = 0;
    const series = (s: SeriesSummary, book?: ComicCard | null): Hit => ({
      i: i++,
      key: `s${s.id}`,
      cover: book?.id ?? s.coverId,
      ratio: book ? book.coverRatio : s.coverRatio,
      name: s.name,
      go: () => nav.go({ name: "series", id: s.id, book: book?.id }),
    });
    const named = (kind: "creator" | "character" | "arc", word: string, n: SearchHits["arcs"][number]): Hit => ({
      i: i++,
      key: `${kind}:${n.name}`,
      cover: n.coverId,
      ratio: null,
      name: n.name,
      detail: `${word} · ${books(n.books)}`,
      go: () => nav.go({ name: "people", kind, value: n.name }),
    });
    // A list of names, cut to a few unless it's unfolded, with the button to change that.
    const folded = (which: Folding, all: NameHit[], hit: (n: NameHit) => Hit): Hit[] => {
      const list = (unfolded[which] ? all : all.slice(0, FEW)).map(hit);
      if (all.length <= FEW) return list;
      return [
        ...list,
        {
          i: i++,
          key: `${which}:more`,
          cover: null,
          ratio: null,
          name: unfolded[which] ? "Show fewer" : `Show all ${all.length.toLocaleString()}`,
          toggles: which,
          go: () => {},
        },
      ];
    };
    const top = r?.series[0] ? series(r.series[0], r.topReading) : null;
    // Every series that matched, the one shown large included.
    const more = (r?.series ?? []).map((s) => series(s));
    // Creators and characters together, the closest matches first, then those on the most books.
    const creators = new Set(r?.creators ?? []);
    const people = [...(r?.creators ?? []), ...(r?.characters ?? [])].sort((a, b) => a.close - b.close || b.books - a.books);
    const who = folded("who", people, (n) => (creators.has(n) ? named("creator", "Creator", n) : named("character", "Character", n)));
    const bookHits = (r?.books ?? []).map((b): Hit => {
      const title = storyTitle(b);
      const whole = `${b.seriesName.trim()} ${b.label}`;
      return {
        i: i++,
        key: `b${b.id}`,
        cover: b.id,
        ratio: b.coverRatio,
        name: title ?? whole,
        detail: title ? whole : undefined,
        go: () => nav.go({ name: "series", id: b.seriesId, book: b.id }),
      };
    });
    const arcs = folded("arcs", r?.arcs ?? [], (n) => named("arc", "Story arc", n));
    return { top, more, who, books: bookHits, arcs, all: [...(top ? [top] : []), ...more, ...who, ...bookHits, ...arcs] };
  });

  const total = $derived(
    found ? found.series.length + found.books.length + found.creators.length + found.characters.length + found.arcs.length : 0,
  );
  const topSeries = $derived(found?.series[0] ?? null);
  const reading = $derived(found?.topReading ?? null);
  const upTo = (list: Hit[]) => list.filter((h) => h.i < shown);
  const more = $derived(upTo(lists.more));
  const who = $derived(upTo(lists.who));
  const bookHits = $derived(upTo(lists.books));
  const arcs = $derived(upTo(lists.arcs));

  // Near the end of what's drawn, draw the next stretch.
  function onscroll() {
    const r = results;
    if (r && shown < lists.all.length && r.scrollHeight - r.scrollTop - r.clientHeight < 1500) shown += STRETCH;
  }

  function pick(h: Hit | undefined) {
    if (!h) return;
    if (h.toggles) {
      // Unfolding leaves the pick on the first name it brought in; folding, back on the button.
      unfolded[h.toggles] = !unfolded[h.toggles];
      if (!unfolded[h.toggles]) {
        sel = lists.all.findIndex((x) => x.key === h.key);
        tick().then(() => moveTo(sel));
      }
      return;
    }
    search.open = false;
    h.go();
  }

  function moveTo(i: number) {
    sel = i;
    results?.querySelectorAll<HTMLElement>("[data-hit]")[i]?.scrollIntoView({ block: "nearest" });
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") search.open = false;
    else if (e.key === "Enter") pick(lists.all[sel]);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const items = [...(results?.querySelectorAll<HTMLElement>("[data-hit]") ?? [])];
      if (!items[sel]) return;
      // Down from the large panel goes to the start of what's under it, not whatever's mid-page.
      const fromTop = lists.top && sel === 0 && e.key === "ArrowDown";
      const to = fromTop ? items[1] : nearest(items, items[sel], e.key === "ArrowDown" ? "down" : "up");
      if (to) {
        browsing = true;
        moveTo(items.indexOf(to));
      } else if (e.key === "ArrowUp") {
        // Up from the top row goes back to what's typed.
        browsing = false;
        results?.scrollTo({ top: 0 });
      }
    } else if (browsing && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
      e.preventDefault();
      const last = Math.min(lists.all.length, shown) - 1;
      moveTo(Math.max(0, Math.min(last, sel + (e.key === "ArrowRight" ? 1 : -1))));
    }
  }

  // Clicking between the results shouldn't take the typing away from the box.
  function keepTyping(e: MouseEvent) {
    if (e.target !== input) e.preventDefault();
  }
</script>

{#snippet cover(h: Hit)}
  <button class="hit" class:on={h.i === sel} data-hit onmousemove={() => (sel = h.i)} onclick={() => pick(h)}>
    <Cover id={h.cover} ratio={h.ratio} />
    <span class="words" class:two={h.detail}>
      <span class="name">{h.name}</span>
      {#if h.detail}<span class="sub fine">{h.detail}</span>{/if}
    </span>
  </button>
{/snippet}

{#snippet row(h: Hit)}
  <button class="row" class:on={h.i === sel} data-hit onmousemove={() => (sel = h.i)} onclick={() => pick(h)}>
    {#if h.toggles}
      <span class="row-name more">{h.name}</span>
    {:else}
      <Cover id={h.cover} width={26} />
      <span class="row-name">{h.name}</span>
      <span class="fine">{h.detail}</span>
    {/if}
  </button>
{/snippet}

{#if search.open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="search-page" onmousedown={keepTyping}>
    <header>
      <input bind:this={input} bind:value={q} onkeydown={onkey} aria-label="Search" placeholder="Series, book, creator, character, arc" spellcheck="false" />
      <span class="fine hint">
        {#if found}{total.toLocaleString()} found<span class="keys"> ·</span>{/if}
        <span class="keys">↑↓ ↵ esc</span>
      </span>
      <button class="ico" title="Close (Esc)" aria-label="Close" onclick={() => (search.open = false)}><Icon name="close" /></button>
    </header>

    <div class="results" bind:this={results} {onscroll}>
      {#if lists.top && topSeries}
        <section>
          <h2>{lists.top.name}</h2>
          <button class="top" class:on={sel === 0} data-hit onmousemove={() => (sel = 0)} onclick={() => pick(lists.top ?? undefined)}>
            <!-- Its picture can finish loading just as the results are cleared. -->
            <Cover id={lists.top?.cover} ratio={lists.top?.ratio} width={186} />
            <span class="info">
              {#if reading}
                <span class="big">{reading.label}</span>
                <span>Page {(reading.currentPage + 1).toLocaleString()} of {(reading.numPages ?? 0).toLocaleString()}</span>
                <span class="bar"><i style:width="{Math.round(progressOf(reading) * 100)}%"></i></span>
                <span class="after">{howMany(topSeries)}</span>
              {:else}
                <span class="big">{howMany(topSeries)}</span>
                {#if topSeries.readCount > 0}
                  <span>{topSeries.readCount === topSeries.count ? "All read" : `${topSeries.readCount.toLocaleString()} read`}</span>
                {/if}
              {/if}
            </span>
          </button>
        </section>
      {/if}

      {#if more.length}
        <section>
          <h2>Series</h2>
          <div class="covers">
            {#each more as h (h.key)}{@render cover(h)}{/each}
          </div>
        </section>
      {/if}

      {#if who.length}
        <section>
          <h2>Who</h2>
          {#each who as h (h.key)}{@render row(h)}{/each}
        </section>
      {/if}

      {#if bookHits.length}
        <section>
          <h2>Books</h2>
          <div class="covers">
            {#each bookHits as h (h.key)}{@render cover(h)}{/each}
          </div>
        </section>
      {/if}

      {#if arcs.length}
        <section>
          <h2>Story arcs</h2>
          {#each arcs as h (h.key)}{@render row(h)}{/each}
        </section>
      {/if}

      {#if found && !lists.all.length}
        <p class="none">Nothing found</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* The page behind can't scroll, and the strip kept for its scrollbar turns the same paper. */
  :global(html:has(.search-page)) {
    overflow: hidden;
  }
  .search-page {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    gap: 40px;
    padding: 36px var(--pad) 0;
    background: var(--bg);
    color: var(--text);
    animation: fade 0.2s var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  header {
    flex: none;
    display: flex;
    align-items: flex-end;
    gap: 16px;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--text);
  }
  input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    padding: 0;
    outline: none;
    color: var(--text);
    font: 400 56px/0.95 var(--display);
    letter-spacing: 0.01em;
    text-transform: uppercase;
    user-select: text;
  }
  input::placeholder {
    color: var(--text-3);
  }
  .hint {
    flex: none;
    padding-bottom: 8px;
    color: var(--text-2);
  }
  header .ico {
    margin-bottom: 2px;
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 40px;
    /* Out to the window's sides, so the scrollbar sits at its edge, well clear of the covers. Its
       room is always kept (10px, see app.css), so the covers end where the line above them does. */
    scrollbar-gutter: stable;
    margin: 0 calc(-1 * var(--pad));
    padding: 4px calc(var(--pad) - 10px) 48px var(--pad);
  }
  section {
    flex: none;
  }
  /* The keys scroll a pick clear of the edges, with its outline. */
  [data-hit] {
    scroll-margin: 10px 0 48px;
  }
  h2 {
    margin: 0 0 10px;
    color: var(--text-2);
    font: 400 17px/1 var(--display);
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  /* The closest match, large. */
  .top {
    display: flex;
    align-items: center;
    gap: 20px;
    width: 100%;
    text-align: left;
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }
  .big {
    font: 400 40px/0.95 var(--display);
    margin-bottom: 4px;
  }
  .bar {
    display: block;
    width: min(100%, 420px);
    height: 2px;
    margin-top: 12px;
    background: var(--track);
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--text);
  }
  .after {
    margin-top: 12px;
  }

  /* Rows of covers, lined up along their bottoms over their names. */
  .covers {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 24px 12px;
    align-items: end;
  }
  @media (max-width: 1000px) {
    .covers {
      grid-template-columns: repeat(5, minmax(0, 1fr));
    }
  }
  @media (max-width: 700px) {
    .covers {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  .hit {
    min-width: 0;
    display: block;
    text-align: left;
  }
  .hit :global(.cover),
  .top :global(.cover) {
    transition: transform 0.2s var(--ease);
  }
  .hit:hover :global(.cover),
  .top:hover :global(.cover) {
    transform: translateY(-3px);
  }
  .hit.on :global(.cover),
  .top.on :global(.cover) {
    outline: 2px solid var(--text);
    outline-offset: 3px;
  }
  .words {
    display: block;
    height: 30px;
    margin-top: 7px;
  }
  .words.two {
    height: 47px;
  }
  .name {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
    font-size: 12px;
    line-height: 1.25;
  }
  .sub {
    display: block;
    overflow: hidden;
    color: var(--text-2);
    font-size: 12px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* A name: who, or a story arc. */
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 4px 10px;
    border-radius: 6px;
    text-align: left;
  }
  .row.on {
    background: rgb(var(--fg) / 0.08);
  }
  .row-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .row .fine {
    flex: none;
    color: var(--text-2);
  }
  /* Lines up with the names above it, past where their covers are. */
  .more {
    padding-left: 38px;
    color: var(--text-2);
  }
  .row:hover .more,
  .row.on .more {
    color: var(--text);
  }

  .none {
    color: var(--text-2);
  }
  /* A phone: no keyboard to point to, and a smaller box to type in. */
  @media (max-width: 700px) {
    .keys {
      display: none;
    }
    input {
      font-size: 40px;
    }
  }
</style>
