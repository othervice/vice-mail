import { useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { applyAccountStatus, invalidateAccountData } from "../features/accounts/queries";
import { handleAiEvent } from "../features/ai/queries";
import { handleComfortEvent } from "../features/comfort/events";
import { handleComposeEvent } from "../features/composer/queries";
import { handleContactsEvent } from "../features/contacts/queries";
// Stage 19 (decision 0057): mbox exports and imports.
import { handleMboxEvent } from "../features/mbox/MboxJobs";
import { handleExtrasEvent } from "../features/extras/queries";
import { operationsQueryKey, outboxCountsQueryKey } from "../features/actions/queries";
import { mailboxesQueryKey } from "../features/mailboxes/queries";
import { favoriteMailboxesQueryKey } from "../features/organize/queries";
import { handleMcpEvent } from "../features/mcp/events";
import { messagesQueryKey } from "../features/messages/queries";
import { preferencesQueryKey } from "../features/preferences/queries";
import { applyPrefetchStatus, messageBodyAvailable, messageBodyFailed } from "../features/reading/queries";
import { refreshAttachmentsView } from "../features/reading/attachmentQueries";
import { handleSettingsSyncEvent } from "../features/settings-sync/events";
import { handleSearchEvent } from "../features/search/events";
import { handleUiPreferencesEvent } from "../features/keyboard/useUiPreferencesSync";
import { handleUpdateEvent } from "../features/updates/queries";
import { handleDesktopEvent } from "../features/desktop/queries";
// Stage 22 (decision 0077): the learning spam filter.
import { handleSpamEvent } from "../features/spam/queries";
import { currentWindowRoute } from "../features/windows/route";
import { desktopIpc, ipc, toCoreError, type CoreError, type CoreEvent, type Unsubscribe } from "../services/ipc";
import { reportOwnFailures } from "../stores/notices";
import { useUiStore } from "../stores/ui";
import { refreshQuery } from "./refreshQuery";

/**
 * Feeds one core event into the query cache. The UI never polls: an event names what changed,
 * and only queries that are on screen ask the core again (`refetchType: "active"` is the
 * default); the rest are marked stale and reload when they are shown next.
 */
export function handleCoreEvent(queryClient: QueryClient, event: CoreEvent): void {
  // Stage 5: drafts, send states and the question after an unclear send.
  handleComposeEvent(queryClient, event);
  // Stages 12 and 11: AI access over MCP and the settings sync.
  handleMcpEvent(queryClient, event);
  handleSettingsSyncEvent(queryClient, event);
  // Stage 7: search results, conversations, unified inbox, groups and identities.
  handleSearchEvent(queryClient, event);
  // Stage 9: sorted inbox, Later, Pinned, Tasks, skipped local rules, notification clicks.
  handleComfortEvent(queryClient, event);
  // Stage 9 part B: mailto: links for the composer, invitations once a body arrived.
  handleExtrasEvent(queryClient, event);
  // Stage 13: progress of AI requests, the request log and the open-reply hints.
  handleAiEvent(queryClient, event);
  // Stage 8: UI preferences saved in another window or taken over from another device.
  handleUiPreferencesEvent(queryClient, event);
  // Stage 15 (decision 0037): update state; a new account may bring the update question.
  handleUpdateEvent(queryClient, event);
  // Stage 21 (decision 0069): the tray menu paused or resumed notifications.
  handleDesktopEvent(queryClient, event);
  // Stage 17 (decision 0050): contacts and their spam marks.
  handleContactsEvent(queryClient, event);
  handleMboxEvent(event);
  // Stage 22 (decision 0077): the spam filter's status and what it did with new mail.
  handleSpamEvent(queryClient, event);
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
      // Stage 16: favourites and folder colours come with this event too.
      refreshQuery(queryClient, favoriteMailboxesQueryKey);
      // Folders may have gone or been rebuilt (UIDVALIDITY): reload the lists of this account
      // too. The engine sends this together with `messagesChanged`; `refreshQuery` merges both.
      for (const query of queryClient.getQueryCache().findAll({ queryKey: ["messages", event.accountId] })) {
        refreshQuery(queryClient, query.queryKey);
      }
      break;
    case "messagesChanged":
      refreshQuery(queryClient, messagesQueryKey(event.accountId, event.mailboxId));
      // Stage 20 (decision 0060): stored bodies (prefetch, opening) add attachments to the view.
      refreshAttachmentsView(queryClient);
      break;
    case "newMail":
      // For notifications (a later stage); the list itself follows `messagesChanged`.
      break;
    case "messageBodyAvailable":
      messageBodyAvailable(queryClient, event.accountId, event.messageId, (key, stillWaits) => {
        refreshQuery(queryClient, key, stillWaits);
      });
      break;
    case "messageBodyFailed":
      messageBodyFailed(queryClient, event.accountId, event.messageId, event.error);
      break;
    case "prefetchProgress":
      applyPrefetchStatus(queryClient, event.accountId, event.status);
      break;
    case "linkActivated":
      // Only a click in a mail gets here (the navigation hook in Rust). The reading pane that
      // shows this rendering asks the core for the target and lets the user confirm it.
      useUiStore.getState().requestLink(event.renderToken, event.index);
      break;
    case "operationsChanged":
      // The status badge and, if it is open, the activity view. The rows themselves change
      // with `messagesChanged`. Failures of the user's own actions get a notice.
      refreshQuery(queryClient, outboxCountsQueryKey(event.accountId));
      refreshQuery(queryClient, operationsQueryKey(event.accountId));
      reportOwnFailures(event.operations);
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
          // Stage 21: the main window's page listens now; requests that waited for it follow.
          if (active && currentWindowRoute().kind === "main") void desktopIpc.mainWindowReady().catch(() => undefined);
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
