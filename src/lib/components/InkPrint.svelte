<script module lang="ts">
  let count = 0;
</script>

<script lang="ts">
  import { rgb } from "$lib/color";
  import { theme } from "$lib/theme.svelte";
  // A cover blown up to fill its panel and printed in a single ink: the art's shadows to its lights.
  // Near-black to a deep shade of `hue` on the dark theme; a soft shade of it to paper on the light
  // one. With no hue (a gray cover, or one the ink would make teal) it prints in plain gray.
  // `at` is how far down the cover (0 to 100) the panel shows.
  let { src, hue, at = 50 }: { src: string; hue: number | null; at?: number } = $props();

  const id = `ink-${++count}`;
  const near = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const ink = $derived.by(() => {
    const [from, to] =
      theme.current === "light"
        ? [hue === null ? near("#a9a397") : rgb(hue, 0.38, 0.66), near("#e4ddd0")]
        : [near("#12100e"), hue === null ? near("#4a4640") : rgb(hue, 0.46, 0.33)];
    return [0, 1, 2].map((i) => `${from[i].toFixed(3)} ${to[i].toFixed(3)}`);
  });
</script>

<div class="print" aria-hidden="true">
  <svg width="0" height="0">
    <filter {id} color-interpolation-filters="sRGB">
      <feColorMatrix type="matrix" values=".3 .59 .11 0 0 .3 .59 .11 0 0 .3 .59 .11 0 0 0 0 0 1 0" />
      <feComponentTransfer>
        <feFuncR type="table" tableValues={ink[0]} />
        <feFuncG type="table" tableValues={ink[1]} />
        <feFuncB type="table" tableValues={ink[2]} />
      </feComponentTransfer>
    </filter>
  </svg>
  <img {src} alt="" style:object-position="50% {at}%" style:filter="url(#{id})" />
</div>

<style>
  .print {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
  }
  svg {
    position: absolute;
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
