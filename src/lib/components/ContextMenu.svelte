<script lang="ts">
  import { menu } from "$lib/state.svelte";

  let el = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });
  const close = () => (menu.open = false);
  const ticks = $derived(menu.items.some((i) => i !== "separator" && i.checked !== undefined));

  // Keep the menu on screen near the window edges.
  $effect(() => {
    if (!menu.open || !el) return;
    const r = el.getBoundingClientRect();
    pos = {
      x: Math.max(8, Math.min(menu.x, window.innerWidth - r.width - 8)),
      y: Math.max(8, Math.min(menu.y, window.innerHeight - r.height - 8)),
    };
  });

  function onkey(e: KeyboardEvent) {
    if (menu.open && e.key === "Escape") {
      e.stopImmediatePropagation();
      close();
    }
  }
</script>

<svelte:window onclick={close} onblur={close} onkeydowncapture={onkey} onresize={close} />

{#if menu.open}
  <div class="menu" bind:this={el} style:left="{pos.x || menu.x}px" style:top="{pos.y || menu.y}px" role="menu">
    {#each menu.items as item}
      {#if item === "separator"}
        <div class="sep"></div>
      {:else}
        <button
          role="menuitem"
          disabled={item.disabled}
          onclick={() => {
            close();
            item.action();
          }}
        >
          {#if ticks}<span class="tick">{item.checked ? "✓" : ""}</span>{/if}
          <span class="label">{item.label}</span>
          {#if item.hint}<span class="hint">{item.hint}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 200;
    min-width: 220px;
    max-height: calc(100vh - 16px);
    overflow-y: auto;
    padding: 6px;
    border-radius: 8px;
    background: var(--menu);
    box-shadow:
      0 0 0 1px var(--line),
      0 18px 44px rgba(0, 0, 0, 0.6);
    animation: open 0.18s var(--ease);
  }
  .menu::-webkit-scrollbar-thumb {
    border-color: var(--menu);
  }
  @keyframes open {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
  }
  button {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    text-align: left;
    padding: 6px 10px;
    border-radius: 5px;
    font-size: 13px;
    color: var(--cap);
  }
  button:hover:not(:disabled) {
    background: #262626;
    color: var(--text);
  }
  button:disabled {
    color: var(--text-3);
    cursor: default;
  }
  .tick {
    width: 10px;
    flex: none;
    font-size: 11px;
  }
  .label {
    flex: 1;
    white-space: nowrap;
  }
  .hint {
    margin-left: 24px;
    font-family: var(--mono);
    font-size: 11px;
    color: var(--text-3);
    white-space: nowrap;
  }
  .sep {
    height: 1px;
    margin: 5px 6px;
    background: var(--line);
  }
</style>
