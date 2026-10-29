<script lang="ts">
  import type { Snippet } from "svelte";
  import { coverUrl } from "$lib/api";

  // A name set huge, each letter cut out of a cover. Covers take the letters in turn, so with fewer
  // books than letters they repeat; with none the letters are only outlined. The name spans the
  // width unless that would make it taller than `tallest` (short names), and then `aside`, when
  // given, sits beside it if there's room (`beside` says whether it did).
  let {
    text,
    covers,
    tallest = 400,
    aside,
    asideWidth = 460,
    beside = $bindable(false),
  }: {
    text: string;
    covers: number[];
    tallest?: number;
    aside?: Snippet;
    asideWidth?: number;
    beside?: boolean;
  } = $props();

  const GAP = 64;
  const uid = $props.id();
  let width = $state(0);
  let fontReady = $state(false);
  document.fonts.load('100px "Bebas Neue"').then(() => (fontReady = true));

  const ctx = document.createElement("canvas").getContext("2d")!;
  const word = $derived(text.toUpperCase());

  const layout = $derived.by(() => {
    if (!fontReady || !width || !word.trim()) return null;
    ctx.font = '100px "Bebas Neue"';
    const m = ctx.measureText(word);
    const byHeight = (100 * tallest) / m.actualBoundingBoxAscent;
    const wide = (m.width * byHeight) / 100;
    const side = !!aside && wide + GAP + asideWidth <= width;
    const room = side ? width - GAP - asideWidth : width;
    const fs = Math.min((100 * room) / m.width, byHeight);
    ctx.font = `${fs}px "Bebas Neue"`;
    const cap = ctx.measureText(word).actualBoundingBoxAscent;
    const H = Math.ceil(cap) + 4;
    // Each letter starts where the name up to it ends, so kerning is kept.
    let k = 0;
    const letters = [...word].flatMap((ch, i) => {
      if (ch === " ") return [];
      const x = ctx.measureText(word.slice(0, i)).width;
      const w = ctx.measureText(word.slice(0, i + 1)).width - x;
      return [{ ch, x, w, cover: covers.length ? covers[k++ % covers.length] : null }];
    });
    return { fs, H, w: Math.ceil(ctx.measureText(word).width), side, letters };
  });
  $effect(() => {
    beside = layout?.side ?? false;
  });
</script>

<div class="headline" class:side={layout?.side} bind:clientWidth={width}>
  <h1 aria-label={text}>
    {#if layout}
      <svg width={layout.w} height={layout.H} aria-hidden="true">
        <defs>
          {#each layout.letters as l, i (i)}
            <clipPath id="{uid}-{i}">
              <text x={l.x} y={layout.H - 2} font-size={layout.fs}>{l.ch}</text>
            </clipPath>
          {/each}
        </defs>
        {#each layout.letters as l, i (i)}
          {#if l.cover !== null}
            <!-- The letter in a dark tone first, so it shows before its cover has loaded. -->
            <text class="under" x={l.x} y={layout.H - 2} font-size={layout.fs}>{l.ch}</text>
            <image
              href={coverUrl(l.cover)}
              x={l.x}
              y="0"
              width={l.w}
              height={layout.H}
              preserveAspectRatio="xMidYMid slice"
              clip-path="url(#{uid}-{i})"
            />
          {:else}
            <text class="outline" x={l.x} y={layout.H - 2} font-size={layout.fs}>{l.ch}</text>
          {/if}
        {/each}
      </svg>
    {/if}
  </h1>
  {#if aside && layout?.side}
    <div class="aside" style:width="{asideWidth}px">{@render aside()}</div>
  {/if}
</div>

<style>
  .headline {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 64px;
  }
  h1 {
    margin: 0;
    font-weight: 400;
    line-height: 0;
  }
  svg {
    display: block;
    overflow: visible;
    font-family: var(--display);
  }
  .under {
    fill: #161616;
  }
  .outline {
    fill: none;
    stroke: #3a3a3a;
    stroke-width: 2;
  }
  .aside {
    flex: none;
  }
</style>
