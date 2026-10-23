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
  let sel = $state<number | null>(null);
  /** Reading-order mode: click panels in turn to number them. */
  let ordering = $state<number[] | null>(null);
  let frame = $state<HTMLDivElement>();
  let dirty = $state(false);
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

  // ---------- dragging ----------
  type Handle = "move" | "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw" | "new";
  let drag: { handle: Handle; i: number; x: number; y: number; orig: Panel } | null = null;
  const MIN = 0.02;
  const clamp = (v: number) => Math.max(0, Math.min(1, v));

  function point(e: PointerEvent) {
    const r = frame!.getBoundingClientRect();
    return { x: clamp((e.clientX - r.left) / r.width), y: clamp((e.clientY - r.top) / r.height) };
  }

  function start(e: PointerEvent, handle: Handle, i: number) {
    if (e.button !== 0 || ordering) return;
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const p = point(e);
    if (handle === "new") {
      panels = [...panels, { x: p.x, y: p.y, w: 0, h: 0 }];
      i = panels.length - 1;
    }
    sel = i;
    drag = { handle, i, x: p.x, y: p.y, orig: { ...panels[i] } };
  }

  function move(e: PointerEvent) {
    if (!drag) return;
    const p = point(e);
    const { handle, orig } = drag;
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
    // Moving keeps a slanted outline (shifted along); resizing makes it a plain box.
    const poly =
      handle === "move" && orig.poly
        ? orig.poly.map(([px, py]) => [px + x - orig.x, py + y - orig.y] as [number, number])
        : undefined;
    panels[drag.i] = poly ? { x, y, w, h, poly } : { x, y, w, h };
    dirty = true;
  }

  function end() {
    if (!drag) return;
    const p = panels[drag.i];
    // A click on empty art draws nothing.
    if (drag.handle === "new" && (p.w < MIN || p.h < MIN)) {
      panels = panels.slice(0, -1);
      sel = null;
    }
    drag = null;
  }

  function remove(i: number) {
    panels = panels.filter((_, k) => k !== i);
    sel = null;
    dirty = true;
  }

  function pick(i: number) {
    if (!ordering) {
      sel = i;
      return;
    }
    if (ordering.includes(i)) return;
    ordering = [...ordering, i];
    if (ordering.length === panels.length) {
      panels = ordering.map((k) => panels[k]);
      ordering = null;
      dirty = true;
    }
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
      sel = null;
      load();
      c.say("Back to the detected panels");
    } catch (e) {
      c.say(String(e));
    }
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Delete" || e.key === "Backspace") {
      if (sel !== null) remove(sel);
    } else if (e.key === "Enter") save();
    else if (e.key === "Escape" && ordering) ordering = null;
    else return;
    e.preventDefault();
    e.stopImmediatePropagation();
  }

  const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];
  const label = (i: number) => (ordering ? (ordering.indexOf(i) >= 0 ? ordering.indexOf(i) + 1 : "") : i + 1);
</script>

<svelte:window onkeydowncapture={onkey} />

<div class="editor" role="dialog" tabindex="-1" aria-label="Edit panels" onpointermove={move} onpointerup={end}>
  <div class="frame-wrap" bind:clientWidth={ww} bind:clientHeight={wh}>
    <div
      class="frame"
      class:ready={!!fit}
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
          {#if p.poly}<polygon class:sel={sel === i} points={p.poly.map((q) => q.join(",")).join(" ")} />{/if}
        {/each}
      </svg>
      {#each panels as p, i (i)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class="box"
          class:shaped={!!p.poly}
          class:sel={sel === i}
          class:numbered={ordering?.includes(i)}
          style:left="{p.x * 100}%"
          style:top="{p.y * 100}%"
          style:width="{p.w * 100}%"
          style:height="{p.h * 100}%"
          onpointerdown={(e) => start(e, "move", i)}
          onclick={(e) => (e.stopPropagation(), pick(i))}
          oncontextmenu={(e) => (e.preventDefault(), e.stopPropagation(), remove(i))}
          role="presentation"
        >
          <span class="num mono">{label(i)}</span>
          {#if sel === i && !ordering}
            {#each HANDLES as h (h)}
              <span class="handle {h}" onpointerdown={(e) => start(e, h, i)} role="presentation"></span>
            {/each}
          {/if}
        </div>
      {/each}
    </div>
  </div>

  <div class="bar">
    <span class="title">Panels <span class="mono">p. {page + 1}</span></span>
    <span
      class="hint"
      title={ordering ? "" : "Drag on the art to add a panel, drag a panel to move it, its edges to resize. Delete or right-click removes."}
    >
      {#if loading}…{:else if ordering}Click in reading order&ensp;<span class="mono">{ordering.length} / {panels.length}</span>{:else}Drag to draw · Del removes{/if}
    </span>
    <div class="actions">
      <button
        class="ico"
        class:on={!!ordering}
        title="Set reading order"
        aria-label="Set reading order"
        disabled={panels.length < 2}
        onclick={() => ((ordering = []), (sel = null))}
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
  .title .mono,
  .hint .mono {
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
