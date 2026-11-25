import { useLayoutEffect, type RefObject } from "react";

/** Room kept between a menu and the edge of what can be seen (px). */
const EDGE = 8;
/** Gap between the button and its menu (px, the menu's `mt-1` / `mb-1`). */
const GAP = 4;
/** A menu is never squeezed below this height (px); below it, it rather overlaps the edge. */
const MIN_HEIGHT = 96;

interface Bounds {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * What can be seen around `element`: the window, cut down by every ancestor that clips its
 * content in an axis (a scrolling column, `overflow-x: clip`).
 */
export function visibleBounds(element: Element): Bounds {
  const root = document.documentElement;
  const bounds: Bounds = { left: 0, top: 0, right: root.clientWidth || window.innerWidth, bottom: root.clientHeight || window.innerHeight };
  for (let node = element.parentElement; node && node !== document.body && node !== root; node = node.parentElement) {
    const style = window.getComputedStyle(node);
    const clipsX = style.overflowX !== "visible";
    const clipsY = style.overflowY !== "visible";
    if (!clipsX && !clipsY) continue;
    const rect = node.getBoundingClientRect();
    if (rect.width === 0 && rect.height === 0) continue;
    if (clipsX) {
      bounds.left = Math.max(bounds.left, rect.left);
      bounds.right = Math.min(bounds.right, rect.right);
    }
    if (clipsY) {
      bounds.top = Math.max(bounds.top, rect.top);
      bounds.bottom = Math.min(bounds.bottom, rect.bottom);
    }
  }
  return bounds;
}

/**
 * Stage 21: keeps an open menu inside what can be seen (the window and the areas that clip it):
 * it opens above its button when there is not enough room below and more above (`data-side`
 * "above" on the menu, which its classes turn into `bottom-full`), it is moved sideways (CSSOM
 * `translate`, no style attribute: CSP) when it would stick out at a side, and it gets a smaller
 * maximum height when neither side has room for all of it. Measured on the DOM each time it
 * opens, before it is painted; no React state, so no second render.
 */
export function useMenuPlacement(open: boolean, anchor: RefObject<HTMLElement | null>, menu: RefObject<HTMLElement | null>): void {
  useLayoutEffect(() => {
    const node = menu.current;
    const button = anchor.current;
    if (!open || !node || !button) return;
    node.style.removeProperty("translate");
    node.style.removeProperty("max-height");
    node.removeAttribute("data-side");
    const box = node.getBoundingClientRect();
    if (box.width === 0 && box.height === 0) return;
    const at = button.getBoundingClientRect();
    const bounds = visibleBounds(button);

    // Up or down: where the whole menu fits, else where there is more room.
    const below = bounds.bottom - EDGE - (at.bottom + GAP);
    const above = at.top - GAP - (bounds.top + EDGE);
    const up = box.height > below && above > below;
    const room = up ? above : below;
    if (box.height > room) node.style.setProperty("max-height", `${String(Math.max(room, MIN_HEIGHT))}px`);
    node.setAttribute("data-side", up ? "above" : "below");

    // Sideways: back inside on the side it sticks out, the start edge first.
    let shift = 0;
    if (box.right > bounds.right - EDGE) shift = bounds.right - EDGE - box.right;
    if (box.left + shift < bounds.left + EDGE) shift = bounds.left + EDGE - box.left;
    if (shift !== 0) node.style.setProperty("translate", `${String(Math.round(shift))}px`);
  }, [open, anchor, menu]);
}
