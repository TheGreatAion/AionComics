<script lang="ts">
  import { tick } from "svelte";
  import { api, coverUrl, storyTitle, type Panel } from "$lib/api";
  import Icon, { type IconName } from "$lib/components/Icon.svelte";
  import WindowControls from "$lib/components/WindowControls.svelte";
  import { fly, FLIGHT_MS, visible } from "$lib/flight";
  import { menu, reader, showMenu } from "$lib/state.svelte";
  import { ACTIONS, keysFor, menuItems } from "./actions";
  import Bookmarks from "./Bookmarks.svelte";
  import { Ctl, FIT_LABEL, type Step } from "./ctl.svelte";
  import Extract from "./Extract.svelte";
  import Flow from "./Flow.svelte";
  import GoTo from "./GoTo.svelte";
  import GuidedView from "./GuidedView.svelte";
  import { chord, loadOverrides, pretty } from "./keys";
  import Options from "./Options.svelte";
  import PageView from "./PageView.svelte";
  import PanelEditor from "./PanelEditor.svelte";
  import { prefs } from "./prefs.svelte";
  import ScrollView from "./ScrollView.svelte";
  import Shortcuts from "./Shortcuts.svelte";
  import Ticks from "./Ticks.svelte";

  let { id, onclose, onopen }: { id: number; onclose: () => void; onopen: (id: number) => void } = $props();

  const c = new Ctl({ open: (n) => onopen(n), close: () => close() });
  let overrides = $state(loadOverrides());
  let pageImg = $state<HTMLImageElement>();
  let chrome = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let clock = $state(new Date());
  /** Bumped after panel edits so Guided View refetches. */
  let panelsRev = $state(0);

  // The cover (or page) the reader grew out of. It flies in onto the first page and back out on close.
  let arriving = $state(!!reader.from && !prefs.scroll && visible(reader.from.getBoundingClientRect()));
  let leaving = $state(false);

  // Never leave a blank screen: if the first page is slow, skip the flight and bring the controls back.
  $effect(() => {
    if (!arriving) return;
    const t = setTimeout(() => (arriving = false), 1500);
    return () => clearTimeout(t);
  });

  $effect(() => {
    const book = id;
    c.error = null;
    c.steps = null;
    c.overlay = null;
    c.rotation = 0;
    api
      .openComic(book)
      .then((r) => book === id && c.load(r))
      .catch((e) => (c.error = String(e)));
  });

  // Guided View stops for the page or spread on screen, and the next spread's ahead of time.
  $effect(() => {
    void panelsRev;
    if (!c.info || c.mode !== "guided") return;
    const [book, pages, rtl] = [c.id, c.shown, c.rtl];
    const key = pages.join("-");
    c.steps = null;
    Promise.all(pages.map((p) => api.pagePanels(book, p, rtl).catch(() => [] as Panel[]))).then((lists) => {
      if (book !== c.id || key !== c.shown.join("-")) return;
      const steps: Step[] = [];
      lists.forEach((list, k) => {
        if (list.length) for (const panel of list) steps.push({ page: pages[k], panel });
        // In a spread, a page with no panels (splash art) still gets a stop of its own.
        else if (pages.length > 1) steps.push({ page: pages[k], panel: null });
      });
      c.steps = steps;
      const at = c.zoomAt;
      if (at && pages.includes(at.page)) c.step = c.stopAt(at.page, at.x, at.y);
      else if (c.landOnLast) c.step = steps.length - 1;
      else if (!prefs.guidedOverview && steps.length && c.step < 0) c.step = 0;
      c.zoomAt = null;
      c.landOnLast = false;
      for (const n of [pages[pages.length - 1] + 1, pages[pages.length - 1] + 2]) {
        if (n <= c.last) api.pagePanels(book, n, rtl).catch(() => {});
      }
    });
  });

  // Save progress shortly after the page settles. Opening a book isn't reading it: nothing is saved
  // until the page moves from where the book opened, so flipping through books leaves them as they were.
  let pending: [number, number, number] | null = null;
  let movedIn = 0;
  function flush() {
    clearTimeout(saveTimer);
    if (!pending) return Promise.resolve();
    const [book, p, count] = pending;
    pending = null;
    return api.saveProgress(book, p, count);
  }
  $effect(() => {
    if (!c.info) return;
    const [book, p, count] = [c.id, c.furthest, c.count];
    // A turn just before switching books still counts.
    if (pending && pending[0] !== book) flush();
    if (movedIn !== book) {
      if (c.page === c.openedAt) return;
      movedIn = book;
    }
    pending = [book, p, count];
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flush, 400);
  });

  $effect(() => {
    const t = setInterval(() => (clock = new Date()), 15000);
    return () => clearInterval(t);
  });

  // Wherever the reader doesn't quite reach (a window edge mid-resize), the page background shows:
  // make that the reader's too.
  const readerBg = $derived(prefs.background === "grey" ? "#1b1b1b" : "#000");
  $effect(() => {
    document.documentElement.style.background = readerBg;
    return () => (document.documentElement.style.background = "");
  });

  function arrive(img: HTMLImageElement) {
    pageImg = img;
    const from = reader.from;
    if (!arriving || !from) return (arriving = false);
    fly(from.currentSrc || from.src, from.getBoundingClientRect(), img.getBoundingClientRect(), {
      hide: [from, img],
      done: () => (arriving = false),
    });
  }

  function close() {
    if (leaving) return;
    if (c.fullscreen) c.setFullscreen(false);
    if (pending && pending[0] !== c.id) flush();
    if (c.info && movedIn === c.id) pending = [c.id, c.furthest, c.count];
    const save = flush();
    const to = reader.from;
    // Shrink back only when the page is shown plainly; from a zoomed or turned page it would jump.
    const plain =
      c.zoom === 1 && c.rotation === 0 && (c.mode === "pages" || (c.mode === "guided" && c.step === -1));
    if (to?.isConnected && pageImg?.isConnected && plain && visible(to.getBoundingClientRect())) {
      leaving = true;
      fly(pageImg.currentSrc || pageImg.src, pageImg.getBoundingClientRect(), to.getBoundingClientRect(), {
        hide: [pageImg, to],
        done: () => save.finally(onclose),
      });
    } else {
      save.finally(onclose);
    }
  }

  function onkey(e: KeyboardEvent) {
    if (menu.open || leaving) return;
    if (e.key === "Escape" && c.overlay) {
      e.preventDefault();
      c.overlay = null;
      return;
    }
    // Typing a page number isn't turning pages.
    if (e.target === pageBox) return;
    if (e.key === "Escape" && c.endCard) {
      c.endCard = false;
      return;
    }
    // A held arrow key stops at the end card; going on to the next book takes a fresh press.
    if (c.endCard && e.repeat) {
      e.preventDefault();
      return;
    }
    // Esc leaves full screen before it leaves the book.
    if (e.key === "Escape" && c.fullscreen) {
      e.preventDefault();
      c.setFullscreen(false);
      return;
    }
    // The shortcuts sheet and the panel editor take the keyboard while open.
    if (c.overlay === "shortcuts" || c.overlay === "editor") return;
    const k = chord(e);
    const a = ACTIONS.find((x) => keysFor(x, overrides).includes(k));
    if (!a || (a.enabled && !a.enabled(c))) return;
    e.preventDefault();
    a.run(c);
    // Turning pages is reading: it puts the bars away rather than calling them up.
    if (PAGE_TURNS.has(a.id)) rest();
    else wake();
  }
  const PAGE_TURNS = new Set(["prev", "next", "left", "right", "first", "last", "prevComic", "nextComic"]);

  // The bars fade after a few idle seconds, but never from under the pointer: resting on the toolbar
  // or the page line keeps them, and leaving them starts the full wait again. Leaving the window
  // puts them away almost at once.
  const IDLE_MS = 3000;
  const AWAY_MS = 250;
  let overBars = false;
  function hideIn(ms: number) {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (!c.overlay && !menu.open && !overBars && !editing) chrome = false;
    }, ms);
  }

  // The page box in the toolbar: shows where you are, and takes a page number to go to.
  let pageBox = $state<HTMLInputElement>();
  let editing = $state(false);
  let draft = $state("");
  const pageText = $derived.by(() => {
    const s = c.shown;
    return s.length === 2 ? `${s[0] + 1}–${s[1] + 1}` : `${s[0] + 1}`;
  });
  function pageKey(e: KeyboardEvent) {
    if (e.key === "Enter") {
      const n = parseInt(draft, 10);
      if (n >= 1) c.goTo(Math.min(n, c.count) - 1);
      pageBox?.blur();
    } else if (e.key === "Escape") {
      pageBox?.blur();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }
  function wake() {
    chrome = true;
    hideIn(IDLE_MS);
  }
  function bars(over: boolean) {
    overBars = over;
    wake();
  }
  function rest() {
    if (!overBars) hideIn(0);
  }

  $effect(() => {
    const html = document.documentElement;
    const away = () => {
      overBars = false;
      hideIn(AWAY_MS);
    };
    html.addEventListener("mouseleave", away);
    return () => html.removeEventListener("mouseleave", away);
  });

  function zone(z: "left" | "middle" | "right") {
    if (c.overlay && c.overlay !== "flow") {
      c.overlay = null;
      return;
    }
    if (z === "left") c.left();
    else if (z === "right") c.right();
    else if (chrome) chrome = false;
    else wake();
  }

  const openMenu = (e: MouseEvent) => showMenu(e, menuItems(c, overrides));

  $effect(() => {
    wake();
    return () => clearTimeout(hideTimer);
  });

  const count = $derived.by(() => {
    if (!c.info) return "";
    const s = c.shown;
    const pages = s.length === 2 ? `${s[0] + 1}–${s[1] + 1}` : `${s[0] + 1}`;
    return `${pages} / ${c.count}`;
  });
  const time = $derived(clock.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
  // Where Guided View is. On a page it found no panels on (past the cover), say how to draw them.
  const panel = $derived.by(() => {
    if (c.mode !== "guided" || !c.steps) return "";
    if (c.steps.length && c.step >= 0) return `Panel ${c.step + 1} / ${c.steps.length}`;
    if (!c.steps.length && c.page > 0 && c.info?.external === false) {
      const k = keysFor(byId.editPanels, overrides)[0];
      return k ? `No panels found · ${pretty(k)} to draw them` : "No panels found";
    }
    return "";
  });
  const byId = Object.fromEntries(ACTIONS.map((a) => [a.id, a]));
  const FIT_ICON = { page: "fitPage", height: "fitHeight", width: "fitWidth" } as const;
  const tip = (id: string) => {
    const a = byId[id];
    const k = keysFor(a, overrides)[0];
    return k ? `${a.label} (${pretty(k)})` : a.label;
  };
  const showChrome = $derived((chrome || c.endCard || !!c.overlay) && !arriving && !leaving);
</script>

<svelte:window onkeydown={onkey} />

<div
  class="reader"
  class:hide-cursor={!chrome && !c.magnifier}
  class:leaving
  style:--fade="{FLIGHT_MS}ms"
  style:--reader-bg={readerBg}
  onmousemove={(e) => bars(!!(e.target as Element).closest?.(".toolbar.shown, .bottom.shown"))}
  oncontextmenu={openMenu}
  role="presentation"
>
  {#if c.error}
    <div class="center msg">
      <p>Couldn’t open this book.</p>
      <p class="sub">{c.error}</p>
      <button class="btn" onclick={onclose}>Back to library</button>
    </div>
  {:else if c.info}
    {#key c.id}
      {#if c.mode === "guided"}
        <GuidedView {c} onzone={zone} onready={arrive} bind:img={pageImg} />
      {:else if c.mode === "scroll"}
        <ScrollView {c} onzone={zone} />
      {:else}
        <PageView {c} onzone={zone} onready={arrive} bind:img={pageImg} />
      {/if}
    {/key}

    {#if c.endCard}
      {@const nextTitle = c.info.next ? storyTitle(c.info.next) : null}
      <div class="end" role="dialog">
        <div class="end-card">
          <p class="done">You finished {[c.info.comic.seriesName.trim(), c.info.comic.label].filter(Boolean).join(" ")}.</p>
          {#if c.info.next}
            <button class="next" onclick={() => onopen(c.info!.next!.id)}>
              <img src={coverUrl(c.info.next.id)} alt="" />
              <span class="nl">{c.info.next.label}</span>
              {#if nextTitle}<span class="nt">{nextTitle}</span>{/if}
            </button>
            <div class="end-actions">
              <button class="btn icon-btn" onclick={() => onopen(c.info!.next!.id)}><Icon name="play" size={14} />{c.info.next.label}</button>
              <button class="ico" title="Back to library" aria-label="Back to library" onclick={close}><Icon name="back" size={18} /></button>
            </div>
          {:else}
            <div class="end-actions">
              <button class="btn icon-btn" onclick={close}><Icon name="back" size={15} />Library</button>
              <button class="link" onclick={() => (c.endCard = false)}>Stay</button>
            </div>
          {/if}
        </div>
      </div>
    {/if}

    <!-- The page number stays on screen after everything else has faded: always in full screen,
         and otherwise when asked for (I, or Options). -->
    {#if (prefs.info || c.fullscreen) && !showChrome}
      <div class="pin mono">{#if panel}{panel}&ensp;·&ensp;{/if}{count}&ensp;·&ensp;{time}</div>
    {/if}

    {#snippet tool(id: string, icon: IconName | null, text = "")}
      {@const a = byId[id]}
      <button
        class="tool"
        class:on={a.checked?.(c)}
        disabled={a.enabled ? !a.enabled(c) : false}
        title={tip(id)}
        aria-label={a.label}
        onclick={() => (a.run(c), wake())}
      >
        {#if icon}<Icon name={icon} />{:else}<span class="txt mono">{text}</span>{/if}
      </button>
    {/snippet}

    <!-- The top strip drags the window; the tools float in one pill in the middle of it. -->
    <header
      class="toolbar"
      class:shown={showChrome}
      data-tauri-drag-region
      role="toolbar"
      tabindex="-1"
      onpointerenter={() => bars(true)}
      onpointerleave={() => bars(false)}
    >
      <div class="pill" data-tauri-drag-region>
        <button class="tool back" onclick={close} title="Back to library (Esc)" aria-label="Back to library">
          <Icon name="back" />
        </button>
        <div class="title" data-tauri-drag-region>
          <span>{c.info.comic.seriesName.trim()}</span>
          {#if c.info.comic.label}<span class="dim">{c.info.comic.label}</span>{/if}
          {#if c.rtl}<span class="dim rtl" title="Reads right to left"><Icon name="readRtl" size={14} /></span>{/if}
        </div>
        <i class="sep"></i>
        <!-- The handful used while reading; everything else is in ⋯ (or right-click) and on the keyboard. -->
        <div class="tools">
          {@render tool("flow", "strip")}
          <button
            class="tool"
            title="{FIT_LABEL[prefs.fit]} (click to change)"
            aria-label="Fit: {FIT_LABEL[prefs.fit]}"
            onclick={() => (c.cycleFit(), wake())}
          >
            {#if prefs.fit === "full"}<span class="txt mono">1:1</span>{:else}<Icon name={FIT_ICON[prefs.fit]} />{/if}
          </button>
          {@render tool("double", "double")}
          {@render tool("guided", "guided")}
          {@render tool("bookmark", "bookmark")}
          {@render tool("options", "options")}
          <button class="tool" onclick={openMenu} title="All tools (or right-click the page)" aria-label="All tools">
            <Icon name="more" />
          </button>
        </div>
        <i class="sep"></i>
        <!-- Type a page number and press Enter to go there. -->
        <label class="status mono" title="Type a page and press Enter">
          <input
            class="pageno"
            bind:this={pageBox}
            value={editing ? draft : pageText}
            inputmode="numeric"
            spellcheck="false"
            aria-label="Page number"
            onfocus={(e) => {
              const box = e.currentTarget;
              editing = true;
              draft = String(c.shown[0] + 1);
              tick().then(() => box.select());
            }}
            oninput={(e) => (draft = e.currentTarget.value.replace(/\D/g, ""))}
            onkeydown={pageKey}
            onblur={() => ((editing = false), wake())}
          />
          <span class="dim">/ {c.count}</span>
          <span class="dim clock">{time}</span>
        </label>
        <button
          class="tool"
          class:on={c.fullscreen}
          onclick={() => c.setFullscreen(!c.fullscreen)}
          title={tip("fullscreen")}
          aria-label="Full screen"
        >
          <Icon name={c.fullscreen ? "exitFullscreen" : "fullscreen"} />
        </button>
      </div>
      {#if !c.fullscreen}<div class="win"><WindowControls /></div>{/if}
    </header>

    <div class="bottom" class:shown={showChrome} onpointerenter={() => bars(true)} onpointerleave={() => bars(false)} role="presentation">
      <Ticks {c} />
      {#if panel}<span class="panel mono">{panel}</span>{/if}
    </div>

    {#if c.overlay === "goto"}<GoTo {c} />{/if}
    {#if c.overlay === "flow"}<Flow {c} />{/if}
    {#if c.overlay === "bookmarks"}<Bookmarks {c} />{/if}
    {#if c.overlay === "options"}<Options {c} />{/if}
    {#if c.overlay === "extract"}<Extract {c} />{/if}
    {#if c.overlay === "shortcuts"}<Shortcuts bind:overrides onclose={() => (c.overlay = null)} />{/if}
    {#if c.overlay === "editor"}
      <PanelEditor {c} onsaved={() => panelsRev++} />
    {/if}

    <div class="note" class:on={!!c.note}>{c.note}</div>
  {:else if !arriving}
    <div class="center spinner"></div>
  {/if}
</div>

<style>
  .reader {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: var(--reader-bg);
    color: var(--text);
    --muted: #6a6a6a;
    /* The library dims away behind the page as it flies in, and returns as it flies back. */
    animation: dim var(--fade) ease-out;
    transition: background var(--fade) ease-in;
  }
  @keyframes dim {
    from {
      background: rgba(0, 0, 0, 0);
    }
  }
  .reader.leaving {
    background: rgba(0, 0, 0, 0);
  }
  .reader.hide-cursor {
    cursor: none;
  }

  .center {
    position: absolute;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
  }
  .msg {
    text-align: center;
  }
  .msg .sub {
    color: var(--muted);
    margin: 8px 0 24px;
  }
  .spinner {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.15);
    border-top-color: rgba(255, 255, 255, 0.7);
    animation: spin 0.8s linear infinite;
    animation-delay: 0.25s;
    opacity: 0;
    animation-fill-mode: forwards;
  }
  @keyframes spin {
    from {
      opacity: 1;
      transform: translate(-50%, -50%) rotate(0);
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%) rotate(360deg);
    }
  }

  /* The tools and the page ticks float over the art in glass, with no bar across the page. */
  .toolbar,
  .bottom {
    position: absolute;
    z-index: 3;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 0.35s,
      transform 0.45s var(--ease);
  }
  .toolbar.shown,
  .bottom.shown {
    opacity: 1;
    transform: none;
    pointer-events: auto;
  }
  .pill,
  .bottom,
  .win {
    background: rgba(16, 16, 16, 0.86);
    backdrop-filter: blur(24px) saturate(1.2);
    border: 1px solid rgba(255, 255, 255, 0.07);
  }
  /* A full-width strip, so the window drags from anywhere along the top; only the pill shows. */
  .toolbar {
    top: 0;
    left: 0;
    right: 0;
    height: 58px;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 10px;
    transform: translateY(-8px);
  }
  .pill {
    height: 40px;
    max-width: calc(100% - 300px);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px 0 4px;
    border-radius: 20px;
  }
  .sep {
    flex: none;
    width: 1px;
    height: 16px;
    margin: 0 6px;
    background: rgba(255, 255, 255, 0.1);
  }
  .bottom {
    left: 16px;
    right: 16px;
    bottom: 12px;
    height: 34px;
    display: flex;
    align-items: center;
    padding: 0 18px;
    border-radius: 17px;
    transform: translateY(8px);
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 240px;
    display: flex;
    align-items: baseline;
    gap: 10px;
    white-space: nowrap;
    overflow: hidden;
    color: #d8d8d8;
    font-size: 13px;
  }
  .title > span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dim {
    color: var(--muted);
  }
  .rtl {
    display: inline-grid;
    align-self: center;
  }
  .tools {
    flex: 0 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 2px;
    overflow: hidden;
  }
  .tool {
    width: 32px;
    height: 32px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 16px;
    color: #9a9a9a;
    transition:
      background 0.15s,
      color 0.15s;
  }
  .tool:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.07);
    color: #fff;
  }
  .tool.on {
    background: rgba(255, 255, 255, 0.13);
    color: #fff;
  }
  .tool:disabled {
    opacity: 0.3;
    cursor: default;
  }
  .back {
    color: #cfcfcf;
  }
  .txt {
    font-size: 11px;
    letter-spacing: -0.02em;
  }
  .status {
    flex: none;
    display: flex;
    align-items: baseline;
    gap: 6px;
    white-space: nowrap;
    color: #d0d0d0;
    cursor: text;
  }
  .pageno {
    field-sizing: content;
    min-width: 2ch;
    padding: 3px 5px;
    margin-right: -2px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: right;
    outline: none;
    transition: background 0.15s;
  }
  .pageno:hover {
    background: rgba(255, 255, 255, 0.07);
  }
  .pageno:focus {
    background: rgba(255, 255, 255, 0.13);
    color: #fff;
  }
  .clock {
    margin-left: 6px;
  }
  /* The window buttons keep their corner, on glass of their own. */
  .win {
    position: absolute;
    top: 0;
    right: 0;
    height: 40px;
    display: flex;
    border-width: 0 0 1px 1px;
    border-radius: 0 0 0 12px;
    overflow: hidden;
  }
  .panel {
    margin-left: 18px;
    color: #9a9a9a;
    white-space: nowrap;
  }
  /* Readable over white margins as well as black ones. */
  .pin {
    position: absolute;
    top: 12px;
    right: 14px;
    z-index: 2;
    padding: 4px 9px;
    border-radius: 5px;
    background: rgba(10, 10, 10, 0.72);
    backdrop-filter: blur(12px);
    color: rgba(255, 255, 255, 0.78);
    pointer-events: none;
  }
  .note {
    position: absolute;
    left: 50%;
    bottom: 62px;
    z-index: 7;
    transform: translate(-50%, 6px);
    padding: 8px 14px;
    border-radius: 6px;
    background: rgba(20, 20, 20, 0.92);
    color: var(--text);
    font-size: 13px;
    opacity: 0;
    pointer-events: none;
    transition:
      opacity 0.3s,
      transform 0.3s var(--ease);
  }
  .note.on {
    opacity: 1;
    transform: translate(-50%, 0);
  }

  .end {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.86);
    backdrop-filter: blur(12px);
    animation: dim 0.4s ease-out;
  }
  .end-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }
  .done {
    color: var(--text-2);
    margin-bottom: 40px;
  }
  .next {
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .next img {
    width: 180px;
    aspect-ratio: 2 / 3;
    object-fit: cover;
    border-radius: 2px;
    transition: transform 0.5s var(--ease);
  }
  .next:hover img {
    transform: translateY(-4px);
  }
  .nl {
    margin-top: 20px;
    font-weight: 300;
    font-size: 28px;
    letter-spacing: -0.03em;
  }
  .nt {
    margin-top: 4px;
    color: var(--text-2);
  }
  .end-actions {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-top: 36px;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }
</style>
