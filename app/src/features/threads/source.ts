import { useMemo } from "react";
import type { AccountId, MailboxId, MessageId, MessageListItem } from "../../services/ipc";
import type { MessageListSource } from "../messages/listSource";
import { buildRows } from "../messages/rows";
import { useThreadPages, type ThreadListPage } from "./queries";

/** Conversation sizes by the id of the row that shows each thread. */
function useCounts(pages: readonly ThreadListPage[] | undefined): ReadonlyMap<MessageId, number> {
  return useMemo(
    () => new Map((pages ?? []).flatMap((page) => page.threads).map((thread) => [thread.message.id, thread.messageCount] as const)),
    [pages],
  );
}

function decorateWith(counts: ReadonlyMap<MessageId, number>) {
  return (item: MessageListItem) => {
    const count = counts.get(item.id) ?? 1;
    return count > 1 ? { threadCount: count } : undefined;
  };
}

/** The list source of a folder in conversation view; undefined while the view is off. */
export function useThreadSource(accountId: AccountId, mailboxId: MailboxId, enabled: boolean): MessageListSource | undefined {
  const query = useThreadPages(accountId, mailboxId, enabled);
  const counts = useCounts(query.data?.pages);
  if (!enabled) return undefined;
  return { query, buildRows, decorate: decorateWith(counts) };
}
