//! Stage 11: every registered kind is captured when its data changes through the core, in the
//! transaction that changes it, and nothing else is (device settings, secrets, `writes_enabled`).
//! Through `AppCore` with the fake server and the real store.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use mail_model::{
    AccountSettings, AccountState, ComposeSettings, CoreEvent, IdentityInput, LanguagePreference,
    MarkReadOnOpen, MessageCategory, NotificationSettings, PlatformShortcuts, Preferences,
    PrefetchPeriod, PrefetchSettings, ReadingPreferences, RuleAction, RuleCondition, RuleInput,
    Secret, Security, SenderKey, ServerSettings, Shortcut, ShortcutOverride, SnoozeChoice,
    TaskMark, TextMatchOp, ThemePreference, UiPreferences, WindowLayout,
};
use mail_store::StoredContact;
use tempfile::TempDir;
use tokio::time::Instant;

use super::*;
use crate::credentials::{Credentials, MemorySecretStore};
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

const MESSAGE: &[u8] = b"From: Bob <bob@friends.example>\r\nTo: alice@fake.test\r\n\
Subject: Lunch\r\nMessage-ID: <lunch-1@friends.example>\r\nContent-Type: text/plain\r\n\r\nHi\r\n";

fn settings() -> AccountSettings {
    let server = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "alice".into(),
    };
    AccountSettings {
        credential: Default::default(),
        protocol: Default::default(),
        email: "alice@fake.test".into(),
        display_name: "Alice".into(),
        imap: server("imap.fake.test", 993),
        smtp: server("smtp.fake.test", 465),
    }
}

async fn kinds(core: &AppCore<FakeConnector>) -> BTreeSet<String> {
    core.store_for_tests()
        .sync_records()
        .await
        .expect("rows")
        .into_iter()
        .map(|r| r.kind)
        .collect()
}

#[tokio::test]
async fn every_registered_kind_is_captured_when_it_changes() {
    let dir = TempDir::new().expect("dir");
    let server = FakeServer::new();
    server.deliver_raw("INBOX", MESSAGE);
    let mut services = CoreServices::with_connector(
        Credentials::new(Arc::new(MemorySecretStore::new())),
        FakeConnector(Arc::clone(&server)),
    );
    services.sync = SyncConfig {
        poll_interval: Duration::from_secs(600),
        settings_sync_auto: false,
        ..SyncConfig::default()
    };
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    let mut events = core.subscribe();
    let account = core
        .add_account(settings(), Secret::new(PASSWORD.into()))
        .await
        .expect("account")
        .id;
    let deadline = Instant::now() + Duration::from_secs(10);
    let message = loop {
        let inbox = core
            .list_mailboxes(account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == "INBOX");
        if let Some(inbox) = inbox {
            let page = core.list_messages(inbox.id, None, 10).await.expect("list");
            if let Some(item) = page.items.first() {
                break item.id;
            }
        }
        assert!(Instant::now() < deadline, "message never arrived");
        let _ = tokio::time::timeout(Duration::from_millis(50), events.recv()).await;
    };
    let accounts = core.list_accounts().await.expect("accounts");
    assert_ne!(accounts[0].status.state, AccountState::Error);

    // Settings: the registered ones are captured, device settings are not.
    core.set_preferences(Preferences {
        theme: ThemePreference::Dark,
        language: LanguagePreference::De,
    })
    .await
    .expect("preferences");
    core.set_reading_preferences(ReadingPreferences {
        mark_read_on_open: MarkReadOnOpen::Never,
        ..ReadingPreferences::default()
    })
    .await
    .expect("reading");
    core.set_compose_settings(ComposeSettings {
        send_delay_seconds: 7,
        ..ComposeSettings::default()
    })
    .await
    .expect("compose");
    core.set_notification_settings(NotificationSettings {
        hide_content: true,
        ..NotificationSettings::default()
    })
    .await
    .expect("notifications");
    core.set_prefetch_settings(PrefetchSettings {
        period: PrefetchPeriod::default(),
        max_message_size: 1_000,
    })
    .await
    .expect("prefetch");
    let mut inbox_settings = core.get_inbox_settings().await.expect("inbox settings");
    inbox_settings.categories = !inbox_settings.categories;
    core.set_inbox_settings(inbox_settings)
        .await
        .expect("inbox");
    // Stage 8: shortcuts and text size are synced; recent commands, layouts and (decision
    // 0028) the AI panel's state are not.
    core.set_ui_preferences(UiPreferences {
        shortcuts: PlatformShortcuts {
            macos: vec![ShortcutOverride {
                command: "archive".into(),
                shortcuts: vec![Shortcut {
                    key: "y".into(),
                    shift: false,
                    modifier: true,
                }],
            }],
            windows: Vec::new(),
        },
        text_size: 115,
        recent_commands: vec!["action:archive".into()],
        ai_panel: mail_model::AiPanelState::Folded,
        // Stage 16 (decision 0036): the accent theme is synced like the text size.
        accent_theme: mail_model::AccentTheme::Green,
        conversations: false,
        // Stage 17: the column's views are synced like the text size.
        column_views: vec![mail_model::ColumnView {
            id: "later".into(),
            shown: false,
        }],
    })
    .await
    .expect("ui preferences");
    core.set_window_layout("main", WindowLayout::default())
        .await
        .expect("layout");

    // Identities with a signature, sender rules, local rules, marks, groups, remote content.
    let identity = core
        .add_identity(
            account,
            IdentityInput {
                email: "alice+work@fake.test".into(),
                name: "Alice at work".into(),
                reply_to: None,
            },
        )
        .await
        .expect("identity");
    core.set_identity_signature(identity.id, Some("Regards, Alice".into()))
        .await
        .expect("signature");
    // Stage 10: a signature of the pool as the reply default, and a template.
    let pooled = core
        .save_signature(mail_model::SignatureInput {
            id: None,
            name: "Short".into(),
            html: "<p><strong>A.</strong></p>".into(),
        })
        .await
        .expect("pool signature")
        .signature;
    let identity_signature = core
        .get_identity_signature_defaults(identity.id)
        .await
        .expect("defaults")
        .new_signature;
    core.set_identity_signature_defaults(identity.id, identity_signature, Some(pooled.id.clone()))
        .await
        .expect("reply default");
    core.save_template(mail_model::TemplateInput {
        id: None,
        name: "Thanks".into(),
        subject: "Thanks, {{first_name}}".into(),
        html: "<p>Hi {{first_name}}</p>".into(),
        // Stage 16: a template with recipients is a record like any other.
        to: vec![mail_model::Address {
            name: None,
            email: "bob@friends.example".into(),
        }],
        cc: Vec::new(),
        bcc: Vec::new(),
    })
    .await
    .expect("template");
    core.set_sender_category(
        SenderKey::address("news@shop.example"),
        Some(MessageCategory::Newsletter),
    )
    .await
    .expect("sender rule");
    core.create_rule(RuleInput {
        name: "Bob".into(),
        enabled: true,
        match_all: true,
        conditions: vec![RuleCondition::From {
            op: TextMatchOp::Contains,
            value: "bob@".into(),
        }],
        actions: vec![RuleAction::MarkRead],
        stop: false,
    })
    .await
    .expect("rule");
    core.snooze_messages(vec![message], SnoozeChoice::Tomorrow)
        .await
        .expect("snooze");
    core.set_pinned(vec![message], true).await.expect("pin");
    core.set_task(vec![message], Some(TaskMark { done: false }))
        .await
        .expect("task");
    // Stage 20 (decision 0059): a reminder if no reply comes.
    core.set_follow_up(vec![message], mail_model::FollowUpChoice::NextWeek)
        .await
        .expect("reminder");
    // Stage 19 (decision 0056): a contact group.
    core.create_contact_list(
        "Team".into(),
        vec![mail_model::Address {
            name: Some("Bob".into()),
            email: "bob@example.org".into(),
        }],
    )
    .await
    .expect("contact group");
    // Stage 16 (decision 0033): the reading list, a folder's favourite and colour and a
    // blocked sender are records; the views that group unread on top are not.
    core.set_reading_list(vec![message], true)
        .await
        .expect("reading list");
    let inbox = core
        .list_mailboxes(account)
        .await
        .expect("mailboxes")
        .into_iter()
        .find(|m| m.path == "INBOX")
        .expect("inbox")
        .id;
    core.set_mailbox_favorite(inbox, true)
        .await
        .expect("favourite");
    core.set_mailbox_color(inbox, mail_model::AccountColor::new(1))
        .await
        .expect("colour");
    core.block_sender("spam@offers.example".into())
        .await
        .expect("blocked sender");
    core.set_unread_first_view("unified:all".into(), true)
        .await
        .expect("unread first");
    core.create_account_group("Work".into(), vec![account])
        .await
        .expect("group");
    // Stage 7 (decision 0026): a saved search is a record; a recent search is not.
    core.save_search("Unread".into(), "is:unread".into())
        .await
        .expect("saved search");
    core.add_recent_search("from:bob".into())
        .await
        .expect("recent search");
    let store = core.store_for_tests();
    store
        .allow_remote_content(account, "bob@friends.example")
        .await
        .expect("remote content");
    // Stage 16 (decision 0036): a pinned attachment is a record (through the store: the fake
    // message has no stored body to list attachments from).
    store
        .set_attachment_pin(
            message,
            0,
            Some(mail_store::AttachmentPinInput {
                index: 0,
                name: "Rechnung.pdf".into(),
                mime_type: "application/pdf".into(),
                size: 12_345,
            }),
            1,
        )
        .await
        .expect("attachment pin");
    // A contact the user named (no command yet; the store path the composer will use).
    store
        .record_contacts(
            vec![StoredContact {
                key: "bob@friends.example".into(),
                local_part: "bob".into(),
                domain: "friends.example".into(),
                group: None,
                sent: 1,
                received: 0,
                last_sent: None,
                last_received: None,
                rank: 0.0,
                names: Vec::new(),
                user_name: Some("Bobby".into()),
                search: " bob bobby".into(),
                seen: 0,
                last_seen: None,
                personal: true,
                kind: 0,
                note: None,
            }],
            Vec::new(),
        )
        .await
        .expect("contact");
    // Stage 17: a picture the user chose for the contact (a JPEG the helper wrote).
    store
        .set_contact_picture(
            "bob@friends.example".into(),
            Vec::new(),
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 16],
        )
        .await
        .expect("contact picture");
    core.set_sync_device_name("Test Mac".into())
        .await
        .expect("device");
    // Secrets of other parts of the app: an AI client's key (MCP) lives in the keychain only.
    let mcp = core
        .create_mcp_client("Test client".into())
        .await
        .expect("mcp client");
    let mcp_key = mcp.token.rsplit('_').next().expect("key part").to_owned();

    let captured = kinds(&core).await;
    let without_table = [RecordKind::AiSettings];
    for kind in RecordKind::ALL {
        if without_table.contains(&kind) {
            continue;
        }
        assert!(
            captured.contains(kind.name()),
            "{kind} was not captured; captured {captured:?}"
        );
    }

    let rows = store.sync_records().await.expect("rows");
    let settings: BTreeSet<String> = rows
        .iter()
        .filter(|r| r.kind == "setting")
        .map(|r| r.id.clone())
        .collect();
    let expected: BTreeSet<String> = SYNCED_SETTINGS
        .iter()
        .map(|k| format!("global/{k}"))
        .collect();
    assert_eq!(settings, expected, "exactly the registered settings");

    // No secrets, no write switch, in any record.
    for row in &rows {
        let text = row
            .content
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        assert!(
            !text.contains(PASSWORD),
            "{}: password in a record",
            row.kind
        );
        assert!(
            !text.contains(&mcp.token),
            "{}: MCP token in a record",
            row.kind
        );
        assert!(
            !text.contains(&mcp_key),
            "{}: MCP key in a record",
            row.kind
        );
        assert!(
            !text.contains("writesEnabled"),
            "{}: writesEnabled",
            row.kind
        );
        assert!(
            !text.contains("from:bob"),
            "{}: a recent search (device only) in a record",
            row.kind
        );
        assert!(
            !text.contains("credential"),
            "{}: keychain reference",
            row.kind
        );
    }

    // Unchanged data keeps its version; a change gets a new one; deleting leaves a tombstone.
    let theme = |rows: &[mail_store::SyncRecordRow]| {
        rows.iter()
            .find(|r| r.id == "global/ui.theme")
            .map(|r| r.version.clone())
            .expect("theme")
    };
    let first = theme(&rows);
    core.set_preferences(Preferences {
        theme: ThemePreference::Dark,
        language: LanguagePreference::De,
    })
    .await
    .expect("same again");
    assert_eq!(theme(&store.sync_records().await.expect("rows")), first);
    core.set_preferences(Preferences {
        theme: ThemePreference::Light,
        language: LanguagePreference::De,
    })
    .await
    .expect("changed");
    assert!(theme(&store.sync_records().await.expect("rows")) > first);
    // The identity names its defaults; the pool signature is its own record.
    let identity_record = store
        .sync_record("identity", &identity_uuid(&rows, "alice+work@fake.test"))
        .await
        .expect("read")
        .expect("identity");
    assert_eq!(
        identity_record.content.expect("content")["signatureReply"],
        serde_json::json!(pooled.id.as_str())
    );
    assert_eq!(
        store
            .sync_record("signature", pooled.id.as_str())
            .await
            .expect("read")
            .expect("pool signature")
            .content
            .expect("content")["html"]
            .as_str()
            .map(str::trim_end),
        Some("<p><strong>A.</strong></p>")
    );
    core.set_identity_signature(identity.id, None)
        .await
        .expect("signature removed");
    let signature = store
        .sync_record("signature", &identity_uuid(&rows, "alice+work@fake.test"))
        .await
        .expect("read")
        .expect("row");
    assert!(signature.content.is_none() && signature.deleted_version.is_some());
    core.shutdown().await;
}

fn identity_uuid(rows: &[mail_store::SyncRecordRow], email: &str) -> String {
    rows.iter()
        .find(|r| {
            r.kind == "identity"
                && r.content
                    .as_ref()
                    .is_some_and(|c| c["email"].as_str() == Some(email))
        })
        .map(|r| r.id.clone())
        .expect("identity record")
}

#[test]
fn captured_content_of_every_table_passes_its_kind() {
    // The shapes the store writes (mail-store `settings_sync::content_of`) against the
    // registry's checks.
    let account = "6f9619ff-8b86-4d11-b42d-00c04fc964ff";
    let server =
        serde_json::json!({ "host": "h", "port": 993, "security": "tls", "username": "u" });
    let cases = [
        ("setting", "global/ui.theme", serde_json::json!("dark")),
        (
            "identity",
            account,
            serde_json::json!({ "account": account, "email": "a@b",
            "name": "A", "replyTo": null, "isDefault": true }),
        ),
        (
            "signature",
            account,
            serde_json::json!({ "text": "Regards" }),
        ),
        (
            "signature",
            account,
            serde_json::json!({ "name": "Work", "text": "A", "html": "<p>A</p>",
            "images": [{ "id": "1", "contentId": "sig-1@signature", "fileName": "a.png",
                         "mime": "image/png", "dataBase64": "iVBORw0KGgo=" }] }),
        ),
        (
            "template",
            account,
            serde_json::json!({ "name": "T", "subject": "S", "html": "<p>B</p>" }),
        ),
        (
            "identity",
            account,
            serde_json::json!({ "account": account, "email": "a@b", "name": "A",
            "replyTo": null, "isDefault": true, "signatureNew": account }),
        ),
        (
            "senderRule",
            "address:a@b",
            serde_json::json!({ "category": "newsletter", "important": false }),
        ),
        (
            "rule",
            account,
            serde_json::json!({ "name": "r", "position": 0, "enabled": true,
            "definition": { "matchAll": true, "conditions": [], "actions": [], "stop": false } }),
        ),
        (
            "snooze",
            &format!("{account}/m@x"),
            serde_json::json!({ "untilMs": 5 }),
        ),
        (
            "pin",
            &format!("{account}/m@x"),
            serde_json::json!({ "pinnedAtMs": 5 }),
        ),
        (
            "task",
            &format!("{account}/m@x"),
            serde_json::json!({ "doneAtMs": null }),
        ),
        (
            "accountGroup",
            account,
            serde_json::json!({ "name": "g", "position": 0, "accounts": [account] }),
        ),
        (
            "savedView",
            account,
            serde_json::json!({ "name": "Unread", "query": "is:unread", "position": 0 }),
        ),
        (
            "remoteContent",
            &format!("{account}/a@b"),
            serde_json::json!({}),
        ),
        ("contact", "a@b", serde_json::json!({ "userName": "A" })),
        (
            "contact",
            "a@b",
            serde_json::json!({ "note": "Met at the fair" }),
        ),
        (
            "contactPicture",
            "a@b",
            serde_json::json!({ "jpeg": "/9j/4AAQ" }),
        ),
        (
            "attachmentPin",
            &format!("{account}/m/x@y/2"),
            serde_json::json!({ "pinnedAtMs": 5, "name": "a.pdf", "mimeType": "application/pdf",
            "size": 3 }),
        ),
        (
            "device",
            account,
            serde_json::json!({ "name": "Mac", "system": "macOS", "lastSeenMs": 1 }),
        ),
        (
            "followUp",
            &format!("{account}/x@y"),
            serde_json::json!({ "dueMs": 9, "sentAtMs": 1 }),
        ),
    ];
    for (kind, id, content) in cases {
        let kind = RecordKind::from_name(kind).expect("kind");
        RecordKey::parse(kind, id).unwrap_or_else(|err| panic!("{kind}: {err}"));
        SyncContent::new(kind, content).unwrap_or_else(|err| panic!("{kind}: {err}"));
    }
    let fields = serde_json::json!({ "email": "a@b", "displayName": "A", "color": 2,
        "imap": server, "smtp": server });
    let mut clock = HlcClock::new(
        Arc::new(crate::clock::SystemClock),
        DeviceId::from_uuid(uuid::Uuid::from_u128(1)),
    );
    let change = mail_store::CapturedChange {
        kind: "account".into(),
        id: account.into(),
        current: Some(fields),
        stored: None,
    };
    let row = super::super::local::capture_one(&change, &mut clock)
        .expect("captured")
        .expect("row");
    assert!(row.content.expect("content")["email"]["version"].is_string());
    let _ = CoreEvent::AccountsChanged;
}
