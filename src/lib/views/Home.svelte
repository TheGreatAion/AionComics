<script lang="ts">
  import { tick, untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { ago, api, pageUrl, progressOf, storyTitle, unitWord, type ComicCard, type Home } from "$lib/api";
  import { lib, nav, openReader, reader, search, sheets } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let data = $state<Home | null>(null);
  let page = $state<HTMLImageElement>();

  $effect(() => {
    void lib.version;
    api.home().then((d) => {
      data = d;
      nav.restore();
    });
  });

  // Everything you're in the middle of, as a pile of covers resting on the page you're on.
  // The book on top is the one the hero describes; flipping changes only what's on top.
  const pile = $derived<ComicCard[]>(
    data ? (data.continueReading.length ? data.continueReading : data.pickUpAgain.slice(0, 12)) : [],
  );
  let topId = $state<number | null>(null);
  const at = $derived(Math.max(0, pile.findIndex((c) => c.id === topId)));
  const hero = $derived(pile[at]);
  const sub = $derived(hero ? storyTitle(hero) : null);
  const empty = $derived(data && !hero);

  const resume = () => hero && openReader(hero.id, page ?? null);

  // How deep a book sits in the pile (0 = on top).
  const depth = (i: number, top = at) => (i - top + pile.length) % pile.length;

  /* ---- Layout: the pile hangs off the page's lower-left; together they're centred in the column. */
  const VISIBLE = 5; // covers you can see in the pile; the rest wait hidden at the bottom
  const STEP = 0.15; // how far each cover underneath peeks out, as a share of the cover width
  const OVER = 0.32; // how much of the top cover lies over the page
  const CAPTION = 34; // room under the pile for the counter
  const PILE_RATIO = 1.54; // every cover in the pile is cut to one comic size, like a real stack

  let colW = $state(0);
  let colH = $state(0);
  let shown = $state<{ src: string; ratio: number } | null>(null);

  const fit = $derived.by(() => {
    if (!colW || !colH || !shown) return null;
    const n = Math.min(pile.length, VISIBLE);
    const out = (cw: number) => cw * (1 - OVER) + (n - 1) * cw * STEP;
    let pageH = colH;
    let pageW = pageH / shown.ratio;
    // The cover shrinks first; only once it's as small as it goes does the page give way.
    let cw = Math.min(260, pageH * 0.38);
    if (out(cw) + pageW > colW) cw = Math.max(96, (colW - pageW) / (1 - OVER + (n - 1) * STEP));
    if (out(cw) + pageW > colW) {
      pageW = colW - out(cw);
      pageH = pageW * shown.ratio;
    }
    const left = (colW - out(cw) - pageW) / 2;
    const pageX = left + out(cw);
    const pageY = (colH - pageH) / 2;
    const bottom = pageY + pageH - CAPTION;
    return { cw, out: out(cw), left, pageX, pageY, pageW, pageH, bottom, step: cw * STEP };
  });

  /* ---- The pile's covers. Each one's place comes from its depth; CSS glides them between places. */
  let spread = $state(false);
  let hot = $state<number | null>(null);
  let flying = $state<number[]>([]);
  const cards = new Map<number, HTMLElement>();
  const reg = (el: HTMLElement, id: number) => {
    cards.set(id, el);
    return { destroy: () => cards.get(id) === el && cards.delete(id) };
  };

  // A small, steady tilt per book, so the pile looks put down by hand rather than stacked by machine.
  const tilt = (id: number) => (((id * 2654435761) >>> 0) % 1000) / 1000 * 3.2 - 1.6;

  function place(d: number, id: number) {
    const f = fit!;
    const k = Math.min(d, VISIBLE - 1);
    const s = f.step * (spread ? 1.5 : 1);
    const lift = hot === id ? 6 : 0;
    return {
      transform: `translate(${-k * s}px, ${-k * s * 0.3 - lift}px) rotate(${d ? tilt(id) : 0}deg)`,
      filter: `brightness(${hot === id ? 1 : 1 - k * 0.14})`,
      opacity: d < VISIBLE ? 1 : 0,
      zIndex: 10 - k,
    };
  }

  let busy = false;
  let queued = 0;

  /** Bring the book `k` places down (k > 0) or up from the bottom (k < 0) to the top. */
  async function flip(k: number) {
    const n = pile.length;
    if (n < 2 || !fit || k % n === 0) return;
    if (busy) return void (queued = k);
    busy = true;
    const from = at;
    const to = (((from + k) % n) + n) % n;
    // The covers that go round: off the top and under (forward), or out from under and on top (back).
    const moving = pile
      .map((c, i) => ({ c, a: depth(i, from), b: depth(i, to) }))
      .filter(({ a, b }) => (k > 0 ? b > a : b < a))
      .sort((x, y) => (k > 0 ? x.a - y.a : y.b - x.b));
    flying = moving.map((m) => m.c.id);
    topId = pile[to].id;
    await tick();
    const cw = fit.cw;
    const aside = `translate(${-cw * 0.95}px, ${-cw * 0.14}px) rotate(-7deg)`;
    const runs = moving.map(({ c, a, b }, j) => {
      const el = cards.get(c.id);
      if (!el) return Promise.resolve();
      const p = place(a, c.id);
      const q = place(b, c.id);
      const [high, low] = [20, 10 - VISIBLE];
      // Out to the side quickly, then an unhurried slide into its new place.
      const [lift, settle] = ["cubic-bezier(0.2, 0.7, 0.3, 1)", "cubic-bezier(0.45, 0, 0.1, 1)"];
      const side = { transform: aside, filter: "brightness(1)", opacity: 1 };
      const frames =
        k > 0
          ? [
              { ...p, zIndex: high, easing: lift },
              { ...side, zIndex: high, offset: 0.4 },
              { ...side, zIndex: low, offset: 0.41, easing: settle },
              { ...q, zIndex: low },
            ]
          : [
              { ...p, opacity: 1, zIndex: low, easing: lift },
              { ...side, zIndex: low, offset: 0.4 },
              { ...side, zIndex: high, offset: 0.41, easing: settle },
              { ...q, zIndex: high },
            ];
      return el
        .animate(frames as Keyframe[], { duration: 640, delay: j * 70, fill: "backwards" })
        .finished.catch(() => {});
    });
    await Promise.all(runs);
    flying = [];
    busy = false;
    if (queued) {
      const next = queued;
      queued = 0;
      flip(next);
    }
  }

  // The wheel over the pile flips through it; a flick is one book, however many wheel events it makes.
  let pileEl = $state<HTMLElement>();
  $effect(() => {
    const el = pileEl;
    if (!el) return;
    let sum = 0;
    let last = 0;
    const onwheel = (e: WheelEvent) => {
      e.preventDefault();
      sum += e.deltaY;
      const now = performance.now();
      if (Math.abs(sum) < 40 || now - last < 240) return;
      last = now;
      flip(sum > 0 ? 1 : -1);
      sum = 0;
    };
    el.addEventListener("wheel", onwheel, { passive: false });
    return () => el.removeEventListener("wheel", onwheel);
  });

  function onkey(e: KeyboardEvent) {
    if (reader.id !== null || search.open || sheets.editing !== null || sheets.matching) return;
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    if ((e.target as HTMLElement)?.closest?.("input, textarea, select, [contenteditable]")) return;
    if (e.key === "ArrowRight") flip(1);
    else if (e.key === "ArrowLeft") flip(-1);
    else return;
    e.preventDefault();
  }

  /* ---- The page on the right follows the book on top, swapping in only once it's decoded. */
  $effect(() => {
    if (!hero) return;
    const src = pageUrl(hero.id, hero.currentPage);
    if (untrack(() => shown?.src) === src) return;
    const im = new Image();
    im.src = src;
    const show = (ratio: number) => {
      if (!hero || pageUrl(hero.id, hero.currentPage) !== src) return;
      const swap = !!untrack(() => shown);
      shown = { src, ratio };
      if (swap) tick().then(() => page?.animate([{ opacity: 0.3 }, { opacity: 1 }], { duration: 380, easing: "ease-out" }));
    };
    im.decode().then(
      () => show(im.naturalHeight / im.naturalWidth || 1.5),
      () => show(1.5),
    );
  });

  // The books either side of the top one are a flip away; have their pages ready.
  $effect(() => {
    const n = pile.length;
    if (n < 2) return;
    for (const i of [at + 1, at - 1 + n]) {
      const c = pile[i % n];
      new Image().src = pageUrl(c.id, c.currentPage);
    }
  });

  // The first time there's a pile, the top cover lifts once to show there's more underneath.
  $effect(() => {
    if (pile.length < 2 || !fit) return;
    let seen = true;
    try {
      seen = localStorage.getItem("aion.pileHint") === "1";
    } catch {}
    if (seen) return;
    const id = untrack(() => hero.id);
    const cw = untrack(() => fit.cw);
    const t = setTimeout(() => {
      try {
        localStorage.setItem("aion.pileHint", "1");
      } catch {}
      cards.get(id)?.animate(
        [
          { transform: "none" },
          { transform: `translate(${-cw * 0.22}px, ${-cw * 0.2}px) rotate(-5deg)`, offset: 0.4 },
          { transform: "none" },
        ],
        { duration: 1300, easing: "cubic-bezier(0.4, 0, 0.2, 1)", composite: "add" },
      );
    }, 1000);
    return () => clearTimeout(t);
  });

  const hotCard = $derived(hot !== null && hot !== hero?.id ? pile.find((c) => c.id === hot) : null);
</script>

<svelte:window onkeydown={onkey} />

{#if data}
  <!-- The first screen: the book you're in, with your library along the bottom. -->
  <div class="screen" class:solo={!hero}>
    {#if hero}
      <section class="hero">
        {#key hero.id}
          <div class="text" in:fly={{ y: 10, duration: 420 }}>
            <div class="when">{ago(hero.lastReadAt)}</div>
            <h1>{hero.seriesName.trim()}</h1>
            <div class="vol">{hero.label}{#if sub}&ensp;&mdash;&ensp;{sub}{/if}</div>
            <div class="where">
              <div class="prog"><i style:width="{progressOf(hero) * 100}%"></i></div>
              {#if hero.numPages}<span class="mono">{hero.currentPage + 1} / {hero.numPages}</span>{/if}
            </div>
            <div class="actions">
              <button class="btn play" onclick={resume}><Icon name="play" size={14} />Resume</button>
              <button
                class="ico"
                title="All {unitWord(hero.isManga, 2)}"
                aria-label="All {unitWord(hero.isManga, 2)}"
                onclick={() => nav.go({ name: "series", id: hero.seriesId })}
              >
                <Icon name="strip" size={18} />
              </button>
            </div>
          </div>
        {/key}

        <div class="page" bind:clientWidth={colW} bind:clientHeight={colH}>
          {#if fit && shown}
            <button
              class="open"
              onclick={resume}
              title="Resume on this page"
              style:left="{fit.pageX}px"
              style:top="{fit.pageY}px"
              style:width="{fit.pageW}px"
              style:height="{fit.pageH}px"
            >
              <img bind:this={page} src={shown.src} alt="" draggable="false" />
            </button>

            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="pile"
              bind:this={pileEl}
              title={pile.length > 1 ? "Continue reading" : undefined}
              style:left="{fit.pageX - fit.cw * (1 - OVER)}px"
              style:top="{fit.bottom}px"
              style:width="{fit.cw}px"
              onpointerenter={() => (spread = true)}
              onpointerleave={() => ((spread = false), (hot = null))}
            >
              {#each pile as c, i (c.id)}
                {@const p = place(depth(i), c.id)}
                {@const top = depth(i) === 0}
                <button
                  class="card"
                  class:flying={flying.includes(c.id)}
                  use:reg={c.id}
                  aria-label={top ? "Resume" : `${c.seriesName.trim()} ${c.label}`}
                  tabindex={depth(i) < VISIBLE ? 0 : -1}
                  style:transform={p.transform}
                  style:filter={p.filter}
                  style:opacity={p.opacity}
                  style:z-index={p.zIndex}
                  style:pointer-events={p.opacity ? undefined : "none"}
                  onpointerenter={() => (hot = c.id)}
                  onpointerleave={() => hot === c.id && (hot = null)}
                  onclick={() => (top ? resume() : flip(depth(i)))}
                >
                  <Cover id={c.id} ratio={PILE_RATIO} width={fit.cw} />
                </button>
              {/each}
            </div>

            {#if hotCard}
              <div class="hot" style:left="{fit.left}px" style:bottom="{colH - fit.bottom + fit.cw * PILE_RATIO + (Math.min(pile.length, VISIBLE) - 1) * fit.step * 0.45 + 22}px" style:width="{fit.out}px">
                <span>{hotCard.seriesName.trim()}</span>
                <span class="mono">{hotCard.label}</span>
              </div>
            {/if}

            {#if pile.length > 1}
              <div class="count" style:left="{fit.left}px" style:top="{fit.bottom + 8}px" style:width="{fit.out}px">
                <button class="ico" aria-label="Previous book" onclick={() => flip(-1)}><Icon name="chevronLeft" size={14} /></button>
                <span class="mono">{at + 1} / {pile.length}</span>
                <button class="ico" aria-label="Next book" onclick={() => flip(1)}><Icon name="chevronRight" size={14} /></button>
              </div>
            {/if}
          {/if}
        </div>
      </section>
    {/if}

    {#if lib.collections.length}
      <section class="library">
        <h2>Library</h2>
        <div class="cols">
          {#each lib.collections as c (c.id)}
            <button class="col" onclick={() => nav.root({ name: "collection", id: c.id })}>
              <div class="fan">
                {#each c.covers as id (id)}<Cover {id} width={112} />{/each}
              </div>
              <div class="name">{c.name}</div>
              <span class="mono">{c.seriesCount.toLocaleString()} series · {c.comicCount.toLocaleString()}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}
  </div>

  {#if empty}
    <p class="empty">Nothing in progress yet. Open something from your library and it will wait for you here.</p>
  {/if}
{/if}

<style>
  /* The hero takes what the library along the bottom leaves of the window. */
  /* On a very short window the first screen runs a little past the bottom rather than crowd. */
  .screen {
    height: max(100vh, 690px);
    display: flex;
    flex-direction: column;
  }
  .screen.solo {
    height: auto;
    min-height: 0;
    padding-top: 140px;
  }
  .hero {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(380px, 5fr) 7fr;
    gap: 48px;
    padding: 64px var(--pad) 36px;
    align-items: center;
  }
  .library {
    flex: none;
    padding: 0 var(--pad) 30px;
  }
  .library h2 {
    margin-bottom: 0;
  }
  .text {
    grid-column: 1;
    grid-row: 1;
    padding-bottom: 32px;
    min-width: 0;
  }
  .when {
    color: var(--cap);
    font-size: 15px;
  }
  h1 {
    margin-top: 18px;
    font-weight: 300;
    font-size: clamp(40px, min(5.6vw, 8.5vh), 84px);
    line-height: 0.98;
    letter-spacing: -0.045em;
    overflow-wrap: anywhere;
  }
  .vol {
    margin-top: 10px;
    font-weight: 300;
    font-size: 24px;
    color: var(--text-2);
    letter-spacing: -0.02em;
  }
  .where {
    margin-top: min(44px, 4vh);
    display: flex;
    align-items: center;
    gap: 16px;
    color: var(--text-2);
  }
  .where .prog {
    width: 200px;
  }
  .actions {
    margin-top: min(36px, 3.4vh);
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .play {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }

  .page {
    grid-column: 2;
    grid-row: 1;
    position: relative;
    height: 100%;
    min-height: 0;
  }
  .open {
    position: absolute;
  }
  .open img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    transition: transform 0.7s var(--ease);
  }
  .open:hover img {
    transform: scale(1.012);
  }

  /* Covers stand on the pile's bottom-left corner, so each one's place is a small shift from there. */
  .pile {
    position: absolute;
    height: 0;
  }
  .card {
    position: absolute;
    left: 0;
    bottom: 0;
    display: block;
    transform-origin: bottom left;
    box-shadow: 0 18px 44px -14px rgba(0, 0, 0, 0.95);
    transition:
      transform 0.55s var(--ease),
      filter 0.55s var(--ease),
      opacity 0.55s var(--ease);
  }
  .card.flying {
    transition: none;
  }
  .hot {
    position: absolute;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 13px;
    line-height: 1.3;
    color: var(--text);
    pointer-events: none;
    z-index: 30;
  }
  .hot .mono {
    color: var(--text-3);
  }
  .count {
    position: absolute;
    display: flex;
    align-items: center;
    gap: 2px;
    color: var(--text-3);
  }
  .count .ico {
    color: var(--text-3);
    padding: 4px;
  }
  .count .ico:hover {
    color: var(--text);
  }

  h2 {
    font-weight: 400;
    font-size: 15px;
    margin-bottom: 28px;
    display: flex;
    gap: 12px;
    align-items: baseline;
  }

  .cols {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 32px;
  }
  .col {
    display: block;
    padding-top: 18px;
  }
  .fan {
    position: relative;
    height: 172px;
  }
  .fan :global(.cover) {
    position: absolute;
    bottom: 0;
    left: 50%;
    box-shadow: 0 20px 50px -16px rgba(0, 0, 0, 0.95);
    transform-origin: bottom center;
    transition:
      transform 0.7s var(--ease),
      filter 0.7s var(--ease);
  }
  /* Front cover is the most recently read series; the two behind lean out from under it. */
  .fan :global(.cover:nth-child(1)) {
    transform: translateX(-50%);
    z-index: 1;
  }
  .fan :global(.cover:nth-child(2)) {
    transform: translateX(-80%) rotate(-6deg) scale(0.88);
    filter: brightness(0.55);
  }
  .fan :global(.cover:nth-child(3)) {
    transform: translateX(-20%) rotate(6deg) scale(0.88);
    filter: brightness(0.55);
  }
  .col:hover .fan :global(.cover:nth-child(1)) {
    transform: translateX(-50%) translateY(-6px);
  }
  .col:hover .fan :global(.cover:nth-child(2)) {
    transform: translateX(-96%) rotate(-10deg) scale(0.88);
    filter: brightness(0.8);
  }
  .col:hover .fan :global(.cover:nth-child(3)) {
    transform: translateX(-4%) rotate(10deg) scale(0.88);
    filter: brightness(0.8);
  }
  .name {
    margin-top: 22px;
    font-weight: 300;
    font-size: 21px;
    letter-spacing: -0.03em;
  }
  .col .mono {
    display: block;
    color: var(--text-3);
    margin-top: 4px;
  }
  .empty {
    padding: 160px var(--pad) 0;
  }
</style>
