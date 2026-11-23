import { create } from "zustand";
import { readLocal, writeLocal } from "../features/keyboard/localFallback";
import type { MessageId } from "../services/ipc";

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
  /** The panel is unfolded (docs/DESIGN.md: right, 380 px, collapsible). Remembered per device. */
  open: boolean;
  toggle: () => void;
  setOpen: (open: boolean) => void;
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

const KEY = "layout.aiPanel";

function loadOpen(): boolean {
  const raw = readLocal(KEY);
  return typeof raw === "boolean" ? raw : true;
}

/** Stage 13: UI state of the AI panel. Its results live in the panel, its settings in the core. */
export const useAiPanelStore = create<AiPanelState>()((set) => ({
  open: loadOpen(),
  toggle: () => {
    set((state) => {
      writeLocal(KEY, !state.open);
      return { open: !state.open };
    });
  },
  setOpen: (open) => {
    writeLocal(KEY, open);
    set({ open });
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
    writeLocal(KEY, true);
    set((state) => ({ open: true, command: { kind, seq: (state.command?.seq ?? 0) + 1 } }));
  },
  clearCommand: () => {
    set({ command: null });
  },
}));
