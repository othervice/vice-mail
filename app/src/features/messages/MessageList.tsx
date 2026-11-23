import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import { Inbox, LoaderCircle } from "lucide-react";
import {
  useCallback,
  useEffect,
  useEffectEvent,
  useId,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentPropsWithRef,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type Ref,
} from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import { MAX_ACTION_MESSAGES, type Account, type Mailbox, type MessageId, type MessageListItem } from "../../services/ipc";
import { MESSAGE_DRAG_TYPE } from "../../stores/drag";
import { isFirstSyncRunning } from "../accounts/selection";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { readListMetrics } from "./listMetrics";
import { MessageRowContent, useRowLabel } from "./MessageRow";
import { useMessagePages } from "./queries";
import { buildRows, dayBounds, firstDayOfWeek, formatMonthSection, type DayBounds, type ListRow, type Section } from "./rows";
import { useToday } from "./useToday";

/** Rows rendered beyond the visible ones in each direction; keeps fast scrolling free of gaps. */
const OVERSCAN = 12;
/** Ask for the next page this many rows before the end of what is loaded. */
const LOAD_AHEAD_ROWS = 80;

/** What the views around the list may ask of it. */
export interface MessageListHandle {
  focus: () => void;
  /** Gives up the selection (closes the reading pane) and puts the focus on the list. */
  clearSelection: () => void;
  /**
   * Before rows leave the list (archived, moved, deleted): if they hold the cursor or the whole
   * selection, the next message after them takes over (docs/DESIGN.md: after an action the focus
   * is on the next mail), or the one before them at the end of the list.
   */
  advancePast: (ids: readonly MessageId[]) => void;
  /** Adds the cursor row to the selection or takes it out (X, Space). */
  toggleCursor: () => void;
  /** Selects every loaded message, at most `MAX_ACTION_MESSAGES`. */
  selectAll: () => void;
  /** Loads the remaining pages (up to `MAX_ACTION_MESSAGES` messages) and selects all of them. */
  loadAndSelectAll: () => Promise<void>;
  /** Whether a node lies inside the list (shortcuts that only apply there). */
  contains: (node: Node | null) => boolean;
}

/** What the list reports to its view whenever the selection or the rows behind it change. */
export interface ListSelection {
  /** The row with the keyboard cursor, if any. */
  cursor: MessageListItem | null;
  /** Selected messages in list order (the reading pane shows the one if there is one). */
  items: readonly MessageListItem[];
  /** Messages loaded so far, and whether the folder has more pages. */
  loaded: number;
  hasMore: boolean;
}

export const NO_SELECTION: ListSelection = { cursor: null, items: [], loaded: 0, hasMore: false };

interface MessageListProps {
  account: Account;
  mailbox: Mailbox;
  /** Name of the folder as shown in the header, for the list's accessible name. */
  folderName: string;
  showRecipients: boolean;
  platform: ShortcutPlatform;
  /** Rows an action is moving out of this folder right now: hidden before the core answers. */
  hiddenIds?: ReadonlySet<MessageId>;
  /** The selection changed (click, keys, focus, or selected rows disappeared). */
  onSelectionChange?: (selection: ListSelection) => void;
  /** Enter or a double click: open the message and move the focus to it. */
  onOpen?: (messageId: MessageId) => void;
  /** Quick actions of a row (mouse shortcuts; the same actions have shortcuts and a toolbar). */
  renderQuickActions?: (item: MessageListItem, selected: boolean) => ReactNode;
  /** Rows are dragged (the selection if the row is part of it, else that row alone). */
  onDragRows?: (items: readonly MessageListItem[]) => void;
  onDragEnd?: () => void;
  handle?: Ref<MessageListHandle>;
}

/** DOM id of a row's preview text, which describes the row's option. */
function previewDomId(optionId: string): string {
  return `${optionId}-preview`;
}

/** A message by id, with its position as fallback when it disappears (deleted elsewhere). */
interface Cursor {
  id: MessageId;
  position: number;
}

interface SelectionState {
  /** The keyboard cursor: `aria-activedescendant`, the row arrow keys move from. */
  cursor: Cursor | null;
  selected: ReadonlySet<MessageId>;
  /** Where a Shift range starts: the last row clicked or moved to without Shift. */
  anchor: Cursor | null;
}

const EMPTY_STATE: SelectionState = { cursor: null, selected: new Set(), anchor: null };

function only(cursor: Cursor): SelectionState {
  return { cursor, selected: new Set([cursor.id]), anchor: cursor };
}

function SectionLabel({ section, bounds }: { section: Section; bounds: DayBounds }) {
  const { t, i18n } = useTranslation();
  const label = section.kind === "month" ? formatMonthSection(section, bounds, i18n.language) : t(`list.sections.${section.kind}`);
  return <span className="truncate">{label}</span>;
}

type MessageOptionProps = ComponentPropsWithRef<"div"> & {
  item: MessageListItem;
  showRecipients: boolean;
  /** Id of the preview text, which describes the option (read after its name). */
  previewId: string | undefined;
  "data-index": number;
};

/** The `option` element of a message row, named for screen readers by `useRowLabel`. */
function MessageOption({ item, showRecipients, previewId, children, ...rest }: MessageOptionProps) {
  const label = useRowLabel(item, showRecipients);
  return (
    <div role="option" aria-label={label} aria-describedby={previewId} {...rest}>
      {children}
    </div>
  );
}

function SkeletonRows({ label }: { label: string }) {
  return (
    <div className="flex flex-col px-2 pt-2" data-testid="list-skeleton">
      <p role="status" className="sr-only">
        {label}
      </p>
      {Array.from({ length: 8 }, (_, i) => (
        <div key={i} aria-hidden className="flex h-(--ui-row-height) items-center gap-3 px-3">
          <span className="w-4" />
          <span className="size-(--ui-avatar) shrink-0 rounded-full bg-hover" />
          <span className="h-3 w-(--ui-list-sender-width) shrink-0 rounded-control bg-hover" />
          <span className="h-3 flex-1 rounded-control bg-hover" />
        </div>
      ))}
    </div>
  );
}

function messagesOf(rows: readonly ListRow[]): MessageListItem[] {
  return rows.flatMap((row) => (row.kind === "message" ? [row.item] : []));
}

/**
 * The message list of one folder: virtualized rows of fixed height (TanStack Virtual), date
 * sections, pages loaded on demand while scrolling, reloaded on core events.
 *
 * Accessibility (decision of stage 4): a multi-select `listbox` with `option`s. The focus stays on
 * the list and `aria-activedescendant` points at the cursor row, which is always rendered, also
 * when scrolled away; screen readers announce "n of m" and the selected state. Options contain no
 * focusable controls: the quick actions in a row are mouse shortcuts (hidden from screen readers,
 * not in the tab order), and every one of them is also a keyboard shortcut and a button of the
 * toolbar in the reading pane or the selection bar. A `grid` with focusable cells would put
 * every action into the tab sequence of every row and fight the virtualization (focused cells
 * must stay mounted), for nothing the shortcuts and the toolbar do not already give.
 *
 * Selection: click selects one, Shift+click a range from the last click, Cmd/Ctrl+click adds or
 * removes one; arrow keys move (with Shift they extend), X or Space toggles the cursor row,
 * Cmd/Ctrl+A selects everything loaded.
 */
export function MessageList({
  account,
  mailbox,
  folderName,
  showRecipients,
  platform,
  hiddenIds,
  onSelectionChange,
  onOpen,
  renderQuickActions,
  onDragRows,
  onDragEnd,
  handle,
}: MessageListProps) {
  const { t, i18n } = useTranslation();
  const query = useMessagePages(account.id, mailbox.id);
  const today = useToday();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  /** The same element as state, so effects that observe it run once it is mounted. */
  const [scrollElement, setScrollElement] = useState<HTMLDivElement | null>(null);
  const dragImageRef = useRef<HTMLDivElement | null>(null);
  const optionPrefix = useId();
  const [metrics] = useState(readListMetrics);
  const [twoLine, setTwoLine] = useState(false);
  const [state, setState] = useState<SelectionState>(EMPTY_STATE);
  /** A pointer press is about to focus the list: its click selects, not the focus handler. */
  const pointerPress = useRef(false);
  /** First visible row before an update, to keep it in place when rows are inserted above. */
  const anchor = useRef<{ key: string; offset: number } | null>(null);

  const items = useMemo(() => {
    const all = query.data?.pages.flatMap((page) => page.items) ?? [];
    return hiddenIds && hiddenIds.size > 0 ? all.filter((item) => !hiddenIds.has(item.id)) : all;
  }, [query.data, hiddenIds]);
  const bounds = useMemo(() => dayBounds(today, firstDayOfWeek(i18n.language)), [today, i18n.language]);
  const rows = useMemo(() => buildRows(items, bounds), [items, bounds]);
  /** Row index of every message, by position. */
  const messageRows = useMemo(() => rows.flatMap((row, index) => (row.kind === "message" ? [index] : [])), [rows]);
  const rowById = useMemo(() => {
    const map = new Map<MessageId, number>();
    rows.forEach((row, index) => {
      if (row.kind === "message") map.set(row.item.id, index);
    });
    return map;
  }, [rows]);

  const messageAt = useCallback(
    (position: number): Extract<ListRow, { kind: "message" }> | undefined => {
      const index = messageRows[position];
      const row = index === undefined ? undefined : rows[index];
      return row?.kind === "message" ? row : undefined;
    },
    [rows, messageRows],
  );

  // The cursor row, or the one now at its old position if it is gone.
  let cursorRow: number | undefined;
  if (state.cursor) {
    cursorRow = rowById.get(state.cursor.id) ?? messageRows[Math.min(state.cursor.position, messageRows.length - 1)];
  }
  const cursorEntry = cursorRow === undefined ? undefined : rows[cursorRow];
  const cursorItem = cursorEntry?.kind === "message" ? cursorEntry.item : null;
  const cursorPosition = cursorEntry?.kind === "message" ? cursorEntry.position : -1;
  const cursorGone = state.cursor !== null && !rowById.has(state.cursor.id);

  // Selected messages still in the list. If the selection disappeared together with the cursor
  // (deleted elsewhere), the message that took the cursor's place is selected instead.
  const selectedItems = useMemo(() => {
    if (state.selected.size === 0) return [];
    const present = messagesOf(rows).filter((item) => state.selected.has(item.id));
    if (present.length === 0 && cursorGone && cursorItem) return [cursorItem];
    return present;
  }, [rows, state.selected, cursorGone, cursorItem]);
  const selectedIds = useMemo(() => new Set(selectedItems.map((item) => item.id)), [selectedItems]);
  const multiSelect = selectedItems.length > 1;
  const hasMore = query.hasNextPage;

  const reportSelection = useEffectEvent((selection: ListSelection) => {
    onSelectionChange?.(selection);
  });
  useEffect(() => {
    reportSelection({ cursor: cursorItem, items: selectedItems, loaded: items.length, hasMore });
  }, [cursorItem, selectedItems, items.length, hasMore]);

  const rowSize = twoLine ? metrics.twoLineRow : metrics.row;
  const count = rows.length + (hasMore ? 1 : 0);

  const estimateSize = useCallback(
    (index: number) => (rows[index]?.kind === "section" ? metrics.section : rowSize),
    [rows, metrics.section, rowSize],
  );
  const getItemKey = useCallback((index: number) => rows[index]?.key ?? "more", [rows]);
  const rangeExtractor = useCallback(
    (range: Range) => {
      const indexes = defaultRangeExtractor(range);
      if (cursorRow === undefined || indexes.includes(cursorRow)) return indexes;
      return [...indexes, cursorRow].sort((a, b) => a - b);
    },
    [cursorRow],
  );

  // TanStack Virtual's instance is mutable by design; this app does not use the React Compiler.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count,
    getScrollElement: () => scrollRef.current,
    estimateSize,
    getItemKey,
    rangeExtractor,
    overscan: OVERSCAN,
    paddingStart: metrics.padding,
    paddingEnd: metrics.padding,
    // Positions and the total height go straight to the DOM through the CSSOM (the app's CSP
    // forbids style attributes, not the CSSOM), and scrolling re-renders only when the range of
    // rendered rows changes.
    directDomUpdates: true,
    // Rows have fixed heights from the tokens; never measure them.
    useCachedMeasurements: true,
  });

  // Below 720 px of list width rows get two lines (docs/DESIGN.md). The virtualizer does not
  // report width changes, so the list watches its own width.
  useLayoutEffect(() => {
    if (!scrollElement) return undefined;
    const observer = new ResizeObserver(() => {
      const width = scrollElement.clientWidth;
      setTwoLine(width > 0 && width < metrics.twoLineBelow);
    });
    observer.observe(scrollElement);
    return () => {
      observer.disconnect();
    };
  }, [scrollElement, metrics.twoLineBelow]);

  // Row heights changed (two-line layout, e.g. the reading pane opened): recompute all
  // positions and keep the cursor row in view.
  const keepCursorInView = useEffectEvent(() => {
    if (cursorRow !== undefined) virtualizer.scrollToIndex(cursorRow, { align: "auto" });
  });
  useLayoutEffect(() => {
    virtualizer.measure();
    keepCursorInView();
  }, [virtualizer, rowSize]);

  // New rows above the first visible one (new mail while scrolled down) must not push the list.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    const kept = anchor.current;
    if (!element || !kept || element.scrollTop <= 0) return;
    const index = rows.findIndex((row) => row.key === kept.key);
    if (index < 0) return;
    let start = metrics.padding;
    for (let i = 0; i < index; i += 1) start += estimateSize(i);
    const wanted = start + kept.offset;
    if (Math.abs(element.scrollTop - wanted) >= 1) element.scrollTop = wanted;
  }, [rows, estimateSize, metrics.padding]);

  const virtualItems = virtualizer.getVirtualItems();
  const lastRendered = virtualizer.range?.endIndex ?? -1;

  // Next page when the end of what is loaded comes near. Never while any fetch runs: a reload
  // after a core event must not be cancelled by it.
  const { fetchNextPage, isFetching, isFetchNextPageError } = query;
  useLayoutEffect(() => {
    if (!hasMore || isFetching || isFetchNextPageError) return;
    if (lastRendered >= rows.length - LOAD_AHEAD_ROWS) void fetchNextPage();
  }, [hasMore, isFetching, isFetchNextPageError, lastRendered, rows.length, fetchNextPage]);

  /** Where a message is now; its remembered position if it is gone. */
  const positionOf = (cursor: Cursor): number => {
    const index = rowById.get(cursor.id);
    const row = index === undefined ? undefined : rows[index];
    return row?.kind === "message" ? row.position : cursor.position;
  };

  /** Every message between the anchor and `to` (inclusive), by position. */
  const range = (from: Cursor | null, to: Cursor): Set<MessageId> => {
    const start = from ? positionOf(from) : to.position;
    const [low, high] = start <= to.position ? [start, to.position] : [to.position, start];
    const ids = new Set<MessageId>();
    for (let position = low; position <= high && ids.size < MAX_ACTION_MESSAGES; position += 1) {
      const row = messageAt(position);
      if (row) ids.add(row.item.id);
    }
    return ids;
  };

  const selectAllOf = (all: readonly MessageListItem[]) => {
    const chosen = all.slice(0, MAX_ACTION_MESSAGES);
    const first = chosen[0];
    if (!first) return;
    setState((current) => ({
      cursor: current.cursor && chosen.some((item) => item.id === current.cursor?.id) ? current.cursor : { id: first.id, position: 0 },
      selected: new Set(chosen.map((item) => item.id)),
      anchor: { id: first.id, position: 0 },
    }));
  };

  useImperativeHandle(
    handle,
    () => ({
      focus: () => {
        scrollRef.current?.focus({ preventScroll: true });
      },
      clearSelection: () => {
        // Focus first: focusing an unselected list selects the first visible message.
        scrollRef.current?.focus({ preventScroll: true });
        setState(EMPTY_STATE);
      },
      advancePast: (ids) => {
        const leaving = new Set(ids);
        const cursorLeaves = state.cursor !== null && leaving.has(state.cursor.id);
        const selectionLeaves = selectedItems.length > 0 && selectedItems.every((item) => leaving.has(item.id));
        if (!cursorLeaves && !selectionLeaves) return;
        const positions = messagesOf(rows).flatMap((item, position) => (leaving.has(item.id) ? [position] : []));
        if (positions.length === 0) return;
        const last = Math.max(...positions);
        const first = Math.min(...positions);
        let next: Extract<ListRow, { kind: "message" }> | undefined;
        for (let position = last + 1; position < messageRows.length && !next; position += 1) {
          const row = messageAt(position);
          if (row && !leaving.has(row.item.id)) next = row;
        }
        for (let position = first - 1; position >= 0 && !next; position -= 1) {
          const row = messageAt(position);
          if (row && !leaving.has(row.item.id)) next = row;
        }
        if (!next) {
          setState(EMPTY_STATE);
          return;
        }
        // Its position once the leaving rows are gone (the fallback if it disappears too).
        const removedBefore = positions.filter((position) => position < (next?.position ?? 0)).length;
        setState(only({ id: next.item.id, position: next.position - removedBefore }));
      },
      toggleCursor: () => {
        const item = cursorItem;
        if (!item) return;
        setState((current) => {
          const selected = new Set(selectedIds);
          if (!selected.delete(item.id)) selected.add(item.id);
          const cursor = { id: item.id, position: cursorPosition };
          return { ...current, cursor, selected, anchor: cursor };
        });
      },
      selectAll: () => {
        selectAllOf(messagesOf(rows));
      },
      loadAndSelectAll: async () => {
        let pages = query.data?.pages ?? [];
        let more = query.hasNextPage;
        const loaded = () => pages.reduce((sum, page) => sum + page.items.length, 0);
        // Page by page, never cancelling a reload that runs (it is awaited instead). Bounded, in
        // case the core kept answering with a next page that adds nothing.
        for (let round = 0; more && loaded() < MAX_ACTION_MESSAGES && round < MAX_ACTION_MESSAGES; round += 1) {
          const result = await fetchNextPage({ cancelRefetch: false });
          pages = result.data?.pages ?? pages;
          more = result.hasNextPage;
        }
        selectAllOf(pages.flatMap((page) => page.items).filter((item) => !hiddenIds?.has(item.id)));
      },
      contains: (node) => Boolean(node && scrollRef.current?.contains(node)),
    }),
  );

  const rememberAnchor = () => {
    const element = scrollRef.current;
    if (!element) return;
    const item = virtualizer.getVirtualItemForOffset(element.scrollTop);
    anchor.current = item ? { key: String(item.key), offset: element.scrollTop - item.start } : null;
  };

  const firstVisible = (): Cursor | null => {
    const top = virtualizer.getVirtualItemForOffset(scrollRef.current?.scrollTop ?? 0)?.index ?? 0;
    for (let index = top; index < rows.length; index += 1) {
      const row = rows[index];
      if (row?.kind === "message") return { id: row.item.id, position: row.position };
    }
    return null;
  };

  const moveTo = (position: number, direction: 1 | -1, extend: boolean) => {
    if (messageRows.length === 0) return;
    const clamped = Math.max(0, Math.min(position, messageRows.length - 1));
    const rowIndex = messageRows[clamped];
    const row = messageAt(clamped);
    if (rowIndex === undefined || !row) return;
    const cursor = { id: row.item.id, position: clamped };
    setState((current) => (extend ? { cursor, selected: range(current.anchor ?? current.cursor, cursor), anchor: current.anchor ?? current.cursor } : only(cursor)));
    // Moving up onto the first row of a section shows the section label too.
    const target = direction < 0 && rows[rowIndex - 1]?.kind === "section" ? rowIndex - 1 : rowIndex;
    virtualizer.scrollToIndex(target, { align: "auto" });
  };

  const isMod = (event: MouseEvent | KeyboardEvent) => (platform === "macos" ? event.metaKey : event.ctrlKey);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const page = Math.max(1, Math.floor((scrollRef.current?.clientHeight ?? 0) / rowSize) - 1);
    const moves: Record<string, [number, 1 | -1]> = {
      ArrowDown: [cursorPosition + 1, 1],
      ArrowUp: [Math.max(0, cursorPosition - 1), -1],
      Home: [0, -1],
      End: [messageRows.length - 1, 1],
      PageDown: [cursorPosition + page, 1],
      PageUp: [Math.max(0, cursorPosition - page), -1],
    };
    if (event.altKey || event.metaKey || event.ctrlKey) return;
    if (event.key === "Enter" && cursorItem) {
      event.preventDefault();
      onOpen?.(cursorItem.id);
      return;
    }
    if (event.key === "Escape" && (state.cursor || state.selected.size > 0)) {
      // Closes the reading pane; the list keeps the focus.
      event.preventDefault();
      setState(EMPTY_STATE);
      return;
    }
    const move = moves[event.key];
    if (!move) return;
    event.preventDefault();
    moveTo(move[0], move[1], event.shiftKey);
  };

  const onRowClick = (event: MouseEvent, row: Extract<ListRow, { kind: "message" }>) => {
    pointerPress.current = false;
    const cursor = { id: row.item.id, position: row.position };
    const onBox = event.target instanceof Element && event.target.closest("[data-row-check]") !== null;
    if (event.shiftKey) {
      setState((current) => ({ cursor, selected: range(current.anchor ?? current.cursor, cursor), anchor: current.anchor ?? current.cursor }));
    } else if (onBox || isMod(event)) {
      setState(() => {
        const selected = new Set(selectedIds);
        if (!selected.delete(row.item.id)) selected.add(row.item.id);
        return { cursor, selected, anchor: cursor };
      });
    } else {
      setState(only(cursor));
    }
    scrollRef.current?.focus({ preventScroll: true });
  };

  const onRowDragStart = (event: DragEvent, row: Extract<ListRow, { kind: "message" }>) => {
    if (!onDragRows) return;
    const dragged = selectedIds.has(row.item.id) ? selectedItems : [row.item];
    const refs = dragged.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id }));
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(MESSAGE_DRAG_TYPE, JSON.stringify(refs));
    const image = dragImageRef.current;
    if (dragged.length > 1 && image) {
      // The drag image is a snapshot taken right now; the text is set before it is taken.
      image.textContent = t("list.dragCount", { count: dragged.length });
      event.dataTransfer.setDragImage(image, 0, 0);
    }
    onDragRows(dragged);
  };

  if (query.isPending) return <SkeletonRows label={t("list.loading")} />;

  if (query.isError && !query.data) {
    return (
      <div className="px-6 py-4">
        <ErrorNotice
          context={t("errors.context.messages")}
          error={query.error}
          onRetry={() => {
            void query.refetch();
          }}
        />
      </div>
    );
  }

  if (items.length === 0) {
    if (isFirstSyncRunning(account.status)) {
      return (
        <div className="flex min-h-0 flex-1 flex-col">
          <EmptyState
            icon={LoaderCircle}
            title={t("list.firstSync.title")}
            body={t("list.firstSync.body", { account: account.displayName || account.email })}
          />
        </div>
      );
    }
    return <EmptyState icon={Inbox} title={t("list.empty.title")} body={t("list.empty.body")} />;
  }

  const total = hasMore ? Math.max(mailbox.total, items.length) : items.length;
  const activeId = cursorItem ? `${optionPrefix}-${String(cursorItem.id)}` : undefined;

  return (
    <>
      {query.isRefetchError && (
        <div className="px-6 pt-4">
          <ErrorNotice
            context={t("errors.context.messages")}
            error={query.error}
            onRetry={() => {
              void query.refetch();
            }}
          />
        </div>
      )}
      <div
        ref={(element) => {
          scrollRef.current = element;
          setScrollElement(element);
        }}
        role="listbox"
        aria-multiselectable
        tabIndex={0}
        aria-label={t("list.label", { folder: folderName })}
        aria-activedescendant={activeId}
        onKeyDown={onKeyDown}
        onFocus={(event) => {
          // The list gets a cursor and a selected option with the focus: the first visible
          // message, unless there is one already. A click selects its own row instead.
          if (event.target !== event.currentTarget) return;
          if (pointerPress.current) {
            pointerPress.current = false;
            return;
          }
          const first = firstVisible();
          if (first) setState((current) => (current.cursor ? current : only(first)));
        }}
        onScroll={rememberAnchor}
        data-testid="message-list"
        className="group/list min-h-0 flex-1 overflow-y-auto px-2 outline-none"
      >
        <div ref={virtualizer.containerRef} role="presentation" className="relative w-full">
          {virtualItems.map((virtualItem) => {
            const row = rows[virtualItem.index];
            const common = {
              ref: virtualizer.measureElement,
              "data-index": virtualItem.index,
            };
            if (!row) {
              return (
                <div
                  key={virtualItem.key}
                  {...common}
                  role="presentation"
                  className="absolute top-0 left-0 flex h-(--ui-row-height) w-full items-center gap-3 px-3"
                >
                  <span aria-hidden className="w-4" />
                  <LoaderCircle aria-hidden className="size-(--ui-icon-small) text-tertiary" strokeWidth={1.75} />
                  <span className="text-13 text-tertiary">{t("list.loadingMore")}</span>
                </div>
              );
            }
            if (row.kind === "section") {
              return (
                <div
                  key={virtualItem.key}
                  {...common}
                  role="presentation"
                  aria-hidden
                  data-testid="list-section"
                  className="absolute top-0 left-0 flex h-(--ui-section-height) w-full items-end px-3 pb-1 text-12 font-semibold text-secondary"
                >
                  <SectionLabel section={row.section} bounds={bounds} />
                </div>
              );
            }
            const selected = selectedIds.has(row.item.id);
            const isCursor = virtualItem.index === cursorRow;
            const optionId = `${optionPrefix}-${String(row.item.id)}`;
            const previewId = row.item.preview?.trim() ? previewDomId(optionId) : undefined;
            return (
              <MessageOption
                key={virtualItem.key}
                {...common}
                id={optionId}
                item={row.item}
                showRecipients={showRecipients}
                previewId={previewId}
                aria-selected={selected}
                aria-setsize={total}
                aria-posinset={row.position + 1}
                draggable={onDragRows ? true : undefined}
                onMouseDown={() => {
                  // The press focuses the list; its click selects this row, not the focus handler.
                  pointerPress.current = true;
                }}
                onClick={(event) => {
                  onRowClick(event, row);
                }}
                onDoubleClick={() => {
                  setState(only({ id: row.item.id, position: row.position }));
                  onOpen?.(row.item.id);
                }}
                onDragStart={(event) => {
                  onRowDragStart(event, row);
                }}
                onDragEnd={() => {
                  pointerPress.current = false;
                  onDragEnd?.();
                }}
                data-testid="message-row"
                className={cx(
                  "group/row absolute top-0 left-0 w-full",
                  twoLine ? "h-(--ui-row-height-two-line)" : "h-(--ui-row-height)",
                )}
              >
                <MessageRowContent
                  item={row.item}
                  accountColor={account.color}
                  bounds={bounds}
                  showRecipients={showRecipients}
                  twoLine={twoLine}
                  selected={selected}
                  cursor={isCursor}
                  multiSelect={multiSelect}
                  previewId={previewId}
                  actions={renderQuickActions?.(row.item, selected)}
                />
              </MessageOption>
            );
          })}
        </div>
      </div>
      {/* Drag image for several rows: off screen, its text set when a drag starts. */}
      <div
        ref={dragImageRef}
        aria-hidden
        className="pointer-events-none fixed top-0 left-0 -translate-y-full rounded-control border border-border bg-surface-raised px-3 py-1 text-13 font-medium text-primary shadow-floating"
      />
      {query.isFetchNextPageError && (
        <div className="px-6 pb-4">
          <ErrorNotice
            context={t("errors.context.moreMessages")}
            error={query.error}
            onRetry={() => {
              void fetchNextPage();
            }}
          />
        </div>
      )}
    </>
  );
}
