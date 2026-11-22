use serde::{Deserialize, Serialize};

use crate::{
    AccountId, AccountStatus, IpcError, MailboxId, MessageId, PrefetchStatus, RenderToken,
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
