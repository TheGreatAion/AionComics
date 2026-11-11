<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type Backup, type ComicCard, type CvStatus, type Options, type Settings, type Storage } from "$lib/api";
  import Cover from "$lib/components/Cover.svelte";
  import Icon, { type IconName } from "$lib/components/Icon.svelte";
  import { look, setBackdrop, setMotion, type Motion } from "$lib/look.svelte";
  import { ACTIONS, keysFor, type Action } from "$lib/reader/actions";
  import { chord, loadOverrides, pretty, saveOverrides } from "$lib/reader/keys";
  import { FIT_LABEL } from "$lib/reader/ctl.svelte";
  import { prefs, savePrefs, type Prefs } from "$lib/reader/prefs.svelte";
  import { lib, nav } from "$lib/state.svelte";
  import { setTheme, theme, type ThemeChoice } from "$lib/theme.svelte";

  let s = $state<Settings | null>(null);
  let cv = $state<CvStatus | null>(null);
  let cvKey = $state("");
  let cvBusy = $state(false);
  let cvError = $state("");
  let error = $state<string | null>(null);
  let backups = $state<Backup[]>([]);
  let backupBusy = $state(false);
  let backupError = $state("");
  // The backup you've asked to restore, waiting for a yes.
  let confirming = $state<string | null>(null);
  let showAll = $state(false);
  let storage = $state<Storage | null>(null);
  let clearing = $state(false);
  let version = $state("");
  // Books taken off Continue reading, to put back.
  let taken = $state<ComicCard[]>([]);

  $effect(() => {
    void lib.version;
    void lib.scanning;
    api.settings().then((v) => (s = v));
    api.cvStatus().then((v) => (cv = v));
    api.backups().then((v) => (backups = v));
    api.takenOffReading().then((v) => (taken = v));
  });

  // While reading waits to reach YACReader, keep its line up to date.
  $effect(() => {
    if (!s?.options.yacreaderSync || !s.yacreader?.pending) return;
    const t = setInterval(() => api.settings().then((v) => (s = v)), 5000);
    return () => clearInterval(t);
  });
  const yacLine = $derived.by(() => {
    const y = s?.yacreader;
    if (!y || !s?.options.yacreaderSync) return "Your page and finished books, saved into YACReader's library too";
    const books = (n: number) => `${n.toLocaleString()} ${n === 1 ? "book" : "books"}`;
    if (y.waiting) return `${books(y.pending)} waiting for YACReader to close`;
    if (y.pending) return `Sending ${books(y.pending)}…`;
    if (y.lastSent) return `Up to date · last sent ${when(y.lastSent.at)}`;
    return "Up to date";
  });
  async function setYacreaderSync(on: boolean) {
    await setOption("yacreaderSync", on);
    s = await api.settings();
  }

  // What you've hidden from the app, with the way back.
  let hiddenItems = $state<{ label: string; at: number }[]>([]);
  $effect(() => {
    void lib.version;
    api.hiddenList().then((v) => (hiddenItems = v));
  });
  async function unhide(h: { label: string; at: number }) {
    hiddenItems = hiddenItems.filter((x) => x !== h);
    lib.scanning = true;
    await api.putBack(h.label, h.at);
  }

  async function putBack(c: ComicCard) {
    await api.unhideFromReading(c.id);
    lib.version++;
  }
  $effect(() => {
    api.storage().then((v) => (storage = v));
    getVersion().then((v) => (version = v));
  });

  // ---------- sections and the jump bar ----------
  const SECTIONS: { id: string; icon: IconName; name: string }[] = [
    { id: "look", icon: "theme", name: "Look" },
    { id: "reading", icon: "fitPage", name: "Reading" },
    { id: "guided", icon: "guided", name: "Guided View" },
    { id: "keys", icon: "keyboard", name: "Keyboard" },
    { id: "library", icon: "folder", name: "Library" },
    { id: "details", icon: "fetch", name: "Details" },
    { id: "storage", icon: "storage", name: "Storage" },
    { id: "backups", icon: "undo", name: "Backups" },
  ];
  const sections: Record<string, HTMLElement> = {};
  let here = $state("look");
  function jump(id: string) {
    sections[id]?.scrollIntoView({ behavior: look.motion === "off" ? "auto" : "smooth", block: "start" });
  }
  // The section you're in: the last one whose caption has scrolled up past the top bar.
  function onscroll() {
    let at = SECTIONS[0].id;
    for (const { id } of SECTIONS) if ((sections[id]?.getBoundingClientRect().top ?? 1e9) < 160) at = id;
    if (innerHeight + scrollY >= document.documentElement.scrollHeight - 4) at = "backups";
    here = at;
  }

  // ---------- look ----------
  const THEMES: { key: ThemeChoice; label: string }[] = [
    { key: "dark", label: "Dark" },
    { key: "light", label: "Light" },
    { key: "system", label: "Match Windows" },
  ];
  const MOTIONS: { key: Motion; label: string }[] = [
    { key: "full", label: "Full" },
    { key: "quick", label: "Quick" },
    { key: "off", label: "Off" },
  ];
  // The little app in each theme's picture: its page, side panel, a line of text and four covers.
  const PAINT = {
    dark: { bg: "#1d1b19", side: "#141210", text: "#ececec" },
    light: { bg: "#ebe5da", side: "#e0d9cb", text: "#1a1815" },
  };
  const COVERS = ["#8a3b2e", "#c9a23a", "#3d5a8c", "#6c7a3a"];

  // ---------- reading ----------
  function set<K extends keyof Prefs>(k: K, v: Prefs[K]) {
    prefs[k] = v;
    savePrefs();
  }
  const FITS: { key: Prefs["fit"]; icon: IconName }[] = [
    { key: "height", icon: "fitHeight" },
    { key: "width", icon: "fitWidth" },
    { key: "page", icon: "fitPage" },
    { key: "full", icon: "fullSize" },
  ];
  // The reader's two backgrounds, as Reader.svelte paints them.
  const BACKS: { key: Prefs["background"]; label: string; bg: string }[] = [
    { key: "black", label: "Black", bg: "#000" },
    { key: "gray", label: "Gray", bg: "#1b1b1b" },
  ];
  const DIMS: { key: Prefs["guidedDim"]; label: string; alpha: number }[] = [
    { key: "light", label: "Dimmed", alpha: 0.25 },
    { key: "strong", label: "Dark", alpha: 0.06 },
    { key: "hidden", label: "Hidden", alpha: 0 },
  ];

  // ---------- keyboard ----------
  let overrides = $state(loadOverrides());
  let listening = $state<string | null>(null);
  let keyNotice = $state("");
  let keyQuery = $state("");
  let allKeys = $state(false);
  // The ones reached for most, shown before "more".
  const COMMON = ["next", "prev", "goto", "guided", "double", "fitPage", "fitWidth", "fitHeight", "flow", "magnifier", "bookmark", "fullscreen", "nextComic", "prevComic", "close"];
  const keyLabel = (a: Action) =>
    a.id === "left" ? "Turn left (next page in manga)" : a.id === "right" ? "Turn right (previous page in manga)" : a.label.replace(/…$/, "");
  const shownKeys = $derived.by(() => {
    const q = keyQuery.trim().toLowerCase();
    if (q) return ACTIONS.filter((a) => keyLabel(a).toLowerCase().includes(q) || keysFor(a, overrides).some((k) => pretty(k).toLowerCase() === q));
    if (allKeys) return [...COMMON.map((id) => ACTIONS.find((a) => a.id === id)!), ...ACTIONS.filter((a) => !COMMON.includes(a.id))];
    return COMMON.map((id) => ACTIONS.find((a) => a.id === id)!);
  });
  // While listening, the next key press becomes the shortcut. Capture phase, so nothing else sees it.
  function capture(e: KeyboardEvent) {
    if (!listening) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.key === "Escape") return void (listening = null);
    if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
    const k = chord(e);
    const next = { ...overrides };
    // A key does one thing: take it away from whatever had it.
    const taken = ACTIONS.find((a) => a.id !== listening && keysFor(a, next).includes(k));
    if (taken) {
      next[taken.id] = keysFor(taken, next).filter((x) => x !== k);
      keyNotice = `${pretty(k)} was ${keyLabel(taken).toLowerCase()}; that now has ${next[taken.id].length ? pretty(next[taken.id][0]) : "no key"}.`;
    } else keyNotice = "";
    next[listening] = [k];
    overrides = next;
    saveOverrides(next);
    listening = null;
  }
  function resetKeys() {
    overrides = {};
    saveOverrides({});
    listening = null;
    keyNotice = "Every shortcut is back to its default.";
  }

  // ---------- engine options ----------
  async function setOption<K extends keyof Options>(k: K, v: Options[K]) {
    if (!s) return;
    s.options[k] = v;
    await api.setOptions(s.options);
  }

  // ---------- library ----------
  async function choose() {
    error = null;
    const dir = await open({ directory: true, title: "Choose your comics folder" });
    if (typeof dir !== "string") return;
    try {
      await api.setLibraryRoot(dir);
      lib.hasRoot = true;
      lib.scanning = true;
    } catch (e) {
      error = String(e);
    }
  }
  function setLayout(key: string) {
    if (!s || s.layout === key || lib.scanning) return;
    s.layout = key as typeof s.layout;
    lib.scanning = true;
    api.setLayout(key);
  }
  function rescan() {
    lib.scanning = true;
    lib.notice = "";
    api.rescan();
  }

  // ---------- details ----------
  async function saveKey(key: string) {
    cvBusy = true;
    cvError = "";
    try {
      await api.cvSetKey(key);
      cvKey = "";
      cv = await api.cvStatus();
    } catch (e) {
      cvError = String(e);
    } finally {
      cvBusy = false;
    }
  }

  // ---------- storage ----------
  const used = $derived(storage ? storage.pages + storage.prints + storage.lists : 0);
  const PARTS = $derived(
    storage
      ? [
          { label: "Pages ready to read", bytes: storage.pages, shade: 1 },
          { label: "Collection backgrounds", bytes: storage.prints, shade: 0.6 },
          { label: "Page lists and details", bytes: storage.lists, shade: 0.3 },
        ]
      : [],
  );
  async function clearPages() {
    clearing = true;
    try {
      await api.clearPages();
      storage = await api.storage();
    } finally {
      clearing = false;
    }
  }

  // ---------- backups ----------
  async function backupNow() {
    backupBusy = true;
    backupError = "";
    try {
      await api.backupNow();
      backups = await api.backups();
    } catch (e) {
      backupError = String(e);
    } finally {
      backupBusy = false;
    }
  }
  async function restore(name: string) {
    backupBusy = true;
    try {
      await api.restoreBackup(name);
    } catch (e) {
      backupError = String(e);
      backupBusy = false;
      confirming = null;
    }
  }

  const bytes = (n: number) =>
    n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(1)} GB` : n >= 1 << 20 ? `${Math.round(n / (1 << 20))} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;
  const when = (t: number) =>
    new Date(t * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
</script>

<svelte:window onkeydowncapture={capture} {onscroll} />

{#snippet toggle(on: boolean, flip: () => void, label: string)}
  <button class="tog" class:on role="switch" aria-checked={on} aria-label={label} onclick={flip}><i></i></button>
{/snippet}

{#snippet choice(opts: readonly { key: string | number; label: string }[], value: string | number, pick: (v: any) => void, label: string)}
  <div class="seg" role="radiogroup" aria-label={label}>
    {#each opts as o (o.key)}
      <button class:on={value === o.key} role="radio" aria-checked={value === o.key} onclick={() => pick(o.key)}>{o.label}</button>
    {/each}
  </div>
{/snippet}

{#snippet mini(paint: { bg: string; side: string; text: string })}
  <span class="app" style:background={paint.bg}>
    <b style:background={paint.side}></b>
    <u style:background={paint.text}></u>
    {#each COVERS as c, i (i)}<em style:left="{34 + i * 22}px" style:background={c}></em>{/each}
  </span>
{/snippet}

<div class="page">
    <div class="top">
      <div class="caption">AionComics{version ? ` ${version}` : ""}</div>
      <h1>Settings</h1>
      <nav class="jump" aria-label="Sections">
        {#each SECTIONS as sec (sec.id)}
          <button class:on={here === sec.id} onclick={() => jump(sec.id)}><Icon name={sec.icon} size={20} /><span>{sec.name}</span></button>
        {/each}
      </nav>
    </div>

    {#if s}
      <div class="cols">
        <div>
          <section bind:this={sections.look}>
            <div class="caption">Look</div>
            <div class="pics wide-row" role="radiogroup" aria-label="Theme">
              {#each THEMES as t (t.key)}
                <button class="pic" class:on={theme.choice === t.key} role="radio" aria-checked={theme.choice === t.key} onclick={() => setTheme(t.key)}>
                  <span class="frame">
                    {#if t.key === "system"}
                      {@render mini(PAINT.dark)}<span class="half">{@render mini(PAINT.light)}</span>
                    {:else}
                      {@render mini(PAINT[t.key])}
                    {/if}
                  </span>
                  <span>{t.label}</span>
                </button>
              {/each}
            </div>
            <div class="row">
              <div class="lbl">Animations<small>How screens and covers move</small></div>
              {@render choice(MOTIONS, look.motion, setMotion, "Animations")}
            </div>
            <div class="row">
              <div class="lbl">Cover behind collection names<small>A cover from the collection, printed in one ink</small></div>
              {@render toggle(look.backdrop, () => setBackdrop(!look.backdrop), "Cover behind collection names")}
            </div>
          </section>

          <section bind:this={sections.guided}>
            <div class="caption">Guided View</div>
            <div class="row">
              <div class="lbl">Start books in Guided View<small>Press V in a book to switch</small></div>
              <div class="chips">
                <button class="chip" class:on={prefs.guidedManga} aria-pressed={prefs.guidedManga} onclick={() => set("guidedManga", !prefs.guidedManga)}>
                  {#if prefs.guidedManga}<Icon name="check" size={14} />{/if}Manga
                </button>
                <button class="chip" class:on={prefs.guidedComics} aria-pressed={prefs.guidedComics} onclick={() => set("guidedComics", !prefs.guidedComics)}>
                  {#if prefs.guidedComics}<Icon name="check" size={14} />{/if}Comics
                </button>
              </div>
            </div>
            <div class="row">
              <div class="lbl">Speed<small>Moving from panel to panel</small></div>
              {@render choice(
                [
                  { key: "slow", label: "Slow" },
                  { key: "normal", label: "Normal" },
                  { key: "fast", label: "Fast" },
                ] as const,
                prefs.guidedSpeed,
                (v) => set("guidedSpeed", v),
                "Speed",
              )}
            </div>
            <div class="row">
              <div class="lbl">Around the panel</div>
              <div class="pics" role="radiogroup" aria-label="Around the panel">
                {#each DIMS as d (d.key)}
                  <button class="pic small" class:on={prefs.guidedDim === d.key} role="radio" aria-checked={prefs.guidedDim === d.key} onclick={() => set("guidedDim", d.key)}>
                    <span class="frame panels">{#each [0, 1, 2, 3] as i (i)}<em style:opacity={i === 1 ? 1 : d.alpha}></em>{/each}</span>
                    <span>{d.label}</span>
                  </button>
                {/each}
              </div>
            </div>
            <div class="row">
              <div class="lbl">Show each page whole first<small>Then zoom into its first panel</small></div>
              {@render toggle(prefs.guidedOverview, () => set("guidedOverview", !prefs.guidedOverview), "Show each page whole first")}
            </div>
            <div class="row">
              <div class="lbl">Read tall panels in parts<small>Top to bottom, closer up</small></div>
              {@render toggle(prefs.guidedParts, () => set("guidedParts", !prefs.guidedParts), "Read tall panels in parts")}
            </div>
          </section>
        </div>

        <div>
          <section bind:this={sections.reading}>
            <div class="caption">Reading</div>
            <div class="row">
              <div class="lbl">Page brightness<small>For dim or faded scans · {prefs.brightness}%</small></div>
              <input class="range" type="range" min="50" max="150" step="5" value={prefs.brightness} aria-label="Page brightness" oninput={(e) => set("brightness", +e.currentTarget.value)} />
            </div>
            <div class="row">
              <div class="lbl">Page contrast<small>For washed-out scans · {prefs.contrast}%</small></div>
              <input class="range" type="range" min="50" max="150" step="5" value={prefs.contrast} aria-label="Page contrast" oninput={(e) => set("contrast", +e.currentTarget.value)} />
            </div>
            <div class="row">
              <div class="lbl">Page fit<small>{FIT_LABEL[prefs.fit]} · changing it in a book changes it here too</small></div>
              <div class="seg" role="radiogroup" aria-label="Page fit">
                {#each FITS as f (f.key)}
                  <button class:on={prefs.fit === f.key} role="radio" aria-checked={prefs.fit === f.key} title={FIT_LABEL[f.key]} aria-label={FIT_LABEL[f.key]} onclick={() => set("fit", f.key)}>
                    <Icon name={f.icon} size={17} />
                  </button>
                {/each}
              </div>
            </div>
            <div class="row">
              <div class="lbl">Two pages side by side<small>Manga pairs right to left</small></div>
              {@render toggle(prefs.double, () => set("double", !prefs.double), "Two pages side by side")}
            </div>
            <div class="row">
              <div class="lbl">Scroll down through pages<small>For webtoons and long strips</small></div>
              {@render toggle(prefs.scroll, () => set("scroll", !prefs.scroll), "Scroll down through pages")}
            </div>
            <div class="row">
              <div class="lbl">Behind the page</div>
              <div class="pics" role="radiogroup" aria-label="Behind the page">
                {#each BACKS as b (b.key)}
                  <button class="pic small" class:on={prefs.background === b.key} role="radio" aria-checked={prefs.background === b.key} onclick={() => set("background", b.key)}>
                    <span class="frame sheet" style:background={b.bg}><em></em></span>
                    <span>{b.label}</span>
                  </button>
                {/each}
              </div>
            </div>
            <div class="row">
              <div class="lbl">Page number and clock<small>Kept in the corner while you read</small></div>
              {@render toggle(prefs.info, () => set("info", !prefs.info), "Page number and clock")}
            </div>
            <div class="row" class:open={taken.length}>
              <div class="lbl">
                Taken off Continue reading
                <small>{taken.length ? "Put one back and it's on Home again, at the page you were on" : "Nothing right now"}</small>
              </div>
            </div>
            {#each taken as c (c.id)}
              <div class="row book">
                <span class="thumb"><Cover id={c.id} ratio={c.coverRatio} width={30} /></span>
                <div class="lbl">
                  {c.seriesName.trim()} {c.label}
                  {#if c.numPages}<small class="fine">{c.currentPage + 1} / {c.numPages}</small>{/if}
                </div>
                <button class="ico" title="Put back on Continue reading" aria-label="Put back on Continue reading" onclick={() => putBack(c)}>
                  <Icon name="undo" size={17} />
                </button>
              </div>
            {/each}
          </section>

          <section bind:this={sections.library}>
            <div class="caption">Library</div>
            <div class="row">
              <div class="lbl">
                <span
                  class="code path"
                  title={s.options.yacreaderSync && s.yacreader
                    ? "Only read, except to add comics and libraries you ask for and to send your reading to YACReader. Nothing there is moved or deleted unless you ask."
                    : "Only read, except to add comics and libraries you ask for. Nothing there is moved, changed or deleted unless you ask."}>{s.libraryRoot ?? "Not set"}</span>
                {#if s.lastScan}
                  <small class="fine">{s.lastScan.comics.toLocaleString()} books · {s.lastScan.series.toLocaleString()} series · scanned {when(s.lastScan.finishedAt)}</small>
                {/if}
              </div>
              <div class="ctl">
                <button class="ico" title="Change folder" aria-label="Change folder" onclick={choose}><Icon name="folder" size={17} /></button>
                <button class="ico" title="Rescan" aria-label="Rescan" disabled={lib.scanning} onclick={rescan}>
                  <span class:spin={lib.scanning}><Icon name="rescan" size={17} /></span>
                </button>
              </div>
            </div>
            {#if error}<p class="err"><Icon name="alert" size={14} />{error}</p>{/if}
            {#if lib.notice}<p class="err"><Icon name="alert" size={14} />{lib.notice}</p>{/if}
            <div class="row">
              <div class="lbl">Folders in your comics folder<small>Only changes how they show here. Nothing is moved.</small></div>
              {@render choice(
                [
                  { key: "auto", label: "Work it out" },
                  { key: "libraries", label: "Libraries" },
                  { key: "series", label: "Series" },
                ] as const,
                s.layout,
                setLayout,
                "Folders in your comics folder",
              )}
            </div>
            {#if hiddenItems.length}
              <div class="row open">
                <div class="lbl">Hidden from AionComics<small>Put one back and it's in your library again, at the page you were on</small></div>
              </div>
              {#each hiddenItems as h (`${h.label}-${h.at}`)}
                <div class="row book">
                  <div class="lbl">{h.label}<small class="fine">hidden {when(h.at)}</small></div>
                  <button class="ico" title="Put back" aria-label="Put {h.label} back" onclick={() => unhide(h)}><Icon name="undo" size={17} /></button>
                </div>
              {/each}
            {/if}
            <div class="row">
              <div class="lbl">Watch for new comics<small>Adds books as soon as they land in the folder</small></div>
              {@render toggle(s.options.watchFolder, () => setOption("watchFolder", !s!.options.watchFolder), "Watch for new comics")}
            </div>
            {#if s.yacreader}
              <div class="row">
                <div class="lbl">Send my reading to YACReader<small>{yacLine}</small></div>
                {@render toggle(s.options.yacreaderSync, () => setYacreaderSync(!s!.options.yacreaderSync), "Send my reading to YACReader")}
              </div>
            {/if}
            <button class="row go" onclick={() => nav.go({ name: "duplicates" })}>
              <div class="lbl">Duplicates<small>Books that may be in your library twice</small></div>
              <span class="ico"><Icon name="chevronRight" size={17} /></span>
            </button>
            <button class="row go" onclick={() => nav.go({ name: "health" })}>
              <div class="lbl">Library check<small>Find books that won't open: missing, damaged or empty files, and covers that won't load</small></div>
              <span class="ico"><Icon name="chevronRight" size={17} /></span>
            </button>
          </section>
        </div>
      </div>

      <div class="cols">
        <section class="wide" bind:this={sections.keys}>
          <div class="caption">Keyboard</div>
          <div class="keybar">
            <label class="find">
              <Icon name="search" size={15} />
              <input bind:value={keyQuery} placeholder="Find a shortcut" spellcheck="false" autocomplete="off" />
            </label>
            <span class="note">{listening ? "Press the new key · Esc cancels" : keyNotice || "Click a key to change it"}</span>
            <button class="ico" title="Put every shortcut back" aria-label="Put every shortcut back" onclick={resetKeys}><Icon name="undo" size={17} /></button>
          </div>
          <div class="keys">
            {#each shownKeys as a (a.id)}
              {@const keys = keysFor(a, overrides)}
              <button class="k" class:listening={listening === a.id} onclick={() => ((listening = listening === a.id ? null : a.id), (keyNotice = ""))}>
                <span>{keyLabel(a)}</span>
                <span class="cap">
                  {#if listening === a.id}
                    <kbd class="wait">…</kbd>
                  {:else if keys.length}
                    {#each pretty(keys[0]).split(/\+(?=.)/) as part, i (i)}{#if i}<span class="plus">+</span>{/if}<kbd>{part}</kbd>{/each}
                  {:else}
                    <span class="none">none</span>
                  {/if}
                </span>
              </button>
            {/each}
          </div>
          {#if !keyQuery.trim() && !allKeys}
            <button class="link more" onclick={() => (allKeys = true)}>{ACTIONS.length - COMMON.length} more</button>
          {/if}
        </section>
      </div>

      <div class="cols">
        <div>
          <section bind:this={sections.details}>
            <div class="caption">Details</div>
            <form class="row" onsubmit={(e) => (e.preventDefault(), cvKey.trim() && saveKey(cvKey))}>
              <div class="lbl">
                Comic Vine
                <small class="fine">free key at <button type="button" class="url" title="Open in your browser" onclick={() => api.cvOpenSite()}>comicvine.gamespot.com/api</button></small>
              </div>
              <div class="ctl">
                <span class="keyfield" class:on={cv?.hasKey}>
                  <Icon name="key" size={16} />
                  <input
                    class="code"
                    type="password"
                    bind:value={cvKey}
                    placeholder={cv?.hasKey ? "••••••••  saved" : "API key"}
                    autocomplete="off"
                    spellcheck="false"
                  />
                </span>
                {#if cvKey.trim()}
                  <button class="ico" title="Check and save" aria-label="Save key" disabled={cvBusy}>
                    <span class:spin={cvBusy}><Icon name={cvBusy ? "rescan" : "check"} size={17} /></span>
                  </button>
                {:else if cv?.hasKey}
                  <button type="button" class="ico" title="Remove key" aria-label="Remove key" disabled={cvBusy} onclick={() => saveKey("")}>
                    <Icon name="close" size={16} />
                  </button>
                {/if}
              </div>
            </form>
            {#if cvError}<p class="err"><Icon name="alert" size={14} />{cvError}</p>{/if}
            {#if cv?.books}
              <div class="meter">
                <div class="bar"><i style:width="{(cv.matched / cv.books) * 100}%"></i></div>
                <span class="fine">
                  {cv.matched.toLocaleString()} of {cv.books.toLocaleString()} books matched{#if cv.queued} · <span class="spin inline"><Icon name="rescan" size={11} /></span> {cv.queued.toLocaleString()} on the way{/if}
                </span>
                {#if cv.lastError}<span class="fine" title="Last problem"><Icon name="alert" size={11} /> {cv.lastError}</span>{/if}
              </div>
            {/if}
            <div class="row">
              <div class="lbl">Read details stored in the files<small>Writers, artists and dates inside each comic</small></div>
              {@render toggle(s.options.readFileDetails, () => setOption("readFileDetails", !s!.options.readFileDetails), "Read details stored in the files")}
            </div>
          </section>

          <!-- Reading progress, bookmarks, panel edits and metadata, copied once a day. -->
          <section bind:this={sections.backups}>
            <div class="caption">Backups</div>
            <div class="row">
              <div class="lbl">
                Last backup
                <small class="fine">{backups.length ? `${when(backups[0].madeAt)} · one a day` : "None yet"}</small>
              </div>
              <button class="ico" title="Back up now" aria-label="Back up now" disabled={backupBusy} onclick={backupNow}>
                <span class:spin={backupBusy && !confirming}><Icon name={backupBusy && !confirming ? "rescan" : "plus"} size={17} /></span>
              </button>
            </div>
            {#if backupError}<p class="err"><Icon name="alert" size={14} />{backupError}</p>{/if}
            {#if backups.length}
              <ul class="backups">
                {#each showAll ? backups : backups.slice(0, 3) as b (b.name)}
                  <li>
                    <span class="fine">{when(b.madeAt)}</span>
                    <span class="fine dim">{bytes(b.size)}</span>
                    {#if confirming === b.name}
                      <span class="ask">Restart with this backup?</span>
                      <button class="ico" title="Restore and restart" aria-label="Restore and restart" disabled={backupBusy} onclick={() => restore(b.name)}>
                        <span class:spin={backupBusy}><Icon name={backupBusy ? "rescan" : "check"} size={16} /></span>
                      </button>
                      <button class="ico" title="Cancel" aria-label="Cancel" disabled={backupBusy} onclick={() => (confirming = null)}><Icon name="close" size={15} /></button>
                    {:else}
                      <button class="ico" title="Restore this backup (it backs up first, so it can be undone)" aria-label="Restore this backup" onclick={() => (confirming = b.name)}>
                        <Icon name="undo" size={16} />
                      </button>
                    {/if}
                  </li>
                {/each}
              </ul>
              {#if backups.length > 3 && !showAll}<button class="link more" onclick={() => (showAll = true)}>{backups.length - 3} more</button>{/if}
            {/if}
            <div class="row">
              <div class="lbl">Keep<small>Older backups are removed</small></div>
              {@render choice(
                [
                  { key: 7, label: "7" },
                  { key: 10, label: "10" },
                  { key: 30, label: "30" },
                ],
                s.options.backupsKeep,
                (v) => setOption("backupsKeep", v),
                "Backups to keep",
              )}
            </div>
          </section>
        </div>

        <div>
          <section bind:this={sections.storage}>
            <div class="caption">Storage</div>
            <div class="meter big">
              <div class="used">
                <b>{storage ? bytes(used) : "…"}</b>
                <span class="fine">kept so books open instantly</span>
              </div>
              <div class="bar stack">
                {#each PARTS as p (p.label)}
                  {#if p.bytes}<i style:flex-grow={p.bytes} style:opacity={p.shade}></i>{/if}
                {/each}
              </div>
              <div class="legend fine">
                {#each PARTS as p (p.label)}
                  <span><b style:opacity={p.shade}></b>{p.label} {bytes(p.bytes)}</span>
                {/each}
              </div>
            </div>
            <div class="row">
              <div class="lbl">Keep pages of books you haven't opened<small>They come back when you open the book</small></div>
              {@render choice(
                [
                  { key: 7, label: "7 days" },
                  { key: 30, label: "30 days" },
                  { key: 90, label: "90 days" },
                ],
                s.options.cacheDays,
                (v) => setOption("cacheDays", v),
                "Keep pages for",
              )}
            </div>
            <div class="row">
              <div class="lbl">
                Clear saved pages
                <small>{storage ? `Frees ${bytes(storage.pages)}; ` : ""}books open a little slower the first time</small>
              </div>
              <button class="clear" disabled={clearing || !storage?.pages} onclick={clearPages}>
                {#if clearing}<span class="spin"><Icon name="rescan" size={14} /></span>{/if}Clear
              </button>
            </div>
          </section>
        </div>
      </div>

      <div class="about fine">
        AionComics {version} · Your data is kept in {s.dataDir}
        <button class="ico" title="Open this folder" aria-label="Open this folder" onclick={() => api.openDataFolder()}><Icon name="folder" size={14} /></button>
      </div>
    {/if}
  </div>

<style>
  .page {
    padding: 0 0 120px;
  }

  /* The heading in a frame, like a collection's name, with a jump to each section. */
  .top {
    position: relative;
    margin: 64px var(--pad) 0;
    padding: 40px 28px 26px;
    border: 4px solid var(--ink);
    background: var(--raised);
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px 32px;
  }
  /* A caption box, as on collection pages. */
  .top > .caption {
    position: absolute;
    left: -4px;
    top: -4px;
  }
  h1 {
    margin-top: 26px;
    font: 400 112px/0.82 var(--display);
    letter-spacing: 0.01em;
  }
  .jump {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .jump button {
    display: grid;
    justify-items: center;
    gap: 7px;
    width: 74px;
    padding: 10px 0 8px;
    color: var(--text-2);
    font-size: 12px;
    border-radius: 4px;
    transition:
      color 0.15s,
      background 0.15s;
  }
  .jump button:hover {
    color: var(--text);
  }
  .jump button.on {
    color: var(--text);
    background: rgb(var(--fg) / 0.07);
  }

  /* Two columns, each a stack, so a short section leaves no hole beside a long one. */
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(420px, 1fr));
    gap: 0 56px;
    margin: 0 var(--pad);
  }
  .wide {
    grid-column: 1 / -1;
  }
  section {
    margin-top: 44px;
    scroll-margin-top: 84px;
  }
  section > .caption {
    margin-bottom: 10px;
  }
  .range {
    flex: none;
    width: 160px;
    accent-color: var(--text);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    width: 100%;
    min-height: 62px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
    text-align: left;
  }
  .row.open {
    border-bottom: 0;
  }
  .row.book {
    justify-content: flex-start;
    gap: 14px;
    min-height: 0;
    padding: 8px 0;
  }
  .row.book .lbl {
    flex: 1;
  }
  .thumb {
    flex: none;
    width: 30px;
  }
  .row.go:hover .ico {
    color: var(--text);
    background: rgb(var(--fg) / 0.07);
  }
  .lbl {
    font-size: 14px;
    color: var(--text);
    min-width: 0;
  }
  .lbl small {
    display: block;
    margin-top: 4px;
    font-size: 12px;
    color: var(--text-3);
  }
  .lbl small.fine {
    font-size: 12px;
  }
  .path {
    font-size: 14px;
    user-select: text;
  }
  .ctl {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: none;
  }

  /* One of a few: a strip of buttons, the chosen one inked. */
  .seg {
    display: flex;
    flex: none;
    border: 1px solid var(--line);
    border-radius: 4px;
    overflow: hidden;
  }
  .seg button {
    height: 34px;
    min-width: 44px;
    padding: 0 13px;
    display: grid;
    place-items: center;
    color: var(--text-2);
    font-size: 13px;
    transition:
      color 0.15s,
      background 0.15s;
  }
  .seg button + button {
    border-left: 1px solid var(--line);
  }
  .seg button:hover:not(.on) {
    color: var(--text);
  }
  .seg button.on {
    background: var(--text);
    color: var(--bg);
  }

  .tog {
    flex: none;
    width: 38px;
    height: 22px;
    border-radius: 11px;
    background: rgb(var(--fg) / 0.14);
    position: relative;
    transition: background 0.2s;
  }
  .tog i {
    position: absolute;
    left: 3px;
    top: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-2);
    transition:
      left 0.2s var(--ease),
      background 0.2s;
  }
  .tog.on {
    background: var(--text);
  }
  .tog.on i {
    left: 19px;
    background: var(--bg);
  }

  .chips {
    display: flex;
    gap: 8px;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--line);
    border-radius: 16px;
    color: var(--text-2);
    font-size: 13px;
  }
  .chip:hover:not(.on) {
    color: var(--text);
  }
  .chip.on {
    background: var(--text);
    color: var(--bg);
    border-color: var(--text);
    padding-left: 10px;
  }

  /* Choices shown as small pictures of what they do. */
  .pics {
    display: flex;
    gap: 10px;
  }
  .pics.wide-row {
    gap: 14px;
    padding: 6px 0 16px;
    border-bottom: 1px solid var(--line);
  }
  .pic {
    display: grid;
    gap: 8px;
    justify-items: center;
    color: var(--text-2);
    font-size: 12px;
  }
  .pic:hover,
  .pic.on {
    color: var(--text);
  }
  .frame {
    display: block;
    position: relative;
    width: 132px;
    height: 82px;
    border-radius: 4px;
    overflow: hidden;
    box-shadow: 0 0 0 1px rgb(var(--fg) / 0.15);
    transition: box-shadow 0.15s;
  }
  .pic.small .frame {
    width: 84px;
    height: 56px;
  }
  .pic.on .frame {
    box-shadow: 0 0 0 2px var(--text);
  }
  .app {
    position: absolute;
    inset: 0;
  }
  .app b {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 24px;
  }
  .app u {
    position: absolute;
    left: 34px;
    top: 14px;
    width: 40px;
    height: 6px;
    border-radius: 1px;
  }
  .app em {
    position: absolute;
    top: 30px;
    width: 17px;
    height: 25px;
    border-radius: 1px;
  }
  /* Match Windows: the light half over the dark. */
  .half {
    position: absolute;
    inset: 0;
    clip-path: inset(0 0 0 50%);
  }
  .sheet em {
    position: absolute;
    left: 30px;
    top: 7px;
    width: 24px;
    height: 42px;
    background: #d8d2c6;
  }
  .panels {
    background: #000;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 3px;
    padding: 5px 16px;
  }
  .panels em {
    background: #d8d2c6;
  }

  /* Keyboard: every reader shortcut as a key cap. */
  .keybar {
    display: flex;
    align-items: center;
    gap: 24px;
    padding: 4px 0 10px;
    border-bottom: 1px solid var(--line);
  }
  .find {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-3);
    width: 320px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }
  .find:focus-within {
    border-color: var(--text-2);
  }
  .find input {
    flex: 1;
    background: none;
    border: 0;
    outline: none;
    color: var(--text);
    font-size: 13px;
    user-select: text;
  }
  .find input::placeholder {
    color: var(--text-3);
  }
  .note {
    flex: 1;
    font-size: 12px;
    color: var(--text-3);
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 0 40px;
  }
  .k {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    height: 46px;
    border-bottom: 1px solid var(--line);
    font-size: 13px;
    color: var(--text);
    text-align: left;
  }
  .k:hover kbd,
  .k.listening kbd {
    border-color: var(--text-2);
  }
  .cap {
    display: flex;
    align-items: center;
    flex: none;
  }
  kbd {
    display: inline-grid;
    place-items: center;
    min-width: 26px;
    height: 24px;
    padding: 0 7px;
    font: 400 12px var(--mono);
    color: var(--text);
    background: rgb(var(--fg) / 0.07);
    border: 1px solid var(--line);
    border-bottom-width: 3px;
    border-radius: 4px;
    transition: border-color 0.15s;
  }
  kbd.wait {
    min-width: 44px;
  }
  .plus {
    margin: 0 4px;
    color: var(--text-3);
    font-size: 12px;
  }
  .none {
    font-size: 12px;
    color: var(--text-3);
  }
  .more {
    margin-top: 12px;
    font-size: 13px;
  }

  /* Bars: Comic Vine's matching, and what the cache holds. */
  .meter {
    display: grid;
    gap: 10px;
    padding: 16px 0;
    border-bottom: 1px solid var(--line);
  }
  .meter > span {
    font-size: 12px;
    color: var(--text-3);
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .bar {
    height: 8px;
    border-radius: 4px;
    background: rgb(var(--fg) / 0.1);
    overflow: hidden;
    display: flex;
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--text);
  }
  .bar.stack {
    height: 14px;
    gap: 2px;
    background: none;
  }
  .bar.stack i {
    flex-basis: 0;
    min-width: 3px;
  }
  .bar.stack i:first-child {
    border-radius: 4px 0 0 4px;
  }
  .bar.stack i:last-child {
    border-radius: 0 4px 4px 0;
  }
  .used {
    display: flex;
    align-items: baseline;
    gap: 12px;
  }
  .used b {
    font: 400 34px/1 var(--display);
    letter-spacing: 0.02em;
  }
  .used span {
    font-size: 12px;
    color: var(--text-3);
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 22px;
    font-size: 12px;
    color: var(--text-2);
  }
  .legend span {
    white-space: nowrap;
  }
  .legend b {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    background: var(--text);
    margin-right: 7px;
  }
  .clear {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 16px;
    border: 1px solid var(--text-2);
    border-radius: 4px;
    color: var(--text);
    font-size: 13px;
    transition: background 0.15s;
  }
  .clear:hover:not(:disabled) {
    background: rgb(var(--fg) / 0.07);
  }
  .clear:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .keyfield {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 200px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
    color: var(--text-3);
  }
  .keyfield.on {
    color: var(--text-2);
  }
  .keyfield:focus-within {
    border-color: var(--text-2);
  }
  .keyfield input {
    flex: 1;
    min-width: 0;
    background: none;
    border: 0;
    outline: none;
    color: var(--text);
    font-size: 12px;
    user-select: text;
  }
  .keyfield input::placeholder {
    color: var(--text-3);
  }
  .url {
    color: var(--text-2);
    font: inherit;
    font-family: var(--mono);
    text-decoration: underline;
    text-decoration-color: var(--line);
    text-underline-offset: 3px;
    cursor: pointer;
    transition:
      color 0.2s,
      text-decoration-color 0.2s;
  }
  .url:hover {
    color: var(--text);
    text-decoration-color: currentColor;
  }

  .backups {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .backups li {
    display: flex;
    align-items: center;
    gap: 16px;
    height: 40px;
    font-size: 12px;
    border-bottom: 1px solid var(--line);
  }
  .backups .dim {
    color: var(--text-3);
    margin-right: auto;
  }
  .backups .ask {
    color: var(--text);
  }

  .about {
    margin: 56px var(--pad) 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-3);
    user-select: text;
  }
  .spin.inline {
    display: inline-grid;
  }
  .spin {
    display: grid;
  }
  .err {
    margin-top: 12px;
    color: var(--text);
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
</style>
