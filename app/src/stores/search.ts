import { create } from "zustand";

/** Where a search looks, relative to what the mail view shows. */
export type SearchScopeChoice = "mailbox" | "account" | "group" | "all";

interface SearchState {
  /** The search field is shown above the list. */
  open: boolean;
  /** The text as typed. */
  query: string;
  /** Enter was pressed for this text: no prefix matching of the last word any more. */
  submitted: boolean;
  scope: SearchScopeChoice;
  /** Counts up whenever the shortcut or the button asks for the field's focus. */
  focusSeq: number;
  /**
   * Lists show conversations (one row per thread) instead of single messages. A UI setting
   * only; the core does not store it (stage 7 contract).
   */
  conversations: boolean;
  openSearch: () => void;
  closeSearch: () => void;
  setQuery: (query: string) => void;
  submit: () => void;
  setScope: (scope: SearchScopeChoice) => void;
  setConversations: (on: boolean) => void;
}

/** UI state of the search and the conversation view (stage 7). Results live in TanStack Query. */
export const useSearchStore = create<SearchState>()((set) => ({
  open: false,
  query: "",
  submitted: false,
  scope: "all",
  focusSeq: 0,
  conversations: false,
  openSearch: () => {
    set((state) => ({ open: true, focusSeq: state.focusSeq + 1 }));
  },
  closeSearch: () => {
    set({ open: false, query: "", submitted: false });
  },
  setQuery: (query) => {
    set({ query, submitted: false });
  },
  submit: () => {
    set({ submitted: true });
  },
  setScope: (scope) => {
    set({ scope });
  },
  setConversations: (conversations) => {
    set({ conversations });
  },
}));
