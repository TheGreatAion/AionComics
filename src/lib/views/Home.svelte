<script lang="ts">
  import { tick, untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { ago, api, pageUrl, progressOf, storyTitle, unitWord, type ComicCard, type Home } from "$lib/api";
  import { lib, nav, openReader, reader, search, sheets } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let data = $state<Home | null>(null);

  $effect(() => {
    void lib.version;
    api.home().then((d) => {
      data = d;
      nav.restore();
    });
  });

  // Everything you're in the middle of, each book its cover and the page you're on, piled up.
  // The book on top is the one the hero describes; flipping changes only what's on top.
  const pile = $derived<ComicCard[]>(
    data ? (data.continueReading.length ? data.continueReading : data.pickUpAgain.slice(0, 12)) : [],
  );
  let topId = $state<number | null>(null);
  const at = $derived(Math.max(0, pile.findIndex((c) => c.id === topId)));
  const hero = $derived(pile[at]);
  const sub = $derived(hero ? storyTitle(hero) : null);
  const empty = $derived(data && !hero);

  const pages = new Map<number, HTMLImageElement>();
  const resume = () => hero && openReader(hero.id, pages.get(hero.id) ?? null);

  // How deep a book sits in the pile (0 = on top).
  const depth = (i: number, top = at) => (i - top + pile.length) % pile.length;

  /* ---- Layout. Sizes are shares of the book's height, so the pile looks the same at any window size. */
  const BEHIND = 3; // books you can see behind the top one; the rest wait hidden at the back
  const PEEK = 0.1; // how far each book behind rises above the one in front
  const SHIFT = 0.045; // and how far it sits to the right
  const SHRINK = 0.025; // and how much smaller it is
  const LIFT = 1.3; // hovering spreads the pile by this much
  const COVER_RATIO = 1.54;
  const GAP = 4; // between a book's cover and its page
  const CAPTION = 34; // room under the pile for the counter

  let colW = $state(0);
  let colH = $state(0);
  // Each book's page shape (height / width), learned as the pages load.
  let ratios = $state<Record<number, number>>({});
  const ratioOf = (id: number) => ratios[id] ?? COVER_RATIO;

  const fit = $derived.by(() => {
    if (!colW || !colH || !hero) return null;
    const n = Math.min(pile.length - 1, BEHIND);
    const caption = pile.length > 1 ? CAPTION : 0;
    const wide = (h: number) => h / COVER_RATIO + GAP + h / ratioOf(hero.id) + n * SHIFT * h * LIFT;
    // Hovering lifts the pile into the space above the hero, so only the resting pile needs room here.
    let h = (colH - caption) / (1 + n * PEEK);
    if (wide(h) > colW) h *= colW / wide(h);
    const tall = h * (1 + n * PEEK);
    const left = (colW - wide(h) + n * SHIFT * h * (LIFT - 1)) / 2;
    const top = (colH - caption - tall) / 2 + tall - h;
    return { h, left, top, coverW: h / COVER_RATIO };
  });

  /* ---- Each book's place comes from its depth; CSS glides them between places. */
  let spread = $state(false);
  let flying = $state<number[]>([]);
  const units = new Map<number, HTMLElement>();
  const reg = (map: Map<number, HTMLElement>) => (el: HTMLElement, id: number) => {
    map.set(id, el);
    return { destroy: () => map.get(id) === el && map.delete(id) };
  };
  const regUnit = reg(units);
  const regPage = reg(pages as Map<number, HTMLElement>);

  function place(d: number) {
    const h = fit!.h;
    const k = Math.min(d, BEHIND);
    const m = spread ? LIFT : 1;
    return {
      transform: `translate(${k * SHIFT * h * m}px, ${-k * PEEK * h * m}px) scale(${1 - k * SHRINK})`,
      filter: `brightness(${1 - k * 0.2})`,
      opacity: d <= BEHIND ? 1 : 0,
      zIndex: 10 - k,
    };
  }

  let busy = false;
  let queued = 0;

  /** Bring the book `k` places back (k > 0) or the one `-k` from the back (k < 0) to the top. */
  async function flip(k: number) {
    const n = pile.length;
    if (n < 2 || !fit || k % n === 0) return;
    if (busy) return void (queued = k);
    busy = true;
    const from = at;
    const to = (((from + k) % n) + n) % n;
    // The books that go round: off the top and to the back (forward), or out from the back and on top.
    const moving = pile
      .map((c, i) => ({ c, a: depth(i, from), b: depth(i, to) }))
      .filter(({ a, b }) => (k > 0 ? b > a : b < a))
      .sort((x, y) => (k > 0 ? x.a - y.a : y.b - x.b));
    flying = moving.map((m) => m.c.id);
    topId = pile[to].id;
    await tick();
    const h = fit.h;
    // Slid down and out to the left, clear of the pile, before going to its new place.
    const out = { transform: `translate(${-h * 0.42}px, ${h * 0.08}px) rotate(-3deg)`, filter: "brightness(1)", opacity: 1 };
    const [high, low] = [20, 10 - BEHIND - 1];
    const [lift, settle] = ["cubic-bezier(0.2, 0.7, 0.3, 1)", "cubic-bezier(0.45, 0, 0.1, 1)"];
    const runs = moving.map(({ c, a, b }, j) => {
      const el = units.get(c.id);
      if (!el) return Promise.resolve();
      const [p, q] = [place(a), place(b)];
      const frames =
        k > 0
          ? [
              { ...p, zIndex: high, easing: lift },
              { ...out, zIndex: high, offset: 0.4 },
              { ...out, zIndex: low, offset: 0.41, easing: settle },
              { ...q, zIndex: low },
            ]
          : [
              { ...p, opacity: 1, zIndex: low, easing: lift },
              { ...out, zIndex: low, offset: 0.4 },
              { ...out, zIndex: high, offset: 0.41, easing: settle },
              { ...q, zIndex: high },
            ];
      return el.animate(frames as Keyframe[], { duration: 680, delay: j * 80, fill: "backwards" }).finished.catch(() => {});
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

  // The first time there's a pile, the top book lifts once to show there's more behind it.
  $effect(() => {
    if (pile.length < 2 || !fit) return;
    let seen = true;
    try {
      seen = localStorage.getItem("aion.pileHint") === "1";
    } catch {}
    if (seen) return;
    const id = untrack(() => hero.id);
    const h = untrack(() => fit.h);
    const t = setTimeout(() => {
      try {
        localStorage.setItem("aion.pileHint", "1");
      } catch {}
      units.get(id)?.animate(
        [
          { transform: "none" },
          { transform: `translate(${-h * 0.12}px, ${h * 0.05}px) rotate(-2deg)`, offset: 0.4 },
          { transform: "none" },
        ],
        { duration: 1300, easing: "cubic-bezier(0.4, 0, 0.2, 1)", composite: "add" },
      );
    }, 1000);
    return () => clearTimeout(t);
  });
</script>

<svelte:window onkeydown={onkey} />

{#if data}
  <!-- The first screen: the book you're in, with your library along the bottom. -->
  <div class="screen" class:solo={!hero}>
    <!-- The book you're in tints the top of Home in its cover's color. -->
    {#if hero}
      <section class="hero">
        {#key hero.id}
          <div class="text" in:fly={{ y: 10, duration: 200 }}>
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

        <div class="stage" bind:clientWidth={colW} bind:clientHeight={colH}>
          {#if fit}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="pile"
              bind:this={pileEl}
              style:left="{fit.left}px"
              style:top="{fit.top}px"
              style:height="{fit.h}px"
              onpointerenter={() => (spread = true)}
              onpointerleave={() => (spread = false)}
            >
              {#each pile as c, i (c.id)}
                {@const d = depth(i)}
                {@const p = place(d)}
                <button
                  class="unit"
                  class:flying={flying.includes(c.id)}
                  class:top={d === 0}
                  use:regUnit={c.id}
                  title={d === 0 ? "Resume" : `${c.seriesName.trim()} ${c.label}`}
                  tabindex={d <= BEHIND ? 0 : -1}
                  style:transform={p.transform}
                  style:filter={p.filter}
                  style:opacity={p.opacity}
                  style:z-index={p.zIndex}
                  style:pointer-events={p.opacity ? undefined : "none"}
                  onclick={() => (d === 0 ? resume() : flip(d))}
                >
                  <Cover id={c.id} ratio={COVER_RATIO} width={fit.coverW} />
                  <div class="leaf" style:width="{fit.h / ratioOf(c.id)}px">
                    {#if d <= BEHIND + 1 || flying.includes(c.id)}
                      <img
                        use:regPage={c.id}
                        src={pageUrl(c.id, c.currentPage)}
                        alt=""
                        draggable="false"
                        onload={(e) => {
                          const im = e.currentTarget as HTMLImageElement;
                          if (im.naturalWidth) ratios[c.id] = im.naturalHeight / im.naturalWidth;
                        }}
                      />
                    {/if}
                  </div>
                </button>
              {/each}
            </div>

            {#if pile.length > 1}
              <div class="count" style:left="{fit.left}px" style:top="{fit.top + fit.h + 8}px">
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
                {#each c.covers as id (id)}<Cover {id} width={112} pages={null} />{/each}
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
    position: relative;
    isolation: isolate;
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
    padding: 64px var(--pad) 30px;
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
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(48px, min(6.8vw, 10.5vh), 108px);
    line-height: 0.9;
    letter-spacing: 0.005em;
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

  .stage {
    grid-column: 2;
    grid-row: 1;
    position: relative;
    height: 100%;
    min-height: 0;
  }
  .pile {
    position: absolute;
  }
  /* One book: its cover with the page you're on beside it. Behind ones rise from the bottom-left corner. */
  .unit {
    position: absolute;
    left: 0;
    top: 0;
    height: 100%;
    display: flex;
    gap: 4px;
    transform-origin: bottom left;
    transition:
      transform 0.55s var(--ease),
      filter 0.55s var(--ease),
      opacity 0.55s var(--ease);
  }
  .unit.flying {
    transition: none;
  }
  .unit > :global(.cover),
  .leaf {
    box-shadow: 0 18px 44px -14px rgba(0, 0, 0, 0.95);
  }
  .leaf {
    height: 100%;
    flex: none;
    overflow: hidden;
    background: var(--raised);
  }
  .leaf img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    object-position: top;
  }
  .unit.top .leaf img {
    object-fit: contain;
  }
  .unit.top:hover {
    translate: 0 -4px;
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
    font-family: var(--display);
    font-weight: 400;
    font-size: 30px;
    line-height: 1;
    letter-spacing: 0.01em;
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
