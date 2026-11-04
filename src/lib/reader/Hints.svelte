<script lang="ts">
  // Two gentle notes, once each, ever: how to get around when you first open a book, and Guided View
  // a few pages later. Each goes when a key is pressed or after a few seconds.
  import { ACTIONS, keysFor } from "./actions";
  import type { Ctl } from "./ctl.svelte";
  import { pretty } from "./keys";

  const SEEN = "aion.readerHints";
  const SHOWN_MS = 8000;
  const PAGES_BEFORE_GUIDED = 4;

  let { c, overrides }: { c: Ctl; overrides: Record<string, string[]> } = $props();

  function load(): { around?: boolean; guided?: boolean } {
    try {
      return JSON.parse(localStorage.getItem(SEEN) ?? "{}") ?? {};
    } catch {
      return {};
    }
  }
  const seen = load();
  function mark(which: "around" | "guided") {
    seen[which] = true;
    try {
      localStorage.setItem(SEEN, JSON.stringify(seen));
    } catch {
      // Might be seen again.
    }
  }

  const key = (id: string, fallback: string) => {
    const action = ACTIONS.find((a) => a.id === id);
    const first = action ? keysFor(action, overrides)[0] : undefined;
    return first ? pretty(first) : fallback;
  };

  let note = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;
  function show(text: string) {
    clearTimeout(timer);
    note = text;
    timer = setTimeout(() => (note = ""), SHOWN_MS);
  }

  // Getting around, as soon as there's a book to read.
  $effect(() => {
    if (!c.info || seen.around || c.info.external) return;
    mark("around");
    const how = c.mode === "scroll" ? "Scroll to read" : "The arrow keys turn pages";
    show(`${how}. Press ${key("shortcuts", "?")} for every shortcut.`);
  });

  // Guided View, once you've settled in, if you haven't found it already.
  let first = -1;
  $effect(() => {
    const page = c.page;
    if (!c.info) return;
    if (c.mode === "guided") mark("guided");
    if (first < 0) first = page;
    if (seen.guided || c.info.webtoon || c.mode !== "pages" || Math.abs(page - first) < PAGES_BEFORE_GUIDED) return;
    mark("guided");
    show(`Press ${key("guided", "V")} for Guided View: it reads the page panel by panel.`);
  });

  $effect(() => () => clearTimeout(timer));
</script>

<svelte:window onkeydown={(e) => note && !["ArrowLeft", "ArrowRight", " ", "PageUp", "PageDown"].includes(e.key) && (note = "")} />

<div class="hint" class:on={!!note} role="status">{note}</div>

<style>
  .hint {
    position: absolute;
    left: 50%;
    bottom: 62px;
    z-index: 7;
    transform: translate(-50%, 6px);
    padding: 10px 16px;
    border-radius: 8px;
    background: rgba(20, 20, 20, 0.92);
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.08);
    color: #eee;
    font-size: 14px;
    white-space: nowrap;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 0.3s,
      transform 0.3s;
  }
  .hint.on {
    opacity: 1;
    transform: translate(-50%, 0);
  }
</style>
