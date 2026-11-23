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
