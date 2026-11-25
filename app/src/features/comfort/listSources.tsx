import { useInfiniteQuery, type InfiniteData, type QueryKey } from "@tanstack/react-query";
import { BookOpen, Clock, Flag, Inbox, ListTodo, MailClock, Pin, Zap } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import {
  comfortIpc,
  conversationIpc,
  parityIpc,
  reminderIpc,
  type Account,
  type InboxCursor,
  type InboxPage,
  type InboxScope,
  type ListCursor,
  type Mailbox,
  type MarkScope,
  type MessageListItem,
  type MessagePage,
  type TaskFilter,
  type ThreadPage,
  type UnifiedView,
} from "../../services/ipc";
import {
  conversationInboxPage,
  conversationKey,
  mergeInfo,
  threadRows,
  type ConversationInfoById,
} from "../threads/conversationRows";
import { decorateConversations } from "../threads/source";
import { useConversationsOn } from "../threads/preference";
import type { MessageListSource } from "../messages/listSource";
import { buildRows, type DayBounds, type ListRow } from "../messages/rows";
import { buildInboxRows, inboxPageItems, type InboxListRow, type InboxViewPage } from "./inboxRows";
import { BlockHeading, BundleRow, ShowAllRow } from "./InboxListRows";
import { inboxQueryKey, markListQueryKey, priorityQueryKey, type MarkListKind } from "./queries";

/** Messages per `listInbox` page; the contract allows 1..500 and counts messages, not rows. */
export const INBOX_PAGE_SIZE = 200;

/** What a list shows (stage 9): a folder's pages, the sorted inbox, or one of the mark views. */
export type ListKind = "folder" | "inbox" | MarkListKind;

interface ListRowsOptions {
  expanded: ReadonlySet<string>;
  toggle: (key: string) => void;
  showAll: () => void;
}

/** The inbox rows as list rows: headings and bundles are drawn rows, messages are options. */
export function toListRows(rows: readonly InboxListRow[], options: ListRowsOptions): ListRow[] {
  let position = 0;
  return rows.map((row): ListRow => {
    switch (row.kind) {
      case "section":
        return row;
      case "message":
        return {
          kind: "message",
          key: row.key,
          item: row.item,
          position: position++,
          ...(row.block === "priority" && { card: "priority" as const }),
          // Stage 17: in a conversation tree, members of an unfolded bundle are one level deeper.
          ...(row.bundle !== null && { card: "bundle" as const, level: 2 }),
        };
      case "block":
        return {
          kind: "extra",
          key: row.key,
          size: "section",
          render: () => <BlockHeading block={row.block} />,
          ...(row.block === "priority" && { card: "priority" as const }),
        };
      case "showAll":
        return {
          kind: "extra",
          key: row.key,
          size: "section",
          card: "priority",
          render: () => <ShowAllRow total={row.total} unread={row.unread} />,
          action: { activate: options.showAll },
        };
      case "bundle":
        return {
          kind: "extra",
          key: row.key,
          size: "row",
          render: () => <BundleRow bundle={row.bundle} expanded={row.expanded} />,
          expanded: row.expanded,
          ...(row.expanded && { card: "bundle" as const }),
          action: {
            activate: () => {
              options.toggle(row.bundle.key);
            },
          },
        };
    }
  });
}

/** The inbox pages as the list reads them (stable function: TanStack keeps the result stable too). */
function selectInboxPages(data: InfiniteData<InboxPage, InboxCursor | null>) {
  return {
    pages: data.pages.map((page): InboxViewPage & { info?: ConversationInfoById } => ({ items: inboxPageItems(page), page })),
    pageParams: data.pageParams,
  };
}

/** Decision 0039: the same for conversation pages; every message stands for its conversation. */
function selectConversationPages(data: InfiniteData<InboxPage, InboxCursor | null>) {
  return {
    pages: data.pages.map((raw): InboxViewPage & { info?: ConversationInfoById } => {
      const { page, info } = conversationInboxPage(raw);
      return { items: inboxPageItems(page), page, info };
    }),
    pageParams: data.pageParams,
  };
}

/** Decision 0039: a page of conversations as message rows. */
function selectThreadPages(data: InfiniteData<ThreadPage, ListCursor | null>) {
  return {
    pages: data.pages.map((page) => {
      const { items, info } = threadRows(page.items);
      return { items, info, nextCursor: page.nextCursor };
    }),
    pageParams: data.pageParams,
  };
}



function mailboxScope(mailbox: Mailbox): InboxScope {
  return { kind: "mailbox", mailboxId: mailbox.id };
}

/**
 * The rows of a list by kind. `folder` gives none (the list reads the folder's pages itself).
 * Every query runs only for its own kind, so a folder view asks the core nothing extra.
 */
/**
 * The sorted inbox of a scope (one account's inbox, or the unified inbox of all visible accounts
 * or a group): blocks and bundles, and the "Priority" list behind "Show all".
 */
function useSortedInboxSource(
  scope: InboxScope,
  keys: { inbox: QueryKey; priority: QueryKey },
  enabled: boolean,
): MessageListSource {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [priorityOnly, setPriorityOnly] = useState(false);
  // Decision 0039: one row per conversation (a UI preference, on by default).
  const conversations = useConversationsOn();

  const inbox = useInfiniteQuery({
    queryKey: conversations ? [...keys.inbox, "conversations"] : keys.inbox,
    queryFn: ({ pageParam }: { pageParam: InboxCursor | null }) =>
      conversations
        ? conversationIpc.listInbox(scope, pageParam, INBOX_PAGE_SIZE)
        : comfortIpc.listInbox(scope, pageParam, INBOX_PAGE_SIZE),
    initialPageParam: null as InboxCursor | null,
    getNextPageParam: (last: InboxPage) => last.nextCursor,
    select: conversations ? selectConversationPages : selectInboxPages,
    enabled: enabled && !priorityOnly,
  });

  const priority = useInfiniteQuery({
    queryKey: keys.priority,
    queryFn: ({ pageParam }: { pageParam: ListCursor | null }) =>
      comfortIpc.listInboxFiltered(scope, { kind: "priority" }, pageParam, INBOX_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (last: MessagePage) => last.nextCursor,
    enabled: enabled && priorityOnly && !conversations,
  });
  const priorityConversations = useInfiniteQuery({
    queryKey: [...keys.priority, "conversations"],
    queryFn: ({ pageParam }: { pageParam: ListCursor | null }) =>
      conversationIpc.listInboxFiltered(scope, { kind: "priority" }, pageParam, INBOX_PAGE_SIZE),
    initialPageParam: null as ListCursor | null,
    getNextPageParam: (last: ThreadPage) => last.nextCursor,
    select: selectThreadPages,
    enabled: enabled && priorityOnly && conversations,
  });

  const toggle = useCallback((key: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }, []);
  const showAll = useCallback(() => {
    setPriorityOnly(true);
  }, []);
  const back = useCallback(() => {
    setPriorityOnly(false);
  }, []);

  const inboxPages = inbox.data?.pages;
  const inboxInfo = useMemo(() => (conversations ? mergeInfo(inboxPages) : null), [conversations, inboxPages]);
  const priorityInfo = useMemo(() => mergeInfo(priorityConversations.data?.pages), [priorityConversations.data]);
  const inboxRows = useCallback(
    (items: readonly MessageListItem[]) => {
      const visible = new Set(items.map((item) => item.id));
      const rows = buildInboxRows(
        inboxPages?.map((entry) => entry.page) ?? [],
        expanded,
        visible,
        inboxInfo ? conversationKey(inboxInfo) : undefined,
      );
      return toListRows(rows, { expanded, toggle, showAll });
    },
    [inboxPages, inboxInfo, expanded, toggle, showAll],
  );
  const priorityRows = useCallback(
    (items: readonly MessageListItem[], bounds: DayBounds): ListRow[] => [
      {
        kind: "extra",
        key: "priority-back",
        size: "section",
        render: () => <PriorityBack />,
        action: { activate: back },
      },
      ...buildRows(items, bounds),
    ],
    [back],
  );

  return useMemo((): MessageListSource => {
    if (priorityOnly) {
      return {
        query: conversations ? priorityConversations : priority,
        buildRows: priorityRows,
        empty: { icon: Zap, title: t("comfort.priority.empty.title"), body: t("comfort.priority.empty.body") },
        ...(conversations && { decorate: decorateConversations(priorityInfo), conversations: true }),
      };
    }
    return {
      query: inbox,
      buildRows: inboxRows,
      ...(inboxInfo && { decorate: decorateConversations(inboxInfo), conversations: true }),
    };
  }, [priorityOnly, conversations, priorityConversations, priority, priorityRows, priorityInfo, inbox, inboxRows, inboxInfo, t]);
}

/**
 * Stage 7: the unified inbox (every visible account, or a group's) as a sorted inbox, through
 * `listInbox` with the unified scope, so it keeps blocks, bundles and hidden snoozes. Keyed under
 * `["unified", view]`, which `messagesChanged` and `inboxChanged` reload.
 */
export function useUnifiedInboxSource(view: UnifiedView, enabled: boolean): MessageListSource {
  return useSortedInboxSource(
    { kind: "unified", view },
    { inbox: ["unified", view, "inbox"], priority: ["unified", view, "priority"] },
    enabled,
  );
}

export function useListSource(kind: ListKind, account: Account, mailbox: Mailbox): MessageListSource | undefined {
  const { t } = useTranslation();
  const [taskFilter, setTaskFilter] = useState<TaskFilter>("open");
  const markScope: MarkScope = { kind: "account", accountId: account.id };
  const sorted = useSortedInboxSource(
    mailboxScope(mailbox),
    { inbox: inboxQueryKey(account.id, mailbox.id), priority: priorityQueryKey(account.id, mailbox.id) },
    kind === "inbox",
  );

  const markKind: MarkListKind =
    kind === "later" || kind === "waiting" || kind === "pinned" || kind === "tasks" || kind === "readingList" || kind === "flagged"
      ? kind
      : "later";
  const marks = useInfiniteQuery({
    queryKey: markListQueryKey(account.id, mailbox.id, markKind, markKind === "tasks" ? taskFilter : ""),
    queryFn: async (): Promise<{ items: MessageListItem[] }> => {
      // Later, Pinned and Tasks come in one answer (at most 1,000, decision 0020): one page.
      if (markKind === "later") return { items: await comfortIpc.listSnoozed(markScope) };
      // Stage 20 (decision 0059): the reminders if no reply comes (at most 1,000 as well).
      if (markKind === "waiting") return { items: await reminderIpc.listFollowUps(markScope) };
      if (markKind === "pinned") return { items: await comfortIpc.listPinned(markScope) };
      // Stage 16: the reading list, most recently added first (at most 1,000 as well).
      if (markKind === "readingList") return { items: await parityIpc.listReadingList(markScope) };
      // The server's flag in every folder but Trash and Junk (at most 1,000 as well).
      if (markKind === "flagged") return { items: await comfortIpc.listFlagged(markScope) };
      return { items: await comfortIpc.listTasks(markScope, taskFilter) };
    },
    initialPageParam: null,
    getNextPageParam: () => null,
    enabled: kind === markKind,
  });

  const plainRows = useCallback((items: readonly MessageListItem[], bounds: DayBounds) => buildRows(items, bounds), []);
  // Stage 16: the reading list is in the order mail was added, so date sections would repeat.
  const addedRows = useCallback(
    (items: readonly MessageListItem[]): ListRow[] =>
      items.map((item, position) => ({ kind: "message", key: `message-${String(item.id)}`, item, position })),
    [],
  );

  return useMemo((): MessageListSource | undefined => {
    switch (kind) {
      case "folder":
        return undefined;
      case "inbox":
        return sorted;
      case "readingList":
        return {
          query: marks,
          buildRows: addedRows,
          total: marks.data?.pages[0]?.items.length ?? 0,
          empty: { icon: BookOpen, title: t("organize.readingList.emptyTitle"), body: t("organize.readingList.emptyBody") },
        };
      case "later":
      case "waiting":
      case "pinned":
      case "flagged":
      case "tasks": {
        const icon =
          kind === "later" ? Clock : kind === "waiting" ? MailClock : kind === "pinned" ? Pin : kind === "flagged" ? Flag : ListTodo;
        const empty =
          kind === "tasks" && taskFilter !== "open"
            ? { icon, title: t(`comfort.tasks.empty.${taskFilter}`), body: t("comfort.views.tasks.emptyBody") }
            : { icon, title: t(`comfort.views.${kind}.emptyTitle`), body: t(`comfort.views.${kind}.emptyBody`) };
        return {
          query: marks,
          buildRows: plainRows,
          total: marks.data?.pages[0]?.items.length ?? 0,
          empty,
          ...(kind === "tasks" && { controls: <TaskFilterBar value={taskFilter} onChange={setTaskFilter} /> }),
        };
      }
    }
  }, [kind, sorted, marks, plainRows, addedRows, taskFilter, t]);
}

const TASK_FILTERS: readonly TaskFilter[] = ["open", "done", "all"];

/** Above the Tasks list: open ones (the default), done ones, or all. */
function TaskFilterBar({ value, onChange }: { value: TaskFilter; onChange: (filter: TaskFilter) => void }) {
  const { t } = useTranslation();
  return (
    <div
      role="group"
      aria-label={t("comfort.tasks.filter")}
      className="flex shrink-0 items-center gap-1 border-b border-border-subtle bg-surface-list px-4 py-2"
      data-testid="task-filter"
    >
      {TASK_FILTERS.map((filter) => (
        <button
          key={filter}
          type="button"
          aria-pressed={value === filter}
          onClick={() => {
            onChange(filter);
          }}
          className={cx(
            "rounded-full px-3 py-1 text-13 transition-colors focus-visible:outline-2 focus-visible:outline-focus-ring",
            value === filter ? "bg-selected font-medium text-primary" : "text-secondary hover:bg-hover",
          )}
        >
          {t(`comfort.tasks.filters.${filter}`)}
        </button>
      ))}
    </div>
  );
}

/**
 * First row of the "Priority" list: its name and the way back to the whole inbox. An option of the
 * list (Enter, Space or a click lead back), so it holds no button of its own.
 */
function PriorityBack() {
  const { t } = useTranslation();
  return (
    <div
      className="flex h-full items-center justify-between gap-2 rounded-card border border-priority-border bg-priority-tint px-3 shadow-card"
      data-testid="priority-back"
    >
      {/* One text names the option (WCAG 2.5.3); the drawn parts are hidden from screen readers. */}
      <span className="sr-only">{`${t("comfort.blocks.priority")}, ${t("comfort.priority.back")}`}</span>
      <span aria-hidden className="flex items-center gap-2 text-13 font-semibold text-primary">
        <Zap aria-hidden className="size-(--ui-icon-small) text-warning" strokeWidth={2} />
        {t("comfort.blocks.priority")}
      </span>
      <span aria-hidden className="flex items-center gap-1 rounded-full px-2 py-1 text-12 font-medium text-accent group-hover/row:underline">
        <Inbox aria-hidden className="size-(--ui-icon-small)" strokeWidth={1.75} />
        {t("comfort.priority.back")}
      </span>
    </div>
  );
}
