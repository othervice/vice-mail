//! Spike D: HTML sandbox. Rust side of the "last line of defence" check.
//!
//! - `decide_navigation` is the navigation hook of the main window (`on_navigation`). WebKit asks
//!   it for every navigation, including navigations inside sub-frames. Only the app itself and
//!   `about:srcdoc` / `about:blank` may load; every other target is cancelled before a request
//!   leaves the process, logged and reported to the UI as a `SandboxEvent`.
//! - `sandbox_breach` is the command a hostile script would call if it ever reached the Tauri
//!   interface from inside a mail frame. Every call is recorded; the e2e test fails on any entry.
//! - Every command that reaches the invoke handler is logged (see `lib.rs`), so an IPC request
//!   from a frame would show up even if it named an unknown command.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, Url};
use tauri_specta::Event;

/// One navigation decision of the main window's navigation hook.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NavigationRecord {
    pub seq: u32,
    pub url: String,
    pub allowed: bool,
}

/// Everything the Rust side observed since the last `sandbox_reset`.
#[derive(Debug, Clone, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SandboxReport {
    pub navigations: Vec<NavigationRecord>,
    /// Vector ids passed to `sandbox_breach`. Must stay empty for every hostile vector.
    pub breaches: Vec<String>,
    /// Names of all commands that reached the invoke handler (capped).
    pub commands: Vec<String>,
}

/// Pushed to the UI when the navigation hook blocks a target (the "link target" the UI shows).
#[derive(Debug, Clone, Serialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct SandboxEvent {
    pub url: String,
    pub allowed: bool,
}

#[derive(Default)]
pub struct SandboxState {
    report: Mutex<SandboxReport>,
    seq: Mutex<u32>,
}

const MAX_COMMANDS: usize = 500;

impl SandboxState {
    fn with<T>(&self, f: impl FnOnce(&mut SandboxReport) -> T) -> T {
        // A poisoned lock only means a previous logger panicked; the data is still usable.
        let mut guard = self.report.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub fn log_command(&self, name: &str) {
        self.with(|r| {
            if r.commands.len() < MAX_COMMANDS {
                r.commands.push(name.to_owned());
            }
        });
    }

    fn log_navigation(&self, url: &Url, allowed: bool) -> u32 {
        let seq = {
            let mut s = self.seq.lock().unwrap_or_else(|e| e.into_inner());
            *s += 1;
            *s
        };
        self.with(|r| {
            r.navigations.push(NavigationRecord {
                seq,
                url: url.to_string(),
                allowed,
            })
        });
        seq
    }
}

/// Pure policy: may the webview (main frame or any sub-frame) navigate to `url`?
pub fn navigation_allowed(url: &Url) -> bool {
    match url.scheme() {
        // The app itself (macOS/Linux custom protocol).
        "tauri" => url.host_str() == Some("localhost"),
        // Initial documents of iframes: `srcdoc` content and empty frames.
        "about" => matches!(url.path(), "srcdoc" | "blank"),
        // `tauri dev` serves the UI from Vite; Windows serves the app as http://tauri.localhost.
        "http" => {
            (cfg!(dev) && url.host_str() == Some("localhost") && url.port() == Some(1420))
                || url.host_str() == Some("tauri.localhost")
                || e2e_positive_control(url)
        }
        _ => false,
    }
}

/// Test builds only: the positive control for link clicks may reach the counter server under
/// `/allowed/`, which proves that a click really activates a link inside a frame.
#[cfg(feature = "e2e")]
fn e2e_positive_control(url: &Url) -> bool {
    url.host_str() == Some("127.0.0.1")
        && url.port() == Some(18999)
        && url.path().starts_with("/allowed/")
}

#[cfg(not(feature = "e2e"))]
fn e2e_positive_control(_url: &Url) -> bool {
    false
}

/// Navigation hook of the main window. Runs on the main thread for every navigation action.
pub fn decide_navigation(app: &AppHandle, url: &Url) -> bool {
    let allowed = navigation_allowed(url);
    if let Some(state) = app.try_state::<SandboxState>() {
        let seq = state.log_navigation(url, allowed);
        if !allowed || url.scheme() == "http" {
            println!(
                "[sandbox] navigation #{seq} {} {url}",
                if allowed { "ALLOWED" } else { "BLOCKED" }
            );
        }
    }
    if !allowed || url.path().starts_with("/allowed/") {
        let event = SandboxEvent {
            url: url.to_string(),
            allowed,
        };
        if let Err(err) = event.emit(app) {
            eprintln!("[sandbox] failed to emit event: {err}");
        }
    }
    allowed
}

/// Called by hostile test scripts if they ever get to run with access to the Tauri interface.
#[tauri::command]
#[specta::specta]
pub fn sandbox_breach(state: tauri::State<'_, SandboxState>, vector: String) {
    eprintln!("[sandbox] BREACH: script of vector `{vector}` reached the Tauri interface");
    state.with(|r| r.breaches.push(vector));
}

#[tauri::command]
#[specta::specta]
pub fn sandbox_report(state: tauri::State<'_, SandboxState>) -> SandboxReport {
    state.with(|r| r.clone())
}

#[tauri::command]
#[specta::specta]
pub fn sandbox_reset(state: tauri::State<'_, SandboxState>) {
    state.with(|r| *r = SandboxReport::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn app_and_frame_documents_are_allowed() {
        assert!(navigation_allowed(&url("tauri://localhost/")));
        assert!(navigation_allowed(&url("tauri://localhost/index.html")));
        assert!(navigation_allowed(&url("about:srcdoc")));
        assert!(navigation_allowed(&url("about:blank")));
        assert!(navigation_allowed(&url("http://tauri.localhost/")));
    }

    #[test]
    fn everything_else_is_blocked() {
        for u in [
            "http://127.0.0.1:18999/v/link-plain/x",
            "https://example.com/",
            "mailto:someone@example.test",
            "tauri://evil.example/",
            "file:///etc/passwd",
            "data:text/html,hi",
            "blob:tauri://localhost/123",
            "about:config",
            "ipc://localhost/sandbox_breach",
            "http://localhost:1421/",
        ] {
            assert!(!navigation_allowed(&url(u)), "{u} must be blocked");
        }
    }

    #[test]
    #[cfg(not(feature = "e2e"))]
    fn positive_control_path_is_blocked_without_e2e_feature() {
        assert!(!navigation_allowed(&url(
            "http://127.0.0.1:18999/allowed/nav"
        )));
    }
}
