import {
  AlarmClockOff,
  Archive,
  Clock,
  ListTodo,
  ListX,
  Pin,
  PinOff,
  RotateCcw,
  SquareCheck,
  SquareCheckBig,
  Flag,
  FlagOff,
  FolderInput,
  ListChecks,
  Search,
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
import { Command, FilePen, Forward, Keyboard, Paperclip, Reply, ReplyAll, SquarePen } from "lucide-react";
import { AppWindow, PanelLeft } from "lucide-react";
// Stage 16: the reading list and blocked senders.
import { BookOpen, BookX, UserX } from "lucide-react";
// Stage 19: source, redirect, edit as new.
import { CopyPlus, CornerUpRight, FileCode } from "lucide-react";
import { openComposer } from "../../stores/composer";
import { useRedirectStore } from "../../stores/redirect";
import { openMessageInWindow, openSourceInWindow } from "../windows/open";
import type { MailViewId } from "../../stores/ui";
import { MAIL_VIEW_ICONS } from "../mail/views";

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
  // Stage 9: local marks (no message moves on the server).
  "togglePin",
  "pin",
  "unpin",
  "snooze",
  "unsnooze",
  "toggleTask",
  "addTask",
  "removeTask",
  "toggleTaskDone",
  "completeTask",
  "reopenTask",
  // Stage 5: writing. Drafts are local, so these also work on read-only accounts.
  "reply",
  "replyAll",
  "forward",
  // Stage 10: the whole message as an `.eml` attachment.
  "forwardAsAttachment",
  "editDraft",
  "compose",
  // Stage 8: the message in a window of its own.
  "openInWindow",
  // Stage 16: the reading list (local, never on the server) and blocking a sender.
  "toggleReadingList",
  "addToReadingList",
  "removeFromReadingList",
  "blockSender",
  // Stage 19 (decision 0055): the source in a window of its own, redirecting, editing as new.
  "viewSource",
  "redirect",
  "editAsNew",
] as const;

export type ActionId = (typeof ACTION_IDS)[number];

/** Labels in `actions.labels` (i18n). Toggles take the label of what they would do now. */
export type ActionLabel = Exclude<
  ActionId,
  "toggleRead" | "toggleFlag" | "togglePin" | "toggleTask" | "toggleTaskDone" | "toggleReadingList"
>;

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
  /** Stage 9: pins, snoozes and tasks are local (keywords follow through the outbox where allowed). */
  setPinned: (items: readonly MessageListItem[], pinned: boolean) => void;
  openSnooze: (items: readonly MessageListItem[]) => void;
  unsnooze: (items: readonly MessageListItem[]) => void;
  setTask: (items: readonly MessageListItem[], mark: { done: boolean } | null) => void;
  /** Stage 16: the reading list (local only) and blocking the senders of these messages. */
  setReadingList: (items: readonly MessageListItem[], on: boolean) => void;
  blockSenders: (items: readonly MessageListItem[]) => void;
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
const someUnpinned = (ctx: ActionContext) => ctx.items.some((item) => !item.pinned);
const somePinned = (ctx: ActionContext) => ctx.items.some((item) => item.pinned);
const someSnoozed = (ctx: ActionContext) => ctx.items.some((item) => Boolean(item.snooze));
const someNoTask = (ctx: ActionContext) => ctx.items.some((item) => !item.task);
const someTask = (ctx: ActionContext) => ctx.items.some((item) => Boolean(item.task));
const someOpenTask = (ctx: ActionContext) => ctx.items.some((item) => item.task?.done === false);
const someDoneTask = (ctx: ActionContext) => ctx.items.some((item) => item.task?.done === true);
const someNotOnReadingList = (ctx: ActionContext) => ctx.items.some((item) => !item.readingList);
const someOnReadingList = (ctx: ActionContext) => ctx.items.some((item) => Boolean(item.readingList));
const someSender = (ctx: ActionContext) => ctx.items.some((item) => Boolean(item.from?.email) && !item.draft);
const always = () => true;
const oneMail = (ctx: ActionContext) => ctx.items.length === 1;

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
  // ---- Stage 9: pins, snoozes and tasks. Local, so they also work on read-only accounts. ----
  togglePin: {
    id: "togglePin",
    label: (ctx) => (someUnpinned(ctx) ? "pin" : "unpin"),
    icon: (ctx) => (someUnpinned(ctx) ? Pin : PinOff),
    shortcuts: both({ key: "p" }),
    writes: false,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      host.setPinned(ctx.items, someUnpinned(ctx));
    },
  },
  pin: {
    id: "pin",
    label: () => "pin",
    icon: () => Pin,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someUnpinned,
    run: (ctx, host) => {
      host.setPinned(ctx.items, true);
    },
  },
  unpin: {
    id: "unpin",
    label: () => "unpin",
    icon: () => PinOff,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: somePinned,
    run: (ctx, host) => {
      host.setPinned(ctx.items, false);
    },
  },
  snooze: {
    id: "snooze",
    label: () => "snooze",
    icon: () => Clock,
    shortcuts: both({ key: "b" }),
    writes: false,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      host.openSnooze(ctx.items);
    },
  },
  unsnooze: {
    id: "unsnooze",
    label: () => "unsnooze",
    icon: () => AlarmClockOff,
    shortcuts: both({ key: "B", shift: true }),
    writes: false,
    needsMessages: true,
    available: someSnoozed,
    run: (ctx, host) => {
      host.unsnooze(ctx.items.filter((item) => item.snooze));
    },
  },
  toggleTask: {
    id: "toggleTask",
    label: (ctx) => (someNoTask(ctx) ? "addTask" : "removeTask"),
    icon: (ctx) => (someNoTask(ctx) ? ListTodo : ListX),
    shortcuts: both({ key: "t" }),
    writes: false,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      if (someNoTask(ctx)) host.setTask(ctx.items.filter((item) => !item.task), { done: false });
      else host.setTask(ctx.items, null);
    },
  },
  addTask: {
    id: "addTask",
    label: () => "addTask",
    icon: () => ListTodo,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someNoTask,
    run: (ctx, host) => {
      host.setTask(
        ctx.items.filter((item) => !item.task),
        { done: false },
      );
    },
  },
  removeTask: {
    id: "removeTask",
    label: () => "removeTask",
    icon: () => ListX,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someTask,
    run: (ctx, host) => {
      host.setTask(
        ctx.items.filter((item) => item.task),
        null,
      );
    },
  },
  toggleTaskDone: {
    id: "toggleTaskDone",
    label: (ctx) => (someOpenTask(ctx) ? "completeTask" : "reopenTask"),
    icon: (ctx) => (someOpenTask(ctx) ? SquareCheckBig : RotateCcw),
    shortcuts: both({ key: "T", shift: true }),
    writes: false,
    needsMessages: true,
    available: someTask,
    run: (ctx, host) => {
      const done = someOpenTask(ctx);
      host.setTask(
        ctx.items.filter((item) => item.task && item.task.done !== done),
        { done },
      );
    },
  },
  completeTask: {
    id: "completeTask",
    label: () => "completeTask",
    icon: () => SquareCheckBig,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someOpenTask,
    run: (ctx, host) => {
      host.setTask(
        ctx.items.filter((item) => item.task?.done === false),
        { done: true },
      );
    },
  },
  reopenTask: {
    id: "reopenTask",
    label: () => "reopenTask",
    icon: () => RotateCcw,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someDoneTask,
    run: (ctx, host) => {
      host.setTask(
        ctx.items.filter((item) => item.task?.done === true),
        { done: false },
      );
    },
  },
  // ---- Stage 5: reply, forward, drafts and new messages open the composer. ----
  reply: {
    id: "reply",
    label: () => "reply",
    icon: () => Reply,
    shortcuts: both({ key: "r" }),
    writes: false,
    needsMessages: true,
    available: (ctx) => oneMail(ctx) && !ctx.items[0]?.draft,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openComposer({ kind: "reply", messageId: item.id });
    },
  },
  replyAll: {
    id: "replyAll",
    label: () => "replyAll",
    icon: () => ReplyAll,
    shortcuts: both({ key: "a" }),
    writes: false,
    needsMessages: true,
    available: (ctx) => oneMail(ctx) && !ctx.items[0]?.draft,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openComposer({ kind: "reply_all", messageId: item.id });
    },
  },
  forward: {
    id: "forward",
    label: () => "forward",
    icon: () => Forward,
    shortcuts: both({ key: "f" }),
    writes: false,
    needsMessages: true,
    available: (ctx) => oneMail(ctx) && !ctx.items[0]?.draft,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openComposer({ kind: "forward", messageId: item.id });
    },
  },
  forwardAsAttachment: {
    id: "forwardAsAttachment",
    label: () => "forwardAsAttachment",
    icon: () => Paperclip,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: (ctx) => oneMail(ctx) && !ctx.items[0]?.draft,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openComposer({ kind: "forward_as_attachment", messageId: item.id });
    },
  },
  // A message of the Drafts folder (also one of another program) opens as a draft.
  editDraft: {
    id: "editDraft",
    label: () => "editDraft",
    icon: () => FilePen,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: (ctx) => oneMail(ctx) && Boolean(ctx.items[0]?.draft),
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openComposer({ kind: "message", messageId: item.id });
    },
  },
  compose: {
    id: "compose",
    label: () => "compose",
    icon: () => SquarePen,
    shortcuts: both({ key: "c" }, { key: "n", mod: true }),
    writes: false,
    needsMessages: false,
    available: always,
    run: (ctx) => {
      openComposer({ kind: "new", accountId: ctx.account.id });
    },
  },
  // ---- Stage 8: the reading pane in a window of its own (docs/SPEC.md). No default shortcut:
  // neither DESIGN nor the stage names one; it can be assigned in Settings → Keyboard. ----
  openInWindow: {
    id: "openInWindow",
    label: () => "openInWindow",
    icon: () => AppWindow,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: oneMail,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openMessageInWindow(ctx.account.id, item.id);
    },
  },
  // ---- Stage 16 (decision 0033): the reading list is local like pins and tasks, so these also
  // work on read-only accounts; blocking a sender is a local rule (the core moves future mail to
  // Junk where the account may write, else hides it). ----
  toggleReadingList: {
    id: "toggleReadingList",
    label: (ctx) => (someNotOnReadingList(ctx) ? "addToReadingList" : "removeFromReadingList"),
    icon: (ctx) => (someNotOnReadingList(ctx) ? BookOpen : BookX),
    shortcuts: both({ key: "q" }),
    writes: false,
    needsMessages: true,
    available: always,
    run: (ctx, host) => {
      if (someNotOnReadingList(ctx)) host.setReadingList(ctx.items.filter((item) => !item.readingList), true);
      else host.setReadingList(ctx.items, false);
    },
  },
  addToReadingList: {
    id: "addToReadingList",
    label: () => "addToReadingList",
    icon: () => BookOpen,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someNotOnReadingList,
    run: (ctx, host) => {
      host.setReadingList(
        ctx.items.filter((item) => !item.readingList),
        true,
      );
    },
  },
  removeFromReadingList: {
    id: "removeFromReadingList",
    label: () => "removeFromReadingList",
    icon: () => BookX,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someOnReadingList,
    run: (ctx, host) => {
      host.setReadingList(
        ctx.items.filter((item) => item.readingList),
        false,
      );
    },
  },
  blockSender: {
    id: "blockSender",
    label: () => "blockSender",
    icon: () => UserX,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: someSender,
    run: (ctx, host) => {
      host.blockSenders(ctx.items);
    },
  },
  // ---- Stage 19 (decision 0055). The source reads only; Cmd/Ctrl+U as in Thunderbird and most
  // mail apps ("U" alone marks unread with Shift, Cmd/Ctrl+U is free and no system key). ----
  viewSource: {
    id: "viewSource",
    label: () => "viewSource",
    icon: () => FileCode,
    shortcuts: both({ key: "u", mod: true }),
    writes: false,
    needsMessages: true,
    available: oneMail,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openSourceInWindow(ctx.account.id, item.id);
    },
  },
  // A send: only with writes on (the core refuses it otherwise, too).
  redirect: {
    id: "redirect",
    label: () => "redirect",
    icon: () => CornerUpRight,
    shortcuts: both(),
    writes: true,
    needsMessages: true,
    available: (ctx) => oneMail(ctx) && !ctx.items[0]?.draft,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) useRedirectStore.getState().open({ accountId: ctx.account.id, messageId: item.id });
    },
  },
  // A local draft, like reply and forward: also on read-only accounts.
  editAsNew: {
    id: "editAsNew",
    label: () => "editAsNew",
    icon: () => CopyPlus,
    shortcuts: both(),
    writes: false,
    needsMessages: true,
    available: oneMail,
    run: (ctx) => {
      const item = ctx.items[0];
      if (item) openComposer({ kind: "edit_as_new", messageId: item.id });
    },
  },
};

/** Quick actions of a row, in the order of docs/DESIGN.md. */
export const QUICK_ACTIONS: readonly ActionId[] = [
  "delete",
  "deletePermanently",
  "archive",
  "toggleRead",
  "toggleFlag",
  "togglePin",
  "move",
  "snooze",
];

/** The toolbar of the reading pane and of the selection bar. */
export const TOOLBAR_ACTIONS: readonly ActionId[] = [
  "reply",
  "replyAll",
  "forward",
  "forwardAsAttachment",
  "editDraft",
  "archive",
  "delete",
  "deletePermanently",
  "toggleRead",
  "toggleFlag",
  "move",
  "tags",
  "spam",
  "notSpam",
  "togglePin",
  "snooze",
  "unsnooze",
  "toggleTask",
  "toggleTaskDone",
  // Stage 16: the reading list and blocking the sender (in "More actions").
  "toggleReadingList",
  "blockSender",
  // Stage 19: in "More actions".
  "redirect",
  "editAsNew",
  "viewSource",
  // Stage 8: last, so the mail actions keep their places.
  "openInWindow",
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
    case "addLabel":
      return "add_label";
    case "removeLabel":
      return "remove_label";
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

// ---- Stage 7: commands that do not act on messages ----

/**
 * Ids of commands of the whole app. Labels: `search.open` for the search, `keyboard.commands`
 * (i18n) for the rest. Stage 8's palette lists them; their shortcuts are changeable like the
 * actions' (`features/keyboard/bindings.ts`).
 */
export const APP_COMMAND_IDS = [
  "search",
  "palette",
  "shortcutHelp",
  // Stage 8: the views of the rail by number (Cmd/Ctrl+1 to 8, like most mail apps).
  "goInbox",
  "goLater",
  "goTasks",
  "goPinned",
  "goSent",
  "goDrafts",
  "goArchive",
  "goTrash",
  // Stage 16: the reading list (Cmd/Ctrl+9, next to the views 1 to 8).
  "goReadingList",
  // Stage 16 (decision 0036): pinned attachments; no shortcut of its own (Cmd/Ctrl+0 resets the
  // zoom in web views), the user can give it one.
  "goPinnedAttachments",
  // SPEC "Markiert": flagged mail of every folder; no shortcut of its own either.
  "goFlagged",
  // Stage 17 (decision 0050): the contacts; no shortcut of its own.
  "goContacts",
  // The accounts-and-folders column (docs/DESIGN.md, "Seitenspalte"): open or fold it.
  "toggleSidebar",
] as const;

export type AppCommandId = (typeof APP_COMMAND_IDS)[number];

/** The view each "go to" command opens. */
export const VIEW_COMMANDS = {
  goInbox: "inbox",
  goLater: "later",
  goTasks: "tasks",
  goPinned: "pinned",
  goSent: "sent",
  goDrafts: "drafts",
  goArchive: "archive",
  goTrash: "trash",
  goReadingList: "readingList",
  goPinnedAttachments: "pinnedAttachments",
  goFlagged: "flagged",
  goContacts: "contacts",
} as const satisfies Partial<Record<AppCommandId, MailViewId>>;

export type ViewCommandId = keyof typeof VIEW_COMMANDS;

export const isViewCommand = (id: string): id is ViewCommandId => id in VIEW_COMMANDS;

const viewCommand = (id: ViewCommandId, digit: number): AppCommandDefinition => ({
  id,
  icon: MAIL_VIEW_ICONS[VIEW_COMMANDS[id]],
  shortcuts: both({ key: String(digit), mod: true }),
});

export interface AppCommandDefinition {
  id: AppCommandId;
  icon: LucideIcon;
  /** Provisional shortcuts per platform; the first one is shown in tooltips. */
  shortcuts: Record<ShortcutPlatform, readonly Shortcut[]>;
}

export const APP_COMMANDS: Record<AppCommandId, AppCommandDefinition> = {
  search: {
    id: "search",
    icon: Search,
    // Cmd/Ctrl+F like every app, "/" like Gmail and Outlook on the web.
    shortcuts: both({ key: "f", mod: true }, { key: "/" }),
  },
  // Stage 8: the command palette (Cmd/Ctrl+K like most apps) and the shortcut overview ("?").
  palette: {
    id: "palette",
    icon: Command,
    shortcuts: both({ key: "k", mod: true }),
  },
  shortcutHelp: {
    id: "shortcutHelp",
    icon: Keyboard,
    shortcuts: both({ key: "?" }),
  },
  goInbox: viewCommand("goInbox", 1),
  goLater: viewCommand("goLater", 2),
  goTasks: viewCommand("goTasks", 3),
  goPinned: viewCommand("goPinned", 4),
  goSent: viewCommand("goSent", 5),
  goDrafts: viewCommand("goDrafts", 6),
  goArchive: viewCommand("goArchive", 7),
  goTrash: viewCommand("goTrash", 8),
  goReadingList: viewCommand("goReadingList", 9),
  goPinnedAttachments: { id: "goPinnedAttachments", icon: MAIL_VIEW_ICONS.pinnedAttachments, shortcuts: both() },
  goFlagged: { id: "goFlagged", icon: MAIL_VIEW_ICONS.flagged, shortcuts: both() },
  goContacts: { id: "goContacts", icon: MAIL_VIEW_ICONS.contacts, shortcuts: both() },
  // Cmd/Ctrl+Shift+S ("sidebar"): with a modifier, so it also works from text fields and is no
  // type-ahead letter in the trees; the system claims neither.
  toggleSidebar: { id: "toggleSidebar", icon: PanelLeft, shortcuts: both({ key: "S", shift: true, mod: true }) },
};
