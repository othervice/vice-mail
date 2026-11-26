import { create } from "zustand";

/**
 * Stage 19: a request to put the focus into the quick filter's text field (the command "Quick
 * filter", Cmd/Ctrl+Shift+K). The bar of the view on screen answers it. Stage 23, step 3
 * (decision 0084): the bar is folded away until the toolbar's filter button or the command shows
 * it, and stays while a filter is set.
 */
interface QuickFilterFocusState {
  request: number;
  /** The bar is shown (besides while a filter is set). */
  shown: boolean;
  /** The text field should take the focus once the bar is there. */
  pending: boolean;
  focus: () => void;
  /** The bar took the focus it was asked to. */
  focused: () => void;
  /** The toolbar's button: shows the bar (its field takes the focus) or folds it away. */
  toggle: () => void;
}

export const useQuickFilterFocus = create<QuickFilterFocusState>()((set) => ({
  request: 0,
  shown: false,
  pending: false,
  focus: () => {
    set((state) => ({ request: state.request + 1, shown: true, pending: true }));
  },
  focused: () => {
    set({ pending: false });
  },
  toggle: () => {
    set((state) => (state.shown ? { shown: false, pending: false } : { request: state.request + 1, shown: true, pending: true }));
  },
}));
