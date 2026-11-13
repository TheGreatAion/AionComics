<script lang="ts">
  import { api, paragraphs, shortDate, unitWord, type CvIssue, type CvProposal, type CvVolume } from "$lib/api";
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

  // ---- the picked series, described beside the list ----

  // Each series' issues, asked for once it's been picked for a moment (the same listing matching
  // uses, so picking it after costs nothing).
  let looked = $state<Record<string, CvIssue[] | "wait" | "none">>({});
  $effect(() => {
    const v = chosen;
    if (!v || step !== "search" || looked[v.id]) return;
    const id = v.id;
    const t = setTimeout(() => {
      looked[id] = "wait";
      api.cvVolumeIssues(id).then((l) => (looked[id] = l)).catch(() => (looked[id] = "none"));
    }, 350);
    return () => clearTimeout(t);
  });
  const shown = $derived(chosen ? looked[chosen.id] : undefined);
  const chosenIssues = $derived(Array.isArray(shown) ? shown : []);

  // Narrowing what came back, by name, year or publisher, without asking Comic Vine again.
  let narrow = $state("");
  const kept = $derived.by(() => {
    const words = narrow.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return volumes;
    return volumes.filter((v) => {
      const hay = [v.name, v.startYear, v.publisher, v.language].filter(Boolean).join(" ").toLowerCase();
      return words.every((w) => hay.includes(w));
    });
  });
  // English editions first; the same series from other countries after them, set apart.
  const home = $derived(kept.filter((v) => !v.language));
  const away = $derived(kept.filter((v) => v.language));

  /** "Spanish publication." says no more than the tag: not a summary. */
  const said = (t: string) => /^\s*[a-z]+\s+(language\s+)?(publication|edition|translation|reprint|version)\.?\s*$/i.test(t);
  /** A line or two about a series for the list: its one-line summary, else its description's first paragraph. */
  function summary(v: CvVolume) {
    if (v.deck?.trim()) return v.deck.trim();
    const p = paragraphs(v.description, 1)[0];
    return p && !said(p) ? p : "";
  }
  /** Comic Vine's bigger picture of the same cover. */
  const bigger = (url: string | null) => url?.replace(/\/uploads\/[a-z_]+\//, "/uploads/scale_medium/") ?? null;
  const mark = (n: string) => (isManga ? `Vol. ${n}` : `#${n}`);
  /** "1 · Episode One" as a number and a title. */
  const split = (line: string | null) => {
    if (!line) return null;
    const [n, ...rest] = line.split(" · ");
    return { n: mark(n), title: rest.join(" · ") };
  };
  const about = $derived(chosen ? paragraphs(chosen.description, 3).filter((p) => !said(p)) : []);
  const sale = $derived.by(() => {
    const dates = chosenIssues.map((i) => i.storeDate ?? i.coverDate).filter((d): d is string => !!d).sort();
    if (!dates.length) return "";
    const [a, b] = [dates[0], dates[dates.length - 1]];
    return a === b ? `On sale ${shortDate(a)}` : `On sale ${shortDate(a)} to ${shortDate(b)}`;
  });
  const fromFirst = $derived(chosenIssues.length && !about.length ? paragraphs(chosenIssues[0].description, 1)[0] ?? "" : "");

  // ---- one book: the issue looked at, described beside the list ----
  let lookAt = $state<string | null>(null);
  const seen = $derived(byId.get(lookAt ?? rows[0]?.issueId ?? "") ?? null);

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
  <div class="sheet" class:wide={step === "search" || (step === "review" && single)} role="dialog" aria-label="Find on Comic Vine">
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
      {#snippet volume(v: CvVolume)}
        {@const line = summary(v)}
        <button class="volume" class:on={chosen?.id === v.id} class:away={!!v.language} onclick={() => (chosen = v)} ondblclick={next}>
          {#if v.image}<img src={v.image} alt="" loading="lazy" />{:else}<span class="noimg"></span>{/if}
          <span class="info">
            <span class="vname">{v.name}</span>
            <span class="meta fine">
              {[v.startYear, v.publisher, v.count ? `${v.count} ${unit(v.count)}` : null].filter(Boolean).join(" · ")}
              {#if v.language}<span class="lang">{v.language}</span>{/if}
            </span>
            {#if line}<span class="deck">{line}</span>{/if}
          </span>
        </button>
      {/snippet}
      <div class="split">
        <div class="results">
          {#if volumes.length > 6}
            <input class="narrow" bind:value={narrow} placeholder="Filter by name, year or publisher" spellcheck="false" />
          {/if}
          {#each home as v (v.id)}{@render volume(v)}{/each}
          {#if away.length}
            <div class="group fine">Other countries · {away.length}</div>
            {#each away as v (v.id)}{@render volume(v)}{/each}
          {/if}
          {#if volumes.length && !kept.length}<p class="empty">None of these match “{narrow}”.</p>{/if}
          {#if !volumes.length}
            {#if busy}
              <div class="wait" title={busy}><span class="spin"><Icon name="rescan" size={22} stroke={1.2} /></span></div>
            {:else if !error}<p class="empty">Nothing found. Try a shorter name.</p>{/if}
          {/if}
        </div>
        <!-- Everything about the series picked. -->
        <div class="detail">
          {#if chosen}
            {@const first = split(chosen.firstIssue)}
            {@const last = split(chosen.lastIssue)}
            {#if chosen.image}<img class="big" src={bigger(chosen.image)} alt="" />{/if}
            <h3>{chosen.name}</h3>
            <div class="meta">
              {[chosen.startYear, chosen.publisher, chosen.count ? `${chosen.count} ${unit(chosen.count)}` : null].filter(Boolean).join(" · ")}
              {#if chosen.language}<span class="lang">{chosen.language}</span>{/if}
            </div>
            {#if first || sale}
              <div class="runs">
                {#if first}<div><b>{first.n}</b> {first.title}</div>{/if}
                {#if last && chosen.lastIssue !== chosen.firstIssue}<div><b>{last.n}</b> {last.title}</div>{/if}
                {#if sale}<div>{sale}</div>{/if}
              </div>
            {/if}
            {#each about as p, k (k)}<p>{p}</p>{/each}
            {#if fromFirst}
              <div class="lbl fine">From {mark(chosenIssues[0].number ?? "1")}</div>
              <p class="from">{fromFirst}</p>
            {/if}
            {#if chosenIssues.length > 1}
              <div class="lbl fine">First {unit(2)}</div>
              <div class="strip">
                {#each chosenIssues.slice(0, 6) as i (i.id)}
                  {#if i.image}<img src={i.image} alt="" title={i.number ? mark(i.number) : ""} loading="lazy" />{/if}
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      </div>
    {:else if step === "review" && single}
      {@const row = rows[0]}
      <!-- The one book, and the issues of the chosen series to say which it is. -->
      <div class="split">
        <div class="issues">
          <div class="book">
            {#if row}<Cover id={row.comicId} width={40} />{/if}
            <span class="local">{label ?? row?.label}{#if row?.section}<span class="section">{row.section}</span>{/if}<span class="fine dim">Your book</span></span>
          </div>
          {#each issues as i (i.id)}
            <button
              class="issue"
              class:on={row?.issueId === i.id}
              onclick={() => {
                lookAt = i.id;
                if (row) pick(row, row.issueId === i.id ? "" : i.id);
              }}
              onmouseenter={() => (lookAt = i.id)}
            >
              {#if i.image}<img src={i.image} alt="" loading="lazy" />{:else}<span class="noimg small"></span>{/if}
              <span class="iname">
                <span>{[i.number ? mark(i.number) : null, i.name].filter(Boolean).join(" ")}</span>
                {#if i.storeDate || i.coverDate}<span class="fine dim">{i.storeDate ? `On sale ${shortDate(i.storeDate)}` : shortDate(i.coverDate)}</span>{/if}
              </span>
              {#if row?.issueId === i.id}<Icon name="check" size={14} />{/if}
            </button>
          {:else}
            <p class="empty">This series has no issues on Comic Vine.</p>
          {/each}
        </div>
        <div class="detail">
          {#if seen}
            {#if seen.image}<img class="big" src={bigger(seen.image)} alt="" />{/if}
            <h3>{[seen.number ? mark(seen.number) : null, seen.name].filter(Boolean).join(" ")}</h3>
            <div class="meta">
              {[seen.storeDate ? `On sale ${shortDate(seen.storeDate)}` : null, seen.coverDate ? `cover date ${new Date(seen.coverDate.slice(0, 10) + "T12:00").toLocaleDateString([], { month: "long", year: "numeric" })}` : null].filter(Boolean).join(" · ")}
            </div>
            {#each paragraphs(seen.description, 4) as p, k (k)}<p>{p}</p>{/each}
          {/if}
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
  /* The list, and what's picked in it described beside it. */
  .split {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 440px) minmax(0, 1fr);
    gap: 30px;
    margin-top: 16px;
  }
  .split .results,
  .split .issues {
    margin: 0 -10px 0 -10px;
    overflow-y: auto;
  }
  .narrow {
    width: calc(100% - 20px);
    margin: 0 10px 8px;
    background: none;
    border: 0;
    border-bottom: 1px solid var(--line);
    padding: 6px 0;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    outline: none;
    user-select: text;
  }
  .detail {
    overflow-y: auto;
    padding-right: 8px;
    min-width: 0;
  }
  .detail .big {
    width: 140px;
    border-radius: 3px;
    display: block;
    box-shadow: 0 0 0 1px var(--line);
  }
  .detail h3 {
    margin: 14px 0 4px;
    font-size: 20px;
    font-weight: 600;
    color: var(--text);
  }
  .detail .meta {
    color: var(--text-2);
    font-size: 13px;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .runs {
    margin-top: 10px;
    color: var(--text-2);
    font-size: 13px;
    line-height: 1.6;
  }
  .runs b {
    color: var(--text);
    font-weight: 500;
  }
  .detail p {
    margin-top: 12px;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.55;
    user-select: text;
  }
  .detail p.from {
    margin-top: 4px;
  }
  .lbl {
    margin-top: 16px;
    color: var(--text-3);
  }
  .strip {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }
  .strip img {
    width: 52px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    border-radius: 2px;
    flex: none;
  }
  /* An edition in another language: tagged, set apart at the end, and quieter. */
  .lang {
    display: inline-block;
    margin-left: 6px;
    padding: 0 8px;
    border: 1px solid rgb(var(--fg) / 0.22);
    border-radius: 999px;
    color: var(--text-2);
    font-size: 12px;
    line-height: 18px;
  }
  .detail .meta .lang {
    margin-left: 0;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 16px 10px 6px;
    color: var(--text-3);
  }
  .group::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--line);
  }
  .volume.away .vname,
  .volume.away img {
    opacity: 0.6;
  }
  .split .book {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 10px 12px;
    margin-bottom: 6px;
    border-bottom: 1px solid var(--line);
  }
  .iname {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .dim {
    color: var(--text-3);
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
  .sheet.wide {
    width: min(1180px, calc(100vw - 48px));
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
