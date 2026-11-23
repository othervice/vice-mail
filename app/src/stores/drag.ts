import { create } from "zustand";
import type { AccountId, Mailbox, MailboxId } from "../services/ipc";

/** Data type of dragged list rows. Only for `types` checks: the session below holds the rows. */
export const MESSAGE_DRAG_TYPE = "application/x-mail-message-refs";

/**
 * Rows dragged out of the message list (native drag and drop). Folders in the sidebar and the
 * rail accept them while a session of the same account runs; `drop` runs the move through the
 * list's action controller, with its undo notice and focus handling.
 */
export interface DragSession {
  accountId: AccountId;
  sourceMailboxId: MailboxId;
  count: number;
  drop: (target: Mailbox) => void;
}

interface DragState {
  session: DragSession | null;
  start: (session: DragSession) => void;
  end: () => void;
}

export const useDragStore = create<DragState>()((set) => ({
  session: null,
  start: (session) => {
    set({ session });
  },
  end: () => {
    set({ session: null });
  },
}));

/** Whether `mailbox` takes the rows of the running session (same account, another real folder). */
export function acceptsDrop(session: DragSession | null, mailbox: Mailbox | undefined): mailbox is Mailbox {
  return (
    session !== null &&
    mailbox !== undefined &&
    mailbox.accountId === session.accountId &&
    mailbox.selectable &&
    mailbox.id !== session.sourceMailboxId
  );
}
