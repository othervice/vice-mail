use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use mail_engine::{AppCore, BuildInfo, CoreConfig, DATABASE_FILE};
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
/// `UPDATE_BINDINGS=1 cargo test -p mail-app bindings` rewrites it.
#[test]
fn bindings_are_fresh() {
    let committed = manifest_dir().join("../src/generated/bindings.ts");
    let dir = TempDir::new().expect("temp dir");
    let fresh = dir.path().join("bindings.ts");
    specta_builder::<tauri::Wry>()
        .export(typescript(), &fresh)
        .expect("tauri-specta export");
    let fresh_text = fs::read_to_string(&fresh).expect("read fresh bindings");

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
    let core = tauri::async_runtime::block_on(AppCore::start(CoreConfig {
        data_dir,
        build: build_info(),
    }));
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
    assert_eq!(
        err["detail"],
        "database schema version 999, this app supports up to 1"
    );
}

/// Leaves a database in `data_dir` as a newer app version would: a current database whose
/// header claims schema version 999 (bytes 60..64 of an SQLite file hold `user_version`).
fn write_newer_database(data_dir: &std::path::Path) {
    let core = tauri::async_runtime::block_on(AppCore::start(CoreConfig {
        data_dir: data_dir.to_owned(),
        build: build_info(),
    }));
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

    let core = tauri::async_runtime::block_on(AppCore::start(CoreConfig {
        data_dir: dir.path().to_owned(),
        build: build_info(),
    }));
    attach_core(app.handle(), &specta, core);
    open_main_window(app.handle()).expect("open main window");
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
