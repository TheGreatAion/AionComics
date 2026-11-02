<script lang="ts">
  import type { Snippet } from "svelte";
  import { coverUrl } from "$lib/api";

  // A name set huge, each letter cut out of a cover. Covers take the letters in turn, so with fewer
  // books than letters they repeat, each time cut from a different part of the cover; with none the
  // letters are only outlined. Behind the covers the
  // name is printed again, a little down and right, as if slightly off register: white on a dark
  // theme, black on a light one. The name spans the width unless that would make it taller than `tallest` (short names), and then `aside`, when
  // given, sits beside it if there's room (`beside` says whether it did).
  let {
    text,
    covers,
    tallest = 400,
    aside,
    asideWidth = 460,
    beside = $bindable(false),
    maxLines = 1,
  }: {
    text: string;
    covers: number[];
    tallest?: number;
    aside?: Snippet;
    asideWidth?: number;
    beside?: boolean;
    /** A long name may break onto this many lines, as evenly as it can, when that sets it bigger. */
    maxLines?: number;
  } = $props();

  const GAP = 64;
  const uid = $props.id();
  let width = $state(0);
  let fontReady = $state(false);
  document.fonts.load('100px "Bebas Neue"').then(() => (fontReady = true));

  const ctx = document.createElement("canvas").getContext("2d")!;
  const word = $derived(text.toUpperCase());

  // Words a line never ends on, since they belong with the word after them ("The / Boy Wonder"),
  // initials included ("H.P. / Lovecraft").
  const CLINGS = /^(THE|A|AN|OF|AND|&|OR|TO|IN|ON|FOR|AT|BY|FROM|WITH|VS\.?|([A-Z]\.)+)$/;
  // Words that start a phrase, so a line can begin with them ("Mighty Avengers / by Brian…").
  const OPENS = /^(THE|A|AN|OF|TO|IN|ON|FOR|AT|BY|FROM|WITH)$/;

  // Every way to break the name into `n` lines at its spaces, each as a list of lines.
  function breaks(words: string[], n: number): string[][] {
    if (n === 1) return [[words.join(" ")]];
    const out: string[][] = [];
    for (let a = 1; a <= words.length - n + 1; a++)
      for (const rest of breaks(words.slice(a), n - 1)) out.push([words.slice(0, a).join(" "), ...rest]);
    return out;
  }

  const layout = $derived.by(() => {
    if (!fontReady || !width || !word.trim()) return null;
    ctx.font = '100px "Bebas Neue"';
    const capAt100 = ctx.measureText(word).actualBoundingBoxAscent;
    // A lone dash stays with the word before it, so no line starts with one.
    const words = word
      .split(" ")
      .filter(Boolean)
      .reduce<string[]>((w, x) => (/^[-–—]$/.test(x) && w.length ? [...w.slice(0, -1), `${w[w.length - 1]} ${x}`] : [...w, x]), []);
    // Lines sit this far apart, as a share of the letters' height.
    const LEAD = 0.2;
    // Of every way to set it, on one line or a few, the one with the biggest letters, weighed by
    // where its lines break: after a dash, colon or comma is a natural pause and counts for a lot,
    // before a word that starts a phrase ("of", "by") for some, and in the middle of a phrase
    // ("Cosmic Ghost / Rider") against it. Each extra line costs a little, so fewer lines win a tie.
    let best: { rows: string[]; fs: number; side: boolean; score: number } | null = null;
    for (let n = 1; n <= Math.min(maxLines, words.length); n++) {
      for (const rows of breaks(words, n)) {
        if (rows.slice(0, -1).some((r) => CLINGS.test(r.split(" ").pop()!))) continue;
        const pause = rows.map((r, i) => i < n - 1 && /[-–—:,]$/.test(r));
        const widths = rows.map((r) => ctx.measureText(r).width);
        const widest = Math.max(...widths);
        // No line much shorter than the longest, so no word is left on its own; a line ending in a
        // pause may be shorter ("Batman – / The Killing Joke").
        if (widths.some((w, i) => w < widest * (pause[i] ? 0.25 : 0.5))) continue;
        const byHeight = (100 * tallest) / (capAt100 * (n + LEAD * (n - 1)));
        const wide = (widest * byHeight) / 100;
        const side = n === 1 && !!aside && wide + GAP + asideWidth <= width;
        const room = side ? width - GAP - asideWidth : width;
        const fs = Math.min((100 * room) / widest, byHeight);
        const pauses = pause.filter(Boolean).length;
        const opens = rows.slice(1).filter((r, i) => !pause[i] && OPENS.test(r.split(" ")[0])).length;
        const mid = n - 1 - pauses - opens;
        const score = fs * (1 + 0.25 * pauses + 0.12 * opens - 0.1 * mid - 0.08 * (n - 1));
        if (!best || score > best.score * 1.001) best = { rows, fs, side, score };
      }
      ctx.font = '100px "Bebas Neue"';
    }
    const { rows, fs, side } = best!;
    ctx.font = `${fs}px "Bebas Neue"`;
    const cap = ctx.measureText(word).actualBoundingBoxAscent;
    const lineH = Math.ceil(cap) + 4;
    const gap = Math.round(cap * LEAD);
    const w = Math.ceil(Math.max(...rows.map((r) => ctx.measureText(r).width)));
    // Each letter starts where its line up to it ends, so kerning is kept; each line is centered.
    // When covers repeat, each letter shows its cover zoomed in and from its own spot on it (the
    // face, the logo, a corner), so the letters don't all look alike.
    const repeat = covers.length > 0 && covers.length < [...word].filter((ch) => ch !== " ").length;
    const spot = (k: number, step: number) => (k * step + 0.3) % 1;
    let k = 0;
    const letters = rows.flatMap((row, r) => {
      const top = r * (lineH + gap);
      const indent = (w - ctx.measureText(row).width) / 2;
      return [...row].flatMap((ch, i) => {
        if (ch === " ") return [];
        const x = indent + ctx.measureText(row.slice(0, i)).width;
        const cw = ctx.measureText(row.slice(0, i + 1)).width - x + indent;
        let crop = { x, y: top, w: cw, h: lineH, fit: "xMidYMid slice" };
        if (repeat) {
          // Two to four lines tall, so a letter shows a part of the cover, not all of it; kept
          // below the top quarter or so, where a cover's title and logo sit.
          const h = lineH * (2.4 + 1.4 * spot(k, 0.618));
          const w = Math.max(cw, h * 0.66);
          // From the right, so a wraparound cover (back and front side by side) shows its front.
          crop = { x: x - spot(k, 0.382) * (w - cw), y: top - (0.3 + 0.7 * spot(k, 0.754)) * (h - lineH), w, h, fit: "xMaxYMid slice" };
        }
        return [{ ch, x, top, base: top + lineH - 2, crop, cover: covers.length ? covers[k++ % covers.length] : null }];
      });
    });
    // How far the second print sits off the first, and how heavy the ink line round each letter
    // is, in step with the letters' size.
    const off = { x: fs * 0.014, y: fs * 0.0125 };
    const line = Math.max(2, fs * 0.006);
    return { fs, H: rows.length * lineH + (rows.length - 1) * gap, lineH, w, side, letters, off, line };
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
              <text x={l.x} y={l.base} font-size={layout.fs}>{l.ch}</text>
            </clipPath>
          {/each}
        </defs>
        {#if covers.length}
          {#each layout.letters as l, i (i)}
            <text class="print" x={l.x + layout.off.x} y={l.base + layout.off.y} font-size={layout.fs}>{l.ch}</text>
          {/each}
        {/if}
        {#each layout.letters as l, i (i)}
          {#if l.cover !== null}
            <!-- The letter in a dark tone first, so it shows before its cover has loaded. -->
            <text class="under" x={l.x} y={l.base} font-size={layout.fs}>{l.ch}</text>
            <image
              href={coverUrl(l.cover)}
              x={l.crop.x}
              y={l.crop.y}
              width={l.crop.w}
              height={l.crop.h}
              preserveAspectRatio={l.crop.fit}
              clip-path="url(#{uid}-{i})"
            />
            <!-- An ink line round the letter keeps its shape on a pale cover. -->
            <text class="line" x={l.x} y={l.base} font-size={layout.fs} stroke-width={layout.line}>{l.ch}</text>
          {:else}
            <text class="outline" x={l.x} y={l.base} font-size={layout.fs}>{l.ch}</text>
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
  /* The name centered; with its aside beside it, the two share the width. */
  .headline {
    display: flex;
    align-items: flex-start;
    justify-content: center;
    gap: 64px;
  }
  .headline.side {
    justify-content: space-between;
  }
  h1 {
    margin: 0;
    font-weight: 400;
    line-height: 0;
  }
  /* Clicks go through: a letter's text box reaches well past its shape, over whatever is below
     (a collection's filter and sort buttons). */
  svg {
    display: block;
    overflow: visible;
    font-family: var(--display);
    pointer-events: none;
  }
  .under {
    fill: var(--raised);
  }
  .print {
    fill: var(--print);
  }
  .line {
    fill: none;
    stroke: var(--ink);
    stroke-linejoin: round;
  }
  .outline {
    fill: none;
    stroke: var(--track);
    stroke-width: 2;
  }
  .aside {
    flex: none;
  }
</style>
