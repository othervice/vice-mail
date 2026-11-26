import { useQueryClient } from "@tanstack/react-query";
import { ipc, toCoreError, type Account, type AccountId, type CoreError } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { accountsQueryKey } from "./queries";

/** The ids in the order after moving `id` one place up (`-1`) or down (`1`); null at the edge. */
export function movedOrder(accounts: readonly Account[], id: AccountId, step: -1 | 1): AccountId[] | null {
  const ids = accounts.map((account) => account.id);
  const index = ids.indexOf(id);
  const target = index + step;
  if (index < 0 || target < 0 || target >= ids.length) return null;
  ids.splice(index, 1);
  ids.splice(target, 0, id);
  return ids;
}

/**
 * The ids in the order after dropping `dragged` before (`after` false) or after the account `onto`;
 * null when nothing changes.
 */
export function droppedOrder(accounts: readonly Account[], dragged: AccountId, onto: AccountId, after: boolean): AccountId[] | null {
  if (dragged === onto) return null;
  const before = accounts.map((account) => account.id);
  if (!before.includes(dragged) || !before.includes(onto)) return null;
  const ids = before.filter((id) => id !== dragged);
  const at = ids.indexOf(onto) + (after ? 1 : 0);
  ids.splice(at, 0, dragged);
  return ids.every((id, i) => id === before[i]) ? null : ids;
}

/**
 * Stage 22 (decision 0078): the user's order and list names of the accounts. Every list of
 * accounts follows the order the core keeps (`listAccounts`); the change shows at once and is
 * synchronised with the settings sync.
 */
export function useAccountArrange() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);

  /** Puts the accounts in this order (at once in the cached list, then as the core answers). */
  const reorder = async (ids: readonly AccountId[]): Promise<boolean> => {
    const previous = queryClient.getQueryData<Account[]>(accountsQueryKey);
    if (previous) {
      const byId = new Map(previous.map((account) => [account.id, account] as const));
      const placed = ids.flatMap((id) => byId.get(id) ?? []);
      queryClient.setQueryData<Account[]>(accountsQueryKey, [...placed, ...previous.filter((account) => !ids.includes(account.id))]);
    }
    try {
      const accounts = await ipc.reorderAccounts([...ids]);
      // Keep the live statuses the list has (the answer carries the core's, which are the same).
      queryClient.setQueryData<Account[]>(accountsQueryKey, accounts);
      return true;
    } catch (e) {
      void queryClient.invalidateQueries({ queryKey: accountsQueryKey });
      push({ kind: "error", context: "accounts", error: toCoreError(e) });
      return false;
    }
  };

  /** One place up or down; false at the edge or on failure. */
  const move = async (id: AccountId, step: -1 | 1): Promise<boolean> => {
    const order = movedOrder(queryClient.getQueryData<Account[]>(accountsQueryKey) ?? [], id, step);
    return order ? reorder(order) : false;
  };

  /** The account's name in the lists; empty shows the address. Answers the error to show. */
  const rename = async (id: AccountId, name: string): Promise<CoreError | null> => {
    try {
      const account = await ipc.renameAccount(id, name);
      queryClient.setQueryData<Account[]>(accountsQueryKey, (accounts) => accounts?.map((a) => (a.id === id ? { ...account, status: a.status } : a)));
      return null;
    } catch (e) {
      return toCoreError(e);
    }
  };

  return { reorder, move, rename };
}
