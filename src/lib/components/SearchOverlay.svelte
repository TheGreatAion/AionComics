<script lang="ts">
  import { api, unitWord, type SeriesSummary } from "$lib/api";
  import { nav, search } from "$lib/state.svelte";
  import Cover from "./Cover.svelte";

  let q = $state("");
  let hits = $state<SeriesSummary[]>([]);
  let sel = $state(0);
  let input = $state<HTMLInputElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    if (!search.open) return;
    q = "";
    hits = [];
    input?.focus();
  });

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
        hits = r.slice(0, 8);
        sel = 0;
      });
    }, 120);
  });

  function pick(s: SeriesSummary | undefined) {
    if (!s) return;
    search.open = false;
    nav.go({ name: "series", id: s.id });
  }

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") search.open = false;
    else if (e.key === "Enter") pick(hits[sel]);
    else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, Math.min(hits.length - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
    }
  }
</script>

{#if search.open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (search.open = false)}>
    <div class="box">
      <input bind:this={input} bind:value={q} onkeydown={onkey} placeholder="Series, writer" spellcheck="false" />
      <ul>
        {#each hits as s, i (s.id)}
          <li>
            <button class:on={i === sel} onmouseenter={() => (sel = i)} onclick={() => pick(s)}>
              <Cover id={s.coverId} ratio={s.coverRatio} width={34} />
              <span class="name">{s.name}</span>
              <span class="mono">{s.count} {unitWord(s.isManga, s.count)}</span>
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
    padding: 18vh var(--pad) 0;
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
    color: #3e3e3e;
  }
  ul {
    list-style: none;
    margin: 18px 0 0;
    padding: 0;
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
  .mono {
    color: var(--text-3);
    margin-left: auto;
  }
  .hint {
    margin-top: 20px;
    color: #3e3e3e;
  }
</style>
