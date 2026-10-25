<script lang="ts">
  import { pageUrl, type Panel } from "$lib/api";
  import { untrack } from "svelte";
  import type { Ctl } from "./ctl.svelte";
  import { dimClip, outline } from "./outline";
  import PageStatus from "./PageStatus.svelte";
  import { DOUBLE_CLICK_MS, GUIDED_DIM, GUIDED_MS, prefs } from "./prefs.svelte";

  let {
    c,
    onzone,
    onready,
    img = $bindable(),
  }: {
    c: Ctl;
    onzone: (z: "left" | "middle" | "right") => void;
    onready: (img: HTMLImageElement) => void;
    img?: HTMLImageElement;
  } = $props();

  let vw = $state(0);
  let vh = $state(0);
  let camera = $state<HTMLDivElement>();
  let settledKey = $state(""); // ease the camera only after a spread's first placement
  let announced = false;
  let failed = $state<number | null>(null);
  let retry = $state(0);

  const ms = $derived(GUIDED_MS[prefs.guidedSpeed]);
  const dim = $derived(GUIDED_DIM[prefs.guidedDim]);

  // The page, or the spread: in double page mode the camera roams across both pages.
  const shown = $derived(c.shown);
  const key = $derived(shown.join("-"));
  // Left to right on screen; a right-to-left spread puts the second page on the left.
  const order = $derived(c.flipped && shown.length === 2 ? [shown[1], shown[0]] : shown);

  // Pages laid side by side at the height of the tallest, in its own pixels so zooming stays sharp.
  const layout = $derived.by(() => {
    const dims = order.map((i) => c.dims[i]);
    if (!dims.every(Boolean)) return null;
    const H = Math.max(...dims.map((d) => d!.h));
    let x = 0;
    const pages: Record<number, { x: number; w: number }> = {};
    for (const [k, i] of order.entries()) {
      const w = (dims[k]!.w * H) / dims[k]!.h;
      pages[i] = { x, w };
      x += w;
    }
    return { W: x, H, pages };
  });

  $effect(() => {
    for (const d of [0, 1, 2, 3, -1, -2]) c.measure(c.page + d);
  });

  // A stop's panel in spread coordinates (0–1 across the whole spread), or a whole page.
  function inSpread(page: number, p: Panel | null, L: NonNullable<typeof layout>): Panel {
    const pg = L.pages[page];
    if (!p) return { x: pg.x / L.W, y: 0, w: pg.w / L.W, h: 1 };
    const sx = (v: number) => (pg.x + v * pg.w) / L.W;
    return {
      x: sx(p.x),
      y: p.y,
      w: (p.w * pg.w) / L.W,
      h: p.h,
      poly: p.poly?.map(([qx, qy]) => [sx(qx), qy] as [number, number]),
    };
  }

  // Camera: scale + translate that frames the current stop (or the whole page or spread).
  const cam = $derived.by(() => {
    const L = layout;
    if (!L || !vw || !vh) return null;
    const { W: nw, H: nh } = L;
    const fitScale = Math.min(vw / nw, vh / nh) * 0.98;
    const s0 = c.steps && c.step >= 0 ? c.steps[c.step] : null;
    if (!s0 || !L.pages[s0.page]) {
      return { s: fitScale, tx: (vw - nw * fitScale) / 2, ty: (vh - nh * fitScale) / 2, clip: dimClip(outline(null, 0, 0)) };
    }
    const p = inSpread(s0.page, s0.panel, L);
    const pad = 0.008 * Math.max(nw, nh);
    const x = Math.max(0, p.x * nw - pad);
    const y = Math.max(0, p.y * nh - pad);
    const w = Math.min(nw, (p.x + p.w) * nw + pad) - x;
    const h = Math.min(nh, (p.y + p.h) * nh + pad) - y;
    const s = Math.min((vw * 0.94) / w, (vh * 0.9) / h, fitScale * 3.2);
    // Frame the box with some room; light just the stop's real outline (slanted panels included)
    // and its border line, so the gutter around it stays dim instead of framing it in white.
    const lit = 0.0025 * Math.max(nw, nh);
    const clip = dimClip(outline(p, lit / nw, lit / nh));
    return { s, tx: vw / 2 - (x + w / 2) * s, ty: vh / 2 - (y + h / 2) * s, clip };
  });

  // What's on screen. While the camera glides it scales the whole page with a transform, which is
  // smooth but drawn soft (a panel came out about a quarter blurrier than the same pixels laid out
  // normally). Once it stops, the page is laid out at its final size instead and drawn sharp. The two
  // look the same, so the swap doesn't show.
  type Cam = NonNullable<typeof cam>;
  let view = $state<Cam | null>(null);
  let crisp = $state(false);
  let moving = $state(false);
  let token = 0;
  let settleTimer: ReturnType<typeof setTimeout> | undefined;

  function settleLater(t: number) {
    clearTimeout(settleTimer);
    settleTimer = setTimeout(() => {
      if (t !== token) return;
      moving = false;
      crisp = true;
    }, ms + 40);
  }

  $effect(() => {
    const next = cam;
    const k = key;
    if (!next) return;
    const t = ++token;
    untrack(() => {
      // A new page or spread, or the first frame: place it, no glide.
      if (!view || settledKey !== k) {
        clearTimeout(settleTimer);
        view = next;
        moving = false;
        crisp = true;
        return;
      }
      // Already gliding: retarget mid-flight.
      if (moving) {
        view = next;
        settleLater(t);
        return;
      }
      // Swap the sharp layout for the same picture as a transform, then glide to the new framing.
      crisp = false;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (t !== token) return;
          moving = true;
          view = next;
          settleLater(t);
        }),
      );
    });
  });
  $effect(() => () => clearTimeout(settleTimer));

  /** Scale baked into the layout: the camera's when sharp, none while it glides. */
  const k = $derived(crisp && view ? view.s : 1);

  // Once a spread is placed: turn easing on, and fly the cover in the first time.
  $effect(() => {
    if (!cam) return;
    const k = key;
    const first = camera?.querySelector("img") ?? undefined;
    img = first;
    const f = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        settledKey = k;
        if (!announced && first) {
          announced = true;
          onready(first);
        }
      }),
    );
    return () => cancelAnimationFrame(f);
  });

  function loaded(i: number, e: Event) {
    const el = e.currentTarget as HTMLImageElement;
    if (!c.dims[i]) c.dims[i] = { w: el.naturalWidth, h: el.naturalHeight };
  }

  // ---------- pointer: tap zones, double-click a panel, drag and wheel to look around ----------
  // Detection isn't always right, so the camera can be moved by hand: drag to pan, wheel to zoom.
  // The next step glides back to the panels.
  let stageEl = $state<HTMLDivElement>();
  let clickTimer: ReturnType<typeof setTimeout> | undefined;
  let drag: { x: number; y: number; tx: number; ty: number; moved: boolean } | null = null;
  let panning = $state(false);

  /** The page under the pointer, and where on it (0–1 across and down). */
  function pointAt(e: MouseEvent) {
    const L = layout;
    if (!L || !camera) return null;
    const r = camera.getBoundingClientRect();
    const sx = ((e.clientX - r.left) / r.width) * L.W;
    const y = (e.clientY - r.top) / r.height;
    for (const [p, g] of Object.entries(L.pages)) {
      if (sx >= g.x && sx <= g.x + g.w) return { page: +p, x: (sx - g.x) / g.w, y };
    }
    return null;
  }

  function down(e: PointerEvent) {
    if (e.button === 0 && view) drag = { x: e.clientX, y: e.clientY, tx: view.tx, ty: view.ty, moved: false };
  }

  function move(e: PointerEvent) {
    if (!drag || !(e.buttons & 1) || !view) return;
    const [dx, dy] = [e.clientX - drag.x, e.clientY - drag.y];
    if (!drag.moved && Math.hypot(dx, dy) < 5) return;
    drag.moved = true;
    panning = true;
    moving = false;
    view = { ...view, tx: drag.tx + dx, ty: drag.ty + dy };
  }

  function wheel(e: WheelEvent) {
    const L = layout;
    if (!view || !L || !stageEl) return;
    e.preventDefault();
    const fit = Math.min(vw / L.W, vh / L.H) * 0.98;
    const s = Math.max(fit * 0.6, Math.min(fit * 8, view.s * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
    const f = s / view.s;
    const r = stageEl.getBoundingClientRect();
    const [mx, my] = [e.clientX - r.left, e.clientY - r.top];
    moving = false;
    view = { ...view, s, tx: mx - (mx - view.tx) * f, ty: my - (my - view.ty) * f };
  }

  // A click waits a moment in case it's the first of a double-click.
  function click(e: MouseEvent) {
    const dragged = drag?.moved;
    drag = null;
    panning = false;
    clearTimeout(clickTimer);
    if (dragged || e.detail > 1) return;
    const x = e.clientX / window.innerWidth;
    const z = x < 0.33 ? "left" : x > 0.67 ? "right" : "middle";
    clickTimer = setTimeout(() => onzone(z), DOUBLE_CLICK_MS);
  }

  function dblclick(e: MouseEvent) {
    clearTimeout(clickTimer);
    const p = pointAt(e);
    if (p) c.zoomInto(p.page, p.x, p.y);
  }
  $effect(() => () => clearTimeout(clickTimer));
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="stage"
  class:panning
  bind:this={stageEl}
  bind:clientWidth={vw}
  bind:clientHeight={vh}
  onclick={click}
  ondblclick={dblclick}
  onpointerdown={down}
  onpointermove={move}
  onwheel={wheel}
  role="presentation"
>
  {#key key}
    <div
      class="camera"
      bind:this={camera}
      class:ready={!!cam}
      class:animate={moving}
      style:--ms="{ms}ms"
      style:width="{(layout?.W ?? 0) * k}px"
      style:height="{(layout?.H ?? 0) * k}px"
      style:transform={view ? `translate(${view.tx}px, ${view.ty}px)${crisp ? "" : ` scale(${view.s})`}` : "none"}
    >
      {#each order as i (i)}
        <img
          src={pageUrl(c.id, i) + (retry ? `?retry=${retry}` : "")}
          decoding="async"
          alt="Page {i + 1}"
          draggable="false"
          style:left="{(layout?.pages[i]?.x ?? 0) * k}px"
          style:width="{(layout?.pages[i]?.w ?? 0) * k}px"
          onload={(e) => loaded(i, e)}
          onerror={() => (failed = i)}
        />
      {/each}
      {#if view}
        <div class="dim" style:--dim={dim} style:clip-path={view.clip}></div>
      {/if}
    </div>
  {/key}
  <PageStatus
    waiting={!cam}
    failed={failed !== null && order.includes(failed) ? failed : null}
    onretry={() => ((failed = null), retry++)}
  />
</div>

<style>
  .stage {
    position: absolute;
    inset: 0;
    overflow: hidden;
  }
  .stage.panning {
    cursor: grabbing;
  }
  .camera {
    position: absolute;
    left: 0;
    top: 0;
    transform-origin: 0 0;
    opacity: 0;
  }
  .camera.ready {
    opacity: 1;
  }
  .camera.animate {
    transition: transform var(--ms) cubic-bezier(0.65, 0, 0.35, 1);
  }
  .camera img {
    position: absolute;
    top: 0;
    height: 100%;
    user-select: none;
  }
  .dim {
    position: absolute;
    inset: 0;
    /* Everything but the stop, in the reader's background color. */
    background: color-mix(in srgb, var(--reader-bg) calc(var(--dim) * 100%), transparent);
    pointer-events: none;
  }
  .camera.animate .dim {
    transition: clip-path var(--ms) cubic-bezier(0.65, 0, 0.35, 1);
  }
</style>
