use serde::{Deserialize, Serialize};

use crate::{
    AccountId, AccountStatus, DraftId, IpcError, MailboxId, MessageId, OperationId, OperationInfo,
    OutgoingMail, PrefetchStatus, RenderToken,
};

/// Operating system the app runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    /// Platform this binary was built for.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Version information shown under Settings, About.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// Short git hash at build time, null if unknown.
    pub commit: Option<String>,
    /// RFC 3339 build timestamp, null if unknown.
    pub build_date: Option<String>,
    pub platform: Platform,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePreference {
    #[default]
    System,
    En,
    De,
}

/// User preferences stored in the `setting` table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub theme: ThemePreference,
    pub language: LanguagePreference,
}

/// When the reading pane marks an opened message as read (stage 4). The UI does the marking
/// (an ordinary `markRead` action), and only for accounts with `writesEnabled`; read-only
/// accounts mark nothing.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum MarkReadOnOpen {
    /// As soon as the message is shown.
    Immediately,
    /// After it was shown for [`MARK_READ_DELAY_MS`] without the selection changing.
    #[default]
    AfterDelay,
    Never,
}

/// The delay of [`MarkReadOnOpen::AfterDelay`]: two seconds.
pub const MARK_READ_DELAY_MS: u32 = 2_000;

/// Reading preferences stored in the `setting` table (stage 4). Separate from [`Preferences`],
/// so the appearance settings keep their shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReadingPreferences {
    pub mark_read_on_open: MarkReadOnOpen,
}

/// The one event stream from the core to the UI. Internally tagged by `type`.
///
/// The UI never polls: it re-queries what an event says has changed. The engine coalesces
/// bursts (for example one `messagesChanged` per stored sync window, not per message).
///
/// - `preferencesChanged`: preferences were stored; carries the stored values.
/// - `accountsChanged`: an account was added or removed; re-query `listAccounts`.
/// - `accountStatusChanged`: sync state of one account changed; carries the new status.
/// - `mailboxesChanged`: folder list, roles or counters of an account changed; re-query
///   `listMailboxes`.
/// - `messagesChanged`: messages of a mailbox were added, changed or removed; re-query the
///   visible pages of `listMessages` (counters come with `mailboxesChanged`).
/// - `newMail`: `count` new unread messages arrived in a mailbox during an incremental sync
///   (never during the first sync of a mailbox). For notifications; lists react to
///   `messagesChanged`.
/// - `messageBodyAvailable`: the body a `getMessage` answered `loading` for is stored; ask again.
/// - `messageBodyFailed`: that fetch failed.
/// - `prefetchProgress`: progress of the background prefetch of an account (about once a second).
/// - `linkActivated`: the user clicked link `index` of the rendering `renderToken`.
/// - `operationsChanged`: outbox operations of an account were recorded, changed state or were
///   removed (cancelled, discarded, replaced by a retry). One event per change of the outbox,
///   so an action on many messages is one event: `operations` carries their complete new
///   states, `removed` the ids that are gone. Re-query `getOutboxCounts` and a visible
///   activity page; show a notice for `failed`/`conflict` states of the user's own batches.
///   The messages' rows change with `messagesChanged`.
/// - `draftsChanged` (stage 5): local drafts of an account were saved, frozen for sending,
///   released, got a new server copy (`changed`) or were deleted, sent or discarded
///   (`removed`). An open composer reloads its draft only if it did not cause the change.
/// - `outgoingChanged` (stage 5): messages in the outbox view changed state (scheduled,
///   sending, verifying, unclear, failed, sent); carries their complete states.
/// - `sendNeedsDecision` (stage 5): it stayed unclear whether a message went out. Ask the user
///   and answer with `resolveUnclearSend`; nothing is sent again before.
/// - `inboxChanged` (stage 9): pins, snoozes, tasks or categories changed (a snooze came back,
///   a sender rule was applied, a local rule pinned something). Re-query the inbox and the
///   views of pinned, snoozed and task messages; `accountId` is null when several accounts
///   changed.
/// - `ruleSkipped` (stage 9): a local rule's action did not run for `count` new messages
///   (`readOnly`: the account is read-only). Show a notice; nothing else happened.
/// - `openMessageRequested` (stage 9): the user clicked a system notification; show the
///   message (in `mailboxId` if given).
/// - `composeRequested` (stage 9): the system opened a `mailto:` link with the app; open the
///   composer with these fields and show `request.notices` (a link that sets blind copies, or
///   tried to set headers that were ignored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoreEvent {
    PreferencesChanged {
        preferences: Preferences,
    },
    AccountsChanged,
    AccountStatusChanged {
        account_id: AccountId,
        status: AccountStatus,
    },
    MailboxesChanged {
        account_id: AccountId,
    },
    MessagesChanged {
        account_id: AccountId,
        mailbox_id: MailboxId,
    },
    NewMail {
        account_id: AccountId,
        mailbox_id: MailboxId,
        count: u32,
    },
    MessageBodyAvailable {
        account_id: AccountId,
        message_id: MessageId,
    },
    MessageBodyFailed {
        account_id: AccountId,
        message_id: MessageId,
        error: IpcError,
    },
    PrefetchProgress {
        account_id: AccountId,
        status: PrefetchStatus,
    },
    LinkActivated {
        render_token: RenderToken,
        index: u32,
    },
    OperationsChanged {
        account_id: AccountId,
        operations: Vec<OperationInfo>,
        removed: Vec<OperationId>,
    },
    DraftsChanged {
        account_id: AccountId,
        changed: Vec<DraftId>,
        removed: Vec<DraftId>,
    },
    OutgoingChanged {
        account_id: AccountId,
        mails: Vec<OutgoingMail>,
    },
    SendNeedsDecision {
        account_id: AccountId,
        mail: OutgoingMail,
    },
    InboxChanged {
        account_id: Option<AccountId>,
    },
    RuleSkipped {
        account_id: AccountId,
        rule_id: crate::RuleId,
        rule_name: String,
        reason: crate::RuleSkipReason,
        count: u32,
    },
    OpenMessageRequested {
        account_id: AccountId,
        mailbox_id: Option<MailboxId>,
        message_id: MessageId,
    },
    ComposeRequested {
        request: crate::ComposeRequest,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format_matches_the_contract() {
        let info = AppInfo {
            version: "0.1.0".into(),
            commit: None,
            build_date: Some("2026-11-22T18:00:00Z".into()),
            platform: Platform::Macos,
        };
        assert_eq!(
            serde_json::to_value(&info).expect("serialize"),
            json!({ "version": "0.1.0", "commit": null, "buildDate": "2026-11-22T18:00:00Z", "platform": "macos" })
        );

        let prefs = Preferences {
            theme: ThemePreference::Dark,
            language: LanguagePreference::De,
        };
        let event = CoreEvent::PreferencesChanged { preferences: prefs };
        assert_eq!(
            serde_json::to_value(&event).expect("serialize"),
            json!({ "type": "preferencesChanged", "preferences": { "theme": "dark", "language": "de" } })
        );

        let parsed: Preferences =
            serde_json::from_value(json!({ "theme": "light", "language": "system" }))
                .expect("deserialize");
        assert_eq!(parsed.theme, ThemePreference::Light);
        assert!(
            serde_json::from_value::<Preferences>(json!({ "theme": "blue", "language": "en" }))
                .is_err()
        );
    }

    #[test]
    fn stage_2_events_wire_format() {
        let account: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let cases = [
            (
                CoreEvent::AccountsChanged,
                json!({ "type": "accountsChanged" }),
            ),
            (
                CoreEvent::AccountStatusChanged {
                    account_id: account,
                    status: AccountStatus::connecting(),
                },
                json!({ "type": "accountStatusChanged",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                        "status": { "state": "connecting", "progress": null, "error": null,
                                    "retryAt": null, "lastSyncAt": null } }),
            ),
            (
                CoreEvent::MailboxesChanged {
                    account_id: account,
                },
                json!({ "type": "mailboxesChanged",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff" }),
            ),
            (
                CoreEvent::MessagesChanged {
                    account_id: account,
                    mailbox_id: MailboxId(3),
                },
                json!({ "type": "messagesChanged",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff", "mailboxId": 3 }),
            ),
            (
                CoreEvent::NewMail {
                    account_id: account,
                    mailbox_id: MailboxId(3),
                    count: 2,
                },
                json!({ "type": "newMail",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                        "mailboxId": 3, "count": 2 }),
            ),
        ];
        for (event, wire) in cases {
            assert_eq!(serde_json::to_value(&event).expect("serialize"), wire);
            let back: CoreEvent = serde_json::from_value(wire).expect("deserialize");
            assert_eq!(back, event);
        }
    }

    #[test]
    fn stage_3_events_wire_format() {
        let account: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let cases = [
            (
                CoreEvent::MessageBodyAvailable {
                    account_id: account,
                    message_id: MessageId(1_764_000_000),
                },
                json!({ "type": "messageBodyAvailable",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                        "messageId": 1_764_000_000 }),
            ),
            (
                CoreEvent::MessageBodyFailed {
                    account_id: account,
                    message_id: MessageId(7),
                    error: IpcError {
                        code: crate::ErrorCode::ConnectionLost,
                        message: "The connection to the server was lost.".into(),
                        detail: None,
                    },
                },
                json!({ "type": "messageBodyFailed",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff", "messageId": 7,
                        "error": { "code": "connection_lost",
                                   "message": "The connection to the server was lost.",
                                   "detail": null } }),
            ),
            (
                CoreEvent::PrefetchProgress {
                    account_id: account,
                    status: PrefetchStatus {
                        state: crate::PrefetchState::Running,
                        done: 3,
                        total: 10,
                        bytes: 4_096,
                    },
                },
                json!({ "type": "prefetchProgress",
                        "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                        "status": { "state": "running", "done": 3, "total": 10, "bytes": 4_096 } }),
            ),
            (
                CoreEvent::LinkActivated {
                    render_token: RenderToken::from_random(0xabc),
                    index: 4,
                },
                json!({ "type": "linkActivated",
                        "renderToken": "00000000000000000000000000000abc", "index": 4 }),
            ),
        ];
        for (event, wire) in cases {
            assert_eq!(serde_json::to_value(&event).expect("serialize"), wire);
            let back: CoreEvent = serde_json::from_value(wire).expect("deserialize");
            assert_eq!(back, event);
        }
    }

    #[test]
    fn stage_4_wire_format() {
        let account: AccountId = "6f9619ff-8b86-4d11-b42d-00c04fc964ff".parse().expect("id");
        let operation = OperationInfo {
            id: OperationId(12),
            batch: crate::BatchId(11),
            account_id: account,
            action: crate::ActionKind::Archive,
            kind: crate::OperationKind::Move,
            state: crate::OperationState::Failed,
            mailbox_id: Some(MailboxId(1)),
            message_id: Some(MessageId(1_764_000_000)),
            target_mailbox_id: Some(MailboxId(2)),
            attempts: 1,
            error: Some(IpcError {
                code: crate::ErrorCode::ServerRejected,
                message: "The server refused the request.".into(),
                detail: Some("NO [TRYCREATE]".into()),
            }),
            conflict: None,
            notice: None,
            created_at: 1_764_000_000_000,
            updated_at: 1_764_000_001_000,
        };
        let event = CoreEvent::OperationsChanged {
            account_id: account,
            operations: vec![operation],
            removed: vec![OperationId(10)],
        };
        let wire = json!({
            "type": "operationsChanged",
            "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
            "operations": [{
                "id": 12, "batch": 11, "accountId": "6f9619ff-8b86-4d11-b42d-00c04fc964ff",
                "action": "archive", "kind": "move", "state": "failed",
                "mailboxId": 1, "messageId": 1_764_000_000, "targetMailboxId": 2,
                "attempts": 1,
                "error": { "code": "server_rejected", "message": "The server refused the request.",
                           "detail": "NO [TRYCREATE]" },
                "conflict": null, "notice": null,
                "createdAt": 1_764_000_000_000_i64, "updatedAt": 1_764_000_001_000_i64
            }],
            "removed": [10]
        });
        assert_eq!(serde_json::to_value(&event).expect("serialize"), wire);
        let back: CoreEvent = serde_json::from_value(wire).expect("deserialize");
        assert_eq!(back, event);

        assert_eq!(
            serde_json::to_value(ReadingPreferences::default()).expect("serialize"),
            json!({ "markReadOnOpen": "afterDelay" })
        );
        for (value, wire) in [
            (MarkReadOnOpen::Immediately, "immediately"),
            (MarkReadOnOpen::AfterDelay, "afterDelay"),
            (MarkReadOnOpen::Never, "never"),
        ] {
            assert_eq!(serde_json::to_value(value).expect("serialize"), wire);
        }
    }

    #[test]
    fn defaults_are_system() {
        assert_eq!(
            Preferences::default(),
            Preferences {
                theme: ThemePreference::System,
                language: LanguagePreference::System
            }
        );
    }
}
