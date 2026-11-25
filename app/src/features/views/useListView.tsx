import type { QueryKey } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { InboxScope, MessageListItem } from "../../services/ipc";
import type { MessageListSource } from "../messages/listSource";
import { monthFirstFor } from "../search/terms";
import { useDebounced } from "../search/useDebounced";
import { isDefaultOrder, useListOrder, useSetListOrder } from "./order";
import { QuickFilterBar } from "./QuickFilterBar";
import { beyondUnread, toQuickFilter, useQuickFilter } from "./quickFilter";
import { useViewSource } from "./source";
import { useListTableConfig, type ListTableConfig } from "./TableHeader";

/** Pause after the last key before the list follows the text, like the search's (150 ms). */
const FILTER_TYPING_DELAY_MS = 150;

/** The view key of a scope, as for "Unread first" (stage 16). */
export function viewKeyOf(scope: InboxScope): string {
  if (scope.kind === "mailbox") return `mailbox:${String(scope.mailboxId)}`;
  return scope.view.kind === "all" ? "unified:all" : `group:${scope.view.groupId}`;
}

interface ListViewOptions {
  /** The view; null where sorting and the quick filter do not apply (Later, Pinned, Tasks). */
  scope: InboxScope | null;
  /** The view's lists' key (`["messages", account, mailbox]` or `["unified", view]`). */
  keyBase: QueryKey;
  conversations: boolean;
  /** The list's selection: kept in place while "Unread" would drop the open message. */
  kept: readonly MessageListItem[];
  showRecipients: boolean;
  mixedAccounts: boolean;
}

export interface ListView {
  /** Rows in the view's order and with its quick filter; undefined: the view's own list applies. */
  source: MessageListSource | undefined;
  /** The view is sorted or filtered beyond "Unread": "Unread only" and "Unread first" step aside. */
  active: boolean;
  /** The table while the wide list is drawn as one. */
  table: ListTableConfig | undefined;
  /** The quick filter bar above the list (null where it does not apply). */
  bar: ReactNode;
}

/**
 * Stage 19 (decisions 0061, 0062): what a folder, an inbox or a unified view adds to its list:
 * the order chosen for it, the quick filter, the table. Newest first with nothing filtered (or
 * only "Unread", stage 16's own list) leaves the view's list as it was: sections, bundles, the
 * priority block and "Unread first" stay theirs.
 */
export function useListView({ scope, keyBase, conversations, kept, showRecipients, mixedAccounts }: ListViewOptions): ListView {
  const viewKey = scope && viewKeyOf(scope);
  const order = useListOrder(viewKey);
  const setOrder = useSetListOrder();
  const filter = useQuickFilter(viewKey);
  const text = useDebounced(filter.text, FILTER_TYPING_DELAY_MS);
  const [monthFirst] = useState(() => monthFirstFor(navigator.language));
  const query = toQuickFilter(filter, text, monthFirst);
  // The text counts once its pause is over: no list in between while the first letters come.
  const active = scope !== null && (!isDefaultOrder(order) || beyondUnread({ ...filter, text }));
  const source = useViewSource({ scope, keyBase, order, filter: query, conversations, kept, enabled: active });
  const table = useListTableConfig({
    order,
    setOrder: (next) => {
      if (viewKey) setOrder.mutate({ view: viewKey, order: next });
    },
    showRecipients,
    mixedAccounts,
  });
  const bar = scope && viewKey ? <QuickFilterBar viewKey={viewKey} scope={scope} keyBase={keyBase} query={query} filter={filter} /> : null;
  return { source, active, table: scope ? table : undefined, bar };
}
