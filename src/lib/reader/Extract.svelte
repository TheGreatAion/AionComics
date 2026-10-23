<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "$lib/components/Icon.svelte";
  import type { Ctl } from "./ctl.svelte";

  let { c }: { c: Ctl } = $props();
  // Start from the pages on screen when the box opened.
  let from = $state(untrack(() => c.page + 1));
  let to = $state(untrack(() => c.furthest + 1));
  const valid = $derived(from >= 1 && to >= from && to <= c.count);
</script>

<form
  class="box"
  onsubmit={(e) => {
    e.preventDefault();
    if (valid) c.extract(from - 1, to - 1);
  }}
>
  <header>
    <h2>Extract pages</h2>
    <button type="button" class="ico" title="Close (Esc)" aria-label="Close" onclick={() => (c.overlay = null)}><Icon name="close" size={16} /></button>
  </header>
  <div class="row">
    <input type="number" min="1" max={c.count} bind:value={from} aria-label="From" onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") c.overlay = null; }} />
    <span>–</span>
    <input type="number" min="1" max={c.count} bind:value={to} aria-label="To" onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") c.overlay = null; }} />
    <span class="mono">/ {c.count}</span>
  </div>
  <div class="quick">
    <button type="button" class="link" onclick={() => ((from = c.page + 1), (to = c.furthest + 1))}>This page</button>
    <button type="button" class="link" onclick={() => ((from = 1), (to = c.count))}>All</button>
  </div>
  <div class="actions">
    <button class="btn" title="Choose a folder" disabled={!valid}><Icon name="folder" size={15} />Save to…</button>
  </div>
</form>

<style>
  .box {
    position: absolute;
    left: 50%;
    top: 40%;
    transform: translate(-50%, -50%);
    z-index: 6;
    width: 380px;
    padding: 26px 28px;
    border-radius: 10px;
    background: rgba(20, 20, 20, 0.92);
    backdrop-filter: blur(20px);
    box-shadow: 0 0 0 1px var(--line), 0 24px 60px rgba(0, 0, 0, 0.6);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: -8px -12px 0 0;
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
  }
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 9px;
  }
  .row {
    margin-top: 22px;
    display: flex;
    align-items: baseline;
    gap: 18px;
    color: var(--text-2);
  }
  input {
    width: 64px;
    color: var(--text);
    background: none;
    border: 0;
    border-bottom: 1px solid #444;
    font-size: 18px;
    text-align: center;
    outline: none;
    user-select: text;
  }
  input::-webkit-inner-spin-button {
    appearance: none;
  }
  .mono {
    color: var(--text-3);
  }
  .quick {
    margin-top: 14px;
    display: flex;
    gap: 20px;
    font-size: 13px;
  }
  .actions {
    margin-top: 28px;
    display: flex;
    align-items: center;
    gap: 24px;
  }
  .btn:disabled {
    opacity: 0.4;
  }
</style>
