<script lang="ts">
  import { menu, type MenuItem } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";

  let el = $state<HTMLDivElement>();
  let subEl = $state<HTMLDivElement>();
  let pos = $state({ x: 0, y: 0 });
  // The list open beside an item: which one, and where.
  let sub = $state<{ at: number; items: MenuItem[]; row: DOMRect; x: number; y: number } | null>(null);
  const close = () => (menu.open = false);
  const ticked = (items: MenuItem[]) => items.some((i) => i !== "separator" && i.checked !== undefined);

  // Keep the menu on screen near the window edges.
  $effect(() => {
    if (!menu.open || !el) return;
    const r = el.getBoundingClientRect();
    pos = {
      x: Math.max(8, Math.min(menu.x, window.innerWidth - r.width - 8)),
      y: Math.max(8, Math.min(menu.y, window.innerHeight - r.height - 8)),
    };
  });
  $effect(() => {
    if (!menu.open) sub = null;
  });

  // Beside the menu on the right, or on the left when there's no room; lined up with its item.
  $effect(() => {
    if (!sub || !subEl || !el) return;
    const m = el.getBoundingClientRect();
    const s = subEl.getBoundingClientRect();
    const x = m.right + s.width + 2 <= window.innerWidth - 8 ? m.right + 2 : m.left - s.width - 2;
    const y = Math.max(8, Math.min(sub.row.top - 6, window.innerHeight - s.height - 8));
    if (x !== sub.x || y !== sub.y) sub = { ...sub, x, y };
  });

  function openSub(at: number, items: MenuItem[], row: HTMLElement) {
    if (sub?.at !== at) sub = { at, items, row: row.getBoundingClientRect(), x: -9999, y: 0 };
  }

  function onkey(e: KeyboardEvent) {
    if (menu.open && e.key === "Escape") {
      e.stopImmediatePropagation();
      if (sub) sub = null;
      else close();
    }
  }
</script>

<svelte:window onclick={close} onblur={close} onkeydowncapture={onkey} onresize={close} />

{#snippet row(item: Exclude<MenuItem, "separator">, ticks: boolean, at: number | null)}
  <button
    role="menuitem"
    class:open={at !== null && sub?.at === at}
    disabled={item.disabled}
    aria-haspopup={item.items ? "menu" : undefined}
    aria-expanded={item.items ? sub?.at === at : undefined}
    onpointerenter={(e) => {
      if (at === null) return;
      if (item.items) openSub(at, item.items, e.currentTarget);
      else sub = null;
    }}
    onclick={(e) => {
      if (item.items) {
        e.stopPropagation();
        if (at !== null) openSub(at, item.items, e.currentTarget);
        return;
      }
      close();
      item.action?.();
    }}
  >
    {#if ticks}<span class="tick">{item.checked ? "✓" : ""}</span>{/if}
    <span class="label">{item.label}</span>
    {#if item.hint}<span class="hint" class:val={!!item.items}>{item.hint}</span>{/if}
    {#if item.items}<span class="more"><Icon name="chevronRight" size={13} stroke={2} /></span>{/if}
  </button>
{/snippet}

{#if menu.open}
  <div class="menu" bind:this={el} style:left="{pos.x || menu.x}px" style:top="{pos.y || menu.y}px" role="menu">
    {#each menu.items as item, i}
      {#if item === "separator"}
        <div class="sep"></div>
      {:else}
        {@render row(item, ticked(menu.items), i)}
      {/if}
    {/each}
  </div>
  {#if sub}
    <div class="menu sub" bind:this={subEl} style:left="{sub.x}px" style:top="{sub.y}px" role="menu">
      {#each sub.items as item}
        {#if item === "separator"}
          <div class="sep"></div>
        {:else}
          {@render row(item, ticked(sub.items), null)}
        {/if}
      {/each}
    </div>
  {/if}
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
  .menu.sub {
    z-index: 201;
    min-width: 180px;
    animation: none;
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
  button:hover:not(:disabled),
  button.open {
    background: rgb(var(--fg) / 0.08);
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
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
  }
  .hint.val {
    margin-left: 16px;
    font-family: inherit;
    font-size: 12px;
  }
  .more {
    align-self: center;
    display: flex;
    margin-right: -4px;
    color: var(--text-3);
  }
  .sep {
    height: 1px;
    margin: 5px 6px;
    background: var(--line);
  }
</style>
