import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { Avatar } from "../accounts/Avatar";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, MailboxId, MessageId, MessageListItem, ThreadEntry } from "../../services/ipc";
import { dayBounds, formatFullDate, formatListDate } from "../messages/rows";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";
import { useAiSourceFocus } from "../ai/sources";
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
 * A conversation in the reading pane (stage 7): the subject as the title, then every message of
 * the thread, oldest first, as a card. The one being read is open (head, body, reply row); the
 * others are folded to one line with sender, the start of the text and the date, and open on a
 * click. Unread messages are marked and the first of them opens first. A thread is always opened
 * by a message id (thread ids change when threads merge).
 */
export function ThreadPane({ account, item, mailboxId, focusRequest, toolbarFor, onEscape, onClose }: ThreadPaneProps) {
  const { t, i18n } = useTranslation();
  const thread = useThread(account.id, item.id, mailboxId);
  const entries = thread.data?.messages ?? [];
  const [chosen, setChosen] = useState<MessageId | null>(null);
  // Stage 13: a source of an AI statement in the panel shows its mail here.
  useAiSourceFocus(
    entries.map((entry) => entry.message.id),
    setChosen,
  );
  const openId = chosen !== null && entries.some((entry) => entry.message.id === chosen) ? chosen : initialEntry(entries, item.id);
  const open = entries.find((entry) => entry.message.id === openId)?.message ?? item;
  const total = thread.data?.total ?? entries.length;
  const bounds = dayBounds(new Date(), 1);

  const conversation =
    entries.length > 1
      ? (card: ReactNode) => (
          <section aria-label={t("threads.label", { count: total })} className="flex flex-col gap-2" data-testid="thread-entries">
            {total > entries.length && (
              <p className="px-1 text-12 text-secondary">{t("threads.truncated", { shown: entries.length, total })}</p>
            )}
            <ol className="flex flex-col gap-2">
              {entries.map((entry) => {
                const message = entry.message;
                const name = sender(message, t("list.unknownSender"));
                const date = formatFullDate(message.date, i18n.language);
                const current = message.id === openId;
                return (
                  <li key={message.id} className="flex flex-col">
                    {/* The open one is its card below; its line stays (hidden) so the list keeps its entries. */}
                    <button
                      type="button"
                      hidden={current}
                      aria-current={current ? "true" : undefined}
                      aria-label={[t("threads.show", { sender: name, date }), message.unread && t("list.unread")].filter(Boolean).join(", ")}
                      onClick={() => {
                        setChosen(message.id);
                      }}
                      className="flex h-12 w-full min-w-0 items-center gap-3 rounded-card border border-border-subtle bg-surface-raised px-3 text-start text-13 shadow-card transition-colors hover:bg-hover"
                      data-testid="thread-entry"
                    >
                      <span className="relative flex shrink-0">
                        <Avatar name={message.from?.name} email={message.from?.email ?? ""} accountColor={account.color} ring={false} />
                        {message.unread && (
                          <span aria-hidden className="absolute -start-1 -top-1 size-(--ui-dot) rounded-full bg-accent ring-2 ring-surface-raised" />
                        )}
                      </span>
                      <span className={cx("w-(--ui-list-sender-width) max-w-1/3 shrink-0 truncate text-primary", message.unread ? "font-semibold" : "font-medium")}>
                        {name}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-secondary">{message.preview?.trim() ?? ""}</span>
                      <span className="shrink-0 text-12 text-tertiary tabular-nums">{formatListDate(message.date, bounds, i18n.language)}</span>
                    </button>
                    {current && card}
                  </li>
                );
              })}
            </ol>
          </section>
        )
      : undefined;

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
      <ReadingPane
        // One pane for the conversation: opening another of its messages keeps the list in place.
        key={item.id}
        account={account}
        messageId={open.id}
        title={item.subject}
        draft={open.draft}
        focusRequest={focusRequest && focusRequest.messageId === item.id ? { ...focusRequest, messageId: open.id } : focusRequest}
        toolbar={toolbarFor?.(open)}
        onEscape={onEscape}
        onClose={onClose}
        {...(conversation && { conversation })}
      />
    </div>
  );
}
