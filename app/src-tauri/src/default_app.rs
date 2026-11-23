//! Stage 9: the app as the system's mail app (`mailto:`), decision 0020.
//!
//! - **macOS**: the bundle declares the `mailto` URL scheme (`Info.plist`, merged by Tauri).
//!   Asking and changing the default handler are Launch Services calls; calling them from Rust
//!   would need `unsafe` FFI (forbidden outside `webview2-guard`), so the app runs them through
//!   the system's own scripting host: `osascript -l JavaScript` with a fixed script and the
//!   bundle id as its only, checked input. `NSWorkspace.URLForApplicationToOpenURL` answers who
//!   handles `mailto:`, `LSSetDefaultHandlerForURLScheme` makes this app the handler. If that is
//!   refused (an unbundled development build), the UI explains Mail's own setting.
//! - **Windows**: the installer registers the app as a `mailto` handler (NSIS hooks,
//!   `windows/installer-hooks.nsh`); since Windows 10 only the user may choose the default, in
//!   Settings. The app opens that page (`ms-settings:defaultapps?registeredAppUser=…`) and
//!   cannot tell whether it is the default.

use mail_model::{DefaultMailApp, DefaultMailAppOutcome};
use tauri::{AppHandle, Runtime};

/// The bundle id, if it is safe to place into a script (letters, digits, `.` and `-`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn safe_identifier(identifier: &str) -> Option<&str> {
    let ok = !identifier.is_empty()
        && identifier.len() <= 255
        && identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    ok.then_some(identifier)
}

#[cfg(target_os = "macos")]
async fn jxa(script: String) -> Option<String> {
    let output = tokio::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", &script])
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(std::time::Duration::from_secs(10), output).await {
        Ok(Ok(out)) if out.status.success() => {
            Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        }
        Ok(Ok(out)) => {
            tracing::warn!(
                status = ?out.status.code(),
                stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                "osascript failed"
            );
            None
        }
        Ok(Err(err)) => {
            tracing::warn!(%err, "osascript could not run");
            None
        }
        Err(_) => {
            tracing::warn!("osascript did not answer in time");
            None
        }
    }
}

/// The bundle id of the app that handles `mailto:` now.
#[cfg(target_os = "macos")]
async fn current_handler() -> Option<String> {
    jxa(r#"ObjC.import("AppKit");
var u = $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL($.NSURL.URLWithString("mailto:a@example.org"));
u.isNil() ? "" : ObjC.unwrap($.NSBundle.bundleWithURL(u).bundleIdentifier)"#
        .to_owned())
    .await
}

#[cfg(target_os = "macos")]
pub(crate) async fn status<R: Runtime>(app: &AppHandle<R>) -> DefaultMailApp {
    let identifier = app.config().identifier.clone();
    let is_default = current_handler()
        .await
        .map(|handler| handler.eq_ignore_ascii_case(&identifier));
    DefaultMailApp {
        is_default,
        direct: true,
    }
}

#[cfg(target_os = "macos")]
pub(crate) async fn make_default<R: Runtime>(app: &AppHandle<R>) -> DefaultMailAppOutcome {
    let identifier = app.config().identifier.clone();
    let Some(id) = safe_identifier(&identifier) else {
        return DefaultMailAppOutcome::Manual;
    };
    let script = format!(
        r#"ObjC.import("CoreServices"); String($.LSSetDefaultHandlerForURLScheme($("mailto"), $("{id}")))"#
    );
    let status = jxa(script).await;
    let now = current_handler().await;
    if now.is_some_and(|h| h.eq_ignore_ascii_case(&identifier)) {
        tracing::info!("set as the default mail app");
        DefaultMailAppOutcome::Done
    } else {
        tracing::warn!(
            status = status.as_deref(),
            "could not become the default mail app"
        );
        DefaultMailAppOutcome::Manual
    }
}

#[cfg(windows)]
pub(crate) async fn status<R: Runtime>(_app: &AppHandle<R>) -> DefaultMailApp {
    DefaultMailApp {
        is_default: None,
        direct: false,
    }
}

#[cfg(windows)]
pub(crate) async fn make_default<R: Runtime>(app: &AppHandle<R>) -> DefaultMailAppOutcome {
    // The name the installer registered under `RegisteredApplications`.
    let name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| app.package_info().name.clone());
    let page = format!(
        "ms-settings:defaultapps?registeredAppUser={}",
        url_component(&name)
    );
    match open::that_detached(&page).or_else(|_| open::that_detached("ms-settings:defaultapps")) {
        Ok(()) => DefaultMailAppOutcome::OpenedSettings,
        Err(err) => {
            tracing::warn!(%err, "Settings could not be opened");
            DefaultMailAppOutcome::Manual
        }
    }
}

#[cfg(windows)]
fn url_component(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) async fn status<R: Runtime>(_app: &AppHandle<R>) -> DefaultMailApp {
    DefaultMailApp {
        is_default: None,
        direct: false,
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub(crate) async fn make_default<R: Runtime>(_app: &AppHandle<R>) -> DefaultMailAppOutcome {
    DefaultMailAppOutcome::Manual
}

/// `mailto:` URLs among the arguments of a start (Windows passes the link as an argument).
pub(crate) fn mailto_arguments<I: IntoIterator<Item = String>>(args: I) -> Vec<String> {
    args.into_iter()
        .filter(|a| {
            a.get(..7)
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case("mailto:"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_and_arguments() {
        assert_eq!(
            safe_identifier("org.example.mail"),
            Some("org.example.mail")
        );
        assert_eq!(safe_identifier("x\"); evil(\""), None);
        assert_eq!(safe_identifier(""), None);
        assert_eq!(
            mailto_arguments(vec![
                "C:\\app.exe".to_owned(),
                "MAILTO:bob@example.org?subject=Hi".to_owned(),
                "--flag".to_owned(),
            ]),
            vec!["MAILTO:bob@example.org?subject=Hi".to_owned()]
        );
    }
}
