<script lang="ts">
  import { coverUrl } from "$lib/api";

  // ratio = height / width; reserving it up front keeps rows from jumping.
  let {
    id,
    ratio = null,
    width,
    pages,
  }: {
    id: number | null | undefined;
    ratio?: number | null;
    /** Fixed width in px; otherwise the cover fills its column. */
    width?: number;
    /**
     * Draw it as a book: a spine down the left and the edge of its pages on the right, as thick as
     * its page count (null when that isn't known: a slim book). Left out, it's a flat picture.
     */
    pages?: number | null;
  } = $props();

  let el = $state<HTMLDivElement>();
  let img = $state<HTMLImageElement>();
  let measured = $state(0);
  // Which cover has finished loading. Keyed by id rather than reset on change: a cached image can
  // finish before any reset runs, and a reset would then hide it for good.
  let loadedId = $state<number | null>(null);
  const loaded = $derived(loadedId === id && id != null);
  // Odd scans (spines, double-wide covers) fall back to a normal book shape.
  const shape = $derived(ratio && ratio > 0.9 && ratio < 2.1 ? ratio : 1.5);

  // How thick the page edge is: about 1px per 55 pages on a 150px-wide cover, scaled with the
  // cover, so an omnibus looks heavy beside a single issue at any size.
  const book = $derived(pages !== undefined);
  const edge = $derived.by(() => {
    if (!book) return 0;
    const w = width ?? measured;
    const count = pages ?? 60;
    return Math.round(Math.max(2, Math.min(16, (count / 55) * (w / 150))));
  });

  // An image can be complete before its load listener is attached.
  $effect(() => {
    if (img?.complete && img.naturalWidth > 0) loadedId = id ?? null;
  });
</script>

<div
  class="cover"
  class:book
  bind:this={el}
  bind:clientWidth={measured}
  style:aspect-ratio="1 / {shape}"
  style:width={width ? `${width}px` : undefined}
  style:--edge="{edge}px"
>
  <div class="face">
    {#if id != null}
      {#key id}
        <img
          bind:this={img}
          src={coverUrl(id)}
          alt=""
          loading="lazy"
          decoding="async"
          draggable="false"
          class:loaded
          onload={() => (loadedId = id)}
        />
      {/key}
    {/if}
  </div>
  {#if edge}<div class="pages" aria-hidden="true"></div>{/if}
</div>

<style>
  .cover {
    position: relative;
    width: 100%;
    flex: none;
    border-radius: 2px;
  }
  .face {
    position: absolute;
    inset: 0;
    overflow: hidden;
    border-radius: inherit;
    background: var(--raised);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0;
    transition: opacity 0.5s;
  }
  img.loaded {
    opacity: 1;
  }
  /* The spine: where a real cover folds round, a soft crease down the left. */
  .book .face::after {
    content: "";
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 6%;
    background: linear-gradient(to right, rgba(0, 0, 0, 0.38), rgba(0, 0, 0, 0.08) 55%, rgba(255, 255, 255, 0.07) 70%, transparent);
    pointer-events: none;
  }
  /* The block of pages showing past the cover's right edge, a shade inset top and bottom. */
  .pages {
    position: absolute;
    top: 2px;
    bottom: 1px;
    left: 100%;
    width: var(--edge);
    border-radius: 0 2px 2px 0;
    background: repeating-linear-gradient(to right, #ebe6da 0 2px, #d6d0c1 2px 3px);
    box-shadow:
      inset -1px 0 0 rgba(0, 0, 0, 0.18),
      inset 0 1px 0 rgba(255, 255, 255, 0.4);
    filter: brightness(0.86);
  }
</style>
