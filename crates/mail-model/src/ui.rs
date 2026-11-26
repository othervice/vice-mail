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
/// Stage 17: most entries of [`UiPreferences::column_views`].
pub const MAX_COLUMN_VIEWS: usize = 32;
/// Stage 17: longest view id of the column (`later`, `pinnedAttachments`, `contacts`).
pub const MAX_COLUMN_VIEW_ID_CHARS: usize = 64;
/// Stage 23 (decision 0085): most entries of [`UiPreferences::sidebar_folds`].
pub const MAX_SIDEBAR_FOLDS: usize = 128;
/// Stage 23 (decision 0085): longest key of a part of the sidebar (`more`,
/// `account:<account uuid>`, `folders:<account uuid>`).
pub const MAX_SIDEBAR_FOLD_KEY_CHARS: usize = 64;

/// One key combination. `mod` is Cmd on macOS and Ctrl on Windows; Option/Alt combinations do
/// not exist (`docs/ARCHITECTURE.md`, "Keyboard shortcuts").
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
/// passes WCAG AA (`app/tests/contrast.test.ts`). The default is "blue" (decision 0053; it was
/// the logo's orange under decision 0038, a stored choice stays). A value this version does not
/// know (from a newer device) reads as the default.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "camelCase")]
pub enum AccentTheme {
    Teal,
    #[default]
    Blue,
    Violet,
    Rose,
    Orange,
    Green,
}

/// Stage 17: one view of the column's "Views" section (and of the rail), in the user's order,
/// shown or hidden. Which views exist is the UI's business, like the commands of the shortcuts:
/// the core checks the form of the id only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ColumnView {
    /// View id of the UI: 1 to 64 ASCII letters, digits, `_` or `-`.
    pub id: String,
    pub shown: bool,
}

/// Stage 23 (decision 0085): a part of the sidebar the user opened or closed ("More", a section,
/// an account, an account's "More folders"), by the UI's key for it. Which parts exist and what
/// they show by default is the UI's business; the core checks the form of the key only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SidebarFold {
    /// 1 to 64 ASCII letters, digits, `_`, `-` or `:`.
    pub key: String,
    pub open: bool,
}

/// The UI settings of `getUiPreferences` / `setUiPreferences`. Shortcuts, text size, (stage
/// 16) the accent theme, (decision 0039) the conversation view and (stage 17) the column's views
/// are synchronised between devices; the recent commands, the AI panel's state and (stage 23) the
/// sidebar's opened and closed parts stay on this device.
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
    /// Decision 0039: lists show one row per conversation (default) instead of one per message;
    /// synced like the text size.
    pub conversations: bool,
    /// Stage 17: the views of the column's "Views" section and of the rail, in the user's order,
    /// each shown or hidden; synced like the text size. Empty is the default: every view in the
    /// UI's order. A view the list does not name (one added by a later version) keeps its default
    /// place and visibility. At most 32 entries, no id twice.
    pub column_views: Vec<ColumnView>,
    /// Stage 23 (decision 0085): the parts of the sidebar the user opened or closed, on this
    /// device (like the window's layout). Empty is the default: every part as the UI shows it
    /// first. At most 128 entries, no key twice.
    pub sidebar_folds: Vec<SidebarFold>,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            shortcuts: PlatformShortcuts::default(),
            text_size: DEFAULT_TEXT_SIZE,
            recent_commands: Vec::new(),
            ai_panel: AiPanelState::Open,
            accent_theme: AccentTheme::Blue,
            conversations: true,
            column_views: Vec::new(),
            sidebar_folds: Vec::new(),
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
    #[error("{0} column views, at most 32")]
    TooManyColumnViews(usize),
    #[error("column view {index}: {reason}")]
    InvalidColumnView { index: usize, reason: &'static str },
    #[error("column view {id:?} appears twice")]
    DuplicateColumnView { id: String },
    #[error("{0} parts of the sidebar, at most 128")]
    TooManySidebarFolds(usize),
    #[error("part of the sidebar {index}: {reason}")]
    InvalidSidebarFold { index: usize, reason: &'static str },
    #[error("part of the sidebar {key:?} appears twice")]
    DuplicateSidebarFold { key: String },
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
        validate_column_views(&self.column_views)?;
        validate_sidebar_folds(&self.sidebar_folds)?;
        validate_overrides(ShortcutPlatform::Macos, &self.shortcuts.macos)?;
        validate_overrides(ShortcutPlatform::Windows, &self.shortcuts.windows)
    }
}

/// Stage 17: the column's views: at most 32, each id 1 to 64 ASCII letters, digits, `_` or `-`,
/// no id twice.
pub fn validate_column_views(views: &[ColumnView]) -> Result<(), UiPreferencesError> {
    if views.len() > MAX_COLUMN_VIEWS {
        return Err(UiPreferencesError::TooManyColumnViews(views.len()));
    }
    let mut seen = BTreeSet::new();
    for (index, view) in views.iter().enumerate() {
        let invalid = |reason| UiPreferencesError::InvalidColumnView { index, reason };
        if view.id.is_empty() {
            return Err(invalid("empty id"));
        }
        if view.id.len() > MAX_COLUMN_VIEW_ID_CHARS {
            return Err(invalid("id longer than 64 characters"));
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-');
        if !view.id.chars().all(allowed) {
            return Err(invalid("only ASCII letters, digits, '_' and '-'"));
        }
        if !seen.insert(view.id.as_str()) {
            return Err(UiPreferencesError::DuplicateColumnView {
                id: view.id.clone(),
            });
        }
    }
    Ok(())
}

/// Stage 23 (decision 0085): the sidebar's parts opened or closed: at most 128, each key 1 to 64
/// ASCII letters, digits, `_`, `-` or `:`, no key twice.
pub fn validate_sidebar_folds(folds: &[SidebarFold]) -> Result<(), UiPreferencesError> {
    if folds.len() > MAX_SIDEBAR_FOLDS {
        return Err(UiPreferencesError::TooManySidebarFolds(folds.len()));
    }
    let mut seen = BTreeSet::new();
    for (index, fold) in folds.iter().enumerate() {
        let invalid = |reason| UiPreferencesError::InvalidSidebarFold { index, reason };
        if fold.key.is_empty() {
            return Err(invalid("empty key"));
        }
        if fold.key.len() > MAX_SIDEBAR_FOLD_KEY_CHARS {
            return Err(invalid("key longer than 64 characters"));
        }
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':');
        if !fold.key.chars().all(allowed) {
            return Err(invalid("only ASCII letters, digits, '_', '-' and ':'"));
        }
        if !seen.insert(fold.key.as_str()) {
            return Err(UiPreferencesError::DuplicateSidebarFold {
                key: fold.key.clone(),
            });
        }
    }
    Ok(())
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WindowLayout {
    pub reading_pane: ReadingPanePosition,
    pub density: Density,
    /// The accounts-and-folders column is shown (default) instead of the icon rail alone.
    pub sidebar_expanded: bool,
    /// Width of the accounts-and-folders column in CSS pixels, [`MIN_SIDEBAR_WIDTH`] to
    /// [`MAX_SIDEBAR_WIDTH`] ([`DEFAULT_SIDEBAR_WIDTH`] by default); the column's drag handle
    /// sets it.
    pub sidebar_width: u16,
    /// Stage 18: width of the message list beside the reading pane on the right, in CSS pixels
    /// at 100 % text size, [`MIN_LIST_WIDTH`] to [`MAX_LIST_WIDTH`] ([`DEFAULT_LIST_WIDTH`] by
    /// default); the drag handle between list and pane sets it. A window too narrow for it
    /// draws the list narrower and keeps this value.
    pub list_width: u16,
    /// Stage 18: height of the message list above the reading pane below it, in CSS pixels at
    /// 100 % text size, [`MIN_LIST_HEIGHT`] to [`MAX_LIST_HEIGHT`] ([`DEFAULT_LIST_HEIGHT`] by
    /// default); the same handle sets it when the pane is below the list.
    pub list_height: u16,
}

/// Narrowest accounts-and-folders column ([`WindowLayout::sidebar_width`]).
pub const MIN_SIDEBAR_WIDTH: u16 = 200;
/// Widest accounts-and-folders column.
pub const MAX_SIDEBAR_WIDTH: u16 = 320;
/// The column's width until the user drags it.
pub const DEFAULT_SIDEBAR_WIDTH: u16 = 240;

/// Narrowest message list beside the reading pane ([`WindowLayout::list_width`]): room for the
/// avatar, a sender and the row's quick actions in the narrow (multi-line) rows.
pub const MIN_LIST_WIDTH: u16 = 320;
/// Widest message list beside the reading pane.
pub const MAX_LIST_WIDTH: u16 = 1600;
/// The list's width beside the pane until the user drags it (24 rem, as before stage 18).
pub const DEFAULT_LIST_WIDTH: u16 = 384;
/// Lowest message list above the reading pane ([`WindowLayout::list_height`]): a few rows.
pub const MIN_LIST_HEIGHT: u16 = 160;
/// Highest message list above the reading pane.
pub const MAX_LIST_HEIGHT: u16 = 1600;
/// The list's height above the pane until the user drags it.
pub const DEFAULT_LIST_HEIGHT: u16 = 320;

impl Default for WindowLayout {
    fn default() -> Self {
        Self {
            reading_pane: ReadingPanePosition::default(),
            density: Density::default(),
            sidebar_expanded: true,
            sidebar_width: DEFAULT_SIDEBAR_WIDTH,
            list_width: DEFAULT_LIST_WIDTH,
            list_height: DEFAULT_LIST_HEIGHT,
        }
    }
}

/// One size a drag handle sets: its name in messages, its field and its limits.
struct SizeField {
    name: &'static str,
    field: fn(&mut WindowLayout) -> &mut u16,
    min: u16,
    max: u16,
    default: u16,
}

const SIZE_FIELDS: [SizeField; 3] = [
    SizeField {
        name: "sidebar width",
        field: |layout| &mut layout.sidebar_width,
        min: MIN_SIDEBAR_WIDTH,
        max: MAX_SIDEBAR_WIDTH,
        default: DEFAULT_SIDEBAR_WIDTH,
    },
    SizeField {
        name: "list width",
        field: |layout| &mut layout.list_width,
        min: MIN_LIST_WIDTH,
        max: MAX_LIST_WIDTH,
        default: DEFAULT_LIST_WIDTH,
    },
    SizeField {
        name: "list height",
        field: |layout| &mut layout.list_height,
        min: MIN_LIST_HEIGHT,
        max: MAX_LIST_HEIGHT,
        default: DEFAULT_LIST_HEIGHT,
    },
];

impl WindowLayout {
    /// Checks the limits; the values come from the UI.
    pub fn validate(&self) -> Result<(), String> {
        let mut layout = *self;
        for size in &SIZE_FIELDS {
            let value = *(size.field)(&mut layout);
            if !(size.min..=size.max).contains(&value) {
                return Err(format!(
                    "{} {value} is outside {} to {} pixels",
                    size.name, size.min, size.max
                ));
            }
        }
        Ok(())
    }

    /// A stored layout as the UI may use it: every size outside its limits (an older version,
    /// an edited setting) takes its default. Returns the layout and the names of those sizes.
    #[must_use]
    pub fn with_sizes_in_range(mut self) -> (Self, Vec<&'static str>) {
        let mut replaced = Vec::new();
        for size in &SIZE_FIELDS {
            let value = (size.field)(&mut self);
            if !(size.min..=size.max).contains(value) {
                *value = size.default;
                replaced.push(size.name);
            }
        }
        (self, replaced)
    }
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
            conversations: false,
            column_views: vec![
                ColumnView {
                    id: "flagged".into(),
                    shown: true,
                },
                ColumnView {
                    id: "later".into(),
                    shown: false,
                },
            ],
            sidebar_folds: vec![SidebarFold {
                key: "more".into(),
                open: true,
            }],
        };
        let wire = json!({
            "shortcuts": {
                "macos": [{ "command": "archive", "shortcuts": [{ "key": "y", "shift": false, "mod": true }] }],
                "windows": [{ "command": "delete", "shortcuts": [] }]
            },
            "textSize": 115,
            "recentCommands": ["action:archive"],
            "aiPanel": "folded",
            "accentTheme": "rose",
            "conversations": false,
            "columnViews": [{ "id": "flagged", "shown": true }, { "id": "later", "shown": false }],
            "sidebarFolds": [{ "key": "more", "open": true }]
        });
        assert_eq!(serde_json::to_value(&prefs).expect("json"), wire);
        assert_eq!(
            serde_json::from_value::<UiPreferences>(wire).expect("parse"),
            prefs
        );
        let layout = WindowLayout {
            reading_pane: ReadingPanePosition::Replace,
            density: Density::Compact,
            sidebar_expanded: false,
            sidebar_width: 300,
            list_width: 512,
            list_height: 240,
        };
        let wire = json!({
            "readingPane": "replace",
            "density": "compact",
            "sidebarExpanded": false,
            "sidebarWidth": 300,
            "listWidth": 512,
            "listHeight": 240
        });
        assert_eq!(serde_json::to_value(layout).expect("json"), wire);
        assert_eq!(
            serde_json::from_value::<WindowLayout>(wire).expect("parse"),
            layout
        );
        // The accounts-and-folders column is shown by default, 240 pixels wide; the list beside
        // the reading pane is 384 pixels wide, above it 320 pixels high.
        assert_eq!(
            serde_json::to_value(WindowLayout::default()).expect("json"),
            json!({
                "readingPane": "right",
                "density": "comfortable",
                "sidebarExpanded": true,
                "sidebarWidth": 240,
                "listWidth": 384,
                "listHeight": 320
            })
        );
        assert!(WindowLayout::default().validate().is_ok());
        for width in [MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH] {
            let edge = WindowLayout {
                sidebar_width: width,
                ..WindowLayout::default()
            };
            assert!(edge.validate().is_ok(), "{width}");
        }
        for width in [0, MIN_SIDEBAR_WIDTH - 1, MAX_SIDEBAR_WIDTH + 1, u16::MAX] {
            let wrong = WindowLayout {
                sidebar_width: width,
                ..WindowLayout::default()
            };
            assert!(wrong.validate().is_err(), "{width}");
        }
    }

    /// Stage 18: the list's width beside the reading pane and its height above it have limits
    /// like the column's width; a stored size outside them reads as its default.
    #[test]
    fn list_sizes_stay_within_their_limits() {
        for width in [MIN_LIST_WIDTH, DEFAULT_LIST_WIDTH, MAX_LIST_WIDTH] {
            let edge = WindowLayout {
                list_width: width,
                ..WindowLayout::default()
            };
            assert!(edge.validate().is_ok(), "{width}");
        }
        for height in [MIN_LIST_HEIGHT, DEFAULT_LIST_HEIGHT, MAX_LIST_HEIGHT] {
            let edge = WindowLayout {
                list_height: height,
                ..WindowLayout::default()
            };
            assert!(edge.validate().is_ok(), "{height}");
        }
        for width in [0, MIN_LIST_WIDTH - 1, MAX_LIST_WIDTH + 1, u16::MAX] {
            let wrong = WindowLayout {
                list_width: width,
                ..WindowLayout::default()
            };
            assert_eq!(
                wrong.validate(),
                Err(format!("list width {width} is outside 320 to 1600 pixels"))
            );
        }
        for height in [0, MIN_LIST_HEIGHT - 1, MAX_LIST_HEIGHT + 1] {
            let wrong = WindowLayout {
                list_height: height,
                ..WindowLayout::default()
            };
            assert_eq!(
                wrong.validate(),
                Err(format!(
                    "list height {height} is outside 160 to 1600 pixels"
                ))
            );
        }
        let sidebar = WindowLayout {
            sidebar_width: 100,
            ..WindowLayout::default()
        };
        assert_eq!(
            sidebar.validate(),
            Err("sidebar width 100 is outside 200 to 320 pixels".to_owned())
        );

        // Stored sizes: each one outside its limits takes its default, the others stay.
        let stored = WindowLayout {
            reading_pane: ReadingPanePosition::Bottom,
            sidebar_width: 280,
            list_width: 2000,
            list_height: 10,
            ..WindowLayout::default()
        };
        let (fixed, replaced) = stored.with_sizes_in_range();
        assert_eq!(
            fixed,
            WindowLayout {
                list_width: DEFAULT_LIST_WIDTH,
                list_height: DEFAULT_LIST_HEIGHT,
                ..stored
            }
        );
        assert_eq!(replaced, vec!["list width", "list height"]);
        assert!(fixed.validate().is_ok());
        let fine = WindowLayout {
            list_width: 700,
            ..WindowLayout::default()
        };
        assert_eq!(fine.with_sizes_in_range(), (fine, Vec::new()));
    }

    #[test]
    fn defaults_are_valid() {
        assert_eq!(UiPreferences::default().text_size, 100);
        assert_eq!(UiPreferences::default().ai_panel, AiPanelState::Open);
        assert!(UiPreferences::default().validate().is_ok());
        // Decision 0053: blue is the default accent, also where a stored value cannot be read.
        assert_eq!(UiPreferences::default().accent_theme, AccentTheme::Blue);
        assert_eq!(AccentTheme::default(), AccentTheme::Blue);
    }

    #[test]
    fn the_ai_panel_state_is_one_of_two_values() {
        let wire = |panel: serde_json::Value| {
            json!({
                "shortcuts": { "macos": [], "windows": [] },
                "textSize": 100,
                "recentCommands": [],
                "aiPanel": panel,
                "accentTheme": "orange",
                "conversations": true,
                "columnViews": [],
                "sidebarFolds": []
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

    /// Stage 23 (decision 0085): nothing opened or closed by default; the keys are simple names
    /// (an account's parts carry its id), bounded, none twice; the field is required on the wire.
    #[test]
    fn sidebar_folds_default_to_empty_and_are_checked() {
        assert!(UiPreferences::default().sidebar_folds.is_empty());
        let fold = |key: &str, open: bool| SidebarFold {
            key: key.into(),
            open,
        };
        let chosen = UiPreferences {
            sidebar_folds: vec![
                fold("more", true),
                fold("favorites", false),
                fold("account:0b6f6c1e-5d1a-4a4b-9f0e-2d7c3a1b9e44", false),
                fold("folders:0b6f6c1e-5d1a-4a4b-9f0e-2d7c3a1b9e44", true),
            ],
            ..UiPreferences::default()
        };
        assert!(chosen.validate().is_ok());
        let at_most: Vec<SidebarFold> = (0..MAX_SIDEBAR_FOLDS)
            .map(|i| fold(&format!("account:{i}"), false))
            .collect();
        assert!(validate_sidebar_folds(&at_most).is_ok());
        let too_many: Vec<SidebarFold> = (0..=MAX_SIDEBAR_FOLDS)
            .map(|i| fold(&format!("account:{i}"), false))
            .collect();
        assert_eq!(
            validate_sidebar_folds(&too_many),
            Err(UiPreferencesError::TooManySidebarFolds(129))
        );
        assert_eq!(
            validate_sidebar_folds(&[fold("more", true), fold("more", false)]),
            Err(UiPreferencesError::DuplicateSidebarFold { key: "more".into() })
        );
        assert!(validate_sidebar_folds(&[fold(&"a".repeat(64), true)]).is_ok());
        for bad in ["", "a b", "mehr…", "a/b", "a.b", &"a".repeat(65)] {
            assert!(
                matches!(
                    validate_sidebar_folds(&[fold(bad, true)]),
                    Err(UiPreferencesError::InvalidSidebarFold { index: 0, .. })
                ),
                "{bad:?}"
            );
        }
        let refused = UiPreferences {
            sidebar_folds: vec![fold("", true)],
            ..UiPreferences::default()
        };
        assert!(refused.validate().is_err());
        let mut wire = serde_json::to_value(UiPreferences::default()).expect("json");
        assert_eq!(wire["sidebarFolds"], json!([]));
        wire["sidebarFolds"] = json!([{ "key": "more", "open": "yes" }]);
        assert!(serde_json::from_value::<UiPreferences>(wire.clone()).is_err());
        wire.as_object_mut().expect("object").remove("sidebarFolds");
        assert!(serde_json::from_value::<UiPreferences>(wire).is_err());
    }

    /// Stage 17: no choice of the column's views by default (the UI's default order, all
    /// shown); a choice is bounded, its ids are simple names, none twice.
    #[test]
    fn column_views_default_to_empty_and_are_checked() {
        assert!(UiPreferences::default().column_views.is_empty());
        let view = |id: &str, shown: bool| ColumnView {
            id: id.into(),
            shown,
        };
        let chosen = UiPreferences {
            column_views: vec![
                view("contacts", true),
                view("later", false),
                view("pinnedAttachments", true),
                view("saved_views-2", false),
            ],
            ..UiPreferences::default()
        };
        assert!(chosen.validate().is_ok());
        // Every view hidden is a choice too (the column shows a hint then).
        let none_shown = UiPreferences {
            column_views: vec![view("later", false), view("tasks", false)],
            ..UiPreferences::default()
        };
        assert!(none_shown.validate().is_ok());

        let at_most: Vec<ColumnView> = (0..MAX_COLUMN_VIEWS)
            .map(|i| view(&format!("v{i}"), true))
            .collect();
        assert!(validate_column_views(&at_most).is_ok());
        let too_many: Vec<ColumnView> = (0..=MAX_COLUMN_VIEWS)
            .map(|i| view(&format!("v{i}"), true))
            .collect();
        assert_eq!(
            validate_column_views(&too_many),
            Err(UiPreferencesError::TooManyColumnViews(33))
        );
        assert_eq!(
            validate_column_views(&[view("later", true), view("later", false)]),
            Err(UiPreferencesError::DuplicateColumnView { id: "later".into() })
        );
        assert!(validate_column_views(&[view(&"a".repeat(64), true)]).is_ok());
        for bad in ["", "a b", "später", "view:later", "a/b", &"a".repeat(65)] {
            assert!(
                matches!(
                    validate_column_views(&[view(bad, true)]),
                    Err(UiPreferencesError::InvalidColumnView { index: 0, .. })
                ),
                "{bad:?}"
            );
        }
        // The wire form: `shown` is a boolean, the field itself is required like the others.
        let mut wire = serde_json::to_value(UiPreferences::default()).expect("json");
        assert_eq!(wire["columnViews"], json!([]));
        wire["columnViews"] = json!([{ "id": "later", "shown": "no" }]);
        assert!(serde_json::from_value::<UiPreferences>(wire.clone()).is_err());
        wire.as_object_mut().expect("object").remove("columnViews");
        assert!(serde_json::from_value::<UiPreferences>(wire).is_err());
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
