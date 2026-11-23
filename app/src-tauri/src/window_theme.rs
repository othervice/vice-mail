//! The native window frame follows the app's theme choice.
//!
//! The UI applies the theme to the page itself; the frame around it belongs to the operating
//! system. On Windows the title bar would stay light over a dark app (or the reverse) whenever
//! the choice differs from the system setting, and on macOS the same holds for the window
//! buttons. `System` hands the decision back to the operating system.

use mail_model::{CoreEvent, ThemePreference};
use tauri::{Manager, Runtime, Theme, WebviewWindow};
use tokio::sync::broadcast::{self, error::RecvError};

/// The window theme for a theme preference; `None` follows the system.
pub fn window_theme(preference: ThemePreference) -> Option<Theme> {
    match preference {
        ThemePreference::System => None,
        ThemePreference::Light => Some(Theme::Light),
        ThemePreference::Dark => Some(Theme::Dark),
    }
}

/// Applies every theme change to `window` until the core shuts down or the window is closed
/// (message windows, stage 8).
pub fn follow<R: Runtime>(window: WebviewWindow<R>, mut events: broadcast::Receiver<CoreEvent>) {
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(CoreEvent::PreferencesChanged { preferences }) => {
                    if window.get_webview_window(window.label()).is_none() {
                        break;
                    }
                    if let Err(err) = window.set_theme(window_theme(preferences.theme)) {
                        tracing::warn!(%err, "could not apply the theme to the window frame");
                    }
                }
                Ok(_) => {}
                // A missed change is caught up by the next one; the page is right regardless.
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_follows_the_operating_system() {
        assert_eq!(window_theme(ThemePreference::System), None);
        assert_eq!(window_theme(ThemePreference::Light), Some(Theme::Light));
        assert_eq!(window_theme(ThemePreference::Dark), Some(Theme::Dark));
    }
}
