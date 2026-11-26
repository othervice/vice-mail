import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { searchIpc, type AccountGroupId, type AccountId } from "../../services/ipc";

export const accountGroupsQueryKey = ["accountGroups"] as const;
export const hiddenAccountsQueryKey = ["hiddenAccounts"] as const;

/** Account groups in the order of the sidebar; refreshed by `accountsChanged`. */
export function useAccountGroups(enabled = true) {
  return useQuery({ queryKey: accountGroupsQueryKey, queryFn: searchIpc.listAccountGroups, enabled });
}

/** Accounts hidden from the unified views on this device. */
export function useHiddenAccounts(enabled = true) {
  return useQuery({ queryKey: hiddenAccountsQueryKey, queryFn: searchIpc.listHiddenAccounts, enabled });
}

/** Lists that mix accounts change with groups and hidden accounts. */
async function invalidateMixed(queryClient: QueryClient): Promise<void> {
  await Promise.all(
    [accountGroupsQueryKey, hiddenAccountsQueryKey, ["unified"], ["search"]].map((queryKey) =>
      queryClient.invalidateQueries({ queryKey }),
    ),
  );
}

export interface GroupInput {
  name: string;
  accountIds: AccountId[];
}

export function useSaveAccountGroup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ groupId, name, accountIds }: GroupInput & { groupId: AccountGroupId | null }) =>
      groupId === null
        ? searchIpc.createAccountGroup(name, accountIds)
        : searchIpc.updateAccountGroup(groupId, name, accountIds),
    onSuccess: () => invalidateMixed(queryClient),
  });
}

export function useDeleteAccountGroup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (groupId: AccountGroupId) => searchIpc.deleteAccountGroup(groupId),
    onSuccess: () => invalidateMixed(queryClient),
  });
}

export function useSetAccountHidden() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ accountId, hidden }: { accountId: AccountId; hidden: boolean }) =>
      searchIpc.setAccountHidden(accountId, hidden),
    onSuccess: () => invalidateMixed(queryClient),
  });
}
