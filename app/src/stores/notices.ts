import { create } from "zustand";
import type {
  AccountId,
  ActionKind,
  BatchId,
  ConflictReason,
  IpcError,
  MessageId,
  OperationInfo,
  RuleSkipReason,
  SpamUndo,
} from "../services/ipc";

/** What an undo notice can take back: the batches of one user action (several if it was split). */
export interface UndoTarget {
  accountId: AccountId;
  batches: readonly BatchId[];
}

/**
 * What a notice says. Stored as data, not as text: the notices component translates it, so the
 * core event handler (which has no `t`) can raise notices too.
 */
export type NoticeContent =
  /** An action was recorded. `undo` is null when there is nothing to undo (permanent delete). */
  | {
      kind: "done";
      action: ActionKind;
      count: number;
      /** Folder name for "Moved to …", tag for "Tag … added". */
      detail?: string;
      undo: UndoTarget | null;
    }
  | { kind: "undone" }
  | { kind: "notUndoable" }
  /** Writes are off for the account: why nothing happened, and where to change it. */
  | { kind: "readOnly"; accountId: AccountId }
  /** The list was out of date (`not_found`) and was reloaded. */
  | { kind: "outdated" }
  /** A command of the core failed right away. */
  | {
      kind: "error";
      context: "action" | "undo" | "markRead" | "window" | "emlFile" | "aiPanel" | "folder" | "transfer" | "appearance";
      error: IpcError;
    }
  /** Operations of one of the user's own batches failed on the server or met a conflict. */
  | {
      kind: "operationFailed";
      action: ActionKind;
      count: number;
      error: IpcError | null;
      conflict: ConflictReason | null;
    }
  /** Stage 16: a copy or move into another account was recorded (its progress is in Activity). */
  | { kind: "transfer"; mode: "copy" | "move"; count: number; folder: string; account: string }
  /** Stage 9: pins, snoozes, tasks and skipped local rules (texts: `features/comfort/notices.ts`). */
  | { kind: "comfort"; notice: ComfortNotice };

/** Stage 9 notices, as data like the others. `messageIds` lets the undo take a mark back. */
export type ComfortNotice =
  | { type: "pinned" | "unpinned" | "unsnoozed" | "taskAdded" | "taskRemoved" | "taskDone" | "taskReopened"; count: number }
  | { type: "snoozed"; count: number; until: number; messageIds: readonly MessageId[] }
  | { type: "failed"; what: "pin" | "snooze" | "unsnooze" | "task" | "readingList" | "block"; error: IpcError }
  /** Stage 16: the reading list, and senders blocked by the user (undo unblocks them). */
  | { type: "readingListAdded" | "readingListRemoved"; count: number }
  | { type: "sendersBlocked"; addresses: readonly string[] }
  /**
   * Stage 17 (decision 0050): a contact marked as spam: `moved` messages went to junk, `kept`
   * stayed (read-only accounts). Undo unblocks the addresses and moves the mail back.
   */
  | { type: "contactSpam"; email: string; name: string; undo: SpamUndo; moved: number; kept: number }
  | { type: "contactNotSpam"; name: string }
  | { type: "ruleSkipped"; ruleName: string; reason: RuleSkipReason; count: number };

export interface Notice {
  id: number;
  content: NoticeContent;
}

/** At most this many notices at once; the oldest goes first. */
const MAX_NOTICES = 3;
/** Batches remembered as the user's own, for failure notices of `operationsChanged`. */
const MAX_OWN_BATCHES = 100;

interface NoticeState {
  notices: readonly Notice[];
  /** The last action that can be undone (shortcut Z), also after its notice is gone. */
  lastUndo: UndoTarget | null;
  /** Batches the user started in this session, with the states already reported for them. */
  ownBatches: ReadonlyMap<BatchId, ReadonlySet<"failed" | "conflict">>;
  push: (content: NoticeContent) => number;
  dismiss: (id: number) => void;
  /** Remembers the batches of an action the user started, so failures are reported. */
  remember: (batches: readonly BatchId[]) => void;
  clearUndo: (target: UndoTarget) => void;
}

let nextId = 1;

export const useNoticeStore = create<NoticeState>()((set) => ({
  notices: [],
  lastUndo: null,
  ownBatches: new Map(),
  push: (content) => {
    const id = nextId++;
    set((state) => {
      // A new undo notice replaces the previous one: only the last action is undone by Z.
      const kept = content.kind === "done" ? state.notices.filter((n) => n.content.kind !== "done") : state.notices;
      return {
        notices: [...kept, { id, content }].slice(-MAX_NOTICES),
        ...(content.kind === "done" && content.undo && { lastUndo: content.undo }),
      };
    });
    return id;
  },
  dismiss: (id) => {
    set((state) => ({ notices: state.notices.filter((n) => n.id !== id) }));
  },
  remember: (batches) => {
    if (batches.length === 0) return;
    set((state) => {
      const next = new Map(state.ownBatches);
      for (const batch of batches) next.set(batch, next.get(batch) ?? new Set());
      while (next.size > MAX_OWN_BATCHES) {
        const oldest = next.keys().next().value;
        if (oldest === undefined) break;
        next.delete(oldest);
      }
      return { ownBatches: next };
    });
  },
  clearUndo: (target) => {
    set((state) => ({
      lastUndo: state.lastUndo === target ? null : state.lastUndo,
      // Its notice goes too: the action was undone or cannot be any more.
      notices: state.notices.filter((n) => !(n.content.kind === "done" && n.content.undo === target)),
    }));
  },
}));

/**
 * `operationsChanged`: a notice for operations of the user's own batches that ended `failed` or
 * `conflict`, once per batch and state (an action on many messages fails in one notice).
 */
export function reportOwnFailures(operations: readonly OperationInfo[]): void {
  const store = useNoticeStore.getState();
  const groups = new Map<string, OperationInfo[]>();
  for (const operation of operations) {
    if (operation.state !== "failed" && operation.state !== "conflict") continue;
    const reported = store.ownBatches.get(operation.batch);
    if (!reported || reported.has(operation.state)) continue;
    const key = `${String(operation.batch)}:${operation.state}`;
    groups.set(key, [...(groups.get(key) ?? []), operation]);
  }
  if (groups.size === 0) return;
  const next = new Map(store.ownBatches);
  for (const group of groups.values()) {
    const first = group[0];
    if (!first) continue;
    const state = first.state as "failed" | "conflict";
    next.set(first.batch, new Set([...(next.get(first.batch) ?? []), state]));
    store.push({
      kind: "operationFailed",
      action: first.action,
      count: group.length,
      error: state === "failed" ? first.error : null,
      conflict: state === "conflict" ? first.conflict : null,
    });
  }
  useNoticeStore.setState({ ownBatches: next });
}
