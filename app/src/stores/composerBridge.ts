import { create } from "zustand";
import type { TemplateId } from "../services/ipc";

/**
 * Stage 10: what the open composer offers to the rest of the app (the command palette inserts a
 * template through it). Null while no composer with a draft is open.
 */
interface ComposerBridgeState {
  insertTemplate: ((templateId: TemplateId) => void) | null;
  setInsertTemplate: (insert: ((templateId: TemplateId) => void) | null) => void;
}

export const useComposerBridge = create<ComposerBridgeState>()((set) => ({
  insertTemplate: null,
  setInsertTemplate: (insertTemplate) => {
    set({ insertTemplate });
  },
}));
