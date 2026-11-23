import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, MailboxId, MessageId, MessageListItem, ThreadEntry } from "../../services/ipc";
import { formatFullDate } from "../messages/rows";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";
import { useThread } from "./queries";

interface ThreadPaneProps {
  account: Account;
  /** The row the user opened: the newest message of the thread in the listed folder. */
  item: MessageListItem;
  /** The listed folder, so entries show their place there; null in mixed lists. */
  mailboxId: MailboxId | null;
  focusRequest: FocusRequest | null;
  /** Mail actions for the open message, if the view can act on it (its folder). */
  toolbarFor?: (message: MessageListItem) => ReactNode;
  onEscape: () => void;
  onClose: () => void;
}

function sender(message: MessageListItem, unknown: string): string {
  return message.from ? message.from.name?.trim() || message.from.email : unknown;
}

/** Which message is open first: the oldest unread one (reading goes on there), else the newest. */
export function initialEntry(entries: readonly ThreadEntry[], fallback: MessageId): MessageId {
  return entries.find((entry) => entry.message.unread)?.message.id ?? entries[entries.length - 1]?.message.id ?? fallback;
}

/**
 * A conversation in the reading pane (stage 7): every message of the thread, oldest first, as a
 * folded line with sender, date and the start of the text; the one being read is open below.
 * Unread messages are marked and the first of them opens first. A thread is always opened by a
 * message id (thread ids change when threads merge).
 */
export function ThreadPane({ account, item, mailboxId, focusRequest, toolbarFor, onEscape, onClose }: ThreadPaneProps) {
  const { t, i18n } = useTranslation();
  const thread = useThread(account.id, item.id, mailboxId);
  const entries = thread.data?.messages ?? [];
  const [chosen, setChosen] = useState<MessageId | null>(null);
  const openId = chosen !== null && entries.some((entry) => entry.message.id === chosen) ? chosen : initialEntry(entries, item.id);
  const open = entries.find((entry) => entry.message.id === openId)?.message ?? item;
  const total = thread.data?.total ?? entries.length;

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col" data-testid="thread-pane">
      {thread.isError && (
        <div className="px-4 pt-3">
          <ErrorNotice
            context={t("threads.loadFailed")}
            error={thread.error}
            onRetry={() => {
              void thread.refetch();
            }}
          />
        </div>
      )}
      {entries.length > 1 && (
        <section
          aria-label={t("threads.label", { count: total })}
          className="flex max-h-48 shrink-0 flex-col gap-1 overflow-y-auto border-b border-border bg-surface px-3 py-2"
          data-testid="thread-entries"
        >
          {total > entries.length && (
            <p className="px-2 text-12 text-secondary">{t("threads.truncated", { shown: entries.length, total })}</p>
          )}
          <ol className="flex flex-col">
            {entries.map((entry) => {
              const message = entry.message;
              const name = sender(message, t("list.unknownSender"));
              const date = formatFullDate(message.date, i18n.language);
              const current = message.id === openId;
              return (
                <li key={message.id}>
                  <button
                    type="button"
                    aria-current={current ? "true" : undefined}
                    aria-label={[t("threads.show", { sender: name, date }), message.unread && t("list.unread")].filter(Boolean).join(", ")}
                    onClick={() => {
                      setChosen(message.id);
                    }}
                    className={cx(
                      "flex h-8 w-full min-w-0 items-center gap-2 rounded-control px-2 text-start text-13",
                      current ? "bg-selected text-primary" : "text-secondary hover:bg-hover hover:text-primary",
                    )}
                    data-testid="thread-entry"
                  >
                    <span aria-hidden className={cx("size-(--ui-dot) shrink-0 rounded-full", message.unread ? "bg-accent" : "bg-transparent")} />
                    <span className={cx("w-(--ui-list-sender-width) shrink-0 truncate", message.unread && "font-semibold text-primary")}>
                      {name}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-tertiary">{message.preview?.trim() ?? ""}</span>
                    <span className="shrink-0 text-12 tabular-nums">{current ? t("threads.shown") : date}</span>
                  </button>
                </li>
              );
            })}
          </ol>
        </section>
      )}
      <ReadingPane
        key={open.id}
        account={account}
        messageId={open.id}
        focusRequest={focusRequest && focusRequest.messageId === item.id ? { ...focusRequest, messageId: open.id } : focusRequest}
        toolbar={toolbarFor?.(open)}
        onEscape={onEscape}
        onClose={onClose}
      />
    </div>
  );
}
