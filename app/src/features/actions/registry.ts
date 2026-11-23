import {
  Archive,
  SquareCheck,
  Flag,
  FlagOff,
  FolderInput,
  ListChecks,
  Mail,
  MailOpen,
  ShieldAlert,
  ShieldCheck,
  Tags,
  Trash2,
  Undo2,
  type LucideIcon,
} from "lucide-react";
import type { Account, ActionKind, MailAction, Mailbox, MailboxRole, MessageListItem } from "../../services/ipc";
import type { Shortcut, ShortcutPlatform } from "./shortcuts";

/**
 * The action directory (Aktionsverzeichnis, docs/stages/08): every mail action the UI offers,
 * once. Quick actions in the rows, the toolbar of the reading pane, the selection bar and the
 * keyboard shortcuts all read it; stage 8 adds the command palette and configurable shortcuts on
 * top of it instead of building them a second time.
 */
export const ACTION_IDS = [
  "archive",
  "delete",
  "deletePermanently",
  "toggleRead",
  "markRead",
  "markUnread",
  "toggleFlag",
  "flag",
  "unflag",
  "move",
  "tags",
  "spam",
  "notSpam",
  "undo",
  "toggleSelect",
  "selectAll",
] as const;

export type ActionId = (typeof ACTION_IDS)[number];

/** Labels in `actions.labels` (i18n). Toggles take the label of what they would do now. */
export type ActionLabel = Exclude<ActionId, "toggleRead" | "toggleFlag">;

/** What an action looks at: the account and folder of the view and the messages it acts on. */
export interface ActionContext {
  account: Account;
  mailbox: Mailbox;
  /** The messages the action is for (selection, or the row of a quick action), in list order. */
  items: readonly MessageListItem[];
  /** An earlier action of this session can still be undone. */
  canUndo: boolean;
}

/** What actions call to do their work; the view's action controller implements it. */
export interface ActionHost {
  apply: (action: MailAction, items: readonly MessageListItem[]) => void;
  confirmDeletePermanently: (items: readonly MessageListItem[]) => void;
  openMove: (items: readonly MessageListItem[]) => void;
  openTags: (items: readonly MessageListItem[]) => void;
  undo: () => void;
  toggleSelect: () => void;
  selectAll: () => void;
}

export interface ActionDefinition {
  id: ActionId;
  label: (ctx: ActionContext) => ActionLabel;
  icon: (ctx: ActionContext) => LucideIcon;
  /** Provisional shortcuts per platform; the first one is shown in tooltips. */
  shortcuts: Record<ShortcutPlatform, readonly Shortcut[]>;
  /** Only when the focus is in the message list (Space, Cmd+A would mean something else elsewhere). */
  listOnly?: boolean;
  /** Changes mail on the server: shown but disabled, with the reason, while writes are off. */
  writes: boolean;
  /** Acts on messages: nothing to do without any. */
  needsMessages: boolean;
  /** Makes sense here at all (e.g. "Delete permanently" only in the trash). */
  available: (ctx: ActionContext) => boolean;
  run: (ctx: ActionContext, host: ActionHost) => void;
}

const both = (...shortcuts: Shortcut[]): Record<ShortcutPlatform, readonly Shortcut[]> => ({
  macos: shortcuts,
  windows: shortcuts,
});

const role = (ctx: ActionContext): MailboxRole => ctx.mailbox.role;
const someUnread = (ctx: ActionContext) => ctx.items.some((item) => item.unread);
const someRead = (ctx: ActionContext) => ctx.items.some((item) => !item.unread);
const someFlagged = (ctx: ActionContext) => ctx.items.some((item) => item.flagged);
const someUnflagged = (ctx: ActionContext) => ctx.items.some((item) => !item.flagged);
const always = () => true;

/** Delete: "#" everywhere; the Delete key, and on macOS also ⌫ (the key labelled "delete"). */
const DELETE_SHORTCUTS: Record<ShortcutPlatform, readonly Shortcut[]> = {
  macos: [{ key: "#" }, { key: "Backspace" }, { key: "Delete" }],
  windows: [{ key: "#" }, { key: "Delete" }],
};

export const ACTIONS: Record<ActionId, ActionDefinition> = {
  archive: {
    id: "archive",
    label: () => "archive",
    icon: () => Archive,
    shortcuts: both({ key: "e" }),
    writes: true,
    needsMessages: true,
    available: (ctx) => role(ctx) !== "archive",
    run: (ctx, host) => {
      host.apply({ type: "archive" }, ctx.items);
    },
  },
  delete: {
    id: "delete",
    label: () => "delete",
    icon: () => Trash2,
    shortcuts: DELETE_SHORTCUTS,
    writes: true,
    needsMessages: true,
    available: (ctx) => role(ctx) !== "trash",
    run: (ctx, host) => {
      host.apply({ type: "delete" }, ctx.items);
    },
  },
  // In the trash, deleting is for good: the one action that asks first (docs/DESIGN.md).
  deletePermanently: {
    id: "deletePermanently",
    label: () => "deletePermanently",
    icon: () => Trash2,
    shortcuts: DELETE_SHORTCUTS,
    writes: true,
    needsMessages: true,
    available: (ctx) => role(ctx) === "trash",
    run: (ctx, host) => {
      host.confirmDeletePermanently(ctx.items);
    },
  },
  toggleRead: {
    id: "toggleRead",
    label: (ctx) => (someUnread(ctx) ? "markRead" : "markUnread"),
    icon: (ctx) => (someUnread(ctx) ? MailOpen : Mail),
    shortcuts: both(),
    writes: true,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      host.apply({ type: someUnread(ctx) ? "markRead" : "markUnread" }, ctx.items);
    },
  },
  markRead: {
    id: "markRead",
    label: () => "markRead",
    icon: () => MailOpen,
    shortcuts: both({ key: "I", shift: true }),
    writes: true,
    needsMessages: true,
    available: someUnread,
    run: (ctx, host) => {
      host.apply({ type: "markRead" }, ctx.items);
    },
  },
  markUnread: {
    id: "markUnread",
    label: () => "markUnread",
    icon: () => Mail,
    shortcuts: both({ key: "U", shift: true }),
    writes: true,
    needsMessages: true,
    available: someRead,
    run: (ctx, host) => {
      host.apply({ type: "markUnread" }, ctx.items);
    },
  },
  toggleFlag: {
    id: "toggleFlag",
    label: (ctx) => (someUnflagged(ctx) ? "flag" : "unflag"),
    icon: (ctx) => (someUnflagged(ctx) ? Flag : FlagOff),
    shortcuts: both({ key: "s" }),
    writes: true,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      host.apply({ type: someUnflagged(ctx) ? "flag" : "unflag" }, ctx.items);
    },
  },
  flag: {
    id: "flag",
    label: () => "flag",
    icon: () => Flag,
    shortcuts: both(),
    writes: true,
    needsMessages: true,
    available: someUnflagged,
    run: (ctx, host) => {
      host.apply({ type: "flag" }, ctx.items);
    },
  },
  unflag: {
    id: "unflag",
    label: () => "unflag",
    icon: () => FlagOff,
    shortcuts: both(),
    writes: true,
    needsMessages: true,
    available: someFlagged,
    run: (ctx, host) => {
      host.apply({ type: "unflag" }, ctx.items);
    },
  },
  move: {
    id: "move",
    label: () => "move",
    icon: () => FolderInput,
    shortcuts: both({ key: "v" }),
    writes: true,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      host.openMove(ctx.items);
    },
  },
  tags: {
    id: "tags",
    label: () => "tags",
    icon: () => Tags,
    shortcuts: both({ key: "l" }),
    writes: true,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      host.openTags(ctx.items);
    },
  },
  spam: {
    id: "spam",
    label: () => "spam",
    icon: () => ShieldAlert,
    shortcuts: both({ key: "!" }),
    writes: true,
    needsMessages: true,
    available: (ctx) => role(ctx) !== "junk",
    run: (ctx, host) => {
      host.apply({ type: "spam" }, ctx.items);
    },
  },
  notSpam: {
    id: "notSpam",
    label: () => "notSpam",
    icon: () => ShieldCheck,
    shortcuts: both({ key: "!" }),
    writes: true,
    needsMessages: true,
    available: (ctx) => role(ctx) === "junk",
    run: (ctx, host) => {
      host.apply({ type: "notSpam" }, ctx.items);
    },
  },
  undo: {
    id: "undo",
    label: () => "undo",
    icon: () => Undo2,
    shortcuts: { macos: [{ key: "z" }, { key: "z", mod: true }], windows: [{ key: "z" }, { key: "z", mod: true }] },
    writes: true,
    needsMessages: false,
    available: (ctx) => ctx.canUndo,
    run: (_ctx, host) => {
      host.undo();
    },
  },
  toggleSelect: {
    id: "toggleSelect",
    label: () => "toggleSelect",
    icon: () => SquareCheck,
    shortcuts: both({ key: "x" }, { key: " " }),
    listOnly: true,
    writes: false,
    needsMessages: false,
    available: always,
    run: (_ctx, host) => {
      host.toggleSelect();
    },
  },
  selectAll: {
    id: "selectAll",
    label: () => "selectAll",
    icon: () => ListChecks,
    shortcuts: both({ key: "a", mod: true }),
    listOnly: true,
    writes: false,
    needsMessages: false,
    available: always,
    run: (_ctx, host) => {
      host.selectAll();
    },
  },
};

/** Quick actions of a row, in the order of docs/DESIGN.md (pin and snooze follow in stage 9). */
export const QUICK_ACTIONS: readonly ActionId[] = ["delete", "deletePermanently", "archive", "toggleRead", "toggleFlag", "move"];

/** The toolbar of the reading pane and of the selection bar. */
export const TOOLBAR_ACTIONS: readonly ActionId[] = [
  "archive",
  "delete",
  "deletePermanently",
  "toggleRead",
  "toggleFlag",
  "move",
  "tags",
  "spam",
  "notSpam",
];

export type ActionState = { kind: "enabled" } | { kind: "readOnly" } | { kind: "unavailable" };

/** Whether an action can run here, and if not, why (shown next to the disabled control). */
export function actionState(definition: ActionDefinition, ctx: ActionContext): ActionState {
  if (definition.needsMessages && ctx.items.length === 0) return { kind: "unavailable" };
  if (!definition.available(ctx)) return { kind: "unavailable" };
  if (definition.writes && !ctx.account.writesEnabled) return { kind: "readOnly" };
  return { kind: "enabled" };
}

/** The `ActionKind` of an action, for texts that name what was done. */
export function actionKind(action: MailAction): ActionKind {
  switch (action.type) {
    case "markRead":
      return "mark_read";
    case "markUnread":
      return "mark_unread";
    case "flag":
      return "flag";
    case "unflag":
      return "unflag";
    case "addKeyword":
      return "add_keyword";
    case "removeKeyword":
      return "remove_keyword";
    case "archive":
      return "archive";
    case "delete":
      return "delete";
    case "deletePermanently":
      return "delete_permanently";
    case "move":
      return "move";
    case "spam":
      return "spam";
    case "notSpam":
      return "not_spam";
  }
}

/** The folder role an action needs (`missing_mailbox` names none: the UI knows it from here). */
export function requiredRole(action: MailAction): MailboxRole | null {
  switch (action.type) {
    case "archive":
      return "archive";
    case "delete":
      return "trash";
    case "spam":
      return "junk";
    case "notSpam":
      return "inbox";
    default:
      return null;
  }
}

/** Whether the rows leave the folder the view shows, so the list hides them at once. */
export function leavesFolder(action: MailAction, mailbox: Mailbox): boolean {
  switch (action.type) {
    case "archive":
    case "delete":
    case "deletePermanently":
    case "spam":
      return true;
    case "move":
      return action.target !== mailbox.id;
    case "notSpam":
      return mailbox.role === "junk";
    default:
      return false;
  }
}
