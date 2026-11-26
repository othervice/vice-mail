import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { useToolbarStore } from "../stores/toolbar";

export type ToolbarSlot = "list" | "reading";

/** The toolbar's slot, or null where there is no toolbar (a message window, a component test). */
export function useToolbarSlot(slot: ToolbarSlot): HTMLElement | null {
  return useToolbarStore((state) => (slot === "list" ? state.listSlot : state.readingSlot));
}

/**
 * Stage 23, step 3 (decision 0084): renders a view's controls into the toolbar across the window.
 * React keeps them in the view's tree (state, context and events stay the view's); in the
 * document they are part of the toolbar, so they come first in the Tab order, before the sidebar,
 * the list and the reading pane, as in every toolbar. Without a toolbar `fallback` renders in
 * place instead.
 */
export function ToolbarPortal({ slot, children, fallback = null }: { slot: ToolbarSlot; children: ReactNode; fallback?: ReactNode }) {
  const target = useToolbarSlot(slot);
  return target ? createPortal(children, target) : fallback;
}
