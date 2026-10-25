<script lang="ts">
  import { ago, api, pageUrl, progressOf, storyTitle, unitWord, type Home } from "$lib/api";
  import { lib, nav, openReader } from "$lib/state.svelte";
  import Book from "$lib/components/Book.svelte";
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

  // The book you're in: from the last month if there is one, else the last thing you put down.
  const hero = $derived(data?.continueReading[0] ?? data?.pickUpAgain[0]);
  const also = $derived(data?.continueReading.slice(1) ?? []);
  const sub = $derived(hero ? storyTitle(hero) : null);
  const empty = $derived(data && !hero);

  const resume = () => hero && openReader(hero.id, page ?? null);
</script>

{#if data}
  <!-- The first screen: the book you're in, with your library along the bottom. -->
  <div class="screen" class:solo={!hero}>
    {#if hero}
      <section class="hero">
        <div class="text">
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
        <div class="page">
          <button onclick={resume} title="Resume on this page">
            <img bind:this={page} src={pageUrl(hero.id, hero.currentPage)} alt="" draggable="false" />
          </button>
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

  {#if also.length}
    <section class="shelf">
      <h2>Also reading</h2>
      <div class="row">
        {#each also as c (c.id)}<Book comic={c} mode="shelf" width={168} />{/each}
      </div>
    </section>
  {/if}

  {#if empty}
    <p class="empty">Nothing in progress yet. Open something from your library and it will wait for you here.</p>
  {/if}
  <div class="foot"></div>
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
    padding: 64px var(--pad) 0;
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
    height: 100%;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px 0 20px;
  }
  .page button {
    height: 100%;
  }
  .page img {
    height: 100%;
    width: auto;
    max-width: 100%;
    object-fit: contain;
    transition: transform 0.7s var(--ease);
  }
  .page button:hover img {
    transform: scale(1.012);
  }

  .shelf {
    padding: 0 var(--pad);
    margin-top: 112px;
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
    margin-bottom: 28px;
    display: flex;
    gap: 12px;
    align-items: baseline;
  }
  .row {
    display: flex;
    gap: 32px;
    align-items: flex-end;
    overflow-x: auto;
    margin: 0 calc(var(--pad) * -1);
    padding: 8px var(--pad) 4px;
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
  .foot {
    height: 140px;
  }
</style>
