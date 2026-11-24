import { sourceIpc, toCoreError, uiIpc, type AccountId, type MessageId } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";

/**
 * Stage 8: one message in a window of its own (an open one comes to the front). A refusal
 * (`not_found`: the message is gone) is a notice with the reason; no window opens then.
 */
export function openMessageInWindow(accountId: AccountId, messageId: MessageId): void {
  uiIpc.openMessageWindow(accountId, messageId).catch((error: unknown) => {
    useNoticeStore.getState().push({ kind: "error", context: "window", error: toCoreError(error) });
  });
}

/**
 * Stage 19 (decision 0055): the message's source in a window of its own (an open one comes to the
 * front). A refusal is a notice with the reason.
 */
export function openSourceInWindow(accountId: AccountId, messageId: MessageId): void {
  sourceIpc.openWindow(accountId, messageId).catch((error: unknown) => {
    useNoticeStore.getState().push({ kind: "error", context: "source", error: toCoreError(error) });
  });
}
