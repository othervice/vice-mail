import { create } from "zustand";
import type { MessageId } from "../services/ipc";

interface PrintRequestState {
  /** The message to print once the reading pane shows it; null: none asked for. */
  messageId: MessageId | null;
  request: (messageId: MessageId) => void;
  clear: () => void;
}

/**
 * Stage 21 (decision 0064): "Print" from the context menu of a row. The print styles print the
 * reading pane's message (stage 9), so the menu opens the message there and asks for it here; the
 * pane's tools print as soon as that message is shown.
 */
export const usePrintRequest = create<PrintRequestState>()((set) => ({
  messageId: null,
  request: (messageId) => {
    set({ messageId });
  },
  clear: () => {
    set({ messageId: null });
  },
}));
