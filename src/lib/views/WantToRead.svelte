<script lang="ts">
  // The series and books you mean to read, laid out like a library's page: the name huge, cut from
  // their covers, then the covers. A series leaves the list once you start it (it's on Continue
  // reading then); one book leaves once you start that book.
  import { untrack } from "svelte";
  import { seriesMenu, want } from "$lib/manage.svelte";
  import { api, unitWord, type ComicCard, type SeriesSummary, type WantList } from "$lib/api";
  import { lib, nav, openReader, showMenu } from "$lib/state.svelte";
  import LetterHeadline from "$lib/components/LetterHeadline.svelte";
  import InkPrint from "$lib/components/InkPrint.svelte";
  import SeriesTile from "$lib/components/SeriesTile.svelte";
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
    <div class="head" class:gone={!items.length}>
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

    {#if !items.length}
      <p class="empty">
        Press <span class="mark"><Icon name="bookmarkOutline" size={15} /></span> on a series' page to put it here, or right-click one
        book for just that book. Once you start reading something, it moves on to Continue reading.
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
  .head.gone {
    visibility: hidden;
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
