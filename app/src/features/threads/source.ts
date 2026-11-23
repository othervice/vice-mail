import { useMemo } from "react";
import type { AccountId, MailboxId, MessageId, MessageListItem, ThreadListItem } from "../../services/ipc";
import type { MessageListSource } from "../messages/listSource";
import { buildRows } from "../messages/rows";
import { conversationDecoration, infoOfThread, type ConversationInfo } from "./conversationRows";
import { useThreadPages } from "./queries";

/** Conversation info by the id of the row that shows each thread. */
export function useThreadInfo(threads: readonly ThreadListItem[] | undefined): ReadonlyMap<MessageId, ConversationInfo> {
  return useMemo(() => new Map((threads ?? []).map((thread) => [thread.message.id, infoOfThread(thread)] as const)), [threads]);
}

/** The decoration of conversation rows: size and senders (decision 0039). */
export function decorateConversations(info: ReadonlyMap<MessageId, ConversationInfo>) {
  return (item: MessageListItem) => conversationDecoration(info.get(item.id));
}

/** The list source of a folder in conversation view; undefined while the view is off. */
export function useThreadSource(accountId: AccountId, mailboxId: MailboxId, enabled: boolean): MessageListSource | undefined {
  const query = useThreadPages(accountId, mailboxId, enabled);
  const threads = useMemo(() => query.data?.pages.flatMap((page) => page.threads), [query.data]);
  const info = useThreadInfo(threads);
  if (!enabled) return undefined;
  return { query, buildRows, decorate: decorateConversations(info) };
}
