<script lang="ts">
  import { pageUrl } from "$lib/api";
  import { untrack } from "svelte";
  import type { Ctl } from "./ctl.svelte";
  import { prefs } from "./prefs.svelte";

  let { c, onzone }: { c: Ctl; onzone: (z: "left" | "middle" | "right") => void } = $props();

  let stage = $state<HTMLDivElement>();
  let list = $state<HTMLDivElement>();
  let vw = $state(0);
  let vh = $state(0);
  let ticking = false;

  $effect(() => {
    c.stage = stage ?? null;
    return () => (c.stage = null);
  });

  // One column: full width for "fit width" (webtoons), otherwise about a page's width at screen height.
  const column = $derived(
    Math.round(c.view.fit === "width" ? vw * Math.min(c.zoom, 1) : Math.min(vw, vh * 0.7 * c.zoom)),
  );
  const pages = $derived(Array.from({ length: c.count }, (_, i) => i));
  const shape = (i: number) => {
    const d = c.dims[i];
    return d ? d.h / d.w : 1.45;
  };

  // Navigation commands (Go to, Home/End, the slider) move the view; scrolling just reports the page.
  // Also re-anchors when the column first gets its width or changes with zoom.
  $effect(() => {
    void c.seek;
    if (!column) return;
    const p = untrack(() => c.page);
    if (!list) return;
    // Wait a frame so the column's new width has been laid out.
    const frame = requestAnimationFrame(() => {
      const el = list?.children[p] as HTMLElement | undefined;
      if (el && stage) stage.scrollTop = el.offsetTop;
    });
    return () => cancelAnimationFrame(frame);
  });

  function onscroll() {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      ticking = false;
      if (!stage || !list) return;
      const mid = stage.scrollTop + stage.clientHeight / 2;
      const kids = list.children;
      let lo = 0;
      let hi = kids.length - 1;
      while (lo < hi) {
        const m = (lo + hi + 1) >> 1;
        if ((kids[m] as HTMLElement).offsetTop <= mid) lo = m;
        else hi = m - 1;
      }
      if (c.page !== lo) c.page = lo;
    });
  }

  function click(e: MouseEvent) {
    const x = e.clientX / window.innerWidth;
    onzone(x < 0.33 ? "left" : x > 0.67 ? "right" : "middle");
  }

  function loaded(i: number, e: Event) {
    const el = e.currentTarget as HTMLImageElement;
    c.dims[i] = { w: el.naturalWidth, h: el.naturalHeight };
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="stage"
  bind:this={stage}
  bind:clientWidth={vw}
  bind:clientHeight={vh}
  {onscroll}
  onclick={click}
  role="presentation"
>
  <div class="list" bind:this={list} style:width="{column}px">
    {#each pages as i (i)}
      <div class="page" style:aspect-ratio="1 / {shape(i)}">
        <img src={pageUrl(c.id, i)} alt="Page {i + 1}" loading="lazy" draggable="false" onload={(e) => loaded(i, e)} />
      </div>
    {/each}
  </div>
</div>

<style>
  .stage {
    position: absolute;
    inset: 0;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: none;
  }
  .stage::-webkit-scrollbar {
    display: none;
  }
  .list {
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .page {
    width: 100%;
    background: #0c0c0c;
  }
  .page img {
    width: 100%;
    height: 100%;
    user-select: none;
    filter: var(--page-filter, none);
  }
</style>
