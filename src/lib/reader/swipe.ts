// Reading on a phone or tablet: a quick sideways flick of a finger turns the page (or steps through
// the panels) as tapping that side of the screen does, so it follows the reading direction too.

export type Side = "left" | "right";

export function swiper(turn: (side: Side) => void) {
  let start: { x: number; y: number; t: number; id: number } | null = null;
  return {
    down(e: PointerEvent) {
      if (e.pointerType !== "touch") return;
      // A second finger is a pinch, not a flick.
      start = e.isPrimary ? { x: e.clientX, y: e.clientY, t: performance.now(), id: e.pointerId } : null;
    },
    /** Whether that was a flick, and the page was turned. `allow`: whether a flick that way turns
     *  now (not while there's more of a zoomed-in page to see on that side). */
    up(e: PointerEvent, allow: (side: Side) => boolean = () => true): boolean {
      const s = start;
      start = null;
      if (!s || e.pointerId !== s.id) return false;
      const [dx, dy] = [e.clientX - s.x, e.clientY - s.y];
      if (Math.abs(dx) < 50 || Math.abs(dx) < Math.abs(dy) * 1.5 || performance.now() - s.t > 800) return false;
      // Finger to the left brings in what's on the right.
      const side: Side = dx < 0 ? "right" : "left";
      if (!allow(side)) return false;
      turn(side);
      return true;
    },
    cancel() {
      start = null;
    },
  };
}
