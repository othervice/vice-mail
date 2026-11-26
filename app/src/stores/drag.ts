import { create } from "zustand";
import type { Account, AccountId, Mailbox, MessageListItem } from "../services/ipc";

/** Data type of dragged list rows. Only for `types` checks: the session below holds the rows. */
export const MESSAGE_DRAG_TYPE = "application/x-mail-message-refs";

/**
 * Stage 22 (decision 0078): data type of an account heading dragged in the left column to put it
 * elsewhere. A type of its own, so folders never take an account and headings never take rows.
 */
export const ACCOUNT_DRAG_TYPE = "application/x-mail-account";

/** Stage 22 (decision 0081): dropping moves; holding Option (macOS) or Ctrl (Windows) copies. */
export type DropMode = "move" | "copy";

/** The roles the narrow rail takes rows for: each row goes to that folder of its own account. */
export type DropRole = "inbox" | "archive" | "trash";

/**
 * Where rows are dropped: a folder (the column's trees and favourites), or in the narrow rail a
 * role, which is the folder with that role in each row's own account.
 */
export type DropTarget = { kind: "folder"; mailbox: Mailbox } | { kind: "role"; role: DropRole };

/**
 * Rows dragged out of any message list (stage 22, decision 0081: a folder's list, the views, the
 * unified inbox, search results; rows of several folders and accounts at once). Folders in the
 * column, the favourites and the rail take them; `drop` runs it through the list that started the
 * drag (its hidden rows, focus and notice).
 */
export interface DragSession {
  /** The rows as dragged (conversation rows stand for their conversation until the drop). */
  items: readonly MessageListItem[];
  count: number;
  drop: (target: DropTarget, mode: DropMode) => void;
}

/**
 * Stage 23: where a dragged account goes, shown as a line: before `target`, or after it (only
 * after the last account of a list).
 */
export interface AccountPlace {
  target: AccountId;
  side: "before" | "after";
}

/** What a folder under the dragged rows says while they are over it (`DropHint`). */
export interface DropHover {
  /** Which drop target this is (only that target clears it again). */
  id: string;
  /** Its place on screen, for the hint next to it. */
  rect: { top: number; right: number; bottom: number; left: number };
  verdict: Exclude<DropVerdict, { kind: "none" }>;
  /** The folder's name as shown. */
  folder: string;
}

interface DragState {
  session: DragSession | null;
  hover: DropHover | null;
  /** Stage 22 (decision 0078): the account heading being dragged to another place. */
  account: AccountId | null;
  /** Stage 23: where it would go if let go now (the line), or null (no line, no drop). */
  accountPlace: AccountPlace | null;
  start: (session: DragSession) => void;
  end: () => void;
  setHover: (hover: DropHover) => void;
  clearHover: (id: string) => void;
  startAccount: (account: AccountId) => void;
  endAccount: () => void;
  setAccountPlace: (place: AccountPlace | null) => void;
}

/**
 * Stage 23: whether a drag event belongs to the drag this page started and the store holds (rows
 * or an account). The store, not the event's types, decides: WebKit may keep custom types hidden
 * until the drop. Files from outside never count.
 */
export function isOwnDrag(event: { dataTransfer: DataTransfer | null }, kind: "rows" | "account"): boolean {
  const { session, account } = useDragStore.getState();
  const own = kind === "rows" ? session !== null : account !== null;
  return own && !Array.from(event.dataTransfer?.types ?? []).includes("Files");
}

/**
 * Stage 23: a drag whose source vanished (rows that left the list, no `dragend`) must not stay in
 * the store: every new drag or press starts clean. Installed once per page, before the first
 * render; the listeners run before the elements' own (capture phase on the window).
 */
export function installDragReset(target: Window = window): void {
  const reset = () => {
    const state = useDragStore.getState();
    if (state.session) state.end();
    if (state.account) state.endAccount();
  };
  target.addEventListener("dragstart", reset, true);
  target.addEventListener("mousedown", reset, true);
}

/** Marks the elements that take a dragged account (the column's accounts, the rail's dots). */
export const ACCOUNT_DROP_ZONE = "data-account-drop-zone";

/**
 * While an account is dragged (stage 23): the line goes when the pointer leaves every list of
 * accounts (no `dragleave` needed, which WebKit reports without `relatedTarget`), and Escape
 * cancels the drag in the page as well (the system's drag ends on its own).
 */
function accountDragListeners(): { install: () => void; remove: () => void } {
  const over = (event: DragEvent) => {
    const target = event.target instanceof Element ? event.target : null;
    if (!target?.closest(`[${ACCOUNT_DROP_ZONE}]`)) useDragStore.getState().setAccountPlace(null);
  };
  const key = (event: KeyboardEvent) => {
    if (event.key === "Escape") useDragStore.getState().endAccount();
  };
  return {
    install: () => {
      window.addEventListener("dragover", over, true);
      window.addEventListener("keydown", key, true);
    },
    remove: () => {
      window.removeEventListener("dragover", over, true);
      window.removeEventListener("keydown", key, true);
    },
  };
}
const accountListeners = accountDragListeners();

export const useDragStore = create<DragState>()((set) => ({
  session: null,
  hover: null,
  account: null,
  accountPlace: null,
  startAccount: (account) => {
    accountListeners.remove();
    accountListeners.install();
    set({ account, accountPlace: null });
  },
  endAccount: () => {
    accountListeners.remove();
    set({ account: null, accountPlace: null });
  },
  setAccountPlace: (place) => {
    set((state) =>
      state.accountPlace?.target === place?.target && state.accountPlace?.side === place?.side ? state : { accountPlace: place },
    );
  },
  start: (session) => {
    set({ session, hover: null });
  },
  end: () => {
    set({ session: null, hover: null });
  },
  setHover: (hover) => {
    set({ hover });
  },
  clearHover: (id) => {
    set((state) => (state.hover?.id === id ? { hover: null } : state));
  },
}));

/**
 * Whether a drop of the session's rows on a target is possible (decision 0081):
 * - `accept` with the mode the modifier asks for;
 * - `refuse` with the reason the target shows while the rows are over it: the target's account
 *   may not write, an account a row comes from may not write (only a move; a copy leaves it as it
 *   is), or (rail) an account of a row has no folder with that role;
 * - `none`: not a target for these rows at all (not a real folder, or every row lies there).
 */
export type DropVerdict =
  | { kind: "accept"; mode: DropMode; count: number }
  | { kind: "refuse"; reason: "targetReadOnly" | "sourceReadOnly"; account: Account }
  | { kind: "refuse"; reason: "noRoleFolder"; account: Account; role: DropRole }
  | { kind: "none" };

export interface VerdictContext {
  accounts: readonly Account[];
  /** The folders of an account as cached (rail roles). */
  mailboxesOf: (accountId: AccountId) => readonly Mailbox[];
}

/** The folder with `role` in `accountId`, if the account has one. */
export function roleFolder(context: VerdictContext, accountId: AccountId, role: DropRole): Mailbox | undefined {
  return context.mailboxesOf(accountId).find((mailbox) => mailbox.role === role && mailbox.selectable);
}

export function dropVerdict(session: DragSession | null, target: DropTarget | undefined, mode: DropMode, context: VerdictContext): DropVerdict {
  if (!session || !target || session.items.length === 0) return { kind: "none" };
  const accountOf = (id: AccountId) => context.accounts.find((account) => account.id === id);
  if (target.kind === "folder") {
    const { mailbox } = target;
    if (!mailbox.selectable) return { kind: "none" };
    const moving = session.items.filter((item) => item.mailboxId !== mailbox.id);
    if (moving.length === 0) return { kind: "none" };
    const owner = accountOf(mailbox.accountId);
    if (!owner) return { kind: "none" };
    if (!owner.writesEnabled) return { kind: "refuse", reason: "targetReadOnly", account: owner };
    if (mode === "move") {
      for (const item of moving) {
        const source = accountOf(item.accountId);
        if (source && !source.writesEnabled) return { kind: "refuse", reason: "sourceReadOnly", account: source };
      }
    }
    return { kind: "accept", mode, count: moving.length };
  }
  let count = 0;
  const seen = new Set<AccountId>();
  for (const item of session.items) {
    const account = accountOf(item.accountId);
    if (!account) continue;
    const folder = roleFolder(context, item.accountId, target.role);
    if (!seen.has(account.id)) {
      seen.add(account.id);
      if (!folder) return { kind: "refuse", reason: "noRoleFolder", account, role: target.role };
    }
    if (folder && item.mailboxId === folder.id) continue;
    if (!account.writesEnabled) return { kind: "refuse", reason: "targetReadOnly", account };
    count += 1;
  }
  return count === 0 ? { kind: "none" } : { kind: "accept", mode, count };
}

/** Whether the pointer's modifier asks for a copy: Option on macOS, Ctrl on Windows. */
export function dropModeOf(event: { altKey: boolean; ctrlKey: boolean }, platform: "macos" | "windows"): DropMode {
  return (platform === "macos" ? event.altKey : event.ctrlKey) ? "copy" : "move";
}
