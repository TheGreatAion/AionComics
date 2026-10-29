<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import Icon from "./Icon.svelte";

  // The window has no OS title bar, so the app draws minimize, maximize and close itself.
  const win = getCurrentWindow();
  let maximized = $state(false);

  onMount(() => {
    const sync = () => win.isMaximized().then((m) => (maximized = m));
    sync();
    const off = win.onResized(sync);
    return () => void off.then((un) => un());
  });
</script>

<div class="controls">
  <button onclick={() => win.minimize()} title="Minimize" aria-label="Minimize"><Icon name="minimize" size={14} stroke={1.2} /></button>
  <button onclick={() => win.toggleMaximize()} title={maximized ? "Restore" : "Maximize"} aria-label={maximized ? "Restore" : "Maximize"}>
    <Icon name={maximized ? "restore" : "maximize"} size={14} stroke={1.2} />
  </button>
  <button class="close" onclick={() => win.close()} title="Close" aria-label="Close"><Icon name="close" size={14} stroke={1.2} /></button>
</div>

<style>
  .controls {
    display: flex;
    align-self: stretch;
  }
  button {
    width: 46px;
    display: grid;
    place-items: center;
    color: var(--text-2);
    transition:
      background 0.15s,
      color 0.15s;
  }
  button:hover {
    background: rgb(var(--fg) / 0.08);
    color: var(--text);
  }
  .close:hover {
    background: rgb(var(--fg) / 0.16);
  }
</style>
