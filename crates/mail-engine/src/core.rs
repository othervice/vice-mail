use std::path::PathBuf;
use std::sync::Arc;

use mail_model::{AppInfo, CoreError, CoreEvent, Platform, Preferences};
use mail_store::{GLOBAL_SCOPE, Store, StoreOptions};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::sync::broadcast;

/// File name of the database inside the app's data directory.
pub const DATABASE_FILE: &str = "mail.sqlite3";

const THEME_KEY: &str = "ui.theme";
const LANGUAGE_KEY: &str = "ui.language";

/// Events buffered per subscriber before a slow subscriber starts missing events.
const EVENT_CAPACITY: usize = 256;

/// Facts about the build, supplied by the shell (it owns the package metadata and build script).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: String,
    pub commit: Option<String>,
    pub build_date: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Directory for the database and its backups (the app's data directory).
    pub data_dir: PathBuf,
    pub build: BuildInfo,
}

/// Facade of the core. The shell forwards every command here; everything is testable without
/// a WebView. Cheap to clone.
#[derive(Clone)]
pub struct AppCore {
    inner: Arc<Inner>,
}

struct Inner {
    info: AppInfo,
    /// The store, or the error that kept it from opening (for example a database written by a
    /// newer app version). Storage-backed calls return that error, so the UI can explain it.
    store: Result<Store, CoreError>,
    events: broadcast::Sender<CoreEvent>,
}

impl AppCore {
    /// Opens the database in `config.data_dir` and starts the core.
    ///
    /// Never fails: if the database cannot be opened, the core still starts, logs the error,
    /// reports it through [`AppCore::startup_error`] and returns it from every call that needs
    /// storage. That way the UI can show a clear message instead of the app vanishing.
    pub async fn start(config: CoreConfig) -> Self {
        let path = config.data_dir.join(DATABASE_FILE);
        let opened =
            tokio::task::spawn_blocking(move || Store::open(&path, StoreOptions::default()))
                .await
                .map_err(|err| CoreError::Internal(format!("database open task failed: {err}")))
                .and_then(|result| result.map_err(CoreError::from));

        match &opened {
            Ok(store) => {
                let report = store.open_report();
                tracing::info!(
                    schema_version = report.version,
                    migrated_from = report.previous_version,
                    backup = report.backup.as_ref().map(|p| p.display().to_string()),
                    "database ready"
                );
            }
            Err(err) => {
                tracing::error!(code = ?err.code(), error = %err, "database could not be opened");
            }
        }

        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                info: AppInfo {
                    version: config.build.version,
                    commit: config.build.commit,
                    build_date: config.build.build_date,
                    platform: Platform::current(),
                },
                store: opened,
                events,
            }),
        }
    }

    pub fn app_info(&self) -> AppInfo {
        self.inner.info.clone()
    }

    /// The error that kept the database from opening, if any.
    pub fn startup_error(&self) -> Option<&CoreError> {
        self.inner.store.as_ref().err()
    }

    /// Subscribes to the event stream. Events sent before subscribing are not replayed.
    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.inner.events.subscribe()
    }

    pub async fn get_preferences(&self) -> Result<Preferences, CoreError> {
        let settings = self.store()?.settings(GLOBAL_SCOPE).await?;
        let mut prefs = Preferences::default();
        for setting in settings {
            match setting.key.as_str() {
                THEME_KEY => prefs.theme = decode(&setting.key, &setting.value),
                LANGUAGE_KEY => prefs.language = decode(&setting.key, &setting.value),
                _ => {}
            }
        }
        Ok(prefs)
    }

    /// Stores `preferences`, then emits [`CoreEvent::PreferencesChanged`] and returns them.
    pub async fn set_preferences(
        &self,
        preferences: Preferences,
    ) -> Result<Preferences, CoreError> {
        let entries = vec![
            (THEME_KEY.to_owned(), encode(&preferences.theme)?),
            (LANGUAGE_KEY.to_owned(), encode(&preferences.language)?),
        ];
        self.store()?.put_settings(GLOBAL_SCOPE, entries).await?;
        tracing::info!(theme = ?preferences.theme, language = ?preferences.language, "preferences stored");
        self.emit(CoreEvent::PreferencesChanged { preferences });
        Ok(preferences)
    }

    fn store(&self) -> Result<&Store, CoreError> {
        self.inner.store.as_ref().map_err(Clone::clone)
    }

    fn emit(&self, event: CoreEvent) {
        // An error only means nobody is subscribed right now; the state is already stored.
        if self.inner.events.send(event).is_err() {
            tracing::debug!("core event dropped: no subscribers");
        }
    }
}

fn encode<T: Serialize>(value: &T) -> Result<String, CoreError> {
    serde_json::to_string(value)
        .map_err(|err| CoreError::Internal(format!("encode setting: {err}")))
}

/// A stored value this version does not understand falls back to the default, so the app still
/// starts; the problem is logged.
fn decode<T: DeserializeOwned + Default>(key: &str, json: &str) -> T {
    serde_json::from_str(json).unwrap_or_else(|err| {
        tracing::warn!(key, %err, "stored setting has an unknown value, using the default");
        T::default()
    })
}
