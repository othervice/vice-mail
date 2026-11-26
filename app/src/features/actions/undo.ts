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
  if (target.partial) {
    await undoEach(target);
    await queryClient.invalidateQueries({ queryKey: ["messages"] });
    return;
  }
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

/**
 * Stage 22 (decision 0081): a drop's batches, each on its own, newest first. A move within an
 * account undoes like any move; a transfer from another account only while nothing of it reached
 * the server (`not_undoable` then, and the rest is still undone). Says what happened.
 */
async function undoEach(target: UndoTarget): Promise<void> {
  const store = useNoticeStore.getState();
  let undone = 0;
  let refused = 0;
  for (const batch of [...target.batches].reverse()) {
    try {
      const result = await ipc.undoAction(batch);
      if (result.batch !== null) store.remember([result.batch]);
      undone += 1;
    } catch (e) {
      const error = toCoreError(e);
      if (error.code === "not_undoable") {
        refused += 1;
        continue;
      }
      if (error.code === "read_only") store.push({ kind: "readOnly", accountId: target.accountId });
      else store.push({ kind: "error", context: "undo", error });
      return;
    }
  }
  if (refused === 0) store.push({ kind: "undone" });
  else if (undone > 0) store.push({ kind: "undonePartly" });
  else store.push({ kind: "notUndoable" });
}
