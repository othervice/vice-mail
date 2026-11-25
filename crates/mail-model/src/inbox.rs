//! Stage 9 (decision 0020): the inbox with categories, bundles, pins, snoozes and tasks; the
//! user's sender rules and local rules; inbox, notification and badge settings; unsubscribing.
//! Types only; the store keeps them, the engine applies them.
//!
//! Marks that travel between devices (pins, snoozes, tasks, rules) refer to a message by
//! account UUID and Message-ID, never by a local id (`docs/ARCHITECTURE.md`, "Data model"). A
//! message without a Message-ID can therefore not be pinned, snoozed or marked as a task.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{
    AccountId, BundleSummary, CategoryDecision, ListCursor, MailboxId, MessageCategory, MessageId,
    MessageListItem, SenderKey, UnifiedView,
};

// ---------------------------------------------------------------------------------------------
// Inbox lists

/// Which inbox a view shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InboxScope {
    /// One mailbox, usually an account's inbox.
    Mailbox { mailbox_id: MailboxId },
    /// The unified inbox, or the inboxes of a group (hidden accounts left out).
    Unified { view: UnifiedView },
}

/// Date section of a row, in the user's time zone (`docs/DESIGN.md`: Heute, Gestern, Diese
/// Woche, then by month). Rows of one section are contiguous in the newest-first list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DateSectionKind {
    /// Today, and anything dated in the future.
    Today,
    Yesterday,
    /// Earlier this week (from Monday), before yesterday.
    ThisWeek,
    /// The calendar month of `start`.
    Month,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DateSection {
    pub kind: DateSectionKind,
    /// Start of the section (local midnight) in Unix milliseconds.
    #[specta(type = Number)]
    pub start: i64,
}

/// One row of the inbox list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InboxRow {
    /// A message of its own (people, important senders, and everything when categories are
    /// off).
    Message {
        item: MessageListItem,
        section: DateSection,
    },
    /// Notifications or newsletters of one date section as one row; it stands where its newest
    /// member is. `members` are the members on this page, newest first; a click expands them in
    /// place. A bundle with one member may be drawn like a normal row.
    Bundle {
        summary: BundleSummary,
        section: DateSection,
        members: Vec<MessageListItem>,
    },
    /// More members of a bundle an earlier page started: append them to that row (same
    /// `summary.key`) and replace its summary with this one.
    More {
        summary: BundleSummary,
        members: Vec<MessageListItem>,
    },
}

/// What stands above the date sections. Only on the first page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxTop {
    /// Pinned messages of the scope, newest first (at most [`MAX_TOP_PINNED`]). They are not
    /// repeated in the rows.
    pub pinned: Vec<MessageListItem>,
    /// Snoozed messages that came back, most recently returned first (at most
    /// [`MAX_TOP_RETURNED`]). Not repeated in the rows; they stay here until the user clears
    /// the mark (`clearSnooze`), snoozes them again or they leave the inbox.
    pub returned: Vec<MessageListItem>,
    /// The newest messages of important senders (at most [`PRIORITY_PREVIEW`]); they also
    /// stand in the rows, never bundled. Empty when categories are off.
    pub priority: Vec<MessageListItem>,
    /// All messages of important senders in the scope ("Alle anzeigen (n)").
    pub priority_total: u32,
    pub priority_unread: u32,
    /// Stage 20 (decision 0059): messages the user sent that got no reply by the time they
    /// asked to be reminded ("No reply since …"), most recently due first (at most
    /// [`crate::MAX_TOP_FOLLOW_UPS`]), each shown where it lies (usually Sent). They stay until
    /// the user clears the reminder or a reply comes.
    pub follow_ups: Vec<MessageListItem>,
}

/// Messages of important senders shown in the "Priority" block.
pub const PRIORITY_PREVIEW: u32 = 3;
/// Pinned messages shown above the inbox.
pub const MAX_TOP_PINNED: u32 = 100;
/// Returned snoozes shown above the inbox.
pub const MAX_TOP_RETURNED: u32 = 100;

/// Keyset cursor of the inbox: the message cursor plus the bundles the current date section
/// has started (opaque; pass it back unchanged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxCursor {
    pub before: ListCursor,
    /// Serialised bundle state, null when categories are off or nothing is open.
    pub bundles: Option<String>,
}

/// One page of the inbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InboxPage {
    pub rows: Vec<InboxRow>,
    /// Null on the last page.
    pub next_cursor: Option<InboxCursor>,
    /// Pinned, returned and priority messages; only on the first page (no cursor given).
    pub top: Option<InboxTop>,
    /// Categories are on: notifications and newsletters are bundled. Off: the rows are purely
    /// chronological.
    pub categories: bool,
    /// Decision 0039 (`listInboxConversations`): every message of the rows and of `top` stands
    /// for its conversation; one summary per such message. Empty for `listInbox`.
    pub conversations: Vec<crate::ConversationSummary>,
}

/// A filtered inbox list ("Alle anzeigen", a click on a bundle's category).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum InboxFilter {
    /// Messages of one category (after the user's sender rules).
    Category { category: MessageCategory },
    /// Messages of important senders.
    Priority,
    /// Stage 16: unread messages only ("Nur ungelesene"), newest first.
    Unread,
}

/// The inbox settings (stage 9). Device settings, stored in the `setting` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct InboxSettings {
    /// Off: no bundles and no priority block, the inbox is purely chronological.
    pub categories: bool,
    pub bundle_notifications: bool,
    pub bundle_newsletters: bool,
}

impl Default for InboxSettings {
    fn default() -> Self {
        Self {
            categories: true,
            bundle_notifications: true,
            bundle_newsletters: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Marks on list rows

/// A snooze on a list row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeMark {
    /// When the message comes (or came) back, Unix milliseconds.
    #[specta(type = Number)]
    pub until: i64,
    /// It came back and waits for the user (shown on top with a mark).
    pub returned: bool,
}

/// A task mark on a list row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskMark {
    pub done: bool,
}

// ---------------------------------------------------------------------------------------------
// Snooze, pins, tasks

/// When a snoozed message comes back. Times are computed in the user's time zone; daylight
/// saving changes are taken into account (a local time that does not exist moves forward).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SnoozeChoice {
    /// Three hours from now, rounded up to the full hour; only while that is still today.
    LaterToday,
    /// Tomorrow at 08:00.
    Tomorrow,
    /// Next Saturday at 09:00 (on a weekend: the following one).
    Weekend,
    /// Next Monday at 08:00.
    NextWeek,
    /// A time the user picked, Unix milliseconds; at least a minute ahead, at most 5 years.
    At {
        #[specta(type = Number)]
        at: i64,
    },
}

/// The times the snooze menu offers right now, Unix milliseconds. `laterToday` is null late in
/// the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeTimes {
    #[specta(type = Option<Number>)]
    pub later_today: Option<i64>,
    #[specta(type = Number)]
    pub tomorrow: i64,
    #[specta(type = Number)]
    pub weekend: i64,
    #[specta(type = Number)]
    pub next_week: i64,
}

/// What `snoozeMessages` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeResult {
    /// When the messages come back, Unix milliseconds.
    #[specta(type = Number)]
    pub until: i64,
}

/// Which tasks the "Aufgaben" view lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TaskFilter {
    #[default]
    Open,
    Done,
    All,
}

/// The accounts a view of pinned, snoozed or task messages covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MarkScope {
    /// Every visible account.
    All,
    /// The visible accounts of a group.
    Group { group_id: crate::AccountGroupId },
    /// One account (also a hidden one).
    Account { account_id: AccountId },
}

// ---------------------------------------------------------------------------------------------
// Categories: why, and the user's sender rules

/// "Why here?" for one message, and the keys an "always as …" can use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageCategoryInfo {
    pub message_id: MessageId,
    /// Null while the message is not classified yet.
    pub decision: Option<CategoryDecision>,
    /// The normalised sender address, its domain and the list, for "always as …" (each null
    /// when the message has none).
    pub address: Option<SenderKey>,
    pub domain: Option<SenderKey>,
    pub list: Option<SenderKey>,
}

/// A rule for a sender, domain or list: the category correction ("always as …") and the
/// important mark. Synced (class sync).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SenderRule {
    /// UUID, lower case.
    pub id: String,
    /// The key as normalised for comparing (lower case, ASCII domain).
    pub key: SenderKey,
    /// The correction; null if the rule only marks the sender important.
    pub category: Option<MessageCategory>,
    pub important: bool,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------------------------
// Local rules (SPEC: "Lokale Regeln")

/// Stable id of a local rule (UUID v4, lower case), synced like the rule.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct RuleId(pub String);

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// How a text condition compares (case and accents ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TextMatchOp {
    Contains,
    /// The whole value (for addresses: the address).
    Is,
    /// Addresses: the domain or a subdomain of it.
    Domain,
}

/// A condition of a local rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "field",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuleCondition {
    /// The From address or name.
    From {
        op: TextMatchOp,
        value: String,
    },
    /// Any To or Cc address or name.
    To {
        op: TextMatchOp,
        value: String,
    },
    Subject {
        op: TextMatchOp,
        value: String,
    },
    /// The `List-Id` identifier (`contains` or `is`).
    List {
        op: TextMatchOp,
        value: String,
    },
    /// The message arrived in this account.
    Account {
        account_id: AccountId,
    },
}

/// An action of a local rule. Moving and marking as read are writing actions: they go through
/// the outbox and are skipped (and reported) on read-only accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuleAction {
    /// To a mailbox of the message's account, by decoded path (stable across devices).
    Move {
        account_id: AccountId,
        path: String,
    },
    MarkRead,
    /// Sets the message's category (this message only; "always as …" is a sender rule).
    SetCategory {
        category: MessageCategory,
    },
    Pin,
    /// Notify about the message, or not, whatever the notification settings say.
    Notify {
        enabled: bool,
    },
}

/// A local rule: runs on new mail in the inbox while the app runs (labelled so in the UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MailRule {
    pub id: RuleId,
    pub name: String,
    pub enabled: bool,
    /// All conditions must hold (`true`) or any (`false`). No condition: every message.
    pub match_all: bool,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    /// Later rules are not evaluated for a message this rule matched.
    pub stop: bool,
    /// Order: lower runs first.
    #[specta(type = Number)]
    pub position: i64,
}

/// What the UI sends to create or change a local rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub name: String,
    pub enabled: bool,
    pub match_all: bool,
    pub conditions: Vec<RuleCondition>,
    pub actions: Vec<RuleAction>,
    pub stop: bool,
}

/// Most local rules.
pub const MAX_RULES: usize = 200;
/// Most conditions and actions per rule.
pub const MAX_RULE_PARTS: usize = 20;
/// Longest rule name and condition value, in characters.
pub const MAX_RULE_TEXT: usize = 200;

/// Why a rule's action did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuleSkipReason {
    /// The account is read-only; moving and marking need writing.
    ReadOnly,
    /// The target mailbox does not exist (any more) or belongs to another account.
    MissingMailbox,
    /// The message has no Message-ID, so it cannot be pinned.
    NoMessageId,
    /// The outbox refused the action (detail in the log).
    Failed,
}

// ---------------------------------------------------------------------------------------------
// Notifications and the badge

/// Quiet hours: no sound and no banner between `start` and `end` (minutes after local
/// midnight; may wrap midnight). Notifications are dropped, not delayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QuietHours {
    pub enabled: bool,
    pub start_minute: u16,
    pub end_minute: u16,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            start_minute: 22 * 60,
            end_minute: 7 * 60,
        }
    }
}

impl QuietHours {
    /// Whether `minute` (after local midnight) lies inside. Equal start and end: never.
    pub fn contains(&self, minute: u16) -> bool {
        if !self.enabled || self.start_minute == self.end_minute {
            return false;
        }
        if self.start_minute < self.end_minute {
            (self.start_minute..self.end_minute).contains(&minute)
        } else {
            minute >= self.start_minute || minute < self.end_minute
        }
    }
}

/// Which new mail is announced (by category after the user's rules; a rule's `notify` action
/// overrides).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifyCategories {
    pub people: bool,
    /// Important senders, whatever their category.
    pub priority: bool,
    pub notifications: bool,
    pub newsletters: bool,
}

impl Default for NotifyCategories {
    fn default() -> Self {
        Self {
            people: true,
            priority: true,
            notifications: false,
            newsletters: false,
        }
    }
}

/// What the Dock (macOS) or taskbar (Windows) icon counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BadgeMode {
    Off,
    /// Unread messages in the inboxes of the visible accounts.
    #[default]
    InboxUnread,
    /// Unread messages of people (and unclassified ones) in those inboxes.
    PeopleUnread,
}

/// Notification settings (device settings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSettings {
    /// Off: no notification at all.
    pub enabled: bool,
    pub new_mail: bool,
    pub categories: NotifyCategories,
    pub failed_sends: bool,
    pub snoozes: bool,
    pub quiet_hours: QuietHours,
    /// Show "New message" instead of sender and subject. The system decides separately whether
    /// previews appear on the lock screen (macOS: Notifications, "Show previews"; Windows:
    /// Notifications, "Show notifications on the lock screen").
    pub hide_content: bool,
    /// Accounts whose new mail is never announced.
    pub muted_accounts: Vec<AccountId>,
    pub badge: BadgeMode,
    /// Stage 16: the sound notifications make (settings stored before have the default).
    pub sound: NotificationSound,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            new_mail: true,
            categories: NotifyCategories::default(),
            failed_sends: true,
            snoozes: true,
            quiet_hours: QuietHours::default(),
            hide_content: false,
            muted_accounts: Vec::new(),
            badge: BadgeMode::default(),
            sound: NotificationSound::default(),
        }
    }
}

/// System sounds a notification may play on macOS: the alert sounds of
/// `/System/Library/Sounds`, and `Default` (the system's notification sound).
pub const MACOS_SOUNDS: &[&str] = &[
    "Default",
    "Basso",
    "Blow",
    "Bottle",
    "Frog",
    "Funk",
    "Glass",
    "Hero",
    "Morse",
    "Ping",
    "Pop",
    "Purr",
    "Sosumi",
    "Submarine",
    "Tink",
];

/// Sounds a notification may play on Windows: the toast sounds `Default`, `IM`, `Mail`,
/// `Reminder` and `SMS` (`ms-winsoundevent:Notification.*`).
pub const WINDOWS_SOUNDS: &[&str] = &["Default", "IM", "Mail", "Reminder", "SMS"];

/// The platform a list of notification sounds is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SoundPlatform {
    Macos,
    Windows,
    /// Neither: the system's notification sound, no choice.
    Other,
}

impl SoundPlatform {
    /// The platform this build runs on.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }

    /// The sounds to choose from (empty for [`SoundPlatform::Other`]).
    pub const fn sounds(self) -> &'static [&'static str] {
        match self {
            Self::Macos => MACOS_SOUNDS,
            Self::Windows => WINDOWS_SOUNDS,
            Self::Other => &[],
        }
    }
}

/// Stage 16: the sound of notifications. One setting for all devices (it is part of the synced
/// `notifications.settings`), with a choice per platform since the systems have different
/// sounds. Quiet hours drop the notification and with it the sound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationSound {
    /// Off: every notification is silent.
    pub enabled: bool,
    /// One of [`MACOS_SOUNDS`]; an unknown name plays `Default`.
    pub macos: String,
    /// One of [`WINDOWS_SOUNDS`]; an unknown name plays `Default`.
    pub windows: String,
    /// Accounts whose notifications show without a sound.
    pub silent_accounts: Vec<AccountId>,
}

impl Default for NotificationSound {
    fn default() -> Self {
        Self {
            enabled: true,
            macos: "Default".into(),
            windows: "Default".into(),
            silent_accounts: Vec::new(),
        }
    }
}

impl NotificationSound {
    /// The sound a notification about `account` plays on `platform`: `None` when sounds are
    /// off or the account is silent; an unknown name (a newer version's choice) plays
    /// `Default`.
    pub fn for_account(&self, account: AccountId, platform: SoundPlatform) -> Option<String> {
        if !self.enabled || self.silent_accounts.contains(&account) {
            return None;
        }
        let chosen = match platform {
            SoundPlatform::Macos => self.macos.as_str(),
            SoundPlatform::Windows => self.windows.as_str(),
            SoundPlatform::Other => "Default",
        };
        let known = platform.sounds().contains(&chosen);
        Some(if known { chosen } else { "Default" }.to_owned())
    }

    /// The first name that is not in its platform's list, if any.
    pub fn unknown_name(&self) -> Option<&str> {
        [
            (self.macos.as_str(), MACOS_SOUNDS),
            (self.windows.as_str(), WINDOWS_SOUNDS),
        ]
        .into_iter()
        .find(|(name, list)| !list.contains(name))
        .map(|(name, _)| name)
    }
}

/// The notification sounds of this platform, for the settings page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSoundChoices {
    pub platform: SoundPlatform,
    /// The names to offer, `Default` first; empty on other platforms.
    pub sounds: Vec<String>,
}

/// The texts of system notifications, in the UI's language. The UI sends them from
/// `app/src/i18n/` at start and when the language changes (`setNotificationTexts`); until then
/// the English defaults apply. `{count}` is replaced by a number, `{subject}` by a subject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationTexts {
    /// Title when content is hidden, or for one message without a sender.
    pub new_message: String,
    /// Title for several messages at once.
    pub new_messages: String,
    /// Body when the subject is empty.
    pub no_subject: String,
    /// Title of a failed send; the body is the subject.
    pub send_failed: String,
    /// Title of a returned snooze; the body is the subject (or hidden).
    pub snooze_due: String,
}

impl Default for NotificationTexts {
    fn default() -> Self {
        Self {
            new_message: "New message".into(),
            new_messages: "{count} new messages".into(),
            no_subject: "(no subject)".into(),
            send_failed: "Message not sent".into(),
            snooze_due: "Snoozed message is back".into(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Unsubscribing (RFC 2369, RFC 8058)

/// What a message offers for unsubscribing, from `List-Unsubscribe` and
/// `List-Unsubscribe-Post`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeOptions {
    /// One-click (RFC 8058): an HTTPS POST the core sends after the click. Shows the host.
    pub one_click_host: Option<String>,
    /// A `mailto:` address: a prepared draft the user sends.
    pub mailto: Option<String>,
    /// An `http`/`https` page that only opens in the browser, after the user confirmed.
    pub web: Option<String>,
}

impl UnsubscribeOptions {
    pub fn is_empty(&self) -> bool {
        self.one_click_host.is_none() && self.mailto.is_none() && self.web.is_none()
    }
}

/// What the reading pane shows besides the body (stage 9): the category ("Why here?"),
/// unsubscribing, calendar invitations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageExtras {
    pub category: MessageCategoryInfo,
    /// Empty when the message offers nothing.
    pub unsubscribe: UnsubscribeOptions,
    /// Invitations of the `text/calendar` parts (`partId` for `openInvite`). Empty while the
    /// body is not stored whole (`bodyComplete` false): ask again after
    /// `messageBodyAvailable`.
    pub invites: Vec<CalendarInviteInfo>,
    pub body_complete: bool,
}

/// One calendar part of a message, read for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CalendarInviteInfo {
    pub part_id: u32,
    pub invite: crate::CalendarInvite,
}

/// How the user chose to unsubscribe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum UnsubscribeMethod {
    OneClick,
    Mailto,
    Web,
}

/// The result of an unsubscribe click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UnsubscribeOutcome {
    /// The list's server accepted the one-click request (HTTP 2xx).
    Requested { status: u16 },
    /// Open the composer with these fields; the user sends the request.
    Compose { request: crate::ComposeRequest },
    /// The page was handed to the system browser.
    Opened,
}

// ---------------------------------------------------------------------------------------------
// The default mail app

/// Whether the app is the system's handler of `mailto:` links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DefaultMailApp {
    /// Null when the app cannot tell (Windows keeps the choice to itself).
    pub is_default: Option<bool>,
    /// The app can make itself the default (macOS); otherwise `setDefaultMailApp` opens the
    /// system's settings (Windows: Settings, Default apps).
    pub direct: bool,
}

/// What `setDefaultMailApp` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DefaultMailAppOutcome {
    /// The app is the default now.
    Done,
    /// The system's settings are open; the user picks the app there.
    OpenedSettings,
    /// It did not work (for example a build that is not installed as an app); the UI explains
    /// the system's own way (macOS: Mail, Settings, General, "Default email reader").
    Manual,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn account(n: u8) -> AccountId {
        format!("00000000-0000-4000-8000-0000000000{n:02}")
            .parse()
            .expect("uuid")
    }

    /// Stage 16: settings stored before have the default sound; the stored shape is camelCase.
    #[test]
    fn notification_settings_of_stage_9_get_the_default_sound() {
        let old: NotificationSettings =
            serde_json::from_value(json!({ "enabled": true, "hideContent": true }))
                .expect("stage 9 settings");
        assert!(old.hide_content);
        assert_eq!(old.sound, NotificationSound::default());
        let value = serde_json::to_value(NotificationSettings::default()).expect("json");
        assert_eq!(
            value["sound"],
            json!({ "enabled": true, "macos": "Default", "windows": "Default",
                    "silentAccounts": [] })
        );
    }

    #[test]
    fn the_sound_depends_on_switch_account_and_platform() {
        let mut sound = NotificationSound {
            macos: "Glass".into(),
            windows: "Mail".into(),
            ..NotificationSound::default()
        };
        assert_eq!(sound.unknown_name(), None);
        let at = |s: &NotificationSound, p| s.for_account(account(1), p);
        assert_eq!(at(&sound, SoundPlatform::Macos).as_deref(), Some("Glass"));
        assert_eq!(at(&sound, SoundPlatform::Windows).as_deref(), Some("Mail"));
        assert_eq!(at(&sound, SoundPlatform::Other).as_deref(), Some("Default"));
        sound.silent_accounts = vec![account(1)];
        assert_eq!(at(&sound, SoundPlatform::Macos), None, "silent account");
        assert_eq!(
            sound
                .for_account(account(2), SoundPlatform::Macos)
                .as_deref(),
            Some("Glass")
        );
        sound.silent_accounts.clear();
        sound.enabled = false;
        assert_eq!(at(&sound, SoundPlatform::Windows), None, "sounds off");
        // A name from elsewhere (a newer version, the other platform's list) plays the default.
        sound.enabled = true;
        sound.macos = "Mail".into();
        assert_eq!(sound.unknown_name(), Some("Mail"));
        assert_eq!(at(&sound, SoundPlatform::Macos).as_deref(), Some("Default"));
        assert!(MACOS_SOUNDS[0] == "Default" && WINDOWS_SOUNDS[0] == "Default");
    }
}
