import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { quotaIpc, type AccountId, type MailboxQuota } from "../../services/ipc";

/** The core keeps an answer this long; the UI asks no more often. */
const QUOTA_STALE_MS = 15 * 60 * 1000;

export const accountQuotaQueryKey = (accountId: AccountId) => ["accountQuota", accountId] as const;

/**
 * Stage 20 (decision 0060): how full the account's mailbox on the server is. Asked over the
 * account's read-only reading connection, at most every 15 minutes.
 */
export function useAccountQuota(accountId: AccountId) {
  return useQuery({
    queryKey: accountQuotaQueryKey(accountId),
    queryFn: () => quotaIpc.get(accountId, false),
    staleTime: QUOTA_STALE_MS,
  });
}

/** "Check again": asks the server now. */
export function useRefreshQuota(accountId: AccountId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => quotaIpc.get(accountId, true),
    onSuccess: (quota: MailboxQuota) => {
      queryClient.setQueryData(accountQuotaQueryKey(accountId), quota);
    },
  });
}

/** The share in use when the mailbox is nearly full (the account then shows a notice), else null. */
export function nearlyFullPercent(quota: MailboxQuota | undefined): number | null {
  return quota?.kind === "reported" && quota.nearlyFull ? quota.percent : null;
}
