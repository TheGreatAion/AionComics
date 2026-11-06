<script lang="ts">
  import { thumbUrl } from "$lib/api";
  import type { Ctl } from "./ctl.svelte";

  // The page line as a row of ticks, one per page (or per few pages in a long book). Hovering shows
  // the page under the pointer (or its spread, in double page mode); a click goes there and a drag scrubs.
  let { c }: { c: Ctl } = $props();

  const SPACING = 7; // fewest pixels between ticks before pages start sharing one

  let width = $state(0);
  let row = $state<HTMLDivElement>();
  let hover = $state<{ page: number; x: number } | null>(null);
  let dragging = false;

  const per = $derived(Math.max(1, Math.ceil(c.count / Math.max(1, Math.floor(width / SPACING)))));
  const ticks = $derived.by(() => {
    const shown = c.shown;
    const [lo, hi] = [shown[0], shown[shown.length - 1]];
    const marks = new Set(c.bookmarks);
    return Array.from({ length: Math.ceil(c.count / per) }, (_, i) => {
      const [a, b] = [i * per, Math.min(c.count, (i + 1) * per) - 1];
      let mark = false;
      for (let p = a; p <= b && !mark; p++) mark = marks.has(p);
      return { a, b, here: a <= hi && b >= lo, read: b < lo, mark };
    });
  });

  function pageAt(clientX: number) {
    const r = row!.getBoundingClientRect();
    let f = Math.min(1, Math.max(0, (clientX - r.left) / r.width));
    if (c.flipped) f = 1 - f;
    return { page: Math.min(c.last, Math.floor(f * c.count)), x: clientX - r.left };
  }

  function move(e: PointerEvent) {
    hover = pageAt(e.clientX);
    if (dragging && !c.shown.includes(hover.page)) c.goTo(hover.page);
  }
  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    dragging = true;
    row!.setPointerCapture(e.pointerId);
    move(e);
    c.goTo(hover!.page);
  }
  function up(e: PointerEvent) {
    dragging = false;
    row?.releasePointerCapture(e.pointerId);
  }

  // The preview shows what a click would put on screen: in double page mode, the whole spread.
  const spread = $derived(hover ? c.spreadAt(hover.page) : []);
  const PAGE_W = 87; // one page in the preview
  const previewW = $derived(spread.length * PAGE_W + (spread.length - 1) * 2 + 8);
  // The preview stays inside the row, however near the ends the pointer is.
  const previewX = $derived(hover ? Math.min(width - previewW / 2, Math.max(previewW / 2, hover.x)) : 0);
</script>

<div
  class="ticks"
  class:flipped={c.flipped}
  bind:this={row}
  bind:clientWidth={width}
  onpointermove={move}
  onpointerdown={down}
  onpointerup={up}
  onpointerleave={() => !dragging && (hover = null)}
  role="slider"
  tabindex="-1"
  aria-label="Page"
  aria-valuemin={1}
  aria-valuemax={c.count}
  aria-valuenow={c.page + 1}
>
  {#each ticks as t (t.a)}
    {@const hot = spread.some((p) => p >= t.a && p <= t.b)}
    <span class="tick" class:here={t.here} class:read={t.read} class:hot>
      {#if t.mark}<i class="mark"></i>{/if}
    </span>
  {/each}

  {#if hover}
    <div class="preview" style:left="{previewX}px" style:width="{previewW}px">
      <div class="pages" class:flipped={c.flipped}>
        {#each spread as p (p)}<img src={thumbUrl(c.id, p)} alt="" draggable="false" />{/each}
      </div>
      <span class="fine">{spread.map((p) => p + 1).join("–")}</span>
    </div>
  {/if}
</div>

<style>
  .ticks {
    position: relative;
    flex: 1;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: pointer;
    touch-action: none;
  }
  .ticks.flipped {
    flex-direction: row-reverse;
  }
  .tick {
    position: relative;
    width: 1px;
    height: 5px;
    background: #4a4a4a;
    transition:
      height 0.2s var(--ease),
      background 0.2s;
  }
  .tick.read {
    background: #bdbdbd;
  }
  .tick.hot {
    height: 10px;
    background: #d6d6d6;
  }
  .tick.here {
    height: 14px;
    background: #f0f0f0;
  }
  .mark {
    position: absolute;
    left: -1px;
    bottom: calc(100% + 4px);
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: #9a9a9a;
  }
  .preview {
    position: absolute;
    bottom: calc(100% + 14px);
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 4px 4px 5px;
    border-radius: 6px;
    background: rgba(18, 18, 18, 0.94);
    border: 1px solid rgba(255, 255, 255, 0.07);
    pointer-events: none;
  }
  .pages {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .pages.flipped {
    flex-direction: row-reverse;
  }
  .preview img {
    flex: 1;
    min-width: 0;
    min-height: 75px;
    border-radius: 2px;
    background: #222;
  }
  .preview .fine {
    color: #d0d0d0;
    font-size: 12px;
  }
</style>
