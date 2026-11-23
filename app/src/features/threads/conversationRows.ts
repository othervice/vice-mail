import type {
  Address,
  ConversationSummary,
  InboxPage,
  InboxRow,
  MessageId,
  MessageListItem,
  ThreadListItem,
} from "../../services/ipc";
import type { RowDecoration } from "../messages/MessageRow";

/**
 * Decision 0039: what a conversation row shows besides its message (the conversation's newest in
 * the listed folder): the size, the senders, and whether any of its messages is unread, flagged
 * or has attachments. Pure, tested without React.
 */
export interface ConversationInfo {
  threadId: number | null;
  messageCount: number;
  unreadCount: number;
  participants: readonly Address[];
  hasAttachments: boolean;
  flagged: boolean;
}

export function infoOfThread(thread: ThreadListItem): ConversationInfo {
  return {
    threadId: thread.threadId,
    messageCount: thread.messageCount,
    unreadCount: thread.unreadCount,
    participants: thread.participants,
    hasAttachments: thread.hasAttachments,
    flagged: thread.flagged,
  };
}

export function infoOfSummary(summary: ConversationSummary): ConversationInfo {
  return {
    threadId: summary.threadId,
    messageCount: summary.messageCount,
    unreadCount: summary.unreadCount,
    participants: summary.participants,
    hasAttachments: summary.hasAttachments,
    flagged: summary.flagged,
  };
}

/**
 * The row's message as the conversation: unread when any message is, flagged when any is, with
 * the paper clip when any has attachments. Toggles of the action directory then name what an
 * action on the conversation does ("Mark as read" while any message is unread). `keepUnread`
 * keeps the message's own state ("unread first" groups by it).
 */
export function asConversation(item: MessageListItem, info: ConversationInfo | undefined, keepUnread = false): MessageListItem {
  if (!info) return item;
  const unread = keepUnread ? item.unread : item.unread || info.unreadCount > 0;
  const flagged = item.flagged || info.flagged;
  const hasAttachments = item.hasAttachments || info.hasAttachments;
  if (unread === item.unread && flagged === item.flagged && hasAttachments === item.hasAttachments) return item;
  return { ...item, unread, flagged, hasAttachments };
}

/** What a conversation row adds: its size (when more than one) and its senders. */
export function conversationDecoration(info: ConversationInfo | undefined): RowDecoration | undefined {
  if (!info || info.messageCount <= 1) return undefined;
  return { threadCount: info.messageCount, participants: info.participants };
}

/**
 * Conversation info by row message id. A plain object, not a `Map`: list pages hold it, and
 * TanStack's structural sharing keeps equal plain data stable between selects (a new `Map` each
 * time would make every page new and the list's selection report again, endlessly).
 */
export type ConversationInfoById = Record<number, ConversationInfo>;

/** The info of every page, by row message id. */
export function mergeInfo(pages: readonly { info?: ConversationInfoById }[] | undefined): ReadonlyMap<MessageId, ConversationInfo> {
  const all = new Map<MessageId, ConversationInfo>();
  for (const page of pages ?? []) for (const [id, info] of Object.entries(page.info ?? {})) all.set(Number(id), info);
  return all;
}

/** Conversation rows of a thread list as message rows, plus their info by row message id. */
export function threadRows(threads: readonly ThreadListItem[], keepUnread = false): { items: MessageListItem[]; info: ConversationInfoById } {
  const info: ConversationInfoById = {};
  const items = threads.map((thread) => {
    const entry = infoOfThread(thread);
    info[thread.message.id] = entry;
    return asConversation(thread.message, entry, keepUnread);
  });
  return { items, info };
}

/**
 * A page of `listInboxConversations` with every message (rows, bundle members, top) replaced by
 * its conversation (`asConversation`). The page keeps its shape, so the sorted inbox builds its
 * blocks, sections and bundles as before.
 */
export function conversationInboxPage(page: InboxPage): { page: InboxPage; info: ConversationInfoById } {
  const info: ConversationInfoById = Object.fromEntries(page.conversations.map((summary) => [summary.messageId, infoOfSummary(summary)]));
  const swap = (item: MessageListItem) => asConversation(item, info[item.id]);
  const rows = page.rows.map((row): InboxRow => (row.kind === "message" ? { ...row, item: swap(row.item) } : { ...row, members: row.members.map(swap) }));
  const top = page.top && {
    ...page.top,
    pinned: page.top.pinned.map(swap),
    returned: page.top.returned.map(swap),
    priority: page.top.priority.map(swap),
  };
  return { page: { ...page, rows, top }, info };
}

/**
 * The key a conversation row is shown once by: its thread, else its message. The priority block
 * and the rows can hold the same conversation at different messages (decision 0039).
 */
export function conversationKey(info: ReadonlyMap<MessageId, ConversationInfo>): (item: MessageListItem) => string {
  return (item) => {
    const thread = info.get(item.id)?.threadId;
    return thread === null || thread === undefined ? `m${String(item.id)}` : `t${String(thread)}`;
  };
}

/**
 * Search hits grouped by conversation: the first (newest) hit of a thread is its row, later hits
 * of the same thread are left out. Hits without a thread stay rows of their own.
 */
export function groupByThread<T>(entries: readonly T[], threadOf: (entry: T) => number | null): T[] {
  const seen = new Set<number>();
  return entries.filter((entry) => {
    const thread = threadOf(entry);
    if (thread === null) return true;
    if (seen.has(thread)) return false;
    seen.add(thread);
    return true;
  });
}
