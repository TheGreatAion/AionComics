<script lang="ts">
  import { api, progressOf, storyTitle, type ComicCard } from "$lib/api";
  import { lib, nav, openReader, sheets, showMenu } from "$lib/state.svelte";
  import Cover from "./Cover.svelte";

  let {
    comic,
    width,
    mode,
  }: {
    comic: ComicCard;
    width?: number;
    /**
     * shelf: a book on Home, captioned with its series (which links to the series page).
     * volume: a book on its series page, captioned with its label; finished ones are dimmed.
     */
    mode: "shelf" | "volume";
  } = $props();

  const progress = $derived(progressOf(comic));
  const reading = $derived(!comic.read && comic.currentPage > 0);
  const name = $derived(mode === "shelf" ? comic.seriesName.trim() : comic.label);
  const detail = $derived.by(() => {
    if (mode === "shelf") return comic.label;
    if (comic.read) return "Read";
    if (reading && comic.numPages) return `${comic.currentPage + 1} / ${comic.numPages}`;
    return storyTitle(comic) ?? comic.year ?? "";
  });

  function open(e: MouseEvent) {
    const img = (e.currentTarget as HTMLElement).querySelector("img");
    openReader(comic.id, img);
  }

  async function act(fn: () => Promise<void>) {
    await fn();
    lib.version++;
  }

  function more(e: MouseEvent) {
    showMenu(e, [
      { label: reading ? "Resume" : "Read", action: () => openReader(comic.id) },
      { label: `Go to ${comic.seriesName.trim()}`, action: () => nav.go({ name: "series", id: comic.seriesId }) },
      "separator",
      comic.read
        ? { label: "Mark as unread", action: () => act(() => api.setRead(comic.id, false)) }
        : { label: "Mark as read", action: () => act(() => api.setRead(comic.id, true)) },
      { label: "Mark earlier ones as read", action: () => act(() => api.markReadBefore(comic.id)) },
      "separator",
      { label: "Edit details…", action: () => (sheets.editing = comic.id) },
      { label: "Show in Explorer", action: () => api.reveal(comic.id) },
    ]);
  }
</script>

<div class="book" class:done={mode === "volume" && comic.read} style:width={width ? `${width}px` : undefined}>
  <button class="hit" onclick={open} oncontextmenu={more} title="{comic.seriesName.trim()} {comic.label}">
    <Cover id={comic.id} ratio={comic.coverRatio} />
  </button>
  <!-- Every tile reserves the reading line and two caption lines, so covers in a row share a baseline. -->
  {#if reading}
    <div class="prog"><i style:width="{progress * 100}%"></i></div>
  {:else}
    <div class="prog blank"></div>
  {/if}
  <div class="cap">
    {#if mode === "shelf"}
      <button class="name" onclick={() => nav.go({ name: "series", id: comic.seriesId })}>{name}</button>
    {:else}
      <span class="name">{name}</span>
    {/if}
    <span class="detail">{detail}</span>
  </div>
</div>

<style>
  .book {
    flex: none;
    min-width: 0;
  }
  .hit {
    display: block;
    width: 100%;
    transition:
      transform 0.5s var(--ease),
      opacity 0.4s;
  }
  .hit :global(.cover) {
    transition: box-shadow 0.5s var(--ease);
  }
  .hit:hover {
    transform: translateY(-4px);
  }
  .hit:hover :global(.cover) {
    box-shadow: 0 18px 40px -12px rgba(0, 0, 0, 0.9);
  }
  .done .hit {
    opacity: 0.6;
  }
  .done .hit:hover {
    opacity: 1;
  }
  .prog {
    margin-top: 10px;
  }
  .cap {
    margin-top: 13px;
    height: 56px;
  }
  .name {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-align: left;
    font-size: 13px;
    line-height: 1.35;
    color: var(--cap);
    transition: color 0.2s;
  }
  button.name:hover,
  .book:hover .name {
    color: var(--text);
  }
  .detail {
    display: block;
    margin-top: 3px;
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
