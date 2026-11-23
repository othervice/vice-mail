import type { QueryClient } from "@tanstack/react-query";
import { ipc, toCoreError } from "../../services/ipc";
import { useNoticeStore, type UndoTarget } from "../../stores/notices";

/**
 * Takes an action back (the notice's "Undo", or Z): the core cancels what is still queued and
 * sends counter-operations for the rest. Several batches (a split selection) go newest first.
 * The rows come back with `messagesChanged`; the lists of the account are reloaded right away too.
 */
export async function undoAction(queryClient: QueryClient, target: UndoTarget): Promise<void> {
  const store = useNoticeStore.getState();
  store.clearUndo(target);
  try {
    for (const batch of [...target.batches].reverse()) {
      const result = await ipc.undoAction(batch);
      if (result.batch !== null) store.remember([result.batch]);
    }
    store.push({ kind: "undone" });
  } catch (e) {
    const error = toCoreError(e);
    if (error.code === "not_undoable") store.push({ kind: "notUndoable" });
    else if (error.code === "read_only") store.push({ kind: "readOnly", accountId: target.accountId });
    else store.push({ kind: "error", context: "undo", error });
  }
  await queryClient.invalidateQueries({ queryKey: ["messages", target.accountId] });
}
