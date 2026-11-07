// A cover flying from one place to another: a book opening into the reader and closing back.

type Box = { left: number; top: number; width: number; height: number };

import { motionMs } from "./look.svelte";

const EASE = "cubic-bezier(.2,.7,0,1)";
/** Closing starts gently and settles slowly, in step with the library fading in behind it. */
export const CLOSE_EASE = "cubic-bezier(.32,.08,.16,1)";
/** How long a cover takes to fly, for the animations chosen in Settings (0: they're off). */
export const flightMs = () => motionMs(360, 620);
/** How long a page takes to fly back into its cover, and how long the reader fades when it can't. */
export const closeMs = () => motionMs(460, 680);
export const fadeMs = () => motionMs(220, 340);

// The longest a flight waits for a picture to be ready before going ahead anyway.
const DECODE_WAIT = 120;
const LANDING_WAIT = 220;
const LANDING_FADE = 130;

export function visible(r: Box) {
  return r.width > 0 && r.top + r.height > 0 && r.top < window.innerHeight;
}

/** Whichever comes first: the picture being ready to draw, or the wait running out. */
function ready(img: HTMLImageElement, wait: number) {
  return Promise.race([img.decode().catch(() => {}), new Promise((r) => setTimeout(r, wait))]);
}

/**
 * Animate a copy of `src` from one box to another, hiding the real elements while it moves. With
 * `landing`, the copy keeps its shape on the way, lands at the height of that image, and melts into it
 * at the end rather than vanishing, so any small difference between the two doesn't show as a jump.
 */
export function fly(
  src: string,
  from: Box,
  to: Box,
  opts: { hide?: HTMLElement[]; done?: () => void; ms?: number; easing?: string; landing?: HTMLImageElement } = {},
) {
  const duration = opts.ms ?? flightMs();
  if (!duration) return void opts.done?.();
  // The copy is laid out at the larger of the two boxes and moved with a transform, which the GPU
  // animates on its own, smooth even while the next screen is still loading. Scaling down from the
  // larger box keeps it sharp the whole way; it would start blurry scaled up from the smaller.
  const base = from.width > to.width ? from : to;
  const at = (b: Box) =>
    `translate(${b.left - base.left}px, ${b.top - base.top}px) scale(${b.width / base.width}, ${b.height / base.height})`;
  // The same shape as the page it started as, centered on the box: a wide spread going into a narrow
  // space isn't squashed.
  const onto = (b: Box): Box => {
    if (!opts.landing) return b;
    const width = (from.width * b.height) / from.height;
    return { left: b.left + (b.width - width) / 2, top: b.top, width, height: b.height };
  };
  const clone = document.createElement("img");
  clone.src = src;
  Object.assign(clone.style, {
    position: "fixed",
    zIndex: "300",
    objectFit: "cover",
    pointerEvents: "none",
    borderRadius: "2px",
    left: `${base.left}px`,
    top: `${base.top}px`,
    width: `${base.width}px`,
    height: `${base.height}px`,
    transformOrigin: "0 0",
    transform: at(from),
    willChange: "transform",
  });
  document.body.appendChild(clone);
  const hidden = opts.hide ?? [];
  // The real picture stays up until its copy can be drawn, so nothing blinks at the start.
  ready(clone, DECODE_WAIT).then(() => {
    hidden.forEach((el) => (el.style.visibility = "hidden"));
    const end = onto(to);
    const anim = clone.animate([{ transform: at(from) }, { transform: at(end) }], {
      duration,
      easing: opts.easing ?? EASE,
      fill: "forwards",
    });
    anim.onfinish = async () => {
      const landing = opts.landing;
      if (landing?.isConnected) await ready(landing, LANDING_WAIT);
      hidden.forEach((el) => (el.style.visibility = ""));
      opts.done?.();
      if (!landing?.isConnected) return clone.remove();
      // The picture may have moved or changed shape while the copy was in the air: glide onto it.
      const last = onto(landing.getBoundingClientRect());
      clone.animate(
        [
          { opacity: 1, transform: at(end) },
          { opacity: 0, transform: at(last) },
        ],
        { duration: LANDING_FADE, easing: "ease-out", fill: "forwards" },
      ).onfinish = () => clone.remove();
    };
  });
}
