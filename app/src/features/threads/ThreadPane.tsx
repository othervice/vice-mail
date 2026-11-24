import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useEffectEvent, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, MessageId, MessageListItem, ThreadEntry } from "../../services/ipc";
import { ReadingPane, type FocusRequest } from "../reading/ReadingPane";
import { useAiSourceFocus } from "../ai/sources";
import { conversationQuery } from "./queries";

interface ThreadPaneProps {
  account: Account;
  /** The conversation row the user opened: its message is the newest in the listed folder. */
  item: MessageListItem;
  /** The row's identity in the list (its thread): the pane stays while a newer message becomes the row's. */
  ident: string;
  focusRequest: FocusRequest | null;
  /** Mail actions for the open message (its own folder's). */
  toolbarFor?: (message: MessageListItem) => ReactNode;
  /** The message the pane shows, once it is known (marking as read on open is the view's job). */
  onShown?: (message: MessageListItem) => void;
  /** A source of an AI answer among the conversation's other messages: show it (in the list). */
  onShowMessage?: (messageId: MessageId) => void;
  onEscape: () => void;
  onClose: () => void;
}

/** Which message is open first: the oldest unread one (reading goes on there), else the newest. */
export function initialEntry(entries: readonly ThreadEntry[], fallback: MessageId): MessageId {
  return entries.find((entry) => entry.message.unread)?.message.id ?? entries[entries.length - 1]?.message.id ?? fallback;
}

/**
 * Stage 17 (decision 0049): the reading pane of a conversation row shows exactly one message: the
 * conversation's oldest unread one, else its newest (in any folder, the user's own answer in Sent
 * too). Which one is decided once, when the conversation is known, and kept while the row stays
 * selected: marking it read, or a reply that becomes the row's message, do not switch the pane.
 * The conversation's other messages are child rows in the list. The AI chips under the message
 * work on the row's message (the panel's conversation), and a source of an AI answer among the
 * conversation's messages is shown through the list (`onShowMessage`). A thread is always opened by
 * a message id (thread ids change when threads merge).
 */
export function ThreadPane({ account, item, ident, focusRequest, toolbarFor, onShown, onShowMessage, onEscape, onClose }: ThreadPaneProps) {
  const { t } = useTranslation();
  const thread = useQuery(conversationQuery(item, ident));
  const entries = thread.data?.messages ?? [];
  const [opened, setOpened] = useState<MessageListItem | null>(null);
  if (opened === null && (thread.data || thread.isError)) {
    const id = thread.data ? initialEntry(entries, item.id) : item.id;
    setOpened(entries.find((entry) => entry.message.id === id)?.message ?? item);
  }
  // Its newest state (read, flagged) while it is part of the conversation, else as it was.
  const current = opened ? (entries.find((entry) => entry.message.id === opened.id)?.message ?? opened) : null;

  const shown = useEffectEvent((message: MessageListItem) => {
    onShown?.(message);
  });
  useEffect(() => {
    if (current) shown(current);
  }, [current]);
  // Stage 13: the conversation's messages count as shown here; another one of them is shown by
  // selecting its row.
  const currentId = current?.id ?? null;
  const show = useCallback(
    (messageId: MessageId) => {
      if (messageId !== currentId) onShowMessage?.(messageId);
    },
    [currentId, onShowMessage],
  );
  useAiSourceFocus(
    entries.map((entry) => entry.message.id),
    show,
  );

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col bg-surface" data-testid="thread-pane" aria-busy={current === null}>
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
      {current && (
        <ReadingPane
          account={account}
          messageId={current.id}
          title={item.subject}
          draft={current.draft}
          // The AI panel works on the row's message (its conversation); its chips stand under
          // whichever message of the conversation is open.
          aiMessageId={item.id}
          focusRequest={focusRequest && focusRequest.messageId === item.id ? { ...focusRequest, messageId: current.id } : focusRequest}
          toolbar={toolbarFor?.(current)}
          onEscape={onEscape}
          onClose={onClose}
        />
      )}
    </div>
  );
}
