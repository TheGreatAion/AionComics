<script lang="ts">
  import { thumbUrl } from "$lib/api";
  import Icon from "$lib/components/Icon.svelte";
  import type { Ctl } from "./ctl.svelte";

  let { c }: { c: Ctl } = $props();
</script>

<aside class="panel">
  <header>
    <h2>Bookmarks</h2>
    <button
      class="ico"
      class:on={c.bookmarks.includes(c.page)}
      title={c.bookmarks.includes(c.page) ? "Remove this page (Ctrl M)" : "Bookmark this page (Ctrl M)"}
      aria-label="Toggle bookmark"
      onclick={() => c.toggleBookmark()}
    >
      <Icon name={c.bookmarks.includes(c.page) ? "close" : "plus"} size={16} />
    </button>
  </header>
  {#if c.bookmarks.length}
    <div class="list">
      {#each c.bookmarks as p (p)}
        <button class:on={c.shown.includes(p)} onclick={() => c.goTo(p)}>
          <img src={thumbUrl(c.id, p)} alt="" loading="lazy" draggable="false" />
          <span class="mono">{p + 1}</span>
        </button>
      {/each}
    </div>
  {:else}
    <p class="empty"><Icon name="bookmark" size={22} stroke={1.2} /></p>
  {/if}
</aside>

<style>
  .panel {
    position: absolute;
    top: 64px;
    right: 20px;
    bottom: 84px;
    z-index: 5;
    width: 260px;
    display: flex;
    flex-direction: column;
    padding: 22px;
    border-radius: 10px;
    background: rgba(20, 20, 20, 0.92);
    backdrop-filter: blur(20px);
    box-shadow: 0 0 0 1px var(--line), 0 24px 60px rgba(0, 0, 0, 0.6);
    animation: slide 0.25s var(--ease);
  }
  @keyframes slide {
    from {
      opacity: 0;
      transform: translateX(12px);
    }
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: -6px -8px 14px 0;
  }
  h2 {
    font-weight: 400;
    font-size: 15px;
  }
  .list {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px 12px;
    align-content: start;
  }
  .list button {
    display: flex;
    flex-direction: column;
    gap: 6px;
    opacity: 0.7;
    transition: opacity 0.2s;
  }
  .list button:hover,
  .list button.on {
    opacity: 1;
  }
  img {
    width: 100%;
    border-radius: 2px;
    background: #141414;
  }
  span {
    font-size: 11px;
    color: var(--text-2);
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    color: var(--text-3);
  }
</style>
