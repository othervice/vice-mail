//! Stage 21 (decisions 0069, 0070): the app in the background. Closing the main window keeps it
//! running with an icon in the menu bar (macOS) or the notification area (Windows), notifications
//! can be paused for a while, and the app can start with the system.
//!
//! All of it is a setting of this device; nothing here travels with the settings sync.

use serde::{Deserialize, Serialize};

/// What closing the main window does, and whether macOS shows the menu-bar icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct BackgroundSettings {
    /// Closing the main window keeps the app running: sync, the new-mail popup and
    /// notifications go on; the window's web view is closed until it is opened again. Off: the
    /// app quits with its last window, as before stage 21.
    pub keep_running: bool,
    /// macOS: the icon in the menu bar. Windows always shows its icon in the notification area
    /// while the app runs (it is the way back to a closed window).
    pub menu_bar_icon: bool,
}

impl Default for BackgroundSettings {
    fn default() -> Self {
        Self {
            keep_running: true,
            menu_bar_icon: true,
        }
    }
}

/// How long notifications pause (menu of the tray icon, Settings → Notifications).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PauseNotifications {
    /// For one hour from now.
    OneHour,
    /// Until 08:00 of the next day, local time.
    UntilTomorrow,
    /// Not any more.
    Resume,
}

/// Local time of day at which "until tomorrow" ends.
pub const PAUSE_UNTIL_TOMORROW_HOUR: i8 = 8;

/// Whether notifications are paused. While paused, new mail shows neither the popup nor a
/// notification and plays no sound, as in quiet hours; nothing is shown later. The counter in
/// the Dock, on the taskbar and on the tray icon goes on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPause {
    /// Milliseconds since the epoch; `None` when not paused (or the pause is over).
    #[specta(type = Option<specta_typescript::Number>)]
    pub until: Option<i64>,
    /// Local wall-clock time of `until` (hour, minute) and whether it is on a later day than now,
    /// for the tray menu, which the shell builds without the UI.
    pub local_time: Option<LocalTime>,
}

/// A local time of day, for the shell's tray menu ("Paused until 14:30").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LocalTime {
    pub hour: u8,
    pub minute: u8,
    /// The time is on a later day than now ("tomorrow 08:00").
    pub later_day: bool,
}

/// "Start with the system": whether the app is registered to start at login, and whether this
/// build can register itself (development and test builds use a registration of their own or
/// none; see decision 0070).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Autostart {
    pub enabled: bool,
    pub available: bool,
}

/// The texts of the tray icon's menu and tooltip in the UI's language. The UI sends them at start
/// and after a language change (`setDesktopTexts`); the core keeps the last ones on this device,
/// so an app started in the background (without the UI) has them too. Until then the English
/// defaults apply. `{product}` is replaced by the product name, `{count}` by a number, `{time}`
/// by a time of day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopTexts {
    pub open: String,
    pub new_message: String,
    pub pause: String,
    pub pause_hour: String,
    pub pause_tomorrow: String,
    pub resume: String,
    /// "Paused until {time}" (a menu entry that cannot be chosen).
    pub paused_until: String,
    /// "Paused until tomorrow, {time}".
    pub paused_until_tomorrow: String,
    pub quit: String,
    /// Tooltip without unread mail: "{product}".
    pub tooltip: String,
    /// Tooltip with unread mail: "{product}: {count} unread".
    pub tooltip_unread: String,
    /// The UI's clock: 12-hour ("2:30 PM") or 24-hour ("14:30").
    pub hour12: bool,
}

impl Default for DesktopTexts {
    fn default() -> Self {
        Self {
            open: "Open {product}".into(),
            new_message: "New Message".into(),
            pause: "Pause Notifications".into(),
            pause_hour: "For 1 Hour".into(),
            pause_tomorrow: "Until Tomorrow".into(),
            resume: "Resume Notifications".into(),
            paused_until: "Paused until {time}".into(),
            paused_until_tomorrow: "Paused until tomorrow, {time}".into(),
            quit: "Quit {product}".into(),
            tooltip: "{product}".into(),
            tooltip_unread: "{product}: {count} unread".into(),
            hour12: false,
        }
    }
}

impl LocalTime {
    /// "14:30" or "2:30 PM".
    pub fn format(self, hour12: bool) -> String {
        if hour12 {
            let suffix = if self.hour < 12 { "AM" } else { "PM" };
            let hour = match self.hour % 12 {
                0 => 12,
                h => h,
            };
            format!("{hour}:{:02} {suffix}", self.minute)
        } else {
            format!("{:02}:{:02}", self.hour, self.minute)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_running_with_an_icon() {
        let settings = BackgroundSettings::default();
        assert!(settings.keep_running);
        assert!(settings.menu_bar_icon);
        // Stored settings without a field read as the default.
        let partial: BackgroundSettings =
            serde_json::from_str(r#"{"keepRunning":false}"#).expect("parse");
        assert!(!partial.keep_running);
        assert!(partial.menu_bar_icon);
    }

    #[test]
    fn times_read_as_the_ui_writes_them() {
        let t = |hour, minute| LocalTime {
            hour,
            minute,
            later_day: false,
        };
        assert_eq!(t(14, 30).format(false), "14:30");
        assert_eq!(t(8, 0).format(false), "08:00");
        assert_eq!(t(14, 30).format(true), "2:30 PM");
        assert_eq!(t(0, 5).format(true), "12:05 AM");
        assert_eq!(t(12, 0).format(true), "12:00 PM");
    }

    #[test]
    fn texts_fall_back_to_english_field_by_field() {
        let texts: DesktopTexts =
            serde_json::from_str(r#"{"quit":"{product} beenden"}"#).expect("parse");
        assert_eq!(texts.quit, "{product} beenden");
        assert_eq!(texts.open, "Open {product}");
    }
}
