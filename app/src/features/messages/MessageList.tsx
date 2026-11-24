import { useQueries, useQueryClient, type QueryObserverResult } from "@tanstack/react-query";
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
import { MAX_ACTION_MESSAGES, type Account, type Mailbox, type MessageId, type MessageListItem, type ThreadView } from "../../services/ipc";
import { MESSAGE_DRAG_TYPE } from "../../stores/drag";
import { useLayoutStore } from "../../stores/layout";
import { useListSelectAnswer, type ListSelectScope } from "../../stores/listSelect";
import { isFirstSyncRunning } from "../accounts/selection";
import type { RowMark } from "../comfort/RowMarks";
import type { ShortcutPlatform } from "../actions/shortcuts";
import { useMailboxes } from "../mailboxes/queries";
import { conversationQuery } from "../threads/queries";
import { childKey, conversationIdent, repliesOf, treePlaces, withChildren, type ChildrenState, type ChildStatus } from "../threads/tree";
import { readListMetrics } from "./listMetrics";
import type { MessageListSource } from "./listSource";
import { ChildRowContent, ChildStatusRow, MessageRowContent, useChildLabel, useRowLabel, type RowDecoration } from "./MessageRow";
import { useMessagePages } from "./queries";
import { ListLayoutContext, ListTreeContext } from "./listLayout";
import { buildRows, dayBounds, firstDayOfWeek, formatFullDate, formatMonthSection, type DayBounds, type ListRow, type Section } from "./rows";
import { useToday } from "./useToday";

/** Marks a row in the priority card leaves out: the card stands for them. */
const PRIORITY_CARD_MARKS: readonly RowMark[] = ["important"];

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
  /**
   * Stage 9 (decision 0028): selects only this message, like a click, scrolls it into view and
   * lets the view open it (`onSelectedById`), if it is among the loaded rows. False otherwise
   * (not loaded, or a member of a closed bundle, which is no row).
   */
  selectById: (messageId: MessageId) => boolean;
  /**
   * Stage 17 (decision 0049): unfolds the conversation `ident` and selects its message
   * `messageId` once its rows are there (a source of an AI answer in the open conversation).
   */
  showInConversation: (ident: string, messageId: MessageId) => void;
}

/** What the list reports to its view whenever the selection or the rows behind it change. */
export interface ListSelection {
  /** The row with the keyboard cursor, if any. */
  cursor: MessageListItem | null;
  /** Stage 17: the cursor row is a message of an unfolded conversation (acts on itself only). */
  cursorIsChild: boolean;
  /** Selected messages in list order (the reading pane shows the one if there is one). */
  items: readonly MessageListItem[];
  /**
   * Stage 17 (decision 0049): selected messages that were chosen as child rows of an unfolded
   * conversation: actions act on them alone, not on their conversation. A message selected as a
   * conversation row as well counts as the conversation.
   */
  childIds: ReadonlySet<MessageId>;
  /**
   * The identity of each selected row, in list order: a conversation's (its thread) stays while a
   * newer message becomes its row's message, so the view keeps the same reading pane.
   */
  keys: readonly string[];
  /** Messages loaded so far, and whether the folder has more pages. */
  loaded: number;
  hasMore: boolean;
}

export const NO_SELECTION: ListSelection = { cursor: null, cursorIsChild: false, items: [], childIds: new Set(), keys: [], loaded: 0, hasMore: false };

interface MessageListProps {
  /** The folder's account and the folder; absent for lists that mix accounts (stage 7). */
  account?: Account | undefined;
  mailbox?: Mailbox | undefined;
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
  /**
   * Quick actions of a row (mouse shortcuts; the same actions have shortcuts and a toolbar).
   * `child`: a message of an unfolded conversation, which acts on itself only (stage 17).
   */
  renderQuickActions?: (item: MessageListItem, selected: boolean, child: boolean) => ReactNode;
  /**
   * Rows are dragged (the selection if the row is part of it, else that row alone); `childIds`:
   * those dragged as child rows, which move alone (stage 17).
   */
  onDragRows?: (items: readonly MessageListItem[], childIds: ReadonlySet<MessageId>) => void;
  onDragEnd?: () => void;
  handle?: Ref<MessageListHandle>;
  /** Stage 9: rows from another source than the folder's pages (sorted inbox, Later, Pinned, Tasks). */
  source?: MessageListSource | undefined;
  /**
   * Stage 9 (decision 0028): the list answers requests to select a message by id (a click on a
   * notification, a source of an AI answer) for this scope; absent: it answers none.
   */
  selectScope?: ListSelectScope | undefined;
  /** A message was selected by id: the view opens it (in place of the list, too). */
  onSelectedById?: (messageId: MessageId) => void;
}

/** DOM id of the option of a drawn row with an action (bundle, "Show all"). */
function actionDomId(prefix: string, key: string): string {
  return `${prefix}-x-${key}`;
}

/** DOM id of a row's full date, which describes the row's option. */
function dateDomId(optionId: string): string {
  return `${optionId}-date`;
}

type MessageRowEntry = Extract<ListRow, { kind: "message" }>;
type ChildRowEntry = Extract<ListRow, { kind: "child" }>;
/** Rows that hold a message and can be selected: messages and, in a tree, child rows. */
type SelectableRow = MessageRowEntry | ChildRowEntry;

function isSelectable(row: ListRow | undefined): row is SelectableRow {
  return row?.kind === "message" || row?.kind === "child";
}

/**
 * A row by its selection key (a message, a conversation in a tree, or a message of an unfolded
 * conversation), its message, and its position as fallback when it disappears (deleted
 * elsewhere).
 */
interface Cursor {
  key: string;
  id: MessageId;
  position: number;
}

interface SelectionState {
  /** The keyboard cursor: `aria-activedescendant`, the row arrow keys move from. */
  cursor: Cursor | null;
  /** Selection keys of the selected rows. */
  selected: ReadonlySet<string>;
  /** Where a Shift range starts: the last row clicked or moved to without Shift. */
  anchor: Cursor | null;
  /**
   * Key of a drawn row with an action (bundle, "Show all") that holds the keyboard cursor
   * instead of a message; absent while the cursor is on a message. Selection stays as it was.
   */
  action?: string | null;
}

const EMPTY_STATE: SelectionState = { cursor: null, selected: new Set(), anchor: null };

function only(cursor: Cursor): SelectionState {
  return { cursor, selected: new Set([cursor.key]), anchor: cursor };
}

/**
 * Where a row sits in its card (decision 0031): the first and last segment close the card, a
 * single row is a card of its own. Undefined for rows outside a card.
 */
function cardEdge(rows: readonly ListRow[], index: number): "start" | "middle" | "end" | "both" | undefined {
  const cardOf = (row: ListRow | undefined) => (row && row.kind !== "section" ? row.card : undefined);
  const card = cardOf(rows[index]);
  if (!card) return undefined;
  const same = (other: ListRow | undefined) => cardOf(other) === card;
  const first = !same(rows[index - 1]);
  const last = !same(rows[index + 1]);
  return first && last ? "both" : first ? "start" : last ? "end" : "middle";
}

function SectionLabel({ section, bounds }: { section: Section; bounds: DayBounds }) {
  const { t, i18n } = useTranslation();
  const label = section.kind === "month" ? formatMonthSection(section, bounds, i18n.language) : t(`list.sections.${section.kind}`);
  return <span className="truncate">{label}</span>;
}

type MessageOptionProps = ComponentPropsWithRef<"div"> & {
  item: MessageListItem;
  showRecipients: boolean;
  /** Id of the full date, which describes the option (read after its name). */
  dateId: string;
  bounds: DayBounds;
  /** Stage 7: conversation size and search highlights of the row. */
  decoration?: RowDecoration | undefined;
  /** Stage 17: `treeitem` in a conversation tree, else `option`. */
  role: "option" | "treeitem";
  "data-index": number;
};

/**
 * The `option` (in a conversation tree the `treeitem`) of a message row. Its accessible name comes
 * from its content (WCAG 2.5.3, axe `label-content-name-mismatch`): a visually hidden text in
 * reading order that contains every visible word of the row (sender, subject, preview, time) plus
 * the states the row only shows as symbols (`useRowLabel`); the drawn row is hidden from screen
 * readers. The full date describes it.
 */
function MessageOption({ item, showRecipients, dateId, bounds, decoration, children, ...rest }: MessageOptionProps) {
  const { i18n } = useTranslation();
  const label = useRowLabel(item, showRecipients, decoration, bounds);
  return (
    <div aria-describedby={dateId} {...rest}>
      <span className="sr-only" data-testid="row-name">
        {label}
      </span>
      {/* Hidden: not part of the name, but `aria-describedby` still reads it. */}
      <span id={dateId} hidden>
        {formatFullDate(item.date, i18n.language)}
      </span>
      {children}
    </div>
  );
}

type ChildOptionProps = ComponentPropsWithRef<"div"> & {
  row: ChildRowEntry;
  dateId: string;
  bounds: DayBounds;
  twoLine: boolean;
  selected: boolean;
  cursor: boolean;
  multiSelect: boolean;
  actions?: ReactNode;
  "data-index": number;
};

/**
 * Stage 17 (decision 0049): the `treeitem` of a message of an unfolded conversation, named by its
 * content like a message row (`useChildLabel`), with the folder where it lies outside the
 * conversation's folder (the user's own answer in Sent).
 */
function ChildOption({ row, dateId, bounds, twoLine, selected, cursor, multiSelect, actions, ...rest }: ChildOptionProps) {
  const { t, i18n } = useTranslation();
  const elsewhere = row.item.mailboxId !== row.parentItem.mailboxId;
  const mailboxes = useMailboxes(elsewhere ? row.item.accountId : null).data;
  const mailbox = elsewhere ? mailboxes?.find((m) => m.id === row.item.mailboxId) : undefined;
  const folder = mailbox ? (mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`)) : null;
  const label = useChildLabel(row.item, folder, bounds);
  return (
    <div role="treeitem" aria-describedby={dateId} {...rest}>
      <span className="sr-only" data-testid="row-name">
        {label}
      </span>
      <span id={dateId} hidden>
        {formatFullDate(row.item.date, i18n.language)}
      </span>
      <ChildRowContent
        item={row.item}
        folder={folder}
        bounds={bounds}
        twoLine={twoLine}
        selected={selected}
        cursor={cursor}
        multiSelect={multiSelect}
        last={row.last}
        actions={actions}
      />
    </div>
  );
}

function SkeletonRows({ label }: { label: string }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col bg-surface-list px-2 pt-2" data-testid="list-skeleton">
      <p role="status" className="sr-only">
        {label}
      </p>
      {Array.from({ length: 8 }, (_, i) => (
        <div key={i} aria-hidden className="flex h-(--ui-row-height) items-center gap-3 px-3">
          <span className="w-4" />
          <span className="size-(--ui-avatar) shrink-0 rounded-full bg-badge" />
          <span className="h-3 w-(--ui-list-sender-width) shrink-0 rounded-full bg-badge" />
          <span className="h-3 flex-1 rounded-full bg-hover" />
        </div>
      ))}
    </div>
  );
}

/** A conversation row in a tree: its identity and how many replies it has. */
interface Conversation {
  ident: string;
  replies: number;
}

/** What the tree knows of the messages of one unfolded conversation, by its identity. */
type ChildQueries = Readonly<Record<string, { status: "pending" | "error" | "success"; data: ThreadView | undefined }>>;

function childrenState(query: ChildQueries[string] | undefined): ChildrenState {
  if (!query || (query.status === "pending" && !query.data)) return { kind: "loading" };
  if (!query.data) return { kind: "error" };
  return { kind: "loaded", messages: query.data.messages.map((entry) => entry.message), total: query.data.total };
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
 * Stage 17 (decision 0049): a list of conversations is a multi-select `tree` instead, flat in the
 * DOM like the sidebar's trees: `treeitem`s with `aria-level`, `aria-posinset`, `aria-setsize` and,
 * on conversations with replies, `aria-expanded`. → unfolds a conversation (then moves to its
 * first message), ← folds it or moves from a message to its conversation; ↑ and ↓ walk every
 * visible row. The messages of an unfolded conversation load when it unfolds, below its row.
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
  source,
  selectScope,
  onSelectedById,
}: MessageListProps) {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const folderQuery = useMessagePages(account?.id ?? "", mailbox?.id ?? 0, source === undefined && account !== undefined && mailbox !== undefined);
  const query = source?.query ?? folderQuery;
  const build = source?.buildRows ?? buildRows;
  const tree = source?.conversations === true;
  const today = useToday();
  const scrollRef = useRef<HTMLDivElement | null>(null);
  /** The same element as state, so effects that observe it run once it is mounted. */
  const [scrollElement, setScrollElement] = useState<HTMLDivElement | null>(null);
  const dragImageRef = useRef<HTMLDivElement | null>(null);
  const optionPrefix = useId();
  // Stage 8: compact or comfortable rows and the text size; the layout store sets the tokens
  // before it changes, so the heights are read again for a new value.
  const metricsKey = useLayoutStore((state) => `${state.density}/${String(state.textSize)}`);
  const [metrics, setMetrics] = useState(readListMetrics);
  const [readFor, setReadFor] = useState(metricsKey);
  if (readFor !== metricsKey) {
    setReadFor(metricsKey);
    setMetrics(readListMetrics());
  }
  const [twoLine, setTwoLine] = useState(false);
  const [state, setState] = useState<SelectionState>(EMPTY_STATE);
  /** Stage 17: identities of the unfolded conversations; they stay through reloads and new mail. */
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  /** Stage 17: a message of a conversation to select once its rows are there. */
  const [pendingChild, setPendingChild] = useState<{ key: string; id: MessageId } | null>(null);
  /** The messages of the pages `loadAndSelectAll` loaded: selected once they are rows. */
  const [selectAllRequest, setSelectAllRequest] = useState<readonly MessageId[] | null>(null);
  /** A pointer press is about to focus the list: its click selects, not the focus handler. */
  const pointerPress = useRef(false);
  /** First visible row before an update, to keep it in place when rows are inserted above. */
  const anchor = useRef<{ key: string; offset: number } | null>(null);

  const items = useMemo(() => {
    const all = query.data?.pages.flatMap((page) => page.items) ?? [];
    return hiddenIds && hiddenIds.size > 0 ? all.filter((item) => !hiddenIds.has(item.id)) : all;
  }, [query.data, hiddenIds]);
  const bounds = useMemo(() => dayBounds(today, firstDayOfWeek(i18n.language)), [today, i18n.language]);
  const baseRows = useMemo(() => build(items, bounds), [build, items, bounds]);

  // Stage 17: the conversation of every row, from the source's decoration. Sources build a new
  // `decorate` on every render; the map is kept while what it says stays the same, so the rows
  // (and the selection reported from them) do not change with every render.
  const decorate = source?.decorate;
  const conversationList = tree
    ? items.map((item) => {
        const decoration = decorate?.(item);
        return [item.id, { ident: conversationIdent(item, decoration), replies: repliesOf(decoration) }] as const;
      })
    : [];
  const conversationSignature = conversationList.map(([id, c]) => `${String(id)}:${c.ident}:${String(c.replies)}`).join(",");
  const [conversations, setConversations] = useState<{ signature: string; map: ReadonlyMap<MessageId, Conversation> }>(() => ({
    signature: conversationSignature,
    map: new Map(conversationList),
  }));
  if (conversations.signature !== conversationSignature) {
    setConversations({ signature: conversationSignature, map: new Map(conversationList) });
  }
  const conversationOf = conversations.map;

  /** The identity of a message row's conversation when it can unfold, else null. */
  const expandable = useCallback(
    (row: MessageRowEntry): string | null => {
      const conversation = conversationOf.get(row.item.id);
      return conversation && conversation.replies > 0 ? conversation.ident : null;
    },
    [conversationOf],
  );

  // The unfolded conversations among the rows, and their messages (loaded on unfolding).
  const unfolded = useMemo(
    () =>
      tree
        ? baseRows.flatMap((row) => {
            if (row.kind !== "message") return [];
            const ident = expandable(row);
            return ident !== null && expanded.has(ident) ? [{ ident, item: row.item }] : [];
          })
        : [],
    [tree, baseRows, expandable, expanded],
  );
  const combineChildren = useCallback(
    (results: QueryObserverResult<ThreadView>[]): ChildQueries =>
      Object.fromEntries(results.map((result, index) => [unfolded[index]?.ident ?? "", { status: result.status, data: result.data }])),
    [unfolded],
  );
  const childQueries = useQueries({
    queries: unfolded.map(({ ident, item }) => conversationQuery(item, ident)),
    combine: combineChildren,
  });
  const retryChildren = useCallback(
    (ident: string, item: MessageListItem) => {
      void queryClient.refetchQueries({ queryKey: conversationQuery(item, ident).queryKey });
    },
    [queryClient],
  );
  const statusRow = useCallback(
    (status: ChildStatus, parent: MessageRowEntry) => {
      const ident = expandable(parent) ?? "";
      const text =
        status.kind === "loading"
          ? t("threads.childrenLoading")
          : status.kind === "error"
            ? t("threads.childrenFailed")
            : t("threads.truncated", { shown: status.shown, total: status.total });
      return {
        render: () => <ChildStatusRow kind={status.kind} text={text} last={status.kind !== "truncated"} />,
        ...(status.kind === "error" && {
          activate: () => {
            retryChildren(ident, parent.item);
          },
        }),
      };
    },
    [expandable, retryChildren, t],
  );

  const rows = useMemo(
    () =>
      tree
        ? withChildren(baseRows, {
            expandable,
            expanded,
            children: (ident) => childrenState(childQueries[ident]),
            hidden: hiddenIds,
            status: statusRow,
          })
        : baseRows,
    [tree, baseRows, expandable, expanded, childQueries, hiddenIds, statusRow],
  );

  /** The selection key of every row ("" for rows that hold no message). */
  const rowKeys = useMemo(() => {
    const taken = new Set<string>();
    return rows.map((row) => {
      if (row.kind === "child") return childKey(row.parent, row.item.id);
      if (row.kind !== "message") return "";
      const ident = tree ? (conversationOf.get(row.item.id)?.ident ?? `m${String(row.item.id)}`) : `m${String(row.item.id)}`;
      // A conversation shown twice (never expected) keeps its message as key the second time.
      const key = taken.has(ident) ? `m${String(row.item.id)}` : ident;
      taken.add(key);
      return key;
    });
  }, [rows, tree, conversationOf]);
  /** Row index of every selectable row (messages, child rows), by position. */
  const selectableRows = useMemo(() => rows.flatMap((row, index) => (isSelectable(row) ? [index] : [])), [rows]);
  /** Row index of every option or tree item (messages, child rows, drawn rows with an action), in order. */
  const entryRows = useMemo(
    () => rows.flatMap((row, index) => (isSelectable(row) || (row.kind === "extra" && row.action) ? [index] : [])),
    [rows],
  );
  /** Place of each option among all options, by row index, for `aria-posinset`. */
  const entryPosition = useMemo(() => new Map(entryRows.map((rowIndex, position) => [rowIndex, position])), [entryRows]);
  /** Stage 17: level, place and set size of every tree item. */
  const places = useMemo(() => (tree ? treePlaces(rows) : null), [tree, rows]);
  const rowByKey = useMemo(() => {
    const map = new Map<string, number>();
    rowKeys.forEach((key, index) => {
      if (key !== "") map.set(key, index);
    });
    return map;
  }, [rowKeys]);
  /** Row index of each conversation's row, by identity (stage 17). */
  const conversationRow = useMemo(() => {
    const map = new Map<string, number>();
    rows.forEach((row, index) => {
      if (row.kind !== "message") return;
      const ident = expandable(row);
      if (ident !== null && !map.has(ident)) map.set(ident, index);
    });
    return map;
  }, [rows, expandable]);

  const selectableAt = useCallback(
    (position: number): SelectableRow | undefined => {
      const index = selectableRows[position];
      const row = index === undefined ? undefined : rows[index];
      return isSelectable(row) ? row : undefined;
    },
    [rows, selectableRows],
  );
  const cursorOf = useCallback(
    (row: SelectableRow): Cursor => ({ key: rowKeys[rows.indexOf(row)] ?? "", id: row.item.id, position: row.position }),
    [rows, rowKeys],
  );
  const cursorAt = (index: number): Cursor | null => {
    const row = rows[index];
    return isSelectable(row) ? { key: rowKeys[index] ?? "", id: row.item.id, position: row.position } : null;
  };

  // The cursor row, or the one now at its old position if it is gone.
  let cursorRow: number | undefined;
  if (state.cursor) {
    cursorRow = rowByKey.get(state.cursor.key) ?? selectableRows[Math.min(state.cursor.position, selectableRows.length - 1)];
  }
  const cursorEntry = cursorRow === undefined ? undefined : rows[cursorRow];
  const cursorSelectable = isSelectable(cursorEntry) ? cursorEntry : undefined;
  const cursorItem = cursorSelectable?.item ?? null;
  const cursorIsChild = cursorSelectable?.kind === "child";
  const cursorPosition = cursorSelectable?.position ?? -1;
  const cursorGone = state.cursor !== null && !rowByKey.has(state.cursor.key);
  // A drawn row with an action that holds the cursor; gone (-1) once its row is.
  const actionIndex = state.action ? rows.findIndex((row) => row.kind === "extra" && row.key === state.action && row.action) : -1;
  const actionRow = actionIndex >= 0 ? rows[actionIndex] : undefined;
  const activeAction = actionRow?.kind === "extra" ? actionRow : undefined;

  // Selected rows still in the list. If the selection disappeared together with the cursor
  // (deleted elsewhere), the row that took the cursor's place is selected instead.
  const selection = useMemo(() => {
    const none = { keys: [] as string[], items: [] as MessageListItem[], childIds: new Set<MessageId>() };
    if (state.selected.size === 0) return none;
    let picked = rows.flatMap((row, index) => {
      const key = rowKeys[index] ?? "";
      return isSelectable(row) && state.selected.has(key) ? [{ row, key }] : [];
    });
    if (picked.length === 0 && cursorGone && cursorSelectable && cursorRow !== undefined) {
      picked = [{ row: cursorSelectable, key: rowKeys[cursorRow] ?? "" }];
    }
    const asRows = new Set(picked.flatMap(({ row }) => (row.kind === "message" ? [row.item.id] : [])));
    const seen = new Set<MessageId>();
    const selectedItems: MessageListItem[] = [];
    const childIds = new Set<MessageId>();
    for (const { row } of picked) {
      if (row.kind === "child" && !asRows.has(row.item.id)) childIds.add(row.item.id);
      if (seen.has(row.item.id)) continue;
      seen.add(row.item.id);
      selectedItems.push(row.item);
    }
    return { keys: picked.map(({ key }) => key), items: selectedItems, childIds };
  }, [rows, rowKeys, state.selected, cursorGone, cursorSelectable, cursorRow]);
  const selectedItems = selection.items;
  const selectedKeys = useMemo(() => new Set(selection.keys), [selection.keys]);
  const multiSelect = selection.keys.length > 1;
  const hasMore = query.hasNextPage;

  const reportSelection = useEffectEvent((next: ListSelection) => {
    onSelectionChange?.(next);
  });
  useEffect(() => {
    reportSelection({
      cursor: cursorItem,
      cursorIsChild,
      items: selection.items,
      childIds: selection.childIds,
      keys: selection.keys,
      loaded: items.length,
      hasMore,
    });
  }, [cursorItem, cursorIsChild, selection, items.length, hasMore]);

  const rowSize = twoLine ? metrics.twoLineRow : metrics.row;
  const count = rows.length + (hasMore ? 1 : 0);

  const estimateSize = useCallback(
    (index: number) => {
      const row = rows[index];
      if (row?.kind === "section" || (row?.kind === "extra" && row.size === "section")) return metrics.section;
      // Stage 17: the messages of an unfolded conversation are one line in every width.
      if (row?.kind === "child" || (row?.kind === "extra" && row.size === "child")) return metrics.childRow;
      // Drawn rows of row size (bundles) take the narrow layout's height too: their chips wrap.
      return rowSize;
    },
    [rows, metrics.section, metrics.childRow, rowSize],
  );
  const getItemKey = useCallback((index: number) => rows[index]?.key ?? "more", [rows]);
  const rangeExtractor = useCallback(
    (range: Range) => {
      const indexes = defaultRangeExtractor(range);
      // The rows `aria-activedescendant` may point at stay rendered.
      const kept = [cursorRow, actionIndex >= 0 ? actionIndex : undefined].filter(
        (index): index is number => index !== undefined && !indexes.includes(index),
      );
      return kept.length === 0 ? indexes : [...indexes, ...kept].sort((a, b) => a - b);
    },
    [cursorRow, actionIndex],
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
  }, [virtualizer, rowSize, metrics.childRow]);

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

  /** Where a row is now; its remembered position if it is gone. */
  const positionOf = (cursor: Cursor): number => {
    const index = rowByKey.get(cursor.key);
    const row = index === undefined ? undefined : rows[index];
    return isSelectable(row) ? row.position : cursor.position;
  };

  /** Every row between the anchor and `to` (inclusive), by position. */
  const range = (from: Cursor | null, to: Cursor): Set<string> => {
    const start = from ? positionOf(from) : to.position;
    const [low, high] = start <= to.position ? [start, to.position] : [to.position, start];
    const keys = new Set<string>();
    for (let position = low; position <= high && keys.size < MAX_ACTION_MESSAGES; position += 1) {
      const index = selectableRows[position];
      const key = index === undefined ? undefined : rowKeys[index];
      if (key) keys.add(key);
    }
    return keys;
  };

  /** Selects one row like a click and scrolls it into view. */
  const selectRow = (index: number) => {
    const cursor = cursorAt(index);
    if (!cursor) return;
    setState(only(cursor));
    virtualizer.scrollToIndex(index, { align: "auto" });
  };

  /** Selects one message by id, like a click, and scrolls it into view; false if it is no row. */
  const selectById = (messageId: MessageId): boolean => {
    // A conversation's own row first, else a message of an unfolded conversation.
    let index = rows.findIndex((row) => row.kind === "message" && row.item.id === messageId);
    if (index < 0) index = rows.findIndex((row) => row.kind === "child" && row.item.id === messageId);
    if (index < 0) return false;
    selectRow(index);
    onSelectedById?.(messageId);
    return true;
  };
  useListSelectAnswer(selectScope, !query.isPending, !query.isFetching, selectById);

  // Stage 17: a message asked for in a conversation that is unfolding: selected once it is a row.
  const pendingIndex = pendingChild ? rowByKey.get(pendingChild.key) : undefined;
  const selectPending = useEffectEvent((index: number, id: MessageId) => {
    setPendingChild(null);
    selectRow(index);
    onSelectedById?.(id);
  });
  useEffect(() => {
    if (pendingChild && pendingIndex !== undefined) selectPending(pendingIndex, pendingChild.id);
  }, [pendingChild, pendingIndex]);

  /** Selects the rows of every loaded message (in a tree, the conversations), at most the limit. */
  const selectAllRows = useCallback(() => {
    const keys = rows.flatMap((row, index) => (row.kind === "message" ? [rowKeys[index] ?? ""] : [])).slice(0, MAX_ACTION_MESSAGES);
    const firstIndex = rows.findIndex((row) => row.kind === "message");
    const first = firstIndex >= 0 ? rows[firstIndex] : undefined;
    if (!first || first.kind !== "message" || keys.length === 0) return;
    const firstCursor = { key: rowKeys[firstIndex] ?? "", id: first.item.id, position: first.position };
    setState((current) => ({
      cursor: current.cursor && keys.includes(current.cursor.key) ? current.cursor : firstCursor,
      selected: new Set(keys),
      anchor: firstCursor,
    }));
  }, [rows, rowKeys]);
  // After `loadAndSelectAll`: once the messages of the pages it loaded are in the list.
  const loadedIds = useMemo(() => new Set(items.map((item) => item.id)), [items]);
  const selectAllNow = useEffectEvent(() => {
    setSelectAllRequest(null);
    selectAllRows();
  });
  useEffect(() => {
    if (selectAllRequest && selectAllRequest.every((id) => loadedIds.has(id))) selectAllNow();
  }, [selectAllRequest, loadedIds]);

  /** Stage 17: unfolds or folds a conversation. Folding moves a cursor inside to its row. */
  const setUnfolded = (ident: string, open: boolean) => {
    setExpanded((current) => {
      if (current.has(ident) === open) return current;
      const next = new Set(current);
      if (open) next.add(ident);
      else next.delete(ident);
      return next;
    });
    if (open) return;
    const prefix = `${ident}/`;
    const parentIndex = conversationRow.get(ident);
    const parent = parentIndex === undefined ? undefined : rows[parentIndex];
    setState((current) => {
      const selected = new Set([...current.selected].filter((key) => !key.startsWith(prefix)));
      const inside = rows.some((row) => row.kind === "extra" && row.key === current.action && row.parent === ident);
      const action = inside ? null : (current.action ?? null);
      if (!current.cursor?.key.startsWith(prefix) || parent?.kind !== "message") {
        return selected.size === current.selected.size && !inside ? current : { ...current, selected, action };
      }
      const cursor = { key: ident, id: parent.item.id, position: parent.position };
      return { cursor, selected: selected.size > 0 ? selected : new Set([ident]), anchor: cursor, action: null };
    });
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
        // Rows that go: messages with one of the ids, and the messages of their conversations.
        const goingIdents = new Set<string>();
        const goes = rows.map((row, index) => {
          if (row.kind === "message" && leaving.has(row.item.id)) {
            goingIdents.add(rowKeys[index] ?? "");
            return true;
          }
          return false;
        });
        rows.forEach((row, index) => {
          if (row.kind === "child" && goingIdents.has(row.parent)) goes[index] = true;
        });
        const cursorLeaves = cursorRow !== undefined && goes[cursorRow] === true;
        const selectionLeaves =
          selection.keys.length > 0 && selection.keys.every((key) => goes[rowByKey.get(key) ?? -1] === true);
        if (!cursorLeaves && !selectionLeaves) return;
        const positions = selectableRows.flatMap((index, position) => (goes[index] ? [position] : []));
        if (positions.length === 0) return;
        const last = Math.max(...positions);
        const first = Math.min(...positions);
        let next: SelectableRow | undefined;
        for (let position = last + 1; position < selectableRows.length && !next; position += 1) {
          const index = selectableRows[position];
          if (index !== undefined && !goes[index]) next = selectableAt(position);
        }
        for (let position = first - 1; position >= 0 && !next; position -= 1) {
          const index = selectableRows[position];
          if (index !== undefined && !goes[index]) next = selectableAt(position);
        }
        if (!next) {
          setState(EMPTY_STATE);
          return;
        }
        // Its position once the leaving rows are gone (the fallback if it disappears too).
        const removedBefore = positions.filter((position) => position < (next?.position ?? 0)).length;
        setState(only({ ...cursorOf(next), position: next.position - removedBefore }));
      },
      toggleCursor: () => {
        const cursor = cursorSelectable;
        if (!cursor || activeAction || cursorRow === undefined) return;
        const key = rowKeys[cursorRow] ?? "";
        setState((current) => {
          const selected = new Set(selectedKeys);
          if (!selected.delete(key)) selected.add(key);
          const at = { key, id: cursor.item.id, position: cursorPosition };
          return { ...current, cursor: at, selected, anchor: at };
        });
      },
      selectAll: selectAllRows,
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
        // The rows of the new pages are built with a later render; select them then.
        setSelectAllRequest(
          pages
            .flatMap((page) => page.items)
            .filter((item) => !hiddenIds?.has(item.id))
            .map((item) => item.id),
        );
      },
      contains: (node) => Boolean(node && scrollRef.current?.contains(node)),
      selectById,
      showInConversation: (ident, messageId) => {
        setUnfolded(ident, true);
        setPendingChild({ key: childKey(ident, messageId), id: messageId });
      },
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
      const cursor = cursorAt(index);
      if (cursor) return cursor;
    }
    return null;
  };

  const moveTo = (position: number, direction: 1 | -1, extend: boolean) => {
    if (selectableRows.length === 0) return;
    const clamped = Math.max(0, Math.min(position, selectableRows.length - 1));
    const rowIndex = selectableRows[clamped];
    const row = selectableAt(clamped);
    if (rowIndex === undefined || !row) return;
    const cursor = { key: rowKeys[rowIndex] ?? "", id: row.item.id, position: clamped };
    setState((current) => (extend ? { cursor, selected: range(current.anchor ?? current.cursor, cursor), anchor: current.anchor ?? current.cursor } : only(cursor)));
    // Moving up onto the first row of a section shows the section label too.
    const target = direction < 0 && rows[rowIndex - 1]?.kind === "section" ? rowIndex - 1 : rowIndex;
    virtualizer.scrollToIndex(target, { align: "auto" });
  };

  /** Moves the cursor onto the option at `index`: a message is selected, a drawn row only focused. */
  const moveToEntry = (index: number, direction: 1 | -1) => {
    const row = rows[index];
    if (isSelectable(row)) {
      moveTo(row.position, direction, false);
      return;
    }
    if (row?.kind !== "extra") return;
    setState((current) => ({ ...current, action: row.key }));
    const target = direction < 0 && rows[index - 1]?.kind === "section" ? index - 1 : index;
    virtualizer.scrollToIndex(target, { align: "auto" });
  };

  const isMod = (event: MouseEvent | KeyboardEvent) => (platform === "macos" ? event.metaKey : event.ctrlKey);

  /**
   * Arrow keys and j/k step over every option, drawn rows with an action included: onto one the
   * cursor moves without changing the selection; onto a message it selects it as before. False:
   * no cursor yet, the message moves apply.
   */
  const step = (direction: 1 | -1): boolean => {
    const from = actionIndex >= 0 ? actionIndex : cursorRow;
    if (from === undefined) return false;
    const next = direction > 0 ? entryRows.find((index) => index > from) : [...entryRows].reverse().find((index) => index < from);
    if (next === undefined) return actionIndex >= 0;
    moveToEntry(next, direction);
    return true;
  };

  /**
   * Stage 17: → and ← in a conversation tree (WAI-ARIA tree view). → unfolds a folded
   * conversation or bundle, on an unfolded one it moves to the first row below it; ← folds an
   * unfolded one, on a row inside it moves to the conversation's (or the bundle's) row.
   */
  const treeKey = (direction: "in" | "out"): boolean => {
    const index = actionIndex >= 0 ? actionIndex : cursorRow;
    if (index === undefined) return false;
    const row = rows[index];
    if (!row) return false;
    const level = places?.get(index)?.level ?? 1;
    const ident = row.kind === "message" ? expandable(row) : null;
    const bundle = row.kind === "extra" && row.expanded !== undefined ? row : undefined;
    const open = ident !== null ? expanded.has(ident) : bundle?.expanded === true;
    if (direction === "in") {
      if (ident === null && !bundle) return true;
      if (!open) {
        if (ident !== null) setUnfolded(ident, true);
        else bundle?.action?.activate();
        return true;
      }
      const below = entryRows.find((entry) => entry > index);
      if (below !== undefined && (places?.get(below)?.level ?? 1) > level) moveToEntry(below, 1);
      return true;
    }
    if (open) {
      if (ident !== null) setUnfolded(ident, false);
      else bundle?.action?.activate();
      return true;
    }
    // Up to the row this one belongs to: the nearest tree item above it one level higher.
    if (level <= 1) return true;
    const parent = [...entryRows].reverse().find((entry) => entry < index && (places?.get(entry)?.level ?? 1) < level);
    if (parent !== undefined) moveToEntry(parent, -1);
    return true;
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const page = Math.max(1, Math.floor((scrollRef.current?.clientHeight ?? 0) / rowSize) - 1);
    const moves: Record<string, [number, 1 | -1]> = {
      ArrowDown: [cursorPosition + 1, 1],
      ArrowUp: [Math.max(0, cursorPosition - 1), -1],
      Home: [0, -1],
      End: [selectableRows.length - 1, 1],
      PageDown: [cursorPosition + page, 1],
      PageUp: [Math.max(0, cursorPosition - page), -1],
      // Stage 8: j/k like most mail apps (by character, so on every layout).
      j: [cursorPosition + 1, 1],
      k: [Math.max(0, cursorPosition - 1), -1],
    };
    if (event.altKey || event.metaKey || event.ctrlKey) return;
    if (activeAction?.action && (event.key === "Enter" || event.key === " ")) {
      // The window's shortcuts (Space selects) do not see it: the key belongs to this row.
      event.preventDefault();
      event.stopPropagation();
      activeAction.action.activate();
      return;
    }
    if (tree && !event.shiftKey && (event.key === "ArrowRight" || event.key === "ArrowLeft")) {
      if (treeKey(event.key === "ArrowRight" ? "in" : "out")) event.preventDefault();
      return;
    }
    if (!event.shiftKey && (event.key === "ArrowDown" || event.key === "j" || event.key === "ArrowUp" || event.key === "k")) {
      if (step(event.key === "ArrowDown" || event.key === "j" ? 1 : -1)) {
        event.preventDefault();
        return;
      }
    }
    if (event.key === "Enter" && cursorItem) {
      event.preventDefault();
      onOpen?.(cursorItem.id);
      return;
    }
    if (event.key === "Escape" && (state.cursor || state.selected.size > 0 || activeAction)) {
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

  const onRowClick = (event: MouseEvent, row: SelectableRow) => {
    pointerPress.current = false;
    // Stage 17: the chevron with the replies unfolds or folds; it selects nothing.
    if (row.kind === "message" && event.target instanceof Element && event.target.closest("[data-row-toggle]") !== null) {
      const ident = expandable(row);
      if (ident !== null) setUnfolded(ident, !expanded.has(ident));
      return;
    }
    const cursor = cursorOf(row);
    const onBox = event.target instanceof Element && event.target.closest("[data-row-check]") !== null;
    if (event.shiftKey) {
      setState((current) => ({ cursor, selected: range(current.anchor ?? current.cursor, cursor), anchor: current.anchor ?? current.cursor }));
    } else if (onBox || isMod(event)) {
      setState(() => {
        const selected = new Set(selectedKeys);
        if (!selected.delete(cursor.key)) selected.add(cursor.key);
        return { cursor, selected, anchor: cursor };
      });
    } else {
      setState(only(cursor));
    }
    scrollRef.current?.focus({ preventScroll: true });
  };

  const onRowDoubleClick = (event: MouseEvent, row: SelectableRow) => {
    if (event.target instanceof Element && event.target.closest("[data-row-toggle]") !== null) return;
    setState(only(cursorOf(row)));
    onOpen?.(row.item.id);
  };

  const onRowDragStart = (event: DragEvent, row: SelectableRow) => {
    if (!onDragRows) return;
    const key = cursorOf(row).key;
    const fromSelection = selectedKeys.has(key);
    const dragged = fromSelection ? selectedItems : [row.item];
    const childIds = fromSelection ? selection.childIds : new Set(row.kind === "child" ? [row.item.id] : []);
    const refs = dragged.map((item) => ({ mailboxId: item.mailboxId, messageId: item.id }));
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData(MESSAGE_DRAG_TYPE, JSON.stringify(refs));
    const image = dragImageRef.current;
    if (dragged.length > 1 && image) {
      // The drag image is a snapshot taken right now; the text is set before it is taken.
      image.textContent = t("list.dragCount", { count: dragged.length });
      event.dataTransfer.setDragImage(image, 0, 0);
    }
    onDragRows(dragged, childIds);
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
    if (account && isFirstSyncRunning(account.status)) {
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
    if (source?.empty) return <EmptyState icon={source.empty.icon} title={source.empty.title} body={source.empty.body} />;
    return <EmptyState icon={Inbox} title={t("list.empty.title")} body={t("list.empty.body")} />;
  }

  // Stage 9: a source's rows can hold messages that are no options (members of a closed bundle).
  // Options are messages and drawn rows with an action (only sources have those).
  const actionCount = entryRows.length - selectableRows.length;
  const total = hasMore
    ? Math.max(source?.total ?? mailbox?.total ?? 0, items.length) + actionCount
    : source
      ? entryRows.length
      : items.length;
  /** Stage 17: the tree attributes of a tree item; the first level counts what is not loaded yet. */
  const treeAttributes = (index: number) => {
    const place = places?.get(index);
    if (!place) return {};
    const setsize = place.level === 1 && hasMore ? Math.max(place.setsize, total) : place.setsize;
    return { "aria-level": place.level, "aria-posinset": place.posinset, "aria-setsize": setsize };
  };
  const childDomId = (key: string) => `${optionPrefix}-child-${key.replace("/", "-")}`;
  const activeId = activeAction
    ? actionDomId(optionPrefix, activeAction.key)
    : cursorSelectable?.kind === "child" && cursorRow !== undefined
      ? childDomId(rowKeys[cursorRow] ?? "")
      : cursorItem
        ? `${optionPrefix}-${String(cursorItem.id)}`
        : undefined;
  const role = tree ? "treeitem" : "option";

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
        role={tree ? "tree" : "listbox"}
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
        className={cx(
          "group/list min-h-0 flex-1 overflow-y-auto bg-surface-list px-2",
          // The cursor row shows the focus ring; without one (nothing loaded yet) the list does.
          activeId ? "outline-none" : "focus-visible:-outline-offset-2",
        )}
      >
        <ListLayoutContext value={twoLine}>
          <ListTreeContext value={tree}>
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
                const edge = cardEdge(rows, virtualItem.index);
                if (row.kind === "extra") {
                  const action = row.action;
                  const active = action !== undefined && virtualItem.index === actionIndex;
                  return (
                    <div
                      key={virtualItem.key}
                      {...common}
                      {...(action
                        ? {
                            role,
                            id: actionDomId(optionPrefix, row.key),
                            ...(tree
                              ? { ...treeAttributes(virtualItem.index), ...(row.expanded !== undefined && { "aria-expanded": row.expanded }) }
                              : { "aria-posinset": (entryPosition.get(virtualItem.index) ?? 0) + 1, "aria-setsize": total }),
                            onMouseDown: () => {
                              pointerPress.current = true;
                            },
                            onClick: () => {
                              pointerPress.current = false;
                              setState((current) => ({ ...current, action: row.key }));
                              scrollRef.current?.focus({ preventScroll: true });
                              action.activate();
                            },
                          }
                        : { role: "presentation" })}
                      data-testid={row.size === "child" ? "child-status" : "list-extra"}
                      data-card={edge && row.card}
                      data-card-edge={edge}
                      className={cx(
                        "group/row absolute top-0 left-0 w-full",
                        row.size === "section"
                          ? "h-(--ui-section-height)"
                          : row.size === "child"
                            ? "h-(--ui-child-row-height)"
                            : twoLine
                              ? "h-(--ui-row-height-two-line)"
                              : "h-(--ui-row-height)",
                        edge && "list-card px-1",
                        action && "cursor-pointer",
                      )}
                    >
                      <div
                        className={cx(
                          "h-full rounded-row",
                          active && "group-focus-visible/list:outline-2 group-focus-visible/list:-outline-offset-2 group-focus-visible/list:outline-focus-ring",
                        )}
                      >
                        {row.render()}
                      </div>
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
                      className="absolute top-0 left-0 flex h-(--ui-section-height) w-full items-end px-3 pb-1 text-12 font-semibold text-tertiary"
                    >
                      <SectionLabel section={row.section} bounds={bounds} />
                    </div>
                  );
                }
                const key = rowKeys[virtualItem.index] ?? "";
                const selected = selectedKeys.has(key);
                const isCursor = virtualItem.index === cursorRow && !activeAction;
                const pointer = {
                  draggable: onDragRows ? true : undefined,
                  onMouseDown: () => {
                    // The press focuses the list; its click selects this row, not the focus handler.
                    pointerPress.current = true;
                  },
                  onClick: (event: MouseEvent) => {
                    onRowClick(event, row);
                  },
                  onDoubleClick: (event: MouseEvent) => {
                    onRowDoubleClick(event, row);
                  },
                  onDragStart: (event: DragEvent) => {
                    onRowDragStart(event, row);
                  },
                  onDragEnd: () => {
                    pointerPress.current = false;
                    onDragEnd?.();
                  },
                };
                if (row.kind === "child") {
                  const optionId = childDomId(key);
                  return (
                    <ChildOption
                      key={virtualItem.key}
                      {...common}
                      {...pointer}
                      id={optionId}
                      row={row}
                      dateId={dateDomId(optionId)}
                      bounds={bounds}
                      twoLine={twoLine}
                      selected={selected}
                      cursor={isCursor}
                      multiSelect={multiSelect}
                      actions={renderQuickActions?.(row.item, selected, true)}
                      aria-selected={selected}
                      {...treeAttributes(virtualItem.index)}
                      data-testid="child-row"
                      data-card={edge && row.card}
                      data-card-edge={edge}
                      className={cx("group/row absolute top-0 left-0 h-(--ui-child-row-height) w-full", edge && "list-card px-1")}
                    />
                  );
                }
                const optionId = `${optionPrefix}-${String(row.item.id)}`;
                const accountName = source?.accountName?.(row.item);
                const decorated = source?.decorate?.(row.item);
                const decoration = accountName ? { ...decorated, account: accountName } : decorated;
                const ident = tree ? expandable(row) : null;
                const open = ident !== null && expanded.has(ident);
                return (
                  <MessageOption
                    key={virtualItem.key}
                    {...common}
                    {...pointer}
                    id={optionId}
                    role={role}
                    item={row.item}
                    showRecipients={showRecipients}
                    dateId={dateDomId(optionId)}
                    bounds={bounds}
                    decoration={decoration}
                    aria-selected={selected}
                    {...(tree
                      ? {
                          ...treeAttributes(virtualItem.index),
                          ...(ident !== null && { "aria-expanded": open }),
                          ...(open && childQueries[ident]?.status === "pending" && { "aria-busy": true }),
                        }
                      : { "aria-setsize": total, "aria-posinset": (entryPosition.get(virtualItem.index) ?? row.position) + 1 })}
                    data-testid="message-row"
                    data-card={edge && row.card}
                    data-card-edge={edge}
                    className={cx(
                      "group/row absolute top-0 left-0 w-full",
                      twoLine ? "h-(--ui-row-height-two-line)" : "h-(--ui-row-height)",
                      edge && "list-card px-1",
                    )}
                  >
                    <MessageRowContent
                      item={row.item}
                      accountColor={source?.accountColor?.(row.item) ?? account?.color ?? 0}
                      bounds={bounds}
                      showRecipients={showRecipients}
                      twoLine={twoLine}
                      selected={selected}
                      cursor={isCursor}
                      multiSelect={multiSelect}
                      decoration={decoration}
                      expanded={ident !== null ? open : undefined}
                      // In the priority card the card says "important" already.
                      omitMarks={row.card === "priority" ? PRIORITY_CARD_MARKS : undefined}
                      actions={renderQuickActions?.(row.item, selected, false)}
                    />
                  </MessageOption>
                );
              })}
            </div>
          </ListTreeContext>
        </ListLayoutContext>
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
