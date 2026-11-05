<script lang="ts">
  import { untrack } from "svelte";
  import { addMenu } from "$lib/add.svelte";
  import { libraryMenu, seriesMenu } from "$lib/manage.svelte";
  import { api, listText, plainText, unitWord, type CollectionView, type SeriesSummary, type SeriesView } from "$lib/api";
  import { lib, nav, openReader } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import LetterHeadline from "$lib/components/LetterHeadline.svelte";
  import InkPrint from "$lib/components/InkPrint.svelte";
  import { hue, tone } from "$lib/color";
  import { look } from "$lib/look.svelte";
  import Icon, { type IconName } from "$lib/components/Icon.svelte";

  type Sort = "name" | "read" | "added";
  // Each sort is a symbol; its name is the tooltip.
  const SORTS: { key: Sort; title: string; icon: IconName | null }[] = [
    { key: "name", title: "A to Z", icon: null },
    { key: "read", title: "Recently read", icon: "clock" },
    { key: "added", title: "Recently added", icon: "arrived" },
  ];

  // Which series to show, besides all of them. Pressing the one that's on goes back to all.
  type Show = "unread" | "reading";
  const SHOWS: { key: Show; title: string; icon: IconName }[] = [
    { key: "unread", title: "Only ones with books still unread", icon: "unread" },
    { key: "reading", title: "Only ones you've started and not finished", icon: "reading" },
  ];
  const stored = (() => {
    try {
      return localStorage.getItem("aion.collectionShow");
    } catch {
      return null;
    }
  })();

  let { id }: { id: number } = $props();
  let data = $state<CollectionView | null>(null);
  let filter = $state("");
  let sort = $state<Sort>((localStorage.getItem("aion.collectionSort") as Sort | null) ?? "name");
  let showing = $state<Show | null>(stored === "unread" || stored === "reading" ? stored : null);

  $effect(() => {
    void lib.version;
    const cid = id;
    api.collection(cid).then((d) => {
      if (cid !== id) return;
      data = d;
      nav.restore();
    });
  });
  $effect(() => {
    try {
      localStorage.setItem("aion.collectionSort", sort);
      if (showing) localStorage.setItem("aion.collectionShow", showing);
      else localStorage.removeItem("aion.collectionShow");
    } catch {}
  });

  const unfinished = (s: SeriesSummary) => s.readCount < s.count;
  const started = (s: SeriesSummary) => unfinished(s) && (s.inProgress || s.readCount > 0);
  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = (data?.series ?? [])
      .filter((s) => s.name.toLowerCase().includes(q))
      .filter((s) => (showing === "unread" ? unfinished(s) : showing === "reading" ? started(s) : true));
    if (sort === "read") return [...list].sort((a, b) => (b.lastReadAt ?? 0) - (a.lastReadAt ?? 0));
    if (sort === "added") return [...list].sort((a, b) => (b.lastAddedAt ?? 0) - (a.lastAddedAt ?? 0));
    return list;
  });
  const total = $derived(data?.series.reduce((n, s) => n + s.count, 0) ?? 0);

  function status(s: SeriesSummary) {
    if (s.readCount === s.count) return "Read";
    if (s.inProgress) return "Reading";
    return `${s.count} ${unitWord(s.isManga, s.count)}`;
  }

  function open(e: MouseEvent, s: SeriesSummary) {
    nav.go({ name: "series", id: s.id });
  }

  // The headline's letters are cut from covers picked at random, a fresh pick each visit. The pick
  // holds while you're here, even as a scan refreshes the page. One cover a letter at most; fewer
  // series than letters and they go round again.
  const letters = $derived(data ? [...data.name].filter((ch) => ch !== " ").length : 0);
  const seed = Math.floor(Math.random() * 2 ** 32);
  const shuffled = (n: number) => Math.imul(n ^ seed, 2654435761) >>> 0;
  const letterCovers = $derived.by(() =>
    (data?.series ?? [])
      .filter((s) => s.coverId != null)
      .sort((a, b) => shuffled(a.id) - shuffled(b.id))
      .slice(0, letters)
      .map((s) => s.coverId!),
  );

  // Behind the headline, one more cover, blown up and printed in a single ink of its own color: a
  // fresh one each visit, like the letters, and one the letters aren't using when there's a choice.
  // Only ever a cover (a page from inside a book could give the story away), and never a teal one.
  // A cover already printed (made smaller and kept, see `print` in lib.rs; every series' cover gets
  // one after a scan) comes first, so it's there with the page. The pick then holds while you're
  // here, even as a refresh brings news of more prints.
  let backdrop = $state<{ id: number; hue: number | null; at: number } | null>(null);
  $effect(() => {
    const pool = (data?.series ?? [])
      .filter((s) => {
        const c = hue(s.coverColor);
        return s.coverId != null && !!c && c.chroma > 0.12 && !(c.h >= 150 && c.h <= 200);
      })
      .sort((a, b) => shuffled(a.id) - shuffled(b.id));
    const free = pool.filter((s) => !letterCovers.includes(s.coverId!));
    const choice = free.length ? free : pool;
    const kept = untrack(() => backdrop);
    if (kept && choice.some((s) => s.coverId === kept.id)) return;
    const s = choice.find((s) => s.printed) ?? choice[0];
    if (s) {
      backdrop = { id: s.coverId!, hue: hue(s.coverColor)!.h, at: 20 + (shuffled(s.id) % 61) };
      return;
    }
    // Nothing here prints in a color: a cover in gray, rather than an empty panel.
    const any = data?.series.find((s) => s.coverId != null);
    if (any && !kept) backdrop = { id: any.coverId!, hue: null, at: 20 + (shuffled(any.id) % 61) };
  });
  // The grid's caption box: what's shown, in what order.
  const section = $derived.by(() => {
    if (filter.trim()) return `Matching “${filter.trim()}”`;
    const order = { name: "A to Z", read: "recently read", added: "recently added" }[sort];
    if (showing) return `${showing === "unread" ? "Still unread" : "Started"}, ${order}`;
    return { name: "Every series, A to Z", read: "Recently read", added: "Recently added" }[sort];
  });

  // With only one or two series there's room to tell each one properly instead of a grid.
  const few = $derived(!!data && data.series.length > 0 && data.series.length <= 2 && !filter.trim() && !showing);
  let features = $state<SeriesView[]>([]);
  $effect(() => {
    const ss = few ? (data?.series ?? []) : [];
    Promise.all(ss.map((s) => api.series(s.id))).then((vs) => (features = vs));
  });
  let beside = $state(false);

  function credits(v: SeriesView) {
    const people = listText(v.writer).split(", ").filter(Boolean);
    const who = people.length > 2 ? `${people[0]} and ${people.length - 1} others` : people.join(" and ");
    return [who, v.years].filter(Boolean).join(" · ");
  }
  // The book to open from a feature: the one being read, else the first not yet read.
  function nextBook(v: SeriesView) {
    const books = v.sections.flatMap((x) => x.comics);
    const going = books.filter((b) => b.currentPage > 0 && !b.read).sort((a, b) => (b.lastReadAt ?? 0) - (a.lastReadAt ?? 0))[0];
    return { book: going ?? books.find((b) => !b.read) ?? books[0], going: !!going };
  }
  function blurb(v: SeriesView, length: number) {
    const t = plainText(v.synopsis);
    return t.length > length ? `${t.slice(0, length).replace(/\s+\S*$/, "")}…` : t;
  }
</script>

{#snippet feature(v: SeriesView, s: SeriesSummary, width: number)}
  {@const next = nextBook(v)}
  <div class="feature" role="group" oncontextmenu={(e) => seriesMenu(e, s, false)}>
    <button class="fcover" data-nav onclick={(e) => open(e, s)} aria-label={s.name}>
      <Cover id={s.coverId} ratio={s.coverRatio} {width} />
    </button>
    <div class="fbody">
      <button class="fname" onclick={(e) => open(e, s)}>{s.name}</button>
      <div class="fmeta">{credits(v)}{credits(v) ? " · " : ""}{s.count} {unitWord(s.isManga, s.count)}</div>
      {#if blurb(v, 260)}<p class="fsyn">{blurb(v, width > 200 ? 300 : 190)}</p>{/if}
      {#if next.book}
        <button class="btn play" onclick={(e) => openReader(next.book.id, (e.currentTarget as HTMLElement).closest(".feature")?.querySelector("img") ?? null)}>
          <Icon name="play" size={14} />{next.going ? "Continue" : "Read"}
          {next.book.label}
        </button>
      {/if}
    </div>
  </div>
{/snippet}

{#if data}
  <div class="page collection">
  <!-- The headline as a comic panel: inked frame, the collection's counts in a caption box on its
       corner, and a cover printed behind the letters. -->
  <div class="top" class:inked={look.backdrop && backdrop}>
  <div class="caption count">
    {#if data.series.length}
      {data.series.length.toLocaleString()} series · {total.toLocaleString()}
      {data.isManga ? unitWord(true, total) : total === 1 ? "book" : "books"}
    {:else}
      Nothing here yet
    {/if}
  </div>
  {#if look.backdrop && backdrop}
    <InkPrint id={backdrop.id} hue={backdrop.hue} at={backdrop.at} />
  {/if}
    {#if data.series.length === 1 && features[0]}
      <LetterHeadline text={data.name} covers={letterCovers} bind:beside>
        {#snippet aside()}{@render feature(features[0], data!.series[0], 160)}{/snippet}
      </LetterHeadline>
    {:else}
      <LetterHeadline text={data.name} covers={letterCovers} />
    {/if}
  </div>
  <div class="head">
    <div class="tools" class:gone={!data.series.length}>
      <label class="filter" title="Filter">
        <Icon name="search" size={15} />
        <input placeholder="" bind:value={filter} spellcheck="false" aria-label="Filter" />
      </label>
      <div class="sorts">
        {#each SHOWS as s (s.key)}
          <button class="ico" class:on={showing === s.key} title={s.title} aria-label={s.title} aria-pressed={showing === s.key} onclick={() => (showing = showing === s.key ? null : s.key)}>
            <Icon name={s.icon} size={16} />
          </button>
        {/each}
      </div>
      <div class="sorts">
        {#each SORTS as s (s.key)}
          <button class="ico" class:on={sort === s.key} title={s.title} aria-label={s.title} onclick={() => (sort = s.key)}>
            {#if s.icon}<Icon name={s.icon} size={16} />{:else}<span class="az mono">A–Z</span>{/if}
          </button>
        {/each}
      </div>
    </div>
    <button class="ico add" title="Add a series" aria-label="Add a series" onclick={(e) => addMenu(e, { collection: data!.id, title: data!.name })}>
      <Icon name="plus" size={18} />
    </button>
    <button
      class="ico add"
      title="More"
      aria-label="More"
      onclick={(e) => libraryMenu(e, { id: data!.id, name: data!.name, seriesCount: data!.series.length, comicCount: total, reading: lib.collections.find((c) => c.id === data!.id)?.reading }, true)}
    >
      <Icon name="more" size={18} />
    </button>
  </div>

  {#if !data.series.length}
    <p class="empty">Press + to add a series, or drag comics anywhere in this window. Their covers will fill these letters.</p>
  {:else if few && !beside}
    <div class="features" class:pair={features.length === 2}>
      {#each features as v, i (v.id)}{@render feature(v, data.series[i], features.length === 1 ? 230 : 170)}{/each}
    </div>
  {:else if few}
    <!-- The one series is beside the headline already. -->
  {:else if shown.length}
    <div class="caption section">{section}</div>
    <div class="grid">
      {#each shown as s (s.id)}
        <!-- Each series' cover on the page, a run of books standing as a small pile behind it. Under
             it, a bar of the cover's own color that fills in as you read. -->
        <button class="series" data-nav style:--tint={tone(s.coverColor, 0.4, 0.5) ?? "var(--track)"} onclick={(e) => open(e, s)} oncontextmenu={(e) => seriesMenu(e, s, false)}>
          <span class="art">
            {#if s.count > 2}<span class="sheet far"></span>{/if}
            {#if s.count > 1}<span class="sheet"></span>{/if}
            <Cover id={s.coverId} ratio={s.coverRatio} pages={null} />
          </span>
          <span class="mark" title={s.readCount ? `${s.readCount} of ${s.count} read` : undefined}>
            {#if s.readCount > 0}<i style:width="{(s.readCount / s.count) * 100}%"></i>{/if}
          </span>
          <span class="cap">
            <span class="name">{s.name}</span>
            <span class="detail">{status(s)}</span>
          </span>
        </button>
      {/each}
    </div>
  {:else if filter.trim()}
    <p class="empty">Nothing matches “{filter}”.</p>
  {:else}
    <p class="empty">
      {showing === "unread" ? "You've finished everything here." : "Nothing here is started and unfinished."}
      <button class="link" onclick={() => (showing = null)}>Show all</button>
    </p>
  {/if}
  <div class="foot"></div>
  </div>
{/if}

<style>
  /* The headline's panel: the name, huge, its letters cut from the collection's covers (see
     LetterHeadline), over a cover printed in one ink (see InkPrint). */
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
    padding: 18px var(--pad) 0;
    display: flex;
    align-items: center;
    gap: 40px;
  }

  /* One or two series: each told properly, cover beside its details. */
  .features {
    padding: 56px var(--pad) 0;
    display: grid;
    gap: 48px;
    max-width: 1000px;
  }
  .features.pair {
    grid-template-columns: 1fr 1fr;
    max-width: none;
  }
  .feature {
    display: flex;
    align-items: flex-start;
    gap: 28px;
  }
  .fcover {
    flex: none;
  }
  .fbody {
    min-width: 0;
  }
  .fname {
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.25;
    text-align: left;
  }
  .fmeta {
    margin-top: 6px;
    color: var(--text-2);
    font-size: 13px;
  }
  .fsyn {
    margin: 14px 0 0;
    color: var(--text-2);
    font-size: 13.5px;
    line-height: 1.6;
  }
  .feature .play {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-top: 20px;
  }
  .tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 28px;
    padding-bottom: 10px;
  }
  .tools.gone {
    visibility: hidden;
  }
  /* Beside the filter and sorts, and still here on a collection with nothing in it. */
  .add {
    flex: none;
    margin: 0 0 10px -32px;
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
    font-size: 11px;
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
  .page {
    position: relative;
    isolation: isolate;
  }
  .series {
    display: block;
    text-align: left;
    min-width: 0;
    padding: 18px 10px 4px;
    margin: -18px -10px -4px;
  }
  .art {
    position: relative;
    display: block;
    transition: transform 0.5s var(--ease);
  }
  /* The books behind the first: plain paper backs, stepping up and to the right. */
  .sheet {
    position: absolute;
    inset: 0;
    border-radius: 2px;
    background: var(--edge);
    box-shadow: 0 0 0 1px rgb(var(--fg) / 0.06);
    transform: translate(5px, -5px);
    transition: transform 0.5s var(--ease);
  }
  .sheet.far {
    background: var(--edge-far);
    transform: translate(10px, -10px);
  }
  .series:hover .sheet {
    transform: translate(7px, -7px);
  }
  .series:hover .sheet.far {
    transform: translate(14px, -14px);
  }
  .art :global(.cover) {
    transition: box-shadow 0.5s var(--ease);
  }
  .series:hover .art {
    transform: translateY(-4px);
  }
  .series:hover .art :global(.cover) {
    box-shadow: 0 18px 40px -12px rgba(0, 0, 0, 0.9);
  }
  /* The series' color, a slim printer's mark; what you've read of it fills in from the left. */
  .mark {
    position: relative;
    display: block;
    height: 3px;
    margin-top: 10px;
    border-radius: 1px;
    background: var(--tint);
    overflow: hidden;
  }
  .mark i {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--text);
  }
  .cap {
    display: block;
    height: 56px;
    margin-top: 13px;
  }
  .name {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    font-size: 13px;
    line-height: 1.35;
    color: var(--cap);
    transition: color 0.2s;
  }
  .series:hover .name {
    color: var(--text);
  }
  .detail {
    display: block;
    margin-top: 3px;
    font-size: 12px;
    color: var(--text-3);
  }
  .empty {
    padding: 72px var(--pad);
  }
  .foot {
    height: 140px;
  }
</style>
