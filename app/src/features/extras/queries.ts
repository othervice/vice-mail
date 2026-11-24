import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { refreshQuery } from "../../app/refreshQuery";
import {
  extrasIpc,
  type AccountId,
  type CoreEvent,
  type MessageExtras,
  type MessageId,
  type UnsubscribeMethod,
} from "../../services/ipc";
import { useMailtoStore } from "../../stores/mailto";
import { currentWindowRoute } from "../windows/route";

/** Keyed by account first, like the message views, so events of one account can address them. */
export const messageExtrasQueryKey = (accountId: AccountId, messageId: MessageId) =>
  ["messageExtras", accountId, messageId] as const;

/**
 * Unsubscribe options and calendar invitations of the message in the reading pane. Purely
 * local: the core reads them from stored headers and the stored body, nothing is fetched.
 */
export function useMessageExtras(accountId: AccountId, messageId: MessageId) {
  return useQuery({
    queryKey: messageExtrasQueryKey(accountId, messageId),
    queryFn: () => extrasIpc.getMessageExtras(messageId),
  });
}

/** Only ever called from the user's click on one way to unsubscribe. */
export function useUnsubscribe(messageId: MessageId) {
  return useMutation({ mutationFn: (method: UnsubscribeMethod) => extrasIpc.unsubscribe(messageId, method) });
}

export function useOpenInvite(messageId: MessageId) {
  return useMutation({ mutationFn: (partId: number) => extrasIpc.openInvite(messageId, partId) });
}

export function useExportMessage() {
  return useMutation({ mutationFn: (messageId: MessageId) => extrasIpc.exportMessage(messageId) });
}

export function usePrintWindow() {
  return useMutation({ mutationFn: () => extrasIpc.printWindow() });
}

export const defaultMailAppQueryKey = ["defaultMailApp"] as const;

/** Asked when the settings show it: the user may have changed it in the system meanwhile. */
export function useDefaultMailApp() {
  return useQuery({ queryKey: defaultMailAppQueryKey, queryFn: extrasIpc.getDefaultMailApp, staleTime: 0 });
}

export function useSetDefaultMailApp() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => extrasIpc.setDefaultMailApp(),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: defaultMailAppQueryKey }),
  });
}

/** Extras read without the whole body (or not answered yet) have no invitations yet. */
const extrasWaitForBody = (extras: MessageExtras | undefined): boolean => !extras?.bodyComplete;

/**
 * Stage 9 part B events: a `mailto:` link the system opened with the app waits for the composer
 * (`composeRequested`); a body that arrived may bring calendar invitations
 * (`messageBodyAvailable`: the extras of that message are asked for again while they wait for it).
 */
export function handleExtrasEvent(queryClient: QueryClient, event: CoreEvent): void {
  switch (event.type) {
    case "composeRequested":
      // Every window hears it; a link from the system opens in the main window (decision 0027).
      if (currentWindowRoute().kind === "main") useMailtoStore.getState().enqueue([event.request]);
      break;
    case "messageBodyAvailable": {
      // Never asked: the first ask finds the body. Asked, with the answer still on its way: the
      // core may have read it before the body was stored, so it waits like one without the body.
      const key = messageExtrasQueryKey(event.accountId, event.messageId);
      if (queryClient.getQueryState(key) && extrasWaitForBody(queryClient.getQueryData<MessageExtras>(key))) {
        refreshQuery(queryClient, key, extrasWaitForBody);
      }
      break;
    }
    default:
      break;
  }
}
