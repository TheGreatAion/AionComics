<script lang="ts">
  // A series with no details yet, and no Comic Vine key to get them with: says what a key is for and
  // takes one right here, once, until it's dismissed. Settings has the same field for later.
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import Icon from "./Icon.svelte";

  const SEEN = "aion.cvNudge";
  let { onsaved }: { onsaved: () => void } = $props();

  let show = $state(false);
  let key = $state("");
  let busy = $state(false);
  let error = $state("");

  onMount(() => {
    let dismissed = false;
    try {
      dismissed = localStorage.getItem(SEEN) === "1";
    } catch {
      // Shown again; no harm done.
    }
    if (!dismissed) api.cvStatus().then((s) => (show = !s.hasKey));
  });

  function dismiss() {
    show = false;
    try {
      localStorage.setItem(SEEN, "1");
    } catch {
      // Not remembered.
    }
  }
  async function save() {
    if (!key.trim() || busy) return;
    busy = true;
    error = "";
    try {
      await api.cvSetKey(key.trim());
      show = false;
      onsaved();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if show}
  <aside class="nudge" aria-label="Comic Vine">
    <div class="say">
      <p>Story, creators and characters for your books can come from Comic Vine, with a free key.</p>
      <div class="get">
        <input bind:value={key} placeholder="Paste your key" spellcheck="false" autocomplete="off" onkeydown={(e) => e.key === "Enter" && save()} />
        <button class="link" disabled={!key.trim() || busy} onclick={save}>{busy ? "Checking…" : "Save"}</button>
        <span class="or">·</span>
        <button class="link" onclick={() => api.cvOpenSite()}>Get a free key</button>
      </div>
      {#if error}<p class="err">{error}</p>{/if}
    </div>
    <button class="ico" title="Not now" aria-label="Not now" onclick={dismiss}><Icon name="close" size={15} /></button>
  </aside>
{/if}

<style>
  .nudge {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 20px;
    margin: 16px var(--pad) 0;
    padding: 14px 10px 14px 18px;
    border-radius: 10px;
    box-shadow: inset 0 0 0 1px var(--line);
  }
  p {
    margin: 0;
    color: var(--text-2);
    font-size: 14px;
  }
  .get {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
    margin-top: 10px;
    font-size: 13px;
  }
  input {
    width: 260px;
    max-width: 100%;
    padding-bottom: 4px;
    border: 0;
    border-bottom: 1px solid var(--line);
    background: none;
    color: var(--text);
    font-family: var(--mono);
    font-size: 12px;
  }
  input:focus-visible {
    outline: none;
    border-color: var(--text-2);
  }
  .or {
    color: var(--text-3);
  }
  .err {
    margin-top: 8px;
    font-size: 13px;
  }
</style>
