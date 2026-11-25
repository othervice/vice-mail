import { useInfiniteQuery, useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ipc,
  type Account,
  type AccountId,
  type OperationId,
  type OperationInfo,
  type OutboxCounts,
  type ReadingPreferences,
} from "../../services/ipc";
import { accountsQueryKey } from "../accounts/queries";

export const outboxCountsQueryKey = (accountId: AccountId) => ["outboxCounts", accountId] as const;
export const operationsQueryKey = (accountId: AccountId) => ["operations", accountId] as const;
export const readingPreferencesQueryKey = ["readingPreferences"] as const;

/** Operations per page of the activity view. */
export const OPERATIONS_PAGE_SIZE = 100;

/** Pending, failed and conflicting operations of every account; `operationsChanged` refreshes them. */
export function useOutboxCounts(accounts: readonly Account[]): OutboxCounts[] {
  return useQueries({
    queries: accounts.map((account) => ({
      queryKey: outboxCountsQueryKey(account.id),
      queryFn: () => ipc.getOutboxCounts(account.id),
    })),
    combine: (results) => results.flatMap((result) => (result.data ? [result.data] : [])),
  });
}

/** The activity of one account, newest first, keyset-paged by the last operation id. */
export function useOperations(accountId: AccountId, enabled = true) {
  return useInfiniteQuery({
    queryKey: operationsQueryKey(accountId),
    queryFn: ({ pageParam }) => ipc.listOperations(accountId, pageParam, OPERATIONS_PAGE_SIZE),
    initialPageParam: null as OperationId | null,
    enabled,
    getNextPageParam: (lastPage: OperationInfo[]) =>
      lastPage.length === OPERATIONS_PAGE_SIZE ? (lastPage[lastPage.length - 1]?.id ?? null) : null,
  });
}

/** Retries failed or conflicting operations (each becomes a new operation of the same batch). */
export function useRetryOperations() {
  return useMutation({
    mutationFn: async (ids: readonly OperationId[]) => {
      for (const id of ids) await ipc.retryOperation(id);
    },
  });
}

export function useDiscardOperations() {
  return useMutation({
    mutationFn: async (ids: readonly OperationId[]) => {
      for (const id of ids) await ipc.discardOperation(id);
    },
  });
}

/** The per-account switch for writes. The answer is the account; `accountsChanged` follows. */
export function useSetWritesEnabled() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, enabled }: { accountId: AccountId; enabled: boolean }) =>
      ipc.setWritesEnabled(accountId, enabled),
    onSuccess: (account) => {
      queryClient.setQueryData<Account[]>(accountsQueryKey, (accounts) =>
        accounts?.map((a) => (a.id === account.id ? account : a)),
      );
    },
  });
}

export function useReadingPreferences() {
  return useQuery({ queryKey: readingPreferencesQueryKey, queryFn: ipc.getReadingPreferences });
}

/** Saves at once in the UI and rolls back if the core refuses (like the other settings). */
export function useSetReadingPreferences() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ipc.setReadingPreferences,
    onMutate: async (next: ReadingPreferences) => {
      await queryClient.cancelQueries({ queryKey: readingPreferencesQueryKey });
      const previous = queryClient.getQueryData<ReadingPreferences>(readingPreferencesQueryKey);
      queryClient.setQueryData(readingPreferencesQueryKey, next);
      return { previous };
    },
    onError: (_error, _next, context) => {
      if (context?.previous) queryClient.setQueryData(readingPreferencesQueryKey, context.previous);
    },
    onSuccess: (saved, _next, context) => {
      queryClient.setQueryData(readingPreferencesQueryKey, saved);
      // Stage 21 (decision 0066): the open mail shows the new choice for the dark theme at once.
      if ((context.previous?.darkMails ?? "adapted") !== (saved.darkMails ?? "adapted")) {
        void queryClient.invalidateQueries({ queryKey: ["message"] });
      }
    },
  });
}
