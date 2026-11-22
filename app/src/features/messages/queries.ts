import { useInfiniteQuery } from "@tanstack/react-query";
import { ipc, type AccountId, type ListCursor, type MailboxId } from "../../services/ipc";

/** Rows per `listMessages` call. A page fills a tall window; the contract allows 1..500. */
export const MESSAGE_PAGE_SIZE = 200;

/** Keyed by account first, so `mailboxesChanged` and `accountsChanged` can address all lists of one account. */
export const messagesQueryKey = (accountId: AccountId, mailboxId: MailboxId) =>
  ["messages", accountId, mailboxId] as const;

/**
 * The message list of one folder, page by page (keyset cursor from the core). A refetch after a
 * core event reloads the pages already shown one after another, each from the fresh cursor of
 * the page before, so new or removed messages never leave a gap between pages.
 */
export function useMessagePages(accountId: AccountId, mailboxId: MailboxId) {
  return useInfiniteQuery({
    queryKey: messagesQueryKey(accountId, mailboxId),
    queryFn: ({ pageParam }) => ipc.listMessages(mailboxId, pageParam, MESSAGE_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
  });
}
