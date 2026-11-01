// A cover flying from one place to another: a book opening into the reader and closing back.

type Box = { left: number; top: number; width: number; height: number };

import { motionMs } from "./look.svelte";

const EASE = "cubic-bezier(.2,.7,0,1)";
/** How long a cover takes to fly, for the animations chosen in Settings (0: they're off). */
export const flightMs = () => motionMs(360, 620);

export function visible(r: Box) {
  return r.width > 0 && r.top + r.height > 0 && r.top < window.innerHeight;
}

/** Animate a copy of `src` from one box to another, hiding the real elements while it moves. */
export function fly(src: string, from: Box, to: Box, opts: { hide?: HTMLElement[]; done?: () => void } = {}) {
  const duration = flightMs();
  if (!duration) return void opts.done?.();
  // The copy sits at its destination and is transformed back to where it started, so the move is
  // a transform the GPU animates on its own, smooth even while the next screen is still loading.
  const clone = document.createElement("img");
  clone.src = src;
  Object.assign(clone.style, {
    position: "fixed",
    zIndex: "300",
    objectFit: "cover",
    pointerEvents: "none",
    borderRadius: "2px",
    left: `${to.left}px`,
    top: `${to.top}px`,
    width: `${to.width}px`,
    height: `${to.height}px`,
    transformOrigin: "0 0",
    willChange: "transform",
  });
  document.body.appendChild(clone);
  const hidden = opts.hide ?? [];
  hidden.forEach((el) => (el.style.visibility = "hidden"));
  const start = `translate(${from.left - to.left}px, ${from.top - to.top}px) scale(${from.width / to.width}, ${from.height / to.height})`;
  const anim = clone.animate([{ transform: start }, { transform: "none" }], {
    duration,
    easing: EASE,
    fill: "forwards",
  });
  anim.onfinish = () => {
    hidden.forEach((el) => (el.style.visibility = ""));
    clone.remove();
    opts.done?.();
  };
}
