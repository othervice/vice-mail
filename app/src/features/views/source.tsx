import { keepPreviousData, useInfiniteQuery, useQuery, type InfiniteData, type QueryKey } from "@tanstack/react-query";
import { ListFilter } from "lucide-react";
import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import {
  viewIpc,
  type InboxScope,
  type ListOrder,
  type ListViewRequest,
  type MessageId,
  type MessageListItem,
  type QuickFilter,
  type ViewCursor,
  type ViewMessagePage,
  type ViewThreadPage,
} from "../../services/ipc";
import type { MessageListSource } from "../messages/listSource";
import { buildRows, buildRowsWithoutSections, type DayBounds } from "../messages/rows";
import { mergeInfo, threadRows, type ConversationInfoById } from "../threads/conversationRows";
import { decorateConversations } from "../threads/source";

/** Rows per page of a sorted or filtered view (the contract allows 1..500). */
export const VIEW_PAGE_SIZE = 200;

/** A page as the list takes it: its rows, conversation info by row id, the next cursor. */
interface ViewPage {
  items: MessageListItem[];
  info?: ConversationInfoById;
  nextCursor: ViewCursor | null;
}

/**
 * Keeps the open message where it was once it no longer matches (opening it marks it read and
 * "Unread" is on), until the selection moves on, so the reading pane does not close under the
 * reader (stage 16 did the same for "Unread only"). It goes back to the place it had among the
 * rows shown before: the order is not by date, so its date does not say where.
 */
function withKept(
  data: InfiniteData<ViewPage, ViewCursor | null>,
  kept: readonly MessageListItem[],
  places: Map<MessageId, number>,
): InfiniteData<ViewPage, ViewCursor | null> {
  const present = new Set(data.pages.flatMap((page) => page.items.map((item) => item.id)));
  const missing = kept.filter((item) => !present.has(item.id) && places.has(item.id));
  if (missing.length === 0) return data;
  const pages = data.pages.map((page) => ({ ...page, items: [...page.items] }));
  for (const item of missing) {
    let at = places.get(item.id) ?? 0;
    const page = pages.find((candidate) => {
      if (at <= candidate.items.length) return true;
      at -= candidate.items.length;
      return false;
    });
    page?.items.splice(at, 0, item);
  }
  return { pages, pageParams: data.pageParams };
}

interface ViewSourceOptions {
  /** The view; null: no source. */
  scope: InboxScope | null;
  /** The view's lists' key (`["messages", account, mailbox]` or `["unified", view]`), so the events that reload it reload this too. */
  keyBase: QueryKey;
  order: ListOrder;
  filter: QuickFilter;
  conversations: boolean;
  /** The selection: kept in place while the unread filter would drop it. */
  kept: readonly MessageListItem[];
  /** The view is sorted or filtered: otherwise its own list applies. */
  enabled: boolean;
}

export function viewRequest(scope: InboxScope, order: ListOrder, filter: QuickFilter): ListViewRequest {
  return { scope, order, filter };
}

/**
 * Stage 19 (decisions 0061, 0062): the rows of a view in another order or with the quick filter,
 * one per message or per conversation. Date order keeps the date sections; the other orders
 * have none (every row names its date). Undefined while the view's own list applies.
 */
export function useViewSource({ scope, keyBase, order, filter, conversations, kept, enabled }: ViewSourceOptions): MessageListSource | undefined {
  const { t } = useTranslation();
  const active = enabled && scope !== null;
  const request = scope && viewRequest(scope, order, filter);
  const places = useRef(new Map<MessageId, number>());
  const keep = filter.unread;
  const select = useCallback(
    (data: InfiniteData<ViewPage, ViewCursor | null>) => {
      const shown = keep ? withKept(data, kept, places.current) : data;
      places.current = new Map(shown.pages.flatMap((page) => page.items).map((item, index) => [item.id, index] as const));
      return shown;
    },
    // The selection is state of the view: the same array until it changes.
    [kept, keep],
  );
  const query = useInfiniteQuery({
    queryKey: [...keyBase, "view", order, filter, conversations ? "conversations" : "messages"],
    queryFn: async ({ pageParam }: { pageParam: ViewCursor | null }): Promise<ViewPage> => {
      const current = request ?? viewRequest({ kind: "unified", view: { kind: "all" } }, order, filter);
      if (conversations) {
        const page: ViewThreadPage = await viewIpc.listConversations(current, pageParam, VIEW_PAGE_SIZE);
        return { ...threadRows(page.items), nextCursor: page.nextCursor };
      }
      const page: ViewMessagePage = await viewIpc.listMessages(current, pageParam, VIEW_PAGE_SIZE);
      return { items: page.items, nextCursor: page.nextCursor };
    },
    initialPageParam: null as ViewCursor | null,
    getNextPageParam: (last: ViewPage) => last.nextCursor,
    select,
    enabled: active,
    // Another order or filter of the same view: the rows shown stay until the new ones are
    // there (no skeleton between two keystrokes).
    placeholderData: keepPreviousData,
  });
  const info = useMemo(() => mergeInfo(query.data?.pages), [query.data]);
  const dated = order.key === "date";
  const build = useCallback(
    (items: readonly MessageListItem[], bounds: DayBounds) => (dated ? buildRows(items, bounds) : buildRowsWithoutSections(items)),
    [dated],
  );
  const filtering = filter.unread || filter.flagged || filter.attachment || filter.contacts || filter.tag !== null || filter.text !== "";
  return useMemo((): MessageListSource | undefined => {
    if (!active) return undefined;
    return {
      query,
      buildRows: build,
      // A filtered view has no folder total: `aria-setsize` counts what is loaded until the end.
      ...(filtering && { total: query.data?.pages.reduce((sum, page) => sum + page.items.length, 0) ?? 0 }),
      ...(filtering && { empty: { icon: ListFilter, title: t("views.filter.emptyTitle"), body: t("views.filter.emptyBody") } }),
      ...(conversations && { decorate: decorateConversations(info), conversations: true }),
    };
  }, [active, query, build, filtering, conversations, info, t]);
}

/** How many messages of the view match its quick filter; off while nothing is set. */
export function useViewCount(scope: InboxScope | null, keyBase: QueryKey, filter: QuickFilter, enabled: boolean) {
  return useQuery({
    queryKey: [...keyBase, "viewCount", filter],
    queryFn: () => viewIpc.count(viewRequest(scope ?? { kind: "unified", view: { kind: "all" } }, { key: "date", ascending: false }, filter)),
    enabled: enabled && scope !== null,
    placeholderData: keepPreviousData,
  });
}

/** The tags of the view's messages, for the tag menu (asked while it is open). */
export function useViewTags(scope: InboxScope | null, keyBase: QueryKey, enabled: boolean) {
  return useQuery({
    queryKey: [...keyBase, "viewTags"],
    queryFn: () => viewIpc.tags(scope ?? { kind: "unified", view: { kind: "all" } }),
    enabled: enabled && scope !== null,
  });
}
