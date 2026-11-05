<script lang="ts">
  import { tick, untrack } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { addMenu, adding } from "$lib/add.svelte";
  import { libraryMenu } from "$lib/manage.svelte";
  import { ago, api, listText, pageUrl, printUrl, progressOf, storyTitle, unitWord, type ComicCard, type Home } from "$lib/api";
  import { lib, nav, openReader, reader, search, sheets, showMenu, undoable } from "$lib/state.svelte";
  import { bareEnter } from "$lib/keynav";
  import { hue } from "$lib/color";
  import Cover from "$lib/components/Cover.svelte";
  import Icon from "$lib/components/Icon.svelte";
  import InkPrint from "$lib/components/InkPrint.svelte";
  import { look, motionMs } from "$lib/look.svelte";

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
  // Nothing in the library at all, once that's known: somewhere to start, not "nothing in progress".
  const noBooks = $derived(lib.collectionsLoaded && !lib.scanning && lib.collections.every((c) => c.comicCount === 0));
  // With one library or none there's only one place to add to; with several, each has its own +.
  const addHere = $derived(
    lib.collections.length === 1
      ? { collection: lib.collections[0].id, title: lib.collections[0].name }
      : lib.collections.length === 0
        ? { title: "your comics" }
        : null,
  );
  // The splash panel prints the top book's cover in its own color; a gray cover, or one that would
  // print teal, prints in gray.
  const heroHue = $derived.by(() => {
    const c = hue(hero?.coverColor);
    return c && c.chroma > 0.12 && !(c.h >= 150 && c.h <= 200) ? c.h : null;
  });
  // Who made the book on top, and how long the rest of it should take at the pace you've read it.
  let about = $state<{ id: number; by: string; secsPerPage: number | null } | null>(null);
  $effect(() => {
    const id = hero?.id;
    if (!id) return;
    let live = true;
    Promise.all([api.comicMeta(id), api.bookStats(id).catch(() => null)]).then(([m, s]) => {
      if (!live) return;
      const names = [m.fields.writer, m.fields.penciller].flatMap((v) => listText(v).split(", ")).filter(Boolean);
      about = {
        id,
        by: [...new Set(names)].slice(0, 2).join(" · "),
        secsPerPage: s && s.pages >= 5 ? s.seconds / s.pages : null,
      };
    }).catch(() => {
      // The book's gone (removed just now); the pile is about to change.
    });
    return () => (live = false);
  });
  const shown = $derived(about && about.id === hero?.id ? about : null);
  const timeLeft = $derived.by(() => {
    if (!hero?.numPages || !shown?.secsPerPage) return null;
    const mins = Math.round(((hero.numPages - hero.currentPage - 1) * shown.secsPerPage) / 60);
    if (mins < 3) return "A few minutes left";
    return mins < 60 ? `About ${mins} min left` : `About ${Math.floor(mins / 60)} hr ${mins % 60} min left`;
  });

  const pages = new Map<number, HTMLImageElement>();
  const resume = () => hero && openReader(hero.id, pages.get(hero.id) ?? null);

  // Off the pile, with the page you're on kept; reading the book again brings it back. Taking off
  // the book on top leaves the next one on top.
  async function remove(c: ComicCard) {
    if (c.id === hero?.id && pile.length > 1) topId = pile[(at + 1) % pile.length].id;
    // Taken back, it's on top again.
    await undoable(`${c.seriesName.trim()} ${c.label} taken off`, () => api.hideFromReading(c.id), () => (topId = c.id));
  }
  const bookMenu = (e: MouseEvent, c: ComicCard) =>
    showMenu(e, [
      { label: "Resume", action: () => openReader(c.id, pages.get(c.id) ?? null) },
      { label: `All ${unitWord(c.isManga, 2)}`, action: () => nav.go({ name: "series", id: c.seriesId }) },
      "separator",
      { label: "Remove from Continue reading", action: () => remove(c) },
    ]);

  // How deep a book sits in the pile (0 = on top).
  const depth = (i: number, top = at) => (i - top + pile.length) % pile.length;

  /* ---- Layout. Sizes are shares of the book's height, so the pile looks the same at any window size. */
  const BEHIND = 3; // books you can see behind the top one; the rest wait hidden at the back
  const PEEK = 0.08; // how far each book behind rises straight up above the one in front
  const SHRINK = 0.05; // and how much smaller it is
  const LIFT = 1.3; // hovering spreads the pile by this much
  const COVER_RATIO = 1.54;
  const GAP = 4; // between a book's cover and its page
  const CAPTION = 58; // room under the pile for the arrows and counter

  let colW = $state(0);
  let colH = $state(0);
  // Each book's page shape (height / width), learned as the pages load.
  let ratios = $state<Record<number, number>>({});
  const ratioOf = (id: number) => ratios[id] ?? COVER_RATIO;

  const fit = $derived.by(() => {
    if (!colW || !colH || !hero) return null;
    const n = Math.min(pile.length - 1, BEHIND);
    const caption = pile.length > 1 ? CAPTION : 0;
    const wide = (h: number) => h / COVER_RATIO + GAP + h / ratioOf(hero.id);
    // Hovering lifts the pile into the space above the hero, so only the resting pile needs room here.
    let h = (colH - caption) / (1 + n * PEEK);
    if (wide(h) > colW) h *= colW / wide(h);
    const tall = h * (1 + n * PEEK);
    const w = wide(h);
    const left = (colW - w) / 2;
    const top = (colH - caption - tall) / 2 + tall - h;
    return { h, left, top, w, coverW: h / COVER_RATIO };
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

  // Books stand centered on the one on top, whatever the shape of their page.
  const unitLeft = (id: number) => (fit!.w - (fit!.coverW + GAP + fit!.h / ratioOf(id))) / 2;

  function place(d: number) {
    const h = fit!.h;
    const k = Math.min(d, BEHIND);
    const m = spread ? LIFT : 1;
    return {
      transform: `translateY(${-k * PEEK * h * m}px) scale(${1 - k * SHRINK})`,
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
    // Slid straight out to the left, clear of the pile, before going to its new place.
    const out = { transform: `translate(${-h * 0.42}px, ${h * 0.04}px)`, filter: "brightness(1)", opacity: 1 };
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
      return el.animate(frames as Keyframe[], { duration: motionMs(680, 820), delay: motionMs(j * 80, j * 80), fill: "backwards" }).finished.catch(() => {});
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

  // ← and → flip the pile (once a collection's picked, they move between collections instead),
  // Enter resumes the book on top.
  function onkey(e: KeyboardEvent) {
    if (reader.id !== null || search.open || sheets.editing !== null || sheets.matching || e.defaultPrevented) return;
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    if ((e.target as HTMLElement)?.closest?.("input, textarea, select, [contenteditable]")) return;
    if (document.activeElement?.closest("[data-nav]")) return;
    if (bareEnter(e)) resume();
    else if (e.key === "ArrowRight") flip(1);
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
          { transform: `translate(${-h * 0.12}px, ${h * 0.03}px)`, offset: 0.4 },
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
      <!-- A splash panel: the book you're in, over its cover printed in one ink. -->
      <section class="hero" class:inked={look.backdrop}>
        {#if look.backdrop}
          {#key hero.id}
            <div class="print" transition:fade={{ duration: motionMs(300, 600) }}>
              <InkPrint id={hero.id} hue={heroHue} at={40} />
            </div>
          {/key}
        {/if}
        <div class="caption when">{ago(hero.lastReadAt)}</div>
        {#key hero.id}
          <div class="text" in:fly={{ y: 10, duration: motionMs(200, 420) }}>
            <h1>{hero.seriesName.trim()}</h1>
            <div class="vol">{hero.label}{#if sub}&ensp;&mdash;&ensp;{sub}{/if}</div>
            <div class="by">{shown?.by ?? ""}</div>
            <div class="where">
              <div class="prog"><i style:width="{progressOf(hero) * 100}%"></i></div>
              {#if hero.numPages}
                <div class="nums mono">
                  <span>Page {hero.currentPage + 1} of {hero.numPages}</span><span>{Math.round(progressOf(hero) * 100)}%</span>
                </div>
              {/if}
              <div class="left">{timeLeft ?? ""}</div>
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
              <button class="ico" title="Remove from Continue reading" aria-label="Remove from Continue reading" onclick={() => remove(hero)}>
                <Icon name="close" size={16} />
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
                  style:left="{unitLeft(c.id)}px"
                  style:transform={p.transform}
                  style:filter={p.filter}
                  style:opacity={p.opacity}
                  style:z-index={p.zIndex}
                  style:pointer-events={p.opacity ? undefined : "none"}
                  onclick={() => (d === 0 ? resume() : flip(d))}
                  oncontextmenu={(e) => bookMenu(e, c)}
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
              <!-- Under the book: arrows to turn to the next, and where you are in the pile. -->
              <div class="turns" style:left="{fit.left + fit.w / 2}px" style:top="{fit.top + fit.h + 14}px">
                <button class="ico" title="Previous book" aria-label="Previous book" onclick={() => flip(-1)}><Icon name="arrowLeft" size={22} /></button>
                <span class="count mono">{at + 1} of {pile.length}</span>
                <button class="ico" title="Next book" aria-label="Next book" onclick={() => flip(1)}><Icon name="arrowRight" size={22} /></button>
              </div>
            {/if}
          {/if}
        </div>
      </section>
    {/if}

    {#if lib.collections.length}
      <!-- The library as a tier of panels, each collection's name and counts in a caption box. -->
      <section class="library" aria-label="Library">
        <div class="cols">
          {#each lib.collections as c (c.id)}
            <div class="slot" role="group" oncontextmenu={(e) => libraryMenu(e, c, false)}>
              <button class="col" data-nav onclick={() => nav.root({ name: "collection", id: c.id })}>
                <div class="caption">
                  <span class="named">{c.name}</span><span class="mono">{c.seriesCount.toLocaleString()} series · {c.comicCount.toLocaleString()}</span>
                </div>
                <div class="fan">
                  {#each c.covers as id (id)}<Cover {id} width={112} pages={null} />{/each}
                </div>
              </button>
              <button class="ico add" title="Add a series to {c.name}" aria-label="Add a series to {c.name}" onclick={(e) => addMenu(e, { collection: c.id, title: c.name })}>
                <Icon name="plus" size={16} />
              </button>
            </div>
          {/each}
        </div>
      </section>
    {/if}
  </div>

  {#if empty && noBooks}
    <div class="start">
      <h1>No comics yet</h1>
      {#if addHere}
        <p>Add some here, or drag files and folders anywhere in this window.</p>
        <div class="ways">
          <button class="btn" onclick={(e) => addMenu(e, addHere)}><Icon name="plus" size={16} />Add comics</button>
          <button class="link" onclick={() => (adding.newLibrary = true)}>New library</button>
        </div>
      {:else}
        <p>Open a library above and press + to add a series to it.</p>
      {/if}
    </div>
  {:else if empty}
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
  /* The splash panel: inked frame, the cover printed behind, when you last read in a caption box
     on its corner. */
  .hero {
    flex: 1;
    min-height: 0;
    position: relative;
    display: grid;
    grid-template-columns: minmax(380px, 5fr) 7fr;
    gap: 48px;
    margin: 76px var(--pad) 20px;
    padding: 40px 48px 16px;
    border: 4px solid var(--ink);
    background: var(--raised);
    align-items: center;
  }
  .print {
    position: absolute;
    inset: 0;
  }
  .hero > .when {
    position: absolute;
    left: -4px;
    top: -4px;
    z-index: 2;
  }
  .library {
    flex: none;
    padding: 0 var(--pad) 30px;
  }
  .text {
    position: relative;
    grid-column: 1;
    grid-row: 1;
    padding-bottom: 16px;
    min-width: 0;
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
  .by {
    margin-top: 8px;
    min-height: 1.4em;
    font-size: 14px;
    color: var(--text-2);
  }
  /* How far along: the line, the page and share, and the time the rest should take. */
  .where {
    margin-top: min(36px, 3.4vh);
    max-width: 360px;
    color: var(--text-2);
  }
  .where .prog {
    height: 2px;
  }
  .nums {
    margin-top: 9px;
    display: flex;
    justify-content: space-between;
    font-size: 13px;
  }
  .left {
    margin-top: 4px;
    min-height: 1.4em;
    font-size: 14px;
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
  /* One book: its cover with the page you're on beside it. Behind ones rise straight up, centered,
     each a little smaller from the top down, so the whole of each rise shows. */
  .unit {
    position: absolute;
    top: 0;
    height: 100%;
    display: flex;
    gap: 4px;
    transform-origin: top center;
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
  .turns {
    position: absolute;
    translate: -50% 0;
    display: flex;
    align-items: center;
    gap: 14px;
    z-index: 11;
  }
  .turns .ico {
    color: var(--text);
    opacity: 0.75;
  }
  .turns .ico:hover {
    opacity: 1;
  }
  .count {
    font-size: 13px;
    color: var(--text-2);
    white-space: nowrap;
  }

  /* Every panel the same width, on every row. */
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 20px;
  }
  .slot {
    position: relative;
    display: flex;
    min-width: 0;
  }
  /* In the panel's corner, under its caption box. */
  .slot > .add {
    position: absolute;
    right: 6px;
    top: 42px;
    z-index: 3;
  }
  .col {
    position: relative;
    display: block;
    flex: 1;
    min-width: 0;
    padding: 52px 0 20px;
    border: 4px solid var(--ink);
    background: var(--raised);
  }
  /* The panel's whole width, so every box is one length: the name, and its counts at the far end. */
  .col > .caption {
    position: absolute;
    left: -4px;
    right: -4px;
    top: -4px;
    z-index: 2;
    width: auto;
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
  }
  .caption .named {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .caption .mono {
    flex: none;
    font-size: 12px;
    letter-spacing: 0;
    text-transform: none;
    color: #55504a;
    vertical-align: 3px;
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
  .empty {
    padding: 160px var(--pad) 0;
  }
  .start {
    padding: 20vh var(--pad) 0;
    max-width: 640px;
  }
  .start h1 {
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(56px, 7vw, 96px);
    line-height: 0.9;
  }
  .start p {
    margin: 22px 0 30px;
    color: var(--text-2);
    font-size: 15px;
  }
  .ways {
    display: flex;
    align-items: center;
    gap: 22px;
  }
  .ways .btn {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
</style>
