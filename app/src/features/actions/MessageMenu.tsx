import {
  AppWindow,
  CalendarClock,
  Copy,
  Eye,
  FileDown,
  Flag,
  Folder,
  FolderInput,
  Lock,
  MailClock,
  PanelRight,
  Printer,
  Reply,
  Tags,
  Clock,
  type LucideIcon,
} from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextMenu, type MenuCloseReason, type MenuEntry, type MenuItemEntry, type SubmenuEntry } from "../../components/ContextMenu";
import { toCoreError, type Account, type FollowUpChoice, type Mailbox, type MessageId, type SnoozeChoice } from "../../services/ipc";
import { useNoticeStore } from "../../stores/notices";
import { usePrintRequest } from "../../stores/printRequest";
import { useBoundShortcuts } from "../../stores/shortcuts";
import { useUiStore } from "../../stores/ui";
import { followUpPresets } from "../comfort/FollowUpPicker";
import { useFollowUpTimes, useSnoozeTimes } from "../comfort/queries";
import { formatSnoozeTime, snoozePresets } from "../comfort/snooze";
import { useExportMessage } from "../extras/queries";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { useMailboxes } from "../mailboxes/queries";
import { buildMailboxTree, isRoleView, type MailboxNode } from "../mailboxes/tree";
import type { RowMenuRequest } from "../messages/MessageList";
import { useFolderName } from "../organize/folders";
import { useOtherAccountFolders } from "../organize/transfer";
import type { FolderActions } from "./FolderActions";
import { hasTag } from "./keywords";
import { ACTIONS, actionState, type ActionContext, type ActionHost, type ActionId, type ActionState } from "./registry";
import { ariaShortcut, formatShortcut, type ShortcutPlatform } from "./shortcuts";

/**
 * The messages of one account and folder the menu acts on, with what its submenus run directly
 * (no picker in between): every action goes through the same action controller as the toolbar,
 * so enablement, the read-only reason, undo and the outbox states are the same.
 */
export interface MenuScope {
  ctx: ActionContext;
  host: ActionHost;
  moveTo: (target: Mailbox) => void;
  copyTo: (target: Mailbox) => void;
  setTag: (tag: string, on: boolean) => void;
  snooze: (choice: SnoozeChoice) => void;
  followUp: (choice: FollowUpChoice) => void;
}

/**
 * A folder view's actions as a menu scope for `ctx.items`; `childIds`: those chosen as child rows
 * of unfolded conversations, which act alone (stage 17), like the selection bar.
 */
export function folderMenuScope(actions: FolderActions, ctx: ActionContext, childIds: ReadonlySet<MessageId>): MenuScope {
  const { controller } = actions;
  const exact = childIds.size > 0 ? childIds : false;
  return {
    ctx,
    host: controller.hostFor(childIds),
    moveTo: (target) => {
      actions.moveOrAsk(target, ctx.items, exact);
    },
    copyTo: (target) => {
      actions.copyTo(target, ctx.items, exact);
    },
    setTag: (tag, on) => {
      if (on) void controller.apply({ type: "addKeyword", keyword: tag }, ctx.items, { detail: tag, exact });
      else
        void controller.apply(
          { type: "removeKeyword", keyword: tag },
          ctx.items.filter((item) => hasTag(item, tag)),
          { detail: tag, exact },
        );
    },
    snooze: (choice) => {
      controller.snoozeNow(ctx.items, choice, exact);
    },
    followUp: (choice) => {
      controller.followUpNow(ctx.items, choice);
    },
  };
}

/** Actions for one message only: with several messages (in several scopes, too) they do not show. */
const SINGLE: ReadonlySet<ActionId> = new Set([
  "reply",
  "replyAll",
  "forward",
  "forwardAsAttachment",
  "editDraft",
  "openInWindow",
  "viewSource",
  "redirect",
  "editAsNew",
]);

function folderIcon(mailbox: Mailbox): LucideIcon {
  return isRoleView(mailbox.role) ? MAIL_VIEW_ICONS[mailbox.role] : Folder;
}

function flatten(nodes: readonly MailboxNode[], depth = 0): { mailbox: Mailbox; depth: number }[] {
  return nodes.flatMap((node) => [{ mailbox: node.mailbox, depth }, ...flatten(node.children, depth + 1)]);
}

interface MessageMenuProps {
  request: RowMenuRequest;
  /** The request's messages by their account and folder (one scope in a folder view). */
  scopes: readonly MenuScope[];
  platform: ShortcutPlatform;
  /** Tags the view knows (its loaded messages), offered under "Tags". */
  knownTags: readonly string[];
  /** The menu closed (the view forgets the request). */
  onClose: () => void;
}

/**
 * Stage 21 (decision 0064): the context menu of messages, in the manner of Thunderbird's: a row of
 * icons (Reply, Archive, Spam, Delete), then opening, answering, marks, snoozing, the task and the
 * reading list, the reminder, moving, copying, tags, and the tools (edit as new, source, .eml,
 * print). Built from the action registry: what the toolbar would not offer is left out, what a
 * read-only account cannot do is shown disabled with the reason (and the way to allow changes at
 * the end), and every action runs through the view's action controller.
 */
export function MessageMenu({ request, scopes, platform, knownTags, onClose }: MessageMenuProps) {
  const { t, i18n } = useTranslation();
  const noteId = useId();
  const shortcutsOf = useBoundShortcuts(platform);
  const showAccountsPage = useUiStore((state) => state.showAccountsPage);
  const push = useNoticeStore((state) => state.push);
  const requestPrint = usePrintRequest((state) => state.request);
  const exportMessage = useExportMessage();
  const folderName = useFolderName();
  const live = scopes.filter((scope) => scope.ctx.items.length > 0);
  const first = live[0];
  const accountIds = new Set(live.map((scope) => scope.ctx.account.id));
  const oneAccount = accountIds.size === 1;
  const oneScope = live.length === 1;
  const mailboxes = useMailboxes(oneAccount ? (first?.ctx.account.id ?? null) : null).data ?? [];
  const others = useOtherAccountFolders(first?.ctx.account.id ?? "");
  const snoozeTimes = useSnoozeTimes();
  const followUpTimes = useFollowUpTimes();
  // The times of the presets are shown relative to when the menu opened.
  const [now] = useState(() => Date.now());

  const close = (reason: MenuCloseReason) => {
    request.close(reason !== "outside");
    onClose();
  };
  if (!first) return null;

  const items = live.flatMap((scope) => scope.ctx.items);
  const single = items.length === 1 ? items[0] : undefined;
  /** All the messages in one context, for labels and toggles (each scope runs its own share). */
  const all: ActionContext = { ...first.ctx, items };

  /** The state of an action over every scope: unavailable in all, read-only in one, else enabled. */
  const stateOf = (id: ActionId): ActionState => {
    if (SINGLE.has(id) && !single) return { kind: "unavailable" };
    const states = live.map((scope) => actionState(ACTIONS[id], scope.ctx));
    if (states.every((state) => state.kind === "unavailable")) return { kind: "unavailable" };
    if (states.some((state) => state.kind === "readOnly")) return { kind: "readOnly" };
    return { kind: "enabled" };
  };
  /** A toggle as the action it would run on all the messages ("Mark as read" or "… unread"). */
  const explicit = (id: ActionId): ActionId => ACTIONS[id].label(all);
  const run = (id: ActionId) => {
    for (const scope of live) {
      if (actionState(ACTIONS[id], scope.ctx).kind === "enabled") ACTIONS[id].run(scope.ctx, scope.host);
    }
  };
  const readOnlyAccount = live.find((scope) => !scope.ctx.account.writesEnabled)?.ctx.account;
  const readOnlyProps = (label: string) => ({
    disabled: true,
    describedBy: noteId,
    title: t("actions.readOnly.tooltip", { label }),
  });

  /** A registry action as a menu item; null where it does not apply here. */
  const actionItem = (id: ActionId, options: { label?: string; icon?: LucideIcon; showUnavailable?: boolean } = {}): MenuItemEntry | null => {
    const state = stateOf(id);
    if (state.kind === "unavailable" && !options.showUnavailable) return null;
    const definition = ACTIONS[id];
    const label = options.label ?? t(`actions.labels.${definition.label(all)}`);
    const shortcut = shortcutsOf(id)[0];
    const readOnly = state.kind === "readOnly";
    return {
      kind: "item",
      id,
      label,
      icon: options.icon ?? definition.icon(all),
      detail: !readOnly && shortcut ? formatShortcut(shortcut, platform) : undefined,
      keyshortcuts: shortcut && ariaShortcut(shortcut, platform),
      ...(readOnly && readOnlyProps(label)),
      ...(state.kind === "unavailable" && { disabled: true, title: label }),
      onSelect: () => {
        run(id);
      },
      testId: `menu-${id}`,
    };
  };
  const present = (entries: readonly (MenuEntry | null | false | undefined)[]): MenuEntry[] =>
    entries.filter((entry): entry is MenuEntry => Boolean(entry));
  const submenu = (id: string, label: string, icon: LucideIcon, entries: readonly (MenuEntry | null | false | undefined)[]): SubmenuEntry | null => {
    const shown = present(entries);
    // A submenu of separators and headings only offers nothing.
    if (!shown.some((entry) => entry.kind === "item" || entry.kind === "submenu")) return null;
    return { kind: "submenu", id, label, icon, entries: shown, testId: `menu-${id}` };
  };
  /** A submenu whose every entry needs writes: disabled as a whole on a read-only account. */
  const writingSubmenu = (id: string, label: string, icon: LucideIcon, entries: readonly (MenuEntry | null | false | undefined)[], state: ActionState): MenuEntry | null => {
    if (state.kind === "unavailable") return null;
    if (state.kind === "readOnly") return { kind: "item", id, label, icon, ...readOnlyProps(label), onSelect: () => undefined, testId: `menu-${id}` };
    return submenu(id, label, icon, entries);
  };

  // ---- the icon row: four fixed places, disabled where they do not apply ----
  const iconRow = [
    actionItem(stateOf("editDraft").kind !== "unavailable" ? "editDraft" : "reply", { showUnavailable: true }),
    actionItem("archive", { showUnavailable: true }),
    actionItem(stateOf("notSpam").kind !== "unavailable" ? "notSpam" : "spam", { showUnavailable: true }),
    actionItem(stateOf("deletePermanently").kind !== "unavailable" ? "deletePermanently" : "delete", { showUnavailable: true }),
  ].filter((entry): entry is MenuItemEntry => entry !== null);

  // ---- open, answer ----
  const open = single
    ? submenu("open", t("messageMenu.open"), Eye, [
        request.open && {
          kind: "item",
          id: "openHere",
          label: t("messageMenu.openHere"),
          icon: PanelRight,
          onSelect: request.open,
          testId: "menu-openHere",
        },
        actionItem("openInWindow", { label: t("messageMenu.openInWindow"), icon: AppWindow }),
      ])
    : null;
  const respond = submenu("respond", t("messageMenu.respond"), Reply, [
    actionItem("reply"),
    actionItem("replyAll"),
    actionItem("forward"),
    actionItem("forwardAsAttachment"),
    actionItem("redirect"),
  ]);

  // ---- marks ----
  const mark = submenu("mark", t("messageMenu.mark"), Flag, [actionItem("markRead"), actionItem("markUnread"), actionItem("flag"), actionItem("unflag")]);
  const snooze = submenu("snooze", t("actions.labels.snooze"), Clock, [
    ...(snoozeTimes.data ? snoozePresets(snoozeTimes.data) : []).map(
      (preset): MenuItemEntry => ({
        kind: "item",
        id: `snooze-${preset.kind}`,
        label: t(`comfort.snooze.${preset.kind}`),
        detail: formatSnoozeTime(preset.at, now, i18n.language),
        onSelect: () => {
          for (const scope of live) scope.snooze({ kind: preset.kind });
        },
        testId: `menu-snooze-${preset.kind}`,
      }),
    ),
    oneScope && actionItem("snooze", { label: t("messageMenu.pickTime"), icon: CalendarClock }),
    stateOf("unsnooze").kind !== "unavailable" && { kind: "separator", id: "snooze-sep" },
    actionItem("unsnooze"),
  ]);
  const followUpOn = stateOf("followUp").kind !== "unavailable";
  const followUp = submenu("followUp", t("messageMenu.followUp"), MailClock, [
    ...(followUpOn && followUpTimes.data ? followUpPresets(followUpTimes.data) : []).map(
      (preset): MenuItemEntry => ({
        kind: "item",
        id: `followUp-${preset.kind}`,
        label: t(`comfort.followUp.${preset.kind}`),
        detail: formatSnoozeTime(preset.at, now, i18n.language),
        onSelect: () => {
          for (const scope of live) {
            if (actionState(ACTIONS.followUp, scope.ctx).kind === "enabled") scope.followUp({ kind: preset.kind });
          }
        },
        testId: `menu-followUp-${preset.kind}`,
      }),
    ),
    followUpOn && oneScope && actionItem("followUp", { label: t("messageMenu.pickTime"), icon: CalendarClock }),
    stateOf("clearFollowUp").kind !== "unavailable" && followUpOn && { kind: "separator", id: "followUp-sep" },
    actionItem("clearFollowUp"),
  ]);

  // ---- move, copy: this account's folders, then the other accounts' (each a submenu) ----
  const current = new Set(live.map((scope) => scope.ctx.mailbox.id));
  const folderItems = (prefix: string, list: readonly Mailbox[], account: Account, skip: (mailbox: Mailbox) => boolean, choose: (target: Mailbox) => void): MenuItemEntry[] =>
    flatten(buildMailboxTree(list))
      .filter(({ mailbox }) => mailbox.selectable && !skip(mailbox))
      .map(({ mailbox, depth }) => {
        const label = folderName(mailbox);
        return {
          kind: "item",
          id: `${prefix}-${String(mailbox.id)}`,
          label,
          icon: folderIcon(mailbox),
          indent: depth,
          ...(!account.writesEnabled && readOnlyProps(label)),
          onSelect: () => {
            choose(mailbox);
          },
          testId: `menu-${prefix}-${String(mailbox.id)}`,
        };
      });
  const otherAccounts = (prefix: string, choose: (target: Mailbox) => void): MenuEntry[] => {
    const entries = others.filter((entry) => entry.mailboxes.length > 0);
    if (entries.length === 0) return [];
    return [
      { kind: "separator", id: `${prefix}-others` },
      ...entries.flatMap((entry) => {
        const name = entry.account.displayName || entry.account.email;
        const sub = submenu(`${prefix}-account-${entry.account.id}`, name, FolderInput, folderItems(prefix, entry.mailboxes, entry.account, () => false, choose));
        return sub ? [sub] : [];
      }),
    ];
  };
  const moveTarget = (target: Mailbox) => {
    for (const scope of live) {
      if (scope.ctx.mailbox.id !== target.id) scope.moveTo(target);
    }
  };
  const copyTarget = (target: Mailbox) => {
    for (const scope of live) {
      if (scope.ctx.mailbox.id !== target.id) scope.copyTo(target);
    }
  };
  const ownAccount = first.ctx.account;
  const move = oneAccount
    ? writingSubmenu(
        "moveTo",
        t("messageMenu.moveTo"),
        FolderInput,
        [
          oneScope && actionItem("move", { label: t("messageMenu.chooseFolder") }),
          oneScope && { kind: "separator", id: "move-sep" },
          ...folderItems("move", mailboxes, ownAccount, (mailbox) => oneScope && current.has(mailbox.id), moveTarget),
          ...otherAccounts("move", moveTarget),
        ],
        stateOf("move"),
      )
    : null;
  const copy = oneAccount
    ? submenu("copyTo", t("messageMenu.copyTo"), Copy, [
        ...folderItems("copy", mailboxes, ownAccount, (mailbox) => oneScope && current.has(mailbox.id), copyTarget),
        ...otherAccounts("copy", copyTarget),
      ])
    : null;
  const tagNames = [...new Set([...knownTags])];
  const tags = oneAccount
    ? writingSubmenu(
        "tags",
        t("actions.labels.tags"),
        Tags,
        [
          ...tagNames.map((tag): MenuItemEntry => {
            const on = items.every((item) => hasTag(item, tag));
            return {
              kind: "item",
              id: `tag-${tag}`,
              label: tag,
              checked: on,
              onSelect: () => {
                for (const scope of live) scope.setTag(tag, !on);
              },
              testId: `menu-tag-${tag}`,
            };
          }),
          tagNames.length > 0 && oneScope && { kind: "separator", id: "tags-sep" },
          oneScope && actionItem("tags", { label: t("messageMenu.editTags") }),
        ],
        stateOf("tags"),
      )
    : null;

  // ---- tools for one message ----
  const exportEntry: MenuItemEntry | null = single
    ? {
        kind: "item",
        id: "saveEml",
        label: t("messageMenu.saveEml"),
        icon: FileDown,
        onSelect: () => {
          exportMessage.mutateAsync(single.id).then(
            (path) => {
              if (path) push({ kind: "exported", path });
            },
            (error: unknown) => {
              push({ kind: "error", context: "export", error: toCoreError(error) });
            },
          );
        },
        testId: "menu-saveEml",
      }
    : null;
  const printEntry: MenuItemEntry | null =
    single && request.open
      ? {
          kind: "item",
          id: "print",
          label: t("messageMenu.print"),
          icon: Printer,
          onSelect: () => {
            // The print styles print the reading pane's message: open it there, print once shown.
            requestPrint(single.id);
            request.open?.();
          },
          testId: "menu-print",
        }
      : null;

  const groups: (MenuEntry | null | false | undefined)[][] = [
    [open, respond],
    [
      actionItem(explicit("toggleRead")),
      mark,
      actionItem(explicit("togglePin")),
      snooze,
      actionItem(explicit("toggleTask")),
      stateOf("toggleTaskDone").kind !== "unavailable" && actionItem(explicit("toggleTaskDone")),
      actionItem(explicit("toggleReadingList")),
      followUp,
    ],
    [move, copy, tags],
    [actionItem("editAsNew"), actionItem("viewSource"), exportEntry, printEntry],
    [
      readOnlyAccount && {
        kind: "item",
        id: "allowChanges",
        label: t("actions.readOnly.open"),
        icon: Lock,
        accent: true,
        note: { id: noteId, text: `${t("actions.readOnly.label")}: ${t("actions.readOnly.explanation")}` },
        onSelect: () => {
          showAccountsPage({ kind: "account", accountId: readOnlyAccount.id });
        },
        testId: "menu-allowChanges",
      },
    ],
  ];
  const entries = groups
    .map((group) => present(group))
    .filter((group) => group.length > 0)
    .flatMap((group, index) => (index === 0 ? group : [{ kind: "separator", id: `group-${String(index)}` } as const, ...group]));

  return (
    <ContextMenu
      label={t("messageMenu.label", { count: items.length })}
      point={request.point}
      iconRow={{ label: t("messageMenu.iconRow"), items: iconRow }}
      entries={entries}
      onClose={close}
      data-testid="message-menu"
    />
  );
}
