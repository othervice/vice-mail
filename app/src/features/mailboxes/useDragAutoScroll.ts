import { useEffect, useRef, type DragEvent } from "react";
import { isOwnDrag } from "../../stores/drag";

/** How close to the column's top or bottom edge a drag starts scrolling it, in px. */
export const AUTO_SCROLL_EDGE = 40;
/** The fastest it scrolls, in px per frame (right at the edge). */
const MAX_SPEED = 14;

/**
 * Pixels per frame for a drag at `y` in a scrolling area from `top` to `bottom`: negative near the
 * top edge, positive near the bottom, faster the closer to the edge, 0 elsewhere.
 */
export function autoScrollSpeed(y: number, top: number, bottom: number): number {
  const fromTop = y - top;
  const fromBottom = bottom - y;
  const speed = (distance: number) => Math.max(1, Math.round((MAX_SPEED * (AUTO_SCROLL_EDGE - Math.max(0, distance))) / AUTO_SCROLL_EDGE));
  if (fromTop < AUTO_SCROLL_EDGE) return -speed(fromTop);
  if (fromBottom < AUTO_SCROLL_EDGE) return speed(fromBottom);
  return 0;
}

/** The column's scrolling during one drag. */
interface Scrolling {
  node: HTMLElement | null;
  speed: number;
  /** When the last `dragover` came (ms). */
  last: number;
  frame: number | null;
}

/** One frame: scrolls while `dragover` keeps coming, stops when it stopped (left, ended, outside the edge). */
function step(scrolling: Scrolling) {
  if (!scrolling.node || scrolling.speed === 0 || Date.now() - scrolling.last > 200) {
    scrolling.frame = null;
    return;
  }
  scrolling.node.scrollTop += scrolling.speed;
  scrolling.frame = requestAnimationFrame(() => {
    step(scrolling);
  });
}

/**
 * Stage 22 (decision 0081): the left column scrolls by itself while dragged rows (or an account)
 * are near its top or bottom edge, so every folder can be reached in one drag. The handler goes on
 * the scrolling element's `onDragOver`; it scrolls on every frame while `dragover` keeps coming.
 */
export function useDragAutoScroll(): (event: DragEvent<HTMLElement>) => void {
  const state = useRef<Scrolling>({ node: null, speed: 0, last: 0, frame: null });

  useEffect(() => {
    const scrolling = state.current;
    return () => {
      if (scrolling.frame !== null) cancelAnimationFrame(scrolling.frame);
    };
  }, []);

  return (event) => {
    // Stage 23: the store knows the page's own drags (WebKit may hide their types until the drop).
    if (!isOwnDrag(event, "rows") && !isOwnDrag(event, "account")) return;
    const box = event.currentTarget.getBoundingClientRect();
    const scrolling = state.current;
    scrolling.node = event.currentTarget;
    scrolling.speed = autoScrollSpeed(event.clientY, box.top, box.bottom);
    scrolling.last = Date.now();
    if (scrolling.speed !== 0 && scrolling.frame === null) {
      scrolling.frame = requestAnimationFrame(() => {
        step(scrolling);
      });
    }
  };
}
