import { create } from "zustand";

/**
 * Stage 23, step 3 (decision 0084): the places of the toolbar across the window
 * (`app/AppToolbar.tsx`) that the view on screen fills, and the list column the toolbar lines its
 * list segment up with. The toolbar registers its slots; the views render into them through
 * `ToolbarPortal` (title, counter and list controls over the list; the open message's actions
 * over the reading pane), so their controls keep the view's state and handlers.
 */
interface ToolbarState {
  /** Over the list: the view's title, its counter and the list's controls. */
  listSlot: HTMLElement | null;
  /** Over the reading pane: the actions of the open message. */
  readingSlot: HTMLElement | null;
  /** The list beside the reading pane (right): the list segment ends where it ends. */
  listColumn: HTMLElement | null;
  setSlots: (list: HTMLElement | null, reading: HTMLElement | null) => void;
  setListColumn: (column: HTMLElement | null) => void;
  /** Forgets the column only if it is still this one (a newer one may have taken its place). */
  dropListColumn: (column: HTMLElement) => void;
}

export const useToolbarStore = create<ToolbarState>()((set, get) => ({
  listSlot: null,
  readingSlot: null,
  listColumn: null,
  setSlots: (listSlot, readingSlot) => {
    set({ listSlot, readingSlot });
  },
  setListColumn: (listColumn) => {
    if (get().listColumn !== listColumn) set({ listColumn });
  },
  dropListColumn: (column) => {
    if (get().listColumn === column) set({ listColumn: null });
  },
}));

/** A ref callback for the list column beside the reading pane (React 19: with its cleanup). */
export function registerListColumn(column: HTMLElement | null): (() => void) | undefined {
  if (!column) return undefined;
  useToolbarStore.getState().setListColumn(column);
  return () => {
    useToolbarStore.getState().dropListColumn(column);
  };
}
