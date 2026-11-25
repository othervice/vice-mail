import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toCoreError, viewIpc, type ListOrder, type SortKey, type ViewOrder } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";

/**
 * Stage 19 (decision 0061): the order of a list view. Every view (its key as for "Unread first":
 * `mailbox:<id>`, `unified:all`, `group:<id>`) is newest first until the user chooses another
 * order; the core keeps the others as a device setting.
 */
export const DEFAULT_ORDER: ListOrder = { key: "date", ascending: false };

export const listOrdersQueryKey = ["views", "orders"] as const;

export function isDefaultOrder(order: ListOrder): boolean {
  return order.key === "date" && !order.ascending;
}

/** The direction a key starts with: newest and largest first, names and subjects A to Z. */
export function defaultAscending(key: SortKey): boolean {
  return key === "sender" || key === "recipient" || key === "subject";
}

/**
 * The keys a view offers: the recipient only in Sent and Drafts (where rows name the
 * recipients), or where it is the order already (chosen in the table).
 */
export function sortKeysFor(showRecipients: boolean, current: SortKey): SortKey[] {
  const keys: SortKey[] = ["date", "sender", "subject", "size"];
  if (showRecipients || current === "recipient") keys.splice(2, 0, "recipient");
  return keys;
}

export function useListOrders() {
  return useQuery({ queryKey: listOrdersQueryKey, queryFn: () => viewIpc.getOrders() });
}

/** The order of `viewKey` (the default for no view, or while the setting loads). */
export function useListOrder(viewKey: string | null): ListOrder {
  const orders = useListOrders().data;
  if (viewKey === null) return DEFAULT_ORDER;
  return orders?.find((entry) => entry.view === viewKey)?.order ?? DEFAULT_ORDER;
}

/** Changes a view's order: shown at once, stored by the core (rolled back if it refuses). */
export function useSetListOrder() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  return useMutation({
    mutationFn: ({ view, order }: { view: string; order: ListOrder }) => viewIpc.setOrder(view, isDefaultOrder(order) ? null : order),
    onMutate: async ({ view, order }) => {
      await queryClient.cancelQueries({ queryKey: listOrdersQueryKey });
      const before = queryClient.getQueryData<ViewOrder[]>(listOrdersQueryKey);
      const rest = (before ?? []).filter((entry) => entry.view !== view);
      queryClient.setQueryData<ViewOrder[]>(listOrdersQueryKey, isDefaultOrder(order) ? rest : [...rest, { view, order }]);
      return { before };
    },
    onSuccess: (orders) => {
      queryClient.setQueryData(listOrdersQueryKey, orders);
    },
    onError: (error, _vars, context) => {
      queryClient.setQueryData(listOrdersQueryKey, context?.before);
      push({ kind: "error", context: "action", error: toCoreError(error) });
    },
  });
}

/**
 * What choosing `key` does to `order`: the same key turns the direction around (a click on the
 * sorted column's header), another key starts in its own direction.
 */
export function orderFor(order: ListOrder, key: SortKey, toggle: boolean): ListOrder {
  if (order.key === key) return toggle ? { key, ascending: !order.ascending } : order;
  return { key, ascending: defaultAscending(key) };
}
