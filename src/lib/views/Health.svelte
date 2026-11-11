<script lang="ts">
  import { api, type HealthKind, type HealthStatus } from "$lib/api";
  import { nav } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon from "$lib/components/Icon.svelte";

  // A check of every book: is the file there, does it open, does it have pages, does its cover read.
  // It finds what would fail to open before you try to read it. Nothing is changed or fixed here:
  // each book found can be shown in Explorer to deal with there, then checked again.
  let status = $state<HealthStatus | null>(null);
  let rechecking = $state<number | null>(null);

  const WHAT: Record<HealthKind, { name: string; why: string }> = {
    missing: { name: "Missing", why: "The file isn't where it was. It may have been moved or deleted outside AionComics." },
    damaged: { name: "Damaged", why: "The file is there but can't be read as a comic. It may be cut short or broken." },
    empty: { name: "No pages", why: "The file opens, but there are no pictures in it." },
    cover: { name: "Cover won't load", why: "The first page doesn't read as a picture." },
  };
  const ORDER: HealthKind[] = ["damaged", "missing", "empty", "cover"];

  async function refresh() {
    status = await api.healthStatus();
  }

  // While a check runs, follow it.
  $effect(() => {
    refresh().then(() => nav.restore());
  });
  $effect(() => {
    if (!status?.running) return;
    const t = setInterval(refresh, 600);
    return () => clearInterval(t);
  });

  async function start() {
    await api.healthStart();
    await refresh();
  }
  async function stop() {
    await api.healthStop();
    await refresh();
  }
  async function recheck(id: number) {
    rechecking = id;
    try {
      await api.healthRecheck(id);
      await refresh();
    } finally {
      rechecking = null;
    }
  }

  const groups = $derived(
    ORDER.map((kind) => ({ kind, books: status?.problems.filter((p) => p.kind === kind) ?? [] })).filter((g) => g.books.length),
  );
  const when = (s: number) => new Date(s * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
  const pct = $derived(status && status.total ? Math.round((status.done / status.total) * 100) : 0);
</script>

<div class="page">
  <h1>Library check</h1>
  <p class="lede">Goes through every book and lists the ones that wouldn't open: files that are missing, damaged or empty, and covers that won't load. Nothing is changed or deleted. A big library takes a while the first time (about half an hour for several thousand books on a hard drive), and it waits whenever you're reading.</p>

  {#if status}
    <div class="bar">
      {#if status.running}
        <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={status.total} aria-valuenow={status.done}>
          <div class="fill" style:width="{pct}%"></div>
        </div>
        <span class="fine">{status.done.toLocaleString()} of {status.total.toLocaleString()} books</span>
        <button class="btn" onclick={stop}>Stop</button>
      {:else}
        <button class="btn" onclick={start}><Icon name="rescan" size={16} />{status.finishedAt ? "Check again" : "Check the library"}</button>
        {#if status.finishedAt}
          <span class="fine">Last checked {when(status.finishedAt)} · {status.problems.length ? `${status.problems.length.toLocaleString()} ${status.problems.length === 1 ? "book" : "books"} found` : "everything opens"}</span>
        {:else if status.done}
          <span class="fine">Stopped after {status.done.toLocaleString()} of {status.total.toLocaleString()} books</span>
        {/if}
      {/if}
    </div>

    {#each groups as g (g.kind)}
      <section>
        <h2>{WHAT[g.kind].name} <span class="count fine">{g.books.length.toLocaleString()}</span></h2>
        <p class="why fine">{WHAT[g.kind].why}</p>
        {#each g.books as b (b.comic.id)}
          <div class="book">
            <button class="art" data-nav onclick={() => nav.go({ name: "series", id: b.comic.seriesId, book: b.comic.id })} title="Open its series">
              <Cover id={b.comic.id} ratio={b.comic.coverRatio} width={44} />
            </button>
            <div class="what">
              <div>{b.comic.seriesName.trim()} <span class="dim">{b.comic.label}</span></div>
              <div class="path code" title={b.relPath}>{b.relPath}</div>
            </div>
            <button class="ico" title="Check it again" aria-label="Check it again" disabled={rechecking === b.comic.id} onclick={() => recheck(b.comic.id)}>
              <Icon name="rescan" size={16} />
            </button>
            <button class="ico" title="Show in Explorer" aria-label="Show in Explorer" disabled={g.kind === "missing"} onclick={() => api.reveal(b.comic.id)}>
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
  .lede {
    margin-top: 14px;
    max-width: 640px;
    color: var(--text-2);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: 26px;
    min-height: 36px;
  }
  .bar .btn {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
  .bar .fine {
    color: var(--text-2);
  }
  .progress {
    width: 320px;
    height: 4px;
    border-radius: 2px;
    background: var(--track, rgb(var(--fg) / 0.12));
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--text);
    transition: width 0.4s var(--ease);
  }
  section {
    margin-top: 34px;
    padding-top: 18px;
    border-top: 1px solid var(--line);
  }
  h2 {
    font-size: 15px;
    font-weight: 600;
  }
  .count {
    margin-left: 6px;
    color: var(--text-3);
    font-weight: 400;
  }
  .why {
    margin: 4px 0 10px;
    color: var(--text-2);
  }
  .book {
    display: grid;
    grid-template-columns: 44px minmax(0, 1fr) 32px 32px;
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
</style>
