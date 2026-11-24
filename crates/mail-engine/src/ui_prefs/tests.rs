//! Stage 8: UI preferences and window layouts through `AppCore` with the real store.

use std::time::Duration;

use mail_model::{
    AiPanelState, ColumnView, CoreError, CoreEvent, Density, ErrorCode, PlatformShortcuts,
    ReadingPanePosition, Shortcut, ShortcutOverride, UiPreferences, WindowLayout,
};
use mail_store::GLOBAL_SCOPE;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::credentials::{Credentials, MemorySecretStore};
use crate::settings_sync::{RecordKey, SettingScope};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

async fn core(dir: &TempDir) -> AppCore {
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0".into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::new(Credentials::new(MemorySecretStore::new())),
    )
    .await;
    assert!(core.startup_error().is_none());
    core
}

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

fn sample() -> UiPreferences {
    UiPreferences {
        shortcuts: PlatformShortcuts {
            macos: vec![
                entry("toggleRead", vec![shortcut("u", true, true)]),
                entry("archive", vec![shortcut("y", false, true)]),
            ],
            windows: vec![entry("delete", Vec::new())],
        },
        text_size: 130,
        recent_commands: vec!["action:archive".into(), "view:inbox".into()],
        ai_panel: AiPanelState::Folded,
        accent_theme: mail_model::AccentTheme::Violet,
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
    }
}

async fn stored(core: &AppCore, key: &str) -> Option<Value> {
    core.store_for_tests()
        .settings(GLOBAL_SCOPE)
        .await
        .expect("settings")
        .into_iter()
        .find(|s| s.key == key)
        .map(|s| serde_json::from_str(&s.value).expect("stored JSON"))
}

fn invalid(result: Result<UiPreferences, CoreError>) -> String {
    match result {
        Err(err) => {
            assert_eq!(err.code(), ErrorCode::InvalidArgument, "{err}");
            err.detail().unwrap_or_default()
        }
        Ok(stored) => panic!("accepted: {stored:?}"),
    }
}

#[tokio::test]
async fn ui_preferences_are_defaults_until_stored() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    let prefs = core.get_ui_preferences().await.expect("read");
    assert_eq!(prefs, UiPreferences::default());
    assert_eq!(prefs.text_size, 100);
}

#[tokio::test]
async fn set_ui_preferences_stores_returns_and_emits() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    let mut events = core.subscribe();

    let stored_prefs = core.set_ui_preferences(sample()).await.expect("store");
    // Returned as stored: each platform's list ordered by command.
    let mut expected = sample();
    expected.shortcuts.macos.reverse();
    assert_eq!(stored_prefs, expected);
    assert_eq!(core.get_ui_preferences().await.expect("read"), expected);

    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("event in time")
        .expect("event");
    assert_eq!(
        event,
        CoreEvent::UiPreferencesChanged {
            preferences: expected.clone()
        }
    );

    // Survives a restart (the same database).
    drop(core);
    let again = self::core(&dir).await;
    assert_eq!(again.get_ui_preferences().await.expect("read"), expected);
}

#[tokio::test]
async fn shortcuts_are_stored_compactly_per_platform() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    core.set_ui_preferences(sample()).await.expect("store");
    assert_eq!(
        stored(&core, "ui.shortcuts.macos").await,
        Some(json!([
            { "command": "archive", "shortcuts": [{ "key": "y", "mod": true }] },
            { "command": "toggleRead", "shortcuts": [{ "key": "u", "shift": true, "mod": true }] }
        ]))
    );
    assert_eq!(
        stored(&core, "ui.shortcuts.windows").await,
        Some(json!([{ "command": "delete", "shortcuts": [] }]))
    );
    assert_eq!(stored(&core, "ui.text_size").await, Some(json!(130)));
    assert_eq!(
        stored(&core, "ui.recent_commands").await,
        Some(json!(["action:archive", "view:inbox"]))
    );
    assert_eq!(stored(&core, "ui.ai_panel").await, Some(json!("folded")));
    assert_eq!(
        stored(&core, "ui.accent_theme").await,
        Some(json!("violet"))
    );
}

/// Decision 0036: the accent theme is a UI preference, synced like the text size, orange by
/// default (decision 0038); only the six themes pass the bridge, and a stored theme this
/// version does not know (a newer device) reads as the default.
#[tokio::test]
async fn the_accent_theme_is_stored_validated_and_synced() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    assert_eq!(
        core.get_ui_preferences().await.expect("read").accent_theme,
        mail_model::AccentTheme::Orange
    );
    let mut prefs = UiPreferences::default();
    // Orange last: what the user chose stays, also the default once chosen.
    for theme in ["teal", "blue", "violet", "rose", "green", "orange"] {
        prefs.accent_theme = serde_json::from_value(json!(theme)).expect("theme");
        let stored_prefs = core.set_ui_preferences(prefs.clone()).await.expect("store");
        assert_eq!(stored_prefs.accent_theme, prefs.accent_theme);
        assert_eq!(stored(&core, "ui.accent_theme").await, Some(json!(theme)));
    }
    // What the UI sends is checked at the bridge (the enum): anything else does not decode.
    let mut wire = serde_json::to_value(UiPreferences::default()).expect("json");
    wire["accentTheme"] = json!("magenta");
    assert!(serde_json::from_value::<UiPreferences>(wire).is_err());
    // A theme from a newer version reads as the default.
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![("ui.accent_theme".to_owned(), "\"magenta\"".to_owned())],
        )
        .await
        .expect("seed");
    assert_eq!(
        core.get_ui_preferences().await.expect("read").accent_theme,
        mail_model::AccentTheme::Orange
    );
    assert!(RecordKey::setting(SettingScope::Global, "ui.accent_theme").is_ok());
}

/// Decision 0039: the conversation view is a UI preference, on by default, synced like the text
/// size; a value that is not a boolean reads as on.
#[tokio::test]
async fn the_conversation_view_is_on_by_default_stored_and_synced() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    let mut events = core.subscribe();
    assert!(core.get_ui_preferences().await.expect("read").conversations);
    let prefs = UiPreferences {
        conversations: false,
        ..UiPreferences::default()
    };
    let stored_prefs = core.set_ui_preferences(prefs).await.expect("store");
    assert!(!stored_prefs.conversations);
    assert_eq!(stored(&core, "ui.conversations").await, Some(json!(false)));
    let announced = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("event in time")
            .expect("event")
        {
            CoreEvent::UiPreferencesChanged { preferences } => break preferences,
            _ => continue,
        }
    };
    assert!(!announced.conversations);
    assert!(!core.get_ui_preferences().await.expect("read").conversations);
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![("ui.conversations".to_owned(), "\"threads\"".to_owned())],
        )
        .await
        .expect("seed");
    assert!(core.get_ui_preferences().await.expect("read").conversations);
    assert!(RecordKey::setting(SettingScope::Global, "ui.conversations").is_ok());
    assert!(crate::settings_sync::SYNCED_SETTINGS.contains(&"ui.conversations"));
}

/// Stage 17: the column's views are a UI preference, the default (an empty list: every view in
/// the UI's order) until the user chooses; the choice is stored in the user's order, survives a
/// restart, is announced, synced like the text size, and a stored list this version cannot read
/// or does not accept reads as the default. "Reset to default" stores the empty list again.
#[tokio::test]
async fn the_column_views_default_round_trip_and_sync() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    let mut events = core.subscribe();
    assert!(
        core.get_ui_preferences()
            .await
            .expect("read")
            .column_views
            .is_empty()
    );
    let view = |id: &str, shown: bool| ColumnView {
        id: id.into(),
        shown,
    };
    let chosen = vec![
        view("contacts", true),
        view("flagged", true),
        view("later", false),
        // A view of a later version: kept as it is.
        view("someFutureView", true),
    ];
    let prefs = UiPreferences {
        column_views: chosen.clone(),
        ..UiPreferences::default()
    };
    let stored_prefs = core.set_ui_preferences(prefs).await.expect("store");
    assert_eq!(
        stored_prefs.column_views, chosen,
        "the user's order, not sorted"
    );
    assert_eq!(
        stored(&core, "ui.column_views").await,
        Some(json!([
            { "id": "contacts", "shown": true },
            { "id": "flagged", "shown": true },
            { "id": "later", "shown": false },
            { "id": "someFutureView", "shown": true }
        ]))
    );
    let announced = loop {
        match tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("event in time")
            .expect("event")
        {
            CoreEvent::UiPreferencesChanged { preferences } => break preferences,
            _ => continue,
        }
    };
    assert_eq!(announced.column_views, chosen);

    // Survives a restart.
    drop(core);
    let core = self::core(&dir).await;
    assert_eq!(
        core.get_ui_preferences().await.expect("read").column_views,
        chosen
    );

    // Refused at the bridge: an id twice, a bad id, too many.
    for wrong in [
        vec![view("later", true), view("later", false)],
        vec![view("go to", true)],
        (0..33).map(|i| view(&format!("v{i}"), true)).collect(),
    ] {
        let detail = invalid(
            core.set_ui_preferences(UiPreferences {
                column_views: wrong,
                ..UiPreferences::default()
            })
            .await,
        );
        assert!(detail.contains("column view"), "{detail}");
    }
    assert_eq!(
        core.get_ui_preferences().await.expect("read").column_views,
        chosen,
        "a refusal changes nothing"
    );

    // Unreadable or not accepted (a damaged value, other limits): the default.
    for damaged in [
        "\"later\"",
        "[{\"id\":\"later\"}]",
        "[{\"id\":\"a b\",\"shown\":true}]",
        "null",
    ] {
        core.store_for_tests()
            .put_settings(
                GLOBAL_SCOPE,
                vec![("ui.column_views".to_owned(), damaged.to_owned())],
            )
            .await
            .expect("seed");
        assert!(
            core.get_ui_preferences()
                .await
                .expect("read")
                .column_views
                .is_empty(),
            "{damaged}"
        );
    }

    // "Reset to default": the empty list.
    core.set_ui_preferences(UiPreferences::default())
        .await
        .expect("reset");
    assert_eq!(stored(&core, "ui.column_views").await, Some(json!([])));
    assert!(RecordKey::setting(SettingScope::Global, "ui.column_views").is_ok());
    assert!(crate::settings_sync::SYNCED_SETTINGS.contains(&"ui.column_views"));
}

/// Decision 0028: the AI panel's state is a UI preference of this device: stored, announced,
/// back after a restart, unreadable values count as open, never synchronised.
#[tokio::test]
async fn the_ai_panel_state_stays_on_this_device() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    assert_eq!(
        core.get_ui_preferences().await.expect("read").ai_panel,
        AiPanelState::Open
    );
    let mut events = core.subscribe();
    let folded = UiPreferences {
        ai_panel: AiPanelState::Folded,
        ..UiPreferences::default()
    };
    assert_eq!(
        core.set_ui_preferences(folded.clone())
            .await
            .expect("store")
            .ai_panel,
        AiPanelState::Folded
    );
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("event in time")
        .expect("event");
    assert!(
        matches!(&event, CoreEvent::UiPreferencesChanged { preferences } if preferences.ai_panel == AiPanelState::Folded),
        "{event:?}"
    );
    drop(core);
    let core = self::core(&dir).await;
    assert_eq!(core.get_ui_preferences().await.expect("read"), folded);
    assert!(RecordKey::setting(SettingScope::Global, "ui.ai_panel").is_err());
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![("ui.ai_panel".into(), "\"sideways\"".into())],
        )
        .await
        .expect("seed");
    assert_eq!(
        core.get_ui_preferences().await.expect("read").ai_panel,
        AiPanelState::Open
    );
}

#[tokio::test]
async fn invalid_ui_preferences_are_refused_and_nothing_changes() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    core.set_ui_preferences(sample()).await.expect("store");
    let mut events = core.subscribe();

    let mut big_text = sample();
    big_text.text_size = 151;
    assert!(invalid(core.set_ui_preferences(big_text).await).contains("text size 151"));

    let mut twice = sample();
    twice.shortcuts.windows = vec![entry("delete", Vec::new()), entry("delete", Vec::new())];
    assert!(invalid(core.set_ui_preferences(twice).await).contains("appears twice"));

    let mut recent = sample();
    recent.recent_commands = (0..6).map(|i| format!("action:{i}")).collect();
    assert!(invalid(core.set_ui_preferences(recent).await).contains("recent commands"));

    let mut long_key = sample();
    long_key.shortcuts.macos = vec![entry(
        "archive",
        vec![shortcut(&"k".repeat(21), false, false)],
    )];
    assert!(invalid(core.set_ui_preferences(long_key).await).contains("longer than 20"));

    let mut sorted = sample();
    sorted.shortcuts.macos.reverse();
    assert_eq!(core.get_ui_preferences().await.expect("read"), sorted);
    assert!(events.try_recv().is_err(), "no event for a refused change");
}

#[tokio::test]
async fn every_command_with_two_shortcuts_fits_a_sync_record() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    // About the size of the action directory (stage 8: 35 actions and app commands), each with
    // two shortcuts and modifiers.
    let all: Vec<ShortcutOverride> = (0..50)
        .map(|i| {
            entry(
                &format!("command{i:02}"),
                vec![
                    shortcut("ArrowDown", true, true),
                    shortcut("F12", false, true),
                ],
            )
        })
        .collect();
    let prefs = UiPreferences {
        shortcuts: PlatformShortcuts {
            macos: all.clone(),
            windows: all,
        },
        ..UiPreferences::default()
    };
    core.set_ui_preferences(prefs).await.expect("fits");
    let value = stored(&core, "ui.shortcuts.macos").await.expect("stored");
    assert!(SyncContent::new(RecordKind::Setting, value).is_ok());

    // The limits alone would allow more than one record holds: refused with the size.
    let huge: Vec<ShortcutOverride> = (0..100)
        .map(|i| {
            entry(
                &format!("{i:0>64}"),
                (0..4)
                    .map(|k| shortcut(&format!("{k:x>20}"), true, true))
                    .collect(),
            )
        })
        .collect();
    let too_large = UiPreferences {
        shortcuts: PlatformShortcuts {
            macos: huge,
            windows: Vec::new(),
        },
        ..UiPreferences::default()
    };
    assert!(too_large.validate().is_ok(), "within the per-field limits");
    let detail = invalid(core.set_ui_preferences(too_large).await);
    assert!(
        detail.contains("macos") && detail.contains("at most 16384"),
        "{detail}"
    );
}

#[tokio::test]
async fn unreadable_stored_values_fall_back_to_defaults() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    let put = |key: &str, value: &str| (key.to_owned(), value.to_owned());
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![
                put("ui.shortcuts.macos", "{\"archive\": \"e\"}"),
                // A list another version might accept, this one does not (duplicate command).
                put(
                    "ui.shortcuts.windows",
                    "[{\"command\":\"a\",\"shortcuts\":[]},{\"command\":\"a\",\"shortcuts\":[]}]",
                ),
                put("ui.text_size", "300"),
                put("ui.recent_commands", "[\"\"]"),
                put("ui.ai_panel", "42"),
                put("ui.accent_theme", "7"),
                put("ui.layout.main", "{\"readingPane\":\"left\"}"),
            ],
        )
        .await
        .expect("seed");
    assert_eq!(
        core.get_ui_preferences().await.expect("read"),
        UiPreferences::default()
    );
    assert_eq!(
        core.get_window_layout("main").await.expect("layout"),
        WindowLayout::default()
    );
}

#[tokio::test]
async fn window_layouts_are_stored_per_slot_on_this_device() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    assert_eq!(
        core.get_window_layout("main").await.expect("default"),
        WindowLayout::default()
    );
    let main = WindowLayout {
        reading_pane: ReadingPanePosition::Bottom,
        density: Density::Compact,
        sidebar_expanded: false,
        sidebar_width: 280,
    };
    let message = WindowLayout {
        reading_pane: ReadingPanePosition::Replace,
        ..WindowLayout::default()
    };
    assert_eq!(
        core.set_window_layout("main", main).await.expect("main"),
        main
    );
    core.set_window_layout("message", message)
        .await
        .expect("message");
    assert_eq!(core.get_window_layout("main").await.expect("main"), main);
    assert_eq!(
        core.get_window_layout("message").await.expect("message"),
        message
    );
    assert_eq!(
        stored(&core, "ui.layout.main").await,
        Some(json!({
            "readingPane": "bottom",
            "density": "compact",
            "sidebarExpanded": false,
            "sidebarWidth": 280
        }))
    );

    // The column's width: 200 to 320 pixels; a refused layout changes nothing.
    for width in [199, 321, 0] {
        let err = core
            .set_window_layout(
                "main",
                WindowLayout {
                    sidebar_width: width,
                    ..main
                },
            )
            .await
            .expect_err("width out of range");
        assert_eq!(err.code(), ErrorCode::InvalidArgument, "{width}");
    }
    assert_eq!(core.get_window_layout("main").await.expect("main"), main);
    // A layout stored before the width existed keeps its other fields; a width out of range
    // reads as the default.
    core.store_for_tests()
        .put_settings(
            GLOBAL_SCOPE,
            vec![
                (
                    "ui.layout.main".to_owned(),
                    r#"{"readingPane":"bottom","density":"compact","sidebarExpanded":false}"#
                        .to_owned(),
                ),
                (
                    "ui.layout.message".to_owned(),
                    r#"{"readingPane":"replace","density":"comfortable","sidebarExpanded":true,"sidebarWidth":900}"#
                        .to_owned(),
                ),
            ],
        )
        .await
        .expect("older layouts");
    assert_eq!(
        core.get_window_layout("main").await.expect("main"),
        WindowLayout {
            sidebar_width: mail_model::DEFAULT_SIDEBAR_WIDTH,
            ..main
        }
    );
    assert_eq!(
        core.get_window_layout("message").await.expect("message"),
        message
    );

    for slot in ["", "Main", "../x", "ui.theme"] {
        let err = core.get_window_layout(slot).await.expect_err("bad slot");
        assert_eq!(err.code(), ErrorCode::InvalidArgument, "{slot:?}");
        let err = core
            .set_window_layout(slot, main)
            .await
            .expect_err("bad slot");
        assert_eq!(err.code(), ErrorCode::InvalidArgument, "{slot:?}");
    }
    // Layouts are no UI preferences and never synchronised (see the capture test).
    assert_eq!(
        core.get_ui_preferences().await.expect("read"),
        UiPreferences::default()
    );
    assert!(RecordKey::setting(SettingScope::Global, "ui.layout.main").is_err());
    assert!(RecordKey::setting(SettingScope::Global, "ui.recent_commands").is_err());
    assert!(RecordKey::setting(SettingScope::Global, "ui.shortcuts.macos").is_ok());
    assert!(RecordKey::setting(SettingScope::Global, "ui.text_size").is_ok());
}

#[tokio::test]
async fn a_message_window_needs_a_message_of_the_account() {
    let dir = TempDir::new().expect("dir");
    let core = core(&dir).await;
    let account = mail_model::AccountId::from_uuid(uuid::Uuid::new_v4());
    let err = core
        .message_window_subject(account, mail_model::MessageId(1))
        .await
        .expect_err("no such message");
    assert_eq!(err.code(), ErrorCode::NotFound);
}
