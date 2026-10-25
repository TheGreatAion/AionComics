<script lang="ts">
  // A dim wash of a book's own color behind the top of a page, fading into the dark. It glides
  // to the next color when the book changes. The page that holds it should isolate its stacking
  // (`isolation: isolate`) so the wash sits behind the content but above the page background.
  let {
    color,
    height = 460,
    strength = 0.44,
  }: { color: string | null | undefined; height?: number | string; strength?: number } = $props();

  // The cover's hue at an even brightness: a near-black cover would otherwise leave no wash at
  // all, and a bright yellow one would shout. Gray covers stay gray.
  const tone = $derived.by(() => {
    const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color ?? "");
    if (!m) return null;
    const [r, g, b] = [m[1], m[2], m[3]].map((h) => parseInt(h, 16) / 255);
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    const d = max - min;
    let h = 0;
    if (d) h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    const l = (max + min) / 2;
    const s = d ? d / (1 - Math.abs(2 * l - 1)) : 0;
    return `hsl(${Math.round(h * 60)} ${Math.round(Math.min(s, 0.75) * 100)}% ${Math.round(Math.min(Math.max(l, 0.42), 0.55) * 100)}%)`;
  });
</script>

<div class="wash" aria-hidden="true" style:height={typeof height === "number" ? `${height}px` : height} style:background-color={tone ?? "transparent"} style:opacity={tone ? strength : 0}></div>

<style>
  .wash {
    position: absolute;
    left: 0;
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
