<script lang="ts">
  import { thumbUrl } from "$lib/api";
  import type { Ctl } from "./ctl.svelte";
  import { chapterName } from "./actions";

  let { c }: { c: Ctl } = $props();
  let strip = $state<HTMLDivElement>();
  const pages = $derived(Array.from({ length: c.count }, (_, i) => i));
  const starts = $derived(new Map(c.chapters.map((ch) => [ch.page, ch])));

  // Keep the current page in the middle of the strip.
  $effect(() => {
    const el = strip?.children[c.page] as HTMLElement | undefined;
    el?.scrollIntoView({ inline: "center", block: "nearest", behavior: "smooth" });
  });
</script>

<div class="flow" bind:this={strip} class:rtl={c.flipped} onwheel={(e) => strip && (strip.scrollLeft += e.deltaY)}>
  {#each pages as i (i)}
    {@const ch = starts.get(i)}
    <button class:on={c.shown.includes(i)} class:starts={!!ch} onclick={() => c.goTo(i)} title={ch ? `${chapterName(ch)}, page ${i + 1}` : `Page ${i + 1}`}>
      {#if starts.size}<span class="chmark fine">{ch ? `Ch. ${ch.number}` : ""}</span>{/if}
      <img src={thumbUrl(c.id, i)} alt="" loading="lazy" draggable="false" />
      <span class="fine">{i + 1}{#if c.bookmarks.includes(i)}&ensp;●{/if}</span>
    </button>
  {/each}
</div>

<style>
  .flow {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 56px;
    z-index: 4;
    display: flex;
    gap: 10px;
    padding: 14px 28px 10px;
    overflow-x: auto;
    scrollbar-width: none;
    background: rgba(14, 14, 14, 0.86);
    backdrop-filter: blur(24px) saturate(1.2);
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    animation: rise 0.3s var(--ease);
  }
  .flow::-webkit-scrollbar {
    display: none;
  }
  .flow.rtl {
    flex-direction: row-reverse;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(12px);
    }
  }
  button {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    opacity: 0.55;
    transition: opacity 0.2s;
  }
  button:hover,
  button.on {
    opacity: 1;
  }
  img {
    height: 120px;
    width: auto;
    min-width: 60px;
    background: #141414;
    border-radius: 2px;
    outline: 1px solid transparent;
    outline-offset: 2px;
  }
  button.on img {
    outline-color: var(--text);
  }
  .fine {
    color: var(--text-2);
    font-size: 12px;
  }
  /* Over the first page of each chapter; the same height empty elsewhere, so pages line up. */
  .chmark {
    align-self: stretch;
    min-height: 16px;
    padding-left: 6px;
    border-left: 1px solid transparent;
    color: var(--text);
    white-space: nowrap;
  }
  button.starts .chmark {
    border-left-color: var(--text-2);
  }
</style>
