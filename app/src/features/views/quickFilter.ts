import { create } from "zustand";
import type { QuickFilter } from "../../services/ipc";

/**
 * Stage 19 (decision 0062): the quick filter of each view, for this session (a filter, not a
 * setting): "Unread" (stage 16's "Unread only"), "Flagged", "With attachment", "From contacts",
 * a tag and the text. Every part narrows the list (AND).
 */
export interface QuickFilterState {
  unread: boolean;
  flagged: boolean;
  attachment: boolean;
  contacts: boolean;
  tag: string | null;
  text: string;
}

export const NO_QUICK_FILTER: QuickFilterState = {
  unread: false,
  flagged: false,
  attachment: false,
  contacts: false,
  tag: null,
  text: "",
};

interface QuickFilterStore {
  views: Readonly<Record<string, QuickFilterState>>;
  set: (view: string, patch: Partial<QuickFilterState>) => void;
  reset: (view: string) => void;
}

export const useQuickFilterStore = create<QuickFilterStore>()((set) => ({
  views: {},
  set: (view, patch) => {
    set((state) => ({ views: { ...state.views, [view]: { ...(state.views[view] ?? NO_QUICK_FILTER), ...patch } } }));
  },
  reset: (view) => {
    set((state) => ({ views: Object.fromEntries(Object.entries(state.views).filter(([key]) => key !== view)) }));
  },
}));

/** The quick filter of a view (nothing set for no view). */
export function useQuickFilter(viewKey: string | null): QuickFilterState {
  return useQuickFilterStore((state) => (viewKey ? (state.views[viewKey] ?? NO_QUICK_FILTER) : NO_QUICK_FILTER));
}

/** Any part is set. */
export function isFiltering(filter: QuickFilterState): boolean {
  return filter.unread || beyondUnread(filter);
}

/** Something besides "Unread" is set: the view's stage 16 "Unread only" list no longer fits. */
export function beyondUnread(filter: QuickFilterState): boolean {
  return filter.flagged || filter.attachment || filter.contacts || filter.tag !== null || filter.text.trim() !== "";
}

/** The filter as the core takes it; `text` as debounced for the query. */
export function toQuickFilter(filter: QuickFilterState, text: string, monthFirst: boolean): QuickFilter {
  return {
    unread: filter.unread,
    flagged: filter.flagged,
    attachment: filter.attachment,
    contacts: filter.contacts,
    tag: filter.tag,
    text: text.trim(),
    monthFirst,
  };
}
