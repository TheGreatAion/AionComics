<script lang="ts">
  import { api, type DupGroup } from "$lib/api";
  import { lib, nav } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon from "$lib/components/Icon.svelte";

  // Books that may be in the library twice. AionComics never deletes a file: each copy can be
  // shown in Explorer to deal with there, and a group that isn't really duplicates can be hidden.
  let groups = $state<DupGroup[] | null>(null);

  $effect(() => {
    void lib.version;
    api.duplicates().then((g) => {
      groups = g;
      nav.restore();
    });
  });

  async function notDuplicates(key: string) {
    await api.ignoreDuplicate(key);
    groups = groups?.filter((g) => g.key !== key) ?? null;
  }

  const size = (n: number | null) => (n == null ? "" : n >= 1_073_741_824 ? `${(n / 1_073_741_824).toFixed(2)} GB` : `${(n / 1_048_576).toFixed(1)} MB`);
  const books = $derived(groups?.reduce((n, g) => n + g.books.length, 0) ?? 0);
</script>

<div class="page">
  <h1>Duplicates</h1>
  {#if groups}
    <p class="sum fine">
      {groups.length ? `${groups.length.toLocaleString()} groups · ${books.toLocaleString()} books` : "None found"}&ensp;·&ensp;nothing here is ever deleted for you
    </p>
    {#each groups as g (g.key)}
      <section>
        <div class="why">
          {#each g.reasons as r (r)}<span class="chip">{r}</span>{/each}
          <button class="link not" onclick={() => notDuplicates(g.key)} title="Hide this group for good">Not duplicates</button>
        </div>
        {#each g.books as b (b.comic.id)}
          <div class="book">
            <button class="art" data-nav onclick={() => nav.go({ name: "series", id: b.comic.seriesId, book: b.comic.id })} title="Open its series">
              <Cover id={b.comic.id} ratio={b.comic.coverRatio} width={44} />
            </button>
            <div class="what">
              <div>{b.comic.seriesName.trim()} <span class="dim">{b.comic.label}</span></div>
              <div class="path code" title={b.relPath}>{b.relPath}</div>
            </div>
            <span class="fine dim">{b.comic.numPages ? `${b.comic.numPages} pp` : ""}</span>
            <span class="fine dim size">{size(b.size)}</span>
            <button class="ico" title="Show in Explorer" aria-label="Show in Explorer" onclick={() => api.reveal(b.comic.id)}>
              <Icon name="folder" size={16} />
            </button>
          </div>
        {/each}
      </section>
    {/each}
  {/if}
</div>

<style>
  .page {
    padding: 120px var(--pad) 140px;
    max-width: 1180px;
  }
  h1 {
    font-family: var(--display);
    font-weight: 400;
    font-size: 52px;
    line-height: 0.9;
  }
  .sum {
    margin-top: 14px;
    color: var(--text-2);
  }
  section {
    margin-top: 34px;
    padding-top: 18px;
    border-top: 1px solid var(--line);
  }
  .why {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }
  .chip {
    padding: 2px 10px;
    border: 1px solid rgb(var(--fg) / 0.12);
    border-radius: 999px;
    color: var(--text-2);
    font-size: 12px;
  }
  .not {
    margin-left: auto;
    font-size: 13px;
  }
  .book {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr) 64px 84px 32px;
    align-items: center;
    gap: 16px;
    padding: 8px 0;
  }
  .art {
    display: block;
  }
  .what {
    min-width: 0;
  }
  .path {
    margin-top: 3px;
    font-size: 12px;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }
  .dim {
    color: var(--text-3);
  }
  .size {
    text-align: right;
  }
</style>
