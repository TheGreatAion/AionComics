<script lang="ts">
  import { untrack } from "svelte";
  import type { Ctl } from "./ctl.svelte";

  let { c }: { c: Ctl } = $props();
  let value = $state(untrack(() => String(c.page + 1)));
  let input = $state<HTMLInputElement>();

  $effect(() => {
    input?.focus();
    input?.select();
  });

  function go(e: SubmitEvent) {
    e.preventDefault();
    const n = parseInt(value, 10);
    if (Number.isFinite(n)) c.goTo(n - 1);
    c.overlay = null;
  }
</script>

<form class="goto" onsubmit={go}>
  <label for="goto-page">Go to page</label>
  <input
    id="goto-page"
    bind:this={input}
    bind:value
    inputmode="numeric"
    autocomplete="off"
    onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") c.overlay = null; }}
  />
  <span class="mono">of {c.count}</span>
</form>

<style>
  .goto {
    position: absolute;
    left: 50%;
    top: 38%;
    transform: translate(-50%, -50%);
    z-index: 6;
    display: flex;
    align-items: baseline;
    gap: 16px;
    padding: 22px 28px;
    border-radius: 10px;
    background: rgba(20, 20, 20, 0.92);
    backdrop-filter: blur(20px);
    box-shadow: 0 0 0 1px var(--line), 0 24px 60px rgba(0, 0, 0, 0.6);
    animation: pop 0.2s var(--ease);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translate(-50%, -46%);
    }
  }
  label {
    color: var(--text-2);
  }
  input {
    width: 90px;
    background: none;
    border: 0;
    border-bottom: 1px solid #444;
    font-weight: 300;
    font-size: 32px;
    letter-spacing: -0.03em;
    text-align: center;
    outline: none;
    user-select: text;
  }
  .mono {
    color: var(--text-3);
  }
</style>
