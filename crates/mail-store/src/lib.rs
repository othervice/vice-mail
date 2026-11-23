//! SQLite storage: the only crate with SQL.
//!
//! - One write connection on its own thread, a small pool of `query_only` reader threads,
//!   WAL mode and the pragmas from decision 0003. No call blocks a Tokio worker.
//! - Forward-only migrations with `rusqlite_migration` (schema version in `user_version`).
//! - A backup (`VACUUM INTO`) before every migration of an existing database; three are kept.
//! - A database newer than this build knows is rejected and left untouched.
//! - Accounts, mailboxes and message headers after decision 0003: `message.id` is the sort date
//!   (allocated on the writer thread), lists are keyset-paged, every list query has a plan test.
//! - Message bodies after decision 0009: zstd-compressed files under `bodies/` next to the
//!   database, written atomically, referenced from `message_body`; previews, prefetch settings
//!   and the remote-content allowlist.
//! - The outbox after decision 0012: operations with their local effects in one transaction,
//!   their states, undo, retry and discard (`outbox_api`).
//! - Stage 7 (`search_api`): the contentless FTS5 index kept with every message and body, its
//!   background fill and rebuild with a swap at the end, search with filters, threads kept when
//!   messages are stored, thread lists, the unified inbox, account groups, hidden accounts and
//!   identities.
//! - Stage 5 (`compose_api`): drafts, sends and their follow-ups in the outbox, the contact
//!   directory for autocomplete, identity signatures, who files sent mail (decision 0018).
//! - Stage 9 (`comfort_api`): categories classified when messages are stored (through the
//!   engine's [`Classifier`]) and the user's sender rules, the inbox lists without snoozed and
//!   pinned messages and per category, snoozes, pins, tasks, local rules (decision 0020).
//! - Stage 11 (`settings_sync`): every change of a synchronised table is captured in the same
//!   transaction (triggers mark it, the engine's [`SyncCapture`] versions it before commit);
//!   records of a pass are applied with a check against concurrent local changes
//!   (decision 0022).
//! - Stage 12 (`mcp`): AI access over MCP: the switch, shared accounts, registered clients
//!   (keys in the keychain) and the access log with its retention (decision 0021).
//! - Stage 15 (`diagnostics`): row counts and page statistics for the diagnostics export,
//!   numbers and schema names only.

mod accounts;
mod ai;
mod attachment_pins;
pub mod backup;
mod bodies;
mod body_api;
mod body_files;
mod categories;
mod comfort_api;
mod compose;
mod compose_api;
mod contacts;
mod contacts_dir;
mod conversations;
mod diagnostics;
mod error;
mod fts;
mod gmail;
mod groups;
mod inbox;
mod jmap;
mod lists;
mod mail_api;
mod mailboxes;
mod marks;
mod mcp;
mod messages;
mod migrations;
mod outbox;
mod outbox_api;
mod parity;
mod reading;
mod rules;
mod saved_searches;
mod schedule;
mod search;
mod search_api;
mod settings;
pub mod settings_sync;
mod signatures;
mod store;
mod threads;
mod workers;

pub use ai::{NewAiRequest, OpenReplyRow};
pub use attachment_pins::{AttachmentPinInput, PinnedAttachmentRow};
pub use bodies::BodyInfo;
pub use body_api::{BODY_PACK_LIMIT, SWEEP_MIN_AGE};
pub use body_files::SweepReport;
pub use categories::{
    CATEGORY_BATCH, Classified, Classifier, Effective, LocalFacts, RuleSet, SenderKeys,
    SenderRuleChange, StoredCategory,
};
pub use comfort_api::{CategoryStep, InboxMailboxes};
pub use compose::{MDN_SENT_KEYWORD, StoredReceiptAnswer};
pub use compose_api::{
    ContactSource, DraftContent, MAX_DRAFTS_PAGE, NewDraftRow, ReceivedReceipt, StoredContact,
    StoredDraft,
};
pub use contacts::MAX_SEARCH_WORDS;
pub use contacts_dir::{
    ContactCursor, MAX_CONTACT_ROWS, MergeOutcome, SeparateOutcome, StoredContactGroup,
    StoredContactPage,
};
pub use diagnostics::{AccountStats, DatabaseStats};
pub use error::StoreError;
pub use fts::{BodyText, BodyTextSource, MAX_BODY_TEXT_BYTES};
pub use groups::MailboxName;
pub use inbox::{ArrivedMessage, InboxListKind};
pub use mail_api::{
    MAX_PAGE_SIZE, MailboxBaseline, MailboxListReport, MailboxSyncState, NewAccount, StoreReport,
    StoredAccount,
};
pub use marks::{MAX_MARK_VIEW, MarkView, MessageKey, ReturnedSnooze, StoredSnooze, StoredTask};
pub use mcp::{NewMcpAccess, StoredMcpClient, StoredMcpState};
pub use migrations::SCHEMA_VERSION;
pub use outbox_api::{MAX_OPERATIONS_PAGE, OperationResult, OutboxChange, StoredOperation};
pub use parity::{TransferItem, TransferPlan};
pub use reading::{
    MAX_PREFETCH_BATCH, MAX_PREVIEW_CHARS, MessageLocation, PrefetchCandidate, StoredMessage,
};
pub use saved_searches::{SavedSearchWrite, StoredRecentSearch, StoredSavedSearch};
pub use schedule::{Reschedule, is_missed, is_waiting_schedule};
pub use search::{MAX_SEARCH_LIMIT, SearchFilter, StoredHit, StoredHitPage};
pub use search_api::{INDEX_BATCH, IndexStep};
pub use settings::{GLOBAL_SCOPE, Setting};
pub use settings_sync::{
    AccountServerChange, ApplyOutcome, ApplySkip, CaptureOutcome, CapturedChange, StoredSyncDevice,
    SyncApply, SyncCapture, SyncProgress, SyncRecordRow, SyncStateRow, TableWrite,
};
pub use signatures::{
    SignatureDefaults, SignatureRow, StoredSignature, StoredSignatureImage, StoredTemplate,
    TemplateRow,
};
pub use store::{BODIES_DIR, OpenReport, Store, StoreOptions};

#[cfg(test)]
mod bench;
#[cfg(test)]
mod body_bench;
#[cfg(test)]
mod body_tests;
#[cfg(test)]
mod compose_tests;
#[cfg(test)]
mod contacts_dir_tests;
#[cfg(test)]
mod conversation_tests;
#[cfg(test)]
mod gmail_tests;
#[cfg(test)]
mod mail_tests;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod plan_tests;
#[cfg(test)]
mod settings_sync_tests;
#[cfg(test)]
mod signature_tests;
#[cfg(test)]
mod stage16_tests;
#[cfg(test)]
mod stage7_plan_tests;
#[cfg(test)]
mod stage7_tests;
#[cfg(test)]
mod stage9_plan_tests;
#[cfg(test)]
mod stage9_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
