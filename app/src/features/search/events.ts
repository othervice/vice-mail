import type { QueryClient, QueryKey } from "@tanstack/react-query";
import { refreshQuery } from "../../app/refreshQuery";
import type { CoreEvent } from "../../services/ipc";

/** Reloads every cached query under `prefix` that is on screen (each coalesced on its own). */
function refreshAll(queryClient: QueryClient, prefix: QueryKey): void {
  for (const query of queryClient.getQueryCache().findAll({ queryKey: prefix })) {
    refreshQuery(queryClient, query.queryKey);
  }
}

/** Query prefixes of stage 7 whose rows come from messages of any account. */
const MIXED_LISTS: readonly QueryKey[] = [["search"], ["unified"]];

/**
 * Stage 7: core events for search results, conversation lists, the unified inbox, account
 * groups, hidden accounts and identities. There are no new events; the existing ones say what
 * changed (contract: `messagesChanged`/`mailboxesChanged` reload the lists concerned).
 */
export function handleSearchEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "messagesChanged":
      refreshQuery(queryClient, ["threads", event.accountId, event.mailboxId]);
      refreshAll(queryClient, ["thread", event.accountId]);
      for (const prefix of MIXED_LISTS) refreshAll(queryClient, prefix);
      break;
    case "mailboxesChanged":
      refreshAll(queryClient, ["threads", event.accountId]);
      refreshAll(queryClient, ["thread", event.accountId]);
      for (const prefix of MIXED_LISTS) refreshAll(queryClient, prefix);
      break;
    case "inboxChanged":
      // Stage 9 marks or categories changed: the unified inbox is a sorted inbox too.
      refreshAll(queryClient, ["unified"]);
      break;
    case "accountsChanged":
      // Groups, hidden accounts and identities change with this event, and so may every list.
      for (const prefix of [["accountGroups"], ["hiddenAccounts"], ["identities"], ["threads"], ["thread"], ...MIXED_LISTS]) {
        void queryClient.invalidateQueries({ queryKey: prefix });
      }
      break;
    default:
      break;
  }
}
