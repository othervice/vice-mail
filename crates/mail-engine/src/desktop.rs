//! Stage 21 (decisions 0069, 0070): the app in the background, as far as the core is concerned.
//! The shell owns the windows, the tray icon and the system's login items; the core keeps what
//! must survive a restart and decides about notifications.
//!
//! | Key (`setting`, global scope) | Content |
//! |---|---|
//! | `desktop.background` | [`BackgroundSettings`]: keep running when the main window closes, the macOS menu-bar icon |
//! | `desktop.close_hint` | `true` once the note "keeps running in the background" was shown |
//! | `desktop.texts` | [`DesktopTexts`] of the tray menu, in the UI's language of the last start |
//! | `notifications.paused_until` | Unix ms until which notifications pause, or `null` |
//!
//! All four are settings of this device: none is in `settings_sync::SYNCED_SETTINGS`.
//!
//! **Pause.** While paused, [`AppCore::notifications_paused`] is true and the core announces no
//! new mail (neither popup nor notification nor sound), no failed send and no returned snooze,
//! exactly as in quiet hours; nothing is announced later. A pause that has run out reads as
//! none. Every change emits `notificationPauseChanged`.

use jiff::Timestamp;
use jiff::civil::Time;
use jiff::tz::TimeZone;
use mail_model::{
    BackgroundSettings, CoreError, CoreEvent, DesktopTexts, LocalTime, NotificationPause,
    PAUSE_UNTIL_TOMORROW_HOUR, PauseNotifications,
};
use mail_store::GLOBAL_SCOPE;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::core::{AppCore, Connector};

pub(crate) const BACKGROUND_KEY: &str = "desktop.background";
pub(crate) const CLOSE_HINT_KEY: &str = "desktop.close_hint";
pub(crate) const TEXTS_KEY: &str = "desktop.texts";
pub(crate) const PAUSE_KEY: &str = "notifications.paused_until";

/// One hour, for [`PauseNotifications::OneHour`].
const HOUR_MS: i64 = 3_600_000;

/// When a pause chosen at `now_ms` ends: an hour later, or 08:00 local time of the next day
/// (a skipped local time moves forward, as with snoozes); `None` resumes.
pub fn pause_until(choice: PauseNotifications, now_ms: i64, zone: &TimeZone) -> Option<i64> {
    match choice {
        PauseNotifications::Resume => None,
        PauseNotifications::OneHour => Some(now_ms.saturating_add(HOUR_MS)),
        PauseNotifications::UntilTomorrow => {
            let now = Timestamp::from_millisecond(now_ms)
                .ok()?
                .to_zoned_datetime(zone.clone());
            let tomorrow = now.date().tomorrow().ok()?;
            let time = Time::new(PAUSE_UNTIL_TOMORROW_HOUR, 0, 0, 0).ok()?;
            tomorrow
                .to_datetime(time)
                .to_zoned_datetime(zone.clone())
                .ok()
                .map(|z| z.timestamp().as_millisecond())
                // A broken zone must not pause for ever: a day at most.
                .or(Some(now_ms.saturating_add(24 * HOUR_MS)))
        }
    }
}

/// The pause as the UI and the tray menu see it at `now_ms`: none once `until` has passed.
pub fn pause_at(until: Option<i64>, now_ms: i64, zone: &TimeZone) -> NotificationPause {
    let Some(until) = until.filter(|&until| until > now_ms) else {
        return NotificationPause::default();
    };
    let local = |ms| {
        Timestamp::from_millisecond(ms)
            .ok()
            .map(|ts| ts.to_zoned_datetime(zone.clone()))
    };
    let local_time = local(until).zip(local(now_ms)).map(|(end, now)| LocalTime {
        hour: u8::try_from(end.hour()).unwrap_or(0),
        minute: u8::try_from(end.minute()).unwrap_or(0),
        later_day: end.date() > now.date(),
    });
    NotificationPause {
        until: Some(until),
        local_time,
    }
}

fn decode<T: DeserializeOwned + Default>(key: &str, value: Option<&str>) -> T {
    match value.map(serde_json::from_str::<T>) {
        Some(Ok(value)) => value,
        Some(Err(err)) => {
            tracing::warn!(key, %err, "stored desktop setting unreadable, using the default");
            T::default()
        }
        None => T::default(),
    }
}

fn encode<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value)
        .map_err(|err| CoreError::Internal(format!("encode desktop setting: {err}")))
}

impl<C: Connector> AppCore<C> {
    async fn desktop_value<T: DeserializeOwned + Default>(
        &self,
        key: &str,
    ) -> Result<T, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(decode(
            key,
            settings
                .iter()
                .find(|s| s.key == key)
                .map(|s| s.value.as_str()),
        ))
    }

    async fn put_desktop_value<T: Serialize>(&self, key: &str, value: &T) -> Result<(), CoreError> {
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(key.to_owned(), encode(value)?)])
            .await?;
        Ok(())
    }

    /// Stage 21: what closing the main window does; the defaults when nothing is stored.
    pub async fn get_background_settings(&self) -> Result<BackgroundSettings, CoreError> {
        self.desktop_value(BACKGROUND_KEY).await
    }

    /// Stores the background settings of this device and returns them.
    pub async fn set_background_settings(
        &self,
        settings: BackgroundSettings,
    ) -> Result<BackgroundSettings, CoreError> {
        self.put_desktop_value(BACKGROUND_KEY, &settings).await?;
        tracing::info!(
            keep_running = settings.keep_running,
            menu_bar_icon = settings.menu_bar_icon,
            "background settings stored"
        );
        Ok(settings)
    }

    /// Whether the one-time note about running in the background was shown on this device.
    pub async fn close_hint_shown(&self) -> Result<bool, CoreError> {
        self.desktop_value(CLOSE_HINT_KEY).await
    }

    /// Remembers that the note was shown (it is shown once per device).
    pub async fn set_close_hint_shown(&self) -> Result<(), CoreError> {
        self.put_desktop_value(CLOSE_HINT_KEY, &true).await
    }

    /// The tray menu's texts of the last UI language (English before the UI ever sent any).
    pub async fn desktop_texts(&self) -> Result<DesktopTexts, CoreError> {
        self.desktop_value(TEXTS_KEY).await
    }

    /// Keeps the tray menu's texts in the UI's language for the next start in the background.
    pub async fn set_desktop_texts(&self, texts: DesktopTexts) -> Result<DesktopTexts, CoreError> {
        self.put_desktop_value(TEXTS_KEY, &texts).await?;
        Ok(texts)
    }

    /// Whether notifications are paused now, and until when.
    pub async fn notification_pause(&self) -> Result<NotificationPause, CoreError> {
        let until: Option<i64> = self.desktop_value(PAUSE_KEY).await?;
        Ok(pause_at(until, self.now_ms(), &self.comfort().zone()))
    }

    /// Pauses notifications for an hour or until tomorrow morning, or resumes them; emits
    /// `notificationPauseChanged` and returns the pause as it is now.
    pub async fn pause_notifications(
        &self,
        choice: PauseNotifications,
    ) -> Result<NotificationPause, CoreError> {
        let now = self.now_ms();
        let zone = self.comfort().zone();
        let until = pause_until(choice, now, &zone);
        self.put_desktop_value(PAUSE_KEY, &until).await?;
        let pause = pause_at(until, now, &zone);
        tracing::info!(?choice, until = ?pause.until, "notification pause changed");
        self.emit_event(CoreEvent::NotificationPauseChanged { pause });
        Ok(pause)
    }

    /// "New message" from the tray menu: asks the UI for an empty composer (`composeRequested`,
    /// like a `mailto:` link without fields); the UI opens it from the selected account.
    pub fn compose_new(&self) {
        tracing::info!("new message requested from the tray");
        self.emit_event(CoreEvent::ComposeRequested {
            request: mail_model::ComposeRequest::default(),
        });
    }

    /// Whether notifications are paused at this moment (an unreadable setting counts as not).
    pub(crate) async fn notifications_paused(&self) -> bool {
        match self.desktop_value::<Option<i64>>(PAUSE_KEY).await {
            Ok(Some(until)) => until > self.now_ms(),
            Ok(None) => false,
            Err(err) => {
                tracing::debug!(%err, "notification pause unreadable");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use jiff::tz::TimeZone;
    use mail_model::{
        BackgroundSettings, CoreEvent, DesktopTexts, LocalTime, NotificationPause,
        PauseNotifications,
    };
    use mail_store::GLOBAL_SCOPE;
    use tempfile::TempDir;

    use super::*;
    use crate::clock::Clock;
    use crate::credentials::{Credentials, MemorySecretStore};
    use crate::settings_sync::SYNCED_SETTINGS;
    use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

    /// 2026-11-25 13:20:00 UTC.
    const NOW: i64 = 1_795_564_800_000 + 13 * 3_600_000 + 20 * 60_000;

    #[derive(Debug)]
    struct ManualClock(AtomicI64);

    impl Clock for ManualClock {
        fn now_ms(&self) -> i64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    async fn core(dir: &TempDir, clock: Arc<ManualClock>) -> AppCore {
        let mut services = CoreServices::new(Credentials::new(MemorySecretStore::new()));
        services.clock = clock;
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
        .await;
        assert!(core.startup_error().is_none());
        core
    }

    #[test]
    fn a_pause_ends_after_an_hour_or_tomorrow_at_eight() {
        let utc = TimeZone::UTC;
        assert_eq!(
            pause_until(PauseNotifications::OneHour, NOW, &utc),
            Some(NOW + 3_600_000)
        );
        assert_eq!(pause_until(PauseNotifications::Resume, NOW, &utc), None);
        // 2026-11-26 08:00 UTC.
        let tomorrow = 1_795_564_800_000 + 24 * 3_600_000 + 8 * 3_600_000;
        assert_eq!(
            pause_until(PauseNotifications::UntilTomorrow, NOW, &utc),
            Some(tomorrow)
        );
        // In Berlin (UTC+1 in November) 13:20 UTC is 14:20; tomorrow 08:00 is 07:00 UTC.
        let berlin = TimeZone::get("Europe/Berlin").expect("tzdb");
        assert_eq!(
            pause_until(PauseNotifications::UntilTomorrow, NOW, &berlin),
            Some(tomorrow - 3_600_000)
        );
        // Shortly after midnight "tomorrow" is still the next calendar day.
        let after_midnight = 1_795_564_800_000 + 60_000;
        assert_eq!(
            pause_until(PauseNotifications::UntilTomorrow, after_midnight, &utc),
            Some(1_795_564_800_000 + 32 * 3_600_000)
        );
    }

    #[test]
    fn the_pause_tells_its_local_end_and_runs_out() {
        let utc = TimeZone::UTC;
        assert_eq!(
            pause_at(Some(NOW + 3_600_000), NOW, &utc),
            NotificationPause {
                until: Some(NOW + 3_600_000),
                local_time: Some(LocalTime {
                    hour: 14,
                    minute: 20,
                    later_day: false
                }),
            }
        );
        let tomorrow = pause_until(PauseNotifications::UntilTomorrow, NOW, &utc);
        assert_eq!(
            pause_at(tomorrow, NOW, &utc).local_time,
            Some(LocalTime {
                hour: 8,
                minute: 0,
                later_day: true
            })
        );
        assert_eq!(pause_at(Some(NOW), NOW, &utc), NotificationPause::default());
        assert_eq!(pause_at(None, NOW, &utc), NotificationPause::default());
    }

    #[tokio::test]
    async fn desktop_settings_are_stored_on_this_device_only() {
        let dir = TempDir::new().expect("temp dir");
        let clock = Arc::new(ManualClock(AtomicI64::new(NOW)));
        let core = core(&dir, Arc::clone(&clock)).await;
        core.set_time_zone(TimeZone::UTC);

        // Defaults.
        assert_eq!(
            core.get_background_settings().await.expect("get"),
            BackgroundSettings::default()
        );
        assert!(!core.close_hint_shown().await.expect("hint"));
        assert_eq!(
            core.desktop_texts().await.expect("texts"),
            DesktopTexts::default()
        );
        assert_eq!(
            core.notification_pause().await.expect("pause"),
            NotificationPause::default()
        );

        // Stored and read back.
        let off = BackgroundSettings {
            keep_running: false,
            menu_bar_icon: false,
        };
        assert_eq!(core.set_background_settings(off).await.expect("set"), off);
        assert_eq!(core.get_background_settings().await.expect("get"), off);
        core.set_close_hint_shown().await.expect("hint");
        assert!(core.close_hint_shown().await.expect("hint"));
        let german = DesktopTexts {
            quit: "{product} beenden".into(),
            hour12: false,
            ..DesktopTexts::default()
        };
        core.set_desktop_texts(german.clone()).await.expect("texts");
        assert_eq!(core.desktop_texts().await.expect("texts"), german);

        // Pause, event, expiry, resume.
        let mut events = core.subscribe();
        let pause = core
            .pause_notifications(PauseNotifications::OneHour)
            .await
            .expect("pause");
        assert_eq!(pause.until, Some(NOW + 3_600_000));
        assert!(core.notifications_paused().await);
        assert!(matches!(
            events.recv().await,
            Ok(CoreEvent::NotificationPauseChanged { pause: p }) if p == pause
        ));
        clock.0.store(NOW + 3_600_000, Ordering::SeqCst);
        assert!(!core.notifications_paused().await, "the hour is over");
        assert_eq!(
            core.notification_pause().await.expect("pause"),
            NotificationPause::default()
        );
        core.pause_notifications(PauseNotifications::UntilTomorrow)
            .await
            .expect("pause");
        assert!(core.notifications_paused().await);
        let resumed = core
            .pause_notifications(PauseNotifications::Resume)
            .await
            .expect("resume");
        assert_eq!(resumed, NotificationPause::default());
        assert!(!core.notifications_paused().await);

        // "New message" from the tray: an empty composer request.
        while events.try_recv().is_ok() {}
        core.compose_new();
        assert!(matches!(
            events.recv().await,
            Ok(CoreEvent::ComposeRequested { request }) if request == mail_model::ComposeRequest::default()
        ));

        // None of it travels with the settings sync.
        let keys: Vec<String> = core
            .store_ref()
            .expect("store")
            .settings(GLOBAL_SCOPE)
            .await
            .expect("settings")
            .into_iter()
            .map(|s| s.key)
            .collect();
        for key in [BACKGROUND_KEY, CLOSE_HINT_KEY, TEXTS_KEY, PAUSE_KEY] {
            assert!(keys.iter().any(|k| k == key), "{key} stored");
            assert!(!SYNCED_SETTINGS.contains(&key), "{key} is a device setting");
        }
    }
}
