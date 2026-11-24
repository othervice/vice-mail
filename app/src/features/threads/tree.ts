import type { ReactNode } from "react";
import type { MessageId, MessageListItem } from "../../services/ipc";
import type { RowDecoration } from "../messages/MessageRow";
import type { ListRow } from "../messages/rows";

/**
 * Stage 17 (decision 0049): conversation lists as a tree, like Thunderbird. A conversation row
 * with more than one message unfolds in place; its messages follow it one level deeper, oldest
 * first, from every folder of its account. Pure, tested without React.
 */

/**
 * The identity of a conversation row: its thread, else its message. Stable while the row's
 * message changes (a reply arrives and becomes the newest), so an unfolded conversation stays
 * unfolded and a selected one selected through reloads, new mail and a new order.
 */
export function conversationIdent(item: MessageListItem, decoration: RowDecoration | undefined): string {
  const thread = decoration?.threadId;
  return thread === null || thread === undefined ? `m${String(item.id)}` : `t${String(thread)}`;
}

/** Replies of a conversation: its messages but the first. */
export function repliesOf(decoration: RowDecoration | undefined): number {
  return Math.max(0, (decoration?.threadCount ?? 1) - 1);
}

/** The selection key of a child row: its conversation's identity and its message. */
export function childKey(parent: string, id: MessageId): string {
  return `${parent}/${String(id)}`;
}

/** What is known about the messages of one unfolded conversation. */
export type ChildrenState =
  | { kind: "loading" }
  | { kind: "error" }
  | {
      kind: "loaded";
      /** Oldest first. */
      messages: readonly MessageListItem[];
      /** Messages in the conversation; more than `messages` when the core cut it to the newest. */
      total: number;
    };

/** Rows of an unfolded conversation that are not messages: loading, failed, cut. */
export type ChildStatus = { kind: "loading" } | { kind: "error" } | { kind: "truncated"; shown: number; total: number };

export interface ChildrenOptions {
  /** The identity of a message row's conversation when it can unfold (more than one message), else null. */
  expandable: (row: Extract<ListRow, { kind: "message" }>) => string | null;
  expanded: ReadonlySet<string>;
  children: (ident: string, row: Extract<ListRow, { kind: "message" }>) => ChildrenState;
  /** Messages an action is moving out right now. */
  hidden?: ReadonlySet<MessageId> | undefined;
  /** Draws a status row; `retry` for a failed load. */
  status: (status: ChildStatus, parent: Extract<ListRow, { kind: "message" }>) => { render: () => ReactNode; activate?: () => void };
}

/**
 * The rows with every unfolded conversation's messages after it, one level deeper and in the
 * conversation's card (priority, bundle). `position` counts messages and child rows, in order:
 * the keyboard and Shift ranges move over both.
 */
export function withChildren(rows: readonly ListRow[], options: ChildrenOptions): ListRow[] {
  const out: ListRow[] = [];
  let position = 0;
  for (const row of rows) {
    if (row.kind !== "message") {
      out.push(row);
      continue;
    }
    out.push({ ...row, position });
    position += 1;
    const ident = options.expandable(row);
    if (ident === null || !options.expanded.has(ident)) continue;
    const level = (row.level ?? 1) + 1;
    const card = row.card ? { card: row.card } : {};
    const state = options.children(ident, row);
    const statusRow = (status: ChildStatus, suffix: string): ListRow => {
      const drawn = options.status(status, row);
      return {
        kind: "extra",
        key: `child-${ident}-${suffix}`,
        size: "child",
        level,
        parent: ident,
        render: drawn.render,
        ...(drawn.activate && { action: { activate: drawn.activate } }),
        ...card,
      };
    };
    if (state.kind !== "loaded") {
      out.push(statusRow(state, state.kind));
      continue;
    }
    const messages = state.messages.filter((message) => !options.hidden?.has(message.id));
    if (state.total > state.messages.length) {
      out.push(statusRow({ kind: "truncated", shown: state.messages.length, total: state.total }, "truncated"));
    }
    messages.forEach((item, index) => {
      out.push({
        kind: "child",
        key: `child-${ident}-${String(item.id)}`,
        item,
        parent: ident,
        parentItem: row.item,
        position,
        level,
        last: index === messages.length - 1,
        ...card,
      });
      position += 1;
    });
  }
  return out;
}

/** Where a tree item stands among its siblings (`aria-level`, `aria-posinset`, `aria-setsize`). */
export interface TreePlace {
  level: number;
  posinset: number;
  setsize: number;
}

/** The level of a row in the tree; null for rows that are no tree items (headings, decoration). */
export function levelOf(row: ListRow): number | null {
  switch (row.kind) {
    case "section":
      return null;
    case "message":
      return row.level ?? 1;
    case "child":
      return row.level;
    case "extra":
      return row.action ? (row.level ?? 1) : null;
  }
}

/**
 * The place of every tree item by row index. Siblings are the items of one level under the same
 * parent: an item one level deeper starts a new group below the item before it. Rows that are no
 * tree items (date headings, loading rows) do not break a group.
 */
export function treePlaces(rows: readonly ListRow[]): Map<number, TreePlace> {
  const places = new Map<number, TreePlace>();
  /** The open groups, outermost first: their level and members (row indexes). */
  const stack: { level: number; members: number[] }[] = [];
  const close = (group: { level: number; members: number[] }) => {
    group.members.forEach((index, i) => {
      places.set(index, { level: group.level, posinset: i + 1, setsize: group.members.length });
    });
  };
  rows.forEach((row, index) => {
    const level = levelOf(row);
    if (level === null) return;
    while (stack.length > 0 && (stack[stack.length - 1]?.level ?? 0) > level) {
      const group = stack.pop();
      if (group) close(group);
    }
    const top = stack[stack.length - 1];
    if (top && top.level === level) top.members.push(index);
    else stack.push({ level, members: [index] });
  });
  while (stack.length > 0) {
    const group = stack.pop();
    if (group) close(group);
  }
  return places;
}
