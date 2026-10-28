<script lang="ts" module>
  /** Shown or hidden, remembered between sessions (Ctrl B, or the button in the top bar). */
  export const side = $state({ open: load() });

  function load() {
    try {
      return localStorage.getItem("aion.side") !== "0";
    } catch {
      return true;
    }
  }

  export function toggleSide() {
    side.open = !side.open;
    try {
      localStorage.setItem("aion.side", side.open ? "1" : "0");
    } catch {
      // Not remembered; it still toggles.
    }
  }
</script>

<script lang="ts">
  // A side panel for getting around fast: the books you're partway through, the series added
  // lately, and every collection opening into its series, with a filter over all of them.
  import { api, progressOf, type ComicCard, type SeriesSummary } from "$lib/api";
  import { lib, nav, openReader } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";

  const READING = 8;
  const ADDED = 8;

  let reading = $state<ComicCard[]>([]);
  let series = $state<Record<number, SeriesSummary[]>>({});
  let filter = $state("");
  // Which collections and sections are open, remembered.
  let opened = $state<Record<string, boolean>>(remembered());

  function remembered(): Record<string, boolean> {
    try {
      return JSON.parse(localStorage.getItem("aion.side.open") ?? "") ?? {};
    } catch {
      return { reading: true, added: true };
    }
  }
  function flip(key: string) {
    opened[key] = !opened[key];
    try {
      localStorage.setItem("aion.side.open", JSON.stringify(opened));
    } catch {
      // Not remembered.
    }
  }

  const route = $derived(nav.current);
  const here = (r: { name: string; id?: number }) =>
    route.name === r.name && (r.id === undefined || ("id" in route && route.id === r.id));

  // Fresh data whenever the library changes.
  $effect(() => {
    void lib.version;
    series = {};
    api.home().then((h) => (reading = h.continueReading.slice(0, READING)));
  });

  // Series lists load when needed: a collection opened, or everything for "Recently added" and
  // the filter.
  const loading = new Set<number>();
  function need(id: number) {
    if (series[id] || loading.has(id)) return;
    loading.add(id);
    api
      .collection(id)
      .then((v) => (series[id] = [...v.series].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }))))
      .finally(() => loading.delete(id));
  }
  $effect(() => {
    for (const c of lib.collections) if (opened[`c${c.id}`] || opened.added || filter) need(c.id);
  });

  const added = $derived(
    Object.values(series)
      .flat()
      .filter((s) => s.lastAddedAt)
      .sort((a, b) => b.lastAddedAt! - a.lastAddedAt!)
      .slice(0, ADDED),
  );

  const words = $derived(filter.trim().toLowerCase().split(/\s+/).filter(Boolean));
  const matches = (s: SeriesSummary) => words.every((w) => s.name.toLowerCase().includes(w));
  const shownSeries = (id: number) => (series[id] ?? []).filter((s) => !words.length || matches(s));

  // Within the series page of the book, or reading it.
  const seriesHere = (id: number) => route.name === "series" && route.id === id;
</script>

<aside>
  <label class="find">
    <Icon name="search" size={14} />
    <input bind:value={filter} placeholder="Filter series" spellcheck="false" />
    {#if filter}
      <button class="clear" aria-label="Clear filter" onclick={() => (filter = "")}><Icon name="close" size={12} /></button>
    {/if}
  </label>

  <div class="scroll">
    {#if !words.length}
      <button class="row top" class:on={here({ name: "home" })} onclick={() => nav.root({ name: "home" })}>Home</button>

      {#if reading.length}
        <button class="head" onclick={() => flip("reading")}>
          <span class="chev" class:open={opened.reading}><Icon name="chevronRight" size={12} /></span>Continue reading
        </button>
        {#if opened.reading}
          {#each reading as c (c.id)}
            <div class="book" class:on={seriesHere(c.seriesId)}>
              <button class="row" title="{c.seriesName.trim()} {c.label}" onclick={() => nav.go({ name: "series", id: c.seriesId, book: c.id })}>
                <span class="name">{c.seriesName.trim()}</span>
                <span class="sub">{c.label}</span>
                <i class="bar"><i style:width="{progressOf(c) * 100}%"></i></i>
              </button>
              <button class="play" title="Read" aria-label="Read {c.seriesName.trim()} {c.label}" onclick={() => openReader(c.id)}>
                <Icon name="play" size={11} />
              </button>
            </div>
          {/each}
        {/if}
      {/if}

      <button class="head" onclick={() => flip("added")}>
        <span class="chev" class:open={opened.added}><Icon name="chevronRight" size={12} /></span>Recently added
      </button>
      {#if opened.added}
        {#each added as s (s.id)}
          <button class="row" class:on={seriesHere(s.id)} onclick={() => nav.go({ name: "series", id: s.id })}>
            <span class="name">{s.name}</span>
          </button>
        {/each}
      {/if}
    {/if}

    <div class="label">Library</div>
    {#each lib.collections as c (c.id)}
      {@const list = shownSeries(c.id)}
      {#if !words.length || list.length}
        <div class="coll" class:on={here({ name: "collection", id: c.id })}>
          <button class="chev-btn" aria-label={opened[`c${c.id}`] ? "Close" : "Open"} onclick={() => flip(`c${c.id}`)}>
            <span class="chev" class:open={opened[`c${c.id}`] || !!words.length}><Icon name="chevronRight" size={12} /></span>
          </button>
          <button class="row" onclick={() => nav.root({ name: "collection", id: c.id })}>
            <span class="name">{c.name}</span>
            <span class="count">{words.length ? list.length : c.seriesCount}</span>
          </button>
        </div>
        {#if opened[`c${c.id}`] || words.length}
          {#each list as s (s.id)}
            <button class="row series" class:on={seriesHere(s.id)} class:done={s.count > 0 && s.readCount >= s.count} onclick={() => nav.go({ name: "series", id: s.id })}>
              <span class="name">{s.name}</span>
            </button>
          {/each}
        {/if}
      {/if}
    {/each}
    {#if words.length && lib.collections.every((c) => !shownSeries(c.id).length)}
      <p class="none">No series called that.</p>
    {/if}
  </div>
</aside>

<style>
  aside {
    position: fixed;
    top: 64px;
    bottom: 0;
    left: 0;
    width: var(--side);
    z-index: 15;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--line);
    background: var(--bg);
    font-size: 13px;
  }
  .find {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 6px 14px 10px;
    padding: 0 10px;
    height: 32px;
    border-radius: 7px;
    background: var(--raised);
    color: var(--text-3);
  }
  .find:focus-within {
    color: var(--text-2);
    box-shadow: inset 0 0 0 1px var(--track);
  }
  .find input:focus-visible {
    outline: none;
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    color: var(--text);
    font-size: 13px;
  }
  .find input::placeholder {
    color: var(--text-3);
  }
  .clear {
    color: var(--text-3);
    display: flex;
  }
  .clear:hover {
    color: var(--text);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 8px 28px;
    scrollbar-width: thin;
    scrollbar-color: transparent transparent;
  }
  .scroll:hover {
    scrollbar-color: var(--track) transparent;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 5px 8px;
    border-radius: 6px;
    text-align: left;
    color: var(--text-2);
    position: relative;
  }
  .row:hover {
    color: var(--text);
    background: rgba(255, 255, 255, 0.04);
  }
  .on > .row,
  .row.on {
    color: var(--text);
    background: rgba(255, 255, 255, 0.07);
  }
  .top {
    margin-bottom: 4px;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub,
  .count {
    flex: none;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text-3);
  }
  .head,
  .label {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    margin: 14px 0 4px;
    padding: 0 8px;
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-3);
  }
  .head:hover {
    color: var(--text-2);
  }
  .label {
    margin-top: 18px;
  }
  .chev {
    display: flex;
    transition: transform 0.2s var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .book {
    position: relative;
  }
  .book .row {
    flex-wrap: wrap;
    padding-right: 30px;
    padding-bottom: 8px;
  }
  .bar {
    position: absolute;
    left: 8px;
    right: 30px;
    bottom: 3px;
    height: 2px;
    background: var(--track);
    border-radius: 1px;
  }
  .bar i {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--text-2);
    border-radius: 1px;
  }
  .play {
    position: absolute;
    right: 4px;
    top: 4px;
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--text-3);
    opacity: 0;
    transition: opacity 0.15s;
  }
  .book:hover .play {
    opacity: 1;
  }
  .play:hover {
    color: var(--bg);
    background: var(--text);
  }
  .coll {
    display: flex;
    align-items: center;
  }
  .coll .row {
    padding-left: 4px;
  }
  .chev-btn {
    flex: none;
    width: 22px;
    height: 26px;
    display: grid;
    place-items: center;
    color: var(--text-3);
  }
  .chev-btn:hover {
    color: var(--text);
  }
  .series {
    padding-left: 30px;
  }
  .series.done:not(.on) {
    color: var(--text-3);
  }
  .series.done:hover {
    color: var(--text);
  }
  .none {
    padding: 8px;
    color: var(--text-3);
  }
</style>
