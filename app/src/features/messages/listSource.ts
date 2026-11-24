import type { InfiniteData, UseInfiniteQueryResult } from "@tanstack/react-query";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import type { AccountColor, MessageListItem } from "../../services/ipc";
import type { RowDecoration } from "./MessageRow";
import type { DayBounds, ListRow } from "./rows";

/** What the list needs of a page: its messages in list order. */
export interface ListPage {
  items: MessageListItem[];
}

export type ListQuery = UseInfiniteQueryResult<InfiniteData<ListPage, unknown>>;

/**
 * Stage 9: rows that are not one folder's pages (the sorted inbox with bundles and blocks, Later,
 * Pinned, Tasks). The list keeps its selection, keys, virtualization and quick actions and only
 * takes the rows from here.
 */
export interface MessageListSource {
  query: ListQuery;
  /** Rows for the messages that are not hidden, in list order. */
  buildRows: (items: readonly MessageListItem[], bounds: DayBounds) => ListRow[];
  /** For `aria-setsize`; default: the folder's total. */
  total?: number;
  /** What an empty list says; default: the folder's empty state. */
  empty?: { icon: LucideIcon; title: string; body: string };
  /** Stage 9: controls above the list that stay when it is empty (the filter of Tasks). */
  controls?: ReactNode;
  /** Stage 7: the account colour of a row, for lists that mix accounts (unified inbox, search). */
  accountColor?: (item: MessageListItem) => AccountColor;
  /**
   * Stage 8: the account's name next to its colour, in lists that mix accounts (the colour is
   * never the only sign of the account).
   */
  accountName?: (item: MessageListItem) => string | undefined;
  /** Stage 7: what a row adds, such as its conversation size or the search highlights. */
  decorate?: (item: MessageListItem) => RowDecoration | undefined;
  /**
   * Stage 17 (decision 0049): the rows are conversations. The list is a tree then: a row with
   * more than one message (`decorate` names its size and thread) unfolds its messages below it.
   */
  conversations?: boolean;
}
