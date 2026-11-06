<script lang="ts">
  import { api, unitWord, type CvIssue, type CvProposal, type CvVolume } from "$lib/api";
  import { lib, nav, sheets } from "$lib/state.svelte";
  import { untrack } from "svelte";
  import Cover from "./Cover.svelte";
  import Icon from "./Icon.svelte";

  // Match a series to Comic Vine in two steps: pick the series (best guess preselected), then check
  // which issue each book is. Confident pairs are filled in; only the uncertain ones need a look.
  // With `comicId`, just that one book: pick its Comic Vine series, then its issue from the list.
  let {
    seriesId,
    isManga,
    name,
    comicId,
    label,
    count,
  }: { seriesId: number; isManga: boolean; name: string; comicId?: number; label?: string; count?: number } = $props();
  const single = untrack(() => comicId != null);

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
    // One book: of the series named like the best match, the one about as long as this book's
    // format is here. A trade among 12 volumes is in the 12-issue "Saga", not the 72-issue one.
    if (single && count && chosen) {
      const name = chosen.name.toLowerCase();
      const same = r.volumes.filter((v) => v.name.toLowerCase() === name && v.count);
      chosen = same.reduce((a, v) => (Math.abs(v.count! - count) < Math.abs(a.count! - count) ? v : a), same[0] ?? chosen);
    }
  }
  // Search straight away with the starting name.
  search(untrack(() => query));

  async function next() {
    if (!chosen) return;
    const v = chosen;
    const r = await run("Loading",() => api.cvPlan(seriesId, v.id));
    if (!r) return;
    issues = r.issues;
    rows = single ? r.proposals.filter((p) => p.comicId === comicId) : r.proposals;
    // One book whose folder kept it from being paired: offer the issue with its number.
    const one = single ? rows[0] : null;
    if (one && !one.issueId) {
      const n = (label ?? one.label).match(/\d+(?:\.\d+)?/)?.[0];
      const hit = n ? issues.find((i) => i.number && Number(i.number) === Number(n)) : undefined;
      if (hit) {
        one.issueId = hit.id;
        one.confidence = "check";
      }
    }
    step = "review";
  }

  async function save() {
    if (!chosen) return;
    const v = chosen;
    const pairs = rows.map((r) => ({ comicId: r.comicId, issueId: r.issueId }));
    const n = await run("Saving", () =>
      single && comicId != null ? api.cvApplyBook(comicId, v.id, rows[0]?.issueId ?? null) : api.cvApply(seriesId, v.id, pairs),
    );
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
      <h2>{step === "review" && chosen ? chosen.name : single ? `Find ${label ?? "this book"} on Comic Vine` : "Find on Comic Vine"}</h2>
      {#if step === "review" && !single}
        <span class="sub fine">
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
              <span class="meta fine">
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
    {:else if step === "review" && single}
      {@const row = rows[0]}
      <!-- The one book, and the issues of the chosen series to say which it is. -->
      <div class="one">
        <div class="book">
          {#if row}<Cover id={row.comicId} width={64} />{/if}
          <span class="local">{label ?? row?.label}{#if row?.section}<span class="section">{row.section}</span>{/if}</span>
        </div>
        <div class="issues">
          {#each issues as i (i.id)}
            <button class="issue" class:on={row?.issueId === i.id} onclick={() => row && pick(row, row.issueId === i.id ? "" : i.id)}>
              {#if i.image}<img src={i.image} alt="" loading="lazy" />{:else}<span class="noimg small"></span>{/if}
              <span class="iname">{issueLabel(i)}</span>
              {#if row?.issueId === i.id}<Icon name="check" size={14} />{/if}
            </button>
          {:else}
            <p class="empty">This series has no issues on Comic Vine.</p>
          {/each}
        </div>
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
        <Icon name="check" size={18} />{single ? (saved ? `${label ?? "Book"} saved` : "Left unmatched") : `${saved} ${unit(saved)} saved`}
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
        {#if single}
          <button class="btn" disabled={!matched || !!busy} onclick={save}>Save</button>
        {:else}
          <!-- Saving with nothing paired is how a wrong match is undone. -->
          <button class="btn" disabled={!rows.length || !!busy} onclick={save}>Save {matched}</button>
        {/if}
      {:else}
        <button class="btn" onclick={close}>Done</button>
      {/if}
    </footer>
  </div>
</div>

<style>
  /* One book: it on the left, its series' issues to pick from on the right. */
  .one {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 150px 1fr;
    gap: 24px;
    padding: 18px 24px;
  }
  .one .book {
    display: flex;
    flex-direction: column;
    gap: 10px;
    font-size: 13px;
  }
  .issues {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .issue {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 10px;
    border-radius: 4px;
    text-align: left;
    color: var(--text-2);
    font-size: 13px;
  }
  .issue:hover {
    background: rgb(var(--fg) / 0.05);
    color: var(--text);
  }
  .issue.on {
    background: rgb(var(--fg) / 0.1);
    color: var(--text);
  }
  .issue img,
  .issue .noimg {
    width: 30px;
    height: 45px;
    object-fit: cover;
    border-radius: 2px;
    flex: none;
  }
  .iname {
    flex: 1;
  }
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
    background: var(--raised);
    box-shadow: 0 0 0 1px var(--line), 0 30px 80px rgba(0, 0, 0, 0.7);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 16px;
  }
  h2 {
    font-weight: 400;
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
    font-weight: 400;
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
    background: var(--raised);
  }
  .volume.on {
    background: var(--line);
  }
  .volume img,
  .noimg {
    width: 54px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    border-radius: 2px;
    background: var(--menu);
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
    font-size: 12px;
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
    font-size: 12px;
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
    background: var(--menu);
  }
  .pair.none img,
  .pair.none .noimg {
    opacity: 0.3;
  }
  select {
    width: 100%;
    min-width: 0;
    background: var(--menu);
    border: 1px solid var(--line);
    border-radius: 6px;
    padding: 6px 8px;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .pair.check select {
    border-color: var(--text-3);
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
