//! In-app updates (stage 15, decision 0037): the device setting "check automatically", the
//! one-time question, the daily throttle and the state machine of check, download and install.
//!
//! The network side lives behind [`UpdateSource`]; the shell implements it with
//! `tauri-plugin-updater` (signature check, platform installers) and hands it over with
//! [`AppCore::start_updates`]. This module decides *whether* the source is asked at all:
//!
//! - automatically only while the user's switch is on, and at most once per
//!   [`UPDATE_CHECK_INTERVAL_MS`] (the time of every attempt is stored, so a failing server is
//!   not asked every hour either);
//! - manually whenever the user clicks "Check now";
//! - downloading and installing only on the user's click, one step at a time.
//!
//! Without a source (tests, builds without the updater) nothing is ever requested and the UI
//! shows only the version.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use mail_model::{
    AvailableUpdate, CoreError, CoreEvent, IpcError, MAX_UPDATE_NOTES_CHARS, UpdateError,
    UpdateInfo, UpdateSettings, UpdateStatus,
};
use mail_store::GLOBAL_SCOPE;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::AppCore;
use crate::core::Connector;

pub mod http;

/// Key of the device setting (not in `SYNCED_SETTINGS`, so it never leaves the device).
const SETTINGS_KEY: &str = "updates";

/// A step of an [`UpdateSource`].
pub type UpdateFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, UpdateError>> + Send + 'a>>;

/// Download progress: bytes so far, total if the server said.
pub type DownloadProgress = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

/// The network side of updates, implemented by the shell.
pub trait UpdateSource: Send + Sync + 'static {
    /// Fetches the update manifest. `None`: no newer version.
    fn check(&self) -> UpdateFuture<'_, Option<AvailableUpdate>>;
    /// Downloads the update found by the last check and verifies its signature; keeps the
    /// verified bytes for [`UpdateSource::install`].
    fn download(&self, progress: DownloadProgress) -> UpdateFuture<'_, ()>;
    /// Installs the downloaded update. Blocking; runs on a blocking thread. In the product it
    /// returns `Ok` only once the restart is under way (macOS) or does not return at all (the
    /// Windows installer takes over and the app exits).
    fn install(&self) -> Result<(), UpdateError>;
}

/// When the background task looks at the switch and the throttle.
#[derive(Debug, Clone, Copy)]
pub struct UpdateSchedule {
    /// After the start, before the first look (the start's own work goes first).
    pub first_delay: Duration,
    /// Between two looks. Whether a look reaches the network decides
    /// [`UpdateSettings::automatic_check_due`].
    pub interval: Duration,
}

impl Default for UpdateSchedule {
    fn default() -> Self {
        Self {
            first_delay: Duration::from_secs(60),
            interval: Duration::from_secs(60 * 60),
        }
    }
}

/// The core's update state (one per core).
#[derive(Default)]
pub(crate) struct UpdatesState {
    source: OnceLock<Arc<dyn UpdateSource>>,
    status: Mutex<Option<UpdateStatus>>,
    /// One step (check, download, install) at a time.
    step: Arc<tokio::sync::Mutex<()>>,
    /// Serialises read-modify-write of the stored setting (the switch and the check time).
    settings: tokio::sync::Mutex<()>,
    /// Wakes the background task (the switch was turned on).
    wake: Arc<Notify>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl UpdatesState {
    fn status(&self) -> UpdateStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(UpdateStatus::Idle)
    }

    fn source(&self) -> Option<Arc<dyn UpdateSource>> {
        self.source.get().cloned()
    }

    /// Ends the background task (the core shuts down).
    pub(crate) fn stop(&self) {
        if let Some(task) = self
            .task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

fn not_supported() -> CoreError {
    UpdateError::NotReady("this build does not update itself".into()).into()
}

/// Cuts release notes to [`MAX_UPDATE_NOTES_CHARS`] and drops control characters other than
/// line breaks and tabs, so the plain-text view gets plain text.
fn clean_notes(notes: Option<String>) -> Option<String> {
    let notes = notes?;
    let cleaned: String = notes
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(MAX_UPDATE_NOTES_CHARS)
        .collect();
    let trimmed = cleaned.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

impl<C: Connector> AppCore<C> {
    /// The shell hands over its update source and starts the background task (on the runtime
    /// that polls this call). Only the first call counts.
    pub async fn start_updates(&self, source: Arc<dyn UpdateSource>, schedule: UpdateSchedule) {
        let state = self.updates();
        if state.source.set(source).is_err() {
            tracing::warn!("update source set twice; the first one stays");
            return;
        }
        let weak = self.downgrade();
        let wake = Arc::clone(&state.wake);
        let task = tokio::spawn(async move {
            tokio::select! {
                () = tokio::time::sleep(schedule.first_delay) => {}
                () = wake.notified() => {}
            }
            loop {
                let Some(core) = weak.upgrade() else { return };
                core.automatic_update_check().await;
                drop(core);
                tokio::select! {
                    () = tokio::time::sleep(schedule.interval) => {}
                    () = wake.notified() => {}
                }
            }
        });
        *state.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
    }

    /// Setting, question and state for Settings → About.
    pub async fn get_update_info(&self) -> Result<UpdateInfo, CoreError> {
        let settings = self.update_settings().await?;
        // The question waits for the first account: a person still trying the app is not asked
        // before they have set anything up (decision 0037).
        let has_account = !self.store_ref()?.accounts().await?.is_empty();
        let supported = self.updates().source().is_some();
        Ok(UpdateInfo {
            current_version: self.app_info().version,
            supported,
            auto_check: settings.auto_check,
            ask_consent: supported && has_account && !settings.consent_answered,
            last_check_ms: settings.last_check_ms,
            status: self.updates().status(),
        })
    }

    /// The switch "Check for updates automatically", also the answer to the one-time question
    /// (either answer marks it as answered). Turning it on lets the background task look at
    /// once; whether that reaches the network still follows the daily throttle.
    pub async fn set_update_auto_check(&self, enabled: bool) -> Result<UpdateInfo, CoreError> {
        self.modify_update_settings(|settings| {
            settings.auto_check = enabled;
            settings.consent_answered = true;
        })
        .await?;
        tracing::info!(enabled, "automatic update check set");
        if enabled {
            self.updates().wake.notify_one();
        }
        self.get_update_info().await
    }

    /// "Check now": asks the update server whatever the switch says. Failures of the check
    /// itself show up in `status` (`failed`), not as an error of this call.
    pub async fn check_for_updates(&self) -> Result<UpdateInfo, CoreError> {
        let source = self.updates().source().ok_or_else(not_supported)?;
        self.run_check(source).await?;
        self.get_update_info().await
    }

    /// Starts downloading the update the last check found and returns at once (`downloading`);
    /// `updateStatusChanged` reports the progress and then `ready` or `failed`.
    pub async fn download_update(&self) -> Result<UpdateInfo, CoreError> {
        let source = self.updates().source().ok_or_else(not_supported)?;
        let version = match self.updates().status() {
            UpdateStatus::Available { update } => update.version,
            // A failed download (or signature check) of a found update may be retried.
            UpdateStatus::Failed {
                version: Some(version),
                ..
            } => version,
            _ => {
                return Err(
                    UpdateError::NotReady("no update was found; check first".into()).into(),
                );
            }
        };
        let Ok(step) = Arc::clone(&self.updates().step).try_lock_owned() else {
            return Err(UpdateError::NotReady("another update step is running".into()).into());
        };
        self.set_update_status(UpdateStatus::Downloading {
            version: version.clone(),
            downloaded: 0,
            total: None,
        });
        let weak = self.downgrade();
        tokio::spawn(async move {
            let _step = step;
            let Some(core) = weak.upgrade() else { return };
            let progress = core.download_progress(version.clone());
            let outcome = source.download(progress).await;
            match outcome {
                Ok(()) => {
                    tracing::info!(%version, "update downloaded and verified");
                    core.set_update_status(UpdateStatus::Ready { version });
                }
                Err(err) => {
                    tracing::warn!(%version, code = ?err.code(), error = %err, "update download failed");
                    core.set_update_status(UpdateStatus::Failed {
                        version: Some(version),
                        error: IpcError::from(CoreError::from(err)),
                    });
                }
            }
        });
        self.get_update_info().await
    }

    /// "Restart to install": installs the downloaded update. On macOS the app bundle is
    /// replaced in place and the app restarts; on Windows the installer runs without questions
    /// and starts the app again. A failure leaves the installed version as it is and shows up
    /// in `status`.
    pub async fn install_update(&self) -> Result<UpdateInfo, CoreError> {
        let source = self.updates().source().ok_or_else(not_supported)?;
        let UpdateStatus::Ready { version } = self.updates().status() else {
            return Err(UpdateError::NotReady("the update is not downloaded yet".into()).into());
        };
        let Ok(_step) = Arc::clone(&self.updates().step).try_lock_owned() else {
            return Err(UpdateError::NotReady("another update step is running".into()).into());
        };
        self.set_update_status(UpdateStatus::Installing {
            version: version.clone(),
        });
        tracing::info!(%version, "installing update");
        let outcome = tokio::task::spawn_blocking(move || source.install())
            .await
            .unwrap_or_else(|err| Err(UpdateError::InstallFailed(format!("install task: {err}"))));
        if let Err(err) = outcome {
            tracing::error!(%version, code = ?err.code(), error = %err, "update install failed");
            self.set_update_status(UpdateStatus::Failed {
                version: Some(version),
                error: IpcError::from(CoreError::from(err)),
            });
        }
        self.get_update_info().await
    }

    /// One look of the background task: reaches the network only with the switch on and the
    /// last check a day ago, and not while an update is already known.
    async fn automatic_update_check(&self) {
        let Some(source) = self.updates().source() else {
            return;
        };
        let settings = match self.update_settings().await {
            Ok(settings) => settings,
            Err(err) => {
                tracing::warn!(error = %err, "update setting unreadable; no automatic check");
                return;
            }
        };
        if !settings.automatic_check_due(self.now_ms()) {
            return;
        }
        if matches!(
            self.updates().status(),
            UpdateStatus::Available { .. }
                | UpdateStatus::Downloading { .. }
                | UpdateStatus::Ready { .. }
                | UpdateStatus::Installing { .. }
        ) {
            return;
        }
        if let Err(err) = self.run_check(source).await {
            tracing::warn!(error = %err, "automatic update check not run");
        }
    }

    async fn run_check(&self, source: Arc<dyn UpdateSource>) -> Result<(), CoreError> {
        let Ok(_step) = Arc::clone(&self.updates().step).try_lock_owned() else {
            return Err(UpdateError::NotReady("another update step is running".into()).into());
        };
        // The attempt counts, whatever its outcome: the throttle holds for a failing server too.
        let started = self.now_ms();
        self.modify_update_settings(|settings| settings.last_check_ms = Some(started))
            .await?;
        self.set_update_status(UpdateStatus::Checking);
        let status = match source.check().await {
            Ok(None) => {
                tracing::info!("update check: up to date");
                UpdateStatus::UpToDate {
                    checked_at_ms: started,
                }
            }
            Ok(Some(update)) => {
                tracing::info!(version = %update.version, "update check: update available");
                UpdateStatus::Available {
                    update: AvailableUpdate {
                        notes: clean_notes(update.notes),
                        ..update
                    },
                }
            }
            Err(err) => {
                tracing::warn!(code = ?err.code(), error = %err, "update check failed");
                UpdateStatus::Failed {
                    version: None,
                    error: IpcError::from(CoreError::from(err)),
                }
            }
        };
        self.set_update_status(status);
        Ok(())
    }

    /// Progress reports, at most one event per percent (or per 512 KiB without a total).
    fn download_progress(&self, version: String) -> DownloadProgress {
        let weak = self.downgrade();
        let last = Mutex::new(None::<u64>);
        Arc::new(move |downloaded, total| {
            let step = match total {
                Some(total) if total > 0 => downloaded.saturating_mul(100) / total,
                _ => downloaded / (512 * 1024),
            };
            {
                let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
                if *last == Some(step) {
                    return;
                }
                *last = Some(step);
            }
            if let Some(core) = weak.upgrade() {
                core.set_update_status(UpdateStatus::Downloading {
                    version: version.clone(),
                    downloaded,
                    total,
                });
            }
        })
    }

    fn set_update_status(&self, status: UpdateStatus) {
        *self
            .updates()
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(status.clone());
        self.emit_event(CoreEvent::UpdateStatusChanged { status });
    }

    async fn update_settings(&self) -> Result<UpdateSettings, CoreError> {
        let stored = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        let settings = stored
            .iter()
            .find(|setting| setting.key == SETTINGS_KEY)
            .map(|setting| {
                serde_json::from_str(&setting.value).unwrap_or_else(|err| {
                    tracing::warn!(error = %err, "update setting unreadable; using defaults (off)");
                    UpdateSettings::default()
                })
            })
            .unwrap_or_default();
        Ok(settings)
    }

    async fn modify_update_settings(
        &self,
        change: impl FnOnce(&mut UpdateSettings),
    ) -> Result<(), CoreError> {
        let _guard = self.updates().settings.lock().await;
        let mut settings = self.update_settings().await?;
        change(&mut settings);
        let json = serde_json::to_string(&settings)
            .map_err(|err| CoreError::Internal(format!("update setting: {err}")))?;
        self.store_ref()?
            .put_settings(GLOBAL_SCOPE, vec![(SETTINGS_KEY.to_owned(), json)])
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
