import type { ParseKeys } from "i18next";
import type { SettingsSectionId } from "../../../stores/ui";

/*
 * Stage 22, item 8 (decision 0073): what the settings search finds. One entry per row of the
 * settings that has a title of its own (a switch, a choice, a field, a block or a further section
 * with its heading), as the i18n keys the row renders: the texts exist once, in `app/src/i18n/`,
 * and the search shows them in the language the UI speaks. The rows mark themselves with their
 * title (`components/settingAnchor.ts`); `tests/settings-search.test.tsx` renders every section
 * and fails for a row that is missing here or says something else than its entry, so a new
 * setting cannot be forgotten.
 *
 * Not here: the controls in a row of the user's own things (a signature, a rule, an account's
 * row in a block); the block's heading stands for them. The pages of a single account (Settings →
 * Accounts → one account) and the actions of a page (buttons) are no options of their own.
 */

type Key = ParseKeys;

export interface SettingsIndexEntry {
  section: SettingsSectionId;
  /** The row's title as it shows it (`data-setting-title`). */
  title: Key;
  /**
   * What the row says under its title; several where it depends on the platform or the state
   * (any of them is searched and the first shown).
   */
  description?: Key | readonly Key[];
  /**
   * The row is told apart by its key (`data-setting-key`): another row of the section shows the
   * same title.
   */
  byKey?: true;
}

export const SETTINGS_INDEX: readonly SettingsIndexEntry[] = [
  // General
  { section: "general", title: "settings.general.language.label", description: "settings.general.language.description" },
  { section: "general", title: "extras.defaultApp.title", description: "extras.defaultApp.description" },
  { section: "general", title: "desktop.background.title" },
  {
    section: "general",
    title: "desktop.background.keepRunning.label",
    description: ["desktop.background.keepRunning.macos", "desktop.background.keepRunning.windows"],
  },
  { section: "general", title: "desktop.background.menuBarIcon.label", description: "desktop.background.menuBarIcon.description" },
  {
    section: "general",
    title: "desktop.background.autostart.label",
    description: ["desktop.background.autostart.description", "desktop.background.autostart.unavailable"],
  },
  // Accounts (the list; a single account's page is reached from it)
  { section: "accounts", title: "groups.title", description: "groups.description" },
  // Reading
  { section: "reading", title: "settings.reading.markRead.title", description: "settings.reading.markRead.description" },
  { section: "reading", title: "settings.reading.markRead.label" },
  { section: "reading", title: "threads.setting.title", description: "threads.setting.description" },
  { section: "reading", title: "threads.setting.label" },
  { section: "reading", title: "settings.reading.darkMails.title", description: "settings.reading.darkMails.description" },
  { section: "reading", title: "settings.reading.darkMails.label" },
  { section: "reading", title: "settings.reading.trackers.title", description: "settings.reading.trackers.description" },
  { section: "reading", title: "settings.reading.trackers.block", description: "settings.reading.trackers.blockHint" },
  { section: "reading", title: "settings.reading.trackers.cleanLinks", description: "settings.reading.trackers.cleanLinksHint" },
  { section: "reading", title: "settings.reading.prefetch.title", description: "settings.reading.prefetch.description" },
  { section: "reading", title: "settings.reading.prefetch.period.label", description: "settings.reading.prefetch.period.description" },
  { section: "reading", title: "settings.reading.prefetch.size.label", description: "settings.reading.prefetch.size.description" },
  { section: "reading", title: "settings.reading.remote.title", description: "settings.reading.remote.description" },
  // Writing
  { section: "compose", title: "composer.settings.delay", description: "composer.settings.delayHint" },
  { section: "compose", title: "writingDefaults.quotePosition.label", description: "writingDefaults.quotePosition.hint" },
  { section: "compose", title: "composer.settings.archiveOnReply", description: "composer.settings.archiveOnReplyHint" },
  { section: "compose", title: "composer.settings.attachmentReminder", description: "composer.settings.attachmentReminderHint" },
  { section: "compose", title: "composer.settings.readReceipts", description: "composer.settings.readReceiptsHint" },
  { section: "compose", title: "snippets.signatures.title", description: "snippets.signatures.hint" },
  { section: "compose", title: "composer.settings.sentCopy", description: ["composer.settings.appSaves", "composer.settings.serverSaves"] },
  { section: "compose", title: "snippets.defaults.hint" },
  { section: "compose", title: "snippets.templates.title", description: "snippets.templates.hint" },
  // Inbox
  { section: "inbox", title: "comfort.settings.categories.label", description: "comfort.settings.categories.description" },
  { section: "inbox", title: "comfort.settings.bundleNotifications.label", description: "comfort.settings.bundleNotifications.description" },
  { section: "inbox", title: "comfort.settings.bundleNewsletters.label", description: "comfort.settings.bundleNewsletters.description" },
  { section: "inbox", title: "organize.unread.settings.title", description: "organize.unread.settings.hint" },
  { section: "inbox", title: "comfort.senderRules.title", description: "comfort.senderRules.description" },
  { section: "inbox", title: "blocked.title", description: "blocked.description" },
  { section: "inbox", title: "comfort.rules.title", description: "comfort.rules.local" },
  // Notifications
  { section: "notifications", title: "comfort.notifications.enabled.label", description: "comfort.notifications.enabled.description", byKey: true },
  { section: "notifications", title: "desktop.pause.title", description: "desktop.pause.description" },
  { section: "notifications", title: "comfort.notifications.newMail.legend" },
  { section: "notifications", title: "comfort.notifications.newMail.label" },
  { section: "notifications", title: "comfort.notifications.style.label", description: "comfort.notifications.style.description" },
  { section: "notifications", title: "comfort.notifications.categories.people" },
  { section: "notifications", title: "comfort.notifications.categories.priority" },
  { section: "notifications", title: "comfort.notifications.categories.notifications", byKey: true },
  { section: "notifications", title: "comfort.notifications.categories.newsletters" },
  { section: "notifications", title: "comfort.notifications.failedSends" },
  { section: "notifications", title: "comfort.notifications.snoozes" },
  { section: "notifications", title: "comfort.notifications.quiet.label", description: "comfort.notifications.quiet.description" },
  { section: "notifications", title: "comfort.notifications.hideContent.label", description: "comfort.notifications.hideContent.description" },
  { section: "notifications", title: "comfort.notifications.accounts" },
  { section: "notifications", title: "sounds.legend", byKey: true },
  { section: "notifications", title: "sounds.enabled.label", description: "sounds.enabled.description" },
  { section: "notifications", title: "sounds.choice", description: "sounds.noPreview", byKey: true },
  { section: "notifications", title: "sounds.accounts" },
  { section: "notifications", title: "comfort.notifications.badge.label", description: "comfort.notifications.badge.description" },
  // Appearance
  { section: "appearance", title: "settings.appearance.theme.label", description: "settings.appearance.theme.description" },
  { section: "appearance", title: "themes.label", description: "themes.description" },
  { section: "appearance", title: "layout.readingPane.label", description: "layout.readingPane.description" },
  { section: "appearance", title: "layout.density.label", description: "layout.density.description" },
  { section: "appearance", title: "views.presentation.label", description: "views.presentation.description" },
  { section: "appearance", title: "layout.textSize.label", description: "layout.textSize.description" },
  // Keyboard
  { section: "keyboard", title: "keyboard.settings.groups.actions" },
  { section: "keyboard", title: "keyboard.settings.groups.commands" },
  // Settings sync (off, or on; the blocks of the other state are not shown)
  { section: "sync", title: "settingsSync.enable.title", description: "settingsSync.enable.description" },
  { section: "sync", title: "settingsSync.join.title" },
  { section: "sync", title: "settingsSync.status.title" },
  { section: "sync", title: "settingsSync.needsPassphrase.title" },
  { section: "sync", title: "settingsSync.passwords.title", description: "settingsSync.passwords.description" },
  { section: "sync", title: "settingsSync.questions.title" },
  { section: "sync", title: "settingsSync.devices.title" },
  { section: "sync", title: "settingsSync.change.title", description: "settingsSync.change.description" },
  { section: "sync", title: "settingsSync.reset.title", description: "settingsSync.reset.description" },
  { section: "sync", title: "settingsSync.disable.title", description: "settingsSync.disable.description" },
  // AI in the app
  { section: "ai", title: "ai.settings.enable.label", description: "ai.settings.enable.description" },
  { section: "ai", title: "ai.profiles.title", description: "ai.profiles.description" },
  { section: "ai", title: "ai.functions.title", description: "ai.functions.intro" },
  { section: "ai", title: "ai.functions.summary", description: "ai.functions.description.summary" },
  { section: "ai", title: "ai.functions.reply_suggestion", description: "ai.functions.description.reply_suggestion" },
  { section: "ai", title: "ai.functions.writing_help", description: "ai.functions.description.writing_help" },
  { section: "ai", title: "ai.functions.search", description: "ai.functions.description.search" },
  { section: "ai", title: "ai.accounts.title", description: "ai.accounts.description" },
  { section: "ai", title: "ai.openReplies.title", description: "ai.openReplies.description" },
  { section: "ai", title: "ai.openReplies.enable", description: "ai.openReplies.enableDescription" },
  { section: "ai", title: "ai.openReplies.days" },
  // AI over MCP
  { section: "mcp", title: "mcp.enable.label", description: "mcp.enable.description" },
  { section: "mcp", title: "mcp.status.title" },
  { section: "mcp", title: "mcp.accounts.title", description: "mcp.accounts.description" },
  { section: "mcp", title: "mcp.clients.title", description: "mcp.clients.description" },
  { section: "mcp", title: "mcp.setup.title", description: "mcp.setup.description" },
  // About
  { section: "about", title: "updates.title" },
  { section: "about", title: "updates.autoCheck", description: "updates.autoCheckHint" },
  { section: "about", title: "settings.about.diagnostics.title", description: "settings.about.diagnostics.intro" },
  { section: "about", title: "settings.about.licenses.title" },
];
