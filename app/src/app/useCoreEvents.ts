import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { applyAccountStatus, invalidateAccountData } from "../features/accounts/queries";
import { mailboxesQueryKey } from "../features/mailboxes/queries";
import { messagesQueryKey } from "../features/messages/queries";
import { preferencesQueryKey } from "../features/preferences/queries";
import { ipc, toCoreError, type CoreError, type CoreEvent, type Unsubscribe } from "../services/ipc";
import { refreshQuery } from "./refreshQuery";

/**
 * Feeds one core event into the query cache. The UI never polls: an event names what changed,
 * and only queries that are on screen ask the core again (`refetchType: "active"` is the
 * default); the rest are marked stale and reload when they are shown next.
 */
export function handleCoreEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "preferencesChanged":
      // Another window (or the core itself) changed them; the effects re-apply theme and language.
      queryClient.setQueryData(preferencesQueryKey, event.preferences);
      break;
    case "accountsChanged":
      void invalidateAccountData(queryClient);
      break;
    case "accountStatusChanged":
      applyAccountStatus(queryClient, event.accountId, event.status);
      break;
    case "mailboxesChanged":
      refreshQuery(queryClient, mailboxesQueryKey(event.accountId));
      // Folders may have gone or been rebuilt (UIDVALIDITY): reload the lists of this account
      // too. The engine sends this together with `messagesChanged`; `refreshQuery` merges both.
      for (const query of queryClient.getQueryCache().findAll({ queryKey: ["messages", event.accountId] })) {
        refreshQuery(queryClient, query.queryKey);
      }
      break;
    case "messagesChanged":
      refreshQuery(queryClient, messagesQueryKey(event.accountId, event.mailboxId));
      break;
    case "newMail":
      // For notifications (a later stage); the list itself follows `messagesChanged`.
      break;
  }
}

/**
 * Subscribes once to the typed event stream of the core and feeds it into the query cache.
 * Returns the subscription error, if any, so the frame can show it.
 */
export function useCoreEvents(): CoreError | null {
  const queryClient = useQueryClient();
  const [error, setError] = useState<CoreError | null>(null);

  useEffect(() => {
    let active = true;
    let unsubscribe: Unsubscribe | undefined;
    ipc
      .onCoreEvent((event) => {
        handleCoreEvent(queryClient, event);
      })
      .then(
        (stop) => {
          if (active) unsubscribe = stop;
          else stop();
        },
        (e: unknown) => {
          if (active) setError(toCoreError(e));
        },
      );
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [queryClient]);

  return error;
}
