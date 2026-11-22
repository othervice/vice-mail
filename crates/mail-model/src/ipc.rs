use serde::{Deserialize, Serialize};

/// Operating system the app runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Macos,
    Windows,
    Linux,
}

impl Platform {
    /// Platform this binary was built for.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Version information shown under Settings, About.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// Short git hash at build time, null if unknown.
    pub commit: Option<String>,
    /// RFC 3339 build timestamp, null if unknown.
    pub build_date: Option<String>,
    pub platform: Platform,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePreference {
    #[default]
    System,
    En,
    De,
}

/// User preferences stored in the `setting` table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub theme: ThemePreference,
    pub language: LanguagePreference,
}

/// The one event stream from the core to the UI. Internally tagged by `type`.
///
/// - `preferencesChanged`: preferences were stored; carries the stored values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoreEvent {
    PreferencesChanged { preferences: Preferences },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_format_matches_the_contract() {
        let info = AppInfo {
            version: "0.1.0".into(),
            commit: None,
            build_date: Some("2026-11-22T18:00:00Z".into()),
            platform: Platform::Macos,
        };
        assert_eq!(
            serde_json::to_value(&info).expect("serialize"),
            json!({ "version": "0.1.0", "commit": null, "buildDate": "2026-11-22T18:00:00Z", "platform": "macos" })
        );

        let prefs = Preferences {
            theme: ThemePreference::Dark,
            language: LanguagePreference::De,
        };
        let event = CoreEvent::PreferencesChanged { preferences: prefs };
        assert_eq!(
            serde_json::to_value(&event).expect("serialize"),
            json!({ "type": "preferencesChanged", "preferences": { "theme": "dark", "language": "de" } })
        );

        let parsed: Preferences =
            serde_json::from_value(json!({ "theme": "light", "language": "system" }))
                .expect("deserialize");
        assert_eq!(parsed.theme, ThemePreference::Light);
        assert!(
            serde_json::from_value::<Preferences>(json!({ "theme": "blue", "language": "en" }))
                .is_err()
        );
    }

    #[test]
    fn defaults_are_system() {
        assert_eq!(
            Preferences::default(),
            Preferences {
                theme: ThemePreference::System,
                language: LanguagePreference::System
            }
        );
    }
}
