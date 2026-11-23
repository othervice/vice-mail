import { useEffect, useRef } from "react";
import { create } from "zustand";
import type { MessageId } from "../../services/ipc";

/**
 * Stage 16 (decision 0036): "Save all attachments" from the command palette. The attachment
 * section of the reading pane says which message it shows and how many attachments it has; the
 * palette asks that section to save them, so the result appears where the attachments are (the
 * same status line as the section's own button).
 */
interface SaveAllRequestState {
  /** Messages whose attachment section is on screen, with their number of attachments. */
  shown: ReadonlyMap<MessageId, number>;
  request: { messageId: MessageId; seq: number } | null;
  requestSaveAll: (messageId: MessageId) => void;
}

export const useSaveAllRequestStore = create<SaveAllRequestState>()((set) => ({
  shown: new Map(),
  request: null,
  requestSaveAll: (messageId) => {
    set((state) => ({ request: { messageId, seq: (state.request?.seq ?? 0) + 1 } }));
  },
}));

/**
 * The message the palette's "Save all attachments" is for: the one the actions act on when its
 * attachments are on screen, else the only message whose attachments are. Null when none.
 */
export function saveAllCandidate(shown: ReadonlyMap<MessageId, number>, target: MessageId | null): { messageId: MessageId; count: number } | null {
  if (target !== null) {
    const count = shown.get(target);
    if (count !== undefined) return { messageId: target, count };
  }
  if (shown.size !== 1) return null;
  const [entry] = shown;
  return entry ? { messageId: entry[0], count: entry[1] } : null;
}

/** The attachment section's side: tells it is on screen and runs requests made for it. */
export function useSaveAllRequest(messageId: MessageId | null, count: number, run: () => void): void {
  const runRef = useRef(run);
  useEffect(() => {
    runRef.current = run;
  });
  useEffect(() => {
    if (messageId === null) return undefined;
    useSaveAllRequestStore.setState((state) => ({ shown: new Map(state.shown).set(messageId, count) }));
    return () => {
      useSaveAllRequestStore.setState((state) => {
        const shown = new Map(state.shown);
        shown.delete(messageId);
        return { shown };
      });
    };
  }, [messageId, count]);
  const request = useSaveAllRequestStore((state) => state.request);
  const handled = useRef(request?.seq ?? 0);
  useEffect(() => {
    if (!request || request.seq <= handled.current) return;
    handled.current = request.seq;
    if (request.messageId === messageId) runRef.current();
  }, [request, messageId]);
}

/** Tests: forget what was shown and asked. */
export function resetSaveAllRequestForTests(): void {
  useSaveAllRequestStore.setState({ shown: new Map(), request: null });
}
