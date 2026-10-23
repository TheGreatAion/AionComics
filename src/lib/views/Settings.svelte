<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, type CvStatus, type Settings } from "$lib/api";
  import Icon from "$lib/components/Icon.svelte";
  import { lib } from "$lib/state.svelte";

  let { welcome = false }: { welcome?: boolean } = $props();
  let s = $state<Settings | null>(null);
  let cv = $state<CvStatus | null>(null);
  let cvKey = $state("");
  let cvBusy = $state(false);
  let cvError = $state("");
  let error = $state<string | null>(null);

  $effect(() => {
    void lib.version;
    void lib.scanning;
    api.settings().then((v) => (s = v));
    api.cvStatus().then((v) => (cv = v));
  });

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

  function rescan() {
    lib.scanning = true;
    lib.notice = "";
    api.rescan();
  }

  const when = (t: number) =>
    new Date(t * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
</script>

{#if welcome}
  <div class="welcome">
    <h1>AionComics</h1>
    <p>Pick the folder with your comics. It's only ever read.</p>
    <button class="btn" onclick={choose}><Icon name="folder" size={16} />Choose folder</button>
    {#if error}<p class="err">{error}</p>{/if}
  </div>
{:else}
  <div class="page">
    <h1>Settings</h1>
    {#if s}
      <section>
        <h2>Library</h2>
        <div class="row">
          <span class="path mono" title="Read-only: AionComics never writes here">{s.libraryRoot ?? "Not set"}</span>
          <button class="ico" title="Change folder" aria-label="Change folder" onclick={choose}><Icon name="folder" size={17} /></button>
          <button class="ico" title="Rescan" aria-label="Rescan" disabled={lib.scanning} onclick={rescan}>
            <span class:spin={lib.scanning}><Icon name="rescan" size={17} /></span>
          </button>
        </div>
        {#if error}<p class="err"><Icon name="alert" size={14} />{error}</p>{/if}
        {#if lib.notice}<p class="err"><Icon name="alert" size={14} />{lib.notice}</p>{/if}
        {#if s.lastScan}
          <p class="stats mono">
            {s.lastScan.comics.toLocaleString()} books · {s.lastScan.series.toLocaleString()} series · {when(s.lastScan.finishedAt)}
          </p>
        {/if}
      </section>

      <section>
        <h2>Comic Vine</h2>
        <form class="row" onsubmit={(e) => (e.preventDefault(), cvKey.trim() && saveKey(cvKey))}>
          <span class="lead" class:on={cv?.hasKey} title={cv?.hasKey ? "Key saved" : "No key yet"}><Icon name="key" size={17} /></span>
          <input
            class="key mono"
            type="password"
            bind:value={cvKey}
            placeholder={cv?.hasKey ? "••••••••  saved" : "API key"}
            autocomplete="off"
            spellcheck="false"
          />
          <button class="ico" title="Check and save" aria-label="Save key" disabled={cvBusy || !cvKey.trim()}>
            <span class:spin={cvBusy}><Icon name={cvBusy ? "rescan" : "check"} size={17} /></span>
          </button>
          {#if cv?.hasKey}
            <button type="button" class="ico" title="Remove key" aria-label="Remove key" disabled={cvBusy} onclick={() => saveKey("")}>
              <Icon name="close" size={16} />
            </button>
          {/if}
        </form>
        {#if cvError}<p class="err"><Icon name="alert" size={14} />{cvError}</p>{/if}
        <p class="stats mono">
          {#if cv?.queued}<span title="Credits and characters still coming from Comic Vine"><span class="spin inline"><Icon name="rescan" size={12} /></span> {cv.queued.toLocaleString()} queued</span> · {/if}
          free key at <span class="url">comicvine.gamespot.com/api</span>
        </p>
        {#if cv?.lastError}<p class="stats mono" title="Last problem"><Icon name="alert" size={12} /> {cv.lastError}</p>{/if}
      </section>
    {/if}
  </div>
{/if}

<style>
  .welcome {
    max-width: 520px;
    padding: 24vh var(--pad) 0;
    margin: 0 auto;
  }
  .welcome h1,
  .page h1 {
    font-weight: 300;
    font-size: clamp(56px, 7vw, 96px);
    line-height: 0.9;
    letter-spacing: -0.05em;
  }
  .welcome p {
    margin: 24px 0 36px;
    color: var(--text-2);
    font-size: 15px;
  }
  .welcome .btn {
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
  .page {
    padding: 160px var(--pad) 140px;
    max-width: 900px;
  }
  section {
    margin-top: 64px;
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
    margin-bottom: 16px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .path {
    color: var(--text);
    font-size: 13px;
    margin-right: 12px;
    user-select: text;
  }
  .lead {
    color: var(--text-3);
    display: grid;
    place-items: center;
    width: 24px;
  }
  .lead.on {
    color: var(--text);
  }
  .key {
    width: 360px;
    margin: 0 8px;
    background: none;
    border: 0;
    border-bottom: 1px solid var(--line);
    padding: 6px 0;
    color: var(--text);
    outline: none;
    user-select: text;
  }
  .key:focus {
    border-color: var(--text-2);
  }
  .key::placeholder {
    color: var(--text-3);
  }
  .stats {
    margin-top: 14px;
    color: var(--text-3);
    font-size: 11px;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .stats > span {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .url {
    color: var(--text-2);
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
