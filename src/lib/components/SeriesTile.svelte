<script lang="ts">
  // A series (or one book) in a grid of covers: its cover, a run of books standing as a small pile
  // behind it, a slim gray bar that fills in as you read, and its name. A ribbon on the cover says
  // it's on your Want to read list.
  import Cover from "./Cover.svelte";

  let {
    coverId,
    ratio,
    books = 1,
    read = 0,
    name,
    detail,
    tag,
    ribbon = false,
    onclick,
    oncontextmenu,
  }: {
    coverId: number | null;
    ratio: number | null;
    /** How many books it holds: two or more stand behind the cover. */
    books?: number;
    /** How many of them are read. */
    read?: number;
    name: string;
    detail: string;
    /** A small paper label before the detail ("Book"). */
    tag?: string;
    ribbon?: boolean;
    onclick: (e: MouseEvent) => void;
    oncontextmenu?: (e: MouseEvent) => void;
  } = $props();
</script>

<button class="tile" data-nav {onclick} {oncontextmenu}>
  <span class="art">
    {#if books > 2}<span class="sheet far"></span>{/if}
    {#if books > 1}<span class="sheet"></span>{/if}
    <Cover id={coverId} {ratio} pages={null} />
    {#if ribbon}
      <svg class="ribbon" width="18" height="28" viewBox="0 0 18 28" role="img" aria-label="On your Want to read list">
        <path d="M1.5 0v26L9 20.5l7.5 5.5V0" fill="#e6dfd1" stroke="#0d0c0b" stroke-width="1.5" stroke-linejoin="round" />
      </svg>
    {/if}
  </span>
  <span class="mark" title={read ? `${read} of ${books} read` : undefined}>
    {#if read > 0}<i style:width="{(read / books) * 100}%"></i>{/if}
  </span>
  <span class="cap">
    <span class="name">{name}</span>
    <span class="detail">{#if tag}<span class="tag">{tag}</span>{/if}{detail}</span>
  </span>
</button>

<style>
  .tile {
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
  .tile:hover .sheet {
    transform: translate(7px, -7px);
  }
  .tile:hover .sheet.far {
    transform: translate(14px, -14px);
  }
  .art :global(.cover) {
    transition: box-shadow 0.5s var(--ease);
  }
  .tile:hover .art {
    transform: translateY(-4px);
  }
  .tile:hover .art :global(.cover) {
    box-shadow: 0 18px 40px -12px rgba(0, 0, 0, 0.9);
  }
  /* A paper bookmark, inked round its edge, tucked in over the top of the cover. */
  .ribbon {
    position: absolute;
    top: -5px;
    right: 12px;
    z-index: 1;
    filter: drop-shadow(0 2px 2px rgb(0 0 0 / 0.5));
  }
  /* A slim gray track; what you've read fills in from the left. */
  .mark {
    position: relative;
    display: block;
    height: 3px;
    margin-top: 10px;
    border-radius: 1px;
    background: var(--track);
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
  .tile:hover .name {
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
  .tag {
    display: inline-block;
    margin-right: 6px;
    padding: 0 5px;
    border-radius: 2px;
    background: var(--text);
    color: var(--bg);
    font-size: 12px;
    line-height: 1.4;
  }
</style>
