//! Stage 21 (decision 0069): the main window comes and goes, the app keeps running.
//!
//! - **Closing the main window** (its close button, ⌘W, Alt+F4) with "keep running" on lets the
//!   window go, web view and all, and keeps the process: sync, the new-mail popup, notifications,
//!   the counter and the tray icon go on. Tauri asks to exit when the last window is gone
//!   (`ExitRequested` without a code); that request is held back. An exit with a code
//!   ([`quit`], the updater's restart) and macOS's own Quit (⌘Q, Dock menu) end the app as
//!   before. With "keep running" off the app quits with its last window, as before stage 21.
//! - **The first time** the window is closed that way, it stays open once and its page explains
//!   how the app keeps running and how to quit ([`CloseToBackgroundAsked`]); the page answers
//!   with `close_main_window`. Without a page that listens (still loading, or broken), the
//!   window simply goes.
//! - **Opening again** ([`show_main_window`]: Dock icon, tray icon, a second start, a
//!   notification, "Read" in the popup, a `mailto:` link) builds a new main window where the last
//!   one was. Requests for the main window that arrive while its page loads
//!   (`openMessageRequested`, `openInboxRequested`, `composeRequested`) wait here and go to the
//!   page once it says it listens (`main_window_ready`).
//! - **Started in the background** (`--background`, the login item of decision 0070): no window
//!   at all until one of the above.

use std::sync::{Mutex, MutexGuard, PoisonError};

use mail_engine::AppCore;
use mail_model::{BackgroundSettings, CoreEvent};
use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, WindowEvent,
};
use tauri_specta::Event;

use crate::MAIN_WINDOW;

/// The argument the login item starts the app with: no window until the user asks for one.
pub const BACKGROUND_ARG: &str = "--background";

/// Whether the app was started in the background (the login item).
pub fn started_in_background(args: impl IntoIterator<Item = String>) -> bool {
    args.into_iter().any(|arg| arg == BACKGROUND_ARG)
}

/// Stage 21: closing the main window keeps the app running; the page explains it once. Sent to
/// the main window only, which then asks for `close_main_window`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase")]
pub struct CloseToBackgroundAsked;

/// What closing the main window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// The window closes; with the last window the app quits ("keep running" off, or quitting).
    Close,
    /// The window and its web view go; the app keeps running.
    Background,
    /// The window stays this once; its page explains running in the background.
    Explain,
}

/// The decision for a close of the main window.
pub fn close_action(
    keep_running: bool,
    quitting: bool,
    hint_shown: bool,
    page_listens: bool,
) -> CloseAction {
    if quitting || !keep_running {
        CloseAction::Close
    } else if !hint_shown && page_listens {
        CloseAction::Explain
    } else {
        CloseAction::Background
    }
}

/// Whether an exit request is held back: only the one Tauri makes when the last window closed
/// (no code), while "keep running" is on and the user did not ask to quit.
pub fn keeps_running(code: Option<i32>, keep_running: bool, quitting: bool) -> bool {
    code.is_none() && keep_running && !quitting
}

/// Requests for the main window's page that wait while it loads.
pub fn waits_for_the_page(event: &CoreEvent) -> bool {
    matches!(
        event,
        CoreEvent::OpenMessageRequested { .. }
            | CoreEvent::OpenInboxRequested { .. }
            | CoreEvent::ComposeRequested { .. }
    )
}

/// Requests kept at most (a page that never loads must not grow this without end).
const MAX_HELD: usize = 32;

/// Where the main window was, to open it there again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Frame {
    position: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
    maximized: bool,
}

#[derive(Debug, Default)]
struct State {
    settings: BackgroundSettings,
    hint_shown: bool,
    quitting: bool,
    /// The main window's page listens to core events.
    page_listens: bool,
    held: Vec<CoreEvent>,
    frame: Option<Frame>,
    /// A main window is being built.
    opening: bool,
}

/// The shell's part of running in the background (managed state).
#[derive(Debug, Default)]
pub struct Lifecycle(Mutex<State>);

impl Lifecycle {
    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// With the stored settings (read once at start, then kept current by the commands).
    pub fn new(settings: BackgroundSettings, hint_shown: bool) -> Self {
        Self(Mutex::new(State {
            settings,
            hint_shown,
            ..State::default()
        }))
    }

    pub fn settings(&self) -> BackgroundSettings {
        self.state().settings
    }

    pub fn set_settings(&self, settings: BackgroundSettings) {
        self.state().settings = settings;
    }

    pub fn set_hint_shown(&self) {
        self.state().hint_shown = true;
    }

    pub fn quitting(&self) -> bool {
        self.state().quitting
    }

    fn close_action(&self) -> CloseAction {
        let s = self.state();
        close_action(
            s.settings.keep_running,
            s.quitting,
            s.hint_shown,
            s.page_listens,
        )
    }

    /// Whether to hold back an exit request with `code`.
    pub fn keeps_running(&self, code: Option<i32>) -> bool {
        let s = self.state();
        keeps_running(code, s.settings.keep_running, s.quitting)
    }

    /// Holds `event` for the main window's page if it does not listen yet; `false` sends it now.
    pub fn hold(&self, event: &CoreEvent) -> bool {
        if !waits_for_the_page(event) {
            return false;
        }
        let mut s = self.state();
        if s.page_listens {
            return false;
        }
        if s.held.len() >= MAX_HELD {
            s.held.remove(0);
        }
        s.held.push(event.clone());
        true
    }

    /// The page listens now: the requests that waited for it, oldest first.
    pub fn page_listens(&self) -> Vec<CoreEvent> {
        let mut s = self.state();
        s.page_listens = true;
        std::mem::take(&mut s.held)
    }

    /// A new main window is built (its page does not listen yet). `false` if one is on its way.
    fn begin_opening(&self) -> bool {
        let mut s = self.state();
        if s.opening {
            return false;
        }
        s.opening = true;
        true
    }

    fn end_opening(&self) {
        self.state().opening = false;
    }

    /// The main window was built: its page has yet to say that it listens.
    pub fn main_window_built(&self) {
        self.state().page_listens = false;
    }

    /// The main window is gone: until a new page listens, requests wait.
    fn main_window_gone(&self) {
        self.state().page_listens = false;
    }

    fn remember(&self, frame: Frame) {
        self.state().frame = Some(frame);
    }

    fn frame(&self) -> Option<Frame> {
        self.state().frame
    }

    fn begin_quit(&self) {
        self.state().quitting = true;
    }
}

/// Sets up the state from the core's settings (at start, before the first window).
pub fn install<R: Runtime>(app: &AppHandle<R>, core: &AppCore) {
    let (settings, hint) = tauri::async_runtime::block_on(async {
        (
            core.get_background_settings().await,
            core.close_hint_shown().await,
        )
    });
    let settings = settings.unwrap_or_else(|err| {
        tracing::warn!(%err, "background settings unreadable, using the defaults");
        BackgroundSettings::default()
    });
    app.manage(Lifecycle::new(settings, hint.unwrap_or(false)));
}

/// Wires the main window: closing it, its scale changing, it going away.
pub fn follow_main_window<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        lifecycle.main_window_built();
    }
    let app = app.clone();
    let handle = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            let Some(lifecycle) = app.try_state::<Lifecycle>() else {
                return;
            };
            match lifecycle.close_action() {
                CloseAction::Close => {}
                CloseAction::Background => {
                    remember_frame(&lifecycle, &handle);
                    tracing::info!("main window closed; the app keeps running in the background");
                }
                CloseAction::Explain => {
                    api.prevent_close();
                    if let Err(err) = CloseToBackgroundAsked.emit_to(&app, MAIN_WINDOW) {
                        // Nobody to explain it to: let the window go after all.
                        tracing::warn!(%err, "close note not sent; closing");
                        remember_frame(&lifecycle, &handle);
                        lifecycle.set_hint_shown();
                        let _ = handle.destroy();
                    }
                }
            }
        }
        WindowEvent::ScaleFactorChanged { .. } => crate::notifications::redraw_badge(&app),
        WindowEvent::Destroyed => {
            if let Some(lifecycle) = app.try_state::<Lifecycle>() {
                lifecycle.main_window_gone();
            }
        }
        _ => {}
    });
}

fn remember_frame<R: Runtime>(lifecycle: &Lifecycle, window: &WebviewWindow<R>) {
    let maximized = window.is_maximized().unwrap_or(false);
    if let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) {
        lifecycle.remember(Frame {
            position,
            size,
            maximized,
        });
    }
}

/// The page answered the note: the window goes into the background (`quit` false) or the app
/// quits. Either way the note is not shown again on this device.
pub async fn close_main_window<R: Runtime>(app: &AppHandle<R>, quit_app: bool) {
    if let Some(core) = app.try_state::<AppCore>()
        && let Err(err) = core.set_close_hint_shown().await
    {
        tracing::warn!(%err, "could not remember the close note");
    }
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        lifecycle.set_hint_shown();
    }
    if quit_app {
        quit(app);
    } else if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        // Asks again (CloseRequested): now it goes into the background.
        if let Err(err) = window.close() {
            tracing::warn!(%err, "main window not closed");
        }
    }
}

/// Quits the app (tray menu, the close note, the second start on Windows never does).
pub fn quit<R: Runtime>(app: &AppHandle<R>) {
    if let Some(lifecycle) = app.try_state::<Lifecycle>() {
        lifecycle.begin_quit();
    }
    tracing::info!("quit requested");
    app.exit(0);
}

/// Brings the main window to the front; builds it first if it was closed (or never opened).
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let Some(lifecycle) = app.try_state::<Lifecycle>() else {
        return;
    };
    if lifecycle.quitting() || !lifecycle.begin_opening() {
        return;
    }
    // Built on the async runtime: building a web view from an event handler of the main thread
    // deadlocks on Windows (WebView2).
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let theme = match app.try_state::<AppCore>() {
            Some(core) => match core.get_preferences().await {
                Ok(preferences) => crate::window_theme::window_theme(preferences.theme),
                Err(_) => None,
            },
            None => None,
        };
        let frame = app.try_state::<Lifecycle>().and_then(|l| l.frame());
        match crate::open_main_window(&app, theme, frame.is_none()) {
            Ok(window) => {
                if let Some(frame) = frame {
                    let _ = window.set_size(frame.size);
                    let _ = window.set_position(frame.position);
                    if frame.maximized {
                        let _ = window.maximize();
                    }
                    let _ = window.show();
                }
                let _ = window.set_focus();
                tracing::info!("main window opened again");
            }
            Err(err) => tracing::error!(%err, "could not open the main window"),
        }
        if let Some(lifecycle) = app.try_state::<Lifecycle>() {
            lifecycle.end_opening();
        }
    });
}

#[cfg(test)]
mod tests {
    use mail_model::{AccountId, ComposeRequest, MessageId};

    use super::*;

    fn account() -> AccountId {
        "6f9619ff-8b86-4011-b42d-00c04fc964ff"
            .parse()
            .expect("account id")
    }

    #[test]
    fn closing_the_main_window_follows_the_settings_and_explains_once() {
        // Keep running off, or quitting: the window closes (the app quits with the last one).
        assert_eq!(close_action(false, false, false, true), CloseAction::Close);
        assert_eq!(close_action(true, true, false, true), CloseAction::Close);
        // The first close explains, if the page can show it.
        assert_eq!(close_action(true, false, false, true), CloseAction::Explain);
        // Without a listening page, or once explained: into the background.
        assert_eq!(
            close_action(true, false, false, false),
            CloseAction::Background
        );
        assert_eq!(
            close_action(true, false, true, true),
            CloseAction::Background
        );
    }

    #[test]
    fn only_the_last_window_closing_is_held_back() {
        assert!(keeps_running(None, true, false));
        assert!(!keeps_running(None, false, false), "keep running off");
        assert!(!keeps_running(None, true, true), "quitting");
        assert!(!keeps_running(Some(0), true, false), "quit from the menu");
        assert!(
            !keeps_running(Some(tauri::RESTART_EXIT_CODE), true, false),
            "the updater's restart"
        );
    }

    #[test]
    fn the_state_machine_from_start_to_quit() {
        let lifecycle = Lifecycle::new(BackgroundSettings::default(), false);
        // The page loads: closing now goes straight to the background, no note.
        lifecycle.main_window_built();
        assert_eq!(lifecycle.close_action(), CloseAction::Background);
        // The page listens: the first close explains.
        assert!(lifecycle.page_listens().is_empty());
        assert_eq!(lifecycle.close_action(), CloseAction::Explain);
        lifecycle.set_hint_shown();
        assert_eq!(lifecycle.close_action(), CloseAction::Background);
        assert!(lifecycle.keeps_running(None));
        // Gone: requests wait for the next page, in order, and are handed over once.
        lifecycle.main_window_gone();
        let open = CoreEvent::OpenMessageRequested {
            account_id: account(),
            mailbox_id: None,
            message_id: MessageId(7),
        };
        let compose = CoreEvent::ComposeRequested {
            request: ComposeRequest::default(),
        };
        assert!(lifecycle.hold(&open));
        assert!(lifecycle.hold(&compose));
        assert!(
            !lifecycle.hold(&CoreEvent::AccountsChanged),
            "not a request"
        );
        assert!(lifecycle.begin_opening());
        assert!(!lifecycle.begin_opening(), "one window at a time");
        lifecycle.main_window_built();
        lifecycle.end_opening();
        assert_eq!(lifecycle.page_listens(), vec![open.clone(), compose]);
        assert!(lifecycle.page_listens().is_empty());
        assert!(!lifecycle.hold(&open), "the page listens: sent at once");
        // Keep running switched off: the window closes and the app with it.
        lifecycle.set_settings(BackgroundSettings {
            keep_running: false,
            menu_bar_icon: true,
        });
        assert_eq!(lifecycle.close_action(), CloseAction::Close);
        assert!(!lifecycle.keeps_running(None));
        // Quit.
        lifecycle.set_settings(BackgroundSettings::default());
        lifecycle.begin_quit();
        assert!(lifecycle.quitting());
        assert_eq!(lifecycle.close_action(), CloseAction::Close);
        assert!(!lifecycle.keeps_running(None));
    }

    #[test]
    fn a_page_that_never_listens_keeps_only_the_newest_requests() {
        let lifecycle = Lifecycle::default();
        for n in 0..100 {
            assert!(lifecycle.hold(&CoreEvent::OpenMessageRequested {
                account_id: account(),
                mailbox_id: None,
                message_id: MessageId(n),
            }));
        }
        let held = lifecycle.page_listens();
        assert_eq!(held.len(), MAX_HELD);
        assert!(matches!(
            held.last(),
            Some(CoreEvent::OpenMessageRequested {
                message_id: MessageId(99),
                ..
            })
        ));
    }

    #[test]
    fn the_login_item_starts_without_a_window() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(started_in_background(args(&["--background"])));
        assert!(!started_in_background(args(&["mailto:a@b.example"])));
        assert!(!started_in_background(args(&[
            "--backgroundx",
            "background"
        ])));
    }
}
