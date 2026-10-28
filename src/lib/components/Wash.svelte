<script lang="ts">
  import { tone } from "$lib/color";
  // A dim wash of a book's own color behind the top of a page, fading into the dark. It glides
  // to the next color when the book changes. The page that holds it should isolate its stacking
  // (`isolation: isolate`) so the wash sits behind the content but above the page background.
  let {
    color,
    height = 460,
    strength = 0.44,
  }: { color: string | null | undefined; height?: number | string; strength?: number } = $props();

  // The cover's hue at an even brightness (see `tone`).
  const shade = $derived(tone(color));
</script>

<div class="wash" aria-hidden="true" style:height={typeof height === "number" ? `${height}px` : height} style:background-color={shade ?? "transparent"} style:opacity={shade ? strength : 0}></div>

<style>
  .wash {
    position: absolute;
    /* On under the side panel, whose glass lets it show through. */
    left: calc(-1 * var(--side-now, 0px));
    right: 0;
    top: 0;
    z-index: -1;
    pointer-events: none;
    -webkit-mask-image: linear-gradient(to bottom, #000 0%, rgba(0, 0, 0, 0.55) 45%, transparent 100%);
    mask-image: linear-gradient(to bottom, #000 0%, rgba(0, 0, 0, 0.55) 45%, transparent 100%);
    transition:
      background-color 0.9s var(--ease),
      opacity 0.9s var(--ease);
  }
</style>
