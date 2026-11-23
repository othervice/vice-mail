//! Desktop-test builds only (`--features e2e`, built by `pnpm --dir app build:e2e`). Nothing in
//! this module is part of a shipped build; `pnpm --dir app test:release-clean` checks the release
//! binary for it (decision 0005).
//!
//! - The embedded W3C WebDriver (`tauri-plugin-wdio-webdriver`) that WebdriverIO drives.
//! - A data directory of the test's own: [`DATA_DIR_ENV`] is required, so a test build never
//!   reads or writes the user's real data or logs, not even by accident.
//! - Test commands (`e2e_*`) for facts WebDriver cannot see, such as the native window buttons
//!   that macOS draws over the web view, and for real (trusted) mouse clicks.
//! - A recorder for Content-Security-Policy violations from the first byte of the page on.
//! - A recorder of every command that reaches the invoke handler (a script in a mail frame
//!   that got hold of the IPC would show up there).
//! - A recording opener in place of the system's: "Open in browser" and opening an attachment
//!   start no other program; the test reads what would have been opened.
//! - An optional extra root CA ([`EXTRA_CA_ENV`]), so the desktop test can set up the account
//!   of the local test server, whose certificate the system does not trust.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use mail_engine::CoreServices;
use mail_engine::RecordingOpener;
use mail_model::{CoreError, ExtraRootCa};
use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Builder, Runtime, WebviewWindow, Wry};

/// Absolute directory for the database and logs of one test run. Required in e2e builds.
pub const DATA_DIR_ENV: &str = "MAIL_APP_E2E_DATA_DIR";

/// Path of a PEM file with an additional trusted root CA (`testmail/.data/tls/ca.pem`). Optional;
/// certificates are still checked, against the system store plus this root.
pub const EXTRA_CA_ENV: &str = "MAIL_APP_E2E_EXTRA_CA";

/// `1` lets the core load external images from loopback addresses (`127.0.0.0/8`, `::1`), where
/// the desktop test's counter server listens. Private and link-local addresses stay refused; the
/// product build has no such switch at all.
pub const LOOPBACK_IMAGES_ENV: &str = "MAIL_APP_E2E_LOOPBACK_IMAGES";

/// Address of the update manifest for the desktop test's local update server (decision 0037),
/// e.g. `http://127.0.0.1:<port>/latest.json`. Optional; without it the shipped endpoint from
/// `tauri.conf.json` applies. Plain HTTP is accepted only for loopback addresses.
pub const UPDATE_ENDPOINT_ENV: &str = "MAIL_APP_E2E_UPDATE_ENDPOINT";

/// The overriding update endpoint ([`UPDATE_ENDPOINT_ENV`]), if set and a valid URL.
pub fn update_endpoint() -> Option<tauri::Url> {
    let value = std::env::var(UPDATE_ENDPOINT_ENV).ok()?;
    match tauri::Url::parse(&value) {
        Ok(url) => Some(url),
        Err(err) => {
            tracing::error!(%err, "{UPDATE_ENDPOINT_ENV} is not a URL; ignored");
            None
        }
    }
}

/// Commands with this prefix go to the test handler; the app's own handler never sees them.
const COMMAND_PREFIX: &str = "e2e_";

/// Stands in for the system opener of the core: records URLs and file paths, opens nothing.
static OPENER: LazyLock<Arc<RecordingOpener>> =
    LazyLock::new(|| Arc::new(RecordingOpener::default()));

/// How often each command name reached the invoke handler since the app started.
static INVOKED: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

/// Runs at document start, before the app's own scripts, in the main frame only (like Tauri's
/// own scripts, decision 0005). The tests read `window.__e2eCspViolations`; WebDriver attaches
/// too late to see violations during start-up, and WebKit's `ReportingObserver` does not report
/// them.
const CSP_RECORDER: &str = r#"
window.__e2eCspViolations = [];
document.addEventListener("securitypolicyviolation", (e) => {
  window.__e2eCspViolations.push({
    directive: e.effectiveDirective,
    blocked: e.blockedURI,
    sample: e.sample,
    source: e.sourceFile,
    line: e.lineNumber,
  });
});
"#;

/// Adds the embedded WebDriver and the test commands to the app.
pub fn install(
    builder: Builder<Wry>,
    app_commands: impl Fn(Invoke<Wry>) -> bool + Send + Sync + 'static,
) -> Builder<Wry> {
    let test_commands: Box<dyn Fn(Invoke<Wry>) -> bool + Send + Sync> =
        Box::new(tauri::generate_handler![
            e2e_window_controls,
            e2e_native_mouse,
            e2e_opened,
            e2e_invoked
        ]);
    builder
        // Listens on 127.0.0.1:$TAURI_WEBDRIVER_PORT (default 4445).
        .plugin(tauri_plugin_wdio_webdriver::init())
        .plugin(
            tauri::plugin::Builder::<Wry>::new("e2e-csp-recorder")
                .js_init_script(CSP_RECORDER)
                .build(),
        )
        .invoke_handler(move |invoke| {
            record_invoke(invoke.message.command());
            if invoke.message.command().starts_with(COMMAND_PREFIX) {
                test_commands(invoke)
            } else {
                app_commands(invoke)
            }
        })
}

fn record_invoke(command: &str) {
    *INVOKED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(command.to_owned())
        .or_default() += 1;
}

/// Adds the root CA named by [`EXTRA_CA_ENV`], if set, to the sync settings, the loopback
/// switch for external images ([`LOOPBACK_IMAGES_ENV`]), and the recording opener: a test
/// build never hands a link or a file to the system.
pub fn with_test_ca(mut services: CoreServices) -> Result<CoreServices, CoreError> {
    services.reading.opener = OPENER.clone();
    services.sync.extra_root_ca = test_ca(std::env::var_os(EXTRA_CA_ENV))?;
    services.reading.remote.allow_loopback = loopback_images(std::env::var_os(LOOPBACK_IMAGES_ENV));
    if services.reading.remote.allow_loopback {
        tracing::info!("desktop-test build: external images may load from loopback addresses");
    }
    Ok(services)
}

/// Only exactly `1` switches it on.
fn loopback_images(value: Option<OsString>) -> bool {
    value.as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// Reads the PEM file named by the value of [`EXTRA_CA_ENV`]; unset or empty is `None`.
fn test_ca(value: Option<OsString>) -> Result<Option<ExtraRootCa>, CoreError> {
    let Some(path) = value.filter(|v| !v.is_empty()).map(PathBuf::from) else {
        return Ok(None);
    };
    let pem = std::fs::read(&path).map_err(|err| {
        CoreError::InvalidArgument(format!("{EXTRA_CA_ENV}={}: {err}", path.display()))
    })?;
    tracing::info!(path = %path.display(), "desktop-test build: trusting an extra root CA");
    Ok(Some(ExtraRootCa::from_pem(pem)))
}

/// The test run's data directory, in place of the app's data directory.
pub fn data_dir<R: Runtime>(_app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(test_root(std::env::var_os(DATA_DIR_ENV))?.join("data"))
}

/// The test run's log directory, in place of the app's log directory.
pub fn log_dir<R: Runtime>(_app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(test_root(std::env::var_os(DATA_DIR_ENV))?.join("logs"))
}

/// The test run's cache directory, in place of the app's cache directory.
pub fn cache_dir<R: Runtime>(_app: &AppHandle<R>) -> tauri::Result<PathBuf> {
    Ok(test_root(std::env::var_os(DATA_DIR_ENV))?.join("cache"))
}

/// Validates the value of [`DATA_DIR_ENV`]. There is no fallback to the real directories.
fn test_root(value: Option<OsString>) -> io::Result<PathBuf> {
    let Some(path) = value.filter(|v| !v.is_empty()).map(PathBuf::from) else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "this is a desktop-test build (feature e2e): set {DATA_DIR_ENV} to an absolute \
                 directory for its data; it never uses the user's data directory"
            ),
        ));
    };
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{DATA_DIR_ENV} must be an absolute path, got `{}`",
                path.display()
            ),
        ));
    }
    Ok(path)
}

/// One native window button, in CSS pixels from the top-left corner of the web view.
#[derive(Debug, Clone, Serialize)]
pub struct ControlRect {
    pub name: &'static str,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowControls {
    /// Close, minimise and zoom, where macOS draws them over the web view.
    pub buttons: Vec<ControlRect>,
    /// Height of the window's content view; with the overlay title bar the web view fills it.
    pub content_height: f64,
    /// Height of the title-bar area that the window lays over the content.
    pub titlebar_height: f64,
}

/// Where the operating system draws the window buttons. WebDriver only sees the web content,
/// so the test asks the window itself.
#[tauri::command]
pub async fn e2e_window_controls(window: WebviewWindow) -> Result<WindowControls, String> {
    #[cfg(target_os = "macos")]
    {
        macos::window_controls(&window).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Err("the native window buttons are only measured on macOS".into())
    }
}

/// A real left click at `x`/`y` (CSS pixels in the main frame's viewport, as
/// `getBoundingClientRect()` gives them; `viewport_height` is `innerHeight`, for the offset of a
/// title bar above the viewport). WebDriver's own clicks are synthetic (`el.click()`): untrusted
/// and without user activation, and they cannot reach into a mail frame. These are `NSEvent`s
/// handed to the WKWebView like the mouse's own (decision 0005).
#[tauri::command]
pub async fn e2e_native_mouse(
    window: WebviewWindow,
    x: f64,
    y: f64,
    viewport_height: Option<f64>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        macos::click(&window, x, y, viewport_height).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, x, y, viewport_height);
        Err("native mouse events are only implemented for macOS".into())
    }
}

/// Everything the core handed to the (recording) system opener so far: URLs as given, files as
/// their path.
#[tauri::command]
pub fn e2e_opened() -> Vec<String> {
    OPENER.opened()
}

/// Every command name that reached the invoke handler since the app started, with its count.
#[tauri::command]
pub fn e2e_invoked() -> BTreeMap<String, u64> {
    INVOKED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, NSObjectProtocol};
    use objc2::{MainThreadMarker, Message};
    use objc2_app_kit::{
        NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSView, NSWindow, NSWindowButton,
    };
    use objc2_foundation::{NSPoint, NSProcessInfo};
    use tauri::WebviewWindow;

    use super::{ControlRect, WindowControls};

    /// Event numbers of the synthesized mouse events; any increasing value will do.
    static EVENT_NUMBER: AtomicIsize = AtomicIsize::new(1000);

    pub async fn window_controls(window: &WebviewWindow) -> Result<WindowControls, String> {
        on_main_window(window, measure).await
    }

    pub async fn click(
        window: &WebviewWindow,
        x: f64,
        y: f64,
        viewport_height: Option<f64>,
    ) -> Result<String, String> {
        on_main_window(window, move |w| send_click(w, x, y, viewport_height)).await
    }

    /// Runs `f` on the main thread with the `NSWindow` of `window`.
    async fn on_main_window<T: Send + 'static>(
        window: &WebviewWindow,
        f: impl FnOnce(&NSWindow) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        // Only compared with the app's windows, never dereferenced, so it may cross threads
        // as a plain address.
        let target = window.ns_window().map_err(|err| err.to_string())? as usize;
        let (tx, rx) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || {
                // The receiver only goes away when the command itself was dropped.
                let _ = tx.send(find_window(target).and_then(|w| f(&w)));
            })
            .map_err(|err| err.to_string())?;
        rx.await
            .map_err(|_| "the main thread dropped the request".to_owned())?
    }

    fn find_window(target: usize) -> Result<Retained<NSWindow>, String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        NSApplication::sharedApplication(mtm)
            .windows()
            .to_vec()
            .into_iter()
            .find(|w| Retained::as_ptr(w) as usize == target)
            .ok_or_else(|| "the main window is not among the app's windows".to_owned())
    }

    /// The first view under `view` (itself included) that is a `class`.
    fn find_view(view: &NSView, class: &AnyClass) -> Option<Retained<NSView>> {
        if view.isKindOfClass(class) {
            return Some(view.retain());
        }
        view.subviews()
            .to_vec()
            .iter()
            .find_map(|sub| find_view(sub, class))
    }

    /// Left button down and up at one point of the web view, delivered to the WKWebView
    /// directly (`NSWindow.sendEvent` did not reach WebKit in Spike D).
    fn send_click(
        window: &NSWindow,
        x: f64,
        y: f64,
        viewport_height: Option<f64>,
    ) -> Result<String, String> {
        let class = AnyClass::get(c"WKWebView").ok_or("WebKit is not loaded")?;
        let content = window
            .contentView()
            .ok_or("the window has no content view")?;
        let view = find_view(&content, class).ok_or("the window has no WKWebView")?;
        let bounds = view.bounds();
        // With an overlay title bar the viewport fills the web view; otherwise it starts below.
        let top_inset = viewport_height.map_or(0.0, |h| (bounds.size.height - h).max(0.0));
        let y = y + top_inset;
        let local_y = if view.isFlipped() {
            y
        } else {
            bounds.size.height - y
        };
        let location = view.convertPoint_toView(NSPoint::new(x, local_y), None);
        let make = |kind: NSEventType| {
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                location,
                NSEventModifierFlags::empty(),
                NSProcessInfo::processInfo().systemUptime(),
                window.windowNumber(),
                None,
                EVENT_NUMBER.fetch_add(1, Ordering::Relaxed),
                1,
                1.0,
            )
            .ok_or_else(|| format!("could not create a {kind:?} event"))
        };
        let down = make(NSEventType::LeftMouseDown)?;
        let up = make(NSEventType::LeftMouseUp)?;
        view.mouseDown(&down);
        view.mouseUp(&up);
        Ok(format!(
            "clicked at ({:.1}, {:.1}) in a {:.0}x{:.0} web view, key window: {}",
            location.x,
            location.y,
            bounds.size.width,
            bounds.size.height,
            window.isKeyWindow()
        ))
    }

    fn measure(window: &NSWindow) -> Result<WindowControls, String> {
        let content = window
            .contentView()
            .ok_or("the window has no content view")?;
        let content_height = content.frame().size.height;
        let titlebar_height = content_height - window.contentLayoutRect().size.height;

        let buttons = [
            ("close", NSWindowButton::CloseButton),
            ("minimize", NSWindowButton::MiniaturizeButton),
            ("zoom", NSWindowButton::ZoomButton),
        ]
        .into_iter()
        .map(|(name, kind)| {
            let button = window
                .standardWindowButton(kind)
                .ok_or_else(|| format!("the window has no {name} button"))?;
            // Window coordinates have their origin at the bottom left; the web view's at the top.
            let rect = button.convertRect_toView(button.bounds(), None);
            Ok(ControlRect {
                name,
                x: rect.origin.x,
                y: content_height - rect.origin.y - rect.size.height,
                width: rect.size.width,
                height: rect.size.height,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

        Ok(WindowControls {
            buttons,
            content_height,
            titlebar_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_is_required_and_absolute() {
        let missing = test_root(None).expect_err("missing");
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert!(missing.to_string().contains(DATA_DIR_ENV));

        let empty = test_root(Some(OsString::new())).expect_err("empty");
        assert_eq!(empty.kind(), io::ErrorKind::NotFound);

        let relative = test_root(Some("relative/dir".into())).expect_err("relative");
        assert_eq!(relative.kind(), io::ErrorKind::InvalidInput);

        let dir = std::env::temp_dir().join("mail-app-e2e");
        assert_eq!(test_root(Some(dir.clone().into())).expect("absolute"), dir);
    }

    #[test]
    fn every_command_that_reaches_the_handler_is_counted() {
        record_invoke("e2e_test_probe");
        record_invoke("e2e_test_probe");
        record_invoke("sandbox_breach_probe");
        let seen = e2e_invoked();
        assert_eq!(seen.get("e2e_test_probe"), Some(&2));
        assert_eq!(seen.get("sandbox_breach_probe"), Some(&1));
    }

    #[test]
    fn the_test_build_never_hands_anything_to_the_system() {
        let services = CoreServices::new(mail_engine::credentials::Credentials::new(
            mail_engine::credentials::MemorySecretStore::new(),
        ));
        let services = with_test_ca(services).expect("services");
        services
            .reading
            .opener
            .open_url("https://example.com/e2e")
            .expect("recorded");
        assert!(e2e_opened().contains(&"https://example.com/e2e".to_owned()));
    }

    #[test]
    fn the_loopback_switch_needs_exactly_one() {
        assert!(loopback_images(Some("1".into())));
        for off in [
            None,
            Some(OsString::new()),
            Some("0".into()),
            Some("true".into()),
            Some("1 ".into()),
        ] {
            assert!(!loopback_images(off.clone()), "{off:?}");
        }
    }

    #[test]
    fn test_ca_is_optional_and_read_from_the_named_file() {
        assert_eq!(test_ca(None).expect("unset"), None);
        assert_eq!(test_ca(Some(OsString::new())).expect("empty"), None);
        let dir = tempfile::TempDir::new().expect("temp dir");
        let file = dir.path().join("ca.pem");
        std::fs::write(&file, b"-----BEGIN CERTIFICATE-----").expect("write");
        let ca = test_ca(Some(file.into())).expect("read").expect("set");
        assert_eq!(ca.pem(), b"-----BEGIN CERTIFICATE-----");
        let err = test_ca(Some(dir.path().join("missing.pem").into())).expect_err("missing");
        assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument);
        assert!(err.to_string().contains(EXTRA_CA_ENV));
    }
}
