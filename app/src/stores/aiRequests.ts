import { create } from "zustand";
import type { AiRequestOutcome, AiRequestPhase, AiTransfer } from "../services/ipc";

/** What the UI knows about one AI request from its `aiRequest` events (stage 13). */
export interface AiRequestProgress {
  /** One per provider request, in order (a question has up to two). */
  transfers: readonly AiTransfer[];
  /** Characters of the answer received so far (the answer is JSON: progress, not text). */
  chars: number;
  /** Set once the core reports the end; the command then resolves or rejects. */
  outcome: AiRequestOutcome | null;
}

interface AiRequestState {
  requests: Readonly<Record<string, AiRequestProgress>>;
  apply: (requestId: string, phase: AiRequestPhase) => void;
  /** The view that started the request no longer shows it. */
  forget: (requestId: string) => void;
}

const EMPTY: AiRequestProgress = { transfers: [], chars: 0, outcome: null };

/**
 * Running AI requests by the id the UI chose. Fed only by the core's `aiRequest` events, so what
 * the UI shows ("N mails, X KB to host") is what the core is about to send, not a guess.
 */
export const useAiRequestStore = create<AiRequestState>()((set) => ({
  requests: {},
  apply: (requestId, phase) => {
    set((state) => {
      const current = state.requests[requestId] ?? EMPTY;
      const next: AiRequestProgress =
        phase.phase === "sending"
          ? { ...current, transfers: [...current.transfers, phase.transfer], chars: 0 }
          : phase.phase === "receiving"
            ? { ...current, chars: phase.chars }
            : { ...current, outcome: phase.outcome };
      return { requests: { ...state.requests, [requestId]: next } };
    });
  },
  forget: (requestId) => {
    set((state) => {
      if (!(requestId in state.requests)) return state;
      const requests = { ...state.requests };
      delete requests[requestId];
      return { requests };
    });
  },
}));
