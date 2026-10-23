<script lang="ts">
  import { api, unitWord, type CvIssue, type CvProposal, type CvVolume } from "$lib/api";
  import { lib, nav, sheets } from "$lib/state.svelte";
  import { untrack } from "svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  // Match a series to Comic Vine in two steps: pick the series (best guess preselected), then check
  // which issue each book is. Confident pairs are filled in; only the uncertain ones need a look.
  let { seriesId, isManga, name }: { seriesId: number; isManga: boolean; name: string } = $props();

  let step = $state<"search" | "review" | "done">("search");
  // Start from the folder name without scene tags like "(2015) (Digital)".
  let query = $state(untrack(() => name.replace(/\s*[([][^)\]]*[)\]]/g, "").trim()));
  let volumes = $state<CvVolume[]>([]);
  let chosen = $state<CvVolume | null>(null);
  let issues = $state<CvIssue[]>([]);
  let rows = $state<CvProposal[]>([]);
  let busy = $state("");
  let error = $state("");
  let saved = $state(0);

  const unit = (n: number) => unitWord(isManga, n);

  async function run<T>(label: string, fn: () => Promise<T>): Promise<T | undefined> {
    busy = label;
    error = "";
    try {
      return await fn();
    } catch (e) {
      error = String(e);
    } finally {
      busy = "";
    }
  }

  async function search(q?: string) {
    const r = await run("Searching",() => api.cvSearch(seriesId, q));
    if (!r) return;
    query = r.query;
    volumes = r.volumes;
    chosen = r.volumes[0] ?? null;
  }
  // Search straight away with the starting name.
  search(untrack(() => query));

  async function next() {
    if (!chosen) return;
    const v = chosen;
    const r = await run("Loading",() => api.cvPlan(seriesId, v.id));
    if (!r) return;
    issues = r.issues;
    rows = r.proposals;
    step = "review";
  }

  async function save() {
    if (!chosen) return;
    const v = chosen;
    const pairs = rows.map((r) => ({ comicId: r.comicId, issueId: r.issueId }));
    const n = await run("Saving",() => api.cvApply(seriesId, v.id, pairs));
    if (n === undefined) return;
    saved = n;
    step = "done";
    lib.version++;
  }

  const byId = $derived(new Map(issues.map((i) => [i.id, i])));
  const matched = $derived(rows.filter((r) => r.issueId).length);
  const unsure = $derived(rows.filter((r) => r.issueId && r.confidence === "check").length);
  const issueLabel = (i: CvIssue) =>
    [i.number ? (isManga ? `Vol. ${i.number}` : `#${i.number}`) : null, i.name, i.coverDate?.slice(0, 4)]
      .filter(Boolean)
      .join(" · ");

  function pick(row: CvProposal, id: string) {
    row.issueId = id || null;
    row.confidence = id ? "sure" : "none";
  }

  const close = () => (sheets.matching = null);

  function onkey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    e.preventDefault();
    e.stopImmediatePropagation();
    close();
  }
</script>

<svelte:window onkeydowncapture={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="sheet" role="dialog" aria-label="Find on Comic Vine">
    <header>
      <h2>{step === "review" && chosen ? chosen.name : "Find on Comic Vine"}</h2>
      {#if step === "review"}
        <span class="sub mono">
          <span title="Matched"><Icon name="check" size={13} />{matched} / {rows.length}</span>
          {#if unsure}<span title="Worth a check"><Icon name="alert" size={13} />{unsure}</span>{/if}
        </span>
      {/if}
      <button class="ico close" title="Close (Esc)" aria-label="Close" onclick={close}><Icon name="close" size={16} /></button>
    </header>

    {#if step === "search"}
      <form class="search" onsubmit={(e) => (e.preventDefault(), search(query))}>
        <input bind:value={query} placeholder="Series name" spellcheck="false" />
        <button class="ico" title="Search" aria-label="Search" disabled={!!busy}><Icon name="search" size={18} /></button>
      </form>
      <div class="results">
        {#each volumes as v (v.id)}
          <button class="volume" class:on={chosen?.id === v.id} onclick={() => (chosen = v)} ondblclick={next}>
            {#if v.image}<img src={v.image} alt="" loading="lazy" />{:else}<span class="noimg"></span>{/if}
            <span class="info">
              <span class="vname">{v.name}</span>
              <span class="meta mono">
                {[v.startYear, v.publisher, v.count ? `${v.count} ${unit(v.count)}` : null].filter(Boolean).join(" · ")}
              </span>
              {#if v.deck}<span class="deck">{v.deck}</span>{/if}
            </span>
          </button>
        {:else}
          {#if busy}
            <div class="wait" title={busy}><span class="spin"><Icon name="rescan" size={22} stroke={1.2} /></span></div>
          {:else if !error}<p class="empty">Nothing found. Try a shorter name.</p>{/if}
        {/each}
      </div>
    {:else if step === "review"}
      <div class="pairs">
        {#each rows as r (r.comicId)}
          {@const issue = r.issueId ? byId.get(r.issueId) : null}
          <div class="pair" class:check={r.confidence === "check"} class:none={!r.issueId}>
            <Cover id={r.comicId} width={44} />
            <span class="local">
              {r.label}{#if r.section}<span class="section">{r.section}</span>{/if}
            </span>
            <span class="arrow">→</span>
            {#if issue?.image}<img src={issue.image} alt="" loading="lazy" />{:else}<span class="noimg small"></span>{/if}
            <select value={r.issueId ?? ""} onchange={(e) => pick(r, (e.currentTarget as HTMLSelectElement).value)}>
              <option value="">—</option>
              {#each issues as i (i.id)}<option value={i.id}>{issueLabel(i)}</option>{/each}
            </select>
            <span class="state" title={r.confidence === "sure" ? "Number and year agree" : r.confidence === "check" ? "Worth a check" : "Not matched"}>
              {#if r.confidence === "sure"}<Icon name="check" size={14} />{:else if r.confidence === "check"}<Icon name="alert" size={14} />{/if}
            </span>
          </div>
        {/each}
      </div>
    {:else}
      <p class="done" title="Credits and characters follow in the background, a few a minute">
        <Icon name="check" size={18} />{saved} {unit(saved)} saved
      </p>
    {/if}

    <footer>
      {#if busy && (step !== "search" || volumes.length)}<span class="busy" title={busy}><span class="spin"><Icon name="rescan" size={15} /></span></span>{/if}
      {#if error}
        <span class="err"><Icon name="alert" size={14} />{error}</span>
        {#if error.includes("API key")}
          <button class="ico" title="Add key in Settings" aria-label="Open Settings" onclick={() => (close(), nav.root({ name: "settings" }))}>
            <Icon name="key" size={16} />
          </button>
        {/if}
      {/if}
      <span class="spacer"></span>
      {#if step === "search"}
        <button class="btn arrow-btn" title="Match {unit(2)}" aria-label="Next" disabled={!chosen || !!busy} onclick={next}>
          <Icon name="arrowRight" size={16} />
        </button>
      {:else if step === "review"}
        <button class="ico" title="Back" aria-label="Back" onclick={() => (step = "search")}><Icon name="back" size={17} /></button>
        <button class="btn" disabled={!matched || !!busy} onclick={save}>Save {matched}</button>
      {:else}
        <button class="btn" onclick={close}>Done</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 150;
    display: grid;
    place-items: center;
    background: rgba(6, 6, 6, 0.72);
    backdrop-filter: blur(18px);
    animation: fade 0.2s var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .sheet {
    width: min(860px, calc(100vw - 48px));
    height: min(760px, calc(100vh - 96px));
    display: flex;
    flex-direction: column;
    padding: 26px 30px 20px;
    border-radius: 12px;
    background: #121212;
    box-shadow: 0 0 0 1px var(--line), 0 30px 80px rgba(0, 0, 0, 0.7);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 16px;
  }
  h2 {
    font-weight: 300;
    font-size: 26px;
    letter-spacing: -0.03em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    color: var(--text-2);
    white-space: nowrap;
    display: flex;
    gap: 14px;
  }
  .sub > span,
  .err,
  .done {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .close {
    margin-left: auto;
    align-self: center;
  }
  .arrow-btn {
    display: inline-grid;
    place-items: center;
    padding: 12px 20px;
  }
  .search {
    margin-top: 18px;
    display: flex;
    gap: 18px;
    align-items: center;
  }
  .search input {
    flex: 1;
    background: none;
    border: 0;
    border-bottom: 1px solid #333;
    font-weight: 300;
    font-size: 20px;
    padding: 6px 0;
    outline: none;
    user-select: text;
  }
  .results,
  .pairs {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 16px -10px 0;
    padding: 0 10px;
  }
  .volume {
    width: 100%;
    display: flex;
    gap: 16px;
    padding: 10px;
    border-radius: 8px;
    text-align: left;
    transition: background 0.15s;
  }
  .volume:hover {
    background: #191919;
  }
  .volume.on {
    background: #222;
  }
  .volume img,
  .noimg {
    width: 54px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    border-radius: 2px;
    background: #1c1c1c;
    flex: none;
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .vname {
    font-size: 15px;
  }
  .meta {
    color: var(--text-2);
    font-size: 11px;
  }
  .deck {
    color: var(--text-3);
    font-size: 12px;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .pair {
    display: grid;
    grid-template-columns: 44px minmax(90px, 160px) 18px 30px 1fr 44px;
    gap: 14px;
    align-items: center;
    padding: 8px 0;
    border-bottom: 1px solid #1c1c1c;
  }
  .local {
    font-size: 13px;
    display: flex;
    flex-direction: column;
  }
  .section {
    color: var(--text-3);
    font-size: 11px;
  }
  .arrow {
    color: var(--text-3);
  }
  .pair img,
  .noimg.small {
    width: 30px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    border-radius: 2px;
    background: #1c1c1c;
  }
  .pair.none img,
  .pair.none .noimg {
    opacity: 0.3;
  }
  select {
    width: 100%;
    min-width: 0;
    background: #1a1a1a;
    border: 1px solid #262626;
    border-radius: 6px;
    padding: 6px 8px;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .pair.check select {
    border-color: #6a6a6a;
  }
  .state {
    color: var(--text-3);
    display: grid;
    justify-items: end;
  }
  .pair.check .state {
    color: var(--text);
  }
  .done {
    flex: 1;
    align-self: flex-start;
    margin-top: 24px;
    color: var(--text-2);
    font-size: 15px;
  }
  .empty {
    color: var(--text-2);
    padding: 20px 10px;
  }
  .wait {
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--text-3);
  }
  .wait .spin {
    display: grid;
  }
  footer {
    display: flex;
    align-items: center;
    gap: 22px;
    padding-top: 16px;
    border-top: 1px solid #1c1c1c;
    margin-top: 12px;
  }
  .spacer {
    flex: 1;
  }
  .busy {
    color: var(--text-2);
    display: grid;
  }
  .busy .spin {
    display: grid;
  }
  .err {
    color: var(--text);
    font-size: 13px;
  }
  .btn:disabled {
    opacity: 0.4;
  }
</style>
