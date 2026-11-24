import { create } from "zustand";
import type { Account, Mailbox } from "../services/ipc";

/** Audit 24.11.2026: "Rename folder…" or "Delete folder…", asked from the folder's menu or the palette. */
export interface FolderEditRequest {
  kind: "rename" | "delete";
  account: Account;
  mailbox: Mailbox;
  /** Where the focus goes back when the question closes (the folder's row), if anywhere. */
  returnFocus?: () => void;
}

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
