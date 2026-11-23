import { create } from "zustand";
import { toCoreError, type CoreError } from "../../services/ipc";
import { saveUiPreferences } from "../keyboard/uiPreferences";

/**
 * Decision 0039: lists show one row per conversation (the default) or one per message. A UI
 * preference of the core (`UiPreferences.conversations`), synced between devices like the text
 * size. The store holds what the lists of this window read; `useUiPreferencesSync` fills it from
 * the core (load, saves, `uiPreferencesChanged`). Anything but a boolean reads as the default.
 */
export const DEFAULT_CONVERSATIONS = true;

interface ConversationState {
  on: boolean;
  /** The core refused the last change; the previous value applies again. */
  saveError: CoreError | null;
  /** The user's choice: shown at once, saved to the core (rolled back if it refuses). */
  setOn: (on: boolean) => void;
  /** What the core holds. */
  applyStored: (on: unknown) => void;
}

export const useConversationStore = create<ConversationState>()((set) => ({
  on: DEFAULT_CONVERSATIONS,
  saveError: null,
  setOn: (on) => {
    set({ on, saveError: null });
    saveUiPreferences({ conversations: on }).catch((error: unknown) => {
      // The cache is back at the previous value; `applyStored` has put it on screen again.
      set({ saveError: toCoreError(error) });
    });
  },
  applyStored: (on) => {
    set({ on: typeof on === "boolean" ? on : DEFAULT_CONVERSATIONS });
  },
}));

/** Whether lists show conversations right now. */
export function useConversationsOn(): boolean {
  return useConversationStore((state) => state.on);
}
