import { create } from "zustand";
import { saveUiPreferences } from "../features/keyboard/uiPreferences";
import { toCoreError, type AiPanelState as CoreAiPanelState, type MessageId } from "../services/ipc";
import { useNoticeStore } from "./notices";

/** A source of an AI statement the user wants to see; `seq` makes a second click a new request. */
export interface AiSourceRequest {
  messageId: MessageId;
  seq: number;
}

/** A command of the palette for the panel (it opens the panel and runs there, visibly). */
export interface AiPanelCommand {
  kind: "summarize" | "suggestReply" | "ask";
  seq: number;
}

interface AiPanelState {
  /**
   * The panel is unfolded (docs/DESIGN.md: right, 380 px, collapsible). Decision 0028: a UI
   * preference of this device in the core (`aiPanel`), not the web view's storage.
   */
  open: boolean;
  toggle: () => void;
  setOpen: (open: boolean) => void;
  /** What the core holds (loaded, saved, or announced by `uiPreferencesChanged`). */
  applyStored: (state: CoreAiPanelState) => void;
  /** The thread pane shows this message of the thread when it has it. */
  sourceRequest: AiSourceRequest | null;
  showSource: (messageId: MessageId) => void;
  /** Decision 0027: the messages the thread pane shows now; a source among them is shown there. */
  shownSources: readonly MessageId[];
  setShownSources: (ids: readonly MessageId[]) => void;
  command: AiPanelCommand | null;
  request: (kind: AiPanelCommand["kind"]) => void;
  /** The panel took the command. */
  clearCommand: () => void;
}

/**
 * Saves the state in the core. A refusal puts the stored value back (through the preferences'
 * cache) and says so; the panel then shows as it is stored.
 */
function persist(open: boolean): void {
  saveUiPreferences({ aiPanel: open ? "open" : "folded" }).catch((error: unknown) => {
    useNoticeStore.getState().push({ kind: "error", context: "aiPanel", error: toCoreError(error) });
  });
}

/** Stage 13: UI state of the AI panel. Its results live in the panel, its settings in the core. */
export const useAiPanelStore = create<AiPanelState>()((set, get) => ({
  // Open until the core's value arrives (the frame waits for the preferences before it shows).
  open: true,
  toggle: () => {
    const open = !get().open;
    set({ open });
    persist(open);
  },
  setOpen: (open) => {
    if (get().open === open) return;
    set({ open });
    persist(open);
  },
  applyStored: (state) => {
    set({ open: state === "open" });
  },
  sourceRequest: null,
  showSource: (messageId) => {
    set((state) => ({ sourceRequest: { messageId, seq: (state.sourceRequest?.seq ?? 0) + 1 } }));
  },
  shownSources: [],
  setShownSources: (ids) => {
    set({ shownSources: ids });
  },
  command: null,
  request: (kind) => {
    const wasOpen = get().open;
    set((state) => ({ open: true, command: { kind, seq: (state.command?.seq ?? 0) + 1 } }));
    if (!wasOpen) persist(true);
  },
  clearCommand: () => {
    set({ command: null });
  },
}));
