//! Stage 18 (decision 0051): the window of the new-mail popup at the bottom right of the screen.
//!
//! The core keeps the cards and decides what is announced ([`mail_engine::comfort`]); this
//! module only shows them. Every [`PopupUpdate`] of the core goes through one queue, so the
//! window follows the changes in order:
//! - cards and no window: the window is built (through [`windows::guard`] and
//!   [`windows::finish`] like every window: navigation hook, no new windows, no downloads,
//!   WebView2 frame guard) at the bottom right of the work area of the monitor that shows the
//!   main window, else the primary one;
//! - cards and a window: the window gets the event [`NewMailPopupChanged`];
//! - no cards: the window is destroyed (no idle web view);
//! - a sound: played by the shell ([`crate::sound`]), as a system notification would.
//!
//! **Never takes the focus.** No decorations, always on top, not in the taskbar or the Window
//! menu, on every Space or desktop. `focused(false)` and `focusable(false)`: on macOS the window
//! can never become key (it is ordered in front without activating the app) and
//! `accept_first_mouse` lets the first click hit a button; on Windows it is created with
//! `WS_EX_NOACTIVATE` and shown without activation. On macOS the window appears once the page
//! has measured itself ([`fit`]); on Windows it is created visible (showing it later would
//! activate it).
//!
//! **Own, minimal rights.** The popup's page (`popup.html`, its own small bundle) may call only
//! [`POPUP_COMMANDS`]; [`restrict`] refuses every other command from this window before it runs
//! (the app's commands are not covered by Tauri's capabilities, see decision 0051). Its
//! capability (`capabilities/new-mail-popup.json`) only lets it listen to events, and the core's
//! events are not sent to it ([`crate::events`]); it hears [`NewMailPopupChanged`] alone.

use std::path::PathBuf;

use mail_engine::AppCore;
use mail_engine::comfort::PopupUpdate;
use mail_model::{NewMailPopup, ThemePreference};
use serde::{Deserialize, Serialize};
use tauri::ipc::Invoke;
use tauri::window::Color;
use tauri::{AppHandle, Manager, Runtime, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri::{LogicalSize, PhysicalPosition, PhysicalRect, PhysicalSize};
use tauri_specta::Event;
use tokio::sync::mpsc;

use crate::{MAIN_WINDOW, sound, windows};

/// Label of the popup's window (and its capability's window).
pub const POPUP_WINDOW: &str = "new-mail-popup";

/// The popup's own page: a separate entry of the UI build with only what the popup needs.
pub const POPUP_PAGE: &str = "popup.html";

/// Width of the popup (logical pixels).
pub const POPUP_WIDTH: f64 = 360.0;

/// Distance from the right and bottom edge of the work area (logical pixels).
pub const POPUP_MARGIN: f64 = 16.0;

/// The height the page may ask for (logical pixels); three cards and "N more" fit.
const MIN_HEIGHT: f64 = 48.0;
const MAX_HEIGHT: f64 = 640.0;

/// What the cards measure in the page before it measured itself (`docs/DESIGN.md`, "Popup für
/// neue Mails"): a card, the line "N more", the padding of the list.
const CARD_HEIGHT: f64 = 132.0;
const MORE_HEIGHT: f64 = 40.0;

/// Corner radius of the window on macOS (`--ui-radius-panel`); Windows 11 rounds undecorated
/// windows with a shadow itself.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const CORNER_RADIUS: f64 = 16.0;

/// The commands the popup's page may call. Everything else from its window is refused.
pub const POPUP_COMMANDS: &[&str] = &[
    "get_new_mail_popup",
    "read_from_popup",
    "delete_from_popup",
    "undo_popup_delete",
    "dismiss_popup",
    "open_inbox_from_popup",
    "fit_new_mail_popup",
];

/// The popup changed while its window is open (cards added, deleted, undone, dismissed). Sent
/// to the popup's window only.
#[derive(Debug, Clone, Serialize, Deserialize, tauri_specta::Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "new-mail-popup-changed")]
pub struct NewMailPopupChanged(pub NewMailPopup);

/// The event's type in TypeScript is exactly `NewMailPopup` (as for `CoreEventMessage`).
impl specta::Type for NewMailPopupChanged {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <NewMailPopup as specta::Type>::definition(types)
    }
}

/// Whether the window `label` may call `command`: every window but the popup's may call
/// anything (the capability `default` names them), the popup only [`POPUP_COMMANDS`].
pub fn may_call(label: &str, command: &str) -> bool {
    label != POPUP_WINDOW || POPUP_COMMANDS.contains(&command)
}

/// Wraps the app's invoke handler: a command from the popup's window that it may not call is
/// refused before it runs (logged with its name only).
pub fn restrict<R: Runtime>(
    handler: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        let allowed = may_call(
            invoke.message.webview_ref().label(),
            invoke.message.command(),
        );
        if !allowed {
            let command = invoke.message.command().to_owned();
            tracing::warn!(%command, "refused a command from the new-mail popup");
            invoke
                .resolver
                .reject(format!("{command} is not allowed in the new-mail popup"));
            return true;
        }
        handler(invoke)
    }
}

/// The popup's queue: the core's updates, applied one after the other on the async runtime.
pub(crate) struct PopupWindow {
    updates: mpsc::UnboundedSender<PopupUpdate>,
}

impl PopupWindow {
    pub(crate) fn install<R: Runtime>(app: &AppHandle<R>) -> Self {
        let (updates, mut receiver) = mpsc::unbounded_channel::<PopupUpdate>();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(update) = receiver.recv().await {
                apply(&app, update);
            }
        });
        Self { updates }
    }

    /// Queues an update; never blocks (the sink must not).
    pub(crate) fn show(&self, update: PopupUpdate) {
        if self.updates.send(update).is_err() {
            tracing::debug!("the popup queue is gone (the app quits)");
        }
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, update: PopupUpdate) {
    if let Some(name) = update.sound.as_deref() {
        sound::play(app, name);
    }
    let existing = app.get_webview_window(POPUP_WINDOW);
    if update.popup.is_empty() {
        if let Some(window) = existing
            && let Err(err) = window.destroy()
        {
            tracing::warn!(%err, "could not close the new-mail popup");
        }
        return;
    }
    if existing.is_some() {
        if let Err(err) = NewMailPopupChanged(update.popup).emit_to(app, POPUP_WINDOW) {
            tracing::warn!(%err, "could not update the new-mail popup");
        }
        return;
    }
    // Without the main window the app is closing: "Read" would have nowhere to go.
    if app.get_webview_window(MAIN_WINDOW).is_none() {
        return;
    }
    if let Err(err) = open(app, &update.popup) {
        tracing::error!(%err, "could not open the new-mail popup");
    }
}

/// A first guess of the page's height, so the window opens about the right size.
pub fn estimated_height(cards: usize, more: u32) -> f64 {
    let cards = cards.clamp(1, mail_model::MAX_POPUP_CARDS) as f64;
    let more = if more > 0 { MORE_HEIGHT } else { 0.0 };
    (cards * CARD_HEIGHT + more).clamp(MIN_HEIGHT, MAX_HEIGHT)
}

/// The height the page asked for, within the limits (a broken number is the smallest).
pub fn clamp_height(height: f64) -> f64 {
    if height.is_finite() {
        height.clamp(MIN_HEIGHT, MAX_HEIGHT)
    } else {
        MIN_HEIGHT
    }
}

/// Where a window of `size` (logical) goes: at the bottom right of `work_area` (physical, the
/// screen without menu bar, Dock or taskbar), `margin` (logical) from both edges, never above or
/// left of the work area.
pub fn bottom_right(
    work_area: PhysicalRect<i32, u32>,
    scale: f64,
    size: LogicalSize<f64>,
    margin: f64,
) -> PhysicalPosition<i32> {
    let physical: PhysicalSize<f64> = size.to_physical(scale);
    let margin = margin * scale;
    let right = f64::from(work_area.position.x) + f64::from(work_area.size.width);
    let bottom = f64::from(work_area.position.y) + f64::from(work_area.size.height);
    let x = (right - physical.width - margin).max(f64::from(work_area.position.x));
    let y = (bottom - physical.height - margin).max(f64::from(work_area.position.y));
    PhysicalPosition::new(x.round() as i32, y.round() as i32)
}

/// The monitor the popup belongs on: the one showing the main window, else the primary one.
fn monitor<R: Runtime>(app: &AppHandle<R>) -> Option<tauri::Monitor> {
    app.get_webview_window(MAIN_WINDOW)
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
}

/// Sizes the window to `height` (logical) and puts it at the bottom right.
fn place<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>, height: f64) {
    let size = LogicalSize::new(POPUP_WIDTH, clamp_height(height));
    if let Err(err) = window.set_size(size) {
        tracing::debug!(%err, "popup size not set");
    }
    if let Some(monitor) = monitor(app) {
        let position = bottom_right(
            *monitor.work_area(),
            monitor.scale_factor(),
            size,
            POPUP_MARGIN,
        );
        if let Err(err) = window.set_position(position) {
            tracing::debug!(%err, "popup position not set");
        }
    }
}

/// The window frame's theme for the app's choice (`None` follows the system).
fn frame_theme(preference: ThemePreference) -> Option<Theme> {
    crate::window_theme::window_theme(preference)
}

/// `--ui-surface-raised` of tokens.css, so a dark popup never flashes white while it loads.
fn background(theme: Theme) -> Color {
    match theme {
        Theme::Dark => Color(0x27, 0x25, 0x22, 0xff),
        _ => Color(0xff, 0xff, 0xff, 0xff),
    }
}

fn open<R: Runtime>(app: &AppHandle<R>, popup: &NewMailPopup) -> tauri::Result<WebviewWindow<R>> {
    let height = estimated_height(popup.cards.len(), popup.more);
    let title = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN_WINDOW)
        .map(|w| w.title.clone())
        .unwrap_or_else(|| app.package_info().name.clone());
    let builder = WebviewWindowBuilder::new(
        app,
        POPUP_WINDOW,
        WebviewUrl::App(PathBuf::from(POPUP_PAGE)),
    )
    .title(title)
    .inner_size(POPUP_WIDTH, height)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .shadow(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible_on_all_workspaces(true)
    .focused(false)
    .focusable(false)
    .accept_first_mouse(true)
    .theme(frame_theme(popup.appearance.theme))
    // macOS: shown by `fit` once the page has measured itself. Windows: shown now, since
    // only the creation shows a window without activating it.
    .visible(cfg!(windows));
    let window = windows::guard(builder, app).build()?;
    windows::finish(app, &window)?;
    if let Ok(theme) = window.theme() {
        let _ = window.set_background_color(Some(background(theme)));
    }
    place(app, &window, height);
    #[cfg(target_os = "macos")]
    macos::style(&window);
    let closing = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            forget_cards(&closing);
        }
    });
    tracing::info!(
        cards = popup.cards.len(),
        more = popup.more,
        "new-mail popup opened"
    );
    Ok(window)
}

/// The window went away while the core still had cards (the app closes it only when there are
/// none): they are dismissed, so the next mail opens a fresh popup.
fn forget_cards<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(core) = app.try_state::<AppCore>() {
            let left = core.new_mail_popup().await;
            if !left.is_empty() {
                core.dismiss_popup(None).await;
            }
        }
    });
}

/// The page measured itself: the window takes that height at the bottom right, and on macOS
/// appears now (ordered in front, never key, the app is not activated).
pub fn fit<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    height: f64,
) -> tauri::Result<()> {
    place(app, window, height);
    if !window.is_visible()? {
        window.show()?;
    }
    #[cfg(target_os = "macos")]
    macos::invalidate_shadow(window);
    Ok(())
}

/// Stage 18: when the main window goes (the app quits), so does the popup.
pub(crate) fn follow_main_window<R: Runtime>(app: &AppHandle<R>, main: &WebviewWindow<R>) {
    let app = app.clone();
    main.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed)
            && let Some(window) = app.get_webview_window(POPUP_WINDOW)
        {
            let _ = window.destroy();
        }
    });
}

/// macOS: what Tauri does not offer, with safe AppKit calls on the main thread.
#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSColor, NSWindow, NSWindowCollectionBehavior};
    use tauri::{Runtime, WebviewWindow};

    use super::CORNER_RADIUS;

    /// The `NSWindow` of `window`, found among the app's windows by its address (never
    /// dereferenced from the raw pointer). On the main thread only.
    fn find(target: usize) -> Option<Retained<NSWindow>> {
        let mtm = MainThreadMarker::new()?;
        NSApplication::sharedApplication(mtm)
            .windows()
            .to_vec()
            .into_iter()
            .find(|w| Retained::as_ptr(w) as usize == target)
    }

    fn on_main<R: Runtime>(window: &WebviewWindow<R>, f: impl FnOnce(&NSWindow) + Send + 'static) {
        let Ok(pointer) = window.ns_window() else {
            return;
        };
        let target = pointer as usize;
        let result = window.run_on_main_thread(move || {
            if let Some(ns_window) = find(target) {
                f(&ns_window);
            }
        });
        if let Err(err) = result {
            tracing::debug!(%err, "popup window not styled");
        }
    }

    /// Rounded corners (the content view's layer, the window itself transparent around it),
    /// a shadow, every Space and full-screen apps, not in the Window menu or the Dock's list,
    /// not in the window cycle, never hidden when the app is not active.
    pub(super) fn style<R: Runtime>(window: &WebviewWindow<R>) {
        on_main(window, |ns_window| {
            ns_window.setExcludedFromWindowsMenu(true);
            ns_window.setHidesOnDeactivate(false);
            ns_window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Transient
                    | NSWindowCollectionBehavior::IgnoresCycle
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            ns_window.setOpaque(false);
            ns_window.setBackgroundColor(Some(&NSColor::clearColor()));
            if let Some(view) = ns_window.contentView() {
                view.setWantsLayer(true);
                if let Some(layer) = view.layer() {
                    layer.setCornerRadius(CORNER_RADIUS);
                    layer.setMasksToBounds(true);
                }
            }
            ns_window.setHasShadow(true);
        });
    }

    /// The shadow follows the rounded content once it is drawn.
    pub(super) fn invalidate_shadow<R: Runtime>(window: &WebviewWindow<R>) {
        on_main(window, |ns_window| ns_window.invalidateShadow());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x: i32, y: i32, width: u32, height: u32) -> PhysicalRect<i32, u32> {
        PhysicalRect {
            position: PhysicalPosition::new(x, y),
            size: PhysicalSize::new(width, height),
        }
    }

    #[test]
    fn the_popup_sits_at_the_bottom_right_of_the_work_area() {
        let size = LogicalSize::new(360.0, 200.0);
        // A Retina screen of 1512 × 982 points, menu bar 33 and Dock 70 points: the work area
        // starts below the menu bar and ends above the Dock.
        let retina = area(0, 66, 3024, 1758);
        assert_eq!(
            bottom_right(retina, 2.0, size, 16.0),
            PhysicalPosition::new(3024 - 720 - 32, 66 + 1758 - 400 - 32)
        );
        // Windows at 150 %, taskbar 48 px at the bottom: above the taskbar.
        let windows = area(0, 0, 2560, 1392);
        assert_eq!(
            bottom_right(windows, 1.5, size, 16.0),
            PhysicalPosition::new(2560 - 540 - 24, 1392 - 300 - 24)
        );
        // A second monitor left of the primary one, Dock on the left: its own coordinates.
        let left = area(-1920, 0, 1920, 1080);
        assert_eq!(
            bottom_right(left, 1.0, size, 16.0),
            PhysicalPosition::new(-360 - 16, 1080 - 200 - 16)
        );
        // A work area smaller than the popup: it stays inside at the top left.
        assert_eq!(
            bottom_right(area(100, 50, 200, 100), 1.0, size, 16.0),
            PhysicalPosition::new(100, 50)
        );
    }

    #[test]
    fn heights_are_estimated_and_clamped() {
        assert_eq!(estimated_height(1, 0), CARD_HEIGHT);
        assert_eq!(estimated_height(3, 2), 3.0 * CARD_HEIGHT + MORE_HEIGHT);
        assert_eq!(
            estimated_height(9, 0),
            3.0 * CARD_HEIGHT,
            "at most three cards"
        );
        assert_eq!(estimated_height(0, 0), CARD_HEIGHT);
        assert_eq!(clamp_height(10.0), MIN_HEIGHT);
        assert_eq!(clamp_height(10_000.0), MAX_HEIGHT);
        assert_eq!(clamp_height(f64::NAN), MIN_HEIGHT);
        assert_eq!(clamp_height(300.0), 300.0);
    }

    #[test]
    fn the_popup_may_only_call_its_own_commands() {
        for command in POPUP_COMMANDS {
            assert!(may_call(POPUP_WINDOW, command), "{command}");
        }
        for command in [
            "list_accounts",
            "apply_action",
            "get_message",
            "set_notification_settings",
            "open_message_window",
            "",
        ] {
            assert!(!may_call(POPUP_WINDOW, command), "{command}");
            assert!(may_call(MAIN_WINDOW, command), "{command}");
        }
        assert!(may_call("message-1-2", "list_accounts"));
        assert!(may_call("composer-x", "get_new_mail_popup"));
    }
}
