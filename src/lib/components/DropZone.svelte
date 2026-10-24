<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";
  import { api, type Placement } from "$lib/api";
  import { nav, reader } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";

  // Drop .cbz/.cbr files on the window to copy them into the comics folder: onto a series page,
  // into that series; onto a collection page, into the series of the same name (or a new one).
  // While dragging, the overlay says where each file will go; nothing is copied until you let go.
  let plan = $state<Placement[] | null>(null);
  let note = $state("");
  let noteTimer: ReturnType<typeof setTimeout> | undefined;

  const target = () => {
    const r = nav.current;
    return {
      series: r.name === "series" ? r.id : null,
      collection: r.name === "collection" ? r.id : null,
    };
  };

  function say(text: string, ms = 3500) {
    clearTimeout(noteTimer);
    note = text;
    if (ms) noteTimer = setTimeout(() => (note = ""), ms);
  }

  onMount(() => {
    const off = getCurrentWebview().onDragDropEvent(async (e) => {
      const ev = e.payload;
      if (reader.id !== null) return;
      if (ev.type === "enter") {
        const t = target();
        plan = await api.planAdd(ev.paths, t.series, t.collection).catch(() => null);
      } else if (ev.type === "leave") {
        plan = null;
      } else if (ev.type === "drop") {
        const t = target();
        const adding = (plan ?? []).filter((p) => p.status === "add" || p.status === "new-series").length;
        plan = null;
        if (!adding) return;
        say(`Copying ${adding}…`, 0);
        try {
          const n = await api.addComics(ev.paths, t.series, t.collection);
          say(n ? `Added ${n}` : "Nothing new to add");
        } catch (err) {
          say(String(err), 6000);
        }
      }
    });
    return () => off.then((un) => un());
  });

  // One line per destination folder.
  const groups = $derived.by(() => {
    if (!plan) return [];
    const by = new Map<string, { folder: string; n: number; isNew: boolean }>();
    for (const p of plan) {
      if ((p.status === "add" || p.status === "new-series") && p.folder) {
        const g = by.get(p.folder) ?? { folder: p.folder, n: 0, isNew: p.status === "new-series" };
        g.n++;
        by.set(p.folder, g);
      }
    }
    return [...by.values()];
  });
  const count = (s: Placement["status"]) => plan?.filter((p) => p.status === s).length ?? 0;
  const pretty = (folder: string) => folder.split("/").join("  /  ");
</script>

{#if plan}
  <div class="drop" role="presentation">
    <div class="card">
      <span class="icon"><Icon name={groups.length ? "plus" : "alert"} size={30} stroke={1.2} /></span>
      {#each groups as g (g.folder)}
        <p class="dest">
          {pretty(g.folder)}
          <span class="mono">{#if g.isNew}new · {/if}{g.n}</span>
        </p>
      {/each}
      {#if count("exists")}<p class="aside">{count("exists")} already there</p>{/if}
      {#if count("name-taken")}<p class="aside">{count("name-taken")} skipped: a different file has that name there</p>{/if}
      {#if count("no-place")}<p class="aside">Open a collection to add new series</p>{/if}
      {#if count("not-comic")}<p class="aside">{count("not-comic")} not .cbz or .cbr</p>{/if}
      {#if groups.length}<p class="hint mono">copied as they are, never moved or renamed</p>{/if}
    </div>
  </div>
{/if}

{#if note}
  <div class="note">
    {#if note.startsWith("Copying")}<span class="spin"><Icon name="rescan" size={14} /></span>{/if}
    {note}
  </div>
{/if}

<style>
  .drop {
    position: fixed;
    inset: 0;
    z-index: 300;
    display: grid;
    place-items: center;
    background: rgba(6, 6, 6, 0.78);
    backdrop-filter: blur(14px);
    pointer-events: none;
    animation: fade 0.15s var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .card {
    min-width: 360px;
    padding: 34px 40px;
    border-radius: 14px;
    box-shadow: 0 0 0 1px var(--line);
    background: #111;
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
    font-weight: 300;
    letter-spacing: -0.01em;
  }
  .dest .mono {
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
    font-size: 11px;
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
    background: #1a1a1a;
    box-shadow: 0 0 0 1px var(--line), 0 12px 40px rgba(0, 0, 0, 0.6);
    font-size: 13px;
  }
  .note .spin {
    display: grid;
  }
</style>
