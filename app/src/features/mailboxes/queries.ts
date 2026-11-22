import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc, type AccountId, type Mailbox, type MailboxId, type MailboxRole } from "../../services/ipc";

export const mailboxesQueryKey = (accountId: AccountId) => ["mailboxes", accountId] as const;

/** Folders of one account with their counters; refreshed by `mailboxesChanged`. */
export function useMailboxes(accountId: AccountId | null | undefined) {
  return useQuery({
    queryKey: mailboxesQueryKey(accountId ?? ""),
    queryFn: () => ipc.listMailboxes(accountId ?? ""),
    enabled: Boolean(accountId),
  });
}

/**
 * Gives a folder a role chosen by the user. The core answers with all folders of the account,
 * because another folder may lose the role at the same time.
 */
export function useSetMailboxRole(accountId: AccountId) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ mailboxId, role }: { mailboxId: MailboxId; role: MailboxRole }) =>
      ipc.setMailboxRole(mailboxId, role),
    onSuccess: (mailboxes: Mailbox[]) => {
      queryClient.setQueryData(mailboxesQueryKey(accountId), mailboxes);
    },
  });
}
