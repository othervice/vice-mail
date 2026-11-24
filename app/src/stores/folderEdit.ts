import { create } from "zustand";
import type { Account, Mailbox } from "../services/ipc";

/**
 * Audit 24.11.2026: "Rename folder…" or "Delete folder…", asked from the folder's menu or the
 * palette. Stage 18: "New folder…" (at the top of the account, `parent` null) and "New
 * subfolder…" (below `parent`).
 */
export type FolderEditRequest =
  | {
      kind: "rename" | "delete";
      account: Account;
      mailbox: Mailbox;
      /** Where the focus goes back when the question closes (the folder's row), if anywhere. */
      returnFocus?: () => void;
    }
  | {
      kind: "create";
      account: Account;
      parent: Mailbox | null;
      returnFocus?: () => void;
    };

interface FolderEditState {
  request: FolderEditRequest | null;
  ask: (request: FolderEditRequest) => void;
  close: () => void;
}

/** The open rename or delete question; `FolderEditHost` shows it. */
export const useFolderEditStore = create<FolderEditState>()((set) => ({
  request: null,
  ask: (request) => {
    set({ request });
  },
  close: () => {
    set({ request: null });
  },
}));
