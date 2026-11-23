//! In-app updates (stage 15, decision 0037): `tauri-plugin-updater` as the core's
//! [`UpdateSource`]. The core decides whether and when to ask (consent, daily throttle, the
//! user's clicks); this module only talks to the plugin:
//!
//! - the manifest comes from the endpoint in `tauri.conf.json` (`plugins.updater.endpoints`;
//!   test builds may point elsewhere), with the app's own HTTP settings
//!   (`mail_engine::updates::http`): rustls, no proxy, redirects only to the update hosts;
//! - a download address outside the update hosts is refused before anything is fetched;
//! - the plugin checks the signature against `plugins.updater.pubkey` and, with
//!   `requireSignedVersion`, that the file was signed for the announced version;
//! - the download has an overall time limit and a size cap (finding N-3 of the stage 15
//!   review): a compromised release cannot make the app download without end before the
//!   signature check rejects it;
//! - macOS replaces the app bundle in place and the app restarts; Windows runs the NSIS
//!   installer in passive mode (`/P /UPDATE`, no "reinstall" page) and the installer starts the
//!   app again.
//!
//! The UI has no updater permission; it uses the app's own commands (`commands::updates`).

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::Poll;
use std::time::Duration;

use mail_engine::updates::http::{ClientConfigurator, UpdateHosts, client_configurator};
use mail_engine::{DownloadProgress, UpdateFuture, UpdateSource};
use mail_model::{AvailableUpdate, UpdateError};
use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Runtime, Url};
use tauri_plugin_updater::{Error as PluginError, Update, UpdaterExt};

/// Longest wait for the manifest.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest time a download may take, from connecting to the last byte (N-3). The app is about
/// 15 MB; this allows for a connection of 20 KB/s.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Largest update file accepted (N-3). The app is about 15 MB; a release file that grows past
/// this is not one of ours, and the download stops there instead of filling the memory.
const MAX_DOWNLOAD_BYTES: u64 = 300 * 1024 * 1024;

/// The plugin, configured from `plugins.updater` in `tauri.conf.json`. Its JavaScript
/// commands stay unusable: no capability grants `updater:*`.
pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// The manifest address from `plugins.updater.endpoints[0]`.
pub(crate) fn configured_endpoint<R: Runtime>(app: &AppHandle<R>) -> Option<Url> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("endpoints")?
        .get(0)?
        .as_str()
        .and_then(|s| Url::parse(s).ok())
}

/// May this copy of the app replace itself? Checked before installing.
pub(crate) type InstallGuard = fn() -> Result<(), String>;

/// The shipped guard: only an installed app updates itself, never a development build.
pub(crate) fn installed_app_only() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| format!("own path unknown: {err}"))?;
    install_location_problem(&exe).map_or(Ok(()), Err)
}

/// Why the app at `exe` cannot replace itself, if it cannot.
fn install_location_problem(exe: &std::path::Path) -> Option<String> {
    let path = exe.to_string_lossy();
    if cfg!(target_os = "macos") {
        if !path.contains(".app/Contents/MacOS/") {
            return Some("not running from an app bundle (development build)".into());
        }
        // Started from the disk image, or from a quarantined copy macOS runs from a read-only
        // place ("App Translocation"): the bundle cannot be replaced there.
        if path.starts_with("/Volumes/") || path.contains("/AppTranslocation/") {
            return Some(
                "running from the disk image or a translocated copy; move the app to \
                 Applications and start it from there"
                    .into(),
            );
        }
        return None;
    }
    if cfg!(windows) {
        // The NSIS installer puts its uninstaller next to the app.
        let installed = exe
            .parent()
            .is_some_and(|dir| dir.join("uninstall.exe").is_file());
        return (!installed).then(|| "not installed by the installer (development build)".into());
    }
    Some("this system does not update itself".into())
}

/// The core's update source on top of the plugin.
pub(crate) struct PluginSource<R: Runtime> {
    app: AppHandle<R>,
    endpoint: Url,
    hosts: UpdateHosts,
    configure: ClientConfigurator,
    install_guard: InstallGuard,
    /// The update the last check found.
    found: Mutex<Option<Update>>,
    /// Downloaded and verified: the update and its bytes.
    downloaded: Mutex<Option<(Update, Vec<u8>)>>,
    /// Size cap and overall time limit of a download ([`MAX_DOWNLOAD_BYTES`],
    /// [`DOWNLOAD_TIMEOUT`]; tests lower them).
    max_download_bytes: u64,
    download_timeout: Duration,
}

impl<R: Runtime> PluginSource<R> {
    pub(crate) fn new(
        app: AppHandle<R>,
        endpoint: Url,
        install_guard: InstallGuard,
    ) -> Result<Self, UpdateError> {
        let hosts = UpdateHosts::for_endpoint(&endpoint)?;
        let configure = client_configurator(hosts.clone())?;
        Ok(Self {
            app,
            endpoint,
            hosts,
            configure,
            install_guard,
            found: Mutex::new(None),
            downloaded: Mutex::new(None),
            max_download_bytes: MAX_DOWNLOAD_BYTES,
            download_timeout: DOWNLOAD_TIMEOUT,
        })
    }

    /// Other download limits (tests).
    #[cfg(test)]
    pub(crate) fn with_download_limits(mut self, max_bytes: u64, timeout: Duration) -> Self {
        self.max_download_bytes = max_bytes;
        self.download_timeout = timeout;
        self
    }

    async fn check_inner(&self) -> Result<Option<AvailableUpdate>, UpdateError> {
        let configure = Arc::clone(&self.configure);
        let app = self.app.clone();
        let updater = self
            .app
            .updater_builder()
            .endpoints(vec![self.endpoint.clone()])
            .map_err(|err| UpdateError::CheckFailed(err.to_string()))?
            .no_proxy()
            .timeout(CHECK_TIMEOUT)
            .configure_client(move |builder| configure(builder))
            // Windows: the installer replaces the running app, so the core stops first, then
            // Tauri cleans up as the plugin's own hook would (the plugin then starts the installer
            // and ends the process).
            .on_before_exit(move || {
                crate::before_exit(&app);
                app.cleanup_before_exit();
            })
            .build()
            .map_err(|err| UpdateError::CheckFailed(err.to_string()))?;
        let update = updater
            .check()
            .await
            .map_err(|err| UpdateError::CheckFailed(err.to_string()))?;
        *self
            .downloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        let Some(update) = update else {
            *self.found.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return Ok(None);
        };
        // The manifest is not signed: its download address must stay on the update hosts, so a
        // changed manifest cannot make the app connect anywhere else.
        if !self.hosts.permits(&update.download_url) {
            *self.found.lock().unwrap_or_else(PoisonError::into_inner) = None;
            return Err(UpdateError::CheckFailed(format!(
                "download address outside the update hosts: {}",
                update.download_url.host_str().unwrap_or("(no host)")
            )));
        }
        let available = AvailableUpdate {
            version: update.version.clone(),
            notes: update.body.clone(),
            pub_date: update
                .raw_json
                .get("pub_date")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
        };
        *self.found.lock().unwrap_or_else(PoisonError::into_inner) = Some(update);
        Ok(Some(available))
    }

    async fn download_inner(&self, progress: DownloadProgress) -> Result<(), UpdateError> {
        let mut update = self
            .found
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(|| UpdateError::NotReady("no update was found; check first".into()))?;
        // N-3: the plugin's own request timeout (connect to last byte; the update a check returns
        // has none), and the same limit around the whole download below.
        update.timeout = Some(self.download_timeout);
        let cap = self.max_download_bytes;
        let too_large = AtomicBool::new(false);
        let mut received: u64 = 0;
        // The download borrows `update`; it ends with this block.
        let bytes = {
            let mut download = std::pin::pin!(update.download(
                |chunk, total| {
                    received = received.saturating_add(u64::try_from(chunk).unwrap_or(u64::MAX));
                    // The announced length decides at the first chunk; without one, what arrived.
                    if received > cap || total.is_some_and(|t| t > cap) {
                        too_large.store(true, Ordering::Relaxed);
                        return;
                    }
                    progress(received, total);
                },
                || {},
            ));
            // The progress callback cannot fail the download, so the future is dropped (closing the
            // connection) as soon as a poll of it saw the cap exceeded.
            let capped = std::future::poll_fn(|cx| {
                let poll = download.as_mut().poll(cx);
                if too_large.load(Ordering::Relaxed) {
                    return Poll::Ready(Err(UpdateError::DownloadFailed(format!(
                        "the update file is larger than {cap} bytes"
                    ))));
                }
                poll.map(|result| result.map_err(|err| map_error(err, UpdateError::DownloadFailed)))
            });
            tokio::time::timeout(self.download_timeout, capped)
                .await
                .map_err(|_| {
                    UpdateError::DownloadFailed(format!(
                        "the download took longer than {:?}",
                        self.download_timeout
                    ))
                })??
        };
        *self
            .downloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((update, bytes));
        Ok(())
    }
}

/// Signature problems are their own error, whatever step found them.
fn map_error(err: PluginError, otherwise: fn(String) -> UpdateError) -> UpdateError {
    match err {
        PluginError::Minisign(_)
        | PluginError::Base64(_)
        | PluginError::SignatureUtf8(_)
        | PluginError::SignedVersionMismatch { .. }
        | PluginError::MissingSignedVersion => UpdateError::SignatureInvalid(err.to_string()),
        other => otherwise(other.to_string()),
    }
}

impl<R: Runtime> UpdateSource for PluginSource<R> {
    fn check(&self) -> UpdateFuture<'_, Option<AvailableUpdate>> {
        Box::pin(self.check_inner())
    }

    fn download(&self, progress: DownloadProgress) -> UpdateFuture<'_, ()> {
        Box::pin(self.download_inner(progress))
    }

    fn install(&self) -> Result<(), UpdateError> {
        (self.install_guard)().map_err(UpdateError::InstallFailed)?;
        let downloaded = self
            .downloaded
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (update, bytes) = downloaded
            .as_ref()
            .ok_or_else(|| UpdateError::NotReady("the update is not downloaded yet".into()))?;
        // Verifies the bytes once more, then: macOS swaps the bundle; Windows starts the
        // installer and ends the process (after `before_exit`).
        update
            .install(bytes)
            .map_err(|err| map_error(err, UpdateError::InstallFailed))?;
        tracing::info!(version = %update.version, "update installed; restarting");
        // Through the event loop, so the core shuts down as on every quit.
        self.app.request_restart();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
