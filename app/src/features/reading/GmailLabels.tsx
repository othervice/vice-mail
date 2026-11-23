import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import { gmailIpc, type Account, type Mailbox, type MessageId } from "../../services/ipc";
import { useMailboxes } from "../mailboxes/queries";

/**
 * Stage 14 (decision 0034): the labels of a Gmail message. Under `["messages", accountId]`, so
 * `mailboxesChanged` (every sync window and every outbox step) refreshes them.
 */
export const gmailLabelsQueryKey = (accountId: string, messageId: MessageId) =>
  ["messages", accountId, "gmailLabels", messageId] as const;

/** System labels shown as chips; the others are folders of their own (inbox, sent, trash …). */
const SHOWN_SYSTEM = ["\\Important", "\\Starred"] as const;

function isUserLabel(label: string): boolean {
  return !label.startsWith("\\");
}

/** Gmail's own folders (`[Gmail]/…`, `[Google Mail]/…`) are not labels one can set. */
function isSystemFolder(path: string): boolean {
  return /^\[(Gmail|Google Mail)\](\/|$)/.test(path);
}

/** User labels one can add: the account's ordinary folders, without those already set. */
export function addableLabels(mailboxes: readonly Mailbox[], labels: readonly string[]): string[] {
  return mailboxes
    .filter((m) => m.role === "custom" && m.selectable && !isSystemFolder(m.path) && !labels.includes(m.path))
    .map((m) => m.path);
}

interface GmailLabelsProps {
  account: Account;
  messageId: MessageId;
}

/** Chips with the message's labels; with writes on, remove and add (through the outbox). */
export function GmailLabels({ account, messageId }: GmailLabelsProps) {
  const { t } = useTranslation();
  const selectId = useId();
  const queryClient = useQueryClient();
  const key = gmailLabelsQueryKey(account.id, messageId);
  const labels = useQuery({ queryKey: key, queryFn: () => gmailIpc.getMessageLabels(messageId) });
  const mailboxes = useMailboxes(labels.data ? account.id : null);
  const change = useMutation({
    mutationFn: ({ label, add }: { label: string; add: boolean }) =>
      gmailIpc.setMessageLabel(messageId, label, add, crypto.randomUUID()),
    onSettled: () => queryClient.invalidateQueries({ queryKey: key }),
  });

  if (labels.isError) {
    return <ErrorNotice context={t("reading.labels.loadFailed")} error={labels.error} onRetry={() => void labels.refetch()} />;
  }
  const list = labels.data;
  if (!list) return null;
  const writable = account.writesEnabled;
  const shown = list.filter((label) => isUserLabel(label) || SHOWN_SYSTEM.some((s) => s === label));
  const addable = writable ? addableLabels(mailboxes.data ?? [], list) : [];
  const name = (label: string) => {
    switch (label) {
      case "\\Important":
        return t("reading.labels.important");
      case "\\Starred":
        return t("reading.labels.starred");
      default:
        return label;
    }
  };

  return (
    <div className="flex flex-col gap-1" data-testid="gmail-labels">
      <ul aria-label={t("reading.labels.region")} className="flex flex-wrap items-center gap-2">
        {shown.length === 0 && <li className="text-12 text-secondary">{t("reading.labels.none")}</li>}
        {shown.map((label) => (
          <li key={label} className="flex h-6 items-center gap-1 rounded-control border border-border px-2 text-12 text-primary">
            <span>{name(label)}</span>
            {writable && isUserLabel(label) && (
              <button
                type="button"
                aria-label={t("reading.labels.remove", { label })}
                disabled={change.isPending}
                onClick={() => change.mutate({ label, add: false })}
                className="-me-1 flex size-4 items-center justify-center rounded-control text-secondary hover:bg-hover hover:text-primary disabled:opacity-50"
              >
                <X aria-hidden className="size-3" strokeWidth={1.75} />
              </button>
            )}
          </li>
        ))}
        {addable.length > 0 && (
          <li className="flex items-center">
            <label htmlFor={selectId} className="sr-only">
              {t("reading.labels.add")}
            </label>
            <select
              id={selectId}
              value=""
              disabled={change.isPending}
              onChange={(e) => {
                const label = addable.find((l) => l === e.target.value);
                if (label) change.mutate({ label, add: true });
              }}
              className="h-6 rounded-control border border-border-strong bg-surface px-1 text-12 text-primary"
              data-testid="gmail-label-add"
            >
              <option value="">{t("reading.labels.add")}</option>
              {addable.map((label) => (
                <option key={label} value={label}>
                  {label}
                </option>
              ))}
            </select>
          </li>
        )}
      </ul>
      {change.isError && <ErrorNotice context={t("reading.labels.changeFailed")} error={change.error} />}
    </div>
  );
}
