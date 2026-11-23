//! Stage 8: UI preferences and window layouts in the `setting` table (decision 0025).
//!
//! | Key | Content | Sync |
//! |---|---|---|
//! | `ui.shortcuts.macos`, `ui.shortcuts.windows` | changed commands of one platform | synced |
//! | `ui.text_size` | percent, 80 to 150 | synced |
//! | `ui.accent_theme` | one of the six accent themes (stage 16, decision 0036) | synced |
//! | `ui.conversations` | one row per conversation (default) or per message (decision 0039) | synced |
//! | `ui.recent_commands` | palette entry keys, newest first | device |
//! | `ui.ai_panel` | the AI panel unfolded or folded in (decision 0028) | device |
//! | `ui.layout.<slot>` | [`WindowLayout`] of one kind of window | device |
//!
//! One key per platform, so a change on the Mac and one on a Windows machine never compete for
//! the same record (the settings sync merges per key, newer wins). The shortcut lists are stored
//! compactly (`shift` and `mod` only when set) and checked against the sync's own record check
//! before they are stored, so a stored list always fits into a sync record.

use mail_model::{
    AccentTheme, AiPanelState, CoreError, CoreEvent, DEFAULT_TEXT_SIZE, PlatformShortcuts,
    Shortcut, ShortcutOverride, ShortcutPlatform, UiPreferences, UiPreferencesError, WindowLayout,
    validate_layout_slot, validate_overrides, validate_recent_commands,
};
use mail_store::GLOBAL_SCOPE;
use serde::{Deserialize, Serialize};

use crate::AppCore;
use crate::core::Connector;
use crate::settings_sync::{RecordKind, SyncContent};

pub(crate) const SHORTCUTS_MACOS_KEY: &str = "ui.shortcuts.macos";
pub(crate) const SHORTCUTS_WINDOWS_KEY: &str = "ui.shortcuts.windows";
pub(crate) const TEXT_SIZE_KEY: &str = "ui.text_size";
/// Stage 16 (decision 0036): synced like the text size.
pub(crate) const ACCENT_THEME_KEY: &str = "ui.accent_theme";
/// Decision 0039: the conversation view, synced like the text size.
pub(crate) const CONVERSATIONS_KEY: &str = "ui.conversations";
pub(crate) const RECENT_COMMANDS_KEY: &str = "ui.recent_commands";
/// Device only (decision 0028): AI in the app is set up per device (0024), and the panel is
/// part of the window's layout, which stays on the device too (0025).
pub(crate) const AI_PANEL_KEY: &str = "ui.ai_panel";
const LAYOUT_PREFIX: &str = "ui.layout.";

/// The stored form of one changed command.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredOverride {
    command: String,
    shortcuts: Vec<StoredShortcut>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredShortcut {
    key: String,
    #[serde(default, skip_serializing_if = "is_false")]
    shift: bool,
    #[serde(default, rename = "mod", skip_serializing_if = "is_false")]
    modifier: bool,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference
fn is_false(value: &bool) -> bool {
    !*value
}

fn to_stored(overrides: &[ShortcutOverride]) -> Vec<StoredOverride> {
    overrides
        .iter()
        .map(|entry| StoredOverride {
            command: entry.command.clone(),
            shortcuts: entry
                .shortcuts
                .iter()
                .map(|s| StoredShortcut {
                    key: s.key.clone(),
                    shift: s.shift,
                    modifier: s.modifier,
                })
                .collect(),
        })
        .collect()
}

fn from_stored(stored: Vec<StoredOverride>) -> Vec<ShortcutOverride> {
    stored
        .into_iter()
        .map(|entry| ShortcutOverride {
            command: entry.command,
            shortcuts: entry
                .shortcuts
                .into_iter()
                .map(|s| Shortcut {
                    key: s.key,
                    shift: s.shift,
                    modifier: s.modifier,
                })
                .collect(),
        })
        .collect()
}

/// The stored JSON of one platform's list, ordered by command so unchanged data keeps its
/// version. Refused if the settings sync would refuse it as a record (size, or a value that
/// looks like key material).
fn encode_overrides(
    platform: ShortcutPlatform,
    overrides: &[ShortcutOverride],
) -> Result<String, CoreError> {
    let mut sorted = overrides.to_vec();
    sorted.sort_by(|a, b| a.command.cmp(&b.command));
    let value = serde_json::to_value(to_stored(&sorted))
        .map_err(|err| CoreError::Internal(format!("encode shortcuts: {err}")))?;
    let text = value.to_string();
    if let Err(err) = SyncContent::new(RecordKind::Setting, value) {
        let max = RecordKind::Setting.spec().max_content_bytes;
        let refused = if text.len() > max {
            UiPreferencesError::TooLarge {
                platform: platform.name(),
                bytes: text.len(),
                max,
            }
            .to_string()
        } else {
            format!("{}: {err}", platform.name())
        };
        return Err(CoreError::InvalidArgument(refused));
    }
    Ok(text)
}

/// A stored list; anything this version cannot read or does not accept (a damaged value, a
/// list from a newer version with other limits) counts as "no changes" and is logged.
fn decode_overrides(platform: ShortcutPlatform, key: &str, json: &str) -> Vec<ShortcutOverride> {
    let parsed = serde_json::from_str::<Vec<StoredOverride>>(json)
        .map_err(|err| err.to_string())
        .map(from_stored)
        .and_then(|list| {
            validate_overrides(platform, &list)
                .map(|()| list)
                .map_err(|err| err.to_string())
        });
    parsed.unwrap_or_else(|reason| {
        tracing::warn!(key, %reason, "stored shortcuts unreadable, using the defaults");
        Vec::new()
    })
}

fn decode_text_size(json: &str) -> u8 {
    match serde_json::from_str::<u8>(json) {
        Ok(size) if (mail_model::MIN_TEXT_SIZE..=mail_model::MAX_TEXT_SIZE).contains(&size) => size,
        _ => {
            tracing::warn!(
                key = TEXT_SIZE_KEY,
                "stored text size unreadable, using 100"
            );
            DEFAULT_TEXT_SIZE
        }
    }
}

/// A theme this version does not know (from a newer device) reads as the default.
fn decode_accent(json: &str) -> AccentTheme {
    serde_json::from_str(json).unwrap_or_else(|err| {
        tracing::warn!(key = ACCENT_THEME_KEY, %err, "stored accent theme unreadable, using the default");
        AccentTheme::default()
    })
}

/// Anything but a boolean reads as the default (on).
fn decode_conversations(json: &str) -> bool {
    serde_json::from_str(json).unwrap_or_else(|err| {
        tracing::warn!(key = CONVERSATIONS_KEY, %err, "stored conversation view unreadable, using on");
        true
    })
}

fn decode_recent(json: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<String>>(json) {
        Ok(list) if validate_recent_commands(&list).is_ok() => list,
        _ => {
            tracing::warn!(
                key = RECENT_COMMANDS_KEY,
                "stored recent commands unreadable"
            );
            Vec::new()
        }
    }
}

fn decode_ai_panel(json: &str) -> AiPanelState {
    serde_json::from_str(json).unwrap_or_else(|err| {
        tracing::warn!(key = AI_PANEL_KEY, %err, "stored AI panel state unreadable, using open");
        AiPanelState::default()
    })
}

/// A stored layout as older versions wrote it: fields missing then take the default (a layout
/// stored before the column's width existed keeps its reading pane, density and sidebar).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredLayout {
    #[serde(default)]
    reading_pane: mail_model::ReadingPanePosition,
    #[serde(default)]
    density: mail_model::Density,
    #[serde(default = "default_sidebar_expanded")]
    sidebar_expanded: bool,
    #[serde(default = "default_sidebar_width")]
    sidebar_width: u16,
}

fn default_sidebar_expanded() -> bool {
    WindowLayout::default().sidebar_expanded
}

fn default_sidebar_width() -> u16 {
    mail_model::DEFAULT_SIDEBAR_WIDTH
}

fn decode_layout(key: &str, json: &str) -> WindowLayout {
    let stored = match serde_json::from_str::<StoredLayout>(json) {
        Ok(stored) => stored,
        Err(err) => {
            tracing::warn!(key, %err, "stored window layout unreadable, using the default");
            return WindowLayout::default();
        }
    };
    let mut layout = WindowLayout {
        reading_pane: stored.reading_pane,
        density: stored.density,
        sidebar_expanded: stored.sidebar_expanded,
        sidebar_width: stored.sidebar_width,
    };
    if let Err(reason) = layout.validate() {
        tracing::warn!(key, %reason, "stored sidebar width not accepted, using the default");
        layout.sidebar_width = mail_model::DEFAULT_SIDEBAR_WIDTH;
    }
    layout
}

fn layout_key(slot: &str) -> Result<String, CoreError> {
    validate_layout_slot(slot).map_err(CoreError::InvalidArgument)?;
    Ok(format!("{LAYOUT_PREFIX}{slot}"))
}

fn encode<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value)
        .map_err(|err| CoreError::Internal(format!("encode setting: {err}")))
}

impl<C: Connector> AppCore<C> {
    /// Stage 8: shortcuts, text size and recent palette commands; defaults for what is not
    /// stored.
    pub async fn get_ui_preferences(&self) -> Result<UiPreferences, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        let mut prefs = UiPreferences::default();
        for setting in settings {
            let (key, value) = (setting.key.as_str(), setting.value.as_str());
            match key {
                SHORTCUTS_MACOS_KEY => {
                    prefs.shortcuts.macos = decode_overrides(ShortcutPlatform::Macos, key, value);
                }
                SHORTCUTS_WINDOWS_KEY => {
                    prefs.shortcuts.windows =
                        decode_overrides(ShortcutPlatform::Windows, key, value);
                }
                TEXT_SIZE_KEY => prefs.text_size = decode_text_size(value),
                ACCENT_THEME_KEY => prefs.accent_theme = decode_accent(value),
                CONVERSATIONS_KEY => prefs.conversations = decode_conversations(value),
                RECENT_COMMANDS_KEY => prefs.recent_commands = decode_recent(value),
                AI_PANEL_KEY => prefs.ai_panel = decode_ai_panel(value),
                _ => {}
            }
        }
        Ok(prefs)
    }

    /// Validates and stores all UI preferences in one transaction, emits `uiPreferencesChanged`
    /// and returns what is stored now (the shortcut lists ordered by command). Shortcuts and
    /// text size reach the other devices through the settings sync.
    pub async fn set_ui_preferences(
        &self,
        preferences: UiPreferences,
    ) -> Result<UiPreferences, CoreError> {
        preferences
            .validate()
            .map_err(|err| CoreError::InvalidArgument(err.to_string()))?;
        let PlatformShortcuts { macos, windows } = &preferences.shortcuts;
        let entries = vec![
            (
                SHORTCUTS_MACOS_KEY.to_owned(),
                encode_overrides(ShortcutPlatform::Macos, macos)?,
            ),
            (
                SHORTCUTS_WINDOWS_KEY.to_owned(),
                encode_overrides(ShortcutPlatform::Windows, windows)?,
            ),
            (TEXT_SIZE_KEY.to_owned(), encode(&preferences.text_size)?),
            (
                ACCENT_THEME_KEY.to_owned(),
                encode(&preferences.accent_theme)?,
            ),
            (
                CONVERSATIONS_KEY.to_owned(),
                encode(&preferences.conversations)?,
            ),
            (
                RECENT_COMMANDS_KEY.to_owned(),
                encode(&preferences.recent_commands)?,
            ),
            (AI_PANEL_KEY.to_owned(), encode(&preferences.ai_panel)?),
        ];
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, entries)
            .await?;
        let stored = self.get_ui_preferences().await?;
        tracing::info!(
            macos = stored.shortcuts.macos.len(),
            windows = stored.shortcuts.windows.len(),
            text_size = stored.text_size,
            accent = ?stored.accent_theme,
            conversations = stored.conversations,
            "UI preferences stored"
        );
        self.emit_event(CoreEvent::UiPreferencesChanged {
            preferences: stored.clone(),
        });
        Ok(stored)
    }

    /// Stage 8: the layout of one kind of window on this device (`slot`, see
    /// [`validate_layout_slot`]); the default when nothing is stored.
    pub async fn get_window_layout(&self, slot: &str) -> Result<WindowLayout, CoreError> {
        let key = layout_key(slot)?;
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(settings
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| decode_layout(&key, &s.value))
            .unwrap_or_default())
    }

    /// Stores the layout of one kind of window on this device and returns it. Not synchronised.
    pub async fn set_window_layout(
        &self,
        slot: &str,
        layout: WindowLayout,
    ) -> Result<WindowLayout, CoreError> {
        let key = layout_key(slot)?;
        layout.validate().map_err(CoreError::InvalidArgument)?;
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(key, encode(&layout)?)])
            .await?;
        tracing::debug!(slot, ?layout, "window layout stored");
        Ok(layout)
    }

    /// Stage 8: the subject of `message` of `account`, before a window opens for it (the
    /// window's title). `NotFound` if the message does not exist or belongs to another account.
    pub async fn message_window_subject(
        &self,
        account: mail_model::AccountId,
        message: mail_model::MessageId,
    ) -> Result<String, CoreError> {
        match self.store_ref()?.message_summary(message).await? {
            Some(summary) if summary.account_id == account => Ok(summary.subject),
            _ => Err(CoreError::NotFound(format!(
                "message {} of account {account}",
                message.0
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
