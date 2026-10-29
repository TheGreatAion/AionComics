<script lang="ts">
  import { api, storyTitle, type PeopleView } from "$lib/api";
  import { lib, nav, openReader } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";

  // Everything in the library by one creator, or with one character or team in it, one row of
  // books per series, the series they're most in first.
  let { kind, value }: { kind: "creator" | "character"; value: string } = $props();
  let data = $state<PeopleView | null>(null);

  $effect(() => {
    void lib.version;
    const [k, v] = [kind, value];
    api.people(k, v).then((d) => {
      if (k !== kind || v !== value) return;
      data = d;
      nav.restore();
    });
  });

  const seriesCount = $derived(data?.series.length ?? 0);
  // What a creator did: "Pencils 412 · Cover 30 · Inks 2". (For a character it'd only repeat the count.)
  const roles = $derived(
    kind === "creator"
      ? (data?.roles ?? []).map(([role, n]) => `${role[0].toUpperCase()}${role.slice(1)} ${n.toLocaleString()}`).join(" · ")
      : "",
  );
  const first = $derived(data?.series[0]?.comics[0]);
</script>

{#if data}
  <div class="page">
    <header>
      <div class="kind">{kind === "creator" ? "Creator" : "Character"}</div>
      <h1>{data.name}</h1>
      <div class="sum mono">
        {data.books.toLocaleString()} {data.books === 1 ? "book" : "books"} · {seriesCount.toLocaleString()} series{#if roles}&ensp;·&ensp;{roles}{/if}
      </div>
    </header>

    {#if !data.books}
      <p class="none">No books in your library list {data.name} yet.</p>
    {/if}

    {#each data.series as s (s.id)}
      <section>
        <div class="head">
          <button class="name" onclick={() => nav.go({ name: "series", id: s.id })}>{s.name}</button>
          <span class="mono dim">{s.collectionName} · {s.comics.length}</span>
        </div>
        <div class="row">
          {#each s.comics as c (c.id)}
            {@const t = storyTitle(c)}
            <button
              class="pick"
              class:done={c.read}
              title="{c.label}{t ? ` · ${t}` : ''}"
              onclick={() => nav.go({ name: "series", id: s.id, book: c.id })}
              ondblclick={() => openReader(c.id)}
            >
              <Cover id={c.id} ratio={c.coverRatio} width={92} pages={c.numPages} />
              <span class="cap mono">{c.label}</span>
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>
{/if}

<style>
  .page {
    position: relative;
    isolation: isolate;
    padding: 120px 0 140px;
  }
  header {
    padding: 0 var(--pad);
  }
  .kind {
    font-size: 13px;
    color: var(--text-2);
  }
  h1 {
    margin-top: 10px;
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(64px, 8vw, 120px);
    line-height: 0.88;
    letter-spacing: 0.005em;
    overflow-wrap: anywhere;
  }
  .sum {
    margin-top: 16px;
    color: var(--text-2);
  }
  .none {
    padding: 48px var(--pad) 0;
    color: var(--text-2);
  }
  section {
    margin-top: 56px;
  }
  .head {
    padding: 0 var(--pad);
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-bottom: 14px;
  }
  .name {
    font-family: var(--display);
    font-size: 30px;
    line-height: 1;
    letter-spacing: 0.01em;
    color: var(--text);
  }
  .name:hover {
    color: var(--strong);
  }
  .dim {
    color: var(--text-3);
  }
  /* A long run scrolls sideways rather than stretching the page. */
  .row {
    display: flex;
    gap: 22px;
    overflow-x: auto;
    padding: 10px var(--pad) 8px;
    align-items: flex-end;
  }
  .pick {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }
  .pick :global(.cover) {
    box-shadow: 0 16px 34px -16px rgba(0, 0, 0, 0.95);
    transition:
      transform 0.45s var(--ease),
      opacity 0.25s;
  }
  .pick.done :global(.cover) {
    opacity: 0.6;
  }
  .pick:hover :global(.cover) {
    transform: translateY(-5px);
    opacity: 1;
  }
  .cap {
    margin-top: 8px;
    font-size: 11px;
    color: var(--text-3);
  }
</style>
