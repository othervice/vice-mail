//! Stage 9: native notifications and the Dock or taskbar counter (decision 0020).
//!
//! - **Notifications** go through `tauri-plugin-notification`, used from Rust only: the UI gets
//!   no notification permission (`capabilities/default.json` is unchanged). The core decides
//!   what to announce ([`mail_engine::comfort`]); this sink only posts. A click (`tap`) is
//!   reported by the plugin where the system reports it (macOS; Windows while the toast is on
//!   screen): the app comes to the front and the core asks the UI to open the message.
//! - **The counter** follows the core's events (debounced): the Dock tile's own badge on macOS
//!   (`NSDockTile.badgeLabel`, set on the app, so it stays current while no window is open), an
//!   overlay icon with the number on the taskbar button on Windows, drawn for the window's
//!   display scale (stage 21, [`crate::badge`]) and drawn again when the scale changes, and the
//!   tray icon ([`crate::tray`]). No `unsafe` here.
//! - **Sound** (stage 16): the core chooses it per notification (or none); [`plugin_sound`]
//!   turns the name into what the plugin hands the system. Without a name a notification is
//!   silent on both systems (macOS: no `soundName`; Windows: `<audio silent="true"/>`).
//! - **The new-mail popup** (stage 18, decision 0051): the core's popup updates go to the
//!   popup's window ([`crate::popup`]), which plays the sound itself.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_engine::AppCore;
use mail_engine::comfort::{Notification, NotificationSink, PopupUpdate};
use mail_model::{AccountId, CoreEvent, MailboxId, MessageId};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::broadcast::{self, error::RecvError};

/// Click targets remembered (older notifications open nothing when clicked).
const MAX_TARGETS: usize = 200;
/// The counter waits this long after a change for more changes.
const BADGE_DEBOUNCE: Duration = Duration::from_millis(400);

type Target = (AccountId, Option<MailboxId>, MessageId);

/// Posts the core's notifications with the plugin.
pub(crate) struct NativeNotifications<R: Runtime> {
    app: AppHandle<R>,
    next_id: AtomicI32,
    targets: Arc<Mutex<VecDeque<(i32, Target)>>>,
    /// Stage 18: the window of the new-mail popup.
    popup: crate::popup::PopupWindow,
}

impl<R: Runtime> NativeNotifications<R> {
    /// Also registers the click handler.
    pub(crate) fn install(app: &AppHandle<R>) -> Arc<Self> {
        let sink = Arc::new(Self {
            app: app.clone(),
            next_id: AtomicI32::new(1),
            targets: Arc::new(Mutex::new(VecDeque::new())),
            popup: crate::popup::PopupWindow::install(app),
        });
        let targets = Arc::clone(&sink.targets);
        let handle = app.clone();
        let registered = app.notification().on_action(move |performed| {
            if performed.action_id() != "tap" {
                return;
            }
            let Some(id) = performed.notification().map(|n| n.id()) else {
                return;
            };
            let target = targets
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .find(|(known, _)| *known == id)
                .map(|(_, target)| *target);
            show_main_window(&handle);
            if let (Some((account, mailbox, message)), Some(core)) =
                (target, handle.try_state::<AppCore>())
            {
                core.notification_clicked(account, mailbox, message);
            }
        });
        if let Err(err) = registered {
            tracing::warn!(%err, "notification clicks are not reported");
        }
        sink
    }
}

impl<R: Runtime> NotificationSink for NativeNotifications<R> {
    fn show(&self, notification: Notification) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if let Some(message) = notification.message_id {
            let mut targets = self.targets.lock().unwrap_or_else(PoisonError::into_inner);
            if targets.len() >= MAX_TARGETS {
                targets.pop_front();
            }
            targets.push_back((
                id,
                (notification.account_id, notification.mailbox_id, message),
            ));
        }
        let mut builder = self
            .app
            .notification()
            .builder()
            .id(id)
            .title(notification.title);
        if !notification.body.is_empty() {
            builder = builder.body(notification.body);
        }
        if let Some(sound) = notification.sound.as_deref() {
            builder = builder.sound(plugin_sound(sound));
        }
        if let Err(err) = builder.show() {
            tracing::warn!(%err, kind = ?notification.kind, "notification not shown");
        }
    }

    fn popup(&self, update: PopupUpdate) {
        self.popup.show(update);
    }
}

/// The sound name the plugin passes on (`notify-rust` `sound_name`) for a name of the core's
/// list (`mail_model::MACOS_SOUNDS`, `WINDOWS_SOUNDS`):
/// - macOS (`NSUserNotification.soundName`): an alert sound of `/System/Library/Sounds` by
///   name; `Default` is `NSUserNotificationDefaultSoundName`, the system's sound.
/// - Windows (toast `<audio>`): `Default`, `IM`, `Mail`, `Reminder`, `SMS` as they are
///   (`ms-winsoundevent:Notification.*`); an unknown name would be silent.
/// - Elsewhere (freedesktop `sound-name` hint): the theme's new-mail sound.
fn plugin_sound(name: &str) -> String {
    if cfg!(target_os = "macos") {
        if name == "Default" {
            "NSUserNotificationDefaultSoundName".to_owned()
        } else {
            name.to_owned()
        }
    } else if cfg!(windows) {
        name.to_owned()
    } else {
        "message-new-email".to_owned()
    }
}

/// Brings the main window to the front (a click on a notification, "Read" in the popup), and
/// opens it again if it was closed into the background (stage 21).
pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    crate::lifecycle::show_main_window(app);
}

// ---------------------------------------------------------------------------------------------
// The counter

/// Follows the core's events and keeps the counter current.
pub(crate) fn follow_badge<R: Runtime>(
    app: AppHandle<R>,
    mut events: broadcast::Receiver<CoreEvent>,
) {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<u32> = None;
        loop {
            match events.recv().await {
                Ok(
                    CoreEvent::MessagesChanged { .. }
                    | CoreEvent::MailboxesChanged { .. }
                    | CoreEvent::AccountsChanged
                    | CoreEvent::InboxChanged { .. }
                    | CoreEvent::OperationsChanged { .. },
                ) => {}
                Ok(_) => continue,
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
            // Collect a burst (one sync window sends several events).
            tokio::time::sleep(BADGE_DEBOUNCE).await;
            while let Ok(_) | Err(broadcast::error::TryRecvError::Lagged(_)) = events.try_recv() {}
            let Some(core) = app.try_state::<AppCore>() else {
                continue;
            };
            let count = match core.badge_count().await {
                Ok(count) => count,
                Err(err) => {
                    tracing::debug!(%err, "badge count unavailable");
                    continue;
                }
            };
            if last != Some(count) {
                set_badge(&app, count);
                last = Some(count);
            }
        }
    });
}

/// Sets the counter now (after a settings change).
pub(crate) async fn refresh_badge<R: Runtime>(app: &AppHandle<R>, core: &AppCore) {
    if let Ok(count) = core.badge_count().await {
        set_badge(app, count);
    }
}

/// The last number shown (managed state), for drawing it again: a new main window, a new
/// display scale, the tray menu.
#[derive(Debug, Default)]
pub(crate) struct Counter(AtomicU32);

/// The number the counter shows now (0 before the first count).
pub(crate) fn current_count<R: Runtime>(app: &AppHandle<R>) -> u32 {
    app.try_state::<Counter>()
        .map_or(0, |c| c.0.load(Ordering::Relaxed))
}

fn set_badge<R: Runtime>(app: &AppHandle<R>, count: u32) {
    if let Some(counter) = app.try_state::<Counter>() {
        counter.0.store(count, Ordering::Relaxed);
    }
    show_count(app, count);
    crate::tray::refresh(app);
}

/// Draws the last number again (the main window is new, or its display scale changed).
pub(crate) fn redraw_badge<R: Runtime>(app: &AppHandle<R>) {
    show_count(app, current_count(app));
}

/// macOS: the Dock tile's badge, on the main thread (AppKit), independent of any window.
#[cfg(target_os = "macos")]
fn show_count<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let result = app.run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let label = (count > 0).then(|| objc2_foundation::NSString::from_str(&count.to_string()));
        objc2_app_kit::NSApplication::sharedApplication(mtm)
            .dockTile()
            .setBadgeLabel(label.as_deref());
    });
    if let Err(err) = result {
        tracing::debug!(%err, "badge not set");
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn show_count<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    let value = (count > 0).then_some(i64::from(count));
    if let Err(err) = window.set_badge_count(value) {
        tracing::debug!(%err, "badge not set");
    }
}

/// Windows: the overlay on the main window's taskbar button, drawn at the size Windows shows it
/// for the window's display scale (16 logical pixels). Without a main window there is no taskbar
/// button; the tray icon shows the number.
#[cfg(windows)]
fn show_count<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    let result = if count > 0 {
        let size = crate::badge::overlay_size(window.scale_factor().unwrap_or(1.0));
        let image = crate::badge::overlay(count, size);
        window.set_overlay_icon(Some(tauri::image::Image::new_owned(
            image.pixels,
            image.width,
            image.height,
        )))
    } else {
        window.set_overlay_icon(None)
    };
    if let Err(err) = result {
        tracing::debug!(%err, "taskbar overlay not set");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_map_to_the_plugin_names_of_the_platform() {
        let platform = mail_model::SoundPlatform::current();
        for name in platform.sounds() {
            let mapped = plugin_sound(name);
            assert!(!mapped.is_empty());
            if cfg!(target_os = "macos") && *name != "Default" {
                assert_eq!(&mapped, name, "an alert sound by its own name");
            }
        }
        if cfg!(target_os = "macos") {
            assert_eq!(
                plugin_sound("Default"),
                "NSUserNotificationDefaultSoundName"
            );
            assert_eq!(plugin_sound("Glass"), "Glass");
        } else if cfg!(windows) {
            assert_eq!(plugin_sound("Mail"), "Mail");
        }
    }
}
