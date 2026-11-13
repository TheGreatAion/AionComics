<script lang="ts">
  import { api, pageUrl, type Panel } from "$lib/api";
  import { untrack } from "svelte";
  import Icon from "$lib/components/Icon.svelte";
  import type { Ctl } from "./ctl.svelte";

  let { c, onsaved }: { c: Ctl; onsaved: () => void } = $props();

  // The page being edited stays fixed even if the reader moves underneath.
  const page = untrack(() => c.page);
  let panels = $state<Panel[]>([]);
  let loading = $state(true);
  /** The picked panels. One has handles; two or more can be merged. */
  let picked = $state<number[]>([]);
  const sel = $derived(picked.length === 1 ? picked[0] : null);
  /** Reading-order mode: click panels in turn to number them. */
  let ordering = $state<number[] | null>(null);
  /** Cut mode: a line drawn across a panel cuts it in two. */
  let cutting = $state(false);
  let cut = $state<{ a: Pt; b: Pt } | null>(null);
  let frame = $state<HTMLDivElement>();
  let dirty = $state(false);
  /** Earlier states of the page's panels, for Ctrl+Z. */
  let past: Panel[][] = [];
  // Fit the page into the space above the bar.
  let ww = $state(0);
  let wh = $state(0);
  let natural = $state({ w: 0, h: 0 });
  const fit = $derived.by(() => {
    if (!natural.w || !ww || !wh) return null;
    const s = Math.min((ww - 56) / natural.w, (wh - 56) / natural.h);
    return { w: natural.w * s, h: natural.h * s };
  });

  function load() {
    loading = true;
    api
      .pagePanels(c.id, page, c.rtl)
      .then((p) => (panels = p))
      .catch(() => (panels = []))
      .finally(() => (loading = false));
  }
  load();

  /** Call before changing the panels, so the change can be undone. */
  function remember() {
    past.push($state.snapshot(panels) as Panel[]);
    if (past.length > 100) past.shift();
    dirty = true;
  }

  function undo() {
    const before = past.pop();
    if (!before) return;
    panels = before;
    picked = [];
    ordering = null;
  }

  // ---------- shapes ----------
  type Pt = [number, number];
  const MIN = 0.02;
  const clamp = (v: number) => Math.max(0, Math.min(1, v));

  /** A panel's outline: its slanted shape if it has one, else its box. Clockwise on screen. */
  const ring = (p: Panel): Pt[] =>
    p.poly && p.poly.length >= 3 ? p.poly : [[p.x, p.y], [p.x + p.w, p.y], [p.x + p.w, p.y + p.h], [p.x, p.y + p.h]];

  /** Twice the area inside a ring; above zero when it runs clockwise on screen. */
  const area2 = (pts: Pt[]) => pts.reduce((s, [x, y], i) => s + x * pts[(i + 1) % pts.length][1] - pts[(i + 1) % pts.length][0] * y, 0);

  /** The panel for an outline: a plain box when the outline fills its box, else box and outline. */
  function shaped(pts: Pt[]): Panel {
    // Guided View lights outlines of up to 8 corners: drop doubled points, then the flattest corners.
    pts = pts.filter((q, i) => Math.hypot(q[0] - pts[(i + 1) % pts.length][0], q[1] - pts[(i + 1) % pts.length][1]) > 1e-4);
    const flat = (i: number) => Math.abs(area2([pts[(i + pts.length - 1) % pts.length], pts[i], pts[(i + 1) % pts.length]]));
    while (pts.length > 3) {
      const i = pts.reduce((best, _, k) => (flat(k) < flat(best) ? k : best), 0);
      if (pts.length <= 8 && flat(i) > 1e-5) break;
      pts = pts.filter((_, k) => k !== i);
    }
    const xs = pts.map((q) => q[0]);
    const ys = pts.map((q) => q[1]);
    const [x, y] = [Math.min(...xs), Math.min(...ys)];
    const [w, h] = [Math.max(...xs) - x, Math.max(...ys) - y];
    return Math.abs(area2(pts)) / 2 >= 0.985 * w * h ? { x, y, w, h } : { x, y, w, h, poly: pts };
  }

  /** The part of a ring on one side of the line through a and b. */
  function half(pts: Pt[], a: Pt, b: Pt, keep: 1 | -1): Pt[] {
    const side = (q: Pt) => keep * ((b[0] - a[0]) * (q[1] - a[1]) - (b[1] - a[1]) * (q[0] - a[0]));
    const out: Pt[] = [];
    pts.forEach((q, i) => {
      const n = pts[(i + 1) % pts.length];
      const [sq, sn] = [side(q), side(n)];
      if (sq >= 0) out.push(q);
      if (sq >= 0 !== sn >= 0) {
        const t = sq / (sq - sn);
        out.push([q[0] + (n[0] - q[0]) * t, q[1] + (n[1] - q[1]) * t]);
      }
    });
    return out;
  }

  /** The smallest outline with no dents that holds all the points, clockwise on screen. */
  function hull(pts: Pt[]): Pt[] {
    const sorted = [...pts].sort((p, q) => p[0] - q[0] || p[1] - q[1]);
    const turn = (o: Pt, p: Pt, q: Pt) => (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
    const chain = (list: Pt[]) => {
      const out: Pt[] = [];
      for (const q of list) {
        while (out.length >= 2 && turn(out[out.length - 2], out[out.length - 1], q) <= 0) out.pop();
        out.push(q);
      }
      return out.slice(0, -1);
    };
    return [...chain(sorted), ...chain(sorted.reverse())];
  }

  /** Cuts the panel under the middle of the drawn line in two along it. */
  function cutAlong(a: Pt, b: Pt) {
    if (!fit) return;
    // A line that is nearly upright or level is meant to be exactly so.
    const [dx, dy] = [Math.abs(b[0] - a[0]) * fit.w, Math.abs(b[1] - a[1]) * fit.h];
    if (Math.hypot(dx, dy) < 12) return;
    if (dx < dy * 0.05) (a = [(a[0] + b[0]) / 2, a[1]]), (b = [a[0], b[1]]);
    else if (dy < dx * 0.05) (a = [a[0], (a[1] + b[1]) / 2]), (b = [b[0], a[1]]);
    const mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    const under = panels
      .map((p, i) => ({ p, i }))
      .filter(({ p }) => mid[0] >= p.x && mid[0] <= p.x + p.w && mid[1] >= p.y && mid[1] <= p.y + p.h)
      .sort((m, n) => m.p.w * m.p.h - n.p.w * n.p.h)[0];
    if (!under) return;
    const pieces = ([1, -1] as const)
      .map((keep) => half(ring(under.p), a, b, keep))
      .filter((r) => r.length >= 3)
      .map(shaped);
    if (pieces.length < 2 || pieces.some((p) => p.w < MIN || p.h < MIN)) {
      c.say("Draw the line right across the panel");
      return;
    }
    // The two halves in reading order: side by side follows the book's direction, else top first.
    const at = (p: Panel) => (dx < dy ? (c.rtl ? -1 : 1) * (p.x + p.w / 2) : p.y + p.h / 2);
    pieces.sort((p, q) => at(p) - at(q));
    remember();
    panels = [...panels.slice(0, under.i), ...pieces, ...panels.slice(under.i + 1)];
    picked = [];
  }

  /** Joins the picked panels into one, in the place of the earliest of them. */
  function merge() {
    if (picked.length < 2) return;
    const ids = [...picked].sort((a, b) => a - b);
    // Plain boxes join into the box around them; with a slanted one, into the outline around them.
    const all = ids.flatMap((i) => ring(panels[i]));
    const [x, y] = [Math.min(...all.map((q) => q[0])), Math.min(...all.map((q) => q[1]))];
    const around = { x, y, w: Math.max(...all.map((q) => q[0])) - x, h: Math.max(...all.map((q) => q[1])) - y };
    const joined = ids.some((i) => panels[i].poly) ? shaped(hull(all)) : around;
    remember();
    panels = panels.flatMap((p, i) => (i === ids[0] ? [joined] : ids.includes(i) ? [] : [p]));
    picked = [ids[0]];
  }

  /** Moves the picked panel to another place in the reading order; the rest close up around it. */
  function place(to: number) {
    if (sel === null) return;
    const from = sel;
    to = Math.min(panels.length - 1, to);
    if (to === from) return;
    remember();
    const rest = panels.filter((_, k) => k !== from);
    panels = [...rest.slice(0, to), panels[from], ...rest.slice(to)];
    picked = [to];
  }

  // ---------- dragging ----------
  type Handle = "move" | "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw" | "new" | "cut";
  let drag: { handle: Handle; i: number; x: number; y: number; orig: Panel; moved: boolean } | null = null;

  function point(e: PointerEvent) {
    const r = frame!.getBoundingClientRect();
    return { x: clamp((e.clientX - r.left) / r.width), y: clamp((e.clientY - r.top) / r.height) };
  }

  function start(e: PointerEvent, handle: Handle, i: number) {
    if (e.button !== 0 || ordering) return;
    e.stopPropagation();
    // Shift-click picks several panels, to merge them.
    if (handle === "move" && e.shiftKey && !cutting) {
      picked = picked.includes(i) ? picked.filter((k) => k !== i) : [...picked, i];
      return;
    }
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const p = point(e);
    if (cutting) {
      cut = { a: [p.x, p.y], b: [p.x, p.y] };
      drag = { handle: "cut", i: -1, x: p.x, y: p.y, orig: { x: 0, y: 0, w: 0, h: 0 }, moved: false };
      return;
    }
    if (handle === "new") {
      remember();
      panels = [...panels, { x: p.x, y: p.y, w: 0, h: 0 }];
      i = panels.length - 1;
    }
    picked = [i];
    drag = { handle, i, x: p.x, y: p.y, orig: $state.snapshot(panels[i]) as Panel, moved: handle === "new" };
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    const p = point(e);
    const { handle, orig } = drag;
    if (handle === "cut") {
      cut = { a: [drag.x, drag.y], b: [p.x, p.y] };
      return;
    }
    const dx = p.x - drag.x;
    const dy = p.y - drag.y;
    let { x, y, w, h } = orig;
    if (handle === "move") {
      x = Math.max(0, Math.min(1 - w, x + dx));
      y = Math.max(0, Math.min(1 - h, y + dy));
    } else if (handle === "new") {
      x = Math.min(drag.x, p.x);
      y = Math.min(drag.y, p.y);
      w = Math.abs(p.x - drag.x);
      h = Math.abs(p.y - drag.y);
    } else {
      if (handle.includes("w")) {
        const nx = Math.min(x + w - MIN, x + dx);
        w += x - nx;
        x = nx;
      }
      if (handle.includes("e")) w = Math.max(MIN, Math.min(1 - x, w + dx));
      if (handle.includes("n")) {
        const ny = Math.min(y + h - MIN, y + dy);
        h += y - ny;
        y = ny;
      }
      if (handle.includes("s")) h = Math.max(MIN, Math.min(1 - y, h + dy));
    }
    if (!drag.moved) {
      if (x === orig.x && y === orig.y && w === orig.w && h === orig.h) return;
      remember();
      drag.moved = true;
    }
    // A slanted outline moves and stretches with its box.
    const poly = orig.poly?.map(([px, py]) => [x + ((px - orig.x) * w) / orig.w, y + ((py - orig.y) * h) / orig.h] as Pt);
    panels[drag.i] = poly ? { x, y, w, h, poly } : { x, y, w, h };
  }

  function end() {
    if (!drag) return;
    if (drag.handle === "cut") {
      if (cut) cutAlong(cut.a, cut.b);
      cut = null;
      drag = null;
      return;
    }
    const p = panels[drag.i];
    // A click on empty art draws nothing.
    if (drag.handle === "new" && (p.w < MIN || p.h < MIN)) {
      panels = panels.slice(0, -1);
      past.pop();
      picked = [];
    }
    drag = null;
  }

  function remove(ids: number[]) {
    if (!ids.length) return;
    remember();
    panels = panels.filter((_, k) => !ids.includes(k));
    picked = [];
  }

  /** Reading-order mode: the next panel in the order. */
  function pick(i: number) {
    if (!ordering || ordering.includes(i)) return;
    ordering = [...ordering, i];
    if (ordering.length === panels.length) reorder();
  }

  /** Leaves reading-order mode: the clicked panels come first, the rest keep their order after them. */
  function reorder() {
    const first = ordering ?? [];
    ordering = null;
    if (!first.length) return;
    remember();
    panels = [...first.map((k) => panels[k]), ...panels.filter((_, k) => !first.includes(k))];
  }

  async function save() {
    try {
      await api.setUserPanels(c.id, page, panels);
      onsaved();
      c.overlay = null;
      c.say(`Saved ${panels.length} ${panels.length === 1 ? "panel" : "panels"} for page ${page + 1}`);
    } catch (e) {
      c.say(String(e));
    }
  }

  async function reset() {
    try {
      await api.setUserPanels(c.id, page, null);
      onsaved();
      dirty = false;
      picked = [];
      past = [];
      load();
      c.say("Back to the detected panels");
    } catch (e) {
      c.say(String(e));
    }
  }

  function onkey(e: KeyboardEvent) {
    const key = e.key.toLowerCase();
    if ((e.ctrlKey || e.metaKey) && key === "z") undo();
    else if (e.ctrlKey || e.metaKey || e.altKey) return;
    else if (e.key === "Delete" || e.key === "Backspace") remove(picked);
    else if (e.key === "Enter") ordering ? reorder() : save();
    else if (e.key === "Escape" && (ordering || cutting)) (ordering = null), (cutting = false), (cut = null);
    else if (ordering) return;
    else if (key === "s") (cutting = !cutting), (picked = []);
    else if (key === "m") merge();
    // A number moves the picked panel to that place in the reading order.
    else if (/^[1-9]$/.test(e.key) && sel !== null) place(Number(e.key) - 1);
    else return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }

  const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];
  const label = (i: number) => (ordering ? (ordering.indexOf(i) >= 0 ? ordering.indexOf(i) + 1 : "") : i + 1);
  const HELP =
    "Drag on the art to add a panel, drag a panel to move it, its edges to resize. Delete or right-click removes. " +
    "S, then a line drawn across a panel, cuts it in two. Shift-click picks several; M merges them. " +
    "Pick a panel and press a number to give it that place in the reading order. Ctrl+Z undoes.";
</script>

<svelte:window onkeydowncapture={onkey} />

<div class="editor" role="dialog" tabindex="-1" aria-label="Edit panels" onpointermove={move} onpointerup={end}>
  <div class="frame-wrap" bind:clientWidth={ww} bind:clientHeight={wh}>
    <div
      class="frame"
      class:ready={!!fit}
      class:cutting
      bind:this={frame}
      style:width="{fit?.w ?? 0}px"
      style:height="{fit?.h ?? 0}px"
      onpointerdown={(e) => start(e, "new", -1)}
      role="presentation"
    >
      <img
        src={pageUrl(c.id, page)}
        alt="Page {page + 1}"
        draggable="false"
        onload={(e) => {
          const el = e.currentTarget as HTMLImageElement;
          natural = { w: el.naturalWidth, h: el.naturalHeight };
        }}
      />
      <!-- Real outlines of slanted panels; the box around each is what the camera frames. -->
      <svg class="shapes" viewBox="0 0 1 1" preserveAspectRatio="none" aria-hidden="true">
        {#each panels as p, i (i)}
          {#if p.poly}<polygon class:sel={picked.includes(i)} points={p.poly.map((q) => q.join(",")).join(" ")} />{/if}
        {/each}
        {#if cut}<line class="cut" x1={cut.a[0]} y1={cut.a[1]} x2={cut.b[0]} y2={cut.b[1]} />{/if}
      </svg>
      {#each panels as p, i (i)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="box"
          class:shaped={!!p.poly}
          class:sel={picked.includes(i)}
          class:numbered={ordering?.includes(i)}
          style:left="{p.x * 100}%"
          style:top="{p.y * 100}%"
          style:width="{p.w * 100}%"
          style:height="{p.h * 100}%"
          onpointerdown={(e) => start(e, "move", i)}
          onclick={(e) => (e.stopPropagation(), pick(i))}
          oncontextmenu={(e) => (e.preventDefault(), e.stopPropagation(), remove([i]))}
          role="presentation"
        >
          <span class="num fine">{label(i)}</span>
          {#if sel === i && !ordering && !cutting}
            {#each HANDLES as h (h)}
              <span class="handle {h}" onpointerdown={(e) => start(e, h, i)} role="presentation"></span>
            {/each}
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <div class="bar">
    <span class="title">Panels <span class="fine">p. {page + 1}</span></span>
    <span class="hint" title={ordering || cutting ? "" : HELP}>
      {#if loading}
        …
      {:else if ordering}
        Click in reading order&ensp;<span class="fine">{ordering.length} / {panels.length}</span>{#if ordering.length}&ensp;· Enter when the rest is right{/if}
      {:else if cutting}
        Draw a line across a panel to cut it
      {:else if picked.length > 1}
        {picked.length} picked · M merges them
      {:else if sel !== null && panels.length > 1}
        Press a number to change its place · Del removes
      {:else}
        Drag to draw · Shift-click to pick several
      {/if}
    </span>
    <div class="actions">
      <button
        class="ico"
        class:on={cutting}
        title="Cut a panel in two (S)"
        aria-label="Cut a panel in two"
        disabled={!panels.length || !!ordering}
        onclick={() => ((cutting = !cutting), (picked = []))}
      >
        <Icon name="cutPanel" size={18} />
      </button>
      <button
        class="ico"
        title="Merge the picked panels (M)"
        aria-label="Merge the picked panels"
        disabled={picked.length < 2 || !!ordering}
        onclick={merge}
      >
        <Icon name="mergePanels" size={18} />
      </button>
      <button
        class="ico"
        class:on={!!ordering}
        title="Set reading order"
        aria-label="Set reading order"
        disabled={panels.length < 2}
        onclick={() => (ordering ? reorder() : ((ordering = []), (picked = []), (cutting = false)))}
      >
        <Icon name="order" size={18} />
      </button>
      <button class="ico" title="Reset to detected" aria-label="Reset to detected" onclick={reset}><Icon name="undo" size={17} /></button>
      <button class="ico" title="Cancel (Esc)" aria-label="Cancel" onclick={() => (c.overlay = null)}><Icon name="close" size={16} /></button>
      <button class="btn" onclick={save} disabled={loading}>{dirty ? "Save" : "Done"}</button>
    </div>
  </div>
</div>

<style>
  .editor {
    position: absolute;
    inset: 0;
    z-index: 9;
    background: var(--reader-bg);
    display: flex;
    flex-direction: column;
  }
  .frame-wrap {
    flex: 1;
    min-height: 0;
    display: grid;
    place-items: center;
  }
  .frame {
    position: relative;
    cursor: crosshair;
    touch-action: none;
    opacity: 0;
  }
  .frame.ready {
    opacity: 1;
  }
  .frame img {
    display: block;
    width: 100%;
    height: 100%;
    opacity: 0.85;
    user-select: none;
    pointer-events: none;
  }
  .box {
    position: absolute;
    border: 1px solid rgba(255, 255, 255, 0.9);
    background: rgba(255, 255, 255, 0.06);
    box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
    cursor: move;
  }
  .box.sel {
    border-width: 2px;
    background: rgba(255, 255, 255, 0.1);
  }
  /* A slanted panel: the outline carries the shape, the box is only its frame. */
  .box.shaped {
    border-style: dashed;
    border-color: rgba(255, 255, 255, 0.45);
    background: none;
  }
  .shapes {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .shapes polygon {
    fill: rgba(255, 255, 255, 0.07);
    stroke: rgba(255, 255, 255, 0.95);
    stroke-width: 1.5px;
    vector-effect: non-scaling-stroke;
  }
  .shapes polygon.sel {
    stroke-width: 2.5px;
  }
  .shapes .cut {
    stroke: var(--text);
    stroke-width: 2px;
    stroke-dasharray: 6 4;
    vector-effect: non-scaling-stroke;
  }
  .frame.cutting .box {
    cursor: crosshair;
  }
  .box.numbered {
    background: rgba(255, 255, 255, 0.18);
  }
  .num {
    position: absolute;
    left: 0;
    top: 0;
    min-width: 22px;
    padding: 3px 6px;
    background: var(--text);
    color: #000;
    font-size: 12px;
    text-align: center;
  }
  .handle {
    position: absolute;
    width: 10px;
    height: 10px;
    background: var(--text);
    box-shadow: 0 0 0 1px #000;
  }
  .nw { left: -6px; top: -6px; cursor: nwse-resize; }
  .n { left: calc(50% - 5px); top: -6px; cursor: ns-resize; }
  .ne { right: -6px; top: -6px; cursor: nesw-resize; }
  .e { right: -6px; top: calc(50% - 5px); cursor: ew-resize; }
  .se { right: -6px; bottom: -6px; cursor: nwse-resize; }
  .s { left: calc(50% - 5px); bottom: -6px; cursor: ns-resize; }
  .sw { left: -6px; bottom: -6px; cursor: nesw-resize; }
  .w { left: -6px; top: calc(50% - 5px); cursor: ew-resize; }

  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 16px 28px 20px;
    border-top: 1px solid var(--line);
  }
  .title {
    flex: none;
  }
  .hint {
    color: var(--text-2);
    font-size: 13px;
    min-width: 0;
  }
  .title .fine,
  .hint .fine {
    color: var(--text-3);
    margin-left: 4px;
  }
  .actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }
  .actions .btn {
    margin-left: 14px;
  }
  .link:disabled {
    opacity: 0.35;
  }
  .btn {
    padding: 9px 20px;
  }
</style>
