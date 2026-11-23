import { useMutation, useQueries, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { refreshQuery } from "../../app/refreshQuery";
import {
  composeIpc,
  type Account,
  type AccountId,
  type ComposeIdentity,
  type ComposeSettings,
  type CoreEvent,
  type OutgoingMail,
  type SentCopyMode,
} from "../../services/ipc";
import { useComposerStore } from "../../stores/composer";

export const composeIdentitiesQueryKey = (accountId: AccountId) => ["composeIdentities", accountId] as const;
export const composeSettingsQueryKey = ["composeSettings"] as const;
export const sendSettingsQueryKey = (accountId: AccountId) => ["sendSettings", accountId] as const;
export const outgoingQueryKey = (accountId: AccountId) => ["outgoing", accountId] as const;
export const draftsQueryKey = ["drafts"] as const;

/** Sends per account the outbox view shows (newest first). */
export const OUTGOING_PAGE_SIZE = 200;
/** Suggestions under a recipient field. */
export const CONTACT_SUGGESTIONS = 8;
/** Wait after the last key before asking for suggestions (contract: about 150 ms). */
export const CONTACT_DEBOUNCE_MS = 150;

/** Sender choice with signatures, default first. */
export function useComposeIdentities(accountId: AccountId | null) {
  return useQuery({
    queryKey: composeIdentitiesQueryKey(accountId ?? ""),
    queryFn: () => composeIpc.listComposeIdentities(accountId ?? ""),
    enabled: accountId !== null,
  });
}

export function useComposeSettings() {
  return useQuery({ queryKey: composeSettingsQueryKey, queryFn: composeIpc.getComposeSettings });
}

/** Saves at once in the UI and rolls back if the core refuses. */
export function useSetComposeSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: composeIpc.setComposeSettings,
    onMutate: async (next: ComposeSettings) => {
      await queryClient.cancelQueries({ queryKey: composeSettingsQueryKey });
      const previous = queryClient.getQueryData<ComposeSettings>(composeSettingsQueryKey);
      queryClient.setQueryData(composeSettingsQueryKey, next);
      return { previous };
    },
    onError: (_error, _next, context) => {
      if (context?.previous) queryClient.setQueryData(composeSettingsQueryKey, context.previous);
    },
    onSuccess: (saved) => {
      queryClient.setQueryData(composeSettingsQueryKey, saved);
    },
  });
}

export function useSendSettings(accountId: AccountId) {
  return useQuery({ queryKey: sendSettingsQueryKey(accountId), queryFn: () => composeIpc.getSendSettings(accountId) });
}

export function useSetSentCopyMode() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, mode }: { accountId: AccountId; mode: SentCopyMode }) =>
      composeIpc.setSentCopyMode(accountId, mode),
    onSuccess: (saved) => {
      queryClient.setQueryData(sendSettingsQueryKey(saved.accountId), saved);
    },
  });
}

export function useSetIdentitySignature() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ identity, signature }: { identity: ComposeIdentity; signature: string | null }) =>
      composeIpc.setIdentitySignature(identity.id, signature),
    onSuccess: (saved) => {
      queryClient.setQueryData<ComposeIdentity[]>(composeIdentitiesQueryKey(saved.accountId), (list) =>
        list?.map((identity) => (identity.id === saved.id ? saved : identity)),
      );
    },
  });
}

/** The outbox view of every account; `outgoingChanged` refreshes it. */
export function useOutgoing(accounts: readonly Account[]) {
  return useQueries({
    queries: accounts.map((account) => ({
      queryKey: outgoingQueryKey(account.id),
      queryFn: () => composeIpc.listOutgoing(account.id, null, OUTGOING_PAGE_SIZE),
    })),
  });
}

/** Local drafts of every account, newest first; `draftsChanged` refreshes them. */
export function useDrafts() {
  return useQuery({ queryKey: draftsQueryKey, queryFn: () => composeIpc.listDrafts(null) });
}

/** The local drafts as they were when the app started (left from a crash or quitting): asked once. */
export function useDraftsAtStart() {
  return useQuery({
    queryKey: ["draftsAtStart"] as const,
    queryFn: () => composeIpc.listDrafts(null),
    staleTime: Infinity,
    gcTime: Infinity,
  });
}

/** Suggestions for what is typed in a recipient field, asked for once typing pauses. */
export function useContactSuggestions(query: string) {
  const [debounced, setDebounced] = useState(query);
  useEffect(() => {
    const timer = window.setTimeout(() => {
      setDebounced(query);
    }, CONTACT_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [query]);
  const term = debounced.trim();
  return useQuery({
    queryKey: ["contacts", term] as const,
    queryFn: () => composeIpc.searchContacts(term, CONTACT_SUGGESTIONS),
    enabled: term.length > 0 && term === query.trim(),
    staleTime: 30_000,
  });
}

/** Puts new send states into the outbox list of the account (newest first) without a reload. */
function mergeOutgoing(queryClient: QueryClient, accountId: AccountId, mails: readonly OutgoingMail[]): void {
  const key = outgoingQueryKey(accountId);
  const current = queryClient.getQueryData<OutgoingMail[]>(key);
  if (!current) {
    refreshQuery(queryClient, key);
    return;
  }
  const byId = new Map(current.map((mail) => [mail.operationId, mail]));
  for (const mail of mails) byId.set(mail.operationId, mail);
  queryClient.setQueryData(
    key,
    [...byId.values()].sort((a, b) => b.operationId - a.operationId),
  );
}

/**
 * The stage 5 events: drafts, send states and the question after an unclear send. Called from
 * `handleCoreEvent`; ignores every other event.
 */
export function handleComposeEvent(queryClient: QueryClient, event: CoreEvent): void {
  const store = useComposerStore.getState();
  switch (event.type) {
    case "draftsChanged":
      refreshQuery(queryClient, draftsQueryKey);
      // Deleted or sent elsewhere (another window): the open composer has nothing left to edit.
      if (store.openDraftId && event.removed.includes(store.openDraftId)) store.close();
      break;
    case "outgoingChanged":
      mergeOutgoing(queryClient, event.accountId, event.mails);
      store.update(event.mails);
      break;
    case "sendNeedsDecision":
      mergeOutgoing(queryClient, event.accountId, [event.mail]);
      store.needsDecision(event.mail);
      break;
    case "accountsChanged":
      // Signatures and identities changed (`setIdentitySignature` emits it too).
      void queryClient.invalidateQueries({ queryKey: ["composeIdentities"] });
      break;
    default:
      break;
  }
}
