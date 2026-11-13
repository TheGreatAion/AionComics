<script lang="ts">
  import Icon from "./Icon.svelte";

  // An issue's covers, one at a time and big, with all of them in a row under it: ← and → or a
  // click on one move between them, Esc or a click beside it closes.
  let { covers, at = 0, title, onclose }: { covers: { small: string; large: string }[]; at?: number; title: string; onclose: () => void } = $props();
  let k = $state(0);
  $effect.pre(() => {
    k = at;
  });
  const go = (d: number) => (k = (k + d + covers.length) % covers.length);

  function onkey(e: KeyboardEvent) {
    const keys: Record<string, () => void> = { Escape: onclose, ArrowLeft: () => go(-1), ArrowRight: () => go(1) };
    const f = keys[e.key];
    if (!f) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    f();
  }
</script>

<svelte:window onkeydowncapture={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="viewer" role="dialog" aria-label="Covers" tabindex="-1" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <button class="ico close" title="Close (Esc)" aria-label="Close" onclick={onclose}><Icon name="close" size={18} /></button>
  <div class="stage">
    <button class="ico turn" title="Previous" aria-label="Previous cover" onclick={() => go(-1)}><Icon name="arrowLeft" size={22} /></button>
    <img class="big" src={covers[k].large} alt="Cover {k + 1}" />
    <button class="ico turn" title="Next" aria-label="Next cover" onclick={() => go(1)}><Icon name="arrowRight" size={22} /></button>
  </div>
  <div class="cap fine">{title} · cover {k + 1} of {covers.length}</div>
  <div class="strip">
    {#each covers as c, i (c.small)}
      <button class:on={i === k} onclick={() => (k = i)} aria-label="Cover {i + 1}"><img src={c.small} alt="" loading="lazy" /></button>
    {/each}
  </div>
</div>

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 160;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 18px;
    background: rgba(8, 8, 8, 0.92);
    backdrop-filter: blur(14px);
    animation: fade 0.2s var(--ease);
    color: #e8e8e8;
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  .close {
    position: absolute;
    top: 18px;
    right: 22px;
    color: #e8e8e8;
  }
  .stage {
    display: flex;
    align-items: center;
    gap: 24px;
    min-height: 0;
  }
  .turn {
    color: #e8e8e8;
  }
  .big {
    height: min(68vh, 900px);
    max-width: 70vw;
    object-fit: contain;
    border-radius: 3px;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    background: #1a1a1a;
  }
  .cap {
    color: #cfcfcf;
  }
  .strip {
    display: flex;
    gap: 10px;
    max-width: 90vw;
    overflow-x: auto;
    padding: 4px;
  }
  .strip button {
    flex: none;
    opacity: 0.55;
    transition: opacity 0.15s;
  }
  .strip button:hover,
  .strip button.on {
    opacity: 1;
  }
  .strip button.on img {
    outline: 2px solid #eee;
    outline-offset: 2px;
  }
  .strip img {
    height: 84px;
    width: auto;
    border-radius: 2px;
    display: block;
  }
</style>
