//! Writing actions and the outbox (stage 4, decision 0012).
//!
//! The UI asks for a [`MailAction`] on some messages ([`MessageRef`]). The core plans it into
//! operations, one per message and change ([`plan_action`]), applies their local effect at
//! once, records them in the outbox (`operation` table) in the same transaction, and executes
//! them on the server in order. Each operation has a visible [`OperationState`].
//!
//! Operations are formulated idempotently: "set `\Seen`" rather than "toggle", and a move
//! records enough (the target's UIDNEXT before sending) to tell afterwards whether it already
//! happened. The planner here is pure logic; the store runs it inside its transaction on
//! freshly read locations, so the plan never works on stale data.
//!
//! IPC-ready types ([`MailAction`], [`MessageRef`], [`OperationInfo`], [`OutboxEvent`], ...)
//! derive `specta::Type` but are not part of the exported bindings yet; the integration of
//! stage 4 registers them. [`OperationPayload`] and the planner inputs are internal.

use std::fmt;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{
    AccountId, CoreEvent, ErrorCode, Flags, IpcError, MailboxId, MailboxRole, MessageId, ModSeq,
    Uid, UidValidity,
};

/// Longest keyword the app sets. IMAP has no limit; this keeps a stray paste out of the server.
pub const MAX_KEYWORD_LEN: usize = 128;

/// A message in one mailbox, as the list shows it (`MessageListItem::mailbox_id` and `id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageRef {
    pub mailbox_id: MailboxId,
    pub message_id: MessageId,
}

/// What the user asks for. One action on many messages is one batch ([`BatchId`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum MailAction {
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    AddKeyword {
        keyword: String,
    },
    RemoveKeyword {
        keyword: String,
    },
    /// Moves to the mailbox with the role `archive`. Without one the action fails with
    /// [`ActionError::MissingMailbox`], so the UI can ask which mailbox to use (then
    /// `setMailboxRole`) or whether to create one (an operation of its own).
    Archive,
    /// Moves to the trash. Messages that already lie in the trash are only deleted with
    /// [`MailAction::DeletePermanently`]; `Delete` on them fails with
    /// [`ActionError::ConfirmationRequired`].
    Delete,
    /// Deletes for good (EXPUNGE). The UI asks before sending it; this action is the explicit
    /// confirmation the core requires.
    DeletePermanently,
    /// Moves to another mailbox of the same account.
    Move {
        target: MailboxId,
    },
    /// Moves to the mailbox with the role `junk`.
    Spam,
    /// Moves messages from the junk mailbox to the inbox; others are left alone.
    NotSpam,
}

impl MailAction {
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::MarkRead => ActionKind::MarkRead,
            Self::MarkUnread => ActionKind::MarkUnread,
            Self::Flag => ActionKind::Flag,
            Self::Unflag => ActionKind::Unflag,
            Self::AddKeyword { .. } => ActionKind::AddKeyword,
            Self::RemoveKeyword { .. } => ActionKind::RemoveKeyword,
            Self::Archive => ActionKind::Archive,
            Self::Delete => ActionKind::Delete,
            Self::DeletePermanently => ActionKind::DeletePermanently,
            Self::Move { .. } => ActionKind::Move,
            Self::Spam => ActionKind::Spam,
            Self::NotSpam => ActionKind::NotSpam,
        }
    }
}

/// What an operation was recorded for, for the activity view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    AddKeyword,
    RemoveKeyword,
    Archive,
    Delete,
    DeletePermanently,
    Move,
    Spam,
    NotSpam,
    CreateMailbox,
    /// Counter-operations of an undone batch.
    Undo,
}

impl ActionKind {
    const ALL: [Self; 14] = [
        Self::MarkRead,
        Self::MarkUnread,
        Self::Flag,
        Self::Unflag,
        Self::AddKeyword,
        Self::RemoveKeyword,
        Self::Archive,
        Self::Delete,
        Self::DeletePermanently,
        Self::Move,
        Self::Spam,
        Self::NotSpam,
        Self::CreateMailbox,
        Self::Undo,
    ];

    /// Text in the database, same as on the wire.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MarkRead => "mark_read",
            Self::MarkUnread => "mark_unread",
            Self::Flag => "flag",
            Self::Unflag => "unflag",
            Self::AddKeyword => "add_keyword",
            Self::RemoveKeyword => "remove_keyword",
            Self::Archive => "archive",
            Self::Delete => "delete",
            Self::DeletePermanently => "delete_permanently",
            Self::Move => "move",
            Self::Spam => "spam",
            Self::NotSpam => "not_spam",
            Self::CreateMailbox => "create_mailbox",
            Self::Undo => "undo",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// A flag as the outbox sets or clears it. `\Recent` cannot be set (RFC 3501).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flag {
    Seen,
    Answered,
    Flagged,
    Draft,
    Deleted,
    /// A keyword (`$Forwarded`, `$Junk`, a user label). Build it with [`Flag::keyword`].
    Keyword(String),
}

impl Flag {
    /// A keyword flag, if `name` is a valid IMAP atom (see [`is_valid_keyword`]).
    pub fn keyword(name: &str) -> Result<Self, ActionError> {
        if is_valid_keyword(name) {
            Ok(Self::Keyword(name.to_owned()))
        } else {
            Err(ActionError::InvalidKeyword(name.to_owned()))
        }
    }

    /// The flag as written in a STORE command.
    pub fn imap(&self) -> &str {
        match self {
            Self::Seen => "\\Seen",
            Self::Answered => "\\Answered",
            Self::Flagged => "\\Flagged",
            Self::Draft => "\\Draft",
            Self::Deleted => "\\Deleted",
            Self::Keyword(name) => name,
        }
    }

    /// Whether the flag is set in `flags` (keywords ignoring ASCII case).
    pub fn is_set(&self, flags: &Flags) -> bool {
        match self {
            Self::Seen => flags.seen,
            Self::Answered => flags.answered,
            Self::Flagged => flags.flagged,
            Self::Draft => flags.draft,
            Self::Deleted => flags.deleted,
            Self::Keyword(name) => flags.has_keyword(name),
        }
    }

    /// Sets or clears the flag in `flags`; keywords stay normalised.
    pub fn apply(&self, flags: &mut Flags, value: bool) {
        match self {
            Self::Seen => flags.seen = value,
            Self::Answered => flags.answered = value,
            Self::Flagged => flags.flagged = value,
            Self::Draft => flags.draft = value,
            Self::Deleted => flags.deleted = value,
            Self::Keyword(name) => {
                flags.keywords.retain(|k| !k.eq_ignore_ascii_case(name));
                if value {
                    flags.keywords.push(name.clone());
                }
                flags.normalize_keywords();
            }
        }
    }

    /// Same flag (keywords compared ignoring ASCII case, as servers do).
    pub fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Keyword(a), Self::Keyword(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a == b,
        }
    }
}

/// Whether `name` can be sent as a keyword (decision 0002: servers answer BAD to invalid ones,
/// and async-imap would swallow that). RFC 3501: `flag-keyword = atom`, an atom is one or more
/// ATOM-CHAR, which excludes `( ) { ] % * " \`, space and control characters; CHAR is 7-bit.
/// Also refused: a leading backslash (system flags) and more than [`MAX_KEYWORD_LEN`] bytes.
pub fn is_valid_keyword(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_KEYWORD_LEN
        && name.bytes().all(|b| {
            (0x21..0x7f).contains(&b)
                && !matches!(b, b'(' | b')' | b'{' | b']' | b'%' | b'*')
                && !matches!(b, b'"' | b'\\')
        })
}

/// Id of one operation (`operation.id`). Never reused; also the order of execution.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct OperationId(#[specta(type = Number)] pub i64);

/// The operations of one action (one call, possibly many messages). Undo works on a batch.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct BatchId(#[specta(type = Number)] pub i64);

impl fmt::Display for OperationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for BatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Where an operation stands.
///
/// `queued` → `in_flight` (the command is about to be sent) → `confirmed` | `failed` |
/// `conflict`. After a crash or a lost connection, `in_flight` operations are checked against
/// the server before anything is repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    /// Recorded, local change applied, waiting for the server.
    Queued,
    /// Sent or about to be sent; the outcome is not known yet.
    InFlight,
    /// The server did it.
    Confirmed,
    /// The server refused or the outbox gave up; the local change was reverted. The user can
    /// retry or discard it.
    Failed,
    /// The server state changed meanwhile (the message was moved, deleted or modified
    /// elsewhere); the server wins and the user is told ([`ConflictReason`]).
    Conflict,
}

impl OperationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::InFlight => "in_flight",
            Self::Confirmed => "confirmed",
            Self::Failed => "failed",
            Self::Conflict => "conflict",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::InFlight,
            Self::Confirmed,
            Self::Failed,
            Self::Conflict,
        ]
        .into_iter()
        .find(|state| state.as_str() == s)
    }

    /// `queued` or `in_flight`: the outbox still works on it.
    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Queued | Self::InFlight)
    }
}

/// What an operation does on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    SetFlag,
    Move,
    Expunge,
    CreateMailbox,
}

impl OperationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SetFlag => "set_flag",
            Self::Move => "move",
            Self::Expunge => "expunge",
            Self::CreateMailbox => "create_mailbox",
        }
    }
}

/// Why an operation ended in [`OperationState::Conflict`]. In every case the server state is
/// kept and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    /// The message is no longer in the mailbox: moved or deleted by another client.
    Gone,
    /// The message was changed on the server after the app last saw it (CONDSTORE
    /// `UNCHANGEDSINCE`), and the server's state differs from what was asked.
    Modified,
    /// The mailbox was recreated on the server (new UIDVALIDITY); its old UIDs are void.
    MailboxReset,
    /// The mailbox no longer exists on the server.
    MailboxGone,
}

impl ConflictReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gone => "gone",
            Self::Modified => "modified",
            Self::MailboxReset => "mailbox_reset",
            Self::MailboxGone => "mailbox_gone",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Gone,
            Self::Modified,
            Self::MailboxReset,
            Self::MailboxGone,
        ]
        .into_iter()
        .find(|reason| reason.as_str() == s)
    }
}

/// Something the user should know about a confirmed operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationNotice {
    /// The server has no UIDPLUS and other messages in the source mailbox were marked deleted,
    /// so a plain EXPUNGE would have removed them too. The message was copied but its original
    /// stays in the source, marked deleted, until the mailbox is expunged.
    LeftMarkedDeleted,
}

impl OperationNotice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LeftMarkedDeleted => "left_marked_deleted",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        (s == Self::LeftMarkedDeleted.as_str()).then_some(Self::LeftMarkedDeleted)
    }
}

/// One operation as the activity view shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OperationInfo {
    pub id: OperationId,
    pub batch: BatchId,
    pub account_id: AccountId,
    pub action: ActionKind,
    pub kind: OperationKind,
    pub state: OperationState,
    /// Source mailbox and message; null for `create_mailbox`, or when they were deleted
    /// locally since.
    pub mailbox_id: Option<MailboxId>,
    pub message_id: Option<MessageId>,
    /// Target of a move.
    pub target_mailbox_id: Option<MailboxId>,
    /// Times the command was sent.
    pub attempts: u32,
    /// Why it failed (`failed`), or the last transient error.
    pub error: Option<IpcError>,
    pub conflict: Option<ConflictReason>,
    pub notice: Option<OperationNotice>,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub created_at: i64,
    #[specta(type = Number)]
    pub updated_at: i64,
}

/// Result of applying an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    /// For undo; null if nothing was recorded (no-op, or every change cancelled a queued one).
    pub batch: Option<BatchId>,
    /// Operations recorded.
    pub queued: u32,
    /// Queued operations that this action cancelled out (for example "read" after a still
    /// queued "unread").
    pub cancelled: u32,
}

/// Events of the outbox. The integration forwards them to the UI: [`OutboxEvent::to_core_event`]
/// covers the list events, the operation events get their own `CoreEvent` variants in the
/// stage 4 integration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum OutboxEvent {
    /// An operation was recorded or changed state; carries its complete state.
    OperationChanged { operation: OperationInfo },
    /// An operation was removed from the outbox (cancelled by undo or a cancelling action, or
    /// discarded by the user).
    OperationRemoved {
        account_id: AccountId,
        id: OperationId,
    },
    /// Local state of messages in a mailbox changed (applied, reverted, confirmed).
    MessagesChanged {
        account_id: AccountId,
        mailbox_id: MailboxId,
    },
    /// Folder list or counters changed (after local changes, or a mailbox was created).
    MailboxesChanged { account_id: AccountId },
}

impl OutboxEvent {
    /// The matching event of the existing stream, for the list events.
    pub fn to_core_event(&self) -> Option<CoreEvent> {
        match self {
            Self::MessagesChanged {
                account_id,
                mailbox_id,
            } => Some(CoreEvent::MessagesChanged {
                account_id: *account_id,
                mailbox_id: *mailbox_id,
            }),
            Self::MailboxesChanged { account_id } => Some(CoreEvent::MailboxesChanged {
                account_id: *account_id,
            }),
            Self::OperationChanged { .. } | Self::OperationRemoved { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Stored operations (internal)

/// The message an operation works on, as it was when the operation was planned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceLocation {
    pub mailbox_id: MailboxId,
    /// Wire name, for SELECT.
    pub mailbox_name: String,
    pub uid_validity: UidValidity,
    pub uid: Uid,
    pub message_id: MessageId,
    /// `Message-ID` header, to find the message after a move.
    pub message_id_header: Option<String>,
    /// RFC822.SIZE, to recognise a moved message without Message-ID.
    pub size: u32,
    /// Local flags when planned; a failed or undone move or delete restores them.
    pub flags: Flags,
    /// MODSEQ of the message as the app last saw it (stored with the location by the sync),
    /// for `UNCHANGEDSINCE`. `None` without CONDSTORE, or when unknown (a location the outbox
    /// added after a move): then the STORE is unconditional.
    pub modseq: Option<ModSeq>,
}

/// Target mailbox of a move.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMailbox {
    pub mailbox_id: MailboxId,
    /// Wire name.
    pub name: String,
}

/// The target's STATUS right before a move was sent. Anything that arrived in the target with
/// a UID at or above `uid_next` came after this moment, so a check after a crash can tell an
/// earlier copy of the same message from the one this operation made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetMark {
    pub uid_validity: UidValidity,
    pub uid_next: Uid,
}

/// What an operation does, stored as JSON in `operation.payload`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum OperationPayload {
    /// Set (`value`) or clear one flag. `before` is the local state it replaced (for revert,
    /// cancelling and undo).
    SetFlag {
        source: SourceLocation,
        flag: Flag,
        value: bool,
        before: bool,
    },
    /// Move to another mailbox. `mark` is recorded right before the command is sent.
    Move {
        source: SourceLocation,
        target: TargetMailbox,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mark: Option<TargetMark>,
    },
    /// Delete for good.
    Expunge { source: SourceLocation },
    /// Create a mailbox (decoded path with the server's delimiter) and give it a role.
    CreateMailbox {
        path: String,
        role: Option<MailboxRole>,
    },
}

impl OperationPayload {
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::SetFlag { .. } => OperationKind::SetFlag,
            Self::Move { .. } => OperationKind::Move,
            Self::Expunge { .. } => OperationKind::Expunge,
            Self::CreateMailbox { .. } => OperationKind::CreateMailbox,
        }
    }

    pub fn source(&self) -> Option<&SourceLocation> {
        match self {
            Self::SetFlag { source, .. } | Self::Move { source, .. } | Self::Expunge { source } => {
                Some(source)
            }
            Self::CreateMailbox { .. } => None,
        }
    }

    pub fn target(&self) -> Option<&TargetMailbox> {
        match self {
            Self::Move { target, .. } => Some(target),
            _ => None,
        }
    }

    /// Moves and expunges take the message out of its mailbox locally.
    pub fn removes_location(&self) -> bool {
        matches!(self, Self::Move { .. } | Self::Expunge { .. })
    }
}

// ---------------------------------------------------------------------------------------------
// Planner

/// A mailbox of the account, as the planner needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMailbox {
    pub id: MailboxId,
    /// Wire name.
    pub name: String,
    pub role: MailboxRole,
    pub selectable: bool,
}

/// A message to act on: where it lies now and the role of that mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanLocation {
    pub source: SourceLocation,
    pub role: MailboxRole,
}

/// Turns an action on messages into operations, one per message (and flag). Messages for which
/// the action is a no-op by place (archiving what is in the archive, "not spam" outside the
/// junk mailbox, moving into the mailbox it is in) get none; whether a flag is already set is
/// decided by the store, which also cancels queued opposites.
///
/// Errors: [`ActionError::MissingMailbox`] when the archive, trash or junk mailbox is missing,
/// [`ActionError::ConfirmationRequired`] for `Delete` on messages in the trash,
/// [`ActionError::InvalidKeyword`], [`ActionError::InvalidTarget`].
pub fn plan_action(
    action: &MailAction,
    locations: &[PlanLocation],
    mailboxes: &[PlanMailbox],
) -> Result<Vec<OperationPayload>, ActionError> {
    let set_flag = |flag: Flag, value: bool| -> Vec<OperationPayload> {
        locations
            .iter()
            .map(|loc| OperationPayload::SetFlag {
                before: flag.is_set(&loc.source.flags),
                source: loc.source.clone(),
                flag: flag.clone(),
                value,
            })
            .collect()
    };
    let by_role = |role: MailboxRole| -> Result<&PlanMailbox, ActionError> {
        mailboxes
            .iter()
            .find(|m| m.role == role && m.selectable)
            .ok_or(ActionError::MissingMailbox(role))
    };
    let move_to = |target: &PlanMailbox, skip: &dyn Fn(&PlanLocation) -> bool| {
        locations
            .iter()
            .filter(|loc| loc.source.mailbox_id != target.id && !skip(loc))
            .map(|loc| OperationPayload::Move {
                source: loc.source.clone(),
                target: TargetMailbox {
                    mailbox_id: target.id,
                    name: target.name.clone(),
                },
                mark: None,
            })
            .collect::<Vec<_>>()
    };
    let none = |_: &PlanLocation| false;

    Ok(match action {
        MailAction::MarkRead => set_flag(Flag::Seen, true),
        MailAction::MarkUnread => set_flag(Flag::Seen, false),
        MailAction::Flag => set_flag(Flag::Flagged, true),
        MailAction::Unflag => set_flag(Flag::Flagged, false),
        MailAction::AddKeyword { keyword } => set_flag(Flag::keyword(keyword)?, true),
        MailAction::RemoveKeyword { keyword } => set_flag(Flag::keyword(keyword)?, false),
        MailAction::Archive => move_to(by_role(MailboxRole::Archive)?, &none),
        MailAction::Spam => move_to(by_role(MailboxRole::Junk)?, &none),
        MailAction::NotSpam => {
            let inbox = by_role(MailboxRole::Inbox)?;
            move_to(inbox, &|loc| loc.role != MailboxRole::Junk)
        }
        MailAction::Delete => {
            if locations.iter().any(|loc| loc.role == MailboxRole::Trash) {
                return Err(ActionError::ConfirmationRequired);
            }
            move_to(by_role(MailboxRole::Trash)?, &none)
        }
        MailAction::DeletePermanently => locations
            .iter()
            .map(|loc| OperationPayload::Expunge {
                source: loc.source.clone(),
            })
            .collect(),
        MailAction::Move { target } => {
            let target = mailboxes
                .iter()
                .find(|m| m.id == *target && m.selectable)
                .ok_or(ActionError::InvalidTarget(*target))?;
            move_to(target, &none)
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Errors

/// Why an action or an outbox request was refused. Nothing was recorded or changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ActionError {
    /// Writing is turned off for the account (`writes_enabled = false`).
    #[error("writes are turned off for account {0}")]
    ReadOnly(AccountId),

    /// The account has no mailbox with this role (archive, trash, junk). The UI asks which
    /// mailbox to use or whether to create one.
    #[error("the account has no mailbox with the role {0:?}")]
    MissingMailbox(MailboxRole),

    /// `Delete` on messages in the trash: deleting for good needs `DeletePermanently`.
    #[error("messages in the trash are only deleted for good with an explicit confirmation")]
    ConfirmationRequired,

    #[error("{0:?} is not a valid keyword (an IMAP atom without spaces or special characters)")]
    InvalidKeyword(String),

    /// The message is not (any more) in that mailbox locally; the list is out of date.
    #[error("message {} is not in mailbox {}", .0.message_id, .0.mailbox_id)]
    MessageNotFound(MessageRef),

    /// The target of a move does not exist, cannot hold messages or belongs to another account.
    #[error("mailbox {0} cannot be the target of a move")]
    InvalidTarget(MailboxId),

    /// One action can only cover messages of one account.
    #[error("the messages belong to more than one account")]
    MixedAccounts,

    #[error("no messages given")]
    NoMessages,

    #[error("operation {0} not found")]
    OperationNotFound(OperationId),

    /// The request does not fit the operation's state (for example retrying a queued one).
    #[error("operation {0} is {1:?}")]
    WrongState(OperationId, OperationState),

    #[error("cannot be undone: {0}")]
    NotUndoable(String),
}

impl ActionError {
    /// Code for the UI. The integration of stage 4 adds dedicated codes for
    /// `MissingMailbox` and `ConfirmationRequired` (with their texts in `app/src/i18n/`); until
    /// then they map to the closest existing code and the variant is in `detail`.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::ReadOnly(_) => ErrorCode::ReadOnly,
            Self::MissingMailbox(_) | Self::MessageNotFound(_) | Self::OperationNotFound(_) => {
                ErrorCode::NotFound
            }
            Self::ConfirmationRequired
            | Self::InvalidKeyword(_)
            | Self::InvalidTarget(_)
            | Self::MixedAccounts
            | Self::NoMessages
            | Self::WrongState(..)
            | Self::NotUndoable(_) => ErrorCode::InvalidArgument,
        }
    }

    /// Short English text for people (fallback for the UI's translation).
    pub fn message(&self) -> &'static str {
        match self {
            Self::ReadOnly(_) => "Changes are turned off for this account.",
            Self::MissingMailbox(MailboxRole::Archive) => "This account has no archive folder.",
            Self::MissingMailbox(MailboxRole::Trash) => "This account has no trash folder.",
            Self::MissingMailbox(MailboxRole::Junk) => "This account has no junk folder.",
            Self::MissingMailbox(_) => "This account lacks the folder for this action.",
            Self::ConfirmationRequired => "Deleting messages from the trash cannot be undone.",
            Self::InvalidKeyword(_) => "Tags cannot contain spaces or special characters.",
            Self::MessageNotFound(_) => "The message is no longer in this folder.",
            Self::InvalidTarget(_) => "Messages cannot be moved to this folder.",
            Self::MixedAccounts => "Messages of different accounts cannot be changed together.",
            Self::NoMessages => "No messages were selected.",
            Self::OperationNotFound(_) => "The action no longer exists.",
            Self::WrongState(..) => "The action cannot be changed in its current state.",
            Self::NotUndoable(_) => "This action cannot be undone.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        Some(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn source(mailbox: i64, uid: Uid) -> SourceLocation {
        SourceLocation {
            mailbox_id: MailboxId(mailbox),
            mailbox_name: format!("box{mailbox}"),
            uid_validity: UidValidity(7),
            uid,
            message_id: MessageId(1_000 + i64::from(uid)),
            message_id_header: Some(format!("{uid}@example.org")),
            size: 100,
            flags: Flags::default(),
            modseq: Some(ModSeq(5)),
        }
    }

    fn mailboxes() -> Vec<PlanMailbox> {
        [
            (1, "INBOX", MailboxRole::Inbox),
            (2, "Archive", MailboxRole::Archive),
            (3, "Trash", MailboxRole::Trash),
            (4, "Junk", MailboxRole::Junk),
            (5, "Projects", MailboxRole::Custom),
        ]
        .into_iter()
        .map(|(id, name, role)| PlanMailbox {
            id: MailboxId(id),
            name: name.into(),
            role,
            selectable: true,
        })
        .collect()
    }

    fn at(mailbox: i64, uid: Uid) -> PlanLocation {
        let role = mailboxes()
            .into_iter()
            .find(|m| m.id == MailboxId(mailbox))
            .map_or(MailboxRole::Custom, |m| m.role);
        PlanLocation {
            source: source(mailbox, uid),
            role,
        }
    }

    fn targets(ops: &[OperationPayload]) -> Vec<(i64, i64)> {
        ops.iter()
            .filter_map(|op| match op {
                OperationPayload::Move { source, target, .. } => {
                    Some((source.mailbox_id.0, target.mailbox_id.0))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn keywords_must_be_atoms() {
        for ok in ["$Forwarded", "$Junk", "projekt-x", "a.b_c:d", "x"] {
            assert!(is_valid_keyword(ok), "{ok:?}");
        }
        for bad in [
            "",
            "two words",
            "paren(",
            "brace{",
            "bracket]",
            "pct%",
            "star*",
            "quote\"",
            "back\\slash",
            "\\Seen",
            "tab\t",
            "Überprüfung",
            "line\r\nA1 LOGOUT",
            &"x".repeat(MAX_KEYWORD_LEN + 1),
        ] {
            assert!(!is_valid_keyword(bad), "{bad:?}");
            assert_eq!(
                Flag::keyword(bad),
                Err(ActionError::InvalidKeyword(bad.to_owned()))
            );
        }
        assert_eq!(Flag::Seen.imap(), "\\Seen");
        assert_eq!(
            Flag::keyword("$Junk").map(|f| f.imap().to_owned()),
            Ok("$Junk".into())
        );
    }

    #[test]
    fn flags_apply_and_compare_keywords_ignoring_case() {
        let mut flags = Flags::from_imap(["$Forwarded"]);
        let kw = Flag::Keyword("projekt".into());
        assert!(!kw.is_set(&flags));
        kw.apply(&mut flags, true);
        assert!(Flag::Keyword("PROJEKT".into()).is_set(&flags));
        Flag::Keyword("$FORWARDED".into()).apply(&mut flags, false);
        assert_eq!(flags.keywords, ["projekt"]);
        Flag::Seen.apply(&mut flags, true);
        assert!(flags.seen && Flag::Seen.is_set(&flags));
        assert!(kw.same(&Flag::Keyword("Projekt".into())));
        assert!(!kw.same(&Flag::Seen));
    }

    #[test]
    fn flag_actions_become_one_operation_per_message() {
        let mut read = at(1, 10);
        read.source.flags.seen = true;
        let ops =
            plan_action(&MailAction::MarkUnread, &[read, at(1, 11)], &mailboxes()).expect("plan");
        assert_eq!(ops.len(), 2);
        assert!(matches!(
            &ops[0],
            OperationPayload::SetFlag {
                flag: Flag::Seen,
                value: false,
                before: true,
                ..
            }
        ));
        assert!(matches!(
            &ops[1],
            OperationPayload::SetFlag {
                value: false,
                before: false,
                ..
            }
        ));
        let err = plan_action(
            &MailAction::AddKeyword {
                keyword: "two words".into(),
            },
            &[at(1, 10)],
            &mailboxes(),
        );
        assert_eq!(err, Err(ActionError::InvalidKeyword("two words".into())));
    }

    #[test]
    fn moves_go_to_the_role_mailbox_and_skip_what_is_there() {
        let locs = [at(1, 1), at(2, 2), at(4, 3)];
        let boxes = mailboxes();
        let plan = |action: MailAction| plan_action(&action, &locs, &boxes).expect("plan");
        assert_eq!(targets(&plan(MailAction::Archive)), [(1, 2), (4, 2)]);
        assert_eq!(targets(&plan(MailAction::Spam)), [(1, 4), (2, 4)]);
        assert_eq!(targets(&plan(MailAction::NotSpam)), [(4, 1)]);
        assert_eq!(targets(&plan(MailAction::Delete)), [(1, 3), (2, 3), (4, 3)]);
        assert_eq!(
            targets(&plan(MailAction::Move {
                target: MailboxId(5)
            })),
            [(1, 5), (2, 5), (4, 5)]
        );
        assert!(
            plan(MailAction::DeletePermanently)
                .iter()
                .all(|op| matches!(op, OperationPayload::Expunge { .. }))
        );
    }

    #[test]
    fn missing_roles_confirmation_and_targets_are_typed_errors() {
        let only_inbox: Vec<_> = mailboxes().into_iter().take(1).collect();
        for (action, role) in [
            (MailAction::Archive, MailboxRole::Archive),
            (MailAction::Delete, MailboxRole::Trash),
            (MailAction::Spam, MailboxRole::Junk),
        ] {
            assert_eq!(
                plan_action(&action, &[at(1, 1)], &only_inbox),
                Err(ActionError::MissingMailbox(role))
            );
        }
        // A non-selectable mailbox does not count.
        let mut boxes = mailboxes();
        if let Some(archive) = boxes.get_mut(1) {
            archive.selectable = false;
        }
        assert_eq!(
            plan_action(&MailAction::Archive, &[at(1, 1)], &boxes),
            Err(ActionError::MissingMailbox(MailboxRole::Archive))
        );
        assert_eq!(
            plan_action(
                &MailAction::Move {
                    target: MailboxId(2)
                },
                &[at(1, 1)],
                &boxes
            ),
            Err(ActionError::InvalidTarget(MailboxId(2)))
        );
        // In the trash, deleting needs the explicit action.
        assert_eq!(
            plan_action(&MailAction::Delete, &[at(1, 1), at(3, 2)], &mailboxes()),
            Err(ActionError::ConfirmationRequired)
        );
        assert_eq!(
            plan_action(&MailAction::DeletePermanently, &[at(3, 2)], &mailboxes())
                .map(|ops| ops.len()),
            Ok(1)
        );
        assert_eq!(
            ActionError::MissingMailbox(MailboxRole::Archive).code(),
            ErrorCode::NotFound
        );
        let account: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        assert_eq!(ActionError::ReadOnly(account).code(), ErrorCode::ReadOnly);
    }

    #[test]
    fn wire_formats() {
        assert_eq!(
            serde_json::to_value(MailAction::Move {
                target: MailboxId(4)
            })
            .expect("serialize"),
            json!({ "type": "move", "target": 4 })
        );
        assert_eq!(
            serde_json::to_value(MailAction::AddKeyword {
                keyword: "$Junk".into()
            })
            .expect("serialize"),
            json!({ "type": "addKeyword", "keyword": "$Junk" })
        );
        assert_eq!(
            serde_json::to_value(MailAction::DeletePermanently).expect("serialize"),
            json!({ "type": "deletePermanently" })
        );
        assert_eq!(
            serde_json::to_value(OperationState::InFlight).expect("serialize"),
            "in_flight"
        );
        for state in [
            OperationState::Queued,
            OperationState::InFlight,
            OperationState::Confirmed,
            OperationState::Failed,
            OperationState::Conflict,
        ] {
            assert_eq!(OperationState::parse(state.as_str()), Some(state));
            assert_eq!(
                serde_json::to_value(state).expect("serialize"),
                state.as_str()
            );
        }
        for kind in ActionKind::ALL {
            assert_eq!(ActionKind::parse(kind.as_str()), Some(kind));
            assert_eq!(
                serde_json::to_value(kind).expect("serialize"),
                kind.as_str()
            );
        }
        for reason in [
            ConflictReason::Gone,
            ConflictReason::Modified,
            ConflictReason::MailboxReset,
            ConflictReason::MailboxGone,
        ] {
            assert_eq!(ConflictReason::parse(reason.as_str()), Some(reason));
            assert_eq!(
                serde_json::to_value(reason).expect("serialize"),
                reason.as_str()
            );
        }
        let notice = OperationNotice::LeftMarkedDeleted;
        assert_eq!(OperationNotice::parse(notice.as_str()), Some(notice));

        // Payloads round-trip through JSON (the `operation.payload` column).
        let payload = OperationPayload::Move {
            source: source(1, 9),
            target: TargetMailbox {
                mailbox_id: MailboxId(2),
                name: "Archive".into(),
            },
            mark: Some(TargetMark {
                uid_validity: UidValidity(3),
                uid_next: 40,
            }),
        };
        let text = serde_json::to_string(&payload).expect("serialize");
        assert!(text.starts_with("{\"kind\":\"move\""), "{text}");
        let back: OperationPayload = serde_json::from_str(&text).expect("deserialize");
        assert_eq!(back, payload);
        let flag = OperationPayload::SetFlag {
            source: source(1, 9),
            flag: Flag::Keyword("$Junk".into()),
            value: true,
            before: false,
        };
        let back: OperationPayload =
            serde_json::from_str(&serde_json::to_string(&flag).expect("serialize"))
                .expect("deserialize");
        assert_eq!(back, flag);
    }
}
