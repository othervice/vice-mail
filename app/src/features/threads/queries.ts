import { queryOptions, useInfiniteQuery, useQuery, type InfiniteData } from "@tanstack/react-query";
import {
  searchIpc,
  type AccountId,
  type ListCursor,
  type MailboxId,
  type MessageId,
  type MessageListItem,
  type ThreadListItem,
  type ThreadPage,
} from "../../services/ipc";
import { MESSAGE_PAGE_SIZE } from "../messages/queries";
import { threadRows } from "./conversationRows";

export const threadsQueryKey = (accountId: AccountId, mailboxId: MailboxId) => ["threads", accountId, mailboxId] as const;
export const threadQueryKey = (accountId: AccountId, messageId: MessageId, mailboxId: MailboxId | null) =>
  ["thread", accountId, messageId, mailboxId] as const;

/**
 * A page of conversations, plus the rows the message list shows: the newest message of each, as
 * the conversation (unread, flagged or with attachments when any of its messages is).
 */
export interface ThreadListPage {
  threads: ThreadListItem[];
  items: MessageListItem[];
  nextCursor: ThreadPage["nextCursor"];
}

function withItems(data: InfiniteData<ThreadPage, ListCursor | null>): InfiniteData<ThreadListPage, ListCursor | null> {
  return {
    ...data,
    pages: data.pages.map((page) => ({
      threads: page.items,
      items: threadRows(page.items).items,
      nextCursor: page.nextCursor,
    })),
  };
}

/** The conversations of one folder, page by page. */
export function useThreadPages(accountId: AccountId, mailboxId: MailboxId, enabled: boolean) {
  return useInfiniteQuery({
    queryKey: threadsQueryKey(accountId, mailboxId),
    queryFn: ({ pageParam }) => searchIpc.listThreads(mailboxId, pageParam, MESSAGE_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled,
    select: withItems,
  });
}

/** Every message of the thread `messageId` belongs to, oldest first; opened by message id. */
export function useThread(accountId: AccountId, messageId: MessageId, mailboxId: MailboxId | null) {
  return useQuery({
    queryKey: threadQueryKey(accountId, messageId, mailboxId),
    queryFn: () => searchIpc.getThread(messageId, mailboxId),
  });
}

/**
 * Stage 17 (decision 0049): the messages of a conversation row, for its child rows and for the one
 * message the reading pane opens. Keyed by the row's identity (`tree.ts`: its thread), not by its
 * message: when a reply becomes the row's message, the unfolded children and the open message stay
 * while they reload. Under `["thread", account]`, which core events reload (`search/events.ts`).
 */
export const conversationQueryKey = (accountId: AccountId, ident: string, mailboxId: MailboxId) =>
  ["thread", accountId, "conversation", ident, mailboxId] as const;

export function conversationQuery(item: MessageListItem, ident: string) {
  return queryOptions({
    queryKey: conversationQueryKey(item.accountId, ident, item.mailboxId),
    queryFn: () => searchIpc.getThread(item.id, item.mailboxId),
  });
}
