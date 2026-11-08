<script lang="ts">
  // The series and books you mean to read, laid out like a library's page: the name huge, cut from
  // their covers, then the covers. A series leaves the list once you start it (it's on Continue
  // reading then); one book leaves once you start that book.
  import { untrack } from "svelte";
  import { seriesMenu, want } from "$lib/manage.svelte";
  import { api, unitWord, type ComicCard, type SearchHits, type SeriesSummary, type WantList } from "$lib/api";
  import { lib, nav, openReader, showMenu } from "$lib/state.svelte";
  import LetterHeadline from "$lib/components/LetterHeadline.svelte";
  import InkPrint from "$lib/components/InkPrint.svelte";
  import SeriesTile from "$lib/components/SeriesTile.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon, { type IconName } from "$lib/components/Icon.svelte";
  import { hue } from "$lib/color";
  import { look } from "$lib/look.svelte";

  type Sort = "added" | "name";
  const SORTS: { key: Sort; title: string; icon: IconName | null }[] = [
    { key: "added", title: "Last put on first", icon: "arrived" },
    { key: "name", title: "A to Z", icon: null },
  ];

  /** One thing on the list: a whole series, or one book of one. */
  type Item = { key: string; at: number; name: string; coverId: number | null; color: string | null; series?: SeriesSummary; book?: ComicCard };

  let data = $state<WantList | null>(null);
  let filter = $state("");
  let sort = $state<Sort>((() => {
    try {
      return localStorage.getItem("aion.wantSort") === "name" ? "name" : "added";
    } catch {
      return "added";
    }
  })());
  $effect(() => {
    try {
      localStorage.setItem("aion.wantSort", sort);
    } catch {}
  });

  $effect(() => {
    void lib.version;
    api.wantList().then((d) => {
      data = d;
      nav.restore();
    });
  });

  const items = $derived.by((): Item[] => {
    if (!data) return [];
    const series = data.series.map((s, i) => ({ key: `s${s.id}`, at: data!.seriesAt[i], name: s.name.trim(), coverId: s.coverId, color: s.coverColor, series: s }));
    const books = data.books.map((b, i) => ({ key: `b${b.id}`, at: data!.booksAt[i], name: b.seriesName.trim(), coverId: b.id, color: b.coverColor, book: b }));
    return [...series, ...books].sort((a, b) => b.at - a.at);
  });
  const shown = $derived.by(() => {
    const words = filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const list = items.filter((it) => words.every((w) => it.name.toLowerCase().includes(w)));
    return sort === "name" ? [...list].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true })) : list;
  });
  const bookCount = $derived(data?.books.length ?? 0);

  // The headline's letters cut from the list's covers, a fresh pick each visit (see Collection).
  const letters = [..."WANTTOREAD"].length;
  const seed = Math.floor(Math.random() * 2 ** 32);
  const shuffled = (n: number) => Math.imul(n ^ seed, 2654435761) >>> 0;
  const letterCovers = $derived(
    items
      .filter((it) => it.coverId != null)
      .sort((a, b) => shuffled(a.coverId!) - shuffled(b.coverId!))
      .slice(0, letters)
      .map((it) => it.coverId!),
  );
  // Behind it, one more cover printed in its own color, and not a teal one.
  let backdrop = $state<{ id: number; hue: number | null; at: number } | null>(null);
  $effect(() => {
    if (!items.length) return void (backdrop = null);
    const pool = items.filter((it) => {
      const c = hue(it.color);
      return it.coverId != null && !!c && c.chroma > 0.12 && !(c.h >= 150 && c.h <= 200);
    });
    const kept = untrack(() => backdrop);
    if (kept && pool.some((it) => it.coverId === kept.id)) return;
    const free = pool.filter((it) => !letterCovers.includes(it.coverId!));
    const pick = (free.length ? free : pool).sort((a, b) => shuffled(a.coverId!) - shuffled(b.coverId!))[0];
    if (pick) backdrop = { id: pick.coverId!, hue: hue(pick.color)!.h, at: 20 + (shuffled(pick.coverId!) % 61) };
    else if (!kept && items[0]?.coverId != null) backdrop = { id: items[0].coverId, hue: null, at: 40 };
  });

  // Adding from here: + opens a finder over the whole library; each series or book in it puts
  // itself on the list, or takes itself off, with a press.
  let adding = $state(false);
  let q = $state("");
  let hits = $state<SearchHits | null>(null);
  let finder = $state<HTMLInputElement>();
  $effect(() => {
    const query = q.trim();
    if (!query) return void (hits = null);
    const t = setTimeout(() => api.search(query).then((h) => q.trim() === query && (hits = h)), 150);
    return () => clearTimeout(t);
  });
  $effect(() => {
    if (adding) finder?.focus();
  });
  const onList = $derived(new Set(items.map((it) => it.key)));
  function outside(e: PointerEvent) {
    if (adding && !(e.target as HTMLElement)?.closest?.(".adder")) adding = false;
  }

  const section = $derived(filter.trim() ? `Matching “${filter.trim()}”` : sort === "name" ? "A to Z" : "Last put on first");

  function detail(it: Item) {
    if (it.book) return it.book.label;
    const s = it.series!;
    return s.readCount > 0 ? `${s.readCount} of ${s.count} read` : `${s.count} ${unitWord(s.isManga, s.count)}`;
  }
  function open(it: Item) {
    if (it.book) nav.go({ name: "series", id: it.book.seriesId, book: it.book.id });
    else nav.go({ name: "series", id: it.series!.id });
  }
  function menuFor(e: MouseEvent, it: Item) {
    if (it.series) return seriesMenu(e, it.series, false);
    const b = it.book!;
    const name = `${b.seriesName.trim()} ${b.label}`;
    showMenu(e, [
      { label: "Read", action: () => openReader(b.id) },
      { label: "Take off Want to read", action: () => want({ book: b.id }, false, name) },
      "separator",
      { label: `Go to ${b.seriesName.trim()}`, action: () => nav.go({ name: "series", id: b.seriesId }) },
    ]);
  }
</script>

<svelte:window onpointerdown={outside} />

{#snippet hit(key: string, coverId: number | null, ratio: number | null, name: string, detail: string, done: boolean, toggle: (on: boolean) => void)}
  {@const on = onList.has(key)}
  <!-- Something already read can't go on: it would leave again straight away. -->
  <button class="hit" class:on disabled={done && !on} title={on ? "On your list. Press to take it off" : done ? "You've read it" : "Put it on your list"} onclick={() => toggle(!on)}>
    <Cover id={coverId} {ratio} width={30} />
    <span class="what">
      <span class="hname">{name}</span>
      <span class="hdetail">{detail}</span>
    </span>
    {#if done && !on}<span class="flag"><Icon name="check" size={16} /></span>{:else}<span class="flag"><Icon name={on ? "bookmark" : "bookmarkOutline"} size={16} /></span>{/if}
  </button>
{/snippet}

{#if data}
  <div class="page">
    <div class="top" class:inked={look.backdrop && backdrop}>
      <div class="caption count">
        {#if items.length}
          {items.length.toLocaleString()} to read{#if bookCount}&nbsp;· {bookCount} single {bookCount === 1 ? "book" : "books"}{/if}
        {:else}
          Nothing on it yet
        {/if}
      </div>
      {#if look.backdrop && backdrop}
        <InkPrint id={backdrop.id} hue={backdrop.hue} at={backdrop.at} />
      {/if}
      <LetterHeadline text="Want to read" covers={letterCovers} />
    </div>
    <div class="head">
      <div class="tools" class:gone={!items.length}>
        <label class="filter" title="Filter">
          <Icon name="search" size={15} />
          <input bind:value={filter} spellcheck="false" aria-label="Filter" />
        </label>
        <div class="sorts">
          {#each SORTS as s (s.key)}
            <button class="ico" class:on={sort === s.key} title={s.title} aria-label={s.title} onclick={() => (sort = s.key)}>
              {#if s.icon}<Icon name={s.icon} size={16} />{:else}<span class="az fine">A–Z</span>{/if}
            </button>
          {/each}
        </div>
      </div>
      <div class="adder">
        <button class="ico" class:on={adding} title="Add to Want to read" aria-label="Add to Want to read" aria-expanded={adding} onclick={() => (adding = !adding)}>
          <Icon name="plus" size={18} />
        </button>
        {#if adding}
          <div class="finder" role="dialog" aria-label="Add to Want to read">
            <label class="find">
              <Icon name="search" size={14} />
              <input
                bind:this={finder}
                bind:value={q}
                placeholder="Find a series or a book"
                spellcheck="false"
                onkeydown={(e) => {
                  if (e.key === "Escape") {
                    e.stopPropagation();
                    adding = false;
                  }
                }}
              />
            </label>
            <div class="hits">
              {#if !q.trim()}
                <p class="hint">Type part of a name. Press a series to put the whole series on, or a book for just that book.</p>
              {:else if hits && !hits.series.length && !hits.books.length}
                <p class="hint">Nothing called that.</p>
              {:else if hits}
                {#if hits.series.length}<div class="group">Series</div>{/if}
                {#each hits.series.slice(0, 8) as s (s.id)}
                  {@render hit(`s${s.id}`, s.coverId, s.coverRatio, s.name.trim(), s.readCount >= s.count ? "Read" : `${s.count} ${unitWord(s.isManga, s.count)}`, s.readCount >= s.count, (on) => want({ series: s.id }, on, s.name.trim()))}
                {/each}
                {#if hits.books.length}<div class="group">Books</div>{/if}
                {#each hits.books.slice(0, 8) as b (b.id)}
                  {@render hit(`b${b.id}`, b.id, b.coverRatio, b.seriesName.trim(), [b.label, b.title && !b.title.includes(b.label) ? b.title : null, b.read ? "Read" : null].filter(Boolean).join(" · "), b.read, (on) => want({ book: b.id }, on, `${b.seriesName.trim()} ${b.label}`))}
                {/each}
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>

    {#if !items.length}
      <p class="empty">
        Press <span class="mark"><Icon name="plus" size={15} /></span> to find a series or a book to put here, or press
        <span class="mark"><Icon name="bookmarkOutline" size={15} /></span> on a series' page. Once you start reading something, it moves on
        to Continue reading.
      </p>
    {:else if shown.length}
      <div class="caption section">{section}</div>
      <div class="grid">
        {#each shown as it (it.key)}
          <SeriesTile
            coverId={it.coverId}
            ratio={it.book ? it.book.coverRatio : it.series!.coverRatio}
            books={it.series?.count ?? 1}
            read={it.series?.readCount ?? 0}
            name={it.name}
            detail={detail(it)}
            tag={it.book ? "Book" : undefined}
            onclick={() => open(it)}
            oncontextmenu={(e) => menuFor(e, it)}
          />
        {/each}
      </div>
    {:else}
      <p class="empty">Nothing matches “{filter}”.</p>
    {/if}
    <div class="foot"></div>
  </div>
{/if}

<style>
  .page {
    position: relative;
    isolation: isolate;
  }
  /* The headline's panel, as on a library's page. */
  .top {
    position: relative;
    margin: 64px var(--pad) 0;
    padding: 44px 24px 40px;
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
  .caption.section {
    margin: 40px var(--pad) 0;
  }
  .head {
    padding: 18px var(--pad) 10px;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 28px;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 28px;
  }
  .tools.gone {
    visibility: hidden;
  }
  /* The finder drops down under +, over the page. */
  .adder {
    position: relative;
    margin-left: -12px;
  }
  .adder > .ico.on {
    color: var(--text);
    background: rgb(var(--fg) / 0.08);
  }
  .finder {
    position: absolute;
    right: 0;
    top: calc(100% + 8px);
    z-index: 5;
    width: 400px;
    padding: 12px 8px 8px;
    border-radius: 8px;
    background: var(--menu);
    box-shadow: 0 18px 40px rgb(0 0 0 / 0.45);
  }
  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 8px 8px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
  }
  .find:focus-within {
    border-color: var(--text-2);
    color: var(--text-2);
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    outline: none;
    color: var(--text);
    user-select: text;
  }
  .hits {
    max-height: 420px;
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .hint {
    margin: 6px 8px 8px;
    color: var(--text-3);
    font-size: 13px;
    line-height: 1.5;
  }
  .group {
    margin: 10px 8px 4px;
    font-size: 12px;
    color: var(--text-3);
  }
  .hit {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 5px 8px;
    border-radius: 6px;
    text-align: left;
  }
  .hit:hover:not(:disabled) {
    background: rgb(var(--fg) / 0.07);
  }
  .hit:disabled {
    cursor: default;
  }
  .hit:disabled .hname {
    color: var(--text-3);
  }
  .hit :global(.cover) {
    flex: none;
  }
  .what {
    flex: 1;
    min-width: 0;
  }
  .hname,
  .hdetail {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hname {
    font-size: 13px;
    color: var(--text);
  }
  .hdetail {
    font-size: 12px;
    color: var(--text-3);
  }
  .flag {
    flex: none;
    display: flex;
    color: var(--text-3);
  }
  .hit.on .flag,
  .hit:hover:not(:disabled) .flag {
    color: var(--text);
  }
  .sorts {
    display: flex;
    gap: 2px;
  }
  .sorts .ico {
    color: var(--text-3);
  }
  .sorts .ico.on {
    color: var(--text);
    background: rgb(var(--fg) / 0.08);
  }
  .az {
    font-size: 12px;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
    border-bottom: 1px solid var(--line);
    transition: border-color 0.2s;
  }
  .filter:focus-within {
    border-color: var(--text-2);
    color: var(--text-2);
  }
  .filter input {
    width: 150px;
    background: none;
    border: 0;
    padding: 6px 0;
    outline: none;
    user-select: text;
  }
  .grid {
    padding: 36px var(--pad) 0;
  }
  .empty {
    padding: 64px var(--pad);
    max-width: 640px;
    line-height: 1.6;
  }
  .empty .mark {
    display: inline-flex;
    vertical-align: -2px;
    color: var(--text-2);
  }
  .foot {
    height: 140px;
  }
</style>
