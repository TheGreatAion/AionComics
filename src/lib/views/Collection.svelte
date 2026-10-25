<script lang="ts">
  import { api, unitWord, type CollectionView, type SeriesSummary } from "$lib/api";
  import { launch } from "$lib/flight";
  import { lib, nav } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon, { type IconName } from "$lib/components/Icon.svelte";

  type Sort = "name" | "read" | "added";
  // Each sort is a symbol; its name is the tooltip.
  const SORTS: { key: Sort; title: string; icon: IconName | null }[] = [
    { key: "name", title: "A to Z", icon: null },
    { key: "read", title: "Recently read", icon: "clock" },
    { key: "added", title: "Recently added", icon: "plus" },
  ];

  let { id }: { id: number } = $props();
  let data = $state<CollectionView | null>(null);
  let filter = $state("");
  let sort = $state<Sort>((localStorage.getItem("aion.collectionSort") as Sort | null) ?? "name");

  $effect(() => {
    void lib.version;
    const cid = id;
    api.collection(cid).then((d) => {
      if (cid !== id) return;
      data = d;
      nav.restore();
    });
  });
  $effect(() => {
    try {
      localStorage.setItem("aion.collectionSort", sort);
    } catch {}
  });

  const shown = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    const list = (data?.series ?? []).filter((s) => s.name.toLowerCase().includes(q));
    if (sort === "read") return [...list].sort((a, b) => (b.lastReadAt ?? 0) - (a.lastReadAt ?? 0));
    if (sort === "added") return [...list].sort((a, b) => (b.lastAddedAt ?? 0) - (a.lastAddedAt ?? 0));
    return list;
  });
  const total = $derived(data?.series.reduce((n, s) => n + s.count, 0) ?? 0);

  function status(s: SeriesSummary) {
    if (s.readCount === s.count) return "Read";
    if (s.inProgress) return "Reading";
    return `${s.count} ${unitWord(s.isManga, s.count)}`;
  }

  function open(e: MouseEvent, s: SeriesSummary) {
    launch((e.currentTarget as HTMLElement).querySelector("img"), `s${s.id}`);
    nav.go({ name: "series", id: s.id });
  }
</script>

{#if data}
  <div class="head">
    <div>
      <h1>{data.name}</h1>
      <div class="meta mono">
        {data.series.length.toLocaleString()} series · {total.toLocaleString()}
        {data.isManga ? unitWord(true, total) : total === 1 ? "book" : "books"}
      </div>
    </div>
    <div class="tools">
      <label class="filter" title="Filter">
        <Icon name="search" size={15} />
        <input placeholder="" bind:value={filter} spellcheck="false" aria-label="Filter" />
      </label>
      <div class="sorts">
        {#each SORTS as s (s.key)}
          <button class="ico" class:on={sort === s.key} title={s.title} aria-label={s.title} onclick={() => (sort = s.key)}>
            {#if s.icon}<Icon name={s.icon} size={16} />{:else}<span class="az mono">A–Z</span>{/if}
          </button>
        {/each}
      </div>
    </div>
  </div>

  {#if shown.length}
    <div class="grid">
      {#each shown as s (s.id)}
        <button class="series" onclick={(e) => open(e, s)}>
          <span class="art"><Cover id={s.coverId} ratio={s.coverRatio} /></span>
          {#if s.readCount > 0 && s.readCount < s.count}
            <span class="prog"><i style:width="{(s.readCount / s.count) * 100}%"></i></span>
          {:else}
            <span class="prog blank"></span>
          {/if}
          <span class="cap">
            <span class="name">{s.name}</span>
            <span class="detail">{status(s)}</span>
          </span>
        </button>
      {/each}
    </div>
  {:else}
    <p class="empty">Nothing matches “{filter}”.</p>
  {/if}
  <div class="foot"></div>
{/if}

<style>
  .head {
    padding: 160px var(--pad) 0;
    display: flex;
    align-items: flex-end;
    gap: 40px;
  }
  h1 {
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(76px, 9.5vw, 150px);
    line-height: 0.86;
    letter-spacing: 0.005em;
  }
  .meta {
    color: var(--text-3);
    margin-top: 20px;
  }
  .tools {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 28px;
    padding-bottom: 10px;
  }
  .sorts {
    display: flex;
    gap: 2px;
  }
  .sorts .ico {
    color: var(--text-3);
  }
  .sorts .ico.on {
    color: var(--text);
    background: rgba(255, 255, 255, 0.08);
  }
  .az {
    font-size: 11px;
  }
  .filter {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
    border-bottom: 1px solid var(--line);
    transition: border-color 0.2s;
  }
  .filter:focus-within {
    border-color: var(--text-2);
    color: var(--text-2);
  }
  .filter input {
    width: 150px;
    background: none;
    border: 0;
    padding: 6px 0;
    outline: none;
    user-select: text;
  }

  .grid {
    padding: 72px var(--pad) 0;
  }
  .series {
    display: block;
    text-align: left;
    min-width: 0;
  }
  .art {
    display: block;
    transition: transform 0.5s var(--ease);
  }
  .art :global(.cover) {
    transition: box-shadow 0.5s var(--ease);
  }
  .series:hover .art {
    transform: translateY(-4px);
  }
  .series:hover .art :global(.cover) {
    box-shadow: 0 18px 40px -12px rgba(0, 0, 0, 0.9);
  }
  .prog {
    display: block;
    margin-top: 10px;
  }
  .cap {
    display: block;
    height: 56px;
    margin-top: 13px;
  }
  .name {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    font-size: 13px;
    line-height: 1.35;
    color: var(--cap);
    transition: color 0.2s;
  }
  .series:hover .name {
    color: var(--text);
  }
  .detail {
    display: block;
    margin-top: 3px;
    font-size: 12px;
    color: var(--text-3);
  }
  .empty {
    padding: 72px var(--pad);
  }
  .foot {
    height: 140px;
  }
</style>
