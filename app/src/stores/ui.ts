import { create } from "zustand";

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

export type View =
  | { kind: "mail"; mailView: MailViewId }
  | { kind: "settings"; section: SettingsSectionId };

interface UiState {
  view: View;
  showMailView: (mailView: MailViewId) => void;
  showSettings: (section?: SettingsSectionId) => void;
}

/** Pure UI state (which view is open). Data from the core lives in TanStack Query, not here. */
export const useUiStore = create<UiState>()((set) => ({
  view: { kind: "mail", mailView: "inbox" },
  showMailView: (mailView) => {
    set({ view: { kind: "mail", mailView } });
  },
  showSettings: (section = "general") => {
    set({ view: { kind: "settings", section } });
  },
}));
