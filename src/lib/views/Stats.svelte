<script lang="ts">
  import { api, duration, type ReadingPeriod, type ReadingStats } from "$lib/api";
  import { lib, nav, openReader } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";

  // What your reading adds up to: recorded from the first sitting on, never guessed before that.
  let data = $state<ReadingStats | null>(null);

  $effect(() => {
    void lib.version;
    api.readingStats().then((d) => {
      data = d;
      nav.restore();
    });
  });

  const PERIODS: [keyof Pick<ReadingStats, "week" | "month" | "year" | "all">, string][] = [
    ["week", "This week"],
    ["month", "This month"],
    ["year", "This year"],
    ["all", "All time"],
  ];
  const finishedLine = (p: ReadingPeriod) => (p.finished ? `${p.finished} finished` : "");
  const peak = $derived(Math.max(1, ...(data?.weeks.map(([, n]) => n) ?? [1])));
  // A week's Monday as a short date: "Oct 26".
  const monday = (days: number) =>
    new Date(days * 86_400_000).toLocaleDateString(undefined, { month: "short", day: "numeric", timeZone: "UTC" });
  const since = $derived(
    data?.since ? new Date(data.since * 1000).toLocaleDateString(undefined, { dateStyle: "long" }) : null,
  );
  const top = (list: { pages: number }[]) => Math.max(1, ...list.map((x) => x.pages));
</script>

{#if data}
  <div class="page">
    <header>
      <h1>Your reading</h1>
      <div class="since mono">{since ? `Recorded since ${since}` : "Nothing recorded yet"}</div>
    </header>

    {#if !data.since}
      <p class="none">Read something and it shows up here: pages, time, streaks, and who you read most.</p>
    {:else}
      <!-- Pages read, and the time and finished books behind them. -->
      <section class="tiles">
        {#each PERIODS as [key, label] (key)}
          {@const p = data[key]}
          <div class="tile">
            <div class="label">{label}</div>
            <div class="big">{p.pages.toLocaleString()}</div>
            <div class="unit">pages</div>
            <div class="mono dim">{duration(p.seconds)}{#if finishedLine(p)}&ensp;·&ensp;{finishedLine(p)}{/if}</div>
          </div>
        {/each}
        <div class="tile">
          <div class="label">Streak</div>
          <div class="big">{data.streak}</div>
          <div class="unit">{data.streak === 1 ? "day" : "days"} in a row</div>
          <div class="mono dim">best {data.bestStreak}</div>
        </div>
      </section>

      <section>
        <h2>Pages per week</h2>
        <div class="weeks">
          {#each data.weeks as [day, pages], i (day)}
            <div class="week" title="Week of {monday(day)}: {pages.toLocaleString()} pages">
              <span class="n mono">{pages ? pages.toLocaleString() : ""}</span>
              <i style:height="{(pages / peak) * 100}%" class:now={i === data.weeks.length - 1}></i>
              <span class="d mono">{i % 3 === 0 || i === data.weeks.length - 1 ? monday(day) : ""}</span>
            </div>
          {/each}
        </div>
      </section>

      {#if data.series.length}
        <section>
          <h2>Most read</h2>
          <div class="row">
            {#each data.series as s (s.seriesId)}
              <button class="pick" onclick={() => s.seriesId && nav.go({ name: "series", id: s.seriesId })}>
                <Cover id={s.coverId} width={104} pages={null} />
                <span class="cap">{s.name}</span>
                <span class="mono dim">{s.pages.toLocaleString()} pages</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}

      <div class="lists">
        {#each [["Creators", "creator", data.creators], ["Characters", "character", data.characters]] as const as [title, kind, list] (kind)}
          {#if list.length}
            <section>
              <h2>{title}</h2>
              <ol>
                {#each list as p (p.name)}
                  <li>
                    <button onclick={() => nav.go({ name: "people", kind, value: p.name })}>
                      <span class="who">{p.name}</span>
                      <span class="bar"><i style:width="{(p.pages / top(list)) * 100}%"></i></span>
                      <span class="mono dim">{p.pages.toLocaleString()}</span>
                    </button>
                  </li>
                {/each}
              </ol>
            </section>
          {/if}
        {/each}
      </div>

      {#if data.finished.length}
        <section>
          <h2>Finished</h2>
          <div class="row">
            {#each data.finished as c (c.id)}
              <button class="pick" title="{c.seriesName.trim()} {c.label}" onclick={() => nav.go({ name: "series", id: c.seriesId, book: c.id })} ondblclick={() => openReader(c.id)}>
                <Cover id={c.id} ratio={c.coverRatio} width={84} pages={c.numPages} />
                <span class="cap">{c.seriesName.trim()}</span>
                <span class="mono dim">{c.label}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .page {
    position: relative;
    isolation: isolate;
    padding: 120px var(--pad) 140px;
  }
  h1 {
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(64px, 8vw, 116px);
    line-height: 0.88;
    letter-spacing: 0.005em;
  }
  .since {
    margin-top: 14px;
    color: var(--text-2);
  }
  .none {
    margin-top: 40px;
    color: var(--text-2);
    max-width: 56ch;
  }
  section {
    margin-top: 56px;
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
    margin-bottom: 18px;
  }
  .dim {
    color: var(--text-3);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: 14px;
  }
  .tile {
    padding: 18px 20px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.035);
    border: 1px solid rgba(255, 255, 255, 0.05);
  }
  .label {
    font-size: 13px;
    color: var(--text-2);
  }
  .big {
    margin-top: 10px;
    font-family: var(--display);
    font-size: 56px;
    line-height: 0.9;
  }
  .unit {
    margin: 2px 0 10px;
    color: var(--text-2);
    font-size: 13px;
  }
  .weeks {
    height: 180px;
    display: flex;
    gap: 10px;
    align-items: stretch;
  }
  .week {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
  }
  .week i {
    display: block;
    width: 100%;
    min-height: 2px;
    border-radius: 3px 3px 0 0;
    background: rgba(255, 255, 255, 0.18);
  }
  .week i.now {
    background: #e8e8e8;
  }
  .week .n,
  .week .d {
    font-size: 11px;
    color: var(--text-3);
    height: 14px;
  }
  .row {
    display: flex;
    gap: 22px;
    overflow-x: auto;
    padding: 8px 0 6px;
    align-items: flex-start;
  }
  .pick {
    flex: none;
    width: 120px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    text-align: left;
    gap: 2px;
  }
  .pick :global(.cover) {
    margin-bottom: 8px;
    box-shadow: 0 16px 34px -16px rgba(0, 0, 0, 0.95);
    transition: transform 0.45s var(--ease);
  }
  .pick:hover :global(.cover) {
    transform: translateY(-5px);
  }
  .cap {
    font-size: 13px;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .pick .mono {
    font-size: 11px;
  }
  .lists {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 0 56px;
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li button {
    width: 100%;
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) 56px;
    align-items: center;
    gap: 14px;
    height: 34px;
    text-align: left;
  }
  .who {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  li button:hover .who {
    color: #fff;
  }
  .bar {
    height: 2px;
    background: rgba(255, 255, 255, 0.08);
    position: relative;
  }
  .bar i {
    position: absolute;
    inset: 0 auto 0 0;
    background: rgba(255, 255, 255, 0.6);
  }
  li .mono {
    text-align: right;
    font-size: 12px;
  }
</style>
