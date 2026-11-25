import { create } from "zustand";
import type { AccountId, MessageId } from "../services/ipc";

/** Stage 19 (decision 0055): the message the redirect dialog is open for. */
export interface RedirectTarget {
  accountId: AccountId;
  messageId: MessageId;
}

interface RedirectState {
  target: RedirectTarget | null;
  open: (target: RedirectTarget) => void;
  close: () => void;
}

/** One redirect dialog per window, opened from the actions, the card menu or the palette. */
export const useRedirectStore = create<RedirectState>((set) => ({
  target: null,
  open: (target) => {
    set({ target });
  },
  close: () => {
    set({ target: null });
  },
}));
