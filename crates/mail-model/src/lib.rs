//! Types shared by every layer: ids, accounts, mailboxes, message headers, flags,
//! capabilities, errors, the [`Secret`] wrapper, the [`MailBackend`] trait and its writing
//! extension [`MailWriter`], writing actions and outbox operations, role detection,
//! message bodies and their MIME structure, and the types that cross the IPC bridge. Pure data and logic: no IO, no dependency on other
//! workspace crates.
//!
//! IPC types derive `serde` and `specta::Type`; the shell exports them to TypeScript with
//! tauri-specta (decision 0004). Structs use camelCase on the wire, error codes snake_case.
//! Types documented as internal (adapter results, sync state) never cross the bridge.

mod account;
mod backend;
mod body;
mod bundle;
mod calendar;
mod capabilities;
mod category;
mod compose;
mod error;
mod ids;
mod inbox;
mod ipc;
mod mailbox;
mod mailto;
mod message;
mod outbox;
mod reading;
mod role;
pub mod search;
mod secret;
mod send;
pub mod threading;
mod uid_set;
mod views;
mod writer;

pub use account::{
    Account, AccountColor, AccountSettings, AccountState, AccountStatus, ConnectionTestResult,
    DiscoveryResult, DiscoverySource, Identity, ProviderHint, Security, ServerSettings,
    SyncProgress,
};
pub use backend::{
    Access, ConnectConfig, ExtraRootCa, IdleOutcome, MailBackend, MailConnector, SelectedMailbox,
    Timeouts,
};
pub use body::{
    BodyFetchPlan, BodySection, Disposition, FetchedSection, LARGE_MESSAGE_THRESHOLD,
    MessageStructure, MimePart, PartPath, RawMessage,
};
pub use bundle::{
    Bundle, BundleItem, BundleKey, BundleOptions, BundleRow, BundleState, BundleSummary,
    DEFAULT_BUNDLE_CHIPS, MAX_BUNDLE_SENDERS, SenderChip, bundle_page,
};
pub use calendar::{
    AttendeeRole, CalendarEvent, CalendarInvite, CalendarPerson, CalendarProblem,
    CalendarProblemCode, CalendarRecurrence, CalendarTime, EventStatus, ParticipationStatus,
    RecurrenceFrequency,
};
pub use capabilities::Capabilities;
pub use category::{
    CategoryCorrection, CategoryDecision, CategoryReason, MessageCategory, SenderKey, SenderKeyKind,
};
pub use compose::{
    AccountSendSettings, ComposeFormat, ComposeIdentity, ComposeSettings, ContactNameRecord,
    ContactSuggestion, DEFAULT_SEND_DELAY_SECS, Draft, DraftAttachment, DraftError, DraftFile,
    DraftId, DraftKind, DraftServerState, DraftState, DraftSummary, DraftUpdate,
    LARGE_ATTACHMENT_WARNING_BYTES, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, MAX_CONTACT_SUGGESTIONS,
    MAX_DRAFT_BODY_BYTES, MAX_RECIPIENTS, MAX_SEND_DELAY_SECS, MAX_SUBJECT_CHARS,
    MAX_TEXT_SIGNATURE_BYTES, NewDraft, OriginalRef, OutgoingMail, QuoteLanguage, QuotePosition,
    SendJob, SendPhase, SendState, SentCopyMode, ServerCopy, UnclearDecision,
};
pub use error::{BackendError, CoreError, ErrorCode, IpcError};
pub use ids::{AccountId, IdentityId, ListCursor, MailboxId, MessageId, ModSeq, Uid, UidValidity};
pub use inbox::{
    BadgeMode, CalendarInviteInfo, DateSection, DateSectionKind, DefaultMailApp,
    DefaultMailAppOutcome, InboxCursor, InboxFilter, InboxPage, InboxRow, InboxScope,
    InboxSettings, InboxTop, MAX_RULE_PARTS, MAX_RULE_TEXT, MAX_RULES, MAX_TOP_PINNED,
    MAX_TOP_RETURNED, MailRule, MarkScope, MessageCategoryInfo, MessageExtras,
    NotificationSettings, NotificationTexts, NotifyCategories, PRIORITY_PREVIEW, QuietHours,
    RuleAction, RuleCondition, RuleId, RuleInput, RuleSkipReason, SenderRule, SnoozeChoice,
    SnoozeMark, SnoozeResult, SnoozeTimes, TaskFilter, TaskMark, TextMatchOp, UnsubscribeMethod,
    UnsubscribeOptions, UnsubscribeOutcome,
};
pub use ipc::{
    AppInfo, CoreEvent, LanguagePreference, MARK_READ_DELAY_MS, MarkReadOnOpen, Platform,
    Preferences, ReadingPreferences, ThemePreference,
};
pub use mailbox::{
    Mailbox, MailboxRole, MailboxStatus, RemoteMailbox, RoleSource, is_selectable, leaf_name,
};
pub use mailto::{ComposeRequest, MailtoField, MailtoNotice};
pub use message::{
    Address, DATE_HEADER_TOLERANCE, FlagUpdate, Flags, MAX_SORT_DATE, MessageActivity,
    MessageHeader, MessageListItem, MessagePage, sort_date,
};
pub use outbox::{
    ActionError, ActionKind, ActionResult, AppendSource, BatchId, ConflictReason, Flag,
    MAX_ACTION_MESSAGES, MAX_KEYWORD_LEN, MailAction, MessageRef, OperationId, OperationInfo,
    OperationKind, OperationNotice, OperationPayload, OperationState, OutboxCounts, OutboxEvent,
    PlanLocation, PlanMailbox, SourceLocation, TargetMailbox, TargetMark, is_valid_keyword,
    plan_action,
};
pub use reading::{
    AttachmentInfo, BodyFormat, BodyUnavailableReason, LINK_PLACEHOLDER_HOST, LinkInfo,
    LinkWarning, MessageBody, MessageProblem, MessageView, MessageViewHeader, OpenAttachmentResult,
    PrefetchPeriod, PrefetchSettings, PrefetchState, PrefetchStatus, RemoteContentScope,
    RenderToken, SaveAttachmentResult, TextSegment, link_placeholder, normalize_sender,
    parse_link_placeholder,
};
pub use role::{RoleAssignment, RoleInput, assign_roles};
pub use secret::Secret;
pub use send::{
    Envelope, MailSender, RecipientRefusal, SendFuture, SendOutcome, SendRejection, SmtpReply,
    SmtpStep,
};
pub use uid_set::UidSet;
pub use views::{
    AccountGroup, AccountGroupId, IdentityInfo, IdentityInput, MAX_GROUP_NAME_CHARS,
    MAX_SEARCH_PAGE, MAX_THREAD_MESSAGES, SearchHighlights, SearchHit, SearchIndexState,
    SearchIndexStatus, SearchPage, SearchProblem, SearchProblemCode, SearchRequest, SearchScope,
    SearchTermInfo, SearchTermKind, ServerSearchResult, TextRange, ThreadEntry, ThreadListItem,
    ThreadPage, ThreadView, UnifiedView, utf16_range,
};
pub use writer::{
    AppendUid, CopyUid, ExpungeOutcome, MailWriter, MoveMethod, MoveOutcome, StoreMode,
    StoreOutcome, WriteConnector,
};
