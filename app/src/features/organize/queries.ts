import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { parityIpc, toCoreError, type AccountColor, type AccountId, type Mailbox, type MailboxId } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { mailboxesQueryKey } from "../mailboxes/queries";

/**
 * Stage 16: favourite folders of every account, in their order (sidebar top, palette). Reloaded
 * with every `mailboxesChanged` (the core sends it for favourites and colours too, also after a
 * settings sync pass from another device).
 */
export const favoriteMailboxesQueryKey = ["favoriteMailboxes"] as const;

export function useFavoriteMailboxes(enabled = true) {
  return useQuery({ queryKey: favoriteMailboxesQueryKey, queryFn: () => parityIpc.listFavoriteMailboxes(), enabled });
}

export const isFavorite = (mailbox: Mailbox) => mailbox.favoritePosition !== undefined && mailbox.favoritePosition !== null;

function refreshFolders(queryClient: QueryClient, accountId: AccountId) {
  void queryClient.invalidateQueries({ queryKey: mailboxesQueryKey(accountId) });
  void queryClient.invalidateQueries({ queryKey: favoriteMailboxesQueryKey });
}

export type FolderChange =
  | { kind: "favorite"; mailbox: Mailbox; favorite: boolean }
  | { kind: "color"; mailbox: Mailbox; color: AccountColor | null }
  | { kind: "reorder"; mailbox: Mailbox; order: MailboxId[] };

/** Favourite on or off, a colour, or a new order of the favourites. Errors show as a notice. */
export function useChangeFolder() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  return useMutation({
    mutationFn: async (change: FolderChange): Promise<void> => {
      switch (change.kind) {
        case "favorite":
          await parityIpc.setMailboxFavorite(change.mailbox.id, change.favorite);
          return;
        case "color":
          await parityIpc.setMailboxColor(change.mailbox.id, change.color);
          return;
        case "reorder": {
          const favorites = await parityIpc.reorderFavoriteMailboxes(change.order);
          queryClient.setQueryData(favoriteMailboxesQueryKey, favorites);
          return;
        }
      }
    },
    onError: (error) => {
      push({ kind: "error", context: "folder", error: toCoreError(error) });
    },
    onSettled: (_data, _error, change) => {
      refreshFolders(queryClient, change.mailbox.accountId);
    },
  });
}
