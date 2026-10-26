<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { api, type ScanStats } from "$lib/api";
  import { lib, nav, openReader, reader, search, sheets } from "$lib/state.svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import DropZone from "$lib/components/DropZone.svelte";
  import Matcher from "$lib/components/Matcher.svelte";
  import MetaEditor from "$lib/components/MetaEditor.svelte";
  import SearchOverlay from "$lib/components/SearchOverlay.svelte";
  import TopBar from "$lib/components/TopBar.svelte";
  import WindowControls from "$lib/components/WindowControls.svelte";
  import Reader from "$lib/reader/Reader.svelte";
  import Collection from "$lib/views/Collection.svelte";
  import Home from "$lib/views/Home.svelte";
  import Series from "$lib/views/Series.svelte";
  import People from "$lib/views/People.svelte";
  import Settings from "$lib/views/Settings.svelte";

  const route = $derived(nav.current);
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    history.scrollRestoration = "manual";
    api.settings().then((s) => {
      lib.hasRoot = !!s.libraryRoot;
      lib.scanning = s.scanning;
    });
    // A comic opened from Explorer: at launch, or while AionComics is already running.
    const openFile = (path: string) =>
      api.openPath(path).then((id) => openReader(id)).catch((e) => ((lib.notice = String(e)), (lib.noticeLabel = "Couldn't open")));
    api.takeOpenRequest().then((p) => p && openFile(p));
    const subs = [
      listen("scan-started", () => (lib.scanning = true)),
      listen<ScanStats | null>("library-changed", (e) => {
        lib.scanning = false;
        const kept = e.payload?.keptMissing ?? 0;
        if (e.payload) {
          lib.notice = kept
            ? `${kept.toLocaleString()} books couldn't be found, so they were kept. Reconnect the drive and rescan.`
            : "";
          lib.noticeLabel = kept ? "Some books missing" : "";
        }
        lib.version++;
      }),
      // Comic Vine details arrive one book at a time; refresh the screen at most every few seconds.
      listen("metadata-changed", () => {
        if (refreshTimer) return;
        refreshTimer = setTimeout(() => {
          refreshTimer = undefined;
          lib.version++;
        }, 4000);
      }),
      listen<string>("open-file", (e) => openFile(e.payload)),
      listen<string>("scan-failed", (e) => {
        lib.scanning = false;
        lib.notice = e.payload;
        lib.noticeLabel = "Library not found";
      }),
    ];
    return () => subs.forEach((p) => p.then((un) => un()));
  });

  $effect(() => {
    void lib.version;
    api.nav().then((n) => (lib.collections = n.collections));
  });

  // The library stays mounted under the reader; stop it scrolling meanwhile. Its scrollbar gutter goes
  // too, since nothing can paint over a gutter and the reader should reach the window's edge; padding
  // the same width keeps the covers where they were, for the flight back.
  $effect(() => {
    if (reader.id === null) return;
    const html = document.documentElement;
    const gutter = innerWidth - html.clientWidth;
    html.style.overflow = "hidden";
    html.style.scrollbarGutter = "auto";
    document.body.style.paddingRight = `${gutter}px`;
    return () => {
      html.style.overflow = "";
      html.style.scrollbarGutter = "";
      document.body.style.paddingRight = "";
    };
  });

  // New page starts at the top; views restore the old position on Back once they've loaded.
  $effect(() => {
    void route;
    window.scrollTo(0, 0);
  });

  function onkey(e: KeyboardEvent) {
    if (reader.id !== null || search.open) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      search.open = true;
    } else if (e.altKey && e.key === "ArrowLeft") nav.back();
  }

  function onmouseup(e: MouseEvent) {
    if (e.button === 3 && reader.id === null) nav.back();
  }
</script>

<svelte:window onkeydown={onkey} {onmouseup} />

{#if lib.hasRoot === false}
  <div class="bare" data-tauri-drag-region><WindowControls /></div>
  <Settings welcome />
{:else}
  <TopBar />
  {#key nav.stack.length + JSON.stringify(route)}
    <main>
      {#if route.name === "home"}
        <Home />
      {:else if route.name === "collection"}
        <Collection id={route.id} />
      {:else if route.name === "series"}
        <Series id={route.id} book={route.book} />
      {:else if route.name === "people"}
        <People kind={route.kind} value={route.value} />
      {:else if route.name === "settings"}
        <Settings />
      {/if}
    </main>
  {/key}
{/if}

{#if reader.id !== null}
  <Reader
    id={reader.id}
    onopen={(id) => {
      reader.from = null;
      reader.id = id;
    }}
    onclose={() => {
      reader.id = null;
      reader.from = null;
      lib.version++;
    }}
  />
{/if}

<SearchOverlay />
{#if sheets.editing !== null}{#key sheets.editing}<MetaEditor id={sheets.editing} />{/key}{/if}
{#if sheets.matching}<Matcher seriesId={sheets.matching.seriesId} isManga={sheets.matching.isManga} name={sheets.matching.name} />{/if}
<ContextMenu />
<DropZone />

<style>
  /* First run has no top bar, but the window still needs a place to drag from and its buttons. */
  .bare {
    position: fixed;
    inset: 0 0 auto 0;
    height: 40px;
    z-index: 20;
    display: flex;
    justify-content: flex-end;
  }
  main {
    min-height: 100vh;
    animation: fade 0.45s var(--ease);
  }
  /* Opacity only: a moving page would throw off covers flying onto it. */
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
