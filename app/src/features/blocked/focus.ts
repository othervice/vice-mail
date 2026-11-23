import { create } from "zustand";

/**
 * Stage 16 (decision 0036): the inbox's note on hidden mail opens Settings → Inbox at the
 * blocked senders. The section takes the request once it shows: it scrolls into view and its
 * heading gets the focus, so keyboard and screen reader users land where the note pointed.
 */
interface BlockedFocusState {
  requested: boolean;
  request: () => void;
  take: () => boolean;
}

export const useBlockedFocusStore = create<BlockedFocusState>()((set, get) => ({
  requested: false,
  request: () => {
    set({ requested: true });
  },
  take: () => {
    if (!get().requested) return false;
    set({ requested: false });
    return true;
  },
}));
