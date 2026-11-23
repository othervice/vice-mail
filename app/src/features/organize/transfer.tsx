import { useQueries, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  ipc,
  PARITY_LIMITS,
  parityIpc,
  toCoreError,
  type Account,
  type BatchId,
  type Mailbox,
  type MessageListItem,
  type TransferMode,
} from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { accountsQueryKey, useAccounts } from "../accounts/queries";
import { mailboxesQueryKey } from "../mailboxes/queries";
import { messagesQueryKey } from "../messages/queries";
import { useFolderName } from "./folders";

/** The folders of every account but `accountId`, for "Move to …" and drops across accounts. */
export function useOtherAccountFolders(accountId: string): { account: Account; mailboxes: Mailbox[] }[] {
  const accounts = (useAccounts().data ?? []).filter((account) => account.id !== accountId);
  const results = useQueries({
    queries: accounts.map((account) => ({
      queryKey: mailboxesQueryKey(account.id),
      queryFn: () => ipc.listMailboxes(account.id),
    })),
  });
  return accounts.map((account, i) => ({ account, mailboxes: results[i]?.data ?? [] }));
}

export interface TransferRequest {
  items: readonly MessageListItem[];
  source: { account: Account; mailbox: Mailbox };
  target: { account: Account; mailbox: Mailbox };
}

function chunks<T>(items: readonly T[], size: number): T[][] {
  const parts: T[][] = [];
  for (let i = 0; i < items.length; i += size) parts.push(items.slice(i, i + size));
  return parts;
}

/**
 * Runs a copy or move into another account through the outbox (`transferMessages`, decision 0033):
 * the copy goes into the target first; for a move the source is removed only once the copy is
 * confirmed. Writes are checked first (the target always, the source for a move), so the user
 * learns which account to allow changes in. No undo: the source removal is recorded later.
 */
export function useTransfer() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  const remember = useNoticeStore((state) => state.remember);
  const folderName = useFolderName();

  return async (request: TransferRequest, mode: TransferMode): Promise<boolean> => {
    const { items, source, target } = request;
    if (items.length === 0) return false;
    if (!target.account.writesEnabled) {
      push({ kind: "readOnly", accountId: target.account.id });
      return false;
    }
    if (mode === "move" && !source.account.writesEnabled) {
      push({ kind: "readOnly", accountId: source.account.id });
      return false;
    }
    const batches: BatchId[] = [];
    let done = 0;
    try {
      for (const part of chunks(items, PARITY_LIMITS.transferMessages)) {
        const refs = part.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id }));
        const result = await parityIpc.transferMessages(refs, target.mailbox.id, mode, crypto.randomUUID());
        if (result.batch !== null) batches.push(result.batch);
        done += part.length;
      }
    } catch (e) {
      const error = toCoreError(e);
      if (error.code === "read_only") {
        // Turned off meanwhile (another window): the account list is out of date.
        void queryClient.invalidateQueries({ queryKey: accountsQueryKey });
        push({ kind: "readOnly", accountId: target.account.writesEnabled ? source.account.id : target.account.id });
      } else if (error.code === "not_found") {
        void queryClient.invalidateQueries({ queryKey: messagesQueryKey(source.account.id, source.mailbox.id) });
        push({ kind: "outdated" });
      } else {
        push({ kind: "error", context: "transfer", error });
      }
    } finally {
      remember(batches);
    }
    if (done > 0) {
      push({
        kind: "transfer",
        mode,
        count: done,
        folder: folderName(target.mailbox),
        account: target.account.displayName || target.account.email,
      });
    }
    return done === items.length;
  };
}

interface TransferDialogProps {
  request: TransferRequest;
  onChoose: (mode: TransferMode) => void;
  onCancel: () => void;
  focusAfter?: () => void;
}

/**
 * The question before anything crosses accounts: what will happen (copy first, the source removed
 * only after the copy arrived), what it needs (changes allowed), and where to follow it.
 */
export function TransferDialog({ request, onChoose, onCancel, focusAfter }: TransferDialogProps) {
  const { t } = useTranslation();
  const folderName = useFolderName();
  const account = request.target.account.displayName || request.target.account.email;
  const sourceName = request.source.account.displayName || request.source.account.email;
  const folder = folderName(request.target.mailbox);
  return (
    <ConfirmDialog
      tone="neutral"
      title={t("organize.transfer.title", { count: request.items.length, folder, account })}
      body={t("organize.transfer.body", { account, source: sourceName })}
      confirmLabel={t("organize.transfer.move")}
      cancelLabel={t("organize.transfer.cancel")}
      secondary={{
        label: t("organize.transfer.copy"),
        onClick: () => {
          onChoose("copy");
        },
      }}
      onConfirm={() => {
        onChoose("move");
      }}
      onCancel={onCancel}
      {...(focusAfter && { focusAfterConfirm: focusAfter })}
      data-testid="transfer-dialog"
    />
  );
}
