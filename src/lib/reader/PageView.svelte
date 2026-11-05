<script lang="ts">
  import { pageUrl, previewUrl } from "$lib/api";
  import type { Ctl } from "./ctl.svelte";
  import PageStatus from "./PageStatus.svelte";
  import { DOUBLE_CLICK_MS, prefs } from "./prefs.svelte";

  /** Turns this close together are skipping; loading waits until none came for QUIET_MS. */
  const SKIPPING_MS = 90;
  const QUIET_MS = 70;

  let {
    c,
    onzone,
    onready,
    img = $bindable(),
  }: {
    c: Ctl;
    onzone: (z: "left" | "middle" | "right") => void;
    /** First time a page is placed, with its element (the reader flies the cover onto it). */
    onready: (img: HTMLImageElement) => void;
    img?: HTMLImageElement;
  } = $props();

  let stage = $state<HTMLDivElement>();
  let area = $state<HTMLDivElement>();
  let vw = $state(0);
  let vh = $state(0);
  let announced = false;
  // A page that failed to load (shown with a retry), and a counter that forces a fresh request.
  let failed = $state<number | null>(null);
  let retry = $state(0);
  const src = (i: number) => c.src(i) + (retry ? `?retry=${retry}` : "");

  $effect(() => {
    c.stage = stage ?? null;
    return () => (c.stage = null);
  });

  const shown = $derived(c.shown);
  // On screen left to right: a right-to-left spread puts the second page on the left.
  const order = $derived(c.flipped && shown.length === 2 ? [shown[1], shown[0]] : shown);
  // A page still loading is laid out by its preview, which is drawn in its place.
  const dims = $derived(order.map((i) => c.dims[i] ?? c.hints[i]));
  $effect(() => {
    for (const i of order) if (!c.dims[i]) c.preview(i);
  });
  const behind = (i: number) => (c.hints[i] ? `url(${previewUrl(c.id, i)})` : undefined);
  const ready = $derived(dims.every(Boolean));
  // Lay pages out at a common height of 1000 units, side by side.
  const unitW = $derived(dims.reduce((s, d) => s + (d ? (d.w * 1000) / d.h : 0), 0));
  const turned = $derived(c.rotation % 180 !== 0);

  const box = $derived.by(() => {
    if (!ready || !vw || !vh || !unitW) return null;
    const W = turned ? 1000 : unitW;
    const H = turned ? unitW : 1000;
    let s: number;
    switch (c.view.fit) {
      case "height":
        s = vh / H;
        break;
      case "width":
        s = vw / W;
        break;
      case "full":
        s = dims[0]!.h / 1000;
        break;
      default:
        s = Math.min(vw / W, vh / H);
    }
    s *= c.zoom;
    return { w: W * s, h: H * s, cw: unitW * s, ch: 1000 * s };
  });

  // Pages are sized from their shapes, so one still loading (no picture yet) keeps its place.
  const width = (k: number) => (box && dims[k] ? `${(dims[k]!.w / dims[k]!.h) * box.ch}px` : undefined);

  const overflows = $derived(!!box && (box.w > vw + 1 || box.h > vh + 1));

  // Skipping (a held arrow key, a drag along the ticks): pages flying past aren't worth loading.
  // A page turned to right after another waits for a moment's quiet, unless it's loaded already.
  let quiet = $state(true);
  let turnedAt = 0;
  $effect(() => {
    void c.page;
    const now = performance.now();
    if (now - turnedAt < SKIPPING_MS) quiet = false;
    turnedAt = now;
    const t = setTimeout(() => (quiet = true), QUIET_MS);
    return () => clearTimeout(t);
  });
  const live = (i: number) => quiet || !!c.dims[i];

  // Once the pages themselves are on screen (not just their previews), look a few pages ahead and
  // one spread behind so pairing and page turns are instant, even turning quickly. Not before:
  // they'd hold up the page itself.
  let painted = $state<Record<number, boolean>>({});
  $effect(() => {
    void c.id;
    painted = {};
  });
  $effect(() => {
    if (!quiet || !order.every((i) => painted[i])) return;
    const p = c.page;
    c.settleAt(p);
    for (const d of [1, 2, 3, 4, 5, -1, -2]) c.measure(p + d);
  });

  // New page: back to the top, and to the right-hand edge when reading right to left.
  $effect(() => {
    void c.page;
    if (!stage) return;
    stage.scrollTop = 0;
    stage.scrollLeft = c.flipped ? stage.scrollWidth : 0;
  });

  // Timing marks for the page benchmark (scripts/bench-pages.mjs): a page asked for, and on screen.
  $effect(() => {
    performance.mark("aion:page", { detail: c.page });
    if (performance.getEntriesByType("mark").length > 400) performance.clearMarks();
  });

  function loaded(i: number, e: Event) {
    const el = e.currentTarget as HTMLImageElement;
    el.decode().then(
      () =>
        requestAnimationFrame(() => {
          painted[i] = true;
          performance.mark("aion:shown", { detail: i });
        }),
      () => {},
    );
    c.dims[i] = { w: el.naturalWidth, h: el.naturalHeight };
    if (!announced) {
      announced = true;
      requestAnimationFrame(() => img && onready(img));
    }
  }

  // ---------- pointer: click zones, drag to pan, magnifier ----------
  let drag: { x: number; y: number; sx: number; sy: number; moved: boolean } | null = null;
  let lens = $state<{ x: number; y: number; ax: number; ay: number } | null>(null);
  const R = 130;

  function down(e: PointerEvent) {
    if (e.button !== 0 || !stage) return;
    drag = { x: e.clientX, y: e.clientY, sx: stage.scrollLeft, sy: stage.scrollTop, moved: false };
  }

  function move(e: PointerEvent) {
    if (c.magnifier && area) {
      const r = area.getBoundingClientRect();
      lens = { x: e.clientX, y: e.clientY, ax: r.left + r.width / 2, ay: r.top + r.height / 2 };
    }
    if (!drag || !(e.buttons & 1) || !stage) return;
    const dx = e.clientX - drag.x;
    const dy = e.clientY - drag.y;
    if (!drag.moved && Math.hypot(dx, dy) < 5) return;
    drag.moved = true;
    stage.scrollLeft = drag.sx - dx;
    stage.scrollTop = drag.sy - dy;
  }

  // A click waits a moment in case it's the first of a double-click, which zooms into the panel
  // under the pointer (Guided View from there on).
  let clickTimer: ReturnType<typeof setTimeout> | undefined;
  function click(e: MouseEvent) {
    const dragged = drag?.moved;
    drag = null;
    clearTimeout(clickTimer);
    if (dragged || e.detail > 1) return;
    const x = e.clientX / window.innerWidth;
    const z = x < 0.33 ? "left" : x > 0.67 ? "right" : "middle";
    clickTimer = setTimeout(() => onzone(z), DOUBLE_CLICK_MS);
  }

  function dblclick(e: MouseEvent) {
    clearTimeout(clickTimer);
    const el = (e.target as HTMLElement).closest("img[data-page]") as HTMLImageElement | null;
    if (!el || c.rotation || c.magnifier) return;
    const r = el.getBoundingClientRect();
    c.zoomInto(+el.dataset.page!, (e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height);
  }
  $effect(() => () => clearTimeout(clickTimer));

  // Wheel: zoom with Ctrl, magnifier strength over the lens, otherwise scroll and turn at the edges.
  let wheelAt = 0;
  let wheelSum = 0;
  function wheel(e: WheelEvent) {
    if (e.ctrlKey) {
      e.preventDefault();
      c.zoomBy(e.deltaY < 0 ? 1.1 : 0.9);
      return;
    }
    if (c.magnifier) {
      e.preventDefault();
      c.lensZoom = Math.max(1.5, Math.min(6, c.lensZoom * (e.deltaY < 0 ? 1.15 : 0.87)));
      return;
    }
    if (!stage) return;
    const atBottom = stage.scrollTop + stage.clientHeight >= stage.scrollHeight - 2;
    const atTop = stage.scrollTop <= 0;
    if ((e.deltaY > 0 && !atBottom) || (e.deltaY < 0 && !atTop)) return;
    const now = performance.now();
    if (now - wheelAt > 400) wheelSum = 0;
    wheelAt = now;
    wheelSum += e.deltaY;
    if (Math.abs(wheelSum) < 120) return;
    wheelSum = 0;
    if (e.deltaY > 0) c.next();
    else c.prev();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="stage"
  class:pan={overflows && !c.magnifier}
  class:lensing={c.magnifier}
  bind:this={stage}
  bind:clientWidth={vw}
  bind:clientHeight={vh}
  onpointerdown={down}
  onpointermove={move}
  onpointerleave={() => (lens = null)}
  onclick={click}
  ondblclick={dblclick}
  onwheel={wheel}
  role="presentation"
>
  <div
    class="area"
    bind:this={area}
    class:ready={!!box}
    style:width="{box?.w ?? vw}px"
    style:height="{box?.h ?? vh}px"
  >
    <div
      class="content"
      style:width="{box?.cw ?? 0}px"
      style:height="{box?.ch ?? 0}px"
      style:transform="translate(-50%, -50%) rotate({c.rotation}deg)"
    >
      {#each order as i, k (i)}
        {#if k === 0}
          <img bind:this={img} src={live(i) ? src(i) : undefined} style:width={width(k)} style:background-image={behind(i)} data-page={i} alt="Page {i + 1}" draggable="false" decoding="async" onload={(e) => loaded(i, e)} onerror={() => (failed = i)} />
        {:else}
          <img src={live(i) ? src(i) : undefined} style:width={width(k)} style:background-image={behind(i)} data-page={i} alt="Page {i + 1}" draggable="false" decoding="async" onload={(e) => loaded(i, e)} onerror={() => (failed = i)} />
        {/if}
      {/each}
    </div>
  </div>
</div>

<PageStatus
  waiting={!box}
  failed={failed !== null && order.includes(failed) ? failed : null}
  onretry={() => ((failed = null), retry++)}
/>

{#if c.magnifier && lens && box}
  <!-- A copy of the spread, scaled up around the cursor and clipped to a circle. -->
  <div class="lens" style:left="{lens.x - R}px" style:top="{lens.y - R}px" style:width="{R * 2}px" style:height="{R * 2}px">
    <div
      class="lens-content"
      style:left="{lens.ax - box.cw / 2 - (lens.x - R)}px"
      style:top="{lens.ay - box.ch / 2 - (lens.y - R)}px"
      style:width="{box.cw}px"
      style:height="{box.ch}px"
      style:transform-origin="{lens.x - (lens.ax - box.cw / 2)}px {lens.y - (lens.ay - box.ch / 2)}px"
      style:transform="scale({c.lensZoom})"
    >
      <div class="lens-turn" style:transform="rotate({c.rotation}deg)">
        {#each order as i (i)}<img src={pageUrl(c.id, i)} alt="" draggable="false" />{/each}
      </div>
    </div>
  </div>
{/if}

<style>
  .stage {
    position: absolute;
    inset: 0;
    overflow: auto;
    display: flex;
    scrollbar-width: none;
  }
  .stage::-webkit-scrollbar {
    display: none;
  }
  .stage.pan {
    cursor: grab;
  }
  .stage.pan:active {
    cursor: grabbing;
  }
  .stage.lensing {
    cursor: none;
  }
  .area {
    position: relative;
    flex: none;
    margin: auto;
    opacity: 0;
  }
  .area.ready {
    opacity: 1;
  }
  .content {
    position: absolute;
    left: 50%;
    top: 50%;
    display: flex;
  }
  .content img,
  .lens-turn img {
    height: 100%;
    width: auto;
    user-select: none;
  }
  .content img,
  .lens-turn img {
    filter: var(--page-filter, none);
  }
  .content img {
    background-size: 100% 100%;
  }
  .lens {
    position: fixed;
    z-index: 5;
    border-radius: 50%;
    overflow: hidden;
    pointer-events: none;
    background: #000;
    box-shadow:
      0 0 0 1px rgba(255, 255, 255, 0.28),
      0 18px 50px rgba(0, 0, 0, 0.7);
  }
  .lens-content {
    position: absolute;
  }
  .lens-turn {
    width: 100%;
    height: 100%;
    display: flex;
  }
</style>
