import { defaultRangeExtractor, useVirtualizer, type Range } from "@tanstack/react-virtual";
import { Inbox, LoaderCircle } from "lucide-react";
import {
  useCallback,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ComponentPropsWithRef,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../components/cx";
import { EmptyState } from "../../components/EmptyState";
import { ErrorNotice } from "../../components/ErrorNotice";
import type { Account, Mailbox, MessageId, MessageListItem } from "../../services/ipc";
import { isFirstSyncRunning } from "../accounts/selection";
import { readListMetrics } from "./listMetrics";
import { MessageRowContent, useRowLabel } from "./MessageRow";
import { useMessagePages } from "./queries";
import { buildRows, dayBounds, firstDayOfWeek, formatMonthSection, type DayBounds, type Section } from "./rows";
import { useToday } from "./useToday";

/** Rows rendered beyond the visible ones in each direction; keeps fast scrolling free of gaps. */
const OVERSCAN = 12;
/** Ask for the next page this many rows before the end of what is loaded. */
const LOAD_AHEAD_ROWS = 80;

interface MessageListProps {
  account: Account;
  mailbox: Mailbox;
  /** Name of the folder as shown in the header, for the list's accessible name. */
  folderName: string;
  showRecipients: boolean;
}

/** Selected message, with its position as fallback when it disappears (deleted elsewhere). */
interface Selection {
  id: MessageId;
  position: number;
}

function SectionLabel({ section, bounds }: { section: Section; bounds: DayBounds }) {
  const { t, i18n } = useTranslation();
  const label = section.kind === "month" ? formatMonthSection(section, bounds, i18n.language) : t(`list.sections.${section.kind}`);
  return <span className="truncate">{label}</span>;
}

type MessageOptionProps = ComponentPropsWithRef<"div"> & {
  item: MessageListItem;
  showRecipients: boolean;
  "data-index": number;
};

/** The `option` element of a message row, named for screen readers by `useRowLabel`. */
function MessageOption({ item, showRecipients, children, ...rest }: MessageOptionProps) {
  const label = useRowLabel(item, showRecipients);
  return (
    <div role="option" aria-label={label} {...rest}>
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
          <span className="w-2" />
          <span className="size-(--ui-avatar) shrink-0 rounded-full bg-hover" />
          <span className="h-3 w-(--ui-list-sender-width) shrink-0 rounded-control bg-hover" />
          <span className="h-3 flex-1 rounded-control bg-hover" />
        </div>
      ))}
    </div>
  );
}

/**
 * The message list of one folder: virtualized rows of fixed height (TanStack Virtual), date
 * sections, pages loaded on demand while scrolling, reloaded on core events.
 *
 * Accessibility: a `listbox` with `option`s, focus stays on the list and `aria-activedescendant`
 * points at the selected row (it is always rendered, also when scrolled away). A listbox fits a
 * single-select list whose rows contain no controls; screen readers announce "n of m". When the
 * rows get quick actions (stage 4), they stay reachable from the keyboard through shortcuts and
 * the toolbar rather than as controls inside the options, or the list becomes a `grid`.
 */
export function MessageList({ account, mailbox, folderName, showRecipients }: MessageListProps) {
  const { t, i18n } = useTranslation();
  const query = useMessagePages(account.id, mailbox.id);
  const today = useToday();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  /** The same element as state, so effects that observe it run once it is mounted. */
  const [scrollElement, setScrollElement] = useState<HTMLDivElement | null>(null);
  const optionPrefix = useId();
  const [metrics] = useState(readListMetrics);
  const [twoLine, setTwoLine] = useState(false);
  const [selection, setSelection] = useState<Selection | null>(null);
  /** First visible row before an update, to keep it in place when rows are inserted above. */
  const anchor = useRef<{ key: string; offset: number } | null>(null);

  const items = useMemo(() => query.data?.pages.flatMap((page) => page.items) ?? [], [query.data]);
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

  // The selected message, or the one now at its old position if it is gone.
  let selectedRow: number | undefined;
  if (selection) {
    selectedRow =
      rowById.get(selection.id) ?? messageRows[Math.min(selection.position, messageRows.length - 1)];
  }
  const selectedItem = selectedRow === undefined ? undefined : rows[selectedRow];
  const selectedPosition = selectedItem?.kind === "message" ? selectedItem.position : -1;

  const rowSize = twoLine ? metrics.twoLineRow : metrics.row;
  const hasMore = query.hasNextPage;
  const count = rows.length + (hasMore ? 1 : 0);

  const estimateSize = useCallback(
    (index: number) => (rows[index]?.kind === "section" ? metrics.section : rowSize),
    [rows, metrics.section, rowSize],
  );
  const getItemKey = useCallback((index: number) => rows[index]?.key ?? "more", [rows]);
  const rangeExtractor = useCallback(
    (range: Range) => {
      const indexes = defaultRangeExtractor(range);
      if (selectedRow === undefined || indexes.includes(selectedRow)) return indexes;
      return [...indexes, selectedRow].sort((a, b) => a - b);
    },
    [selectedRow],
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

  // Row heights changed (two-line layout): recompute all positions.
  useLayoutEffect(() => {
    virtualizer.measure();
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

  const rememberAnchor = () => {
    const element = scrollRef.current;
    if (!element) return;
    const item = virtualizer.getVirtualItemForOffset(element.scrollTop);
    anchor.current = item ? { key: String(item.key), offset: element.scrollTop - item.start } : null;
  };

  const firstVisibleSelection = (): Selection | null => {
    const top = virtualizer.getVirtualItemForOffset(scrollRef.current?.scrollTop ?? 0)?.index ?? 0;
    for (let index = top; index < rows.length; index += 1) {
      const row = rows[index];
      if (row?.kind === "message") return { id: row.item.id, position: row.position };
    }
    return null;
  };

  const select = (position: number, direction: 1 | -1) => {
    if (messageRows.length === 0) return;
    const clamped = Math.max(0, Math.min(position, messageRows.length - 1));
    const rowIndex = messageRows[clamped];
    const row = rowIndex === undefined ? undefined : rows[rowIndex];
    if (rowIndex === undefined || row?.kind !== "message") return;
    setSelection({ id: row.item.id, position: clamped });
    // Moving up onto the first row of a section shows the section label too.
    const target = direction < 0 && rows[rowIndex - 1]?.kind === "section" ? rowIndex - 1 : rowIndex;
    virtualizer.scrollToIndex(target, { align: "auto" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const page = Math.max(1, Math.floor((scrollRef.current?.clientHeight ?? 0) / rowSize) - 1);
    const moves: Record<string, [number, 1 | -1]> = {
      ArrowDown: [selectedPosition + 1, 1],
      ArrowUp: [Math.max(0, selectedPosition - 1), -1],
      Home: [0, -1],
      End: [messageRows.length - 1, 1],
      PageDown: [selectedPosition + page, 1],
      PageUp: [Math.max(0, selectedPosition - page), -1],
    };
    const move = moves[event.key];
    if (!move || event.altKey || event.metaKey || event.ctrlKey) return;
    event.preventDefault();
    select(move[0], move[1]);
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
  const activeId =
    selectedItem?.kind === "message" ? `${optionPrefix}-${String(selectedItem.item.id)}` : undefined;

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
        tabIndex={0}
        aria-label={t("list.label", { folder: folderName })}
        aria-activedescendant={activeId}
        onKeyDown={onKeyDown}
        onFocus={(event) => {
          // A single-select listbox gets a selected option with the focus: the first visible
          // message, unless there is a selection already (a click sets it just before).
          if (event.target !== event.currentTarget) return;
          const first = firstVisibleSelection();
          if (first) setSelection((current) => current ?? first);
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
                  <span aria-hidden className="w-2" />
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
            const selected = virtualItem.index === selectedRow;
            return (
              <MessageOption
                key={virtualItem.key}
                {...common}
                id={`${optionPrefix}-${String(row.item.id)}`}
                item={row.item}
                showRecipients={showRecipients}
                aria-selected={selected}
                aria-setsize={total}
                aria-posinset={row.position + 1}
                onMouseDown={(event) => {
                  // Keeps the browser from focusing first and selecting the first row on focus.
                  event.preventDefault();
                }}
                onClick={() => {
                  setSelection({ id: row.item.id, position: row.position });
                  scrollRef.current?.focus({ preventScroll: true });
                }}
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
                />
              </MessageOption>
            );
          })}
        </div>
      </div>
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
