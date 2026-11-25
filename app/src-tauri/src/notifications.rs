//! Stage 9: native notifications and the Dock or taskbar counter (decision 0020).
//!
//! - **Notifications** go through `tauri-plugin-notification`, used from Rust only: the UI gets
//!   no notification permission (`capabilities/default.json` is unchanged). The core decides
//!   what to announce ([`mail_engine::comfort`]); this sink only posts. A click (`tap`) is
//!   reported by the plugin where the system reports it (macOS; Windows while the toast is on
//!   screen): the app comes to the front and the core asks the UI to open the message.
//! - **The counter** follows the core's events (debounced): `set_badge_count` on macOS, an
//!   overlay icon with the number on the taskbar button on Windows. No `unsafe` here.
//! - **Sound** (stage 16): the core chooses it per notification (or none); [`plugin_sound`]
//!   turns the name into what the plugin hands the system. Without a name a notification is
//!   silent on both systems (macOS: no `soundName`; Windows: `<audio silent="true"/>`).
//! - **The new-mail popup** (stage 18, decision 0051): the core's popup updates go to the
//!   popup's window ([`crate::popup`]), which plays the sound itself.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicI32, Ordering};
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

/// Brings the main window to the front (a click on a notification, "Read" in the popup).
pub(crate) fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
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

#[cfg(not(windows))]
fn set_badge<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    let value = (count > 0).then_some(i64::from(count));
    if let Err(err) = window.set_badge_count(value) {
        tracing::debug!(%err, "badge not set");
    }
}

#[cfg(windows)]
fn set_badge<R: Runtime>(app: &AppHandle<R>, count: u32) {
    let Some(window) = app.get_webview_window(crate::MAIN_WINDOW) else {
        return;
    };
    let icon = (count > 0).then(|| overlay::badge(count));
    let result = match &icon {
        Some(rgba) => window.set_overlay_icon(Some(tauri::image::Image::new(
            rgba,
            overlay::SIZE,
            overlay::SIZE,
        ))),
        None => window.set_overlay_icon(None),
    };
    if let Err(err) = result {
        tracing::debug!(%err, "taskbar overlay not set");
    }
}

/// The taskbar overlay of Windows: a red disc with the number (capped at 99).
/// Drawn here from a 3×5 digit font, so no image files or font libraries are needed.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) mod overlay {
    /// Pixels per side (Windows scales the overlay to 16×16 logical pixels).
    pub const SIZE: u32 = 32;

    const DIGITS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b010, 0b010, 0b010],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];

    /// RGBA pixels of the overlay for `count` (> 0).
    pub fn badge(count: u32) -> Vec<u8> {
        let size = SIZE as usize;
        let mut rgba = vec![0u8; size * size * 4];
        let center = (size as f32 - 1.0) / 2.0;
        let radius = size as f32 / 2.0;
        for y in 0..size {
            for x in 0..size {
                let (dx, dy) = (x as f32 - center, y as f32 - center);
                if dx * dx + dy * dy <= radius * radius {
                    let i = (y * size + x) * 4;
                    if let Some(px) = rgba.get_mut(i..i + 4) {
                        px.copy_from_slice(&[0xD9, 0x30, 0x25, 0xFF]);
                    }
                }
            }
        }
        let shown = count.min(99);
        let digits: Vec<usize> = shown
            .to_string()
            .bytes()
            .map(|b| usize::from(b - b'0'))
            .collect();
        // Each digit 3×5 cells of `scale` pixels, one cell apart.
        let scale = if digits.len() == 1 { 4 } else { 3 };
        let width = digits.len() * 3 * scale + (digits.len() - 1) * scale;
        let left = (size - width) / 2;
        let top = (size - 5 * scale) / 2;
        for (n, digit) in digits.iter().enumerate() {
            let x0 = left + n * 4 * scale;
            for (row, bits) in DIGITS[*digit].iter().enumerate() {
                for col in 0..3 {
                    if bits & (0b100 >> col) == 0 {
                        continue;
                    }
                    for py in 0..scale {
                        for px in 0..scale {
                            let x = x0 + col * scale + px;
                            let y = top + row * scale + py;
                            let i = (y * size + x) * 4;
                            if let Some(p) = rgba.get_mut(i..i + 4) {
                                p.copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
                            }
                        }
                    }
                }
            }
        }
        rgba
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn badges_have_the_size_and_white_digits() {
            for count in [1, 7, 42, 99, 1_000] {
                let rgba = badge(count);
                assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);
                let white = rgba.chunks(4).filter(|p| *p == [0xFF; 4]).count();
                assert!(white > 20, "{count}: digits drawn");
            }
            assert_eq!(badge(1_000), badge(99), "capped at 99");
        }
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
