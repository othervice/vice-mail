import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type InfiniteData, type QueryKey } from "@tanstack/react-query";
import { ArrowUpNarrowWide, ListFilter, MailCheck, MailOpen } from "lucide-react";
import { useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { create } from "zustand";
import { cx } from "../../components/cx";
import {
  comfortIpc,
  parityIpc,
  toCoreError,
  type InboxScope,
  type ListCursor,
  type MessageListItem,
  type MessagePage,
  type UnreadFirstCursor,
  type UnreadFirstPage,
} from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { OverflowMenu, type OverflowItem } from "../actions/OverflowMenu";
import type { MessageListSource } from "../messages/listSource";
import { buildRows, type DayBounds, type ListRow } from "../messages/rows";

/** Messages per page of the unread lists (the contract allows 1..500). */
export const UNREAD_PAGE_SIZE = 200;

/**
 * Stage 16: the key of a view for "Unread first" (a device setting of the core, `ui.unread_first`)
 * and for "Unread only" (this session). The contract suggests these forms.
 */
export function unreadViewKey(scope: InboxScope): string {
  if (scope.kind === "mailbox") return `mailbox:${String(scope.mailboxId)}`;
  return scope.view.kind === "all" ? "unified:all" : `group:${scope.view.groupId}`;
}

interface OnlyUnreadState {
  /** Views that show their unread mail only, for this session (a filter, not a setting). */
  views: ReadonlySet<string>;
  toggle: (view: string) => void;
}

export const useOnlyUnreadStore = create<OnlyUnreadState>()((set) => ({
  views: new Set<string>(),
  toggle: (view) => {
    set((state) => {
      const next = new Set(state.views);
      if (!next.delete(view)) next.add(view);
      return { views: next };
    });
  },
}));

export const unreadFirstViewsQueryKey = ["parity", "unreadFirstViews"] as const;

export function useUnreadFirstViews() {
  return useQuery({ queryKey: unreadFirstViewsQueryKey, queryFn: () => parityIpc.getUnreadFirstViews() });
}

/** Turns "Unread first" on or off for one view; the answer is every view with it on. */
export function useSetUnreadFirstView() {
  const queryClient = useQueryClient();
  const push = useNoticeStore((state) => state.push);
  return useMutation({
    mutationFn: ({ view, enabled }: { view: string; enabled: boolean }) => parityIpc.setUnreadFirstView(view, enabled),
    onSuccess: (views) => {
      queryClient.setQueryData(unreadFirstViewsQueryKey, views);
    },
    onError: (error) => {
      push({ kind: "error", context: "action", error: toCoreError(error) });
    },
  });
}

/** How a view orders or filters its unread mail right now. */
export type UnreadMode = "only" | "first" | null;

export function useUnreadMode(viewKey: string | null): UnreadMode {
  const only = useOnlyUnreadStore((state) => (viewKey ? state.views.has(viewKey) : false));
  const first = useUnreadFirstViews().data?.includes(viewKey ?? "") ?? false;
  if (!viewKey) return null;
  if (only) return "only";
  return first ? "first" : null;
}

/** Heading of the unread and the read group: drawn only; the rows' own names say "unread". */
function GroupHeading({ group }: { group: "unread" | "read" }) {
  const { t } = useTranslation();
  const Icon = group === "unread" ? MailCheck : MailOpen;
  return (
    <div aria-hidden data-testid={`unread-group-${group}`} className="flex h-full items-end gap-2 px-3 pb-1 text-12 font-semibold text-tertiary">
      <Icon className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
      <span className="truncate">{t(group === "unread" ? "organize.unread.unreadHeading" : "organize.unread.readHeading")}</span>
    </div>
  );
}

/** "Unread first": a heading over the unread rows, then one over the read rows (no date sections). */
export function unreadFirstRows(items: readonly MessageListItem[]): ListRow[] {
  const rows: ListRow[] = [];
  let position = 0;
  let group: "unread" | "read" | null = null;
  for (const item of items) {
    const next = item.unread ? "unread" : "read";
    if (next !== group) {
      group = next;
      const heading = next;
      rows.push({ kind: "extra", key: `unread-group-${heading}`, size: "section", render: () => <GroupHeading group={heading} /> });
    }
    rows.push({ kind: "message", key: `message-${String(item.id)}`, item, position: position++ });
  }
  return rows;
}

/**
 * Keeps the open message in "Unread only" once it was read (opening marks it read): it stays in
 * its place until the selection moves on, so the reading pane does not close under the reader.
 */
function withKept(
  data: InfiniteData<MessagePage, ListCursor | null>,
  kept: readonly MessageListItem[],
): InfiniteData<MessagePage, ListCursor | null> {
  const present = new Set(data.pages.flatMap((page) => page.items.map((item) => item.id)));
  const missing = kept.filter((item) => !present.has(item.id));
  if (missing.length === 0) return data;
  // Into the page whose date range holds the message (the last loaded one for older messages).
  const pages = data.pages.map((page) => ({ ...page, items: [...page.items] }));
  for (const item of missing) {
    const page = pages.find((candidate) => (candidate.items.at(-1)?.date ?? -Infinity) <= item.date) ?? pages.at(-1);
    if (!page) continue;
    page.items.push(item);
    page.items.sort((a, b) => b.date - a.date || b.id - a.id);
  }
  return { pages, pageParams: data.pageParams };
}

/**
 * The list of a view while "Unread only" or "Unread first" is on, else nothing (the view's own
 * list applies). Folder lists live below `["messages", account, folder]`, the unified ones below
 * `["unified", view]`, so the events that reload a view reload these too.
 */
export function useUnreadSource(
  scope: InboxScope | null,
  keyBase: QueryKey,
  mode: UnreadMode,
  kept: readonly MessageListItem[],
): MessageListSource | undefined {
  const { t } = useTranslation();
  const selectOnly = useCallback(
    (data: InfiniteData<MessagePage, ListCursor | null>) => withKept(data, kept),
    // The selection is state of the view: the same array until it changes.
    [kept],
  );

  const only = useInfiniteQuery({
    queryKey: [...keyBase, "unread"],
    queryFn: ({ pageParam }: { pageParam: ListCursor | null }) =>
      comfortIpc.listInboxFiltered(scope ?? { kind: "unified", view: { kind: "all" } }, { kind: "unread" }, pageParam, UNREAD_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (last: MessagePage) => last.nextCursor,
    select: selectOnly,
    enabled: scope !== null && mode === "only",
  });

  const first = useInfiniteQuery({
    queryKey: [...keyBase, "unreadFirst"],
    queryFn: ({ pageParam }: { pageParam: UnreadFirstCursor | null }) =>
      parityIpc.listUnreadFirst(scope ?? { kind: "unified", view: { kind: "all" } }, pageParam, UNREAD_PAGE_SIZE),
    initialPageParam: null as UnreadFirstCursor | null,
    getNextPageParam: (last: UnreadFirstPage) => last.nextCursor,
    enabled: scope !== null && mode === "first",
  });

  const dated = useCallback((items: readonly MessageListItem[], bounds: DayBounds) => buildRows(items, bounds), []);
  const grouped = useCallback((items: readonly MessageListItem[]) => unreadFirstRows(items), []);

  return useMemo((): MessageListSource | undefined => {
    if (mode === "only") {
      return {
        query: only,
        buildRows: dated,
        total: only.data?.pages[0]?.items.length ?? 0,
        empty: { icon: MailCheck, title: t("organize.unread.emptyTitle"), body: t("organize.unread.emptyBody") },
      };
    }
    if (mode === "first") return { query: first, buildRows: grouped };
    return undefined;
  }, [mode, only, first, dated, grouped, t]);
}

/**
 * In the header of a folder or inbox view: the "Unread only" switch and the options menu with
 * "Unread first". Both name the view they change by being in its header.
 */
export function UnreadControls({ viewKey }: { viewKey: string }) {
  const { t } = useTranslation();
  const only = useOnlyUnreadStore((state) => state.views.has(viewKey));
  const toggle = useOnlyUnreadStore((state) => state.toggle);
  const firstViews = useUnreadFirstViews();
  const setFirst = useSetUnreadFirstView();
  const first = firstViews.data?.includes(viewKey) ?? false;
  const items: OverflowItem[] = [
    {
      id: "unread-first",
      label: first ? t("organize.unread.firstOff") : t("organize.unread.firstOn"),
      icon: ArrowUpNarrowWide,
      ...(firstViews.isPending && { inert: true }),
      onSelect: () => {
        if (!firstViews.isPending) setFirst.mutate({ view: viewKey, enabled: !first });
      },
      "data-testid": "unread-first-toggle",
    },
  ];
  return (
    <div className="flex shrink-0 items-center gap-1" data-testid="unread-controls">
      <button
        type="button"
        role="switch"
        aria-checked={only}
        onClick={() => {
          toggle(viewKey);
        }}
        data-testid="only-unread"
        className={cx(
          "flex h-8 items-center gap-2 rounded-control px-2 text-13 transition-colors hover:bg-hover focus-visible:outline-2 focus-visible:outline-focus-ring",
          only ? "font-medium text-primary" : "text-secondary",
        )}
      >
        <ListFilter aria-hidden className="size-(--ui-icon-small) shrink-0" strokeWidth={1.75} />
        <span>{t("organize.unread.only")}</span>
        <span
          aria-hidden
          className={cx(
            "relative flex h-4 w-8 shrink-0 items-center rounded-full border p-px transition-colors",
            only ? "justify-end border-accent bg-accent" : "justify-start border-border-strong bg-track",
          )}
        >
          <span className={cx("size-3 rounded-full", only ? "bg-surface-raised" : "bg-secondary")} />
        </span>
      </button>
      <OverflowMenu
        label={t("organize.unread.menu")}
        data-testid="list-options"
        items={items}
      />
    </div>
  );
}
