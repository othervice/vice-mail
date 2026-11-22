//! Desktop-test builds only (`--features e2e`, built by `pnpm --dir app build:e2e`). Nothing in
//! this module is part of a shipped build; `pnpm --dir app test:release-clean` checks the release
//! binary for it (decision 0005).
//!
//! - The embedded W3C WebDriver (`tauri-plugin-wdio-webdriver`) that WebdriverIO drives.
//! - A data directory of the test's own: [`DATA_DIR_ENV`] is required, so a test build never
//!   reads or writes the user's real data or logs, not even by accident.
//! - Test commands (`e2e_*`) for facts WebDriver cannot see, such as the native window buttons
//!   that macOS draws over the web view.
//! - A recorder for Content-Security-Policy violations from the first byte of the page on.
//! - An optional extra root CA ([`EXTRA_CA_ENV`]), so the desktop test can set up the account
//!   of the local test server, whose certificate the system does not trust.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use mail_engine::CoreServices;
use mail_model::{CoreError, ExtraRootCa};
use serde::Serialize;
use tauri::ipc::Invoke;
use tauri::{AppHandle, Builder, Runtime, WebviewWindow, Wry};

/// Absolute directory for the database and logs of one test run. Required in e2e builds.
pub const DATA_DIR_ENV: &str = "MAIL_APP_E2E_DATA_DIR";

/// Path of a PEM file with an additional trusted root CA (`testmail/.data/tls/ca.pem`). Optional;
/// certificates are still checked, against the system store plus this root.
pub const EXTRA_CA_ENV: &str = "MAIL_APP_E2E_EXTRA_CA";

/// Commands with this prefix go to the test handler; the app's own handler never sees them.
const COMMAND_PREFIX: &str = "e2e_";

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
        Box::new(tauri::generate_handler![e2e_window_controls]);
    builder
        // Listens on 127.0.0.1:$TAURI_WEBDRIVER_PORT (default 4445).
        .plugin(tauri_plugin_wdio_webdriver::init())
        .plugin(
            tauri::plugin::Builder::<Wry>::new("e2e-csp-recorder")
                .js_init_script(CSP_RECORDER)
                .build(),
        )
        .invoke_handler(move |invoke| {
            if invoke.message.command().starts_with(COMMAND_PREFIX) {
                test_commands(invoke)
            } else {
                app_commands(invoke)
            }
        })
}

/// Adds the root CA named by [`EXTRA_CA_ENV`], if set, to the sync settings.
pub fn with_test_ca(mut services: CoreServices) -> Result<CoreServices, CoreError> {
    services.sync.extra_root_ca = test_ca(std::env::var_os(EXTRA_CA_ENV))?;
    Ok(services)
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

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSWindowButton};
    use tauri::WebviewWindow;

    use super::{ControlRect, WindowControls};

    pub async fn window_controls(window: &WebviewWindow) -> Result<WindowControls, String> {
        // Only compared with the app's windows, never dereferenced, so it may cross threads
        // as a plain address.
        let target = window.ns_window().map_err(|err| err.to_string())? as usize;
        let (tx, rx) = tokio::sync::oneshot::channel();
        window
            .run_on_main_thread(move || {
                // The receiver only goes away when the command itself was dropped.
                let _ = tx.send(measure(target));
            })
            .map_err(|err| err.to_string())?;
        rx.await
            .map_err(|_| "the main thread dropped the measurement".to_owned())?
    }

    fn measure(target: usize) -> Result<WindowControls, String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let window = NSApplication::sharedApplication(mtm)
            .windows()
            .to_vec()
            .into_iter()
            .find(|w| Retained::as_ptr(w) as usize == target)
            .ok_or("the main window is not among the app's windows")?;
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
