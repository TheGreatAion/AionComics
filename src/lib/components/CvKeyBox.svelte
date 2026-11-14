<script lang="ts">
  // No Comic Vine key yet: what a key adds to the whole library, a field to paste one into, and
  // once it's taken, the series you're on as the place to start. Opened from "Add details?" on a
  // series page; Settings has the same field for later.
  import { onMount } from "svelte";
  import { api, coverUrl } from "$lib/api";
  import Icon from "./Icon.svelte";

  let {
    name,
    cover,
    onclose,
    onfind,
  }: {
    name: string;
    /** A book from the series, for its cover. */
    cover?: number | null;
    onclose: () => void;
    /** The key is in: start with this series. */
    onfind: () => void;
  } = $props();

  let key = $state("");
  let busy = $state(false);
  let error = $state("");
  let done = $state(false);
  let box = $state<HTMLDivElement>();
  let input = $state<HTMLInputElement>();

  // Comic Vine keys are 40 letters and digits, so a whole one is checked as soon as it's pasted.
  const WHOLE = /^[0-9a-f]{40}$/i;

  onMount(() => input?.focus());

  async function save() {
    const k = key.trim();
    if (!k || busy) return;
    busy = true;
    error = "";
    try {
      await api.cvSetKey(k);
      done = true;
    } catch (e) {
      error = String(e).replace(/^Error:\s*/, "");
      if (/accept/i.test(error) && k.length < 40) error += " It looks cut short.";
    } finally {
      busy = false;
    }
  }
  function oninput() {
    error = "";
    if (WHOLE.test(key.trim())) save();
  }
  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }
  function outside(e: PointerEvent) {
    if (box && !box.contains(e.target as Node) && !(e.target as Element).closest?.(".cvkey-toggle")) onclose();
  }
</script>

<svelte:window onkeydowncapture={onkey} onpointerdown={outside} />

<div class="box" bind:this={box} role="dialog" aria-label="Add details to your library">
  <button class="ico x" title={done ? "Close" : "Not now"} aria-label={done ? "Close" : "Not now"} onclick={onclose}>
    <Icon name="close" size={15} />
  </button>
  {#if !done}
    <div class="t">Add details to your library</div>
    <p>Comic Vine can fill in the story, creators and characters for every series you have. It takes a free key, added once.</p>
    <label class="field" class:bad={!!error}>
      <Icon name="key" size={16} />
      <input
        bind:this={input}
        bind:value={key}
        class="code"
        type="password"
        placeholder="Paste your key"
        autocomplete="off"
        spellcheck="false"
        aria-label="Comic Vine key"
        disabled={busy}
        {oninput}
        onkeydown={(e) => e.key === "Enter" && save()}
      />
      {#if busy}
        <span class="spin"><Icon name="rescan" size={16} /></span>
      {:else if error}
        <Icon name="alert" size={16} />
      {:else if key.trim()}
        <button class="ico" title="Check and save" aria-label="Save key" onclick={save}><Icon name="check" size={16} /></button>
      {/if}
    </label>
    {#if busy}
      <div class="row">Checking with Comic Vine…</div>
    {:else if error}
      <div class="row err">{error}</div>
      <div class="row"><button class="link" onclick={() => api.cvOpenSite()}><Icon name="external" size={14} />Open your key page</button></div>
    {:else}
      <div class="row"><button class="link" onclick={() => api.cvOpenSite()}><Icon name="external" size={14} />Get a free key</button></div>
      <div class="fine">Sign up on Comic Vine, then copy the key from its API page and paste it here.</div>
    {/if}
  {:else}
    <div class="t"><Icon name="check" size={18} />You're set</div>
    <p>Every series can get details now. On any series, press <span class="inl"><Icon name="fetch" size={15} /></span> to add them.</p>
    <p class="fine">Books whose files already name their Comic Vine issue fill in by themselves.</p>
    <div class="here">
      {#if cover != null}<img src={coverUrl(cover)} alt="" />{/if}
      <div class="name"><span class="k">Start with this one</span><span class="n">{name}</span></div>
      <button class="go" onclick={onfind}>Add its details <Icon name="arrowRight" size={15} /></button>
    </div>
  {/if}
</div>

<style>
  .box {
    position: absolute;
    top: calc(100% + 10px);
    right: 0;
    z-index: 40;
    width: 392px;
    max-width: calc(100vw - 32px);
    padding: 18px 18px 16px;
    border-radius: 8px;
    background: var(--menu);
    box-shadow:
      0 0 0 1px var(--line),
      0 18px 44px rgba(0, 0, 0, 0.6);
    text-align: left;
    white-space: normal;
    animation: open 0.18s var(--ease);
  }
  @keyframes open {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  .x {
    position: absolute;
    top: 8px;
    right: 8px;
    color: var(--text-3);
  }
  .t {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    padding-right: 28px;
    color: var(--text);
    font-size: 16px;
    font-weight: 500;
  }
  p {
    margin: 0;
    padding-right: 26px;
    color: var(--text-2);
    font-size: 14px;
    line-height: 1.5;
  }
  .inl {
    display: inline-flex;
    vertical-align: -3px;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 36px;
    margin-top: 16px;
    border-bottom: 1px solid var(--line);
    color: var(--text-2);
  }
  .field:focus-within,
  .field.bad {
    border-color: var(--text-2);
  }
  .field input {
    flex: 1;
    min-width: 0;
    padding: 7px 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--text);
    font-size: 12px;
    user-select: text;
  }
  .field input::placeholder {
    color: var(--text-3);
    font-family: var(--font);
    font-size: 13px;
  }
  .field .ico {
    width: 28px;
    height: 28px;
  }
  .spin {
    display: inline-flex;
    animation: turn 0.9s linear infinite;
  }
  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    color: var(--text-2);
    font-size: 13px;
  }
  .row.err {
    color: var(--text);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--text-2);
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .link:hover {
    color: var(--text);
  }
  .fine {
    margin-top: 6px;
    color: var(--text-3);
    font-size: 12px;
    line-height: 1.5;
  }
  p.fine {
    margin-top: 8px;
    font-size: 13px;
  }
  .here {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 16px;
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }
  .here img {
    width: 40px;
    height: 60px;
    object-fit: cover;
    border-radius: 2px;
    box-shadow: 0 0 0 1px var(--line);
  }
  .name {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .k {
    color: var(--text-3);
    font-size: 12px;
  }
  .n {
    overflow: hidden;
    color: var(--text);
    font-size: 14px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .go {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-radius: 999px;
    background: var(--text);
    color: var(--bg);
    font-size: 13px;
  }
  .go:hover {
    opacity: 0.88;
  }
</style>
