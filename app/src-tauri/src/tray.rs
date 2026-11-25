//! Stage 21 (decision 0069): the icon in the notification area (Windows) or the menu bar
//! (macOS), with the unread count and a menu: open the app, a new message, pause notifications,
//! quit.
//!
//! - **Windows:** always there while the app runs (the way back to a closed window). The icon is
//!   the app icon's own entry for the small-icon size of the primary display (16/20/24/32 px,
//!   `Image::from_app_icon_resource`) with the count in a red pill drawn for that size
//!   ([`crate::badge::on_icon`]); the tooltip names the count. A left click opens the window, a
//!   right click the menu.
//! - **macOS:** the mono glyph as a template image (the system colours it for light and dark
//!   menu bars), the count as the item's title next to it; a click opens the menu. Shown unless
//!   the user switched it off ([`mail_model::BackgroundSettings::menu_bar_icon`]); the Dock icon
//!   stays either way.
//! - The texts come from the UI (`setDesktopTexts`), kept by the core for a start in the
//!   background. The menu is rebuilt on every change: count, pause, texts, settings.

use mail_engine::AppCore;
use mail_model::{DesktopTexts, NotificationPause, PauseNotifications};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

/// Id of the app's one tray icon.
pub const TRAY_ID: &str = "main";

/// Ids of the menu's entries.
pub mod ids {
    pub const OPEN: &str = "open";
    pub const COMPOSE: &str = "compose";
    pub const PAUSE: &str = "pause";
    pub const PAUSE_HOUR: &str = "pause-hour";
    pub const PAUSE_TOMORROW: &str = "pause-tomorrow";
    pub const PAUSED: &str = "paused";
    pub const RESUME: &str = "resume";
    pub const QUIT: &str = "quit";
}

/// One entry of the menu.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Entry {
    Item {
        id: &'static str,
        text: String,
        enabled: bool,
    },
    Submenu {
        id: &'static str,
        text: String,
        items: Vec<Entry>,
    },
    Separator,
}

/// What the tray shows: tooltip, the title next to the icon (macOS), the count drawn into the
/// icon (Windows) and the menu.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Model {
    pub tooltip: String,
    pub title: Option<String>,
    pub count: u32,
    pub menu: Vec<Entry>,
}

fn fill(text: &str, product: &str) -> String {
    text.replace("{product}", product)
}

/// The tray for `unread` messages (the counter's number) and the pause as it is.
pub fn model(texts: &DesktopTexts, product: &str, unread: u32, pause: &NotificationPause) -> Model {
    let item = |id, text: &str, enabled| Entry::Item {
        id,
        text: fill(text, product),
        enabled,
    };
    let mut pause_items = vec![
        item(ids::PAUSE_HOUR, &texts.pause_hour, true),
        item(ids::PAUSE_TOMORROW, &texts.pause_tomorrow, true),
        Entry::Separator,
    ];
    if let Some(time) = pause.local_time.filter(|_| pause.until.is_some()) {
        let pattern = if time.later_day {
            &texts.paused_until_tomorrow
        } else {
            &texts.paused_until
        };
        pause_items.push(Entry::Item {
            id: ids::PAUSED,
            text: pattern.replace("{time}", &time.format(texts.hour12)),
            enabled: false,
        });
    }
    pause_items.push(item(ids::RESUME, &texts.resume, pause.until.is_some()));
    let tooltip = if unread == 0 {
        fill(&texts.tooltip, product)
    } else {
        fill(&texts.tooltip_unread, product).replace("{count}", &unread.to_string())
    };
    Model {
        tooltip,
        title: (unread > 0).then(|| unread.to_string()),
        count: unread,
        menu: vec![
            item(ids::OPEN, &texts.open, true),
            item(ids::COMPOSE, &texts.new_message, true),
            Entry::Separator,
            Entry::Submenu {
                id: ids::PAUSE,
                text: fill(&texts.pause, product),
                items: pause_items,
            },
            Entry::Separator,
            item(ids::QUIT, &texts.quit, true),
        ],
    }
}

/// What a menu entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Compose,
    Pause(PauseNotifications),
    Quit,
}

/// The action of the entry with `id`; `None` for the status line and unknown ids.
pub fn action(id: &str) -> Option<Action> {
    Some(match id {
        ids::OPEN => Action::Open,
        ids::COMPOSE => Action::Compose,
        ids::PAUSE_HOUR => Action::Pause(PauseNotifications::OneHour),
        ids::PAUSE_TOMORROW => Action::Pause(PauseNotifications::UntilTomorrow),
        ids::RESUME => Action::Pause(PauseNotifications::Resume),
        ids::QUIT => Action::Quit,
        _ => return None,
    })
}

/// Runs a menu action (also the desktop test's way to "click" an entry).
pub fn perform<R: Runtime>(app: &AppHandle<R>, action: Action) {
    match action {
        Action::Open => crate::lifecycle::show_main_window(app),
        Action::Compose => {
            crate::lifecycle::show_main_window(app);
            if let Some(core) = app.try_state::<AppCore>() {
                core.compose_new();
            }
        }
        Action::Pause(choice) => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(core) = app.try_state::<AppCore>()
                    && let Err(err) = core.pause_notifications(choice).await
                {
                    tracing::warn!(%err, "notifications not paused");
                }
                // The core's event refreshes the tray as well; this covers a lost event.
                refresh(&app);
            });
        }
        Action::Quit => crate::lifecycle::quit(app),
    }
}

/// Whether this platform shows the tray icon with these settings.
pub fn shown(menu_bar_icon: bool) -> bool {
    cfg!(windows) || (cfg!(target_os = "macos") && menu_bar_icon)
}

fn product<R: Runtime>(app: &AppHandle<R>) -> String {
    app.config()
        .product_name
        .clone()
        .unwrap_or_else(|| app.package_info().name.clone())
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, entries: &[Entry]) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        append(app, entry, &|item| menu.append(item))?;
    }
    Ok(menu)
}

fn append<R: Runtime>(
    app: &AppHandle<R>,
    entry: &Entry,
    add: &dyn Fn(&dyn IsMenuItem<R>) -> tauri::Result<()>,
) -> tauri::Result<()> {
    match entry {
        Entry::Item { id, text, enabled } => {
            add(&MenuItem::with_id(app, *id, text, *enabled, None::<&str>)?)
        }
        Entry::Separator => add(&PredefinedMenuItem::separator(app)?),
        Entry::Submenu { id, text, items } => {
            let submenu = Submenu::with_id(app, *id, text, true)?;
            for item in items {
                append(app, item, &|i| submenu.append(i))?;
            }
            add(&submenu)
        }
    }
}

/// The icon for `count` unread messages.
#[cfg(target_os = "macos")]
fn icon<R: Runtime>(_app: &AppHandle<R>, _count: u32) -> Option<tauri::image::Image<'static>> {
    // Generated by scripts/icons.sh from docs/brand/glyph-mono.svg: 36 px for 18 pt.
    Some(tauri::include_image!("icons/tray/macos-template.png"))
}

#[cfg(windows)]
fn icon<R: Runtime>(app: &AppHandle<R>, count: u32) -> Option<tauri::image::Image<'static>> {
    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map_or(1.0, |m| m.scale_factor());
    let size = crate::badge::tray_size(scale);
    let base = match tauri::image::Image::from_app_icon_resource(size) {
        Ok(image) => image,
        Err(err) => {
            tracing::warn!(%err, size, "no app icon of the tray's size");
            return app.default_window_icon().cloned().map(|i| i.to_owned());
        }
    };
    let drawn = crate::badge::on_icon(
        &crate::badge::Rgba {
            width: base.width(),
            height: base.height(),
            pixels: base.rgba().to_vec(),
        },
        count,
    );
    Some(tauri::image::Image::new_owned(
        drawn.pixels,
        drawn.width,
        drawn.height,
    ))
}

#[cfg(not(any(target_os = "macos", windows)))]
fn icon<R: Runtime>(app: &AppHandle<R>, _count: u32) -> Option<tauri::image::Image<'static>> {
    app.default_window_icon().cloned().map(|i| i.to_owned())
}

/// Applies `model` to the tray icon, building it first if needed.
fn apply<R: Runtime>(app: &AppHandle<R>, model: &Model, visible: bool) -> tauri::Result<()> {
    let existing = app.tray_by_id(TRAY_ID);
    if !visible {
        if let Some(tray) = existing {
            tray.set_visible(false)?;
        }
        return Ok(());
    }
    let menu = build_menu(app, &model.menu)?;
    let tray = match existing {
        Some(tray) => {
            tray.set_menu(Some(menu))?;
            if let Some(image) = icon(app, model.count) {
                tray.set_icon_with_as_template(Some(image), cfg!(target_os = "macos"))?;
            }
            tray.set_visible(true)?;
            tray
        }
        None => {
            let mut builder = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&menu)
                .icon_as_template(cfg!(target_os = "macos"))
                // Windows: a left click opens the window, the menu is on the right button.
                .show_menu_on_left_click(!cfg!(windows))
                .on_menu_event(|app, event| {
                    if let Some(action) = action(event.id().as_ref()) {
                        perform(app, action);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if cfg!(windows)
                        && let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                    {
                        crate::lifecycle::show_main_window(tray.app_handle());
                    }
                });
            if let Some(image) = icon(app, model.count) {
                builder = builder.icon(image);
            }
            builder.build(app)?
        }
    };
    tray.set_tooltip(Some(&model.tooltip))?;
    // macOS: the count beside the glyph (the system's font, always sharp).
    if cfg!(target_os = "macos") {
        tray.set_title(model.title.as_deref())?;
    }
    Ok(())
}

/// Reads count, pause, texts and settings and updates the tray (off the caller's thread).
pub fn refresh<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(core) = app.try_state::<AppCore>() else {
            return;
        };
        let texts = core.desktop_texts().await.unwrap_or_default();
        let pause = core.notification_pause().await.unwrap_or_default();
        let count = crate::notifications::current_count(&app);
        let settings = app
            .try_state::<crate::lifecycle::Lifecycle>()
            .map(|l| l.settings())
            .unwrap_or_default();
        let model = model(&texts, &product(&app), count, &pause);
        let visible = shown(settings.menu_bar_icon);
        // Menus are built on the main thread.
        let handle = app.clone();
        if let Err(err) = app.run_on_main_thread(move || {
            if let Err(err) = apply(&handle, &model, visible) {
                tracing::warn!(%err, "tray icon not updated");
            }
        }) {
            tracing::warn!(%err, "tray icon not updated");
        }
        // A pause that runs out changes the menu without an event.
        if let Some(until) = pause.until {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
            let wait = until.saturating_sub(now).max(0);
            let later = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(
                    u64::try_from(wait).unwrap_or(0) + 1_000,
                ))
                .await;
                refresh(&later);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use mail_model::LocalTime;

    use super::*;

    fn ids_of(entries: &[Entry]) -> Vec<&'static str> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Item { id, .. } | Entry::Submenu { id, .. } => Some(*id),
                Entry::Separator => None,
            })
            .collect()
    }

    fn pause_items(model: &Model) -> &[Entry] {
        model
            .menu
            .iter()
            .find_map(|e| match e {
                Entry::Submenu { items, .. } => Some(items.as_slice()),
                _ => None,
            })
            .expect("pause submenu")
    }

    #[test]
    fn the_menu_opens_writes_pauses_and_quits() {
        let m = model(
            &DesktopTexts::default(),
            "Vice Mail",
            0,
            &NotificationPause::default(),
        );
        assert_eq!(
            ids_of(&m.menu),
            [ids::OPEN, ids::COMPOSE, ids::PAUSE, ids::QUIT]
        );
        assert_eq!(
            m.menu[0],
            Entry::Item {
                id: ids::OPEN,
                text: "Open Vice Mail".into(),
                enabled: true
            }
        );
        assert_eq!(
            m.menu.last(),
            Some(&Entry::Item {
                id: ids::QUIT,
                text: "Quit Vice Mail".into(),
                enabled: true
            })
        );
        // Not paused: no status line, "Resume" cannot be chosen.
        assert_eq!(
            ids_of(pause_items(&m)),
            [ids::PAUSE_HOUR, ids::PAUSE_TOMORROW, ids::RESUME]
        );
        assert!(pause_items(&m).contains(&Entry::Item {
            id: ids::RESUME,
            text: "Resume Notifications".into(),
            enabled: false
        }));
        assert_eq!(m.tooltip, "Vice Mail");
        assert_eq!(m.title, None);
    }

    #[test]
    fn the_count_shows_in_tooltip_and_title() {
        let m = model(
            &DesktopTexts::default(),
            "Vice Mail",
            12,
            &NotificationPause::default(),
        );
        assert_eq!(m.tooltip, "Vice Mail: 12 unread");
        assert_eq!(m.title.as_deref(), Some("12"));
        assert_eq!(m.count, 12);
    }

    #[test]
    fn a_pause_says_until_when_and_can_be_resumed() {
        let pause = |hour, later_day| NotificationPause {
            until: Some(1),
            local_time: Some(LocalTime {
                hour,
                minute: 30,
                later_day,
            }),
        };
        let m = model(&DesktopTexts::default(), "Vice Mail", 0, &pause(14, false));
        let items = pause_items(&m);
        assert!(items.contains(&Entry::Item {
            id: ids::PAUSED,
            text: "Paused until 14:30".into(),
            enabled: false
        }));
        assert!(items.contains(&Entry::Item {
            id: ids::RESUME,
            text: "Resume Notifications".into(),
            enabled: true
        }));
        // German, 24-hour clock, tomorrow.
        let german = DesktopTexts {
            paused_until_tomorrow: "Pausiert bis morgen, {time}".into(),
            quit: "{product} beenden".into(),
            ..DesktopTexts::default()
        };
        let m = model(&german, "Vice Mail", 0, &pause(8, true));
        assert!(pause_items(&m).contains(&Entry::Item {
            id: ids::PAUSED,
            text: "Pausiert bis morgen, 08:30".into(),
            enabled: false
        }));
        assert!(m.menu.contains(&Entry::Item {
            id: ids::QUIT,
            text: "Vice Mail beenden".into(),
            enabled: true
        }));
        // English with a 12-hour clock.
        let english = DesktopTexts {
            hour12: true,
            ..DesktopTexts::default()
        };
        let m = model(&english, "Vice Mail", 0, &pause(14, false));
        assert!(pause_items(&m).iter().any(|e| matches!(
            e,
            Entry::Item { id: ids::PAUSED, text, .. } if text == "Paused until 2:30 PM"
        )));
    }

    #[test]
    fn every_entry_that_can_be_chosen_has_an_action() {
        let m = model(
            &DesktopTexts::default(),
            "Vice Mail",
            3,
            &NotificationPause {
                until: Some(1),
                local_time: None,
            },
        );
        let mut all = ids_of(&m.menu);
        all.extend(ids_of(pause_items(&m)));
        for id in all {
            if id == ids::PAUSE {
                continue;
            }
            assert!(action(id).is_some(), "{id}");
        }
        assert_eq!(action(ids::PAUSED), None);
        assert_eq!(
            action(ids::PAUSE_TOMORROW),
            Some(Action::Pause(PauseNotifications::UntilTomorrow))
        );
        assert_eq!(action("nonsense"), None);
    }

    #[test]
    fn windows_always_shows_the_icon_macos_by_choice() {
        if cfg!(windows) {
            assert!(shown(false));
        } else if cfg!(target_os = "macos") {
            assert!(shown(true));
            assert!(!shown(false));
        }
    }
}
