import { create } from "zustand";
import type { AccountId, MailboxId } from "../services/ipc";

/** Mail views of the icon rail, top to bottom (docs/DESIGN.md). Ids double as i18n keys in `nav`. */
export const MAIL_VIEW_IDS = [
  "inbox",
  "later",
  "tasks",
  "pinned",
  "sent",
  "drafts",
  "archive",
  "trash",
  "folders",
  "savedViews",
] as const;

export type MailViewId = (typeof MAIL_VIEW_IDS)[number];

export const SETTINGS_SECTION_IDS = ["general", "accounts", "appearance", "about"] as const;

export type SettingsSectionId = (typeof SETTINGS_SECTION_IDS)[number];

/** What Settings → Accounts shows: the list, the setup form or one account. */
export type AccountsPage = { kind: "list" } | { kind: "add" } | { kind: "account"; accountId: AccountId };

export type View =
  | { kind: "mail"; mailView: MailViewId }
  /** A folder picked in the folder tree that has no view of its own in the rail. */
  | { kind: "mailbox"; accountId: AccountId; mailboxId: MailboxId }
  | { kind: "settings"; section: SettingsSectionId };

interface UiState {
  view: View;
  /**
   * The account the rail's views refer to until the unified inbox exists (stage 7). Null means
   * "the first account"; a removed account falls back to that as well.
   */
  selectedAccountId: AccountId | null;
  /** The rail is unfolded into the labelled sidebar with the folder tree. */
  sidebarExpanded: boolean;
  /** Folders of the tree whose subfolders are hidden. Everything is unfolded by default. */
  collapsedMailboxes: ReadonlySet<MailboxId>;
  accountsPage: AccountsPage;
  showMailView: (mailView: MailViewId) => void;
  showMailbox: (accountId: AccountId, mailboxId: MailboxId) => void;
  selectAccount: (accountId: AccountId) => void;
  showSettings: (section?: SettingsSectionId) => void;
  showAccountsPage: (page: AccountsPage) => void;
  toggleSidebar: () => void;
  toggleMailbox: (mailboxId: MailboxId) => void;
}

/** Pure UI state (which view is open). Data from the core lives in TanStack Query, not here. */
export const useUiStore = create<UiState>()((set) => ({
  view: { kind: "mail", mailView: "inbox" },
  selectedAccountId: null,
  sidebarExpanded: false,
  collapsedMailboxes: new Set<MailboxId>(),
  accountsPage: { kind: "list" },
  showMailView: (mailView) => {
    set({ view: { kind: "mail", mailView } });
  },
  showMailbox: (accountId, mailboxId) => {
    set({ view: { kind: "mailbox", accountId, mailboxId }, selectedAccountId: accountId });
  },
  selectAccount: (accountId) => {
    // A folder of another account makes no sense for the new one: back to its inbox.
    set((state) => ({
      selectedAccountId: accountId,
      view: state.view.kind === "mailbox" ? { kind: "mail", mailView: "inbox" } : state.view,
    }));
  },
  showSettings: (section = "general") => {
    set({ view: { kind: "settings", section }, ...(section === "accounts" && { accountsPage: { kind: "list" } }) });
  },
  showAccountsPage: (accountsPage) => {
    set({ view: { kind: "settings", section: "accounts" }, accountsPage });
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
}));
