<script lang="ts">
  import { ACTIONS, keysFor, type Action } from "./actions";
  import { chord, pretty, saveOverrides } from "./keys";

  let { overrides = $bindable(), onclose }: { overrides: Record<string, string[]>; onclose: () => void } = $props();

  let editing = $state(false);
  let listening = $state<string | null>(null);
  let notice = $state("");

  const GROUPS: Record<string, string> = {
    file: "Files",
    books: "Books",
    pages: "Pages",
    fit: "Fit",
    layout: "Layout",
    zoom: "Zoom",
    marks: "Bookmarks",
    guided: "Guided View",
    view: "View",
    app: "AionComics",
  };
  // "Back to library" sits with the app-wide keys.
  const groups = $derived(
    Object.keys(GROUPS).map((g) => ({
      name: GROUPS[g],
      actions: ACTIONS.filter((a) => a.group === g || (g === "app" && a.group === "close")),
    })),
  );

  function label(a: Action) {
    if (a.id === "left") return "Turn left (next page in manga)";
    if (a.id === "right") return "Turn right (previous page in manga)";
    return a.label;
  }

  // While listening, the next key press becomes the shortcut. Capture phase, so the reader never sees it.
  function capture(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.key === "Escape") {
      listening = null;
      return;
    }
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    const k = chord(e);
    const next = { ...overrides };
    // A key does one thing: take it away from whatever had it.
    const taken = ACTIONS.find((a) => a.id !== listening && keysFor(a, next).includes(k));
    if (taken) {
      next[taken.id] = keysFor(taken, next).filter((x) => x !== k);
      notice = `${pretty(k)} was ${taken.label.toLowerCase()}; that now has ${next[taken.id].length ? pretty(next[taken.id][0]) : "no key"}.`;
    } else notice = "";
    next[listening] = [k];
    overrides = next;
    saveOverrides(next);
    listening = null;
  }

  function reset() {
    overrides = {};
    saveOverrides({});
    notice = "All shortcuts are back to their defaults.";
  }
</script>

<svelte:window onkeydowncapture={capture} />

<div class="sheet" role="dialog" aria-label="Keyboard shortcuts">
  <header>
    <h2>Keyboard shortcuts</h2>
    <button class="link" class:on={editing} onclick={() => ((editing = !editing), (listening = null))}>
      {editing ? "Done" : "Edit"}
    </button>
    {#if editing}<button class="link" onclick={reset}>Reset all</button>{/if}
    <button class="link close" onclick={onclose}>Close</button>
  </header>
  {#if editing}
    <p class="hint">{notice || "Click one, press the new key · esc cancels"}</p>
  {/if}
  <div class="cols">
    {#each groups as g (g.name)}
      {#if g.actions.length}
        <section>
          <h3>{g.name}</h3>
          {#each g.actions as a (a.id)}
            <div class="row">
              <span>{label(a)}</span>
              {#if editing}
                <button class="key edit" class:listening={listening === a.id} onclick={() => (listening = a.id)}>
                  {listening === a.id ? "Press a key" : keysFor(a, overrides).map(pretty).join(", ") || "None"}
                </button>
              {:else}
                <span class="key">{keysFor(a, overrides).map(pretty).join(", ") || "—"}</span>
              {/if}
            </div>
          {/each}
        </section>
      {/if}
    {/each}
  </div>
</div>

<style>
  .sheet {
    position: absolute;
    inset: 0;
    z-index: 8;
    overflow-y: auto;
    padding: 72px 72px 56px;
    background: rgba(6, 6, 6, 0.9);
    backdrop-filter: blur(24px);
    animation: fade 0.25s var(--ease);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  header {
    display: flex;
    align-items: baseline;
    gap: 28px;
  }
  h2 {
    font-weight: 400;
    font-size: 40px;
    letter-spacing: -0.04em;
    margin-right: 12px;
  }
  .close {
    margin-left: auto;
  }
  .hint {
    margin-top: 16px;
    color: var(--text-2);
  }
  .cols {
    margin-top: 44px;
    columns: 3 300px;
    column-gap: 64px;
  }
  section {
    break-inside: avoid;
    margin-bottom: 36px;
  }
  h3 {
    font-weight: 600;
    font-size: 13px;
    color: var(--text-2);
    margin-bottom: 10px;
  }
  .row {
    display: flex;
    justify-content: space-between;
    gap: 16px;
    padding: 5px 0;
    color: var(--cap);
  }
  .key {
    font-family: var(--mono);
    font-size: 12px;
    color: var(--text);
    white-space: nowrap;
  }
  .key.edit {
    padding: 2px 8px;
    margin: -2px -8px;
    border-radius: 4px;
    background: #1c1c1c;
  }
  .key.edit:hover {
    background: #262626;
  }
  .key.listening {
    background: var(--text);
    color: var(--bg);
  }
</style>
