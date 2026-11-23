import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { searchIpc, type AccountId, type IdentityId, type IdentityInfo, type IdentityInput } from "../../services/ipc";

export const identitiesQueryKey = (accountId: AccountId) => ["identities", accountId] as const;

/** Identities of one account, the default first; refreshed by `accountsChanged`. */
export function useIdentities(accountId: AccountId) {
  return useQuery({ queryKey: identitiesQueryKey(accountId), queryFn: () => searchIpc.listIdentities(accountId) });
}

/** Adds (`identityId` null) or changes an identity. */
export function useSaveIdentity(accountId: AccountId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ identityId, identity }: { identityId: IdentityId | null; identity: IdentityInput }) =>
      identityId === null ? searchIpc.addIdentity(accountId, identity) : searchIpc.updateIdentity(identityId, identity),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: identitiesQueryKey(accountId) }),
  });
}

export function useSetDefaultIdentity(accountId: AccountId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (identityId: IdentityId) => searchIpc.setDefaultIdentity(identityId),
    onSuccess: (identities: IdentityInfo[]) => {
      queryClient.setQueryData(identitiesQueryKey(accountId), identities);
    },
  });
}

export function useDeleteIdentity(accountId: AccountId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (identityId: IdentityId) => searchIpc.deleteIdentity(identityId),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: identitiesQueryKey(accountId) }),
  });
}
