import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { contactsIpc, toCoreError, type ContactEntry, type CoreError, type SpamMark } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { blockedSendersQueryKey } from "../blocked/queries";
import { refreshComfortLists } from "../comfort/queries";
import { refreshContacts } from "./queries";

/** A mark waiting for the answer to "Move the existing mail to Junk?". */
export interface PendingSpamMark {
  contact: ContactEntry;
  name: string;
  mark: SpamMark;
  /** Messages in accounts that may write. */
  movable: number;
  /** Messages in read-only accounts: they stay. */
  kept: number;
}

/**
 * Stage 17 (decision 0050): "Mark as spam" on a contact. The core blocks every address of the
 * contact right away (stage 16: new mail goes to Junk, or is hidden on read-only accounts); when
 * mail from it lies in accounts that may write, the view asks whether to move it to Junk too
 * (through the outbox). The notice afterwards undoes both. "Not spam" unblocks the addresses.
 */
export function useContactSpam() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  const remember = useNoticeStore((state) => state.remember);
  const [pending, setPending] = useState<PendingSpamMark | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CoreError | null>(null);

  const refresh = () => {
    refreshContacts(queryClient);
    void queryClient.invalidateQueries({ queryKey: blockedSendersQueryKey });
    refreshComfortLists(queryClient, null);
  };

  const notice = (done: PendingSpamMark, batches: readonly number[], moved: number, kept: number) => {
    push({
      kind: "comfort",
      notice: {
        type: "contactSpam",
        email: done.contact.email,
        name: done.name,
        undo: { addresses: done.mark.blocked, batches: [...batches] },
        moved,
        kept,
      },
    });
  };

  const mark = async (contact: ContactEntry, name: string) => {
    setBusy(true);
    setError(null);
    try {
      const result = await contactsIpc.markContactSpam(contact.email);
      refresh();
      const movable = result.mail.filter((m) => m.writesEnabled).reduce((n, m) => n + m.count, 0);
      const kept = result.mail.filter((m) => !m.writesEnabled).reduce((n, m) => n + m.count, 0);
      const done: PendingSpamMark = { contact: result.contact, name, mark: result, movable, kept };
      if (movable > 0) setPending(done);
      else notice(done, [], 0, kept);
    } catch (e) {
      setError(toCoreError(e));
    } finally {
      setBusy(false);
    }
  };

  /** The answer to the question: move the mail (`true`) or leave it where it is. */
  const answer = async (move: boolean) => {
    const done = pending;
    setPending(null);
    if (!done) return;
    if (!move) {
      notice(done, [], 0, done.kept);
      return;
    }
    setBusy(true);
    try {
      const moved = await contactsIpc.moveContactMailToJunk(done.contact.email);
      remember(moved.batches);
      refresh();
      notice(
        done,
        moved.batches,
        moved.moved,
        moved.kept.reduce((n, m) => n + m.count, 0),
      );
    } catch (e) {
      setError(toCoreError(e));
      notice(done, [], 0, done.kept);
    } finally {
      setBusy(false);
    }
  };

  const notSpam = async (contact: ContactEntry, name: string) => {
    setBusy(true);
    setError(null);
    try {
      const addresses = contact.addresses.filter((a) => a.blocked).map((a) => a.email);
      await contactsIpc.undoContactSpam(contact.email, { addresses, batches: [] });
      refresh();
      push({ kind: "comfort", notice: { type: "contactNotSpam", name } });
    } catch (e) {
      setError(toCoreError(e));
    } finally {
      setBusy(false);
    }
  };

  return { mark, answer, notSpam, pending, busy, error };
}
