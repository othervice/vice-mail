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
//! **Attachments** are decoded from the stored body; of a partial body, the sections are
//! fetched first. Saving goes to the path the shell's dialog returned ([`files`]); opening
//! writes a read-only copy into the app's own directory and asks for confirmation for
//! executable types first.
//!
//! **Prefetch** ([`prefetch`]) runs per account after each completed sync pass.

mod fetch;
mod files;
pub mod index_text;
mod links;
pub mod opener;
mod prefetch;
pub mod remote;
mod view;

#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_content::{RemotePolicy, RenderOptions, render_html, render_text};
use mail_model::{
    AccountId, AccountState, BodyFormat, BodySection, BodyUnavailableReason, CoreError, CoreEvent,
    IpcError, LinkInfo, MailConnector, MailboxRole, MessageBody, MessageId, MessageView,
    OpenAttachmentResult, PrefetchSettings, PrefetchStatus, RemoteContentScope, RenderToken,
};
use mail_store::{Store, StoredMessage};
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
use links::{Rendering, Renderings};
use prefetch::Prefetcher;
use view::{Attachment, AttachmentSource, Content};

/// Delay of the start-up sweep of body files.
pub const SWEEP_DELAY: Duration = Duration::from_secs(5);

/// What reading gets from the shell. The default is the product's.
pub struct ReadingServices {
    /// Opens links and attachments with the system's default applications.
    pub opener: Arc<dyn SystemOpener>,
    pub remote: RemoteConfig,
    /// Directory for the copies of opened attachments, emptied at every start. `None`:
    /// `<data dir>/open-attachments`. The shell passes its cache directory.
    pub open_dir: Option<PathBuf>,
}

impl Default for ReadingServices {
    fn default() -> Self {
        Self {
            opener: Arc::new(DesktopOpener),
            remote: RemoteConfig::default(),
            open_dir: None,
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
    fetching: Mutex<HashSet<MessageId>>,
    pub(crate) prefetch: Prefetcher,
    listener: Mutex<Option<JoinHandle<()>>>,
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
            fetching: Mutex::default(),
            prefetch: Prefetcher::default(),
            listener: Mutex::default(),
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
    ) -> Result<MessageView, CoreError> {
        let summary = self.summary(message).await?;
        if let Some(content) = self.load_content(message).await? {
            self.backfill_preview(&summary, &content);
            return self
                .render(summary, content, format, Remote::IfAllowed)
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
        self.render(summary, content, BodyFormat::Html, Remote::Load)
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
    ) -> Result<MessageView, CoreError> {
        let locations = self.store.message_locations(summary.id).await?;
        let own_mail = locations
            .iter()
            .any(|l| matches!(l.role, MailboxRole::Sent | MailboxRole::Drafts));
        let token = Renderings::new_token();
        let header = content.header(&summary, own_mail);
        let formats = content.formats();
        let problems = content.problems();
        let attachments = content.attachments.iter().map(|a| a.info.clone()).collect();
        let partial = content.partial;

        let (body, links) = if content.shows_html(format) {
            let load = match remote {
                Remote::Load => true,
                Remote::IfAllowed => match &summary.from {
                    Some(from) => {
                        self.store
                            .remote_content_allowed(summary.account_id, &from.email)
                            .await?
                    }
                    None => false,
                },
            };
            let html = content.parsed.html.clone().unwrap_or_default();
            let mut options = RenderOptions::new(token.clone());
            options.inline = content.parsed.inline_images();
            let (mut rendered, html, options) = blocking(move || {
                let rendered = render_html(&html, &options);
                (rendered, html, options)
            })
            .await?;
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
                    let mut options = options;
                    options.remote = RemotePolicy::Allow(images);
                    rendered = blocking(move || render_html(&html, &options)).await?;
                }
            }
            (
                MessageBody::Html {
                    srcdoc: rendered.srcdoc,
                    render_token: token.clone(),
                    blocked_remote: rendered.blocked_count,
                    remote_loaded: load,
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
                account: summary.account_id,
                message: summary.id,
                links,
            },
        );
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

    pub(crate) fn describe_link(
        &self,
        token: &RenderToken,
        index: u32,
    ) -> Result<LinkInfo, CoreError> {
        let (link, check) = self.renderings.link(token, index)?;
        Ok(links::link_info(&link, &check))
    }

    pub(crate) async fn open_link(&self, token: &RenderToken, index: u32) -> Result<(), CoreError> {
        let (_, check) = self.renderings.link(token, index)?;
        let url = check.open_url.ok_or_else(|| {
            CoreError::InvalidArgument(format!(
                "only http, https and mailto links open (this one is {:?})",
                check.scheme
            ))
        })?;
        let opener = Arc::clone(&self.opener);
        blocking(move || opener.open_url(&url)).await??;
        tracing::info!(scheme = %check.scheme, host = check.host.as_deref(), "link opened in the system");
        Ok(())
    }

    /// The navigation hook saw a click on a link placeholder: tells the UI, if the token is
    /// one of ours. Returns whether it was.
    pub(crate) fn link_activated(&self, token: RenderToken, index: u32) -> bool {
        let Some(rendering) = self.renderings.get(&token) else {
            tracing::warn!(index, "click on a link of an unknown rendering ignored");
            return false;
        };
        tracing::debug!(message = rendering.message.0, index, "link clicked");
        self.emit(CoreEvent::LinkActivated {
            render_token: token,
            index,
        });
        true
    }

    // -----------------------------------------------------------------------------------------
    // Attachments
    // -----------------------------------------------------------------------------------------

    async fn attachment(&self, message: MessageId, index: u32) -> Result<Attachment, CoreError> {
        let content = self.load_content(message).await?.ok_or_else(|| {
            CoreError::NotFound(format!("the body of message {message} is not stored"))
        })?;
        content
            .attachments
            .into_iter()
            .find(|a| a.info.index == index)
            .ok_or_else(|| CoreError::NotFound(format!("attachment {index} of message {message}")))
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
