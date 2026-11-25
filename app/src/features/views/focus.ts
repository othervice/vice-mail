import { create } from "zustand";

/**
 * Stage 19: a request to put the focus into the quick filter's text field (the command "Quick
 * filter", Cmd/Ctrl+Shift+K). The bar of the view on screen answers it.
 */
interface QuickFilterFocusState {
  request: number;
  focus: () => void;
}

export const useQuickFilterFocus = create<QuickFilterFocusState>()((set) => ({
  request: 0,
  focus: () => {
    set((state) => ({ request: state.request + 1 }));
  },
}));
