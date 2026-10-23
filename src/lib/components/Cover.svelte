<script lang="ts">
  import { coverUrl } from "$lib/api";
  import { land } from "$lib/flight";

  // ratio = height / width; reserving it up front keeps rows from jumping.
  let {
    id,
    ratio = null,
    width,
    flip,
  }: {
    id: number | null | undefined;
    ratio?: number | null;
    /** Fixed width in px; otherwise the cover fills its column. */
    width?: number;
    /** Shared-element key: a cover launched with the same key flies in here. */
    flip?: string;
  } = $props();

  let el = $state<HTMLDivElement>();
  let img = $state<HTMLImageElement>();
  // Which cover has finished loading. Keyed by id rather than reset on change: a cached image can
  // finish before any reset runs, and a reset would then hide it for good.
  let loadedId = $state<number | null>(null);
  const loaded = $derived(loadedId === id && id != null);
  // Odd scans (spines, double-wide covers) fall back to a normal book shape.
  const shape = $derived(ratio && ratio > 0.9 && ratio < 2.1 ? ratio : 1.5);

  // An image can be complete before its load listener is attached.
  $effect(() => {
    if (img?.complete && img.naturalWidth > 0) loadedId = id ?? null;
  });

  $effect(() => {
    if (el && flip) land(el, flip);
  });
</script>

<div class="cover" bind:this={el} style:aspect-ratio="1 / {shape}" style:width={width ? `${width}px` : undefined}>
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

<style>
  .cover {
    position: relative;
    width: 100%;
    flex: none;
    overflow: hidden;
    border-radius: 2px;
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
</style>
