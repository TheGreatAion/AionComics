<script lang="ts">
  import { lib, nav, search } from "$lib/state.svelte";
  import Icon from "./Icon.svelte";
  import { side, toggleSide } from "./SidePanel.svelte";
  import WindowControls from "./WindowControls.svelte";

  let scrolled = $state(false);
  const route = $derived(nav.current);
  const active = (id: number) => route.name === "collection" && route.id === id;
</script>

<svelte:window onscroll={() => (scrolled = window.scrollY > 24)} />

<!-- The window has no OS title bar: this bar is where you drag it from. -->
<header class:solid={scrolled} data-tauri-drag-region>
  <button class="ico panel" class:on={side.open} title="Side panel (Ctrl B)" aria-label="Side panel" onclick={toggleSide}>
    <Icon name="sidebar" size={17} />
  </button>
  {#if nav.canGoBack}
    <button class="back link" onclick={() => nav.back()} title="Back (Alt+←)">&larr;</button>
  {/if}
  <button class="mark" onclick={() => nav.root({ name: "home" })}>AionComics</button>
  <nav data-tauri-drag-region>
    <button class="link" class:on={route.name === "home"} onclick={() => nav.root({ name: "home" })}>Home</button>
    {#each lib.collections as c (c.id)}
      <button class="link" class:on={active(c.id)} onclick={() => nav.root({ name: "collection", id: c.id })}>
        {c.name}
      </button>
    {/each}
  </nav>
  <div class="right" data-tauri-drag-region>
    {#if lib.scanning}
      <span class="ico" title="Scanning your library"><span class="spin"><Icon name="rescan" size={16} /></span></span>
    {:else if lib.notice}
      <button class="ico on" title={lib.notice} aria-label={lib.noticeLabel} onclick={() => nav.root({ name: "settings" })}>
        <Icon name="alert" size={17} />
      </button>
    {/if}
    <button class="ico" title="Search (Ctrl K)" aria-label="Search" onclick={() => (search.open = true)}>
      <Icon name="search" size={17} />
    </button>
    <button class="ico" class:on={route.name === "stats"} title="Your reading" aria-label="Your reading" onclick={() => nav.root({ name: "stats" })}>
      <Icon name="stats" size={17} />
    </button>
    <button
      class="ico"
      class:on={route.name === "settings"}
      title="Settings"
      aria-label="Settings"
      onclick={() => nav.root({ name: "settings" })}
    >
      <Icon name="options" size={17} />
    </button>
  </div>
  <div class="win"><WindowControls /></div>
</header>

<style>
  header {
    position: fixed;
    inset: 0 0 auto 0;
    z-index: 20;
    height: 64px;
    display: flex;
    align-items: center;
    gap: 40px;
    /* Room on the right for the window buttons. */
    padding: 0 calc(var(--pad) + 120px) 0 var(--pad);
    border-bottom: 1px solid transparent;
    transition:
      background 0.4s,
      border-color 0.4s;
  }
  header.solid {
    background: rgba(10, 10, 10, 0.86);
    backdrop-filter: blur(24px);
    border-color: var(--line);
  }
  .win {
    position: absolute;
    top: 0;
    right: 0;
    height: 40px;
    display: flex;
  }
  /* Tucked in beside the name, closer than the bar's other gaps. */
  .panel {
    margin: 0 -30px 0 -14px;
  }
  .back {
    margin: 0 -28px 0 0;
    font-size: 16px;
  }
  .mark {
    font-weight: 500;
    font-size: 15px;
    letter-spacing: -0.01em;
  }
  nav,
  .right {
    display: flex;
    align-items: center;
    gap: 28px;
  }
  .right {
    margin-left: auto;
    gap: 6px;
  }

</style>
