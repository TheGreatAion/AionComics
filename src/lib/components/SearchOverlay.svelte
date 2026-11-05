<script lang="ts">
  import { api, storyTitle, unitWord } from "$lib/api";
  import { nav, search } from "$lib/state.svelte";
  import Cover from "./Cover.svelte";

  // One list of whatever matched: series, then books, then who and what's in them.
  type Hit = { key: string; cover: number | null; ratio: number | null; name: string; detail: string; go: () => void };

  let q = $state("");
  let hits = $state<Hit[]>([]);
  let sel = $state(0);
  let input = $state<HTMLInputElement>();
  let list = $state<HTMLUListElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (!search.open) return;
    q = "";
    hits = [];
    input?.focus();
  });

  const count = (n: number) => `${n.toLocaleString()} ${n === 1 ? "book" : "books"}`;

  $effect(() => {
    const query = q.trim();
    clearTimeout(timer);
    if (!query) {
      hits = [];
      return;
    }
    timer = setTimeout(() => {
      api.search(query).then((r) => {
        if (q.trim() !== query) return;
        hits = [
          ...r.series.map((s) => ({
            key: `s${s.id}`,
            cover: s.coverId,
            ratio: s.coverRatio,
            name: s.name,
            detail: `${s.count} ${unitWord(s.isManga, s.count)}`,
            go: () => nav.go({ name: "series", id: s.id }),
          })),
          ...r.books.map((b) => ({
            key: `b${b.id}`,
            cover: b.id,
            ratio: b.coverRatio,
            name: storyTitle(b) ?? `${b.seriesName.trim()} ${b.label}`,
            detail: storyTitle(b) ? `${b.seriesName.trim()} ${b.label}` : "Book",
            go: () => nav.go({ name: "series", id: b.seriesId, book: b.id }),
          })),
          ...(
            [
              ["creator", "Creator", r.creators],
              ["character", "Character", r.characters],
              ["arc", "Story arc", r.arcs],
            ] as const
          ).flatMap(([kind, word, names]) =>
            names.map((n) => ({
              key: `${kind}:${n.name}`,
              cover: n.coverId,
              ratio: null,
              name: n.name,
              detail: `${word} · ${count(n.books)}`,
              go: () => nav.go({ name: "people", kind, value: n.name }),
            })),
          ),
        ];
        sel = 0;
      });
    }, 120);
  });

  function pick(h: Hit | undefined) {
    if (!h) return;
    search.open = false;
    h.go();
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") search.open = false;
    else if (e.key === "Enter") pick(hits[sel]);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, Math.min(hits.length - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
      list?.children[sel]?.scrollIntoView({ block: "nearest" });
    }
  }
</script>

{#if search.open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (search.open = false)}>
    <div class="box">
      <input bind:this={input} bind:value={q} onkeydown={onkey} placeholder="Series, book, creator, character, arc" spellcheck="false" />
      <ul bind:this={list}>
        {#each hits as h, i (h.key)}
          <li>
            <button class:on={i === sel} onmouseenter={() => (sel = i)} onclick={() => pick(h)}>
              <Cover id={h.cover} ratio={h.ratio} width={34} />
              <span class="name">{h.name}</span>
              <span class="mono">{h.detail}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if q.trim() && !hits.length}
        <p class="hint mono">Nothing yet</p>
      {:else}
        <p class="hint mono">↑↓&emsp;↵&emsp;esc</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgba(6, 6, 6, 0.78);
    backdrop-filter: blur(28px);
    padding: 14vh var(--pad) 0;
    animation: fade 0.25s var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .box {
    max-width: 680px;
    margin: 0 auto;
  }
  input {
    width: 100%;
    background: none;
    border: 0;
    border-bottom: 1px solid #333;
    font-weight: 300;
    font-size: 34px;
    line-height: 1.2;
    letter-spacing: -0.03em;
    padding: 0 0 16px;
    outline: none;
    user-select: text;
  }
  input::placeholder {
    color: var(--text-3);
  }
  ul {
    list-style: none;
    margin: 18px 0 0;
    padding: 0 12px;
    margin-inline: -12px;
    max-height: calc(86vh - 150px);
    overflow-y: auto;
    scrollbar-width: thin;
  }
  li button {
    width: calc(100% + 24px);
    margin: 0 -12px;
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 10px 12px;
    border-radius: 6px;
    text-align: left;
  }
  li button.on {
    background: var(--menu);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    flex: none;
    color: var(--text-3);
    margin-left: auto;
  }
  .hint {
    margin-top: 20px;
    color: var(--text-3);
  }
</style>
