<script module lang="ts">
  let count = 0;
</script>

<script lang="ts">
  import { coverUrl, printUrl } from "$lib/api";
  import { rgb } from "$lib/color";
  // A cover blown up to fill its panel and printed in a single ink: the art's shadows to its lights,
  // near-black to a deep shade of `hue`, the same on both themes (its panel takes the dark colors,
  // see .inked in app.css). With no hue (a gray cover, or one the ink would make teal) it prints in
  // plain gray. `at` is how far down the cover (0 to 100) the panel shows. The small cover shows
  // straight away, softly, and the sharp print takes over when it's ready, which may mean opening
  // the comic if it hasn't been made yet.
  let { id, hue, at = 50 }: { id: number; hue: number | null; at?: number } = $props();

  const filterId = `ink-${++count}`;
  let sharp = $state(false);

  const near = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const ink = $derived.by(() => {
    const [from, to] = [near("#12100e"), hue === null ? near("#4a4640") : rgb(hue, 0.46, 0.33)];
    return [0, 1, 2].map((i) => `${from[i].toFixed(3)} ${to[i].toFixed(3)}`);
  });
</script>

<div class="print" aria-hidden="true">
  <svg width="0" height="0">
    <filter id={filterId} color-interpolation-filters="sRGB">
      <feColorMatrix type="matrix" values=".3 .59 .11 0 0 .3 .59 .11 0 0 .3 .59 .11 0 0 0 0 0 1 0" />
      <feComponentTransfer>
        <feFuncR type="table" tableValues={ink[0]} />
        <feFuncG type="table" tableValues={ink[1]} />
        <feFuncB type="table" tableValues={ink[2]} />
      </feComponentTransfer>
    </filter>
  </svg>
  {#key id}
    <img class="soon" src={coverUrl(id)} alt="" style:object-position="50% {at}%" style:filter="url(#{filterId}) blur(3px)" />
    <img
      class="sharp"
      class:in={sharp}
      src={printUrl(id)}
      alt=""
      style:object-position="50% {at}%"
      style:filter="url(#{filterId})"
      onload={() => (sharp = true)}
    />
  {/key}
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
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  /* A touch larger, so its soft edge stays outside the panel. */
  .soon {
    transform: scale(1.02);
  }
  .sharp {
    opacity: 0;
    transition: opacity 0.35s;
  }
  .sharp.in {
    opacity: 1;
  }
</style>
