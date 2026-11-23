//! Stage 8: the UI's own settings the core keeps (decision 0025): the user's shortcut changes
//! per platform, the text size, the recently used palette commands and the layout per window.
//!
//! Pure data and validation. Where they are stored, and which of them the settings sync carries
//! (shortcuts and text size; recent commands and layouts stay on the device), is `mail-engine`'s
//! business.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Smallest text size in percent of the system's.
pub const MIN_TEXT_SIZE: u8 = 80;
/// Largest text size in percent of the system's.
pub const MAX_TEXT_SIZE: u8 = 150;
/// The text size when nothing is stored.
pub const DEFAULT_TEXT_SIZE: u8 = 100;
/// Most recently used palette commands kept.
pub const MAX_RECENT_COMMANDS: usize = 5;
/// Longest key name of a shortcut (`event.key`: "e", "#", "ArrowDown", "F12").
pub const MAX_SHORTCUT_KEY_CHARS: usize = 20;
/// Most shortcuts one command can have on one platform.
pub const MAX_SHORTCUTS_PER_COMMAND: usize = 4;
/// Most changed commands per platform.
pub const MAX_SHORTCUT_OVERRIDES: usize = 100;
/// Longest command id (`archive`, `toggleRead`, `shortcutHelp`).
pub const MAX_COMMAND_ID_CHARS: usize = 64;
/// Longest palette entry key (`action:archive`, `folder:<account uuid>:<mailbox id>`).
pub const MAX_RECENT_COMMAND_CHARS: usize = 200;

/// One key combination. `mod` is Cmd on macOS and Ctrl on Windows; Option/Alt combinations do
/// not exist (`docs/ARCHITECTURE.md`, Tastaturkürzel).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Shortcut {
    /// The character the key produces (`event.key`), 1 to 20 characters.
    pub key: String,
    pub shift: bool,
    #[serde(rename = "mod")]
    pub modifier: bool,
}

/// The complete list of shortcuts the user gave one command (an empty list: the command has no
/// shortcut). Commands without an entry keep the defaults of the action directory, which only
/// the UI knows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutOverride {
    /// Command id of the action directory: 1 to 64 ASCII letters, digits, `.`, `_`, `-`, `:`.
    pub command: String,
    pub shortcuts: Vec<Shortcut>,
}

/// The user's shortcut changes for both platforms: a synced setting carries both, and each
/// device uses the list of its own system.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlatformShortcuts {
    pub macos: Vec<ShortcutOverride>,
    pub windows: Vec<ShortcutOverride>,
}

/// Which platform's shortcut list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutPlatform {
    Macos,
    Windows,
}

impl ShortcutPlatform {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

/// Stage 13 (decision 0028): whether the AI panel of the main window is unfolded or folded in.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum AiPanelState {
    #[default]
    Open,
    Folded,
}

/// Stage 16 (decision 0036): the accent colour family of the UI, in light and dark. Every one
/// passes WCAG AA (`app/tests/contrast.test.ts`). The default is "orange", the logo's colour
/// (decision 0038). A value this version does not know (from a newer device) reads as the
/// default.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum AccentTheme {
    Teal,
    Blue,
    Violet,
    Rose,
    #[default]
    Orange,
    Green,
}

/// The UI settings of `getUiPreferences` / `setUiPreferences`. Shortcuts, text size and (stage
/// 16) the accent theme are synchronised between devices; the recent commands and the AI
/// panel's state stay on this device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UiPreferences {
    pub shortcuts: PlatformShortcuts,
    /// Percent of the system's text size, 80 to 150.
    pub text_size: u8,
    /// Keys of the palette entries used last, newest first, at most 5, no duplicates.
    pub recent_commands: Vec<String>,
    /// The AI panel unfolded (default) or folded in; device only (decision 0028).
    pub ai_panel: AiPanelState,
    /// Stage 16 (decision 0036): the accent colours; synced like the text size. Only the six
    /// themes are accepted (anything else is refused when the value is read from the UI).
    pub accent_theme: AccentTheme,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            shortcuts: PlatformShortcuts::default(),
            text_size: DEFAULT_TEXT_SIZE,
            recent_commands: Vec::new(),
            ai_panel: AiPanelState::Open,
            accent_theme: AccentTheme::Orange,
        }
    }
}

/// Why UI preferences were refused. Crosses the bridge as `invalid_argument` with this text as
/// the detail.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UiPreferencesError {
    #[error("text size {0} is outside 80 to 150 percent")]
    TextSize(u8),
    #[error("{0} recent commands, at most 5")]
    TooManyRecentCommands(usize),
    #[error("recent command {index}: {reason}")]
    InvalidRecentCommand { index: usize, reason: &'static str },
    #[error("recent command {index} appears twice")]
    DuplicateRecentCommand { index: usize },
    #[error("{platform}: {count} changed commands, at most 100")]
    TooManyOverrides {
        platform: &'static str,
        count: usize,
    },
    #[error("{platform}: command {index}: {reason}")]
    InvalidCommand {
        platform: &'static str,
        index: usize,
        reason: &'static str,
    },
    #[error("{platform}: command {command:?} appears twice")]
    DuplicateCommand {
        platform: &'static str,
        command: String,
    },
    #[error("{platform}: command {command:?} has {count} shortcuts, at most 4")]
    TooManyShortcuts {
        platform: &'static str,
        command: String,
        count: usize,
    },
    #[error("{platform}: command {command:?}, shortcut {index}: {reason}")]
    InvalidShortcut {
        platform: &'static str,
        command: String,
        index: usize,
        reason: &'static str,
    },
    #[error("{platform}: command {command:?} has the same shortcut twice")]
    DuplicateShortcut {
        platform: &'static str,
        command: String,
    },
    /// The stored list would not fit into one settings sync record.
    #[error("{platform}: the shortcut changes take {bytes} bytes, at most {max}")]
    TooLarge {
        platform: &'static str,
        bytes: usize,
        max: usize,
    },
}

impl UiPreferences {
    /// Checks every limit; the values come from the UI or from another device.
    pub fn validate(&self) -> Result<(), UiPreferencesError> {
        if !(MIN_TEXT_SIZE..=MAX_TEXT_SIZE).contains(&self.text_size) {
            return Err(UiPreferencesError::TextSize(self.text_size));
        }
        validate_recent_commands(&self.recent_commands)?;
        validate_overrides(ShortcutPlatform::Macos, &self.shortcuts.macos)?;
        validate_overrides(ShortcutPlatform::Windows, &self.shortcuts.windows)
    }
}

/// The recent palette commands: at most 5, each 1 to 200 characters without control
/// characters, no duplicates.
pub fn validate_recent_commands(commands: &[String]) -> Result<(), UiPreferencesError> {
    if commands.len() > MAX_RECENT_COMMANDS {
        return Err(UiPreferencesError::TooManyRecentCommands(commands.len()));
    }
    let mut seen = BTreeSet::new();
    for (index, command) in commands.iter().enumerate() {
        let invalid = |reason| UiPreferencesError::InvalidRecentCommand { index, reason };
        if command.is_empty() {
            return Err(invalid("empty"));
        }
        if command.chars().count() > MAX_RECENT_COMMAND_CHARS {
            return Err(invalid("longer than 200 characters"));
        }
        if command.chars().any(char::is_control) {
            return Err(invalid("contains a control character"));
        }
        if !seen.insert(command.as_str()) {
            return Err(UiPreferencesError::DuplicateRecentCommand { index });
        }
    }
    Ok(())
}

/// One platform's shortcut changes (see the limits above).
pub fn validate_overrides(
    platform: ShortcutPlatform,
    overrides: &[ShortcutOverride],
) -> Result<(), UiPreferencesError> {
    let name = platform.name();
    if overrides.len() > MAX_SHORTCUT_OVERRIDES {
        return Err(UiPreferencesError::TooManyOverrides {
            platform: name,
            count: overrides.len(),
        });
    }
    let mut commands = BTreeSet::new();
    for (index, entry) in overrides.iter().enumerate() {
        if let Some(reason) = command_problem(&entry.command) {
            return Err(UiPreferencesError::InvalidCommand {
                platform: name,
                index,
                reason,
            });
        }
        if !commands.insert(entry.command.as_str()) {
            return Err(UiPreferencesError::DuplicateCommand {
                platform: name,
                command: entry.command.clone(),
            });
        }
        if entry.shortcuts.len() > MAX_SHORTCUTS_PER_COMMAND {
            return Err(UiPreferencesError::TooManyShortcuts {
                platform: name,
                command: entry.command.clone(),
                count: entry.shortcuts.len(),
            });
        }
        let mut shortcuts = BTreeSet::new();
        for (index, shortcut) in entry.shortcuts.iter().enumerate() {
            if let Some(reason) = key_problem(&shortcut.key) {
                return Err(UiPreferencesError::InvalidShortcut {
                    platform: name,
                    command: entry.command.clone(),
                    index,
                    reason,
                });
            }
            if !shortcuts.insert((&shortcut.key, shortcut.shift, shortcut.modifier)) {
                return Err(UiPreferencesError::DuplicateShortcut {
                    platform: name,
                    command: entry.command.clone(),
                });
            }
        }
    }
    Ok(())
}

fn command_problem(command: &str) -> Option<&'static str> {
    if command.is_empty() {
        return Some("empty");
    }
    if command.len() > MAX_COMMAND_ID_CHARS {
        return Some("longer than 64 characters");
    }
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':');
    if !command.chars().all(allowed) {
        return Some("only ASCII letters, digits, '.', '_', '-' and ':'");
    }
    None
}

fn key_problem(key: &str) -> Option<&'static str> {
    if key.is_empty() {
        return Some("empty key");
    }
    if key.chars().count() > MAX_SHORTCUT_KEY_CHARS {
        return Some("key longer than 20 characters");
    }
    if key.chars().any(char::is_control) {
        return Some("key contains a control character");
    }
    None
}

/// Where a window shows the opened message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ReadingPanePosition {
    /// Right of the list (the default).
    #[default]
    Right,
    /// Below the list.
    Bottom,
    /// In place of the list.
    Replace,
}

/// Row height of the list: comfortable (default) or compact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Density {
    #[default]
    Comfortable,
    Compact,
}

/// The layout of one window, stored per window on this device (not synchronised: the screens
/// differ). `getWindowLayout` / `setWindowLayout` take the window from the calling window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WindowLayout {
    pub reading_pane: ReadingPanePosition,
    pub density: Density,
    /// The icon rail unfolded into the labelled sidebar.
    pub sidebar_expanded: bool,
}

/// Longest layout slot name (see [`validate_layout_slot`]).
pub const MAX_LAYOUT_SLOT_CHARS: usize = 32;

/// A layout slot names the kind of window whose layout is stored (`main`, `message`): 1 to 32
/// lowercase ASCII letters, digits or `-`. The shell maps window labels to slots, so the
/// settings do not grow with every message window.
pub fn validate_layout_slot(slot: &str) -> Result<(), String> {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    if slot.is_empty() || slot.len() > MAX_LAYOUT_SLOT_CHARS || !slot.chars().all(allowed) {
        return Err(format!(
            "window layout slot {slot:?}: 1 to 32 lowercase letters, digits or '-'"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn shortcut(key: &str, shift: bool, modifier: bool) -> Shortcut {
        Shortcut {
            key: key.into(),
            shift,
            modifier,
        }
    }

    fn entry(command: &str, shortcuts: Vec<Shortcut>) -> ShortcutOverride {
        ShortcutOverride {
            command: command.into(),
            shortcuts,
        }
    }

    fn with_macos(overrides: Vec<ShortcutOverride>) -> UiPreferences {
        UiPreferences {
            shortcuts: PlatformShortcuts {
                macos: overrides,
                windows: Vec::new(),
            },
            ..UiPreferences::default()
        }
    }

    #[test]
    fn wire_format_matches_the_contract() {
        let prefs = UiPreferences {
            shortcuts: PlatformShortcuts {
                macos: vec![entry("archive", vec![shortcut("y", false, true)])],
                windows: vec![entry("delete", Vec::new())],
            },
            text_size: 115,
            recent_commands: vec!["action:archive".into()],
            ai_panel: AiPanelState::Folded,
            accent_theme: AccentTheme::Rose,
        };
        let wire = json!({
            "shortcuts": {
                "macos": [{ "command": "archive", "shortcuts": [{ "key": "y", "shift": false, "mod": true }] }],
                "windows": [{ "command": "delete", "shortcuts": [] }]
            },
            "textSize": 115,
            "recentCommands": ["action:archive"],
            "aiPanel": "folded",
            "accentTheme": "rose"
        });
        assert_eq!(serde_json::to_value(&prefs).expect("json"), wire);
        assert_eq!(
            serde_json::from_value::<UiPreferences>(wire).expect("parse"),
            prefs
        );
        let layout = WindowLayout {
            reading_pane: ReadingPanePosition::Replace,
            density: Density::Compact,
            sidebar_expanded: true,
        };
        assert_eq!(
            serde_json::to_value(layout).expect("json"),
            json!({ "readingPane": "replace", "density": "compact", "sidebarExpanded": true })
        );
        assert_eq!(
            serde_json::to_value(WindowLayout::default()).expect("json"),
            json!({ "readingPane": "right", "density": "comfortable", "sidebarExpanded": false })
        );
    }

    #[test]
    fn defaults_are_valid() {
        assert_eq!(UiPreferences::default().text_size, 100);
        assert_eq!(UiPreferences::default().ai_panel, AiPanelState::Open);
        assert!(UiPreferences::default().validate().is_ok());
    }

    #[test]
    fn the_ai_panel_state_is_one_of_two_values() {
        let wire = |panel: serde_json::Value| {
            json!({
                "shortcuts": { "macos": [], "windows": [] },
                "textSize": 100,
                "recentCommands": [],
                "aiPanel": panel,
                "accentTheme": "orange"
            })
        };
        for (text, state) in [
            ("open", AiPanelState::Open),
            ("folded", AiPanelState::Folded),
        ] {
            let prefs: UiPreferences = serde_json::from_value(wire(json!(text))).expect("parse");
            assert_eq!(prefs.ai_panel, state);
        }
        for refused in [json!("sideways"), json!(true), json!(null), json!("Open")] {
            assert!(
                serde_json::from_value::<UiPreferences>(wire(refused.clone())).is_err(),
                "{refused}"
            );
        }
        let mut missing = wire(json!("open"));
        missing.as_object_mut().expect("object").remove("aiPanel");
        assert!(serde_json::from_value::<UiPreferences>(missing).is_err());
    }

    #[test]
    fn text_size_stays_between_80_and_150() {
        for (size, ok) in [
            (79, false),
            (80, true),
            (150, true),
            (151, false),
            (0, false),
        ] {
            let prefs = UiPreferences {
                text_size: size,
                ..UiPreferences::default()
            };
            assert_eq!(prefs.validate().is_ok(), ok, "size {size}");
        }
    }

    #[test]
    fn recent_commands_are_bounded_and_unique() {
        let five: Vec<String> = (0..5).map(|i| format!("action:{i}")).collect();
        assert!(validate_recent_commands(&five).is_ok());
        let six: Vec<String> = (0..6).map(|i| format!("action:{i}")).collect();
        assert_eq!(
            validate_recent_commands(&six),
            Err(UiPreferencesError::TooManyRecentCommands(6))
        );
        let twice = vec!["view:inbox".to_owned(), "view:inbox".to_owned()];
        assert_eq!(
            validate_recent_commands(&twice),
            Err(UiPreferencesError::DuplicateRecentCommand { index: 1 })
        );
        for bad in [String::new(), "a".repeat(201), "line\nbreak".into()] {
            assert!(matches!(
                validate_recent_commands(&[bad]),
                Err(UiPreferencesError::InvalidRecentCommand { index: 0, .. })
            ));
        }
        assert!(validate_recent_commands(&["ü".repeat(200)]).is_ok());
    }

    #[test]
    fn shortcut_overrides_are_checked() {
        let ok = with_macos(vec![
            entry(
                "archive",
                vec![shortcut("e", false, false), shortcut("y", false, true)],
            ),
            entry("delete", Vec::new()),
            entry("palette", vec![shortcut("ArrowDown", true, true)]),
        ]);
        assert!(ok.validate().is_ok());

        let duplicate = with_macos(vec![
            entry("archive", Vec::new()),
            entry("archive", Vec::new()),
        ]);
        assert_eq!(
            duplicate.validate(),
            Err(UiPreferencesError::DuplicateCommand {
                platform: "macos",
                command: "archive".into()
            })
        );

        let same_twice = with_macos(vec![entry(
            "archive",
            vec![shortcut("e", false, false), shortcut("e", false, false)],
        )]);
        assert!(matches!(
            same_twice.validate(),
            Err(UiPreferencesError::DuplicateShortcut { .. })
        ));
        // Same key, other modifiers: different shortcuts.
        let modifiers = with_macos(vec![entry(
            "archive",
            vec![shortcut("e", false, false), shortcut("e", true, false)],
        )]);
        assert!(modifiers.validate().is_ok());

        let many = with_macos(vec![entry(
            "archive",
            (0..5)
                .map(|i| shortcut(&i.to_string(), false, false))
                .collect(),
        )]);
        assert!(matches!(
            many.validate(),
            Err(UiPreferencesError::TooManyShortcuts { count: 5, .. })
        ));

        for key in ["", &"x".repeat(21), "\u{7}"] {
            let bad = with_macos(vec![entry("archive", vec![shortcut(key, false, false)])]);
            assert!(
                matches!(
                    bad.validate(),
                    Err(UiPreferencesError::InvalidShortcut { index: 0, .. })
                ),
                "key {key:?}"
            );
        }
        let longest = with_macos(vec![entry(
            "archive",
            vec![shortcut(&"ß".repeat(20), false, false)],
        )]);
        assert!(longest.validate().is_ok());

        for command in ["", "arch ive", "archivé", &"a".repeat(65)] {
            let bad = with_macos(vec![entry(command, Vec::new())]);
            assert!(
                matches!(
                    bad.validate(),
                    Err(UiPreferencesError::InvalidCommand { index: 0, .. })
                ),
                "command {command:?}"
            );
        }

        let too_many = UiPreferences {
            shortcuts: PlatformShortcuts {
                macos: Vec::new(),
                windows: (0..101)
                    .map(|i| entry(&format!("c{i}"), Vec::new()))
                    .collect(),
            },
            ..UiPreferences::default()
        };
        assert_eq!(
            too_many.validate(),
            Err(UiPreferencesError::TooManyOverrides {
                platform: "windows",
                count: 101
            })
        );
    }

    #[test]
    fn layout_slots_are_simple_names() {
        assert!(validate_layout_slot("main").is_ok());
        assert!(validate_layout_slot("message").is_ok());
        for bad in ["", "Main", "a/b", "ui.theme", &"a".repeat(33)] {
            assert!(validate_layout_slot(bad).is_err(), "{bad:?}");
        }
    }
}
