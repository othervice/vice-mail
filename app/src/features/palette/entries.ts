import {
  Activity,
  FileText,
  Folder,
  Inbox,
  LayoutPanelLeft,
  MessagesSquare,
  PanelBottom,
  PanelRight,
  Rows3,
  Rows4,
  Search,
  Send,
  Settings,
  SquareStack,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { Account, Mailbox } from "../../services/ipc";
import { openComposer } from "../../stores/composer";
import { useComposerBridge } from "../../stores/composerBridge";
import { currentActionTarget, useActionTargetStore, type ActionTarget } from "../../stores/actionTarget";
import { useLayoutStore, type Density, type ReadingPanePosition } from "../../stores/layout";
import { useNoticeStore } from "../../stores/notices";
import { useSearchStore } from "../../stores/search";
import { useBoundShortcuts } from "../../stores/shortcuts";
import { usePaletteStore } from "../../stores/palette";
import { MAIL_VIEW_IDS, SETTINGS_SECTION_IDS, useUiStore, type MailViewId } from "../../stores/ui";
import { useSelectedAccount } from "../accounts/selection";
import {
  ACTION_IDS,
  ACTIONS,
  APP_COMMANDS,
  actionState,
  VIEW_COMMANDS,
  type ActionId,
  type ActionLabel,
  type ViewCommandId,
} from "../actions/registry";
import type { Shortcut, ShortcutPlatform } from "../actions/shortcuts";
import { MAIL_VIEW_ICONS } from "../mail/views";
import { useAiPaletteEntries } from "../ai/palette";
import { useEmlPaletteEntries } from "../eml/palette";
import { useMailboxes } from "../mailboxes/queries";
import { useTemplates } from "../snippets/queries";
// Stage 16: favourite folders of every account.
import { Star } from "lucide-react";
import { useFavoriteMailboxes } from "../organize/queries";
// Stage 16 (decision 0036): "Save all attachments" of the open message.
import { FolderDown } from "lucide-react";
import { saveAllCandidate, useSaveAllRequestStore } from "../reading/saveAllRequest";
// Decision 0039: one row per conversation or per message.
import { useConversationStore } from "../threads/preference";

export type PaletteGroup = "actions" | "commands" | "views" | "folders" | "accounts" | "settings" | "layout" | "search";

export const GROUP_ORDER: readonly PaletteGroup[] = [
  "actions",
  "commands",
  "views",
  "folders",
  "accounts",
  "layout",
  "settings",
  "search",
];

export interface PaletteEntry {
  /** Stable across openings (recently used commands are stored by it). */
  key: string;
  group: PaletteGroup;
  label: string;
  /** Second line: a folder's path, or why the entry cannot run here. */
  detail?: string;
  icon: LucideIcon;
  shortcut?: Shortcut;
  /** Why it cannot run here; the entry is shown but disabled. */
  disabled?: string;
  /** Extra words the search matches (e.g. the account of a folder). */
  keywords?: readonly string[];
  run: () => void;
}

/**
 * Toggles of the action directory and the explicit actions they run. The palette lists the
 * explicit ones (a toggle always runs one of them, under the same label), so no entry appears
 * twice; the toggle's shortcut is shown on the explicit action it would run.
 */
export const PALETTE_TOGGLES: readonly (readonly [toggle: ActionId, forms: readonly ActionLabel[]])[] = [
  ["toggleRead", ["markRead", "markUnread"]],
  ["toggleFlag", ["flag", "unflag"]],
  ["togglePin", ["pin", "unpin"]],
  ["toggleTask", ["addTask", "removeTask"]],
  ["toggleTaskDone", ["completeTask", "reopenTask"]],
  // Stage 16
  ["toggleReadingList", ["addToReadingList", "removeFromReadingList"]],
];

const TOGGLE_IDS = new Set<ActionId>(PALETTE_TOGGLES.map(([toggle]) => toggle));

const isExplicit = (id: ActionId): id is ActionLabel => !TOGGLE_IDS.has(id);

/** The "go to" command of a view of the rail, for its shortcut. */
const VIEW_COMMAND_OF = new Map(
  (Object.entries(VIEW_COMMANDS) as [ViewCommandId, MailViewId][]).map(([command, view]) => [view, command]),
);

/** The actions the palette lists, in the order of the directory. */
export const PALETTE_ACTION_IDS: readonly ActionLabel[] = ACTION_IDS.filter(isExplicit);

interface ActionEntryOptions {
  platform: ShortcutPlatform;
  target: ActionTarget | null;
  /** A mail view is open (it offers its actions once a message is chosen). */
  inMailView: boolean;
  /** The account a new message is from without a view (like the header's button). */
  account: Account | null;
}

function useActionEntries({ platform, target, inMailView, account }: ActionEntryOptions): PaletteEntry[] {
  const { t } = useTranslation();
  const push = useNoticeStore((state) => state.push);
  const shortcutsOf = useBoundShortcuts(platform);
  const ctx = target?.ctx ?? null;
  return PALETTE_ACTION_IDS.map((id): PaletteEntry => {
    const definition = ACTIONS[id];
    const label = t(`actions.labels.${id}`);
    // A toggle's shortcut where the action has none of its own ("s" flags or unflags).
    const toggle = PALETTE_TOGGLES.find(
      ([toggleId, forms]) => forms.includes(id) && (!ctx || ACTIONS[toggleId].label(ctx) === id),
    )?.[0];
    const shortcut = shortcutsOf(id)[0] ?? (toggle ? shortcutsOf(toggle)[0] : undefined);
    const base = {
      key: `action:${id}`,
      group: "actions" as const,
      label,
      icon: definition.icon(ctx ?? NO_CONTEXT),
      ...(shortcut && { shortcut }),
    };
    // A new message needs no message to act on: from the selected account, like the header's
    // button, also where no view offers its actions (the unified inbox before a message is open).
    if (id === "compose" && !ctx && account) {
      return {
        ...base,
        run: () => {
          openComposer({ kind: "new", accountId: account.id });
        },
      };
    }
    // No view with messages, or one that has nothing to act on yet.
    if (!target && !inMailView) return { ...base, disabled: t("palette.disabled.noView"), run: () => undefined };
    if (!target || !ctx) return { ...base, disabled: t("palette.disabled.noMessage"), run: () => undefined };
    if (target.allowed && !target.allowed.has(id)) {
      return { ...base, disabled: t("palette.disabled.notHere"), run: () => undefined };
    }
    const state = actionState(definition, ctx);
    if (state.kind === "unavailable") {
      const reason = definition.needsMessages && ctx.items.length === 0 ? "noMessage" : "notHere";
      return { ...base, disabled: t(`palette.disabled.${reason}`), run: () => undefined };
    }
    if (state.kind === "readOnly") {
      return {
        ...base,
        detail: t("actions.readOnly.label"),
        run: () => {
          push({ kind: "readOnly", accountId: ctx.account.id });
        },
      };
    }
    return {
      ...base,
      run: () => {
        definition.run(ctx, target.host);
      },
    };
  });
}

/**
 * Only the toggles' icons look at the context, and the palette lists no toggles; this stands in
 * for the context while there is none, to satisfy the signature.
 */
const NO_CONTEXT = { account: null, mailbox: null, items: [], canUndo: false } as never;

const PANE_ICONS: Record<ReadingPanePosition, LucideIcon> = {
  right: PanelRight,
  bottom: PanelBottom,
  replace: SquareStack,
};

const DENSITY_ICONS: Record<Density, LucideIcon> = { comfortable: Rows3, compact: Rows4 };

/** Where the palette's places lead: the UI store's navigation. */
function usePaletteUi() {
  const ui = useUiStore();
  const openOverview = usePaletteStore((state) => state.openOverview);
  return {
    ...ui,
    openOverview,
    showUnifiedInbox: () => {
      ui.showUnifiedInbox({ kind: "all" });
    },
    /** Search and its hand-over live in the mail views. */
    ensureMailView: () => {
      if (ui.view.kind !== "mail" && ui.view.kind !== "mailbox") ui.showMailView("inbox");
    },
  };
}

/**
 * Everything the command palette offers (docs/stages/08): the actions of the directory, the app
 * commands, places to go (views, folders, accounts, settings), the layout and the hand-over of
 * the typed text to the search.
 */
export function usePaletteEntries(platform: ShortcutPlatform, query: string): PaletteEntry[] {
  const { t } = useTranslation();
  const ui = usePaletteUi();
  const target = useActionTargetStore((state) => currentActionTarget(state.targets));
  const shortcutsOf = useBoundShortcuts(platform);
  const { accounts, account } = useSelectedAccount();
  const mailboxes = useMailboxes(account?.id ?? null).data ?? [];
  const favorites = useFavoriteMailboxes().data ?? [];
  const layout = useLayoutStore();
  const conversations = useConversationStore();
  const openSearch = useSearchStore((state) => state.openSearch);
  const setQuery = useSearchStore((state) => state.setQuery);
  const accountList = accounts.data ?? [];
  const noAccount = account ? undefined : t("palette.disabled.noAccount");

  const inMailView = ui.view.kind === "mail" || ui.view.kind === "mailbox";
  const entries: PaletteEntry[] = [...useActionEntries({ platform, target, inMailView, account })];

  const searchShortcut = shortcutsOf("search")[0];
  entries.push({
    key: "command:search",
    group: "commands",
    label: t("search.open"),
    icon: APP_COMMANDS.search.icon,
    ...(searchShortcut && { shortcut: searchShortcut }),
    ...(noAccount && { disabled: noAccount }),
    run: () => {
      ui.ensureMailView();
      openSearch();
    },
  });
  const helpShortcut = shortcutsOf("shortcutHelp")[0];
  entries.push({
    key: "command:shortcutHelp",
    group: "commands",
    label: t("keyboard.commands.shortcutHelp"),
    icon: APP_COMMANDS.shortcutHelp.icon,
    ...(helpShortcut && { shortcut: helpShortcut }),
    run: ui.openOverview,
  });

  // Stage 10: templates go into the open composer (filled for its first recipient).
  const insertTemplate = useComposerBridge((state) => state.insertTemplate);
  const templates = useTemplates(insertTemplate !== null);
  if (insertTemplate) {
    for (const template of templates.data ?? []) {
      entries.push({
        key: `template:${template.id}`,
        group: "commands",
        label: t("writing.templates.paletteInsert", { name: template.name }),
        icon: FileText,
        ...(template.subject && { keywords: [template.subject] }),
        run: () => {
          insertTemplate(template.id);
        },
      });
    }
  }

  // Stage 13: the AI commands of the panel (none while AI is off).
  entries.push(...useAiPaletteEntries());
  // Stage 9 (decision 0027): "Open .eml file…".
  entries.push(...useEmlPaletteEntries());

  // Stage 16 (decision 0036): every attachment of the open message into one folder; the
  // attachment section of the reading pane runs it and shows the result.
  const shownAttachments = useSaveAllRequestStore((state) => state.shown);
  const requestSaveAll = useSaveAllRequestStore((state) => state.requestSaveAll);
  const targetItems = target?.ctx?.items ?? [];
  const candidate = saveAllCandidate(shownAttachments, targetItems.length === 1 ? (targetItems[0]?.id ?? null) : null);
  const saveAllDisabled = !candidate
    ? t("palette.disabled.noMessage")
    : candidate.count < 1
      ? t("attachments.saveAll.paletteDisabled")
      : undefined;
  entries.push({
    key: "command:saveAllAttachments",
    group: "commands",
    label: t("attachments.saveAll.palette"),
    icon: FolderDown,
    ...(saveAllDisabled && { disabled: saveAllDisabled }),
    run: () => {
      if (candidate && candidate.count > 0) requestSaveAll(candidate.messageId);
    },
  });

  for (const id of MAIL_VIEW_IDS) {
    const command = VIEW_COMMAND_OF.get(id);
    const viewShortcut = command ? shortcutsOf(command)[0] : undefined;
    entries.push({
      key: `view:${id}`,
      group: "views",
      label: t("palette.goTo", { name: t(`nav.${id}`) }),
      icon: MAIL_VIEW_ICONS[id],
      ...(viewShortcut && { shortcut: viewShortcut }),
      // "Folders" is no view of its own: like the rail's entry it opens the column, and focuses it.
      run: () => {
        if (id === "folders") ui.openSidebar();
        else ui.showMailView(id);
      },
    });
  }
  if (accountList.length > 1) {
    entries.push({
      key: "view:unified",
      group: "views",
      // The view's name as the sidebar writes it, like every other "Go to" entry.
      label: t("palette.goTo", { name: t("unified.title") }),
      icon: Inbox,
      run: () => {
        ui.showUnifiedInbox();
      },
    });
  }
  entries.push(
    { key: "view:activity", group: "views", label: t("palette.goTo", { name: t("activity.nav") }), icon: Activity, run: ui.showActivity },
    { key: "view:outbox", group: "views", label: t("palette.goTo", { name: t("composer.outbox.nav") }), icon: Send, run: ui.showOutbox },
  );

  // Stage 16: the favourites first, in their order, with their account (any account).
  for (const mailbox of favorites) {
    const owner = accountList.find((candidate) => candidate.id === mailbox.accountId);
    if (!owner) continue;
    const ownerName = owner.displayName || owner.email;
    const name = mailbox.role === "custom" ? mailbox.name : t(`roles.${mailbox.role}`);
    entries.push({
      key: `favorite:${String(mailbox.id)}`,
      group: "folders",
      label: t("organize.favorites.paletteOpen", { name }),
      detail: accountList.length > 1 ? t("organize.favorites.inAccount", { name: mailbox.path, account: ownerName }) : mailbox.path,
      keywords: [ownerName, mailbox.path],
      icon: Star,
      run: () => {
        ui.showMailbox(mailbox.accountId, mailbox.id);
      },
    });
  }

  if (account) {
    const accountName = account.displayName || account.email;
    for (const mailbox of folderChoices(mailboxes)) {
      entries.push({
        key: `folder:${account.id}:${String(mailbox.id)}`,
        group: "folders",
        label: t("palette.openFolder", { name: mailbox.name }),
        detail: mailbox.path,
        keywords: [accountName],
        icon: Folder,
        run: () => {
          ui.showMailbox(account.id, mailbox.id);
        },
      });
    }
  }
  if (accountList.length > 1) {
    for (const other of accountList) {
      const name = other.displayName ? t("nav.account", { name: other.displayName, email: other.email }) : other.email;
      entries.push({
        key: `account:${other.id}`,
        group: "accounts",
        label: t("palette.switchAccount", { name }),
        icon: UserRound,
        ...(other.id === account?.id && { detail: t("palette.current") }),
        run: () => {
          ui.selectAccount(other.id);
        },
      });
    }
  }

  for (const position of ["right", "bottom", "replace"] as const) {
    entries.push({
      key: `layout:pane:${position}`,
      group: "layout",
      label: t(`layout.readingPane.command.${position}`),
      icon: PANE_ICONS[position],
      ...(layout.readingPane === position && { detail: t("palette.current") }),
      run: () => {
        layout.setLayout({ readingPane: position });
      },
    });
  }
  for (const density of ["comfortable", "compact"] as const) {
    entries.push({
      key: `layout:density:${density}`,
      group: "layout",
      label: t(`layout.density.command.${density}`),
      icon: DENSITY_ICONS[density],
      ...(layout.density === density && { detail: t("palette.current") }),
      run: () => {
        layout.setLayout({ density });
      },
    });
  }
  // The column as it shows now: docked, or (in a narrow window) over the content.
  const sidebarShown = ui.sidebarNarrow ? ui.sidebarOverlay : ui.sidebarExpanded;
  const sidebarShortcut = shortcutsOf("toggleSidebar")[0];
  entries.push({
    key: "layout:conversations",
    group: "layout",
    label: t(conversations.on ? "threads.toggleOff" : "threads.toggleOn"),
    icon: MessagesSquare,
    run: () => {
      conversations.setOn(!conversations.on);
    },
  });
  entries.push({
    key: "layout:sidebar",
    group: "layout",
    label: t(sidebarShown ? "layout.sidebar.collapse" : "layout.sidebar.expand"),
    icon: LayoutPanelLeft,
    ...(sidebarShortcut && { shortcut: sidebarShortcut }),
    run: ui.toggleSidebar,
  });

  for (const id of SETTINGS_SECTION_IDS) {
    entries.push({
      key: `settings:${id}`,
      group: "settings",
      label: t("palette.openSettings", { name: t(`settings.${id}.title`) }),
      icon: Settings,
      run: () => {
        ui.showSettings(id);
      },
    });
  }

  const text = query.trim();
  if (text) {
    entries.push({
      key: "search:query",
      group: "search",
      label: t("palette.searchFor", { query: text }),
      icon: Search,
      ...(noAccount && { disabled: noAccount }),
      run: () => {
        ui.ensureMailView();
        openSearch();
        setQuery(text);
      },
    });
  }
  return entries;
}

/** Own folders (the role folders have views of their own), in path order. */
function folderChoices(mailboxes: readonly Mailbox[]): Mailbox[] {
  return mailboxes.filter((m) => m.selectable && m.role === "custom").sort((a, b) => a.path.localeCompare(b.path));
}
