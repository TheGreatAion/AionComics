<script lang="ts">
  import { tick } from "svelte";
  import { api, coverUrl, duration, storyTitle, type BookStats, type BookTrouble, type ComicCard, type GuidedSpot, type Panel } from "$lib/api";
  import Icon, { type IconName } from "$lib/components/Icon.svelte";
  import WindowControls from "$lib/components/WindowControls.svelte";
  import Wash from "$lib/components/Wash.svelte";
  import { CLOSE_EASE, closeMs, fadeMs, fly, flightMs, visible } from "$lib/flight";
  import { lib, menu, reader, showMenu } from "$lib/state.svelte";
  import { ACTIONS, chapterName, keysFor, menuItems } from "./actions";
  import Bookmarks from "./Bookmarks.svelte";
  import { Ctl, FIT_LABEL, type Step } from "./ctl.svelte";
  import Extract from "./Extract.svelte";
  import Flow from "./Flow.svelte";
  import GuidedView from "./GuidedView.svelte";
  import { chord, loadOverrides, pretty } from "./keys";
  import Options from "./Options.svelte";
  import PageView from "./PageView.svelte";
  import PanelEditor from "./PanelEditor.svelte";
  import { pageFilter, prefs } from "./prefs.svelte";
  import ScrollView from "./ScrollView.svelte";
  import Hints from "./Hints.svelte";
  import Shortcuts from "./Shortcuts.svelte";
  import Ticks from "./Ticks.svelte";

  let { id, onclose, onopen }: { id: number; onclose: () => void; onopen: (id: number) => void } = $props();

  const c = new Ctl({
    open: (n) => onopen(n),
    close: () => close(),
    // G (Go to page) puts you in the page box in the toolbar.
    focusPage: () => {
      wake();
      tick().then(() => pageBox?.focus());
    },
  });
  let overrides = $state(loadOverrides());
  let pageImg = $state<HTMLImageElement>();
  let chrome = $state(true);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let clock = $state(new Date());
  /** Bumped after panel edits so Guided View refetches. */
  let panelsRev = $state(0);

  // The cover (or page) the reader grew out of. It flies in onto the first page and back out on close.
  // (Not into continuous scroll, which has no one page to land on; see the load below.)
  let arriving = $state(!!reader.from && visible(reader.from.getBoundingClientRect()));
  let leaving = $state(false);
  let leaveMs = $state(0);

  // Never leave a blank screen: if the first page is slow, skip the flight and bring the controls back.
  $effect(() => {
    if (!arriving) return;
    const t = setTimeout(() => (arriving = false), 1500);
    return () => clearTimeout(t);
  });

  // A book that won't open: what's wrong with it, in words, and where to go from there.
  let trouble = $state<BookTrouble | null>(null);
  let attempt = $state(0);
  const WRONG = {
    drive: "The drive or folder your comics are on isn't connected right now.",
    missing: "The file isn't where it was. It may have been moved or deleted.",
    damaged: "The file is damaged, or isn't a comic AionComics can read.",
  };

  $effect(() => {
    const book = id;
    void attempt;
    c.error = null;
    trouble = null;
    c.steps = null;
    c.overlay = null;
    c.rotation = 0;
    api
      .openComic(book)
      .then((r) => {
        if (book !== id) return;
        c.load(r);
        if (c.mode === "scroll") arriving = false;
      })
      .catch((e) => {
        if (book !== id) return;
        // The last book's pages are gone: keys mustn't go on turning them out of sight.
        if (pending) flush();
        c.info = null;
        c.error = String(e);
        arriving = false;
        api.bookTrouble(book).then((t) => book === id && (trouble = t), () => {});
      });
  });

  // Guided View stops for the page or spread on screen; the pages after it are worked out ahead.
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
      const back = c.resume;
      const stop = back && c.page === c.openedAt ? c.stopOf(back) : -1;
      c.resume = null;
      if (at && pages.includes(at.page)) {
        c.step = c.stopAt(at.page, at.x, at.y);
        c.part = 0;
      } else if (back && stop >= 0) {
        c.step = stop;
        c.part = back.part;
      } else if (c.landOnLast) c.step = steps.length - 1;
      else if (!prefs.guidedOverview && steps.length && c.step < 0) c.step = 0;
      c.zoomAt = null;
      c.landOnLast = false;
      // The rest of the book's panels, from the next page on, in the background.
      api.panelsAhead(book, pages[pages.length - 1] + 1, rtl).catch(() => {});
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

  // Keep the panel Guided View is on, so the book reopens there. Unlike the page, stepping through a
  // page's panels counts even if the page never moves.
  let pendingSpot: [number, GuidedSpot | null] | null = null;
  let spotTimer: ReturnType<typeof setTimeout> | undefined;
  function flushSpot() {
    clearTimeout(spotTimer);
    if (!pendingSpot) return Promise.resolve();
    const [book, spot] = pendingSpot;
    pendingSpot = null;
    return api.saveGuidedSpot(book, spot);
  }
  $effect(() => {
    // Until the book is back on its saved panel, that's still the one to keep.
    if (!c.info || c.info.external || c.resume) return;
    const [book, spot] = [c.id, c.spot];
    if (pendingSpot && pendingSpot[0] !== book) flushSpot();
    if (spot === undefined) return;
    pendingSpot = [book, spot];
    clearTimeout(spotTimer);
    spotTimer = setTimeout(flushSpot, 400);
  });

  // The end card says how the read went, once the last page's save is in.
  let read = $state<BookStats | null>(null);
  $effect(() => {
    if (!c.endCard || !c.info) return void (read = null);
    const book = c.id;
    flush().then(() => api.bookStats(book).then((s) => book === c.id && (read = s)));
  });
  // The end of a series: the book on top of your pile is offered instead of a next one.
  let waiting = $state<ComicCard | null>(null);
  $effect(() => {
    if (!c.endCard || !c.info || c.info.next || c.info.external) return void (waiting = null);
    const book = c.id;
    api.home().then((h) => {
      if (book === c.id) waiting = [...h.continueReading, ...h.pickUpAgain].find((b) => b.id !== book) ?? null;
    });
  });
  $effect(() => {
    c.fromPile = waiting?.id ?? null;
  });

  const readLine = $derived.by(() => {
    if (!read?.sittings) return [];
    const days = read.firstAt && read.lastAt ? Math.round((read.lastAt - read.firstAt) / 86_400) + 1 : 1;
    return [
      `${read.pages.toLocaleString()} ${read.pages === 1 ? "page" : "pages"}`,
      `${read.sittings} ${read.sittings === 1 ? "sitting" : "sittings"}`,
      duration(read.seconds),
      ...(days > 1 ? [`over ${days} days`] : []),
    ];
  });

  $effect(() => {
    const t = setInterval(() => (clock = new Date()), 15000);
    return () => clearInterval(t);
  });

  // Wherever the reader doesn't quite reach (a window edge mid-resize), the page background shows:
  // make that the reader's too. Not while it opens or closes: the library shows through then, and it
  // should be on its own background, not jump to it at the end.
  const readerBg = $derived(prefs.background === "gray" ? "#1b1b1b" : "#000");
  $effect(() => {
    if (arriving || leaving) return;
    document.documentElement.style.background = document.body.style.background = readerBg;
    return () => (document.documentElement.style.background = document.body.style.background = "");
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
    const wasFull = c.fullscreen;
    if (wasFull) c.setFullscreen(false);
    if (pending && pending[0] !== c.id) flush();
    if (c.info && movedIn === c.id) pending = [c.id, c.furthest, c.count];
    if (pendingSpot && pendingSpot[0] !== c.id) flushSpot();
    const spot = c.spot;
    if (c.info && !c.info.external && !c.resume && spot !== undefined) pendingSpot = [c.id, spot];
    const save = Promise.all([flush(), flushSpot()]);
    // The library catches up now, while the reader still mostly covers it: the page lands on the
    // page you stopped on, not on the old one that then changes.
    save.finally(() => lib.version++);
    const to = reader.from;
    // Shrink back only when the page is shown plainly; from a zoomed or turned page it would jump, and
    // leaving full screen moves everything mid-flight. Otherwise the reader fades away.
    const plain =
      !wasFull && c.zoom === 1 && c.rotation === 0 && (c.mode === "pages" || (c.mode === "guided" && c.step === -1));
    if (to?.isConnected && pageImg?.isConnected && plain && visible(to.getBoundingClientRect())) {
      leaveMs = closeMs();
      leaving = true;
      fly(pageImg.currentSrc || pageImg.src, pageImg.getBoundingClientRect(), to.getBoundingClientRect(), {
        hide: [pageImg, to],
        ms: leaveMs,
        easing: CLOSE_EASE,
        landing: to,
        done: () => save.finally(onclose),
      });
    } else {
      leaveMs = fadeMs();
      leaving = true;
      setTimeout(() => save.finally(onclose), leaveMs);
    }
  }

  function onkey(e: KeyboardEvent) {
    if (menu.open || leaving) return;
    if (c.error) {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
      }
      return;
    }
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
  // The chapter you're in, and how much of it is left: "Ch. 14 · 6 pages left".
  const chapter = $derived.by(() => {
    const k = c.chapterAt;
    if (k === null) return null;
    const list = c.chapters;
    const end = list[k + 1]?.page ?? c.count;
    const left = end - (c.shown[c.shown.length - 1] + 1);
    const rest = left <= 0 ? "last page" : left === 1 ? "1 page left" : `${left} pages left`;
    return { name: chapterName(list[k]), rest };
  });

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
  style:--fade="{flightMs()}ms"
  style:--leave="{leaveMs}ms"
  style:--reader-bg={readerBg}
  style:--page-filter={pageFilter()}
  onmousemove={(e) => bars(!!(e.target as Element).closest?.(".toolbar.shown, .bottom.shown"))}
  oncontextmenu={openMenu}
  role="presentation"
>
  {#if c.error}
    <div class="center msg trouble" role="alert">
      <h2>Can’t open {trouble?.title || "this book"}</h2>
      <p>{trouble ? WRONG[trouble.kind] : "Something went wrong opening it."}</p>
      {#if trouble}<p class="file code" title={c.error}>{trouble.fileName}</p>{/if}
      <div class="ways">
        {#if trouble?.next}
          <button class="btn icon-btn" onclick={() => onopen(trouble!.next!.id)}><Icon name="play" size={14} />{trouble.next.label}</button>
        {/if}
        <button class="ico" title="Try again" aria-label="Try again" onclick={() => attempt++}><Icon name="rescan" size={18} /></button>
        {#if trouble?.canShow}
          <button class="ico" title="Show in Explorer" aria-label="Show in Explorer" onclick={() => api.reveal(id)}><Icon name="folder" size={18} /></button>
        {/if}
        <button class="ico" title="Back to library (Esc)" aria-label="Back to library" onclick={close}><Icon name="back" size={18} /></button>
      </div>
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
        <!-- In the next book's color: it's what's waiting. -->
        <Wash color={(c.info.next ?? c.info.comic).coverColor} height="100%" strength={0.3} />
        <div class="end-card">
          <p class="done">You finished {[c.info.comic.seriesName.trim(), c.info.comic.label].filter(Boolean).join(" ")}.</p>
          {#if readLine.length}
            <p class="read fine">{#each readLine as part, i}{#if i}<span class="dot">·</span>{/if}<span>{part}</span>{/each}</p>
          {/if}
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
          {:else if waiting}
            {@const w = waiting}
            <p class="pile-note">Next from your pile</p>
            <button class="next" onclick={() => onopen(w.id)}>
              <img src={coverUrl(w.id)} alt="" />
              <span class="nl">{w.seriesName.trim()}</span>
              <span class="nt">{w.label}{#if w.numPages}&ensp;·&ensp;page {w.currentPage + 1} of {w.numPages}{/if}</span>
            </button>
            <div class="end-actions">
              <button class="btn icon-btn" onclick={() => onopen(w.id)}><Icon name="play" size={14} />Resume</button>
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
      <div class="pin fine">{#if panel}{panel}&ensp;·&ensp;{/if}{count}&ensp;·&ensp;{time}</div>
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
        {#if icon}<Icon name={icon} />{:else}<span class="txt fine">{text}</span>{/if}
      </button>
    {/snippet}

    <!-- The top strip drags the window; the tools hang from its middle in one solid tab. -->
    <header
      class="toolbar"
      class:shown={showChrome}
      data-tauri-drag-region
      role="toolbar"
      tabindex="-1"
      onpointerenter={() => bars(true)}
      onpointerleave={() => bars(false)}
    >
      <div class="tab" data-tauri-drag-region>
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
            title="{FIT_LABEL[c.view.fit]} (click to change)"
            aria-label="Fit: {FIT_LABEL[c.view.fit]}"
            onclick={() => (c.cycleFit(), wake())}
          >
            {#if c.view.fit === "full"}<span class="txt fine">1:1</span>{:else}<Icon name={FIT_ICON[c.view.fit]} />{/if}
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
        <label class="status fine" title="Type a page and press Enter">
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
      {#if chapter}<span class="chapter fine" title="{chapter.name} · {chapter.rest}"><span class="chname">{chapter.name}</span><span class="dim">&nbsp;· {chapter.rest}</span></span>{/if}
      <Ticks {c} />
      {#if panel}<span class="panel fine">{panel}</span>{/if}
    </div>

    {#if c.overlay === "flow"}<Flow {c} />{/if}
    {#if c.overlay === "bookmarks"}<Bookmarks {c} />{/if}
    {#if c.overlay === "options"}<Options {c} />{/if}
    {#if c.overlay === "extract"}<Extract {c} />{/if}
    {#if c.overlay === "shortcuts"}<Shortcuts bind:overrides onclose={() => (c.overlay = null)} />{/if}
    {#if c.overlay === "editor"}
      <PanelEditor {c} onsaved={() => panelsRev++} />
    {/if}

    <div class="note" class:on={!!c.note}>{c.note}</div>
    <Hints {c} {overrides} />
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
  }
  @keyframes dim {
    from {
      background: rgba(0, 0, 0, 0);
    }
  }
  /* Opacity and scale alone, which the GPU animates by itself while the library refreshes behind. */
  .reader.leaving {
    opacity: 0;
    transform: scale(0.985);
    pointer-events: none;
    transition:
      opacity var(--leave) cubic-bezier(0.4, 0, 0.2, 1),
      transform var(--leave) cubic-bezier(0.4, 0, 0.2, 1);
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
  .trouble {
    width: min(620px, calc(100vw - 64px));
  }
  .trouble h2 {
    font: 400 44px/0.95 var(--display);
    letter-spacing: 0.01em;
    overflow-wrap: anywhere;
  }
  .trouble p {
    margin-top: 14px;
    color: var(--text-2);
    font-size: 15px;
  }
  .trouble .file {
    margin-top: 8px;
    color: var(--text-3);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .ways {
    margin-top: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
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

  /* The tools hang from the top edge and the page ticks rise from the bottom one, with no bar
     across the page. */
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
  .bottom {
    background: rgba(16, 16, 16, 0.86);
    backdrop-filter: blur(24px) saturate(1.2);
    border: 1px solid rgba(255, 255, 255, 0.07);
  }
  /* Solid, square at the top edge it hangs from. */
  .tab,
  .win {
    background: #141414;
    border: 1px solid #2a2a2a;
    border-top: 0;
    box-shadow:
      inset 0 -1px 0 rgba(255, 255, 255, 0.04),
      0 10px 26px rgba(0, 0, 0, 0.55);
  }
  /* A full-width strip, so the window drags from anywhere along the top; only the tab shows. */
  .toolbar {
    top: 0;
    left: 0;
    right: 0;
    height: 46px;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    transform: translateY(-8px);
  }
  .tab {
    height: 46px;
    max-width: calc(100% - 300px);
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px 0 6px;
    border-radius: 0 0 8px 8px;
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
    bottom: 0;
    height: 36px;
    display: flex;
    align-items: center;
    padding: 0 18px;
    border-bottom: 0;
    border-radius: 8px 8px 0 0;
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
    border-radius: 4px;
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
    font-size: 12px;
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
    border-radius: 4px;
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
  /* The window buttons keep their corner, on a tab of their own. */
  .win {
    position: absolute;
    top: 0;
    right: 0;
    height: 46px;
    display: flex;
    border-right: 0;
    border-radius: 0 0 0 8px;
    overflow: hidden;
  }
  .chapter {
    display: flex;
    max-width: 34%;
    margin-right: 18px;
    color: #d0d0d0;
    white-space: nowrap;
  }
  .chname {
    overflow: hidden;
    text-overflow: ellipsis;
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
    isolation: isolate;
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
  /* How the read went: "226 pages · 3 sittings · 2 h 10 min · over 4 days". */
  .read {
    margin: -26px 0 36px;
    color: var(--text);
    display: flex;
    gap: 10px;
  }
  .read .dot {
    color: var(--text-3);
  }
  .pile-note {
    margin-bottom: 18px;
    color: var(--text-3);
    font-size: 13px;
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
    font-family: var(--display);
    font-weight: 400;
    font-size: 40px;
    line-height: 0.95;
    letter-spacing: 0.01em;
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
