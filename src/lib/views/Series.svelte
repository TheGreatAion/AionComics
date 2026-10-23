<script lang="ts">
  import { api, listText, plainText, unitWord, type SeriesView } from "$lib/api";
  import { lib, nav, openReader, sheets, showMenu } from "$lib/state.svelte";
  import Book from "$lib/components/Book.svelte";
  import Cover from "$lib/components/Cover.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let { id }: { id: number } = $props();
  let data = $state<SeriesView | null>(null);
  let head = $state<HTMLElement>();
  let synEl = $state<HTMLParagraphElement>();
  let expanded = $state(false);
  let clamped = $state(false);

  $effect(() => {
    void lib.version;
    const sid = id;
    api.series(sid).then((d) => {
      if (sid !== id) return;
      data = d;
      nav.restore();
    });
  });

  const all = $derived(data?.sections.flatMap((s) => s.comics) ?? []);
  const first = $derived(all[0]);
  const readCount = $derived(all.filter((c) => c.read).length);
  // The count by the Read button is the main run, so it matches the heading below it; other
  // editions and extras ("Perfect Edition", "Annuals") count in their own headings.
  const main = $derived(data?.sections.find((s) => !s.name) ?? data?.sections[0]);
  const mainRead = $derived(main?.comics.filter((c) => c.read).length ?? 0);
  const mainCount = $derived(main?.comics.length ?? 0);
  // Resume what's in progress, else the first unread, else start over.
  const target = $derived(all.find((c) => !c.read && c.currentPage > 0) ?? all.find((c) => !c.read) ?? first);
  const inProgress = $derived(!!target && !target.read && target.currentPage > 0);
  const action = $derived(
    !target
      ? ""
      : inProgress
        ? target.label
        : readCount === all.length
          ? "Again"
          : readCount > 0
            ? target.label
            : "Read",
  );
  const actionTitle = $derived(
    !target ? "" : inProgress ? `Continue ${target.label}` : readCount === all.length ? "Read again" : `Read ${target.label}`,
  );
  // "Makoto Yukimura · Kodansha Comics USA · 2013–2026", with the artist when it isn't the writer.
  const by = $derived.by(() => {
    if (!data) return "";
    const writer = listText(data.writer);
    const artist = listText(data.artist);
    const art = artist && artist !== writer ? `art by ${artist}` : "";
    return [writer, art, data.publisher, data.years].filter(Boolean).join(" · ");
  });
  const findOnCv = () => data && (sheets.matching = { seriesId: data.id, isManga: data.isManga, name: data.name });
  const synopsis = $derived(plainText(data?.synopsis ?? null));
  const titled = $derived((data?.sections.length ?? 0) > 1 || !!data?.sections[0]?.name);
  const shape = $derived(first?.coverRatio && first.coverRatio > 0.9 && first.coverRatio < 2.1 ? first.coverRatio : 1.5);
  const coverH = $derived(Math.min(440, Math.max(340, 300 * shape)));

  // Only offer "More" when the synopsis is actually cut off.
  $effect(() => {
    void synopsis;
    if (synEl && !expanded) clamped = synEl.scrollHeight > synEl.clientHeight + 2;
  });

  async function act(fn: () => Promise<void>) {
    await fn();
    lib.version++;
  }

  function resume() {
    if (!target) return;
    const img = document.querySelector<HTMLImageElement>(`[data-book="${target.id}"] img`) ?? head?.querySelector("img");
    openReader(target.id, img ?? null);
  }

  function options(e: MouseEvent) {
    if (!data) return;
    const sid = data.id;
    showMenu(e, [
      { label: "Mark series as read", action: () => act(() => api.setSeriesRead(sid, true)) },
      { label: "Mark series as unread", action: () => act(() => api.setSeriesRead(sid, false)) },
      "separator",
      { label: data.cvVolumeId ? "Match again on Comic Vine…" : "Find on Comic Vine…", action: findOnCv },
      { label: "Show in Explorer", action: () => first && api.reveal(first.id) },
    ]);
  }
</script>

{#if data}
  <section class="head" bind:this={head}>
    <Cover id={first?.id} ratio={shape} width={Math.round(coverH / shape)} flip="s{data.id}" />
    <div class="info">
      <button class="crumb link" onclick={() => nav.go({ name: "collection", id: data!.collectionId })}>
        {data.collectionName}
      </button>
      <h1>{data.name}</h1>
      {#if by}<div class="by">{by}</div>{/if}
      {#if synopsis}
        <p class="syn" class:open={expanded} bind:this={synEl}>{synopsis}</p>
        {#if clamped || expanded}
          <button class="more link" onclick={() => (expanded = !expanded)}>{expanded ? "Less" : "More"}</button>
        {/if}
      {/if}
      <div class="actions">
        {#if target}<button class="btn play" title={actionTitle} onclick={resume}><Icon name="play" size={14} />{action}</button>{/if}
        <span class="mono stats">
          {#if inProgress && target?.numPages}<span title="Page in {target.label}">{`${target.currentPage + 1} / ${target.numPages}`}</span>{/if}
          <span title="{mainRead} of {mainCount} {unitWord(data.isManga, mainCount)} read"><Icon name="check" size={13} />{mainRead} / {mainCount}</span>
        </span>
        <span class="tools">
          {#if !data.cvVolumeId && !synopsis}
            <button class="ico" title="Find details on Comic Vine" aria-label="Find details on Comic Vine" onclick={findOnCv}>
              <Icon name="fetch" size={18} />
            </button>
          {/if}
          <button class="ico" title="More" aria-label="More" onclick={options} oncontextmenu={options}><Icon name="more" size={18} /></button>
        </span>
      </div>
      {#if data.backfill}
        {@const b = data.backfill}
        <p class="backfill" title="You're on {b.beforeLabel}">
          {b.count} earlier {unitWord(data.isManga, b.count)} read?
          <button class="ico" title="Mark them read" aria-label="Mark them read" onclick={() => act(() => api.markReadBefore(b.beforeId))}>
            <Icon name="check" size={16} />
          </button>
          <button class="ico" title="Not now" aria-label="Not now" onclick={() => act(() => api.dismissBackfill(data!.id))}>
            <Icon name="close" size={15} />
          </button>
        </p>
      {/if}
    </div>
  </section>

  {#each data.sections as s (s.name)}
    <section class="sec">
      {#if titled}
        <h2>
          {s.name || (data.isManga ? "Volumes" : "Issues")}
          <span class="mono" title="{s.comics.filter((c) => c.read).length} read">{s.comics.length}</span>
        </h2>
      {/if}
      <div class="grid">
        {#each s.comics as c (c.id)}
          <div data-book={c.id}><Book comic={c} mode="volume" /></div>
        {/each}
      </div>
    </section>
  {/each}
  <div class="foot"></div>
{/if}

<style>
  .head {
    padding: 140px var(--pad) 0;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 72px;
    align-items: end;
  }
  .crumb {
    font-size: 13px;
  }
  h1 {
    margin-top: 16px;
    font-weight: 300;
    font-size: clamp(48px, 5.4vw, 80px);
    line-height: 0.98;
    letter-spacing: -0.045em;
    overflow-wrap: anywhere;
  }
  .by {
    margin-top: 18px;
    color: var(--text-2);
    font-size: 15px;
  }
  .syn {
    margin-top: 28px;
    max-width: 64ch;
    color: var(--text-2);
    font-size: 15px;
    line-height: 1.65;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    user-select: text;
  }
  .syn.open {
    display: block;
  }
  .more {
    margin-top: 8px;
    font-size: 13px;
  }
  .actions {
    margin-top: 36px;
    display: flex;
    align-items: center;
    gap: 28px;
  }
  .stats {
    color: var(--text-3);
    display: flex;
    gap: 16px;
  }
  .stats > span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .play {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }
  .tools {
    display: flex;
    gap: 2px;
    margin-left: -12px;
  }
  .backfill {
    margin-top: 22px;
    color: var(--text-2);
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .sec {
    padding: 96px var(--pad) 0;
  }
  .sec + .sec {
    padding-top: 72px;
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
    display: flex;
    gap: 12px;
    align-items: baseline;
    margin-bottom: 36px;
  }
  h2 .mono {
    color: var(--text-3);
  }
  .grid > div {
    min-width: 0;
  }
  .foot {
    height: 140px;
  }
</style>
