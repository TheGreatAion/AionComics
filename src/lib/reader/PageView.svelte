<script lang="ts">
  import { pageUrl } from "$lib/api";
  import type { Ctl } from "./ctl.svelte";
  import PageStatus from "./PageStatus.svelte";
  import { prefs } from "./prefs.svelte";

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
  const src = (i: number) => pageUrl(c.id, i) + (retry ? `?retry=${retry}` : "");

  $effect(() => {
    c.stage = stage ?? null;
    return () => (c.stage = null);
  });

  const shown = $derived(c.shown);
  // On screen left to right: a right-to-left spread puts the second page on the left.
  const order = $derived(c.flipped && shown.length === 2 ? [shown[1], shown[0]] : shown);
  const dims = $derived(order.map((i) => c.dims[i]));
  const ready = $derived(dims.every(Boolean));
  // Lay pages out at a common height of 1000 units, side by side.
  const unitW = $derived(dims.reduce((s, d) => s + (d ? (d.w * 1000) / d.h : 0), 0));
  const turned = $derived(c.rotation % 180 !== 0);

  const box = $derived.by(() => {
    if (!ready || !vw || !vh || !unitW) return null;
    const W = turned ? 1000 : unitW;
    const H = turned ? unitW : 1000;
    let s: number;
    switch (prefs.fit) {
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

  const overflows = $derived(!!box && (box.w > vw + 1 || box.h > vh + 1));

  // Look one spread ahead and behind so pairing and page turns are instant.
  $effect(() => {
    const p = c.page;
    for (const d of [-2, -1, 1, 2, 3]) c.measure(p + d);
  });

  // New page: back to the top, and to the right-hand edge when reading right to left.
  $effect(() => {
    void c.page;
    if (!stage) return;
    stage.scrollTop = 0;
    stage.scrollLeft = c.flipped ? stage.scrollWidth : 0;
  });

  function loaded(i: number, e: Event) {
    const el = e.currentTarget as HTMLImageElement;
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

  function click(e: MouseEvent) {
    const dragged = drag?.moved;
    drag = null;
    if (dragged) return;
    const x = e.clientX / window.innerWidth;
    onzone(x < 0.33 ? "left" : x > 0.67 ? "right" : "middle");
  }

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
          <img bind:this={img} src={src(i)} alt="Page {i + 1}" draggable="false" decoding="async" onload={(e) => loaded(i, e)} onerror={() => (failed = i)} />
        {:else}
          <img src={src(i)} alt="Page {i + 1}" draggable="false" decoding="async" onload={(e) => loaded(i, e)} onerror={() => (failed = i)} />
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
