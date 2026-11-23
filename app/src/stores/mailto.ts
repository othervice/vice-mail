import { create } from "zustand";
import type { ComposeRequest } from "../services/ipc";

/** A `mailto:` link from the system, waiting for the composer. */
export interface PendingCompose {
  id: number;
  request: ComposeRequest;
}

interface MailtoState {
  /** Oldest first. The first one opens as soon as there is an account and no open composer. */
  pending: readonly PendingCompose[];
  enqueue: (requests: readonly ComposeRequest[]) => void;
  /** Takes the oldest request out of the queue. */
  shift: () => PendingCompose | null;
  /** The user dropped every waiting link. */
  clear: () => void;
}

/** At most this many links wait; a burst of clicks beyond it drops the oldest. */
export const MAX_PENDING_COMPOSE = 10;

let nextId = 1;

/**
 * `mailto:` links the system handed to the app (at start or while it runs). Kept here, not in
 * the composer: a link can arrive before there is an account or while another message is being
 * written, and must not replace that one.
 */
export const useMailtoStore = create<MailtoState>()((set, get) => ({
  pending: [],
  enqueue: (requests) => {
    if (requests.length === 0) return;
    set((state) => ({
      pending: [...state.pending, ...requests.map((request) => ({ id: nextId++, request }))].slice(-MAX_PENDING_COMPOSE),
    }));
  },
  shift: () => {
    const [first, ...rest] = get().pending;
    if (!first) return null;
    set({ pending: rest });
    return first;
  },
  clear: () => {
    set({ pending: [] });
  },
}));
