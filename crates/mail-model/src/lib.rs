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
mod ai;
mod backend;
mod body;
mod bundle;
mod calendar;
mod capabilities;
mod category;
mod compose;
mod contacts;
mod eml;
mod error;
mod ids;
mod inbox;
mod ipc;
mod mailbox;
mod mailto;
mod mcp;
mod message;
mod outbox;
mod reading;
mod role;
mod saved_search;
pub mod search;
mod secret;
mod send;
mod settings_sync;
mod snippets;
pub mod threading;
mod ui;
mod uid_set;
mod views;
mod writer;

pub use account::{
    Account, AccountColor, AccountSettings, AccountState, AccountStatus, ConnectionTestResult,
    DiscoveryResult, DiscoverySource, Identity, ProviderHint, Security, ServerSettings,
    SyncProgress,
};
pub use ai::{
    AI_REQUEST_MAX_ENTRIES, AI_REQUEST_RETENTION_DAYS, AiAccountCloud, AiAnswer, AiAvailability,
    AiClaim, AiConnectionTest, AiDeadline, AiFailure, AiFunction, AiFunctionChoice, AiKeyChange,
    AiLocality, AiModel, AiPreset, AiProfileInfo, AiProfileInput, AiProviderKind,
    AiReplySuggestion, AiRequestEntry, AiRequestId, AiRequestOutcome, AiRequestPage,
    AiRequestPhase, AiSettingsInfo, AiSummary, AiTargetLanguage, AiTone, AiTransfer,
    AiTransferInput, AiTrimming, AiWritingResult, AiWritingTask, MAX_AI_PASSAGE_CHARS,
    MAX_AI_PROFILE_NAME_CHARS, MAX_AI_PROFILES, MAX_AI_QUESTION_CHARS, MAX_AI_REQUEST_ID_CHARS,
    MAX_AI_REQUEST_PAGE, OpenReplyHint, OpenReplyKind, OpenReplySettings,
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
    AccountSendSettings, CleanedPaste, ComposeFormat, ComposeIdentity, ComposeSettings,
    ContactNameRecord, ContactSuggestion, DEFAULT_SEND_DELAY_SECS, Draft, DraftAttachment,
    DraftError, DraftFile, DraftId, DraftKind, DraftServerState, DraftSizeCheck, DraftState,
    DraftSummary, DraftUpdate, InlineImagesAdded, LARGE_ATTACHMENT_WARNING_BYTES,
    MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, MAX_CONTACT_SUGGESTIONS, MAX_DRAFT_BODY_BYTES,
    MAX_INLINE_IMAGE_BYTES, MAX_RECIPIENTS, MAX_SCHEDULE_AHEAD_MS, MAX_SEND_DELAY_SECS,
    MAX_SUBJECT_CHARS, MAX_TEXT_SIGNATURE_BYTES, MISSED_SEND_GRACE_MS, MissedSendDecision,
    NewDraft, OriginalRef, OutgoingMail, QuoteLanguage, QuotePosition, SendJob, SendLater,
    SendPhase, SendState, SentCopyMode, ServerCopy, UnclearDecision,
};
pub use contacts::{
    ContactAddressInfo, ContactDuplicate, ContactEntry, ContactMergeRequest, ContactPage,
    ContactRequestError, DuplicateReason, MAX_CONTACT_PAGE, MAX_DUPLICATE_SCAN,
};
pub use eml::{
    EmlError, EmlFile, EmlFileId, EmlMessageView, MAX_EML_FILE_BYTES, MAX_OPEN_EML_FILES,
    OpenEmlFileResult,
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
pub use mailto::{ComposeRequest, LinkOpened, MailtoField, MailtoNotice};
pub use mcp::{
    MAX_MCP_ACCESS_PAGE, MAX_MCP_CLIENT_NAME_CHARS, MAX_MCP_CLIENTS, MCP_ACCESS_MAX_ENTRIES,
    MCP_ACCESS_RETENTION_DAYS, McpAccessEntry, McpAccessId, McpAccessOutcome, McpAccessPage,
    McpAccountAccess, McpAccountShare, McpClientConfig, McpClientCreated, McpClientInfo,
    McpClientKind, McpConnectionTest, McpHelperInfo, McpHostState, McpRefusal, McpSettingsInfo,
    McpSetup, McpStatus, McpTestProblem, McpTool,
};
pub use message::{
    Address, DATE_HEADER_TOLERANCE, FlagUpdate, Flags, MAX_MESSAGES_BY_IDS, MAX_SORT_DATE,
    MessageActivity, MessageHeader, MessageListItem, MessagePage, sort_date,
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
pub use saved_search::{
    MAX_RECENT_SEARCHES, MAX_SAVED_SEARCH_NAME_CHARS, MAX_SAVED_SEARCHES, RecentSearch,
    SavedSearch, SavedSearchError, SavedSearchId, clean_saved_search_name, clean_search_query,
};
pub use secret::Secret;
pub use send::{
    Envelope, MailSender, RecipientRefusal, SendFuture, SendOutcome, SendRejection, SmtpLimits,
    SmtpReply, SmtpStep,
};
pub use settings_sync::{
    MIN_SYNC_PASSPHRASE_CHARS, SETTINGS_SYNC_FOLDER, SYNC_ID_HEADER, SYNC_VERSION_HEADER,
    SettingsSyncFailure, SettingsSyncPreview, SettingsSyncState, SettingsSyncStatus, SyncDevice,
    SyncFolder, SyncFolderConnector, SyncFolderEntry, SyncFolderState, SyncKindCount, SyncQuestion,
    is_settings_sync_folder,
};
pub use snippets::{
    FilledTemplateInfo, IdentitySignatureDefaults, MAX_SIGNATURE_IMAGE_BYTES, MAX_SIGNATURE_IMAGES,
    MAX_SIGNATURE_IMAGES_BYTES, MAX_SIGNATURES, MAX_SNIPPET_NAME_CHARS, MAX_TEMPLATES,
    SanitizeReport, SavedSignature, SavedTemplate, SignatureId, SignatureImageData,
    SignatureImageInfo, SignatureInfo, SignatureInput, TemplateFillProblem, TemplateId,
    TemplateInfo, TemplateInput,
};
pub use ui::{
    AiPanelState, DEFAULT_TEXT_SIZE, Density, MAX_COMMAND_ID_CHARS, MAX_LAYOUT_SLOT_CHARS,
    MAX_RECENT_COMMAND_CHARS, MAX_RECENT_COMMANDS, MAX_SHORTCUT_KEY_CHARS, MAX_SHORTCUT_OVERRIDES,
    MAX_SHORTCUTS_PER_COMMAND, MAX_TEXT_SIZE, MIN_TEXT_SIZE, PlatformShortcuts,
    ReadingPanePosition, Shortcut, ShortcutOverride, ShortcutPlatform, UiPreferences,
    UiPreferencesError, WindowLayout, validate_layout_slot, validate_overrides,
    validate_recent_commands,
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
