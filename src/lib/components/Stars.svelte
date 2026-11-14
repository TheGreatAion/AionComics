<script lang="ts">
  // Five stars: your rating of something, and when `onrate` is given, a way to set it. Hovering
  // shows what a click would give (`preview`); clicking the stars it already has takes it away.
  let {
    value,
    size = 16,
    gap = 3,
    onrate,
    preview = $bindable(null),
    label = "Your rating",
  }: {
    /** 1 to 5; an average shows rounded to whole stars. */
    value: number | null | undefined;
    size?: number;
    gap?: number;
    onrate?: (stars: number) => void;
    preview?: number | null;
    label?: string;
  } = $props();

  const STAR = "M12 3.6l2.55 5.2 5.75.84-4.16 4.05.98 5.72L12 16.7l-5.12 2.7.98-5.72L3.7 9.64l5.75-.84z";
  const shown = $derived(preview ?? (value ? Math.round(value) : 0));
</script>

{#if onrate}
  <span class="stars" role="radiogroup" tabindex="-1" aria-label={label} style:gap="{gap}px" onpointerleave={() => (preview = null)}>
    {#each [1, 2, 3, 4, 5] as n (n)}
      <button
        class="star"
        class:on={n <= shown}
        role="radio"
        aria-checked={value === n}
        aria-label="{n} {n === 1 ? 'star' : 'stars'}"
        title={value === n ? "Take the rating away" : undefined}
        onpointerenter={() => (preview = n)}
        onfocus={() => (preview = n)}
        onblur={() => (preview = null)}
        onclick={() => onrate(n)}
      >
        <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true"><path d={STAR} /></svg>
      </button>
    {/each}
  </span>
{:else}
  <span class="stars" role="img" aria-label="{label}: {value ? (Math.round(value * 10) / 10).toString() : 'none'} of 5" style:gap="{gap}px">
    {#each [1, 2, 3, 4, 5] as n (n)}
      <span class="star" class:on={n <= shown}>
        <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true"><path d={STAR} /></svg>
      </span>
    {/each}
  </span>
{/if}

<style>
  .stars {
    display: inline-flex;
    align-items: center;
    vertical-align: middle;
  }
  .star {
    display: inline-flex;
    padding: 0;
    color: var(--star-off, var(--text-3));
    line-height: 0;
  }
  button.star {
    padding: 2px;
    margin: -2px;
    border-radius: 3px;
    transition: transform 0.15s var(--ease);
  }
  button.star:hover {
    transform: scale(1.12);
  }
  .star.on {
    color: var(--star-on, var(--text));
  }
  svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linejoin: round;
  }
  .on svg {
    fill: currentColor;
  }
</style>
