<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import { adding } from "$lib/add.svelte";
  import { api, type AddProgress, type Placement } from "$lib/api";
  import { lib, nav, reader } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";
  import { dropOn, side } from "./SidePanel.svelte";

  // Drop comics on the window to copy them into the comics folder: onto a series page or a series
  // in the side panel, into that series; onto a collection page or a collection in the side panel,
  // into the series of the same name (or a new one). Folders bring the comics inside them (a folder
  // of loose pages becomes one book), and a .zip or .rar of several books is unpacked. While
  // dragging, the overlay says where each book will go; nothing is copied until you let go.
  // The + buttons bring comics here too (see add.svelte.ts), to be confirmed in the same words.
  let plan = $state<Placement[] | null>(null);
  let note = $state("");
  let progress = $state<AddProgress | null>(null);
  let noteTimer: ReturnType<typeof setTimeout> | undefined;

  type Target = { key: string; series: number | null; collection: number | null };

  // The side panel row under the pointer, or else the page you're on.
  function targetAt(pos?: { x: number; y: number }): Target {
    if (pos && side.open) {
      const px = window.devicePixelRatio || 1;
      const row = document.elementFromPoint(pos.x / px, pos.y / px)?.closest<HTMLElement>("[data-drop]");
      const key = row?.dataset.drop;
      if (key) {
        const [kind, id] = key.split(":");
        return { key, series: kind === "series" ? +id : null, collection: kind === "collection" ? +id : null };
      }
    }
    const r = nav.current;
    const series = r.name === "series" ? r.id : null;
    const collection = r.name === "collection" ? r.id : null;
    return { key: `page:${series}:${collection}`, series, collection };
  }

  let paths: string[] = [];
  let aimed = "";
  let asked = 0;
  async function aim(t: Target) {
    if (t.key === aimed) return;
    aimed = t.key;
    dropOn.key = t.key.startsWith("page:") ? null : t.key;
    const n = ++asked;
    const p = await api.planAdd(paths, t.series, t.collection).catch(() => null);
    if (n === asked && aimed) plan = p;
  }
  function stop() {
    asked++;
    aimed = "";
    dropOn.key = null;
    plan = null;
  }

  function say(text: string, ms = 3500) {
    clearTimeout(noteTimer);
    note = text;
    if (ms) noteTimer = setTimeout(() => (note = ""), ms);
  }

  async function copyIn(paths: string[], series: number | null, collection: number | null, name: string | null, n: number) {
    progress = null;
    say(`Copying ${n}…`, 0);
    try {
      const added = await api.addComics(paths, series, collection, name);
      say(added ? `Added ${added}` : "Nothing new to add");
    } catch (err) {
      say(String(err), 6000);
    } finally {
      progress = null;
    }
  }

  // Comics chosen with a + button. For a collection they're one series under a name you can change,
  // or kept apart as their names say when they look like several.
  let sorted = $state<Placement[] | null>(null);
  let named = $state<Placement[] | null>(null);
  let name = $state("");
  let apart = $state(false);
  const ask = $derived(adding.ask);
  const toSeries = $derived(ask?.target.series ?? null);
  const toCollection = $derived(ask?.target.collection ?? null);
  const folders = (places: Placement[]) => {
    const seen = new Map<string, number>();
    for (const p of places) if (p.folder) seen.set(p.folder, (seen.get(p.folder) ?? 0) + 1);
    return [...seen].sort((a, b) => b[1] - a[1]).map(([f]) => f);
  };
  const seriesCount = $derived(folders(sorted ?? []).length);
  // Anything but adding to a series is a matter of which series, even with no library yet.
  const naming = $derived(!!ask && toSeries === null && !apart);

  let chosen = 0;
  $effect(() => {
    const a = adding.ask;
    const n = ++chosen;
    sorted = named = null;
    apart = false;
    name = "";
    if (!a) return;
    say("Looking through it…", 0);
    api
      .planAdd(a.paths, a.target.series ?? null, a.target.collection ?? null)
      .then((places) => {
        if (n !== chosen) return;
        note = "";
        name = folders(places)[0]?.split("/").pop() ?? "";
        // From one place they're taken for one series, as asked; from several, for what their names say.
        const from = new Set(places.map((p) => p.source.split(" › ")[0].replace(/[\\/][^\\/]*$/, "")));
        apart = folders(places).length > 1 && from.size > 1;
        sorted = places;
      })
      .catch((err) => {
        if (n === chosen) {
          say(String(err), 6000);
          adding.ask = null;
        }
      });
  });
  // The chosen books under the name as it's typed.
  $effect(() => {
    const a = adding.ask;
    const typed = name.trim();
    if (!a || a.target.series !== undefined || !sorted || !typed) {
      named = null;
      return;
    }
    const n = ++asked;
    const t = setTimeout(async () => {
      const p = await api.planAdd(a.paths, null, a.target.collection ?? null, typed).catch(() => null);
      if (n === asked) named = p;
    }, 120);
    return () => clearTimeout(t);
  });

  function closeAsk() {
    adding.ask = null;
  }
  function confirmAsk() {
    if (!ask || !ready) return;
    const { paths } = ask;
    const [series, collection, as] = [toSeries, toCollection, naming ? name.trim() : null];
    closeAsk();
    copyIn(paths, series, collection, as, adds);
  }
  function askKeys(e: KeyboardEvent) {
    if ((!ask && !adding.newLibrary) || (e.key !== "Escape" && e.key !== "Enter")) return;
    if (e.key === "Enter" && e.target instanceof HTMLButtonElement) return;
    e.preventDefault();
    e.stopPropagation();
    if (adding.newLibrary) {
      if (e.key === "Escape") closeLibrary();
      else makeLibrary();
    } else if (e.key === "Escape") closeAsk();
    else confirmAsk();
  }

  // A new library: named here, made at once, and opened ready for its first series.
  let libraryName = $state("");
  let libraryError = $state("");
  function closeLibrary() {
    adding.newLibrary = false;
    libraryName = libraryError = "";
  }
  async function makeLibrary() {
    if (!libraryName.trim()) return;
    try {
      const id = await api.newLibrary(libraryName);
      closeLibrary();
      lib.version++;
      nav.root({ name: "collection", id });
    } catch (err) {
      libraryError = String(err);
    }
  }
  function pickAll(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  // "1.2 of 4.5 GB", or in MB under a gigabyte.
  function amount(at: number, of: number) {
    const big = of >= 1e9;
    const n = (v: number) => (big ? (v / 1e9).toFixed(1) : Math.round(v / 1e6).toString());
    return `${n(at)} of ${n(of)} ${big ? "GB" : "MB"}`;
  }

  onMount(() => {
    const off = getCurrentWebview().onDragDropEvent(async (e) => {
      const ev = e.payload;
      if (reader.id !== null) return;
      if (ev.type === "enter") {
        paths = ev.paths;
        aimed = "";
        aim(targetAt(ev.position));
      } else if (ev.type === "over") {
        if (aimed) aim(targetAt(ev.position));
      } else if (ev.type === "leave") {
        stop();
      } else if (ev.type === "drop") {
        const t = targetAt(ev.position);
        stop();
        // Planned again for where it landed: the pointer may have moved since the last plan.
        const placed = await api.planAdd(ev.paths, t.series, t.collection).catch(() => [] as Placement[]);
        const toAdd = placed.filter((p) => p.status === "add" || p.status === "new-series").length;
        if (!toAdd) {
          if (placed.some((p) => p.status === "exists")) say("Already there");
          return;
        }
        copyIn(ev.paths, t.series, t.collection, null, toAdd);
      }
    });
    const unlisten = listen<AddProgress>("add-progress", (e) => {
      const p = e.payload;
      if (p.done >= p.total) return;
      progress = p;
      const size = p.totalBytes > 50e6 ? ` · ${amount(p.bytes, p.totalBytes)}` : "";
      say(p.total > 1 ? `Copying ${p.done + 1} of ${p.total}${size}` : `Copying${size}`, 0);
    });
    return () => {
      off.then((un) => un());
      unlisten.then((un) => un());
    };
  });

  // What the card shows: the drag under way, or the books waiting to be confirmed.
  const showing = $derived(ask ? (naming ? (name.trim() ? named : []) : sorted) : plan);

  // One line per destination folder.
  const groups = $derived.by(() => {
    if (!showing) return [];
    const by = new Map<string, { folder: string; n: number; isNew: boolean }>();
    for (const p of showing) {
      if ((p.status === "add" || p.status === "new-series") && p.folder) {
        const g = by.get(p.folder) ?? { folder: p.folder, n: 0, isNew: p.status === "new-series" };
        g.n++;
        by.set(p.folder, g);
      }
    }
    return [...by.values()];
  });
  const count = (s: Placement["status"]) => showing?.filter((p) => p.status === s).length ?? 0;
  const made = (how: Placement["how"]) =>
    showing?.filter((p) => p.how === how && (p.status === "add" || p.status === "new-series")).length ?? 0;
  const adds = $derived(groups.reduce((n, g) => n + g.n, 0));
  const ready = $derived(adds > 0);
  const pretty = (folder: string) => folder.split("/").join("  /  ");
</script>

<svelte:window onkeydowncapture={askKeys} />

{#snippet where()}
  {#each groups as g (g.folder)}
    <p class="dest">
      {pretty(g.folder)}
      <span class="fine">{g.isNew ? "new · " : ""}{g.n}</span>
    </p>
  {/each}
  {#if count("exists")}<p class="aside">{count("exists")} already there</p>{/if}
  {#if count("name-taken")}<p class="aside">{count("name-taken")} skipped: a different file has that name there</p>{/if}
  {#if count("no-place")}<p class="aside">{ask ? "That name can't be a folder" : "Open a collection, or point at one in the side panel, to add new series"}</p>{/if}
  {#if count("not-comic")}<p class="aside">{count("not-comic")} not a comic, a folder or a .zip of comics</p>{/if}
  {#if made("unpack")}<p class="aside">{made("unpack")} unpacked from a .zip or .rar</p>{/if}
  {#if made("pack")}<p class="aside">{made("pack")} made from {made("pack") === 1 ? "a folder" : "folders"} of pages</p>{/if}
  {#if groups.length}<p class="hint fine">copied in, your originals stay where they are</p>{/if}
{/snippet}

{#if plan}
  <div class="drop" role="presentation">
    <div class="card">
      <span class="icon"><Icon name={groups.length ? "plus" : "alert"} size={30} stroke={1.2} /></span>
      {@render where()}
    </div>
  </div>
{/if}

{#if ask && sorted}
  <div class="drop asking" role="presentation" onclick={(e) => e.target === e.currentTarget && closeAsk()}>
    <div class="card" role="dialog" aria-modal="true" aria-label="Add comics">
      <h2>Add to {ask.target.title}</h2>
      {#if naming}
        <label class="named">
          <span>Series</span>
          <input bind:value={name} use:pickAll spellcheck="false" />
        </label>
      {/if}
      {@render where()}
      {#if toSeries === null && seriesCount > 1}
        <button class="link apart" onclick={() => (apart = !apart)}>
          {apart ? "Make them one series" : `Keep them as ${seriesCount} series`}
        </button>
      {/if}
      <div class="choices">
        <button class="link" onclick={closeAsk}>Cancel</button>
        <button class="btn" disabled={!ready} onclick={confirmAsk}>Add{adds ? ` ${adds}` : ""}</button>
      </div>
    </div>
  </div>
{/if}

{#if adding.newLibrary}
  <div class="drop asking" role="presentation" onclick={(e) => e.target === e.currentTarget && closeLibrary()}>
    <div class="card" role="dialog" aria-modal="true" aria-label="New library">
      <h2>New library</h2>
      <label class="named">
        <span>Name</span>
        <input bind:value={libraryName} oninput={() => (libraryError = "")} use:pickAll spellcheck="false" />
      </label>
      {#if libraryError}<p class="aside">{libraryError}</p>{/if}
      <p class="hint fine">a new folder in your comics folder</p>
      <div class="choices">
        <button class="link" onclick={closeLibrary}>Cancel</button>
        <button class="btn" disabled={!libraryName.trim()} onclick={makeLibrary}>Create</button>
      </div>
    </div>
  </div>
{/if}

{#if note}
  <div class="note">
    {#if note.startsWith("Copying")}<span class="spin"><Icon name="rescan" size={14} /></span>{/if}
    {note}
    {#if progress && progress.totalBytes}
      <i class="bar"><i style:width="{(progress.bytes / progress.totalBytes) * 100}%"></i></i>
    {/if}
  </div>
{/if}

<style>
  /* Over the page, leaving the side panel clear to aim at. */
  .drop {
    position: fixed;
    inset: 0 0 0 var(--side-now);
    z-index: 300;
    display: grid;
    place-items: center;
    background: rgba(6, 6, 6, 0.78);
    backdrop-filter: blur(14px);
    pointer-events: none;
    animation: fade 0.15s var(--ease);
  }
  /* Waiting on an answer: over everything, and it takes the clicks. */
  .drop.asking {
    inset: 0;
    pointer-events: auto;
  }
  h2 {
    margin: 0 0 8px;
    font-size: 19px;
    font-weight: 500;
    letter-spacing: -0.01em;
  }
  .named {
    align-self: stretch;
    display: flex;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
    font-size: 13px;
    text-align: left;
  }
  .named:focus-within {
    border-color: var(--text-2);
  }
  .named input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    color: var(--text);
    font-size: 16px;
  }
  .named input:focus-visible {
    outline: none;
  }
  .apart {
    margin-top: 4px;
    font-size: 13px;
  }
  .choices {
    align-self: stretch;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 22px;
    margin-top: 14px;
  }
  .choices .btn:disabled {
    opacity: 0.4;
    pointer-events: none;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .card {
    min-width: 360px;
    max-width: 560px;
    padding: 34px 40px;
    border-radius: 14px;
    box-shadow: 0 0 0 1px var(--line);
    background: var(--raised);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
  }
  .icon {
    display: grid;
    margin-bottom: 8px;
    color: var(--text);
  }
  .dest {
    font-size: 17px;
    font-weight: 400;
    letter-spacing: -0.01em;
  }
  .dest .fine {
    margin-left: 10px;
    color: var(--text-3);
  }
  .aside {
    color: var(--text-2);
    font-size: 13px;
  }
  .hint {
    margin-top: 10px;
    color: var(--text-3);
    font-size: 12px;
  }
  .note {
    position: fixed;
    left: 50%;
    bottom: 32px;
    transform: translateX(-50%);
    z-index: 300;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 18px;
    border-radius: 999px;
    background: var(--menu);
    box-shadow: 0 0 0 1px var(--line), 0 12px 40px rgba(0, 0, 0, 0.6);
    font-size: 13px;
    white-space: nowrap;
  }
  .note .spin {
    display: grid;
  }
  .note .bar {
    position: absolute;
    left: 18px;
    right: 18px;
    bottom: 4px;
    height: 2px;
    border-radius: 1px;
    background: var(--track);
    overflow: hidden;
  }
  .note .bar i {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--text-2);
    transition: width 0.15s linear;
  }
</style>
