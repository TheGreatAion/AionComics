<script lang="ts" module>
  /** Shown or hidden, remembered between sessions (Ctrl B, or the panel button). */
  export const side = $state({ open: load() });

  function load() {
    try {
      return localStorage.getItem("aion.side") !== "0";
    } catch {
      return true;
    }
  }

  /** The row comics are being dragged over ("series:12", "collection:3"), set by the drop zone. */
  export const dropOn = $state({ key: null as string | null });

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
  // lately, and every collection opening into its series, with a filter over all of them. While
  // it's open it is the app's navigation: the top bar keeps only back, search, stats and settings.
  import { addMenu, adding } from "$lib/add.svelte";
  import { libraryMenu, seriesMenu } from "$lib/manage.svelte";
  import { api, progressOf, type ComicCard, type SeriesSummary } from "$lib/api";
  import { lib, nav, openReader, showMenu, undoable } from "$lib/state.svelte";
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

  // Fresh data whenever the library changes. The lists on screen stay until their fresh ones arrive,
  // so nothing collapses and jumps back.
  $effect(() => {
    void lib.version;
    api.home().then((h) => (reading = h.continueReading.slice(0, READING)));
  });

  // Series lists load when needed: a collection opened, or everything for "Recently added" and
  // the filter.
  const loading = new Set<number>();
  const loadedAt = new Map<number, number>();
  function need(id: number) {
    const version = lib.version;
    if (loadedAt.get(id) === version || loading.has(id)) return;
    loading.add(id);
    loadedAt.set(id, version);
    api
      .collection(id)
      .then((v) => (series[id] = [...v.series].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }))))
      .finally(() => {
        loading.delete(id);
        // The library changed again while this was on its way.
        if (lib.version !== version) need(id);
      });
  }
  $effect(() => {
    for (const c of lib.collections) if (opened[`c${c.id}`] || opened.added || filter) need(c.id);
  });

  const added = $derived(
    lib.collections
      .flatMap((c) => series[c.id] ?? [])
      .filter((s) => s.lastAddedAt)
      .sort((a, b) => b.lastAddedAt! - a.lastAddedAt!)
      .slice(0, ADDED),
  );

  const words = $derived(filter.trim().toLowerCase().split(/\s+/).filter(Boolean));
  const matches = (s: SeriesSummary) => words.every((w) => s.name.toLowerCase().includes(w));
  const shownSeries = (id: number) => (series[id] ?? []).filter((s) => !words.length || matches(s));

  // On the series page of this series (or of a book in it).
  const seriesHere = (id: number) => route.name === "series" && route.id === id;

  // Comics held over a folded collection for a moment open it, so a series inside can be aimed at.
  $effect(() => {
    const key = dropOn.key;
    if (!key?.startsWith("collection:") || opened[`c${key.slice(11)}`]) return;
    const t = setTimeout(() => flip(`c${key.slice(11)}`), 700);
    return () => clearTimeout(t);
  });
</script>

<!-- Always there, slid away when hidden, so it can glide in and out. -->
<aside class:open={side.open} inert={!side.open} aria-label="Library">
  <!-- The panel's top is the window's top: where the app's name sits, and where it's dragged from. -->
  <div class="top" data-tauri-drag-region>
    <button class="ico" title="Hide the side panel (Ctrl B)" aria-label="Hide the side panel" onclick={toggleSide}>
      <Icon name="sidebar" size={17} />
    </button>
    <button class="mark" onclick={() => nav.root({ name: "home" })}>AionComics</button>
  </div>

  <label class="find">
    <Icon name="search" size={14} />
    <input bind:value={filter} placeholder="Find a series" spellcheck="false" />
    {#if filter}
      <button class="clear" aria-label="Clear" onclick={() => (filter = "")}><Icon name="close" size={12} /></button>
    {/if}
  </label>

  <div class="scroll">
    {#if !words.length}
      <button class="row home" class:on={here({ name: "home" })} onclick={() => nav.root({ name: "home" })}>Home</button>

      {#if reading.length}
        <button class="head" onclick={() => flip("reading")}>
          Continue reading<span class="chev" class:open={opened.reading}><Icon name="chevronRight" size={11} /></span>
        </button>
        {#if opened.reading}
          {#each reading as c (c.id)}
            <div
              class="book"
              class:on={seriesHere(c.seriesId)}
              class:dropping={dropOn.key === `series:${c.seriesId}`}
              data-drop="series:{c.seriesId}"
              role="group"
              oncontextmenu={(e) =>
                showMenu(e, [
                  { label: "Read", action: () => openReader(c.id) },
                  "separator",
                  { label: "Remove from Continue reading", action: () => undoable(`${c.seriesName.trim()} ${c.label} taken off`, () => api.hideFromReading(c.id)) },
                ])}
            >
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

      {#if added.length}
        <button class="head" onclick={() => flip("added")}>
          Recently added<span class="chev" class:open={opened.added}><Icon name="chevronRight" size={11} /></span>
        </button>
      {/if}
      {#if opened.added}
        {#each added as s (s.id)}
          <button
            class="row"
            class:on={seriesHere(s.id)}
            class:dropping={dropOn.key === `series:${s.id}`}
            data-drop="series:{s.id}"
            onclick={() => nav.go({ name: "series", id: s.id })}
            oncontextmenu={(e) => seriesMenu(e, s, seriesHere(s.id))}
          >
            <span class="name">{s.name}</span>
          </button>
        {/each}
      {/if}
    {/if}

    <div class="head label">
      Library
      <button class="fold new" title="New library" aria-label="New library" onclick={() => (adding.newLibrary = true)}>
        <Icon name="plus" size={13} />
      </button>
    </div>
    <!-- Your Want to read list, first among the libraries. -->
    {#if !words.length}
      <div class="coll want" class:on={here({ name: "wanted" })}>
        <button class="row" onclick={() => nav.root({ name: "wanted" })}>
          <span class="icon"><Icon name="bookmark" size={14} /></span>
          <span class="name">Want to read</span>
          <span class="count">{lib.wanted}</span>
        </button>
        <span class="fold"></span>
      </div>
    {/if}
    {#each lib.collections as c (c.id)}
      {@const list = shownSeries(c.id)}
      {@const unfolded = opened[`c${c.id}`] || !!words.length}
      {#if !words.length || list.length}
        <div
          class="coll"
          class:on={here({ name: "collection", id: c.id })}
          class:dropping={dropOn.key === `collection:${c.id}`}
          data-drop="collection:{c.id}"
          role="group"
          oncontextmenu={(e) => libraryMenu(e, c, here({ name: "collection", id: c.id }))}
        >
          <button class="row" onclick={() => nav.root({ name: "collection", id: c.id })}>
            <span class="name">{c.name}</span>
            <span class="count">{words.length ? list.length : c.seriesCount}</span>
          </button>
          {#if unfolded}
            <button
              class="fold"
              title="Add a series to {c.name}"
              aria-label="Add a series to {c.name}"
              onclick={(e) => addMenu(e, { collection: c.id, title: c.name })}
            >
              <Icon name="plus" size={13} />
            </button>
          {/if}
          <button class="fold" aria-label={unfolded ? `Fold away ${c.name}` : `Show the series in ${c.name}`} onclick={() => flip(`c${c.id}`)}>
            <span class="chev" class:open={unfolded}><Icon name="chevronRight" size={12} /></span>
          </button>
        </div>
        {#if unfolded}
          <div class="list">
            {#each list as s (s.id)}
              <button
                class="row series"
                class:on={seriesHere(s.id)}
                class:done={s.count > 0 && s.readCount >= s.count}
                class:dropping={dropOn.key === `series:${s.id}`}
                data-drop="series:{s.id}"
                onclick={() => nav.go({ name: "series", id: s.id })}
                oncontextmenu={(e) => seriesMenu(e, s, seriesHere(s.id))}
              >
                <span class="name">{s.name}</span>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    {/each}
    {#if words.length && lib.collections.every((c) => !shownSeries(c.id).length)}
      <p class="none">No series called that.</p>
    {/if}
  </div>
</aside>

<style>
  /* Glass over the page, like the top bar once you scroll: the cover's color runs on underneath. */
  aside {
    position: fixed;
    top: 0;
    bottom: 0;
    left: 0;
    width: var(--side);
    z-index: 25;
    display: flex;
    flex-direction: column;
    background: var(--side-bg);
    backdrop-filter: blur(28px);
    box-shadow: inset -1px 0 0 rgb(var(--fg) / 0.04);
    transform: translateX(-100%);
    transition: transform 0.45s var(--ease);
  }
  aside.open {
    transform: none;
  }
  .top {
    flex: none;
    height: 64px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 18px;
  }
  .mark {
    font-weight: 600;
    font-size: 15px;
    letter-spacing: -0.01em;
  }
  /* The same quiet underline as the filter on a collection's page. */
  .find {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 2px 20px 14px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
    transition: border-color 0.2s;
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
    color: var(--text);
  }
  .find input::placeholder {
    color: var(--text-3);
  }
  .find input:focus-visible {
    outline: none;
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
    padding: 0 10px 32px;
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
    padding: 5px 10px;
    border-radius: 7px;
    text-align: left;
    color: var(--text-2);
    position: relative;
    transition:
      color 0.15s,
      background 0.15s;
  }
  .row:hover {
    color: var(--text);
  }
  .on > .row,
  .row.on {
    color: var(--text);
    background: rgb(var(--fg) / 0.07);
  }
  /* The row comics will go into when they're let go. */
  .dropping > .row,
  .row.dropping {
    color: var(--text);
    background: rgb(var(--fg) / 0.14);
    box-shadow: inset 0 0 0 1px rgb(var(--fg) / 0.28);
  }
  .home {
    font-size: 15px;
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
    font-size: 12px;
    color: var(--text-3);
  }
  /* Section titles as the rest of the app writes them ("Library", "Today"): plain, a size up. */
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    margin: 26px 0 8px;
    padding: 0 10px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }
  .head .chev {
    color: var(--text-3);
    opacity: 0;
    transition:
      opacity 0.2s,
      transform 0.25s var(--ease);
  }
  /* The fold arrow shows when it matters: pointed at, or folded away. */
  .head:hover .chev,
  .head .chev:not(.open) {
    opacity: 1;
  }
  .chev {
    display: flex;
    transition: transform 0.25s var(--ease);
  }
  .chev.open {
    transform: rotate(90deg);
  }
  .book {
    position: relative;
  }
  .book .row {
    flex-wrap: wrap;
    padding-right: 32px;
    padding-bottom: 9px;
  }
  .bar {
    position: absolute;
    left: 10px;
    right: 32px;
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
  /* Collections in the big titles' face, as on Home and their own pages. */
  .coll {
    display: flex;
    align-items: center;
  }
  .coll .row {
    align-items: center;
  }
  .coll .name {
    font-family: var(--display);
    font-size: 21px;
    line-height: 1.15;
    letter-spacing: 0.01em;
  }
  .want .icon {
    flex: none;
    display: flex;
    align-self: center;
    margin-right: -1px;
    color: var(--text-2);
  }
  .fold {
    flex: none;
    width: 26px;
    height: 30px;
    display: grid;
    place-items: center;
    color: var(--text-3);
  }
  .fold:hover {
    color: var(--text);
  }
  /* In line with the collections' own buttons below it. */
  .fold.new {
    margin-left: auto;
    margin-right: -10px;
    height: 20px;
  }
  .list {
    margin: 2px 0 10px;
  }
  .series {
    padding-left: 18px;
  }
  .series.done:not(.on) {
    color: var(--text-3);
  }
  .series.done:hover {
    color: var(--text);
  }
  .none {
    padding: 8px 10px;
    color: var(--text-3);
  }
</style>
