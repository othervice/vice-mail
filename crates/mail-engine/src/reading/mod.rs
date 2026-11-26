//! Reading messages (stage 3): bodies on demand and in advance, the reading pane's view, links,
//! external images and attachments. [`crate::AppCore`] forwards to [`Reading`].
//!
//! **Opening** (`getMessage`): a stored body is parsed, sanitised and rendered at once (the
//! work runs on Tokio's blocking pool). Otherwise the body is fetched over the account's
//! reading connection ([`fetch`]) in the background and the call answers `loading`;
//! `messageBodyAvailable` or `messageBodyFailed` follows. A second call for the same message
//! starts no second fetch. Offline (the account's sync is `offline` or `error`) it answers
//! `unavailable / offline`, and `unavailable / notOnServer` when no stored location can hold it
//! any more. A failed fetch is reported once, by `messageBodyFailed`; the next `getMessage`
//! (the UI's "try again") starts a new fetch.
//!
//! **Renderings**: every view gets a new random render token; the real link targets stay here
//! under it ([`links`]). The navigation hook reports clicks as `linkActivated`; `describeLink`
//! checks the target (`mail_content::check_link`), `openLink` hands only `http`, `https` and
//! `mailto` to the system ([`opener`]).
//!
//! **External images** stay blocked unless the user loads them once or always for the sender
//! (`remote_content_sender`). Then the core fetches them itself ([`remote`]) and renders again
//! with the bytes. Limit, stated plainly: the `From` address can be forged; a forger of an
//! allowed sender gets their tracking images loaded (no code runs, nothing else changes).
//!
//! **Images** (inline, allowed external, `data:` URLs in the HTML) are shown only re-encoded
//! ([`images`], finding C-4, decision 0046): a first rendering lists the ones it needs, the
//! helper process encodes them again in one batch, a second rendering shows them; results are
//! kept in memory, so a message pays this only the first time.
//!
//! **Attachments** are decoded from the stored body; of a partial body, the sections are
//! fetched first. Saving goes to the path the shell's dialog returned ([`files`]); opening
//! writes a read-only copy into the app's own directory and asks for confirmation for
//! executable types first.
//!
//! **Prefetch** ([`prefetch`]) runs per account after each completed sync pass.

mod eml;
mod export;
mod fetch;
mod files;
mod images;
pub mod index_text;
mod links;
pub mod opener;
mod prefetch;
pub mod remote;
mod source;
mod view;

#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_content::{RemotePolicy, RenderOptions, render_html, render_text, sniff_image};
use mail_model::{
    AccountId, AccountState, BodyFormat, BodySection, BodyUnavailableReason, CoreError, CoreEvent,
    DarkMails, IpcError, LinkInfo, MailConnector, MailScheme, MailboxRole, MessageBody, MessageId,
    MessageView, OpenAttachmentResult, PrefetchSettings, PrefetchStatus, RemoteContentScope,
    RenderToken, TrackerSummary,
};
use mail_store::{GLOBAL_SCOPE, Store, StoredMessage};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::credentials::Credentials;
use crate::sync::account::Registry;
use crate::sync::{SyncConfig, emit};

pub use links::RENDERINGS;
pub use opener::{DesktopOpener, RecordingOpener, SystemOpener};
pub use remote::RemoteConfig;

use fetch::{Connections, Fetched};
pub(crate) use links::FoundLinks;
use links::{RenderOwner, Rendering, Renderings};
use prefetch::Prefetcher;
use view::{Attachment, AttachmentSource, Content};

/// Delay of the start-up sweep of body files.
pub const SWEEP_DELAY: Duration = Duration::from_secs(5);

/// Stage 20 (decision 0060): how long an answer about an account's quota is kept.
pub const QUOTA_KEEP: Duration = Duration::from_secs(15 * 60);

/// Stage 20 (decision 0058): setting key (global, synced, JSON boolean) of tracker blocking in
/// images; missing means on.
pub(crate) const BLOCK_TRACKERS_KEY: &str = "reading.block_trackers";
/// Stage 20: setting key of removing tracking parameters from links; missing means on.
pub(crate) const CLEAN_LINKS_KEY: &str = "reading.clean_links";
/// Stage 21 (decision 0066): setting key (global, synced, JSON `"adapted"` or `"light"`) of
/// HTML mail in the dark theme; missing means adapted.
pub(crate) const DARK_MAILS_KEY: &str = "reading.dark_mails";

/// Stage 20: the tracker settings a rendering uses; stage 21: and how it shows a mail's
/// colours in the dark theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Privacy {
    pub block_trackers: bool,
    pub clean_links: bool,
    pub dark_mails: DarkMails,
}

/// What reading gets from the shell. The default is the product's.
pub struct ReadingServices {
    /// Opens links and attachments with the system's default applications.
    pub opener: Arc<dyn SystemOpener>,
    pub remote: RemoteConfig,
    /// Directory for the copies of opened attachments, emptied at every start. `None`:
    /// `<data dir>/open-attachments`. The shell passes its cache directory.
    pub open_dir: Option<PathBuf>,
    /// Finding C-2 (decision 0043): the `mail-mcp` helper that renders image and PDF previews
    /// in a child process within a time limit and a memory budget, and (finding C-4, decision
    /// 0046) re-encodes the images of the mails shown. `None`: both run in this process (tests
    /// and tools). The app passes the helper next to its executable; when that file is missing,
    /// those previews and images are refused rather than decoded here.
    pub preview_helper: Option<PathBuf>,
}

impl Default for ReadingServices {
    fn default() -> Self {
        Self {
            opener: Arc::new(DesktopOpener),
            remote: RemoteConfig::default(),
            open_dir: None,
            preview_helper: None,
        }
    }
}

/// Whether external images load for a rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Remote {
    /// Only if the sender is on the account's allowlist.
    IfAllowed,
    /// The user asked for them.
    Load,
}

pub(crate) struct Reading<C: MailConnector> {
    pub(crate) store: Store,
    pub(crate) events: broadcast::Sender<CoreEvent>,
    pub(crate) connections: Connections<C>,
    accounts: Arc<Registry>,
    renderings: Renderings,
    remote: remote::RemoteImages,
    opener: Arc<dyn SystemOpener>,
    open_dir: PathBuf,
    preview_helper: Option<PathBuf>,
    /// C-4: the images of renderings, re-encoded and kept.
    images: images::Images,
    fetching: Mutex<HashSet<MessageId>>,
    pub(crate) prefetch: Prefetcher,
    listener: Mutex<Option<JoinHandle<()>>>,
    /// Stage 9: `.eml` files opened read-only (decision 0027).
    eml: eml::EmlFiles,
    /// Stage 20: whether links get cleaned, as last read from the settings (for the links of
    /// AI results, which are remembered without a store read).
    clean_links: AtomicBool,
    /// Stage 20 (decision 0060): the last answer about each account's quota, and when.
    quotas: Mutex<std::collections::HashMap<AccountId, (Instant, mail_model::MailboxQuota)>>,
}

impl<C: MailConnector> Drop for Reading<C> {
    fn drop(&mut self) {
        self.prefetch.abort_all();
        if let Some(task) = self
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            task.abort();
        }
    }
}

/// Runs CPU work (parsing, sanitising) on Tokio's blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, CoreError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| CoreError::Internal(format!("reading task failed: {err}")))
}

impl<C: MailConnector + 'static> Reading<C> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start(
        store: Store,
        events: broadcast::Sender<CoreEvent>,
        connector: Arc<C>,
        credentials: Credentials,
        sync: Arc<SyncConfig>,
        accounts: Arc<Registry>,
        services: ReadingServices,
        data_dir: &Path,
    ) -> Arc<Self> {
        let open_dir = services
            .open_dir
            .unwrap_or_else(|| data_dir.join("open-attachments"));
        let reading = Arc::new(Self {
            store,
            events,
            connections: Connections::new(connector, credentials, sync),
            accounts,
            renderings: Renderings::default(),
            remote: remote::RemoteImages::new(&services.remote),
            opener: services.opener,
            open_dir,
            images: images::Images::new(services.preview_helper.clone()),
            preview_helper: services.preview_helper,
            fetching: Mutex::default(),
            prefetch: Prefetcher::default(),
            listener: Mutex::default(),
            eml: eml::EmlFiles::default(),
            clean_links: AtomicBool::new(true),
            quotas: Mutex::default(),
        });
        reading.sweep_later();
        let listener = reading.spawn_listener();
        *reading
            .listener
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(listener);
        reading
    }

    /// Removes the copies of attachments opened in an earlier session. Awaited at start, before
    /// anything can be opened again.
    pub(crate) async fn clear_open_dir(&self) {
        let open_dir = self.open_dir.clone();
        if let Err(err) =
            tokio::task::spawn_blocking(move || files::clear_open_dir(&open_dir)).await
        {
            tracing::warn!(error = %err, "clearing opened attachments failed");
        }
    }

    /// Body files nothing refers to (crash leftovers, failed removals), a few seconds after
    /// start, off the start path. Holds the store only while it sweeps, so a core that is
    /// dropped right away closes its database at once.
    fn sweep_later(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(SWEEP_DELAY).await;
            let Some(store) = weak.upgrade().map(|reading| reading.store.clone()) else {
                return;
            };
            if let Err(err) = store.sweep_body_files(mail_store::SWEEP_MIN_AGE).await {
                tracing::warn!(error = %err, "sweeping body files failed");
            }
        });
    }

    /// Prefetch follows the accounts' sync: a completed pass (`idle`) starts a run, `offline`
    /// and `error` pause it.
    fn spawn_listener(self: &Arc<Self>) -> JoinHandle<()> {
        let mut events = self.events.subscribe();
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(CoreEvent::AccountStatusChanged { account_id, status }) => {
                        let Some(reading) = weak.upgrade() else {
                            return;
                        };
                        match status.state {
                            AccountState::Idle => reading.prefetch.wake(&reading, account_id),
                            AccountState::Offline | AccountState::Error => {
                                reading.prefetch.pause(&reading, account_id);
                            }
                            AccountState::Connecting | AccountState::Syncing => {}
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, "prefetch listener fell behind");
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        })
    }

    pub(crate) fn emit(&self, event: CoreEvent) {
        emit(&self.events, event);
    }

    /// Stops prefetching and closes the reading connection of a removed account.
    pub(crate) fn forget_account(&self, account: AccountId) {
        self.prefetch.forget(account);
        self.connections.forget(account);
        self.renderings.forget_account(account);
    }

    /// The password changed: the next read connects with the new one.
    pub(crate) fn credentials_changed(&self, account: AccountId) {
        self.connections.forget(account);
    }

    pub(crate) fn shutdown(&self) {
        self.prefetch.abort_all();
    }

    // -----------------------------------------------------------------------------------------
    // Opening
    // -----------------------------------------------------------------------------------------

    pub(crate) async fn get_message(
        self: &Arc<Self>,
        message: MessageId,
        format: BodyFormat,
        scheme: MailScheme,
    ) -> Result<MessageView, CoreError> {
        let summary = self.summary(message).await?;
        if let Some(content) = self.load_content(message).await? {
            self.backfill_preview(&summary, &content);
            return self
                .render(summary, content, format, Remote::IfAllowed, scheme)
                .await;
        }
        let body = self.body_missing(&summary).await?;
        Ok(MessageView {
            id: summary.id,
            account_id: summary.account_id,
            header: view::stored_header(&summary),
            body,
            formats: Vec::new(),
            attachments: Vec::new(),
            partial: false,
            problems: Vec::new(),
        })
    }

    /// Renders the HTML again with its external images, loaded by the core. `sender` also puts
    /// the sender on the account's allowlist first.
    pub(crate) async fn load_remote_content(
        self: &Arc<Self>,
        message: MessageId,
        scope: RemoteContentScope,
        scheme: MailScheme,
    ) -> Result<MessageView, CoreError> {
        let summary = self.summary(message).await?;
        let content = self.load_content(message).await?.ok_or_else(|| {
            CoreError::NotFound(format!("the body of message {message} is not stored"))
        })?;
        if scope == RemoteContentScope::Sender {
            let sender = summary
                .from
                .as_ref()
                .map(|a| a.email.clone())
                .ok_or_else(|| {
                    CoreError::InvalidArgument("the message has no sender address".into())
                })?;
            self.store
                .allow_remote_content(summary.account_id, &sender)
                .await?;
            tracing::info!(account = %summary.account_id, "remote content allowed for a sender");
        }
        self.render(summary, content, BodyFormat::Html, Remote::Load, scheme)
            .await
    }

    async fn summary(&self, message: MessageId) -> Result<StoredMessage, CoreError> {
        self.store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))
    }

    /// The stored body, parsed; `None` if nothing usable is stored.
    async fn load_content(&self, message: MessageId) -> Result<Option<Content>, CoreError> {
        let Some(info) = self.store.body_info(message).await? else {
            return Ok(None);
        };
        if info.complete {
            let Some(raw) = self.store.load_message(message).await? else {
                return Ok(None);
            };
            return blocking(move || Content::complete(&raw)).await.map(Some);
        }
        let Some(structure) = info.structure else {
            return Ok(None);
        };
        let sections = self.store.load_sections(message, None).await?;
        if !sections.iter().any(|s| s.section == BodySection::Header) {
            return Ok(None);
        }
        blocking(move || Content::partial(&structure, &sections))
            .await
            .map(Some)
    }

    async fn render(
        &self,
        summary: StoredMessage,
        content: Content,
        format: BodyFormat,
        remote: Remote,
        scheme: MailScheme,
    ) -> Result<MessageView, CoreError> {
        let locations = self.store.message_locations(summary.id).await?;
        let own_mail = locations
            .iter()
            .any(|l| matches!(l.role, MailboxRole::Sent | MailboxRole::Drafts));
        let header = content.header(&summary, own_mail);
        let formats = content.formats();
        let problems = content.problems();
        let attachments = content.attachments.iter().map(|a| a.info.clone()).collect();
        let partial = content.partial;
        let load = match remote {
            Remote::Load => true,
            Remote::IfAllowed => match &summary.from {
                Some(from) if content.shows_html(format) => {
                    self.store
                        .remote_content_allowed(summary.account_id, &from.email)
                        .await?
                }
                _ => false,
            },
        };
        let owner = RenderOwner::Message {
            account: summary.account_id,
            message: summary.id,
        };
        let body = self
            .render_body(&content, format, load, owner, scheme)
            .await?;
        Ok(MessageView {
            id: summary.id,
            account_id: summary.account_id,
            header,
            body,
            formats,
            attachments,
            partial,
            problems,
        })
    }

    /// Stage 20 (decision 0060): how full the account's mailbox is, over the reading
    /// connection (read-only). An answer is kept for [`QUOTA_KEEP`] unless `refresh`; a failure
    /// (offline, a refusal) is `unknown` with the error and not kept.
    pub(crate) async fn quota(
        &self,
        account: AccountId,
        refresh: bool,
    ) -> Result<mail_model::MailboxQuota, CoreError> {
        let stored = self
            .store
            .account(account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {account}")))?;
        if !refresh {
            let cached = self
                .quotas
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&account)
                .filter(|(at, _)| at.elapsed() < QUOTA_KEEP)
                .map(|(_, quota)| quota.clone());
            if let Some(quota) = cached {
                return Ok(quota);
            }
        }
        let asked = async {
            let mut held = self.connections.hold(&stored).await?;
            held.quota().await.map_err(CoreError::from)
        }
        .await;
        let quota = match asked {
            Ok(Some(quota)) => mail_model::MailboxQuota::reported(quota),
            Ok(None) => mail_model::MailboxQuota::NotReported,
            Err(err) => {
                tracing::debug!(account = %account, code = ?err.code(), "quota unknown");
                return Ok(mail_model::MailboxQuota::Unknown {
                    error: Some(IpcError::from(err)),
                });
            }
        };
        self.quotas
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(account, (Instant::now(), quota.clone()));
        Ok(quota)
    }

    /// Stage 20: the tracker settings as stored (missing or unreadable: on).
    pub(crate) async fn privacy(&self) -> Privacy {
        let mut privacy = Privacy {
            block_trackers: true,
            clean_links: true,
            dark_mails: DarkMails::Adapted,
        };
        match self.store.settings(GLOBAL_SCOPE).await {
            Ok(settings) => {
                for setting in settings {
                    if setting.key == DARK_MAILS_KEY {
                        match serde_json::from_str::<DarkMails>(&setting.value) {
                            Ok(dark_mails) => privacy.dark_mails = dark_mails,
                            Err(err) => {
                                tracing::warn!(key = %setting.key, %err, "dark mails setting unreadable; adapted");
                            }
                        }
                        continue;
                    }
                    let slot = match setting.key.as_str() {
                        BLOCK_TRACKERS_KEY => &mut privacy.block_trackers,
                        CLEAN_LINKS_KEY => &mut privacy.clean_links,
                        _ => continue,
                    };
                    match serde_json::from_str::<bool>(&setting.value) {
                        Ok(on) => *slot = on,
                        Err(err) => {
                            tracing::warn!(key = %setting.key, %err, "tracker setting unreadable; on");
                        }
                    }
                }
            }
            Err(err) => tracing::warn!(error = %err, "tracker settings unreadable; on"),
        }
        self.clean_links
            .store(privacy.clean_links, Ordering::Relaxed);
        privacy
    }

    /// The body of `content` in `format`: sanitised HTML (external images loaded by the core
    /// when `load`, every image re-encoded; trackers blocked and counted per the setting;
    /// stage 21: colours for `scheme` as the setting "Mails in the dark theme" says) or text,
    /// under a new render token whose links are remembered for `owner`.
    async fn render_body(
        &self,
        content: &Content,
        format: BodyFormat,
        load: bool,
        owner: RenderOwner,
        scheme: MailScheme,
    ) -> Result<MessageBody, CoreError> {
        let token = Renderings::new_token();
        let privacy = self.privacy().await;
        let (body, links) = if content.shows_html(format) {
            let html = content.parsed.html.clone().unwrap_or_default();
            let mut options = RenderOptions::new(token.clone());
            options.block_trackers = privacy.block_trackers;
            // Stage 21 (decision 0066): "always light" shows the mail as in the light theme.
            options.scheme = match (scheme, privacy.dark_mails) {
                (MailScheme::Dark, DarkMails::Adapted) => MailScheme::Dark,
                _ => MailScheme::Light,
            };
            options.inline = content.parsed.inline_images();
            // C-4: what is kept already; a message seen before renders once.
            options.images = self.images.known(options.inline.images());
            let (mut rendered, html, mut options) = blocking(move || {
                let rendered = render_html(&html, &options);
                (rendered, html, options)
            })
            .await?;
            let mut wanted = std::mem::take(&mut rendered.pending_images);
            let mut again = false;
            if load && !rendered.remote_resources.is_empty() {
                let urls = rendered
                    .remote_resources
                    .iter()
                    .map(|r| r.url.clone())
                    .collect();
                let images = self
                    .remote
                    .fetch_all(urls, options.max_image_bytes, options.max_total_image_bytes)
                    .await;
                if !images.is_empty() {
                    // Re-encoded in the same batch as the rest.
                    wanted.extend(
                        images
                            .values()
                            .filter(|bytes| sniff_image(bytes).is_some())
                            .cloned(),
                    );
                    options.remote = RemotePolicy::Allow(images);
                    again = true;
                }
            }
            if !wanted.is_empty() {
                options.images.extend(self.images.reencode(wanted).await);
                again = true;
            }
            if again {
                rendered = blocking(move || render_html(&html, &options)).await?;
            }
            (
                MessageBody::Html {
                    srcdoc: rendered.srcdoc,
                    render_token: token.clone(),
                    blocked_remote: rendered.blocked_count,
                    remote_loaded: load,
                    trackers: TrackerSummary {
                        pixels: rendered.trackers.pixels,
                        known_hosts: rendered.trackers.known_hosts,
                        parameters: rendered.trackers.parameters,
                        hosts: rendered.trackers.hosts,
                    },
                    colors: rendered.colors,
                },
                rendered.links,
            )
        } else {
            let text = content.parsed.display_text().into_owned();
            let rendered = blocking(move || render_text(&text)).await?;
            (
                MessageBody::Text {
                    segments: view::text_segments(&rendered),
                    render_token: token.clone(),
                },
                rendered.links,
            )
        };
        self.renderings.insert(
            token,
            Rendering {
                owner,
                links,
                clean_links: privacy.clean_links,
            },
        );
        Ok(body)
    }

    /// A body stored without a list preview (for example by an older version) gets one.
    fn backfill_preview(&self, summary: &StoredMessage, content: &Content) {
        if summary.preview.is_some() || content.parsed.preview.trim().is_empty() {
            return;
        }
        let store = self.store.clone();
        let events = self.events.clone();
        let (message, account) = (summary.id, summary.account_id);
        let preview = content.parsed.preview.clone();
        tokio::spawn(async move {
            if let Err(err) = store.set_previews(vec![(message, preview)]).await {
                tracing::warn!(error = %err, "list preview could not be stored");
                return;
            }
            notify_lists(&store, &events, account, message).await;
        });
    }

    /// Not stored: why not, or start the fetch.
    async fn body_missing(
        self: &Arc<Self>,
        summary: &StoredMessage,
    ) -> Result<MessageBody, CoreError> {
        let state = self.accounts.status(summary.account_id).map(|s| s.state);
        if matches!(
            state,
            None | Some(AccountState::Offline | AccountState::Error)
        ) {
            return Ok(MessageBody::Unavailable {
                reason: BodyUnavailableReason::Offline,
                error: None,
            });
        }
        let locations = self.store.message_locations(summary.id).await?;
        if fetch::usable_locations(locations).is_empty() {
            return Ok(MessageBody::Unavailable {
                reason: BodyUnavailableReason::NotOnServer,
                error: None,
            });
        }
        self.start_fetch(summary.clone());
        Ok(MessageBody::Loading)
    }

    /// Fetches the body in the background, once per message at a time. A panic in the adapter
    /// is a failed fetch, not a lost one.
    fn start_fetch(self: &Arc<Self>, summary: StoredMessage) {
        let message = summary.id;
        let account = summary.account_id;
        if !self
            .fetching
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(message)
        {
            return;
        }
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let worker = Arc::clone(&this);
            let result = tokio::spawn(async move { worker.fetch_for_open(&summary).await })
                .await
                .unwrap_or_else(|err| {
                    Err(CoreError::Internal(format!(
                        "the fetch failed unexpectedly: {err}"
                    )))
                });
            this.fetching
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&message);
            let error = match result {
                Ok(true) => {
                    this.emit(CoreEvent::MessageBodyAvailable {
                        account_id: account,
                        message_id: message,
                    });
                    return;
                }
                Ok(false) => IpcError::from(CoreError::NotFound(
                    "the message is no longer on the server".into(),
                )),
                Err(err) => {
                    tracing::warn!(message = message.0, code = ?err.code(), error = %err, "fetching a body failed");
                    IpcError::from(err)
                }
            };
            this.emit(CoreEvent::MessageBodyFailed {
                account_id: account,
                message_id: message,
                error,
            });
        });
    }

    /// Stage 12 (AI access over MCP): the parsed body and the reading pane's attachment list.
    /// A body that is not stored is fetched now over the account's read-only reading
    /// connection (`BODY.PEEK`: nothing becomes read) while the account is online; `None`
    /// when it is offline, the message is no longer on the server, or nothing usable is stored.
    pub(crate) async fn content_for_model(
        &self,
        summary: &StoredMessage,
    ) -> Result<Option<(mail_content::ParsedMessage, Vec<mail_model::AttachmentInfo>)>, CoreError>
    {
        let split = |content: Content| {
            let attachments = content.attachments.into_iter().map(|a| a.info).collect();
            (content.parsed, attachments)
        };
        if let Some(content) = self.load_content(summary.id).await? {
            return Ok(Some(split(content)));
        }
        let state = self.accounts.status(summary.account_id).map(|s| s.state);
        if matches!(
            state,
            None | Some(AccountState::Offline | AccountState::Error)
        ) {
            return Ok(None);
        }
        if !self.fetch_for_open(summary).await? {
            return Ok(None);
        }
        Ok(self.load_content(summary.id).await?.map(split))
    }

    /// `true` once stored, `false` if the server no longer has the message.
    async fn fetch_for_open(&self, summary: &StoredMessage) -> Result<bool, CoreError> {
        let account = self
            .store
            .account(summary.account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", summary.account_id)))?;
        let started = Instant::now();
        let fetched = fetch::fetch_body(
            &self.connections,
            &self.store,
            &account,
            summary.id,
            summary.size,
            fetch::OPEN_LIMIT,
        )
        .await?;
        if matches!(fetched, Fetched::NotOnServer) {
            return Ok(false);
        }
        self.store_fetched(summary.id, fetched).await?;
        tracing::debug!(
            message = summary.id.0,
            ms = started.elapsed().as_millis() as u64,
            "body fetched for opening"
        );
        notify_lists(&self.store, &self.events, summary.account_id, summary.id).await;
        Ok(true)
    }

    /// Stores a fetched body with its list preview; returns how many bodies were stored.
    pub(crate) async fn store_fetched(
        &self,
        message: MessageId,
        fetched: Fetched,
    ) -> Result<usize, CoreError> {
        match fetched {
            Fetched::Whole(raw) => self.store_whole_bodies(vec![(message, raw)]).await,
            Fetched::Sections(structure, sections) => {
                let (preview, structure, sections) = blocking(move || {
                    let preview = Content::partial(&structure, &sections).parsed.preview;
                    (preview, structure, sections)
                })
                .await?;
                self.store
                    .store_body_sections(message, structure, sections)
                    .await?;
                self.store.set_previews(vec![(message, preview)]).await?;
                Ok(1)
            }
            Fetched::NotOnServer => Ok(0),
        }
    }

    /// Stores whole messages (one `store_bodies`) and their list previews.
    pub(crate) async fn store_whole_bodies(
        &self,
        bodies: Vec<(MessageId, Vec<u8>)>,
    ) -> Result<usize, CoreError> {
        if bodies.is_empty() {
            return Ok(0);
        }
        let (previews, bodies) = blocking(move || {
            let previews: Vec<(MessageId, String)> = bodies
                .iter()
                .map(|(id, raw)| (*id, mail_content::parse(raw).preview))
                .collect();
            (previews, bodies)
        })
        .await?;
        let stored: HashSet<MessageId> = self
            .store
            .store_bodies(bodies)
            .await?
            .into_iter()
            .map(|info| info.message)
            .collect();
        let previews: Vec<_> = previews
            .into_iter()
            .filter(|(id, _)| stored.contains(id))
            .collect();
        self.store.set_previews(previews).await?;
        Ok(stored.len())
    }

    // -----------------------------------------------------------------------------------------
    // Links
    // -----------------------------------------------------------------------------------------

    /// Decision 0028: the links found in an AI result, under a token of their own for
    /// `describe_link`/`open_link`; `None` when there are none.
    pub(crate) fn remember_found_links(
        &self,
        found: FoundLinks,
        accounts: Vec<AccountId>,
    ) -> Option<RenderToken> {
        self.renderings
            .insert_found(found, accounts, self.clean_links.load(Ordering::Relaxed))
    }

    pub(crate) fn describe_link(
        &self,
        token: &RenderToken,
        index: u32,
    ) -> Result<LinkInfo, CoreError> {
        Ok(links::link_info(&self.renderings.link(token, index)?))
    }

    pub(crate) async fn open_link(
        &self,
        token: &RenderToken,
        index: u32,
    ) -> Result<mail_model::LinkOpened, CoreError> {
        let links::CheckedLink { check, removed, .. } = self.renderings.link(token, index)?;
        let url = check.open_url.ok_or_else(|| {
            CoreError::InvalidArgument(format!(
                "only http, https and mailto links open (this one is {:?})",
                check.scheme
            ))
        })?;
        // Stage 10: a `mailto:` link opens the app's own composer, not the system's mail app.
        if check.scheme.eq_ignore_ascii_case("mailto") {
            let request = mail_content::parse_mailto(&url)?;
            tracing::info!(
                recipients = request.to.len() + request.cc.len() + request.bcc.len(),
                "mailto link of a mail opens the composer"
            );
            return Ok(mail_model::LinkOpened::Compose { request });
        }
        let opener = Arc::clone(&self.opener);
        blocking(move || opener.open_url(&url)).await??;
        tracing::info!(
            scheme = %check.scheme,
            host = check.host.as_deref(),
            tracking_removed = removed.len(),
            "link opened in the system"
        );
        Ok(mail_model::LinkOpened::Opened)
    }

    /// The navigation hook saw a click on a link placeholder: tells the UI, if the token is
    /// one of ours. Returns whether it was.
    pub(crate) fn link_activated(&self, token: RenderToken, index: u32) -> bool {
        let Some(rendering) = self.renderings.get(&token) else {
            tracing::warn!(index, "click on a link of an unknown rendering ignored");
            return false;
        };
        match &rendering.owner {
            RenderOwner::Message { message, .. } => {
                tracing::debug!(message = message.0, index, "link clicked");
            }
            RenderOwner::File(_) => tracing::debug!(index, "link of an opened file clicked"),
            // AI results are never shown in a frame; a navigation with their token is forged.
            RenderOwner::AiResult { .. } => {
                tracing::warn!(index, "navigation to a link of an AI result ignored");
                return false;
            }
        }
        self.emit(CoreEvent::LinkActivated {
            render_token: token,
            index,
        });
        true
    }

    // -----------------------------------------------------------------------------------------
    // Attachments
    // -----------------------------------------------------------------------------------------

    /// Attachment `index` of a message as the reading pane lists it. Stage 20 (decision 0060):
    /// a body no longer stored (the attachments view lists what was stored once) is fetched
    /// first, as opening the message would.
    async fn attachment(&self, message: MessageId, index: u32) -> Result<Attachment, CoreError> {
        let content = match self.load_content(message).await? {
            Some(content) => content,
            None => {
                let summary = self.summary(message).await?;
                if !self.fetch_for_open(&summary).await? {
                    return Err(CoreError::NotFound(format!(
                        "message {message} is no longer on the server"
                    )));
                }
                self.load_content(message).await?.ok_or_else(|| {
                    CoreError::NotFound(format!("the body of message {message} is not stored"))
                })?
            }
        };
        content
            .attachments
            .into_iter()
            .find(|a| a.info.index == index)
            .ok_or_else(|| CoreError::NotFound(format!("attachment {index} of message {message}")))
    }

    /// What the reading pane shows of attachment `index` (pinning keeps it).
    pub(crate) async fn attachment_info(
        &self,
        message: MessageId,
        index: u32,
    ) -> Result<mail_model::AttachmentInfo, CoreError> {
        Ok(self.attachment(message, index).await?.info)
    }

    /// The sanitised name, for the save dialog.
    pub(crate) async fn attachment_name(
        &self,
        message: MessageId,
        index: u32,
    ) -> Result<String, CoreError> {
        Ok(self.attachment(message, index).await?.info.name)
    }

    /// The decoded bytes; sections of a partial body are fetched first if not stored.
    async fn attachment_bytes(
        &self,
        message: MessageId,
        attachment: &Attachment,
    ) -> Result<Vec<u8>, CoreError> {
        match &attachment.source {
            AttachmentSource::Whole { part_id } => {
                let part_id = *part_id;
                let raw = self.store.load_message(message).await?.ok_or_else(|| {
                    CoreError::NotFound(format!("the body of message {message} is not stored"))
                })?;
                blocking(move || mail_content::attachment_bytes(&raw, part_id))
                    .await?
                    .ok_or_else(|| {
                        CoreError::Internal("the attachment could not be decoded".into())
                    })
            }
            AttachmentSource::Section { path } => {
                let wanted = vec![
                    BodySection::Mime(path.clone()),
                    BodySection::Part(path.clone()),
                ];
                let mut sections = self
                    .store
                    .load_sections(message, Some(wanted.clone()))
                    .await?;
                if sections.len() < wanted.len() {
                    sections = self.fetch_attachment_sections(message, &wanted).await?;
                }
                let find = |section: &BodySection| {
                    sections
                        .iter()
                        .find(|s| &s.section == section)
                        .map(|s| s.data.as_slice())
                };
                let (Some(mime), Some(part)) = (find(&wanted[0]), find(&wanted[1])) else {
                    return Err(CoreError::NotFound(
                        "the server did not return the attachment".into(),
                    ));
                };
                let mut entity = Vec::with_capacity(mime.len() + part.len());
                entity.extend_from_slice(mime);
                entity.extend_from_slice(part);
                blocking(move || mail_content::attachment_bytes(&entity, 0))
                    .await?
                    .ok_or_else(|| {
                        CoreError::Internal("the attachment could not be decoded".into())
                    })
            }
        }
    }

    async fn fetch_attachment_sections(
        &self,
        message: MessageId,
        wanted: &[BodySection],
    ) -> Result<Vec<mail_model::FetchedSection>, CoreError> {
        let summary = self.summary(message).await?;
        let account = self
            .store
            .account(summary.account_id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", summary.account_id)))?;
        let structure = self
            .store
            .body_info(message)
            .await?
            .and_then(|info| info.structure)
            .ok_or_else(|| {
                CoreError::NotFound(format!("the body of message {message} is not stored"))
            })?;
        let fetched =
            fetch::fetch_sections(&self.connections, &self.store, &account, message, wanted)
                .await?
                .ok_or_else(|| {
                    CoreError::NotFound("the message is no longer on the server".into())
                })?;
        self.store
            .store_body_sections(message, structure, fetched.clone())
            .await?;
        Ok(fetched)
    }

    /// Writes attachment `index` to `path` (chosen by the user in the system dialog).
    pub(crate) async fn save_attachment_to(
        &self,
        message: MessageId,
        index: u32,
        path: PathBuf,
    ) -> Result<PathBuf, CoreError> {
        let attachment = self.attachment(message, index).await?;
        let bytes = self.attachment_bytes(message, &attachment).await?;
        let saved = blocking(move || files::save_atomically(&path, &bytes)).await??;
        tracing::info!(message = message.0, index, "attachment saved");
        Ok(saved)
    }

    /// Stage 16 (decision 0036): the preview of attachment `index` (`page` of a PDF, 0-based).
    /// Only kinds the core previews; the bytes are decoded and encoded again in Rust, fetched
    /// first if only part of the message is stored.
    pub(crate) async fn preview_attachment(
        &self,
        message: MessageId,
        index: u32,
        page: u32,
    ) -> Result<mail_model::AttachmentPreview, CoreError> {
        let attachment = self.attachment(message, index).await?;
        let kind = preview_kind_of(&attachment.info)?;
        let bytes = self.attachment_bytes(message, &attachment).await?;
        let preview = self.render_preview(kind, bytes, page).await?;
        tracing::debug!(message = message.0, index, ?kind, "attachment previewed");
        Ok(preview)
    }

    /// Stage 16 (decision 0036), "save all": every attachment of the message into `folder`
    /// (chosen by the user in the system dialog), sanitised names, taken names numbered.
    /// Returns how many were written and how many of them under another name. A failure stops
    /// there; what was written stays.
    pub(crate) async fn save_all_attachments_to(
        &self,
        message: MessageId,
        folder: PathBuf,
    ) -> Result<(u32, u32), CoreError> {
        let content = self.load_content(message).await?.ok_or_else(|| {
            CoreError::NotFound(format!("the body of message {message} is not stored"))
        })?;
        if content.attachments.is_empty() {
            return Err(CoreError::InvalidArgument(format!(
                "message {message} has no attachments"
            )));
        }
        let (mut saved, mut renamed) = (0u32, 0u32);
        for attachment in &content.attachments {
            let bytes = self.attachment_bytes(message, attachment).await?;
            let name = mail_content::sanitize_filename(&attachment.info.name);
            if save_into(&folder, name, bytes).await? {
                renamed += 1;
            }
            saved += 1;
        }
        tracing::info!(message = message.0, saved, renamed, "all attachments saved");
        Ok((saved, renamed))
    }

    /// Opens a copy of attachment `index` with the system's default app; executable types only
    /// when `confirmed_executable`.
    pub(crate) async fn open_attachment(
        &self,
        message: MessageId,
        index: u32,
        confirmed_executable: bool,
    ) -> Result<OpenAttachmentResult, CoreError> {
        let attachment = self.attachment(message, index).await?;
        if attachment.info.executable && !confirmed_executable {
            return Ok(OpenAttachmentResult::NeedsConfirmation);
        }
        let bytes = self.attachment_bytes(message, &attachment).await?;
        let dir = self.open_dir.clone();
        let folder = Renderings::new_token().as_str().to_owned();
        let name = mail_content::sanitize_filename(&attachment.info.name);
        let path =
            blocking(move || files::write_for_opening(&dir, &folder, &name, &bytes)).await??;
        let opener = Arc::clone(&self.opener);
        blocking(move || opener.open_path(&path)).await??;
        tracing::info!(
            message = message.0,
            index,
            executable = attachment.info.executable,
            "attachment opened"
        );
        Ok(OpenAttachmentResult::Opened)
    }

    // -----------------------------------------------------------------------------------------
    // Prefetch and remote-content settings
    // -----------------------------------------------------------------------------------------

    pub(crate) async fn set_prefetch_settings(
        &self,
        settings: PrefetchSettings,
    ) -> Result<PrefetchSettings, CoreError> {
        let settings = settings.validated()?;
        let stored = self.store.set_prefetch_settings(settings).await?;
        tracing::info!(period = ?stored.period, max = stored.max_message_size, "prefetch settings stored");
        self.prefetch.restart_all();
        Ok(stored)
    }

    pub(crate) fn prefetch_status(&self, account: AccountId) -> PrefetchStatus {
        self.prefetch.status(account)
    }

    /// Images the composer shows (a draft's inline images, a signature's pictures), re-encoded
    /// like the images of mails (C-4) in one batch and kept the same way: the web view decodes
    /// no image bytes the core did not write. `None` for one that cannot be shown. The draft or
    /// signature keeps and sends its own bytes.
    pub(crate) async fn editor_images(
        &self,
        images: Vec<mail_content::ImageBytes>,
    ) -> Vec<Option<String>> {
        let reencoded = self.images.reencode(images.clone()).await;
        images
            .iter()
            .map(|image| reencoded.get(image).map(str::to_owned))
            .collect()
    }

    /// Batches of mail images re-encoded so far (C-4; a cache hit runs none).
    #[cfg(test)]
    pub(crate) fn image_batches(&self) -> usize {
        self.images.batches()
    }
}

/// The kind of preview of an attachment, or `preview_unsupported`; refuses one that is
/// certainly too large before its bytes are fetched.
pub(crate) fn preview_kind_of(
    info: &mail_model::AttachmentInfo,
) -> Result<mail_model::PreviewKind, CoreError> {
    let kind = info.preview.ok_or_else(|| {
        mail_model::PreviewError::Unsupported(format!("{} ({})", info.name, info.mime_type))
    })?;
    // The size of a partial message's attachment is an estimate; the bytes are checked again.
    if info.size > mail_model::MAX_PREVIEW_INPUT_BYTES {
        return Err(mail_model::PreviewError::TooLarge(format!(
            "{} bytes, at most {}",
            info.size,
            mail_model::MAX_PREVIEW_INPUT_BYTES
        ))
        .into());
    }
    Ok(kind)
}

impl<C: MailConnector + 'static> Reading<C> {
    /// Decodes and encodes a preview: images and PDF in the helper process when there is one
    /// (finding C-2, decision 0043), text and everything without a helper on the blocking pool.
    pub(crate) async fn render_preview(
        &self,
        kind: mail_model::PreviewKind,
        bytes: Vec<u8>,
        page: u32,
    ) -> Result<mail_model::AttachmentPreview, CoreError> {
        match (&self.preview_helper, kind) {
            (Some(helper), mail_model::PreviewKind::Image | mail_model::PreviewKind::Pdf) => {
                Ok(mail_mcp::preview::render_isolated(
                    helper,
                    kind,
                    bytes,
                    page,
                    mail_mcp::preview::PreviewLimits::default(),
                )
                .await?)
            }
            _ => Ok(blocking(move || mail_content::render_preview(kind, &bytes, page)).await??),
        }
    }
}

/// Writes one file of "save all"; whether it got another name than `name`.
pub(crate) async fn save_into(
    folder: &Path,
    name: String,
    bytes: Vec<u8>,
) -> Result<bool, CoreError> {
    let folder = folder.to_path_buf();
    let written = blocking(move || {
        files::save_new_in(&folder, &name, &bytes).map(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy() != name)
        })
    })
    .await??;
    Ok(written)
}

/// The lists that show `message` changed (its preview).
async fn notify_lists(
    store: &Store,
    events: &broadcast::Sender<CoreEvent>,
    account: AccountId,
    message: MessageId,
) {
    match store.message_locations(message).await {
        Ok(locations) => {
            for location in locations {
                emit(
                    events,
                    CoreEvent::MessagesChanged {
                        account_id: account,
                        mailbox_id: location.mailbox_id,
                    },
                );
            }
        }
        Err(err) => tracing::debug!(error = %err, "locations for the list update unreadable"),
    }
}
