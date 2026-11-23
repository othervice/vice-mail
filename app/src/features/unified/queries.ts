import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import {
  searchIpc,
  type AccountGroupId,
  type AccountId,
  type ListCursor,
  type UnifiedView,
} from "../../services/ipc";
import { MESSAGE_PAGE_SIZE } from "../messages/queries";

export const unifiedQueryKey = (view: UnifiedView) => ["unified", view] as const;
export const accountGroupsQueryKey = ["accountGroups"] as const;
export const hiddenAccountsQueryKey = ["hiddenAccounts"] as const;

/** The unified inbox (or a group's inboxes), mixed by date; every row keeps its account. */
export function useUnifiedPages(view: UnifiedView, enabled: boolean) {
  return useInfiniteQuery({
    queryKey: unifiedQueryKey(view),
    queryFn: ({ pageParam }) => searchIpc.listUnifiedInbox(view, pageParam, MESSAGE_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled,
  });
}

/** Account groups in the order of the sidebar; refreshed by `accountsChanged`. */
export function useAccountGroups(enabled = true) {
  return useQuery({ queryKey: accountGroupsQueryKey, queryFn: searchIpc.listAccountGroups, enabled });
}

/** Accounts hidden from the unified views on this device. */
export function useHiddenAccounts() {
  return useQuery({ queryKey: hiddenAccountsQueryKey, queryFn: searchIpc.listHiddenAccounts });
}

/** Lists that mix accounts change with groups and hidden accounts. */
async function invalidateMixed(queryClient: QueryClient): Promise<void> {
  await Promise.all(
    [accountGroupsQueryKey, hiddenAccountsQueryKey, ["unified"], ["unifiedThreads"], ["search"]].map((queryKey) =>
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
