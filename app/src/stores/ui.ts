import { create } from "zustand";
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
  "pinned",
  "sent",
  "drafts",
  "archive",
  "trash",
  "folders",
  "savedViews",
] as const;

export type MailViewId = (typeof MAIL_VIEW_IDS)[number];

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
  | { kind: "mail"; mailView: MailViewId; savedSearchId?: SavedSearchId }
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
  /** The rail is unfolded into the labelled sidebar with the folder tree. */
  sidebarExpanded: boolean;
  /** Folders of the tree whose subfolders are hidden. Everything is unfolded by default. */
  collapsedMailboxes: ReadonlySet<MailboxId>;
  accountsPage: AccountsPage;
  linkRequest: LinkRequest | null;
  showMailView: (mailView: MailViewId) => void;
  showMailbox: (accountId: AccountId, mailboxId: MailboxId) => void;
  /** Stage 7: the results of a saved search (a view of the sidebar). */
  showSavedSearch: (savedSearchId: SavedSearchId) => void;
  selectAccount: (accountId: AccountId) => void;
  showUnifiedInbox: (view: UnifiedView) => void;
  showSettings: (section?: SettingsSectionId) => void;
  showAccountsPage: (page: AccountsPage) => void;
  showActivity: () => void;
  showOutbox: () => void;
  toggleSidebar: () => void;
  toggleMailbox: (mailboxId: MailboxId) => void;
  requestLink: (renderToken: RenderToken, index: number) => void;
  dismissLink: () => void;
}

/** Pure UI state (which view is open). Data from the core lives in TanStack Query, not here. */
export const useUiStore = create<UiState>()((set) => ({
  view: { kind: "mail", mailView: "inbox" },
  selectedAccountId: null,
  unifiedInbox: null,
  sidebarExpanded: false,
  collapsedMailboxes: new Set<MailboxId>(),
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
    set((state) => ({ sidebarExpanded: !state.sidebarExpanded }));
  },
  toggleMailbox: (mailboxId) => {
    set((state) => {
      const next = new Set(state.collapsedMailboxes);
      if (!next.delete(mailboxId)) next.add(mailboxId);
      return { collapsedMailboxes: next };
    });
  },
  requestLink: (renderToken, index) => {
    set((state) => ({ linkRequest: { renderToken, index, seq: (state.linkRequest?.seq ?? 0) + 1 } }));
  },
  dismissLink: () => {
    set({ linkRequest: null });
  },
}));
