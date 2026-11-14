// A number that counts up to its value when it first shows (Your reading's big numbers). Not with
// animations off; longer with them on Full.
import { look } from "./look.svelte";

export function countUp(node: HTMLElement, value: number) {
  const show = (v: number) => (node.textContent = v.toLocaleString());
  let raf = 0;
  if (look.motion !== "off" && value > 0) {
    const t0 = performance.now();
    const ms = look.motion === "full" ? 1100 : 700;
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / ms);
      show(Math.round(value * (1 - Math.pow(1 - k, 3))));
      if (k < 1) raf = requestAnimationFrame(step);
    };
    show(0);
    raf = requestAnimationFrame(step);
  }
  return {
    update(v: number) {
      cancelAnimationFrame(raf);
      value = v;
      show(v);
    },
    destroy() {
      cancelAnimationFrame(raf);
    },
  };
}
