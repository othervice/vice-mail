use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, DATABASE_FILE};
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{App, Listener, WebviewWindow, WebviewWindowBuilder};
use tempfile::TempDir;

use super::*;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Fails when `app/src/generated/bindings.ts` is stale.
/// `UPDATE_BINDINGS=1 cargo test -p mail-app bindings` rewrites it; `BINDINGS_OUT=<file>` writes a
/// fresh copy elsewhere.
#[test]
fn bindings_are_fresh() {
    let committed = manifest_dir().join("../src/generated/bindings.ts");
    let dir = TempDir::new().expect("temp dir");
    let fresh = dir.path().join("bindings.ts");
    specta_builder::<tauri::Wry>()
        .export(typescript(), &fresh)
        .expect("tauri-specta export");
    let fresh_text = fs::read_to_string(&fresh).expect("read fresh bindings");

    // `BINDINGS_OUT=<file>` writes the fresh bindings there instead, to compare them with the
    // committed file without touching it.
    if let Some(out) = std::env::var_os("BINDINGS_OUT") {
        fs::write(out, &fresh_text).expect("write bindings copy");
        return;
    }
    if std::env::var("UPDATE_BINDINGS").as_deref() == Ok("1") {
        fs::create_dir_all(committed.parent().expect("parent")).expect("mkdir");
        fs::write(&committed, &fresh_text).expect("write bindings");
        return;
    }
    let committed_text = fs::read_to_string(&committed).unwrap_or_default();
    assert!(
        committed_text == fresh_text,
        "{} is stale; run `UPDATE_BINDINGS=1 cargo test -p mail-app bindings`",
        committed.display()
    );
}

#[test]
fn event_name_is_core_event() {
    assert_eq!(CORE_EVENT_NAME, "core-event");
}

// ---------------------------------------------------------------------------------------------
// The real command handler and event forwarding on Tauri's mock runtime
// ---------------------------------------------------------------------------------------------

struct Harness {
    _dir: TempDir,
    app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
}

/// Product services with an in-memory keychain: no test touches the real keychain.
fn services() -> CoreServices {
    CoreServices::new(Credentials::new(MemorySecretStore::new()))
}

fn build_info() -> BuildInfo {
    BuildInfo {
        version: "9.8.7".into(),
        commit: Some("0123456789".into()),
        build_date: Some("2026-11-22T18:00:00Z".into()),
    }
}

/// Wires the app exactly like `run()` does (same specta builder, `attach_core`), but on the
/// mock runtime and with a temporary data directory.
fn harness(prepare: impl FnOnce(&std::path::Path)) -> Harness {
    let dir = TempDir::new().expect("temp dir");
    let data_dir = dir.path().join("data");
    fs::create_dir_all(&data_dir).expect("mkdir");
    prepare(&data_dir);

    let specta = specta_builder::<MockRuntime>();
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir,
            build: build_info(),
        },
        services(),
    ));
    attach_core(app.handle(), &specta, core);
    let window = WebviewWindowBuilder::new(&app, MAIN_WINDOW, Default::default())
        .build()
        .expect("window");
    Harness {
        _dir: dir,
        app,
        window,
    }
}

fn invoke(h: &Harness, cmd: &str, args: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .expect("url"),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(&h.window, request)
        .map(|body| body.deserialize::<Value>().expect("JSON response"))
}

#[test]
fn command_returns_a_typed_response() {
    let h = harness(|_| {});

    let info = invoke(&h, "get_app_info", json!({})).expect("get_app_info");
    assert_eq!(
        info,
        json!({
            "version": "9.8.7",
            "commit": "0123456789",
            "buildDate": "2026-11-22T18:00:00Z",
            "platform": serde_json::to_value(mail_model::Platform::current()).expect("platform"),
        })
    );

    let prefs = invoke(&h, "get_preferences", json!({})).expect("get_preferences");
    assert_eq!(prefs, json!({ "theme": "system", "language": "system" }));
}

#[test]
fn command_error_is_typed() {
    let h = harness(write_newer_database);

    let err = invoke(&h, "get_preferences", json!({})).expect_err("must fail");
    assert_eq!(err["code"], "database_too_new");
    assert!(err["message"].as_str().is_some_and(|m| !m.is_empty()));
    // The supported version grows with every migration; only its shape is fixed here.
    let detail = err["detail"].as_str().expect("detail is a string");
    let supported = detail
        .strip_prefix("database schema version 999, this app supports up to ")
        .and_then(|rest| rest.parse::<u32>().ok())
        .unwrap_or_else(|| panic!("unexpected detail: {detail}"));
    assert!((1..999).contains(&supported), "unexpected detail: {detail}");
}

/// Leaves a database in `data_dir` as a newer app version would: a current database whose
/// header claims schema version 999 (bytes 60..64 of an SQLite file hold `user_version`).
fn write_newer_database(data_dir: &std::path::Path) {
    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir: data_dir.to_owned(),
            build: build_info(),
        },
        services(),
    ));
    assert!(core.startup_error().is_none());
    drop(core); // closes the store, which checkpoints and removes the WAL
    let path = data_dir.join(DATABASE_FILE);
    let mut bytes = fs::read(&path).expect("read database");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    fs::write(&path, bytes).expect("write database");
}

#[test]
fn set_preferences_emits_core_event_to_the_ui() {
    let h = harness(|_| {});
    let (tx, rx) = mpsc::channel::<String>();
    h.app.listen_any(CORE_EVENT_NAME, move |event| {
        let _ = tx.send(event.payload().to_owned());
    });

    let prefs = json!({ "theme": "dark", "language": "de" });
    let stored = invoke(&h, "set_preferences", json!({ "preferences": prefs })).expect("set");
    assert_eq!(stored, prefs);

    let payload = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("core-event within 5 s");
    let payload: Value = serde_json::from_str(&payload).expect("JSON payload");
    assert_eq!(
        payload,
        json!({ "type": "preferencesChanged", "preferences": prefs })
    );

    let again = invoke(&h, "get_preferences", json!({})).expect("get");
    assert_eq!(again, prefs);
}

/// Framework errors are plain strings, not `IpcError` (decision 0004); `ipc.ts` maps them.
#[test]
fn invalid_argument_is_rejected_by_the_framework_as_a_string() {
    let h = harness(|_| {});
    let err = invoke(
        &h,
        "set_preferences",
        json!({ "preferences": { "theme": "blue", "language": "de" } }),
    )
    .expect_err("invalid enum value");
    assert!(err.is_string(), "{err}");
    assert!(
        err.as_str().is_some_and(|s| s.contains("set_preferences")),
        "{err}"
    );
}

// ---------------------------------------------------------------------------------------------
// Real tauri.conf.json and capabilities (on the mock runtime)
// ---------------------------------------------------------------------------------------------

fn invoke_from(
    window: &WebviewWindow<MockRuntime>,
    cmd: &str,
    args: Value,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: if cfg!(windows) {
            "http://tauri.localhost"
        } else {
            "tauri://localhost"
        }
        .parse()
        .expect("url"),
        body: InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(window, request).map(|body| body.deserialize::<Value>().unwrap_or(Value::Null))
}

/// Startup order and permissions with the shipped configuration: the main window comes from
/// `tauri.conf.json` (created only after the core is managed), the window may listen to core
/// events but not emit events, and app commands work under the real ACL.
#[test]
fn shipped_config_window_and_capabilities() {
    let dir = TempDir::new().expect("temp dir");
    let specta = specta_builder::<MockRuntime>();
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(context())
        .expect("mock app with the real config");
    assert!(
        app.get_webview_window(MAIN_WINDOW).is_none(),
        "the window must not exist before the core is managed"
    );

    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir: dir.path().to_owned(),
            build: build_info(),
        },
        services(),
    ));
    let theme = initial_theme(&core);
    assert_eq!(
        theme, None,
        "without stored preferences the frame follows the system"
    );
    attach_core(app.handle(), &specta, core);
    open_main_window(app.handle(), theme).expect("open main window");
    let window = app.get_webview_window(MAIN_WINDOW).expect("main window");

    assert!(invoke_from(&window, "get_app_info", json!({})).is_ok());

    let listen = invoke_from(
        &window,
        "plugin:event|listen",
        json!({ "event": CORE_EVENT_NAME, "target": { "kind": "Any" }, "handler": 7 }),
    );
    assert!(listen.is_ok(), "listen must be allowed: {listen:?}");

    let emit = invoke_from(
        &window,
        "plugin:event|emit",
        json!({ "event": CORE_EVENT_NAME, "payload": null }),
    );
    let denied = emit
        .as_ref()
        .err()
        .and_then(Value::as_str)
        .is_some_and(|m| m.contains("not allowed"));
    assert!(denied, "the ACL must refuse emit from the UI: {emit:?}");
}

// ---------------------------------------------------------------------------------------------
// Stage 2 commands (behaviour is tested in mail-engine; here: wiring, arguments, error codes)
// ---------------------------------------------------------------------------------------------

#[test]
fn account_and_mail_commands_are_wired_with_typed_errors() {
    let h = harness(|_| {});
    let unknown = "6f9619ff-8b86-4d11-b42d-00c04fc964ff";

    assert_eq!(
        invoke(&h, "list_accounts", json!({})).expect("list"),
        json!([])
    );
    let known = invoke(
        &h,
        "discover_known_provider",
        json!({ "email": "someone@gmail.com" }),
    )
    .expect("known provider");
    assert_eq!(known["source"], "knownProvider");
    assert_eq!(known["providerHint"], "gmail");
    assert_eq!(known["imap"]["security"], "tls");

    for (cmd, args) in [
        ("list_mailboxes", json!({ "accountId": unknown })),
        ("retry_account", json!({ "accountId": unknown })),
        ("remove_account", json!({ "accountId": unknown })),
        (
            "list_messages",
            json!({ "mailboxId": 1, "cursor": null, "limit": 50 }),
        ),
        (
            "set_mailbox_role",
            json!({ "mailboxId": 1, "role": "archive" }),
        ),
    ] {
        let err = invoke(&h, cmd, args).expect_err(cmd);
        assert_eq!(err["code"], "not_found", "{cmd}: {err}");
    }
    let err = invoke(
        &h,
        "list_messages",
        json!({ "mailboxId": 1, "cursor": null, "limit": 0 }),
    )
    .expect_err("limit 0");
    assert_eq!(err["code"], "invalid_argument");

    // Nothing listens on the port: the login check fails with its own code, nothing is
    // stored, and the password never comes back.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = closed.local_addr().expect("addr").port();
    drop(closed);
    let server = json!({ "host": "127.0.0.1", "port": port, "security": "tls", "username": "a" });
    let settings =
        json!({ "email": "a@example.org", "displayName": "", "imap": server, "smtp": server });
    for cmd in ["test_connection", "add_account"] {
        let err = invoke(
            &h,
            cmd,
            json!({ "settings": settings, "password": "pw-never-echoed" }),
        )
        .expect_err(cmd);
        assert_eq!(err["code"], "connection_refused", "{cmd}: {err}");
        assert!(!err.to_string().contains("pw-never-echoed"));
    }
    assert_eq!(
        invoke(&h, "list_accounts", json!({})).expect("list"),
        json!([])
    );

    let mut bad = settings.clone();
    bad["email"] = json!("not-an-address");
    let err = invoke(
        &h,
        "add_account",
        json!({ "settings": bad, "password": "x" }),
    )
    .expect_err("invalid");
    assert_eq!(err["code"], "invalid_argument");
}

// ---------------------------------------------------------------------------------------------
// Stage 4 commands: typed answers of actions on a seeded database (no server; the outbox
// cannot connect, so operations stay queued and undo cancels them)
// ---------------------------------------------------------------------------------------------

const SEEDED_ACCOUNT: &str = "0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a01";

/// One account (writes off, its server unreachable, no password stored) with an INBOX and a
/// trash folder, each holding one unread message.
fn seed_account(data_dir: &std::path::Path) {
    use mail_engine::credentials::CredentialRef;
    use mail_model::{
        AccountId, Address, Flags, MessageHeader, RemoteMailbox, Security, ServerSettings,
        UidValidity,
    };
    use mail_store::{MailboxBaseline, NewAccount, Store, StoreOptions};

    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = closed.local_addr().expect("addr").port();
    drop(closed);
    tauri::async_runtime::block_on(async {
        let store =
            Store::open(&data_dir.join(DATABASE_FILE), StoreOptions::default()).expect("store");
        let id: AccountId = SEEDED_ACCOUNT.parse().expect("id");
        let server = ServerSettings {
            host: "127.0.0.1".into(),
            port,
            security: Security::Tls,
            username: "a@example.org".into(),
        };
        store
            .insert_account(NewAccount {
                id,
                email: "a@example.org".into(),
                display_name: "A".into(),
                imap: server.clone(),
                smtp: server,
                credential_ref: CredentialRef::for_account(id).to_string(),
            })
            .await
            .expect("account");
        let remote = |name: &str, attributes: &[&str]| RemoteMailbox {
            name: name.into(),
            path: name.into(),
            delimiter: Some('/'),
            attributes: attributes.iter().map(|a| (*a).to_owned()).collect(),
            status: None,
        };
        store
            .sync_mailboxes(
                id,
                vec![remote("INBOX", &[]), remote("Trash", &["\\Trash"])],
            )
            .await
            .expect("mailboxes");
        let uv = UidValidity(1);
        for (n, mailbox) in store
            .mailboxes(id)
            .await
            .expect("list")
            .into_iter()
            .enumerate()
        {
            let n = i64::try_from(n).expect("small");
            store
                .reset_mailbox(
                    mailbox.id,
                    MailboxBaseline {
                        uid_validity: uv,
                        uid_next: 2,
                        highest_modseq: None,
                    },
                )
                .await
                .expect("baseline");
            let header = MessageHeader {
                uid: 1,
                modseq: None,
                flags: Flags::default(),
                size: 100,
                internal_date: 1_764_000_000 + n,
                date: Some(1_764_000_000 + n),
                subject: format!("Message {n}"),
                from: vec![Address {
                    name: None,
                    email: "b@example.org".into(),
                }],
                to: Vec::new(),
                cc: Vec::new(),
                message_id: Some(format!("{n}@commands.test")),
                in_reply_to: None,
                references: Vec::new(),
                list_id: None,
                list_unsubscribe: None,
                auto_submitted: None,
                precedence: None,
                has_attachments: false,
                category_headers: Vec::new(),
                gmail: None,
            };
            store
                .store_headers(mailbox.id, uv, vec![header], None)
                .await
                .expect("headers");
        }
    });
}

#[test]
fn action_commands_answer_typed_results() {
    let h = harness(seed_account);
    let (tx, rx) = mpsc::channel::<String>();
    h.app.listen_any(CORE_EVENT_NAME, move |event| {
        let _ = tx.send(event.payload().to_owned());
    });

    let mailboxes =
        invoke(&h, "list_mailboxes", json!({ "accountId": SEEDED_ACCOUNT })).expect("mailboxes");
    let id_of = |path: &str| {
        mailboxes
            .as_array()
            .expect("array")
            .iter()
            .find(|m| m["path"] == path)
            .map(|m| m["id"].clone())
            .unwrap_or_else(|| panic!("no {path}"))
    };
    let (inbox, trash) = (id_of("INBOX"), id_of("Trash"));
    let first = |mailbox: &Value| {
        invoke(
            &h,
            "list_messages",
            json!({ "mailboxId": mailbox, "cursor": null, "limit": 50 }),
        )
        .expect("list")["items"][0]
            .clone()
    };
    let row = first(&inbox);
    assert_eq!(row["unread"], true);
    assert!(
        row.get("activity").is_none(),
        "absent when nothing is pending: {row}"
    );
    let at = json!([{ "mailboxId": inbox, "messageId": row["id"] }]);
    let apply = |action: Value, messages: &Value, request: &str| {
        invoke(
            &h,
            "apply_action",
            json!({ "action": action, "messages": messages, "requestId": request }),
        )
    };

    // New accounts are read-only: refused with its own code.
    let err = apply(json!({ "type": "markRead" }), &at, "req-1").expect_err("read-only");
    assert_eq!(err["code"], "read_only", "{err}");
    assert_eq!(first(&inbox)["unread"], true);

    let account = invoke(
        &h,
        "set_writes_enabled",
        json!({ "accountId": SEEDED_ACCOUNT, "enabled": true }),
    )
    .expect("writes on");
    assert_eq!(account["writesEnabled"], true);
    assert_eq!(account["id"], SEEDED_ACCOUNT);

    let result = apply(json!({ "type": "markRead" }), &at, "req-2").expect("mark read");
    let batch = result["batch"].clone();
    assert!(batch.is_number(), "{result}");
    assert_eq!(
        result,
        json!({ "batch": batch, "queued": 1, "cancelled": 0 })
    );
    let row = first(&inbox);
    assert_eq!(row["unread"], false, "local at once");
    assert_eq!(row["activity"], "pending");
    assert_eq!(
        invoke(
            &h,
            "get_outbox_counts",
            json!({ "accountId": SEEDED_ACCOUNT })
        )
        .expect("counts"),
        json!({ "accountId": SEEDED_ACCOUNT, "pending": 1, "failed": 0, "conflict": 0 })
    );
    let ops = invoke(
        &h,
        "list_operations",
        json!({ "accountId": SEEDED_ACCOUNT, "before": null, "limit": 10 }),
    )
    .expect("operations");
    assert_eq!(ops[0]["batch"], batch);
    assert_eq!(ops[0]["action"], "mark_read");
    assert_eq!(ops[0]["kind"], "set_flag");
    assert_eq!(ops[0]["state"], "queued");
    assert_eq!(ops[0]["accountId"], SEEDED_ACCOUNT);

    // The operation reached the UI as one typed event.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let operations_changed = loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let payload = rx.recv_timeout(left).expect("operationsChanged within 5 s");
        let event: Value = serde_json::from_str(&payload).expect("JSON payload");
        if event["type"] == "operationsChanged" {
            break event;
        }
    };
    assert_eq!(operations_changed["accountId"], SEEDED_ACCOUNT);
    assert_eq!(operations_changed["operations"][0]["batch"], batch);
    assert_eq!(operations_changed["removed"], json!([]));

    // The codes the UI acts on.
    let err = apply(json!({ "type": "archive" }), &at, "req-3").expect_err("no archive");
    assert_eq!(err["code"], "missing_mailbox", "{err}");
    let in_trash = json!([{ "mailboxId": trash, "messageId": first(&trash)["id"] }]);
    let err = apply(json!({ "type": "delete" }), &in_trash, "req-4").expect_err("in the trash");
    assert_eq!(err["code"], "confirmation_required", "{err}");
    let err = apply(
        json!({ "type": "addKeyword", "keyword": "a b" }),
        &at,
        "req-5",
    )
    .expect_err("bad tag");
    assert_eq!(err["code"], "invalid_keyword", "{err}");
    let err = apply(json!({ "type": "markRead" }), &at, "").expect_err("no request id");
    assert_eq!(err["code"], "invalid_argument", "{err}");
    let err = apply(json!({ "type": "explode" }), &at, "req-6").expect_err("unknown action");
    assert!(err.is_string(), "framework error: {err}");

    // Undo cancels the queued operation and restores the row.
    let undone = invoke(&h, "undo_action", json!({ "batch": batch })).expect("undo");
    assert_eq!(undone["cancelled"], 1, "{undone}");
    assert_eq!(undone["queued"], 0, "{undone}");
    let row = first(&inbox);
    assert_eq!(row["unread"], true);
    assert!(row.get("activity").is_none(), "{row}");
    assert_eq!(
        invoke(
            &h,
            "get_outbox_counts",
            json!({ "accountId": SEEDED_ACCOUNT })
        )
        .expect("counts")["pending"],
        0
    );
    let err = invoke(&h, "retry_operation", json!({ "operationId": 999_999 })).expect_err("gone");
    assert_eq!(err["code"], "not_found", "{err}");
    let err = invoke(&h, "discard_operation", json!({ "operationId": 999_999 })).expect_err("gone");
    assert_eq!(err["code"], "not_found", "{err}");

    // Reading preferences.
    assert_eq!(
        invoke(&h, "get_reading_preferences", json!({})).expect("reading prefs"),
        json!({ "markReadOnOpen": "afterDelay" })
    );
    let never = json!({ "markReadOnOpen": "never" });
    assert_eq!(
        invoke(
            &h,
            "set_reading_preferences",
            json!({ "preferences": never })
        )
        .expect("set"),
        never
    );
    assert_eq!(
        invoke(&h, "get_reading_preferences", json!({})).expect("reading prefs"),
        never
    );
}

// ---------------------------------------------------------------------------------------------
// Stage 8: UI preferences, window layouts, message windows (decision 0025)
// ---------------------------------------------------------------------------------------------

fn code_of(err: &Value) -> Option<&str> {
    err.get("code").and_then(Value::as_str)
}

#[test]
fn ui_preference_commands_store_validate_and_emit() {
    let h = harness(|_| {});
    let (tx, rx) = mpsc::channel::<String>();
    h.app.listen_any(CORE_EVENT_NAME, move |event| {
        let _ = tx.send(event.payload().to_owned());
    });

    let defaults = invoke(&h, "get_ui_preferences", json!({})).expect("get");
    assert_eq!(
        defaults,
        json!({
            "shortcuts": { "macos": [], "windows": [] },
            "textSize": 100,
            "recentCommands": [],
            "aiPanel": "open",
            // Decisions 0036 and 0053: synced, blue by default.
            "accentTheme": "blue",
            // Decision 0039: one row per conversation by default.
            "conversations": true,
            // Stage 17: no choice of the column's views, every view in the UI's order.
            "columnViews": []
        })
    );

    let prefs = json!({
        "shortcuts": {
            "macos": [
                { "command": "toggleRead", "shortcuts": [{ "key": "u", "shift": true, "mod": true }] },
                { "command": "archive", "shortcuts": [{ "key": "y", "shift": false, "mod": true }] }
            ],
            "windows": [{ "command": "delete", "shortcuts": [] }]
        },
        "textSize": 115,
        "recentCommands": ["action:archive"],
        "aiPanel": "folded",
        "accentTheme": "violet",
        "conversations": false,
        "columnViews": [{ "id": "flagged", "shown": true }, { "id": "later", "shown": false }]
    });
    let stored = invoke(&h, "set_ui_preferences", json!({ "preferences": prefs })).expect("set");
    assert_eq!(stored["accentTheme"], "violet");
    assert_eq!(stored["conversations"], false);
    // Stage 17: in the user's order, not sorted.
    assert_eq!(
        stored["columnViews"],
        json!([{ "id": "flagged", "shown": true }, { "id": "later", "shown": false }])
    );
    // Decision 0028: the AI panel's state is part of the preferences.
    assert_eq!(stored["aiPanel"], "folded");
    // Ordered by command, as stored.
    assert_eq!(stored["shortcuts"]["macos"][0]["command"], "archive");
    assert_eq!(stored["shortcuts"]["macos"][1]["command"], "toggleRead");
    assert_eq!(stored["textSize"], 115);
    assert_eq!(
        invoke(&h, "get_ui_preferences", json!({})).expect("get"),
        stored
    );
    let payload = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("core-event within 5 s");
    let payload: Value = serde_json::from_str(&payload).expect("JSON payload");
    assert_eq!(
        payload,
        json!({ "type": "uiPreferencesChanged", "preferences": stored })
    );

    // A broken limit is a typed error with a detail; nothing changes.
    let mut duplicate = stored.clone();
    duplicate["shortcuts"]["windows"] = json!([
        { "command": "delete", "shortcuts": [] },
        { "command": "delete", "shortcuts": [] }
    ]);
    let err = invoke(
        &h,
        "set_ui_preferences",
        json!({ "preferences": duplicate }),
    )
    .expect_err("duplicate command");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    assert!(
        err["detail"]
            .as_str()
            .is_some_and(|d| d.contains("appears twice")),
        "{err}"
    );
    // Decision 0028: only "open" or "folded"; the bridge refuses anything else.
    let mut sideways = stored.clone();
    sideways["aiPanel"] = json!("sideways");
    invoke(&h, "set_ui_preferences", json!({ "preferences": sideways }))
        .expect_err("unknown panel state");
    // Decision 0036: only the six accent themes.
    let mut magenta = stored.clone();
    magenta["accentTheme"] = json!("magenta");
    invoke(&h, "set_ui_preferences", json!({ "preferences": magenta }))
        .expect_err("unknown accent theme");
    let mut small = stored.clone();
    small["textSize"] = json!(79);
    let err =
        invoke(&h, "set_ui_preferences", json!({ "preferences": small })).expect_err("too small");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    assert_eq!(
        invoke(&h, "get_ui_preferences", json!({})).expect("get"),
        stored
    );
}

/// Decision 0036: the attachment and blocked-sender commands are wired with their argument
/// names and answer typed errors (no account, nothing stored).
#[test]
fn stage_16_attachment_commands_are_wired() {
    let h = harness(|_| {});
    for (cmd, args) in [
        (
            "preview_attachment",
            json!({ "messageId": 1, "index": 0, "page": 0 }),
        ),
        (
            "set_attachment_pinned",
            json!({ "messageId": 1, "index": 0, "pinned": true }),
        ),
        ("get_attachment_pins", json!({ "messageId": 1 })),
    ] {
        let err = invoke(&h, cmd, args).expect_err(cmd);
        assert_eq!(code_of(&err), Some("not_found"), "{cmd}: {err}");
    }
    assert_eq!(
        invoke(
            &h,
            "list_pinned_attachments",
            json!({ "scope": { "kind": "all" } })
        )
        .expect("pinned"),
        json!([])
    );
    assert_eq!(
        invoke(
            &h,
            "count_blocked_hidden",
            json!({ "scope": { "kind": "unified", "view": { "kind": "all" } } })
        )
        .expect("count"),
        json!(0)
    );
    let err = invoke(
        &h,
        "preview_eml_attachment",
        json!({ "fileId": "0".repeat(32), "index": 0, "page": 0 }),
    )
    .expect_err("no such file");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
}

#[test]
fn window_layout_belongs_to_the_calling_window() {
    let h = harness(|_| {});
    let layout = json!({
        "readingPane": "bottom",
        "density": "compact",
        "sidebarExpanded": false,
        "sidebarWidth": 300,
        "listWidth": 500,
        "listHeight": 260
    });
    // The accounts-and-folders column is shown by default, 240 pixels wide; the list is 384
    // pixels wide beside the reading pane and 320 high above it (stage 18).
    let default = json!({
        "readingPane": "right",
        "density": "comfortable",
        "sidebarExpanded": true,
        "sidebarWidth": 240,
        "listWidth": 384,
        "listHeight": 320
    });
    assert_eq!(
        invoke(&h, "get_window_layout", json!({})).expect("get"),
        default
    );
    assert_eq!(
        invoke(&h, "set_window_layout", json!({ "layout": layout })).expect("set"),
        layout
    );
    assert_eq!(
        invoke(&h, "get_window_layout", json!({})).expect("get"),
        layout
    );
    // The width is checked: 200 to 320 pixels.
    let mut too_wide = layout.clone();
    too_wide["sidebarWidth"] = json!(400);
    let err = invoke(&h, "set_window_layout", json!({ "layout": too_wide })).expect_err("width");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    // Stage 18: so are the list's width (320 to 1600) and height (160 to 1600), and both are
    // part of the layout the UI sends.
    for (field, value) in [("listWidth", 300), ("listHeight", 100), ("listWidth", 1700)] {
        let mut wrong = layout.clone();
        wrong[field] = json!(value);
        let err = invoke(&h, "set_window_layout", json!({ "layout": wrong })).expect_err(field);
        assert_eq!(
            code_of(&err),
            Some("invalid_argument"),
            "{field} {value}: {err}"
        );
    }
    let mut without = layout.clone();
    without
        .as_object_mut()
        .expect("object")
        .remove("listHeight");
    assert!(invoke(&h, "set_window_layout", json!({ "layout": without })).is_err());

    // A message window has a layout of its own, shared by all message windows.
    let label = |id: i64| {
        windows::message_window_label(
            SEEDED_ACCOUNT.parse().expect("id"),
            mail_model::MessageId(id),
        )
    };
    let first = WebviewWindowBuilder::new(&h.app, label(1), Default::default())
        .build()
        .expect("message window");
    assert_eq!(
        invoke_from(&first, "get_window_layout", json!({})).expect("get"),
        default
    );
    let replace = json!({
        "readingPane": "replace",
        "density": "comfortable",
        "sidebarExpanded": false,
        "sidebarWidth": 240,
        "listWidth": 384,
        "listHeight": 320
    });
    invoke_from(&first, "set_window_layout", json!({ "layout": replace })).expect("set");
    let second = WebviewWindowBuilder::new(&h.app, label(2), Default::default())
        .build()
        .expect("second message window");
    assert_eq!(
        invoke_from(&second, "get_window_layout", json!({})).expect("get"),
        replace
    );
    assert_eq!(
        invoke(&h, "get_window_layout", json!({})).expect("main unchanged"),
        layout
    );

    // The UI cannot name another window: an argument is ignored, the caller decides.
    assert_eq!(
        invoke(&h, "get_window_layout", json!({ "window": "message" })).expect("get"),
        layout
    );
    // A window the app does not know has no layout.
    let other = WebviewWindowBuilder::new(&h.app, "other", Default::default())
        .build()
        .expect("other window");
    let err = invoke_from(&other, "get_window_layout", json!({})).expect_err("no layout");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
}

/// The message window under the shipped configuration: opened by the main window for a stored
/// message, once per message, with the main window's permissions (listen yes, emit no) and the
/// same navigation decision; a window outside the capability's labels gets nothing.
#[test]
fn message_window_opens_once_with_the_main_windows_permissions() {
    let dir = TempDir::new().expect("temp dir");
    seed_account(dir.path());
    let specta = specta_builder::<MockRuntime>();
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(context())
        .expect("mock app with the real config");
    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir: dir.path().to_owned(),
            build: build_info(),
        },
        services(),
    ));
    attach_core(app.handle(), &specta, core);
    let main = open_main_window(app.handle(), None).expect("open main window");

    let mailboxes = invoke_from(
        &main,
        "list_mailboxes",
        json!({ "accountId": SEEDED_ACCOUNT }),
    )
    .expect("mailboxes");
    let inbox = mailboxes
        .as_array()
        .expect("array")
        .iter()
        .find(|m| m["path"] == "INBOX")
        .map(|m| m["id"].clone())
        .expect("inbox");
    let page = invoke_from(
        &main,
        "list_messages",
        json!({ "mailboxId": inbox, "cursor": null, "limit": 10 }),
    )
    .expect("list");
    let message_id = page["items"][0]["id"].as_i64().expect("message id");
    let label = windows::message_window_label(
        SEEDED_ACCOUNT.parse().expect("id"),
        mail_model::MessageId(message_id),
    );

    let args = json!({ "accountId": SEEDED_ACCOUNT, "messageId": message_id });
    assert_eq!(
        invoke_from(&main, "open_message_window", args.clone()).expect("open"),
        Value::Null
    );
    let window = app.get_webview_window(&label).expect("message window");
    let url = window.url().expect("url");
    assert_eq!(url.path(), "/index.html");
    assert_eq!(
        url.query(),
        Some(format!("window=message&accountId={SEEDED_ACCOUNT}&messageId={message_id}").as_str())
    );

    // A second call focuses the same window.
    invoke_from(&main, "open_message_window", args).expect("again");
    assert_eq!(
        app.webview_windows().len(),
        2,
        "main and one message window"
    );

    // Unknown message, or one of another account: not found, no window.
    let err = invoke_from(
        &main,
        "open_message_window",
        json!({ "accountId": SEEDED_ACCOUNT, "messageId": 999_999 }),
    )
    .expect_err("unknown message");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
    let err = invoke_from(
        &main,
        "open_message_window",
        json!({ "accountId": "6f9619ff-8b86-4011-b42d-00c04fc964ff", "messageId": message_id }),
    )
    .expect_err("other account");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
    assert_eq!(app.webview_windows().len(), 2);

    // The message window: app commands and listening work, emitting does not.
    assert!(invoke_from(&window, "get_app_info", json!({})).is_ok());
    let listen = |w: &WebviewWindow<MockRuntime>| {
        invoke_from(
            w,
            "plugin:event|listen",
            json!({ "event": CORE_EVENT_NAME, "target": { "kind": "Any" }, "handler": 7 }),
        )
    };
    assert!(listen(&window).is_ok(), "listen must be allowed");
    let emit = invoke_from(
        &window,
        "plugin:event|emit",
        json!({ "event": CORE_EVENT_NAME, "payload": null }),
    );
    assert!(
        emit.as_ref()
            .err()
            .and_then(Value::as_str)
            .is_some_and(|m| m.contains("not allowed")),
        "the ACL must refuse emit from a message window: {emit:?}"
    );
    // Message and composer windows may close themselves (capability `secondary-windows`), the
    // main window may not.
    let not_allowed = |r: &Result<Value, Value>| {
        r.as_ref()
            .err()
            .and_then(Value::as_str)
            .is_some_and(|m| m.contains("not allowed"))
    };
    let main = app.get_webview_window("main").expect("main window");
    let close_main = invoke_from(&main, "plugin:window|close", json!({ "label": "main" }));
    assert!(
        not_allowed(&close_main),
        "the main window must not close itself: {close_main:?}"
    );
    let label = window.label().to_owned();
    let close_message = invoke_from(&window, "plugin:window|close", json!({ "label": label }));
    assert!(
        !not_allowed(&close_message),
        "a message window may close itself: {close_message:?}"
    );
    // A window outside the capability's labels has no permission at all.
    let stranger = WebviewWindowBuilder::new(&app, "stranger", Default::default())
        .build()
        .expect("stranger");
    assert!(
        listen(&stranger)
            .err()
            .and_then(|e| e.as_str().map(str::to_owned))
            .is_some_and(|m| m.contains("not allowed")),
        "no capability for other windows"
    );

    // Composer windows (stage 10 composer in a window of its own): one per draft, the same
    // capability. An id that is no UUID or an unknown draft opens nothing.
    let err = invoke_from(&main, "open_composer_window", json!({ "draftId": "../x" }))
        .expect_err("not a uuid");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    let unknown = "0b5e8f43-6c1f-4a51-9a51-0c4f5b3f6a02";
    let err = invoke_from(&main, "open_composer_window", json!({ "draftId": unknown }))
        .expect_err("unknown draft");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
    assert_eq!(app.webview_windows().len(), 3, "main, message, stranger");
    let draft = mail_model::DraftId::parse(unknown).expect("draft id");
    let composer = windows::open_composer_window(app.handle(), &draft, "Re: Lunch", None)
        .expect("composer window");
    assert_eq!(composer.label(), windows::composer_window_label(&draft));
    assert_eq!(
        composer.url().expect("url").query(),
        Some(format!("window=composer&draftId={unknown}").as_str())
    );
    // Opening it again (a parallel call that lost the race) focuses the same window.
    windows::open_composer_window(app.handle(), &draft, "Re: Lunch", None).expect("again");
    assert_eq!(app.webview_windows().len(), 4);
    assert!(listen(&composer).is_ok(), "listen must be allowed");
    assert!(
        invoke_from(
            &composer,
            "plugin:event|emit",
            json!({ "event": CORE_EVENT_NAME, "payload": null }),
        )
        .is_err(),
        "the ACL must refuse emit from a composer window"
    );
    assert_eq!(
        invoke_from(&composer, "get_window_layout", json!({})).expect("layout")["readingPane"],
        "right"
    );

    // The navigation decision every window is built with: the app itself, never a link
    // placeholder (it is reported to the core instead) or anything else.
    let decide = windows::navigation_policy(app.handle());
    let app_url = if cfg!(windows) {
        "http://tauri.localhost/index.html?window=message"
    } else {
        "tauri://localhost/index.html?window=message"
    };
    assert!(decide(&app_url.parse().expect("url")));
    let placeholder = mail_model::link_placeholder(
        &mail_model::RenderToken::parse(&"a".repeat(32)).expect("token"),
        0,
    );
    assert!(!decide(&placeholder.parse().expect("url")));
    assert!(!decide(&"https://example.com/".parse().expect("url")));
}

/// Stage 7 (decision 0026): saved and recent searches through the real command handler, with
/// the typed error codes on the wire.
#[test]
fn saved_and_recent_searches_cross_the_bridge() {
    let h = harness(|_| {});
    let saved = invoke(
        &h,
        "save_search",
        json!({ "name": " Unread ", "query": "is:unread" }),
    )
    .expect("save_search");
    assert_eq!(saved["name"], "Unread");
    assert_eq!(saved["query"], "is:unread");
    let id = saved["id"].as_str().expect("id").to_owned();
    let err =
        invoke(&h, "save_search", json!({ "name": "unread", "query": "x" })).expect_err("taken");
    assert_eq!(err["code"], "saved_search_exists", "{err}");
    let renamed = invoke(
        &h,
        "rename_saved_search",
        json!({ "savedSearchId": id, "name": "Neu" }),
    )
    .expect("rename");
    assert_eq!(renamed["name"], "Neu");
    let list = invoke(&h, "list_saved_searches", json!({})).expect("list");
    assert_eq!(list.as_array().map(Vec::len), Some(1));
    invoke(&h, "delete_saved_search", json!({ "savedSearchId": id })).expect("delete");
    let err = invoke(&h, "delete_saved_search", json!({ "savedSearchId": id })).expect_err("gone");
    assert_eq!(err["code"], "not_found");

    let recent = invoke(&h, "add_recent_search", json!({ "query": "from:bob" })).expect("add");
    assert_eq!(recent[0]["query"], "from:bob");
    assert!(recent[0]["usedAt"].is_number());
    invoke(&h, "clear_recent_searches", json!({})).expect("clear");
    let recent = invoke(&h, "list_recent_searches", json!({})).expect("list");
    assert_eq!(recent, json!([]));
}

/// Stage 9 (decision 0027): an `.eml` file the system hands over opens read-only in a message
/// window of its own (the main window's permissions), is served by the core through the reading
/// pipeline, keeps a refusal for its window, and is forgotten when the window closes. Stage 13:
/// `get_messages_by_ids` names stored messages and leaves unknown ones out.
#[test]
fn eml_files_open_in_message_windows_and_messages_come_by_id() {
    let dir = TempDir::new().expect("temp dir");
    seed_account(dir.path());
    let specta = specta_builder::<MockRuntime>();
    let app = mock_builder()
        .invoke_handler(specta.invoke_handler())
        .build(context())
        .expect("mock app with the real config");
    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir: dir.path().to_owned(),
            build: build_info(),
        },
        services(),
    ));
    attach_core(app.handle(), &specta, core);
    let main = open_main_window(app.handle(), None).expect("open main window");

    let files = TempDir::new().expect("files");
    let good = files.path().join("Letter.eml");
    fs::write(
        &good,
        "From: Grace <grace@example.org>\r\nSubject: From a file\r\nContent-Type: text/html\r\n\r\n\
         <p>Hi <img src=\"https://tracker.example/p.png\"><script>x()</script></p>\r\n",
    )
    .expect("write");
    let bad = files.path().join("notes.eml");
    fs::write(&bad, "no mail here").expect("write");
    let paths = eml_files::eml_arguments(
        vec![
            good.display().to_string(),
            "mailto:bob@example.org".into(),
            bad.display().to_string(),
        ],
        None,
    );
    assert_eq!(paths, [good.clone(), bad.clone()]);
    eml_files::open_files(app.handle(), paths);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let eml_windows = || {
        app.webview_windows()
            .into_values()
            .filter(|w| w.label().starts_with("message-file-"))
            .collect::<Vec<_>>()
    };
    while eml_windows().len() < 2 {
        assert!(std::time::Instant::now() < deadline, "no eml windows");
        std::thread::sleep(Duration::from_millis(20));
    }
    let file_id = |w: &WebviewWindow<MockRuntime>| {
        let url = w.url().expect("url");
        assert_eq!(url.path(), "/index.html");
        url.query_pairs()
            .find(|(k, _)| k == "fileId")
            .map(|(_, v)| v.into_owned())
            .expect("fileId")
    };
    // The window of the readable file is the one the core serves; the other keeps its error.
    let (served, refused): (Vec<_>, Vec<_>) = eml_windows().into_iter().partition(|w| {
        invoke_from(
            w,
            "get_eml_message",
            json!({ "fileId": file_id(w), "format": "auto" }),
        )
        .is_ok()
    });
    assert_eq!((served.len(), refused.len()), (1, 1));
    let (letter, notes) = (&served[0], &refused[0]);
    let letter_id = file_id(letter);
    assert_eq!(
        letter.url().expect("url").query(),
        Some(format!("window=eml&fileId={letter_id}").as_str())
    );

    let view = invoke_from(
        letter,
        "get_eml_message",
        json!({ "fileId": letter_id, "format": "auto" }),
    )
    .expect("view");
    assert_eq!(view["fileName"], "Letter.eml");
    assert_eq!(view["header"]["subject"], "From a file");
    assert_eq!(view["body"]["kind"], "html");
    assert_eq!(view["body"]["blockedRemote"], 1);
    let srcdoc = view["body"]["srcdoc"].as_str().expect("srcdoc");
    assert!(!srcdoc.contains("<script") && !srcdoc.contains("tracker.example"));
    let err = invoke_from(
        notes,
        "get_eml_message",
        json!({ "fileId": file_id(notes), "format": "auto" }),
    )
    .expect_err("not a mail");
    assert_eq!(code_of(&err), Some("eml_invalid"), "{err}");
    let err = invoke_from(
        letter,
        "get_eml_message",
        json!({ "fileId": "../../etc/passwd", "format": "auto" }),
    )
    .expect_err("not an id");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    let err = invoke_from(
        letter,
        "open_eml_attachment",
        json!({ "fileId": letter_id, "index": 0, "confirmedExecutable": false }),
    )
    .expect_err("no attachment");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
    // A message window: it may listen to core events, and its layout is the message windows'.
    assert!(
        invoke_from(
            letter,
            "plugin:event|listen",
            json!({ "event": CORE_EVENT_NAME, "target": { "kind": "Any" }, "handler": 7 }),
        )
        .is_ok()
    );
    assert!(invoke_from(letter, "get_window_layout", json!({})).is_ok());

    // Closing the window forgets the file (the mock runtime sends no window events, so the
    // hook the window is built with is called as Tauri calls it).
    let id = mail_model::EmlFileId::parse(&letter_id).expect("id");
    assert!(!windows::forget_eml_file(
        app.handle(),
        &id,
        &tauri::WindowEvent::Focused(false)
    ));
    assert!(windows::forget_eml_file(
        app.handle(),
        &id,
        &tauri::WindowEvent::Destroyed
    ));
    let err = invoke_from(
        &main,
        "get_eml_message",
        json!({ "fileId": letter_id, "format": "auto" }),
    )
    .expect_err("forgotten");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");

    // Stage 13: messages by id, in the order asked; unknown ones left out; bounded.
    let mailboxes = invoke_from(
        &main,
        "list_mailboxes",
        json!({ "accountId": SEEDED_ACCOUNT }),
    )
    .expect("mailboxes");
    let inbox = mailboxes
        .as_array()
        .expect("array")
        .iter()
        .find(|m| m["path"] == "INBOX")
        .map(|m| m["id"].clone())
        .expect("inbox");
    let page = invoke_from(
        &main,
        "list_messages",
        json!({ "mailboxId": inbox, "cursor": null, "limit": 10 }),
    )
    .expect("list");
    let first = page["items"][0]["id"].as_i64().expect("message id");
    let items = invoke_from(
        &main,
        "get_messages_by_ids",
        json!({ "messageIds": [999_999, first] }),
    )
    .expect("by ids");
    let items = items.as_array().expect("array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], first);
    assert_eq!(items[0]["accountId"], SEEDED_ACCOUNT);
    let too_many: Vec<i64> = (1..=i64::from(mail_model::MAX_MESSAGES_BY_IDS) + 1).collect();
    let err = invoke_from(
        &main,
        "get_messages_by_ids",
        json!({ "messageIds": too_many }),
    )
    .expect_err("too many");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Decision 0039: the sorted inbox as conversations
// ---------------------------------------------------------------------------------------------

#[test]
fn conversation_commands_answer_typed_pages_and_errors() {
    let h = harness(|_| {});
    let unified = json!({ "kind": "unified", "view": { "kind": "all" } });
    // No account yet: empty pages of the right shape.
    let page = invoke(
        &h,
        "list_inbox_conversations",
        json!({ "scope": unified, "cursor": null, "limit": 50 }),
    )
    .expect("inbox");
    assert_eq!(page["rows"], json!([]));
    assert_eq!(page["conversations"], json!([]));
    assert_eq!(page["nextCursor"], Value::Null);
    let page = invoke(
        &h,
        "list_inbox_conversations_filtered",
        json!({ "scope": unified, "filter": { "kind": "unread" }, "cursor": null, "limit": 50 }),
    )
    .expect("filtered");
    assert_eq!(page, json!({ "items": [], "nextCursor": null }));
    let page = invoke(
        &h,
        "list_unread_first_conversations",
        json!({ "scope": unified, "cursor": null, "limit": 50 }),
    )
    .expect("unread first");
    assert_eq!(page["items"], json!([]));
    // A mailbox that does not exist, a limit out of range, no rows: typed errors.
    let err = invoke(
        &h,
        "list_inbox_conversations",
        json!({ "scope": { "kind": "mailbox", "mailboxId": 99 }, "cursor": null, "limit": 50 }),
    )
    .expect_err("unknown mailbox");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
    let err = invoke(
        &h,
        "list_inbox_conversations",
        json!({ "scope": unified, "cursor": null, "limit": 0 }),
    )
    .expect_err("limit out of range");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    let err = invoke(
        &h,
        "conversation_targets",
        json!({ "rows": [], "intent": { "kind": "action", "action": { "type": "archive" } } }),
    )
    .expect_err("no rows");
    assert_eq!(code_of(&err), Some("invalid_argument"), "{err}");
    let err = invoke(
        &h,
        "conversation_targets",
        json!({ "rows": [{ "mailboxId": 1, "messageId": 5 }], "intent": { "kind": "snooze" } }),
    )
    .expect_err("unknown message");
    assert_eq!(code_of(&err), Some("not_found"), "{err}");
}

// ---------------------------------------------------------------------------------------------
// Stage 18 (decision 0051): the new-mail popup's window and its rights
// ---------------------------------------------------------------------------------------------

/// The popup's window, wired as `run()` does (restricted handler, real capabilities): it may
/// call its own commands and listen to events, nothing else; the main window keeps everything.
#[test]
fn the_popup_window_may_only_call_its_own_commands() {
    let dir = TempDir::new().expect("temp dir");
    let specta = specta_builder::<MockRuntime>();
    let app = mock_builder()
        .invoke_handler(popup::restrict(specta.invoke_handler()))
        .build(context())
        .expect("mock app with the real config");
    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir: dir.path().to_owned(),
            build: build_info(),
        },
        services(),
    ));
    attach_core(app.handle(), &specta, core);
    let main = open_main_window(app.handle(), None).expect("open main window");
    let popup = WebviewWindowBuilder::new(
        &app,
        popup::POPUP_WINDOW,
        tauri::WebviewUrl::App(popup::POPUP_PAGE.into()),
    )
    .build()
    .expect("popup window");

    let empty = invoke_from(&popup, "get_new_mail_popup", json!({})).expect("own command");
    assert_eq!(empty["cards"], json!([]));
    assert_eq!(empty["more"], json!(0));
    assert_eq!(empty["appearance"]["accent"], json!("orange"));
    let dismissed = invoke_from(&popup, "dismiss_popup", json!({ "messageId": null }));
    assert!(dismissed.is_ok(), "{dismissed:?}");
    let gone = invoke_from(&popup, "read_from_popup", json!({ "messageId": 1 }))
        .expect_err("no such card");
    assert_eq!(code_of(&gone), Some("not_found"));

    for (command, args) in [
        ("list_accounts", json!({})),
        ("get_preferences", json!({})),
        ("get_app_info", json!({})),
        (
            "apply_action",
            json!({ "action": { "type": "delete" }, "messages": [], "requestId": "x" }),
        ),
        (
            "open_message_window",
            json!({ "accountId": "x", "messageId": 1 }),
        ),
    ] {
        let refused = invoke_from(&popup, command, args).expect_err(command);
        assert!(
            refused
                .as_str()
                .is_some_and(|m| m.contains("not allowed in the new-mail popup")),
            "{command}: {refused:?}"
        );
    }
    // Tauri's own permissions: listening only.
    let listen = invoke_from(
        &popup,
        "plugin:event|listen",
        json!({ "event": "new-mail-popup-changed", "target": { "kind": "Any" }, "handler": 7 }),
    );
    assert!(listen.is_ok(), "listen must be allowed: {listen:?}");
    for (command, args) in [
        (
            "plugin:event|emit",
            json!({ "event": CORE_EVENT_NAME, "payload": null }),
        ),
        (
            "plugin:window|start_dragging",
            json!({ "label": popup::POPUP_WINDOW }),
        ),
        (
            "plugin:window|close",
            json!({ "label": popup::POPUP_WINDOW }),
        ),
    ] {
        let denied = invoke_from(&popup, command, args);
        assert!(
            denied
                .as_ref()
                .err()
                .and_then(Value::as_str)
                .is_some_and(|m| m.contains("not allowed")),
            "{command}: {denied:?}"
        );
    }

    // The main window keeps every command, but cannot size the popup's window.
    assert!(invoke_from(&main, "list_accounts", json!({})).is_ok());
    let fit = invoke_from(&main, "fit_new_mail_popup", json!({ "height": 200.0 }))
        .expect_err("only the popup sizes itself");
    assert_eq!(code_of(&fit), Some("invalid_argument"));
}

/// Every command the popup may call exists under that name in the bindings.
#[test]
fn the_popup_s_commands_exist() {
    let dir = TempDir::new().expect("temp dir");
    let file = dir.path().join("bindings.ts");
    specta_builder::<tauri::Wry>()
        .export(typescript(), &file)
        .expect("tauri-specta export");
    let text = fs::read_to_string(&file).expect("bindings");
    for command in popup::POPUP_COMMANDS {
        assert!(
            text.contains(&format!("TAURI_INVOKE(\"{command}\"")),
            "{command} is not a command"
        );
    }
    assert!(text.contains("\"new-mail-popup-changed\""));
}

/// The core's events reach every window but the popup's.
#[test]
fn core_events_skip_the_popup() {
    use tauri::EventTarget;
    assert!(events::hears_core_events(&EventTarget::webview_window(
        MAIN_WINDOW
    )));
    assert!(events::hears_core_events(&EventTarget::webview_window(
        "message-1-2"
    )));
    assert!(events::hears_core_events(&EventTarget::App));
    assert!(!events::hears_core_events(&EventTarget::webview_window(
        popup::POPUP_WINDOW
    )));
    assert!(!events::hears_core_events(&EventTarget::webview(
        popup::POPUP_WINDOW
    )));
    assert!(!events::hears_core_events(&EventTarget::labeled(
        popup::POPUP_WINDOW
    )));
}
