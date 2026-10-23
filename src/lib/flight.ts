// Shared-element transitions: a cover lifts off one screen and lands on the next.

type Box = { left: number; top: number; width: number; height: number };

const EASE = "cubic-bezier(.2,.7,0,1)";
export const FLIGHT_MS = 620;

let pending: { src: string; from: Box; key: string; at: number } | null = null;

/** Remember where a cover is before navigating; the matching cover on the next screen calls `land`. */
export function launch(img: HTMLImageElement | null | undefined, key: string) {
  if (!img || !img.complete || !img.naturalWidth) return;
  pending = { src: img.currentSrc || img.src, from: img.getBoundingClientRect(), key, at: performance.now() };
}

/** Fly the pending cover onto `target` if it was launched for this key. */
export function land(target: HTMLElement, key: string) {
  const p = pending;
  if (!p || p.key !== key || performance.now() - p.at > 2000) return;
  pending = null;
  const to = target.getBoundingClientRect();
  if (!visible(p.from) || !visible(to)) return;
  fly(p.src, p.from, to, { hide: [target] });
}

export function visible(r: Box) {
  return r.width > 0 && r.top + r.height > 0 && r.top < window.innerHeight;
}

/** Animate a copy of `src` from one box to another, hiding the real elements while it moves. */
export function fly(src: string, from: Box, to: Box, opts: { hide?: HTMLElement[]; done?: () => void } = {}) {
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
    duration: FLIGHT_MS,
    easing: EASE,
    fill: "forwards",
  });
  anim.onfinish = () => {
    hidden.forEach((el) => (el.style.visibility = ""));
    clone.remove();
    opts.done?.();
  };
}
