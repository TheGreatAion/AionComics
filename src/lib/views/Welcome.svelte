<script lang="ts">
  // The first thing anyone sees: where their comics are (or a fresh start), then what was found there
  // and how the folders were read, with a way to correct that.
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type CollectionSummary } from "$lib/api";
  import Icon from "$lib/components/Icon.svelte";
  import { lib } from "$lib/state.svelte";

  const SHOWN = 5;

  let step = $state<"start" | "fresh" | "looking" | "found">("start");
  let error = $state<string | null>(null);
  let books = $state(0);
  let series = $state(0);
  let libraries = $state<CollectionSummary[]>([]);
  let layout = $state("auto");
  let correcting = $state(false);

  async function begin(start: () => Promise<unknown>) {
    error = null;
    lib.welcoming = true;
    lib.scanning = true;
    try {
      await start();
      lib.hasRoot = true;
      step = "looking";
    } catch (e) {
      lib.welcoming = false;
      lib.scanning = false;
      error = String(e);
    }
  }

  async function pick(title: string) {
    error = null;
    const dir = await open({ directory: true, title });
    if (typeof dir === "string") begin(() => api.setLibraryRoot(dir));
  }

  // The folder's been read: show what's there, or go straight to the empty Home.
  $effect(() => {
    if (step === "looking" && !lib.scanning) show();
  });
  async function show() {
    const [s, n] = await Promise.all([api.settings(), api.nav()]);
    lib.collections = n.collections;
    if (!s.lastScan?.comics) {
      lib.welcoming = false;
      return;
    }
    books = s.lastScan.comics;
    series = s.lastScan.series;
    libraries = n.collections;
    layout = s.layout;
    step = "found";
  }

  async function read(as: string) {
    correcting = false;
    lib.scanning = true;
    step = "looking";
    await api.setLayout(as);
  }

  const many = (n: number, one: string, more: string) => `${n.toLocaleString()} ${n === 1 ? one : more}`;
</script>

<div class="welcome">
  <h1>AionComics</h1>
  {#if step === "start"}
    <p>A place for your comics and manga. It never moves, renames or deletes what's in your comics folder.</p>
    <div class="choices">
      <button class="btn" onclick={() => pick("Choose your comics folder")}><Icon name="folder" size={16} />I have a comics folder</button>
      <button class="btn quiet" onclick={() => (step = "fresh")}><Icon name="plus" size={16} />I'm starting fresh</button>
    </div>
  {:else if step === "fresh"}
    <p>A folder called Comics will be made in your Documents, to keep what you add.</p>
    <div class="choices">
      <button class="btn" onclick={() => begin(() => api.startFresh())}>Sounds good</button>
      <button class="link" onclick={() => pick("Choose where to keep your comics")}>Choose somewhere else</button>
      <button class="link" onclick={() => (step = "start")}>Back</button>
    </div>
  {:else if step === "looking"}
    <p class="looking"><span class="spin"><Icon name="rescan" size={15} /></span>Looking through your comics…</p>
  {:else}
    <p>Found {many(libraries.length, "library", "libraries")}, {many(series, "series", "series")} and {many(books, "book", "books")}.</p>
    <ul>
      {#each libraries.slice(0, SHOWN) as c (c.id)}
        <li><span>{c.name}</span><span class="mono">{many(c.seriesCount, "series", "series")} · {c.comicCount.toLocaleString()}</span></li>
      {/each}
      {#if libraries.length > SHOWN}<li class="rest">and {libraries.length - SHOWN} more</li>{/if}
    </ul>
    {#if correcting}
      <div class="ways">
        {#if layout !== "libraries"}
          <button onclick={() => read("libraries")}>Each folder is a library<small class="mono">Marvel / Saga / Saga 001.cbz</small></button>
        {/if}
        {#if layout !== "series"}
          <button onclick={() => read("series")}>Each folder is a series<small class="mono">Saga / Saga 001.cbz</small></button>
        {/if}
        {#if layout !== "auto"}
          <button onclick={() => read("auto")}>Work it out for me<small class="mono">Folder by folder</small></button>
        {/if}
      </div>
    {/if}
    <div class="choices">
      <button class="btn" onclick={() => (lib.welcoming = false)}>Looks right</button>
      {#if !correcting}<button class="link" onclick={() => (correcting = true)}>Something's off</button>{/if}
    </div>
  {/if}
  {#if error}<p class="err"><Icon name="alert" size={14} />{error}</p>{/if}
</div>

<style>
  .welcome {
    max-width: 560px;
    padding: 22vh var(--pad) 40px;
    margin: 0 auto;
  }
  h1 {
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(64px, 8vw, 116px);
    line-height: 0.88;
    letter-spacing: 0.005em;
  }
  p {
    margin: 24px 0 32px;
    color: var(--text-2);
    font-size: 15px;
    line-height: 1.5;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 22px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
  .btn.quiet {
    background: none;
    color: var(--text);
    box-shadow: inset 0 0 0 1px var(--line);
  }
  .btn.quiet:hover {
    background: rgb(var(--fg) / 0.07);
  }
  .looking {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .looking .spin {
    display: grid;
  }
  ul {
    margin: -12px 0 32px;
    padding: 0;
    list-style: none;
    font-size: 15px;
  }
  li {
    display: flex;
    justify-content: space-between;
    gap: 20px;
    padding: 9px 0;
    border-bottom: 1px solid var(--line);
  }
  li .mono {
    color: var(--text-3);
    font-size: 12px;
    white-space: nowrap;
  }
  .rest {
    color: var(--text-3);
    font-size: 13px;
  }
  .ways {
    display: grid;
    gap: 8px;
    margin-bottom: 28px;
  }
  .ways button {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px 16px;
    border-radius: 10px;
    text-align: left;
    box-shadow: inset 0 0 0 1px var(--line);
    transition: background 0.15s;
  }
  .ways button:hover {
    background: rgb(var(--fg) / 0.07);
  }
  .ways small {
    color: var(--text-3);
    font-size: 12px;
  }
  .err {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 20px 0 0;
  }
</style>
