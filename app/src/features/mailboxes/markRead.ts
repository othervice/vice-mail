import { CheckCheck } from "lucide-react";
import { useTranslation } from "react-i18next";
import { ipc, toCoreError, type Account, type BatchId, type Mailbox } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import type { OverflowItem } from "../actions/OverflowMenu";

/**
 * What a "Mark … as read" of the column covers: one folder, an account (its inbox and own folders,
 * what its counter counts), or the inboxes of the accounts of the unified inbox or a group.
 */
export type MarkReadScope = "folder" | "account" | "inboxes";

/** Folders of one account to mark. */
export interface MarkReadTarget {
  account: Account;
  mailboxes: readonly Mailbox[];
}

function unreadIn(mailboxes: readonly Mailbox[]): Mailbox[] {
  return mailboxes.filter((mailbox) => mailbox.selectable && mailbox.unread > 0);
}

/**
 * Stage 21 (decision 0063): "Mark folder as read" or "Mark all as read" for a menu of the column.
 * Not there while nothing is unread. On accounts that only read it stays in the menu, unavailable,
 * with the reason under its name, and a click says how to allow changes (like renaming). Every
 * account is one call (one batch each); the notice undoes them together. `name` is the folder,
 * account or view named in the notice.
 */
export function useMarkReadItem(scope: MarkReadScope, targets: readonly MarkReadTarget[], name: string): OverflowItem[] {
  const { t } = useTranslation();
  const push = useNoticeStore((state) => state.push);
  const remember = useNoticeStore((state) => state.remember);
  const withUnread = targets.flatMap((target) => {
    const mailboxes = unreadIn(target.mailboxes);
    return mailboxes.length > 0 ? [{ account: target.account, mailboxes }] : [];
  });
  const first = withUnread[0];
  if (!first) return [];
  const writable = withUnread.filter((target) => target.account.writesEnabled);
  const readOnly = writable.length === 0;

  const run = async () => {
    const batches: BatchId[] = [];
    let count = 0;
    for (const target of writable) {
      try {
        const result = await ipc.markMailboxesRead(
          target.account.id,
          target.mailboxes.map((mailbox) => mailbox.id),
          crypto.randomUUID(),
        );
        if (result.batch === null) continue;
        batches.push(result.batch);
        count += target.mailboxes.reduce((sum, mailbox) => sum + mailbox.unread, 0);
      } catch (e) {
        const error = toCoreError(e);
        if (error.code === "read_only") push({ kind: "readOnly", accountId: target.account.id });
        else push({ kind: "error", context: "markFolderRead", error });
      }
    }
    const owner = writable[0];
    if (batches.length === 0 || !owner) return;
    remember(batches);
    push({ kind: "done", action: "mark_mailbox_read", count, detail: name, undo: { accountId: owner.account.id, batches } });
  };

  return [
    {
      id: "mark-read",
      label: t(`markRead.${scope}`),
      icon: CheckCheck,
      ...(readOnly && { inert: true, hint: t("markRead.readOnly") }),
      onSelect: () => {
        if (readOnly) push({ kind: "readOnly", accountId: first.account.id });
        else void run();
      },
      "data-testid": `mark-read-${scope}`,
    },
  ];
}

/** The folders an account's "Mark all as read" covers: what its counter in the column counts. */
export function accountReadScope(mailboxes: readonly Mailbox[]): Mailbox[] {
  return mailboxes.filter((mailbox) => mailbox.role === "inbox" || mailbox.role === "custom");
}
