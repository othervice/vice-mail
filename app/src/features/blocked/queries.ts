import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { parityIpc, type BlockedSender } from "../../services/ipc";

/** Stage 16: blocked senders (synced between devices; `inboxChanged` follows every change). */
export const blockedSendersQueryKey = ["parity", "blockedSenders"] as const;

export function useBlockedSenders() {
  return useQuery({ queryKey: blockedSendersQueryKey, queryFn: () => parityIpc.listBlockedSenders() });
}

/** Blocks (`blocked: true`) or unblocks one bare address; the answer is the whole list. */
export function useSetSenderBlocked() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ address, blocked }: { address: string; blocked: boolean }): Promise<BlockedSender[]> =>
      blocked ? parityIpc.blockSender(address) : parityIpc.unblockSender(address),
    onSuccess: (list) => {
      queryClient.setQueryData(blockedSendersQueryKey, list);
    },
  });
}
