import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  ipc,
  type Account,
  type AccountId,
  type AccountSettings,
  type AccountStatus,
} from "../../services/ipc";

export const accountsQueryKey = ["accounts"] as const;

/** All accounts, oldest first, with their live status (kept current by `accountStatusChanged`). */
export function useAccounts() {
  return useQuery({ queryKey: accountsQueryKey, queryFn: ipc.listAccounts });
}

/** Puts a status from `accountStatusChanged` into the cached list, without asking the core again. */
export function applyAccountStatus(queryClient: QueryClient, accountId: AccountId, status: AccountStatus): void {
  queryClient.setQueryData<Account[]>(accountsQueryKey, (accounts) =>
    accounts?.map((account) => (account.id === accountId ? { ...account, status } : account)),
  );
}

/**
 * Everything cached about accounts, folders and messages is stale after an account was added or
 * removed (a removed account takes its local data with it).
 */
export async function invalidateAccountData(queryClient: QueryClient): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: accountsQueryKey }),
    queryClient.invalidateQueries({ queryKey: ["mailboxes"] }),
    queryClient.invalidateQueries({ queryKey: ["messages"] }),
    queryClient.invalidateQueries({ queryKey: ["outboxCounts"] }),
    queryClient.invalidateQueries({ queryKey: ["operations"] }),
  ]);
}

export interface AccountCredentials {
  settings: AccountSettings;
  password: string;
}

/**
 * Server settings from the core's built-in table of well-known providers. Purely local, so it
 * runs while the user types (debounced by the caller); nothing leaves the computer.
 */
export function useKnownProvider(email: string | null) {
  return useQuery({
    queryKey: ["discovery", "known", email ?? ""],
    queryFn: () => ipc.discoverKnownProvider(email ?? ""),
    enabled: email !== null,
  });
}

/** DNS SRV and the domain's autoconfig file. Contacts the network, so only ever on a click. */
export function useDiscoverOnline() {
  return useMutation({ mutationFn: (email: string) => ipc.discoverOnline(email) });
}

export function useTestConnection() {
  return useMutation({
    mutationFn: ({ settings, password }: AccountCredentials) => ipc.testConnection(settings, password),
  });
}

export function useAddAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ settings, password }: AccountCredentials) => ipc.addAccount(settings, password),
    onSuccess: async () => {
      // The core also emits `accountsChanged`; this makes the new account visible without waiting.
      await invalidateAccountData(queryClient);
    },
  });
}

export function useRemoveAccount() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (accountId: AccountId) => ipc.removeAccount(accountId),
    onSuccess: async () => {
      await invalidateAccountData(queryClient);
    },
  });
}

export function useRetryAccount() {
  return useMutation({ mutationFn: (accountId: AccountId) => ipc.retryAccount(accountId) });
}

export function useUpdatePassword() {
  return useMutation({
    mutationFn: ({ accountId, password }: { accountId: AccountId; password: string }) =>
      ipc.updatePassword(accountId, password),
  });
}
