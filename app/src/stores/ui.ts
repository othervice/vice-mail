import { create } from "zustand";
import { DEFAULT_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH } from "../services/ipc";
import type { AccountId, MailboxId, RenderToken, SavedSearchId, UnifiedView } from "../services/ipc";

/** Mail views of the icon rail, top to bottom (docs/DESIGN.md). Ids double as i18n keys in `nav`. */
export const MAIL_VIEW_IDS = [
  "inbox",
  "later",
  "tasks",
  // Stage 16: the reading list ("Zum Lesen"), a local mark like Tasks.
  "readingList",
  // Stage 16 (decision 0036): pinned attachments ("Angeheftete Anhänge"), next to it.
  "pinnedAttachments",
  // SPEC "Markiert": what carries the server's flag, in every folder but Trash and Junk.
  "flagged",
  "pinned",
  "sent",
  "drafts",
  "archive",
  "trash",
  "folders",
  "savedViews",
] as const;

export type MailViewId = (typeof MAIL_VIEW_IDS)[number];

/** The rail's "Folders" opens the accounts-and-folders column; every other entry is a view. */
export type ShownMailViewId = Exclude<MailViewId, "folders">;

/**
 * Width of the accounts-and-folders column in px at 100 % text size (it scales with the text).
 * The limits are the core's (`WindowLayout.sidebarWidth`, stored with the window's layout).
 */
export const SIDEBAR_WIDTH = { min: MIN_SIDEBAR_WIDTH, default: DEFAULT_SIDEBAR_WIDTH, max: MAX_SIDEBAR_WIDTH, step: 16 } as const;

export function clampSidebarWidth(width: number): number {
  return Math.round(Math.min(SIDEBAR_WIDTH.max, Math.max(SIDEBAR_WIDTH.min, width)));
}

export const SETTINGS_SECTION_IDS = [
  "general",
  "accounts",
  "reading",
  // Stage 5: undo delay, sent copies, signatures.
  "compose",
  // Stage 10: the contact directory.
  "contacts",
  // Stage 9: categories, bundles and sender rules; notifications.
  "inbox",
  "notifications",
  "appearance",
  // Stage 8: shortcuts.
  "keyboard",
  "sync",
  // Stage 13: AI in the app (next to AI over MCP, stage 12).
  "ai",
  "mcp",
  "about",
] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTION_IDS)[number];

/** What Settings → Accounts shows: the list, the setup form or one account. */
export type AccountsPage = { kind: "list" } | { kind: "add" } | { kind: "account"; accountId: AccountId };

export type View =
  /** Stage 7: `savedSearchId` with `savedViews`: one saved search's results instead of the list. */
  | { kind: "mail"; mailView: ShownMailViewId; savedSearchId?: SavedSearchId }
  /** A folder picked in the folder tree that has no view of its own in the rail. */
  | { kind: "mailbox"; accountId: AccountId; mailboxId: MailboxId }
  | { kind: "settings"; section: SettingsSectionId }
  /** Changes waiting for the server, failed or changed elsewhere, of every account (stage 4). */
  | { kind: "activity" }
  /** Stage 5: sends of every account ("Ausgang"): waiting, failed, unclear. */
  | { kind: "outbox" };

/**
 * A link the user activated in the reading pane: a click in an HTML mail (the core's navigation
 * hook sends `linkActivated`) or in a plain-text mail. `seq` makes a second click on the same
 * link a new request. The pane only acts on it if it shows the rendering `renderToken`.
 */
export interface LinkRequest {
  renderToken: RenderToken;
  index: number;
  seq: number;
}

interface UiState {
  view: View;
  /**
   * The account the rail's views refer to until the unified inbox exists (stage 7). Null means
   * "the first account"; a removed account falls back to that as well.
   */
  selectedAccountId: AccountId | null;
  /**
   * Stage 7: the Inbox view shows the unified inbox of every visible account or of a group
   * instead of the selected account's inbox. Picking an account or a folder ends it.
   */
  unifiedInbox: UnifiedView | null;
  /**
   * The accounts-and-folders column is open (docs/DESIGN.md, "Seitenspalte"); folded, the narrow
   * icon rail shows instead. Stored with the window's layout (`useStoredSidebar`).
   */
  sidebarExpanded: boolean;
  /**
   * The window is too narrow for the column next to the list and the reading pane
   * (`useSidebarFit`): the rail shows, and the column opens over the content instead.
   */
  sidebarNarrow: boolean;
  /** In a narrow window: the column lies over the content (not stored). */
  sidebarOverlay: boolean;
  /** Width of the column in px at 100 % text size, 200 to 320 (`SIDEBAR_WIDTH`). */
  sidebarWidth: number;
  /** The column should take the focus once it shows (`openSidebar`); it clears this then. */
  sidebarFocusRequested: boolean;
  /** Folders of the tree whose subfolders are hidden. Everything is unfolded by default. */
  collapsedMailboxes: ReadonlySet<MailboxId>;
  /** Accounts of the column whose folders are hidden. */
  collapsedAccounts: ReadonlySet<AccountId>;
  /** Labelled sections of the column ("views", "savedSearches", "favorites") folded to their label. */
  collapsedSections: ReadonlySet<string>;
  accountsPage: AccountsPage;
  linkRequest: LinkRequest | null;
  showMailView: (mailView: ShownMailViewId) => void;
  showMailbox: (accountId: AccountId, mailboxId: MailboxId) => void;
  /** Stage 7: the results of a saved search (a view of the sidebar). */
  showSavedSearch: (savedSearchId: SavedSearchId) => void;
  selectAccount: (accountId: AccountId) => void;
  showUnifiedInbox: (view: UnifiedView) => void;
  showSettings: (section?: SettingsSectionId) => void;
  showAccountsPage: (page: AccountsPage) => void;
  showActivity: () => void;
  showOutbox: () => void;
  /** The column's toggle (button, shortcut, palette): docked or folded, in a narrow window over the content. */
  toggleSidebar: () => void;
  /** "Go to Folders" (palette): shows the column (docked, or over the content) and focuses it. */
  openSidebar: () => void;
  sidebarFocused: () => void;
  setSidebarNarrow: (narrow: boolean) => void;
  closeSidebarOverlay: () => void;
  setSidebarWidth: (width: number) => void;
  toggleMailbox: (mailboxId: MailboxId) => void;
  toggleAccount: (accountId: AccountId) => void;
  toggleSection: (section: string) => void;
  requestLink: (renderToken: RenderToken, index: number) => void;
  dismissLink: () => void;
}

/** Pure UI state (which view is open). Data from the core lives in TanStack Query, not here. */
export const useUiStore = create<UiState>()((set) => ({
  view: { kind: "mail", mailView: "inbox" },
  selectedAccountId: null,
  unifiedInbox: null,
  sidebarExpanded: false,
  sidebarNarrow: false,
  sidebarOverlay: false,
  sidebarWidth: SIDEBAR_WIDTH.default,
  sidebarFocusRequested: false,
  collapsedMailboxes: new Set<MailboxId>(),
  collapsedAccounts: new Set<AccountId>(),
  collapsedSections: new Set<string>(),
  accountsPage: { kind: "list" },
  linkRequest: null,
  showMailView: (mailView) => {
    set({ view: { kind: "mail", mailView } });
  },
  showSavedSearch: (savedSearchId) => {
    set({ view: { kind: "mail", mailView: "savedViews", savedSearchId } });
  },
  showMailbox: (accountId, mailboxId) => {
    set({ view: { kind: "mailbox", accountId, mailboxId }, selectedAccountId: accountId, unifiedInbox: null });
  },
  selectAccount: (accountId) => {
    // A folder of another account makes no sense for the new one: back to its inbox.
    set((state) => ({
      selectedAccountId: accountId,
      unifiedInbox: null,
      view: state.view.kind === "mailbox" ? { kind: "mail", mailView: "inbox" } : state.view,
    }));
  },
  showUnifiedInbox: (unifiedInbox) => {
    set({ unifiedInbox, view: { kind: "mail", mailView: "inbox" } });
  },
  showSettings: (section = "general") => {
    set({ view: { kind: "settings", section }, ...(section === "accounts" && { accountsPage: { kind: "list" } }) });
  },
  showAccountsPage: (accountsPage) => {
    set({ view: { kind: "settings", section: "accounts" }, accountsPage });
  },
  showActivity: () => {
    set({ view: { kind: "activity" } });
  },
  showOutbox: () => {
    set({ view: { kind: "outbox" } });
  },
  toggleSidebar: () => {
    set((state) => (state.sidebarNarrow ? { sidebarOverlay: !state.sidebarOverlay } : { sidebarExpanded: !state.sidebarExpanded }));
  },
  openSidebar: () => {
    set((state) => ({ ...(state.sidebarNarrow ? { sidebarOverlay: true } : { sidebarExpanded: true }), sidebarFocusRequested: true }));
  },
  sidebarFocused: () => {
    set({ sidebarFocusRequested: false });
  },
  setSidebarNarrow: (sidebarNarrow) => {
    set((state) => (state.sidebarNarrow === sidebarNarrow ? state : { sidebarNarrow, sidebarOverlay: false }));
  },
  closeSidebarOverlay: () => {
    set({ sidebarOverlay: false });
  },
  setSidebarWidth: (width) => {
    set({ sidebarWidth: clampSidebarWidth(width) });
  },
  toggleMailbox: (mailboxId) => {
    set((state) => {
      const next = new Set(state.collapsedMailboxes);
      if (!next.delete(mailboxId)) next.add(mailboxId);
      return { collapsedMailboxes: next };
    });
  },
  toggleAccount: (accountId) => {
    set((state) => {
      const next = new Set(state.collapsedAccounts);
      if (!next.delete(accountId)) next.add(accountId);
      return { collapsedAccounts: next };
    });
  },
  toggleSection: (section) => {
    set((state) => {
      const next = new Set(state.collapsedSections);
      if (!next.delete(section)) next.add(section);
      return { collapsedSections: next };
    });
  },
  requestLink: (renderToken, index) => {
    set((state) => ({ linkRequest: { renderToken, index, seq: (state.linkRequest?.seq ?? 0) + 1 } }));
  },
  dismissLink: () => {
    set({ linkRequest: null });
  },
}));
