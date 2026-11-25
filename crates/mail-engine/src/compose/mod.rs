//! Composing and sending (stage 5, decision 0018): drafts, sending through the outbox,
//! the outbox view, compose settings, identities with text signatures and the contact
//! directory for autocomplete.
//!
//! - **Drafts** are local rows saved by the composer every few seconds (`saveDraft`: the
//!   composer owns the content, the core cannot pull it; a save is one small transaction, so a
//!   crash loses at most the last seconds). After [`ComposeConfig::draft_save_delay`] without a
//!   save, and at once when the composer closes, the newest version goes to the Drafts folder
//!   through the outbox (`append`, the old version removed). Read-only accounts keep drafts
//!   local.
//! - **Sending** freezes the draft and writes its bytes (built once: the Message-ID and the
//!   date stay for every attempt), then queues the `send` operation after the undo delay.
//!   Cancelling within the delay deletes it; nothing reached the network. The outbox submits,
//!   and after acceptance appends the Sent copy (unless the server files it), removes the
//!   draft's copy and marks the original `\Answered`/`$Forwarded`.
//! - **Attachments** are copied into the compose directory when added, from a path the shell
//!   took from the system file dialog or a drag-and-drop event of the window (never a path the
//!   page made up, see `app/src-tauri/src/drops.rs`).
//!
//! [`ComposeConfig::draft_save_delay`]: crate::outbox::ComposeConfig::draft_save_delay

pub(crate) mod build;
pub(crate) mod contacts;
mod edit_as_new;
pub mod files;
mod inline;
pub(crate) mod outgoing;
mod receipts;
mod redirect;
mod schedule;
pub(crate) mod snippets;
mod stage10;
#[cfg(test)]
mod stage10_tests;
#[cfg(test)]
mod stage19_tests;
#[cfg(test)]
mod stage20_tests;

#[cfg(test)]
pub(crate) mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use mail_content::{Original, QuoteOptions, ReplyKind};
use mail_model::{
    AccountId, AccountSendSettings, ActionError, Address, BodyFormat, ComposeFormat,
    ComposeIdentity, ComposeSettings, ContactSuggestion, CoreError, CoreEvent, Draft, DraftError,
    DraftFile, DraftId, DraftKind, DraftServerState, DraftState, DraftSummary, DraftUpdate,
    IdentityId, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, MAX_CONTACT_SUGGESTIONS,
    MAX_DRAFT_BODY_BYTES, MAX_INLINE_IMAGE_BYTES, MAX_RECIPIENTS, MAX_SEND_DELAY_SECS,
    MAX_SUBJECT_CHARS, MailScheme, MailboxRole, MessageBody, MessageId, NewDraft, OperationId,
    OperationPayload, OriginalRef, OutgoingMail, QuoteLanguage, QuotePosition, SendJob, SendPhase,
    SentCopyMode, ServerCopy, TextSegment, UnclearDecision,
};
use mail_store::{DraftContent, GLOBAL_SCOPE, NewDraftRow, Store, StoredDraft};
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;

use crate::clock::{Clock, SystemClock};
use crate::core::Connector;
use crate::outbox::{ComposeConfig, Outbox};
use crate::reading::Reading;

/// Setting key of the undo delay (global scope, JSON number of seconds).
const SEND_DELAY_KEY: &str = "compose.send_delay_seconds";
/// Stage 8 (decision 0025): the position of the answer in new replies and forwards.
const QUOTE_POSITION_KEY: &str = "compose.default_quote_position";
/// Stage 16 (decision 0033): new replies start with "send and archive" on.
const ARCHIVE_ON_REPLY_KEY: &str = "compose.archive_on_reply";
/// Stage 16: how read-receipt requests are handled (`ask` or `never`).
const READ_RECEIPTS_KEY: &str = "compose.read_receipts";
/// Stage 20 (decision 0058): the attachment reminder before sending (JSON boolean).
const ATTACHMENT_REMINDER_KEY: &str = "compose.attachment_reminder";
/// Longest request id of a send (the UI sends a UUID).
const MAX_REQUEST_ID: usize = 64;
/// How long creating a reply waits for the original's body to be fetched.
const BODY_WAIT: Duration = Duration::from_secs(30);
/// Drafts saved after a restart wait this long before going to the server.
const RESTART_SAVE_DELAY: Duration = Duration::from_secs(2);

fn now_ms() -> i64 {
    SystemClock.now_ms()
}

fn random_u128() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

fn random_bytes() -> [u8; 16] {
    *uuid::Uuid::new_v4().as_bytes()
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, CoreError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| CoreError::Internal(format!("compose task failed: {err}")))
}

/// Whether the account's server files sent mail itself when the user did not choose: Gmail.
pub(crate) fn files_sent_itself(imap_host: &str) -> bool {
    mail_model::is_gmail_host(imap_host)
}

fn valid_request_id(request: &str) -> Result<(), CoreError> {
    if request.is_empty()
        || request.len() > MAX_REQUEST_ID
        || !request
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(CoreError::InvalidArgument(
            "the request id must be 1 to 64 letters, digits, '-' or '_'".into(),
        ));
    }
    Ok(())
}

/// A MIME type from the file name's extension; `application/octet-stream` otherwise.
fn mime_of(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "ics" => "text/calendar",
        "vcf" => "text/vcard",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "eml" => "message/rfc822",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// Addresses as the composer sent them: trimmed, empty ones dropped.
fn clean_addresses(list: &[Address]) -> Vec<Address> {
    list.iter()
        .filter_map(|a| {
            let email = a.email.trim().to_owned();
            if email.is_empty() {
                return None;
            }
            let name = a
                .name
                .as_deref()
                .map(|n| n.chars().filter(|c| !c.is_control()).collect::<String>())
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty());
            Some(Address { name, email })
        })
        .collect()
}

/// The checked editable fields of a save.
fn content_of(update: &DraftUpdate) -> Result<DraftContent, CoreError> {
    let to = clean_addresses(&update.to);
    let cc = clean_addresses(&update.cc);
    let bcc = clean_addresses(&update.bcc);
    if to.len() + cc.len() + bcc.len() > MAX_RECIPIENTS {
        return Err(CoreError::InvalidArgument(format!(
            "at most {MAX_RECIPIENTS} recipients"
        )));
    }
    if update.body_html.len() + update.body_text.len() > MAX_DRAFT_BODY_BYTES {
        return Err(CoreError::InvalidArgument(format!(
            "the message text is larger than {MAX_DRAFT_BODY_BYTES} bytes"
        )));
    }
    let subject: String = update
        .subject
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_SUBJECT_CHARS)
        .collect();
    Ok(DraftContent {
        identity: update.identity_id,
        to,
        cc,
        bcc,
        subject,
        format: update.format,
        body_html: update.body_html.clone(),
        body_text: update.body_text.clone(),
        include_quote: update.include_quote,
        quote_position: update.quote_position,
        include_signature: update.include_signature,
        signature: update.signature_id.clone(),
    })
}

/// `<subject>.eml` for a message forwarded as an attachment (`Message.eml` without subject).
fn forwarded_file_name(subject: &str) -> String {
    let base: String = subject
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(120)
        .collect();
    let base = base.trim();
    let base = if base.is_empty() { "Message" } else { base };
    mail_content::sanitize_filename(&format!("{base}.eml"))
}

/// The plain text of a rendered text body.
fn segments_text(segments: &[TextSegment]) -> String {
    segments
        .iter()
        .map(|s| match s {
            TextSegment::Text { text } | TextSegment::Link { text, .. } => text.as_str(),
        })
        .collect()
}

/// A header line value with quotes and control characters made harmless.
fn header_value(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn header_address(address: &Address) -> String {
    match address.name.as_deref().filter(|n| !n.is_empty()) {
        Some(name) => format!(
            "\"{}\" <{}>",
            header_value(name).replace(['\\', '"'], ""),
            header_value(&address.email)
        ),
        None => format!("<{}>", header_value(&address.email)),
    }
}

/// A raw message from stored header data and a plain text: the original of a reply when only
/// part of a large message is stored.
fn synthetic_original(summary: &mail_store::StoredMessage, text: &str) -> Vec<u8> {
    let mut raw = String::new();
    if let Some(from) = &summary.from {
        raw.push_str(&format!("From: {}\r\n", header_address(from)));
    }
    let list = |list: &[Address]| {
        list.iter()
            .map(header_address)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !summary.to.is_empty() {
        raw.push_str(&format!("To: {}\r\n", list(&summary.to)));
    }
    if !summary.cc.is_empty() {
        raw.push_str(&format!("Cc: {}\r\n", list(&summary.cc)));
    }
    raw.push_str(&format!("Subject: {}\r\n", header_value(&summary.subject)));
    if let Some(id) = &summary.message_id_header {
        raw.push_str(&format!("Message-ID: <{}>\r\n", header_value(id)));
    }
    if let Ok(date) = jiff::Timestamp::from_second(summary.date) {
        raw.push_str(&format!(
            "Date: {}\r\n",
            date.strftime("%a, %d %b %Y %H:%M:%S +0000")
        ));
    }
    raw.push_str("MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n");
    raw.push_str(&text.replace("\r\n", "\n").replace('\n', "\r\n"));
    raw.into_bytes()
}

fn utc_offset_minutes() -> i32 {
    jiff::tz::TimeZone::system()
        .to_offset(jiff::Timestamp::now())
        .seconds()
        / 60
}

/// The composing service behind [`crate::AppCore`]'s compose commands.
pub(crate) struct Compose<C: Connector> {
    inner: Arc<Inner<C>>,
}

/// What [`Compose::copy_content`] took over of a message.
struct CopiedContent {
    format: ComposeFormat,
    body_html: String,
    body_text: String,
    attachments: Vec<DraftFile>,
    inline_images: usize,
    lossy: bool,
}

impl<C: Connector> Clone for Compose<C> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct Inner<C: Connector> {
    store: Store,
    outbox: Outbox<C>,
    reading: Arc<Reading<C>>,
    events: broadcast::Sender<CoreEvent>,
    root: PathBuf,
    config: ComposeConfig,
    /// Pending server saves: one timer per draft, restarted with every local save.
    timers: Mutex<HashMap<DraftId, JoinHandle<()>>>,
    contacts_kick: Arc<Notify>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    /// Stage 10: the core's clock (when a scheduled send is due, missed; tests move it).
    clock: Arc<dyn Clock>,
    /// Stage 10: the SMTP SIZE limit per account, as last read in this session.
    limits: Mutex<HashMap<AccountId, schedule::KnownLimit>>,
}

impl<C: Connector> Drop for Inner<C> {
    fn drop(&mut self) {
        for (_, timer) in self
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            timer.abort();
        }
        for task in self
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            task.abort();
        }
    }
}

impl<C: Connector> Compose<C> {
    pub(crate) fn new(
        store: Store,
        outbox: Outbox<C>,
        reading: Arc<Reading<C>>,
        events: broadcast::Sender<CoreEvent>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let root = files::compose_dir(&store);
        let config = outbox.compose_config().clone();
        Self {
            inner: Arc::new(Inner {
                store,
                outbox,
                reading,
                events,
                root,
                config,
                timers: Mutex::default(),
                contacts_kick: Arc::new(Notify::new()),
                tasks: Mutex::default(),
                clock,
                limits: Mutex::default(),
            }),
        }
    }

    /// Sweeps leftover files, schedules drafts that never reached the server (a crash, an
    /// offline quit) and starts the contact directory's job.
    pub(crate) async fn start(&self) {
        {
            let lock = self.inner.outbox.files_lock();
            let _held = lock.lock().await;
            files::sweep(&self.inner.store).await;
        }
        match self.inner.store.drafts_to_save().await {
            Ok(drafts) => {
                for draft in drafts {
                    self.schedule(draft, RESTART_SAVE_DELAY);
                }
            }
            Err(err) => tracing::warn!(error = %err, "drafts to save unreadable"),
        }
        let job = contacts_job(
            Arc::downgrade(&self.inner),
            Arc::clone(&self.inner.contacts_kick),
            self.inner.config.contacts_delay,
        );
        let listener = contacts_listener(
            self.inner.events.subscribe(),
            Arc::clone(&self.inner.contacts_kick),
        );
        let mut tasks = self
            .inner
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        tasks.push(job);
        tasks.push(listener);
    }

    /// Stops timers and background work (the app quits). Unsaved server copies are saved at
    /// the next start; the local drafts are complete.
    pub(crate) fn shutdown(&self) {
        for (_, timer) in self
            .inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            timer.abort();
        }
        for task in self
            .inner
            .tasks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
        {
            task.abort();
        }
    }

    fn emit(&self, event: CoreEvent) {
        crate::sync::emit(&self.inner.events, event);
    }

    fn drafts_changed(&self, account: AccountId, changed: Vec<DraftId>, removed: Vec<DraftId>) {
        self.emit(CoreEvent::DraftsChanged {
            account_id: account,
            changed,
            removed,
        });
    }

    // -- scheduling of server saves ------------------------------------------------------

    fn schedule(&self, draft: DraftId, delay: Duration) {
        let weak = Arc::downgrade(&self.inner);
        let id = draft.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Some(inner) = weak.upgrade() {
                Compose { inner }.save_to_server(&id).await;
            }
        });
        if let Some(old) = self
            .inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(draft, timer)
        {
            old.abort();
        }
    }

    fn unschedule(&self, draft: &DraftId) {
        if let Some(old) = self
            .inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(draft)
        {
            old.abort();
        }
    }

    /// Queues the newest version for the Drafts folder (nothing if writes are off, there is
    /// no Drafts folder or the server has this version).
    async fn save_to_server(&self, draft: &DraftId) {
        self.inner
            .timers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(draft);
        match self
            .inner
            .store
            .enqueue_draft_save(draft.clone(), uuid::Uuid::new_v4().to_string())
            .await
        {
            Ok(Some(change)) => self.inner.outbox.changed(&change),
            Ok(None) => {}
            Err(err) => tracing::warn!(error = %err, "a draft could not be queued for the server"),
        }
    }

    // -- views -----------------------------------------------------------------------------

    async fn info(&self, draft: StoredDraft) -> Result<Draft, CoreError> {
        let store = &self.inner.store;
        let local_only = match store.account(draft.account).await? {
            Some(account) => {
                !account.writes_enabled
                    || store
                        .mailbox_with_role(draft.account, MailboxRole::Drafts)
                        .await?
                        .is_none()
            }
            None => true,
        };
        let server = if draft.saving {
            DraftServerState::Saving
        } else if draft.server_copy.is_some() && draft.server_revision >= Some(draft.revision) {
            DraftServerState::Saved
        } else if local_only {
            DraftServerState::LocalOnly
        } else {
            DraftServerState::Pending
        };
        Ok(Draft {
            id: draft.id,
            account_id: draft.account,
            identity_id: draft.identity,
            kind: draft.kind,
            to: draft.to,
            cc: draft.cc,
            bcc: draft.bcc,
            subject: draft.subject,
            format: draft.format,
            body_html: draft.body_html,
            body_text: draft.body_text,
            quote_html: draft.quote_html,
            quote_text: draft.quote_text,
            include_quote: draft.include_quote,
            quote_position: draft.quote_position,
            include_signature: draft.include_signature,
            signature_id: draft.signature.clone(),
            attachments: draft.attachments.iter().map(DraftFile::info).collect(),
            original_message_id: draft.original,
            revision: draft.revision,
            state: draft.state,
            server,
            outgoing: draft.outgoing,
            archive_original: draft.archive_original,
            request_read_receipt: draft.request_receipt,
            follow_up_at: draft.follow_up_at,
            original_kept: draft.kept_original.is_some(),
            created_at: draft.created_at,
            updated_at: draft.updated_at,
        })
    }

    async fn stored(&self, id: &DraftId) -> Result<StoredDraft, CoreError> {
        self.inner
            .store
            .draft(id.clone())
            .await?
            .ok_or_else(|| CoreError::Draft(DraftError::NotFound(id.clone())))
    }

    pub(crate) async fn get(&self, id: DraftId) -> Result<Draft, CoreError> {
        let draft = self.stored(&id).await?;
        self.info(draft).await
    }

    /// Local drafts of one account, or of all, newest change first.
    pub(crate) async fn list(
        &self,
        account: Option<AccountId>,
    ) -> Result<Vec<DraftSummary>, CoreError> {
        let store = &self.inner.store;
        let accounts = match account {
            Some(id) => {
                if store.account(id).await?.is_none() {
                    return Err(CoreError::NotFound(format!("account {id}")));
                }
                vec![id]
            }
            None => store.accounts().await?.into_iter().map(|a| a.id).collect(),
        };
        let mut out = Vec::new();
        for account in accounts {
            for draft in store.drafts(account, mail_store::MAX_DRAFTS_PAGE).await? {
                out.push(DraftSummary {
                    id: draft.id,
                    account_id: draft.account,
                    kind: draft.kind,
                    subject: draft.subject,
                    to: draft.to.into_iter().take(5).collect(),
                    state: draft.state,
                    attachment_count: u32::try_from(draft.attachments.len()).unwrap_or(u32::MAX),
                    updated_at: draft.updated_at,
                });
            }
        }
        out.sort_by_key(|d| std::cmp::Reverse(d.updated_at));
        Ok(out)
    }

    // -- creating --------------------------------------------------------------------------

    pub(crate) async fn create(&self, request: NewDraft) -> Result<Draft, CoreError> {
        let draft = match request {
            NewDraft::New {
                account_id,
                identity_id,
                to,
            } => self.create_new(account_id, identity_id, to).await?,
            NewDraft::Reply {
                message_id,
                language,
            } => {
                self.create_answer(message_id, DraftKind::Reply, language)
                    .await?
            }
            NewDraft::ReplyAll {
                message_id,
                language,
            } => {
                self.create_answer(message_id, DraftKind::ReplyAll, language)
                    .await?
            }
            NewDraft::Forward {
                message_id,
                language,
            } => {
                self.create_answer(message_id, DraftKind::Forward, language)
                    .await?
            }
            NewDraft::ForwardAsAttachment {
                message_id,
                language,
            } => {
                self.create_forward_as_attachment(message_id, language)
                    .await?
            }
            NewDraft::Compose {
                account_id,
                identity_id,
                request,
            } => {
                self.create_from_request(account_id, identity_id, request)
                    .await?
            }
            NewDraft::EditAsNew { message_id } => self.create_edit_as_new(message_id).await?,
        };
        // Stage 16: "send and archive" as the compose setting says, for replies.
        let draft = if matches!(draft.kind, DraftKind::Reply | DraftKind::ReplyAll)
            && draft.original_message_id_header.is_some()
            && self.compose_settings().await?.archive_on_reply
        {
            self.inner
                .store
                .set_draft_send_options(
                    draft.id.clone(),
                    true,
                    draft.request_receipt,
                    draft.follow_up_at,
                )
                .await?
        } else {
            draft
        };
        tracing::info!(account = %draft.account, kind = draft.kind.as_str(), "draft created");
        self.drafts_changed(draft.account, vec![draft.id.clone()], Vec::new());
        self.info(draft).await
    }

    fn empty_content(
        identity: IdentityId,
        to: Vec<Address>,
        format: ComposeFormat,
    ) -> DraftContent {
        DraftContent {
            identity: Some(identity),
            to,
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            format,
            body_html: String::new(),
            body_text: String::new(),
            include_quote: true,
            quote_position: QuotePosition::Above,
            include_signature: true,
            signature: None,
        }
    }

    async fn create_new(
        &self,
        account: AccountId,
        identity: Option<IdentityId>,
        to: Vec<Address>,
    ) -> Result<StoredDraft, CoreError> {
        let store = &self.inner.store;
        if store.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        let identities = store.compose_identities(account).await?;
        let chosen = match identity {
            Some(id) => identities.iter().find(|(i, _)| i.id == id).ok_or_else(|| {
                CoreError::Draft(DraftError::NoIdentity(format!(
                    "identity {} is not one of the account's",
                    id.0
                )))
            })?,
            None => identities
                .iter()
                .find(|(i, _)| i.is_default)
                .or_else(|| identities.first())
                .ok_or_else(|| {
                    CoreError::Draft(DraftError::NoIdentity("the account has none".into()))
                })?,
        };
        let id = DraftId::from_random(random_u128());
        let message_id = mail_content::new_message_id(&chosen.0.email, random_bytes());
        let row = NewDraftRow {
            id,
            account,
            identity: Some(chosen.0.id),
            kind: DraftKind::New,
            message_id_header: message_id,
            original: None,
            original_message_id_header: None,
            in_reply_to: None,
            references: Vec::new(),
            content: Self::empty_content(chosen.0.id, clean_addresses(&to), ComposeFormat::Html),
            quote_html: None,
            quote_text: None,
            attachments: Vec::new(),
            server_copy: None,
            kept_original: None,
            now: now_ms(),
        };
        Ok(store.create_draft(row).await?)
    }

    /// Stage 10: a new draft with the fields of a compose request (a `mailto:` link), checked
    /// like a save; the body is plain text, put in as editor HTML; `In-Reply-To` and
    /// `References` thread it.
    async fn create_from_request(
        &self,
        account: AccountId,
        identity: Option<IdentityId>,
        request: mail_model::ComposeRequest,
    ) -> Result<StoredDraft, CoreError> {
        let in_reply_to = match request.in_reply_to.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(id) => {
                let id = id.trim_start_matches('<').trim_end_matches('>');
                let usable = id.len() <= 250
                    && id.contains('@')
                    && id
                        .chars()
                        .all(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>' | '"' | '\\'));
                if !usable {
                    return Err(CoreError::InvalidArgument(
                        "inReplyTo is not one valid Message-ID".into(),
                    ));
                }
                Some(id.to_owned())
            }
        };
        let store = &self.inner.store;
        if store.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        let identities = store.compose_identities(account).await?;
        let chosen = match identity {
            Some(id) => identities.iter().find(|(i, _)| i.id == id).ok_or_else(|| {
                CoreError::Draft(DraftError::NoIdentity(format!(
                    "identity {} is not one of the account's",
                    id.0
                )))
            })?,
            None => identities
                .iter()
                .find(|(i, _)| i.is_default)
                .or_else(|| identities.first())
                .ok_or_else(|| {
                    CoreError::Draft(DraftError::NoIdentity("the account has none".into()))
                })?,
        };
        let body = request.body.unwrap_or_default();
        let content = content_of(&DraftUpdate {
            identity_id: Some(chosen.0.id),
            to: request.to,
            cc: request.cc,
            bcc: request.bcc,
            subject: request.subject.unwrap_or_default(),
            format: ComposeFormat::Html,
            body_html: if body.trim().is_empty() {
                String::new()
            } else {
                mail_content::text_to_editor_html(&body)
            },
            body_text: String::new(),
            include_quote: true,
            quote_position: QuotePosition::Above,
            include_signature: true,
            signature_id: None,
        })?;
        let row = NewDraftRow {
            id: DraftId::from_random(random_u128()),
            account,
            identity: Some(chosen.0.id),
            kind: DraftKind::New,
            message_id_header: mail_content::new_message_id(&chosen.0.email, random_bytes()),
            original: None,
            original_message_id_header: None,
            in_reply_to: in_reply_to.clone(),
            references: in_reply_to.into_iter().collect(),
            content,
            quote_html: None,
            quote_text: None,
            attachments: Vec::new(),
            server_copy: None,
            kept_original: None,
            now: now_ms(),
        };
        Ok(store.create_draft(row).await?)
    }

    /// The raw original, fetched if it is not stored; `None` when only part of it can be had
    /// (a large message): then `text` is its plain text from the reading pane.
    async fn original_raw(
        &self,
        message: MessageId,
    ) -> Result<(Option<Vec<u8>>, Option<String>), CoreError> {
        let store = &self.inner.store;
        if let Some(raw) = store.load_message(message).await? {
            return Ok((Some(raw), None));
        }
        let mut events = self.inner.events.subscribe();
        let view = self
            .inner
            .reading
            .get_message(message, BodyFormat::Text, MailScheme::Light)
            .await?;
        if let MessageBody::Text { segments, .. } = &view.body {
            return Ok((
                store.load_message(message).await?,
                Some(segments_text(segments)),
            ));
        }
        if matches!(view.body, MessageBody::Loading) {
            let waited = tokio::time::timeout(BODY_WAIT, async {
                loop {
                    match events.recv().await {
                        Ok(CoreEvent::MessageBodyAvailable { message_id, .. })
                            if message_id == message =>
                        {
                            return true;
                        }
                        Ok(CoreEvent::MessageBodyFailed { message_id, .. })
                            if message_id == message =>
                        {
                            return false;
                        }
                        Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return false,
                    }
                }
            })
            .await;
            if waited == Ok(true) {
                if let Some(raw) = store.load_message(message).await? {
                    return Ok((Some(raw), None));
                }
                let view = self
                    .inner
                    .reading
                    .get_message(message, BodyFormat::Text, MailScheme::Light)
                    .await?;
                if let MessageBody::Text { segments, .. } = &view.body {
                    return Ok((None, Some(segments_text(segments))));
                }
            }
        }
        Ok((None, None))
    }

    async fn create_answer(
        &self,
        message: MessageId,
        kind: DraftKind,
        language: QuoteLanguage,
    ) -> Result<StoredDraft, CoreError> {
        self.create_answer_with(message, kind, language, false)
            .await
    }

    /// Stage 10: a forward with the original as a `message/rfc822` attachment.
    async fn create_forward_as_attachment(
        &self,
        message: MessageId,
        language: QuoteLanguage,
    ) -> Result<StoredDraft, CoreError> {
        self.create_answer_with(message, DraftKind::Forward, language, true)
            .await
    }

    async fn create_answer_with(
        &self,
        message: MessageId,
        kind: DraftKind,
        language: QuoteLanguage,
        as_attachment: bool,
    ) -> Result<StoredDraft, CoreError> {
        let store = &self.inner.store;
        let summary = store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let account = summary.account_id;
        let identities = store.identities(account).await?;
        if identities.is_empty() {
            return Err(CoreError::Draft(DraftError::NoIdentity(
                "the account has none".into(),
            )));
        }
        let (raw, text) = self.original_raw(message).await?;
        let whole = raw.is_some();
        if as_attachment && !whole {
            return Err(CoreError::InvalidArgument(
                "only part of this large message is stored; forward it inline instead".into(),
            ));
        }
        let original_bytes = if as_attachment { raw.clone() } else { None };
        let raw =
            raw.unwrap_or_else(|| synthetic_original(&summary, text.as_deref().unwrap_or("")));
        let options = QuoteOptions {
            language: match language {
                QuoteLanguage::En => mail_content::QuoteLanguage::English,
                QuoteLanguage::De => mail_content::QuoteLanguage::German,
            },
            utc_offset_minutes: utc_offset_minutes(),
        };
        let identities_for_work = identities.clone();
        let prepared = blocking(move || {
            let original = Original::parse(&raw);
            match kind {
                DraftKind::Forward => {
                    mail_content::prepare_forward(&original, &identities_for_work, options)
                }
                DraftKind::ReplyAll => mail_content::prepare_reply(
                    &original,
                    ReplyKind::All,
                    &identities_for_work,
                    options,
                ),
                _ => mail_content::prepare_reply(
                    &original,
                    ReplyKind::Sender,
                    &identities_for_work,
                    options,
                ),
            }
        })
        .await?;
        let identity = prepared
            .identity
            .or_else(|| identities.iter().find(|i| i.is_default).map(|i| i.id))
            .or_else(|| identities.first().map(|i| i.id))
            .ok_or_else(|| CoreError::Draft(DraftError::NoIdentity("none".into())))?;
        let sender_email = identities
            .iter()
            .find(|i| i.id == identity)
            .map(|i| i.email.clone())
            .unwrap_or_default();
        let id = DraftId::from_random(random_u128());

        // Forwarded attachments: the core's own copies. The sweep of compose files waits until
        // the draft row exists.
        let lock = self.inner.outbox.files_lock();
        let _held = lock.lock().await;
        let mut attachments = Vec::new();
        if let Some(bytes) = original_bytes {
            let file_id = "1".to_owned();
            let root = self.inner.root.clone();
            let draft = id.clone();
            let fid = file_id.clone();
            let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let sha256 =
                blocking(move || files::write_attachment(&root, &draft, &fid, &bytes)).await??;
            attachments.push(DraftFile {
                id: file_id.clone(),
                file_name: forwarded_file_name(&summary.subject),
                mime: "message/rfc822".to_owned(),
                size,
                sha256,
                file: files::attachment_path(&id, &file_id),
                content_id: None,
            });
        } else if kind == DraftKind::Forward {
            if whole {
                for (n, attachment) in prepared.attachments.iter().enumerate() {
                    let file_id = (n + 1).to_string();
                    let root = self.inner.root.clone();
                    let draft = id.clone();
                    let bytes = attachment.bytes.clone();
                    let fid = file_id.clone();
                    let sha256 =
                        blocking(move || files::write_attachment(&root, &draft, &fid, &bytes))
                            .await??;
                    attachments.push(DraftFile {
                        id: file_id.clone(),
                        file_name: mail_content::sanitize_filename(&attachment.file_name),
                        mime: attachment.mime.clone(),
                        size: u64::try_from(attachment.bytes.len()).unwrap_or(u64::MAX),
                        sha256,
                        file: files::attachment_path(&id, &file_id),
                        content_id: None,
                    });
                }
            } else {
                attachments = self.forward_partial_attachments(message, &id).await?;
            }
        }
        let mut content = Self::empty_content(identity, prepared.to.clone(), ComposeFormat::Html);
        content.cc = prepared.cc.clone();
        content.subject = prepared.subject.clone();
        content.quote_position = self.compose_settings().await?.default_quote_position;
        let row = NewDraftRow {
            id,
            account,
            identity: Some(identity),
            kind,
            message_id_header: mail_content::new_message_id(&sender_email, random_bytes()),
            original: Some(message),
            original_message_id_header: summary.message_id_header.clone(),
            in_reply_to: prepared.in_reply_to.clone(),
            references: prepared.references.clone(),
            content,
            quote_html: (!as_attachment).then(|| prepared.html.clone()),
            quote_text: (!as_attachment).then(|| prepared.text.clone()),
            attachments,
            server_copy: None,
            kept_original: None,
            now: now_ms(),
        };
        Ok(store.create_draft(row).await?)
    }

    /// The attachments of a message stored only in part, saved through the reading pane's
    /// attachment path (it fetches what is missing).
    async fn forward_partial_attachments(
        &self,
        message: MessageId,
        draft: &DraftId,
    ) -> Result<Vec<DraftFile>, CoreError> {
        let view = self
            .inner
            .reading
            .get_message(message, BodyFormat::Text, MailScheme::Light)
            .await?;
        let mut out = Vec::new();
        for (n, attachment) in view.attachments.iter().enumerate() {
            let file_id = (n + 1).to_string();
            let relative = files::attachment_path(draft, &file_id);
            let path = files::resolve(&self.inner.root, &relative)?;
            if let Some(parent) = path.parent() {
                let parent = parent.to_owned();
                blocking(move || std::fs::create_dir_all(parent))
                    .await?
                    .map_err(files::io_error("create draft directory"))?;
            }
            let saved = self
                .inner
                .reading
                .save_attachment_to(message, attachment.index, path)
                .await?;
            let bytes = {
                let saved = saved.clone();
                blocking(move || std::fs::read(saved))
                    .await?
                    .map_err(files::io_error("read forwarded attachment"))?
            };
            out.push(DraftFile {
                id: file_id,
                file_name: attachment.name.clone(),
                mime: attachment.mime_type.clone(),
                size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                sha256: files::sha256_hex(&bytes),
                file: relative,
                content_id: None,
            });
        }
        Ok(out)
    }

    /// Opens a message of the Drafts folder: the app's own draft with that Message-ID if there
    /// is one, otherwise a new local draft made from it (also from other programs: text, HTML
    /// through the allowlist with the draft's own inline images, attachments).
    ///
    /// If the draft can be taken over completely, it keeps its Message-ID and saving replaces
    /// the server copy. If not (an image from the web, a part the editor cannot show, a message
    /// stored only in part or damaged, no Message-ID), the copy in the folder is never touched:
    /// the draft gets a Message-ID of its own, is saved next to it, and says so
    /// (`Draft::original_kept`); opening that copy again finds this draft.
    pub(crate) async fn open_from_message(&self, message: MessageId) -> Result<Draft, CoreError> {
        let store = &self.inner.store;
        let summary = store
            .message_summary(message)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("message {message}")))?;
        let account = summary.account_id;
        if let Some(id) = &summary.message_id_header {
            if let Some(draft) = store.draft_for_message(account, id.clone()).await? {
                return self.info(draft).await;
            }
            if let Some(draft) = store.draft_for_kept_original(account, id.clone()).await? {
                return self.info(draft).await;
            }
        }
        let (raw, text) = self.original_raw(message).await?;
        // Only part of the message could be had: what is missing would be lost.
        let mut lossy = raw.is_none();
        let raw = match raw {
            Some(raw) => raw,
            None => synthetic_original(&summary, text.as_deref().unwrap_or("")),
        };
        let identities = store.identities(account).await?;
        let raw_for_work = raw.clone();
        let parsed = blocking(move || mail_content::parse(&raw_for_work)).await?;
        lossy |= parsed
            .problems
            .iter()
            .any(|p| !matches!(p, mail_content::ParseProblem::InvalidDate));
        let headers = &parsed.headers;
        let identity = mail_content::choose_identity(&identities, headers, &[])
            .map(|i| i.id)
            .ok_or_else(|| CoreError::Draft(DraftError::NoIdentity("none".into())))?;
        let id = DraftId::from_random(random_u128());
        let kept_id = headers
            .message_id
            .clone()
            .filter(|m| !m.is_empty() && m.contains('@') && m.is_ascii());
        let sender_email = identities
            .iter()
            .find(|i| i.id == identity)
            .map(|i| i.email.clone())
            .unwrap_or_default();

        let lock = self.inner.outbox.files_lock();
        let held = lock.lock().await;
        let copied = self.copy_content(&id, &raw, &parsed, &sender_email).await?;
        lossy |= copied.lossy;
        let (format, body_html, body_text, attachments) = (
            copied.format,
            copied.body_html,
            copied.body_text,
            copied.attachments,
        );
        let inline_images = copied.inline_images;
        let keep_original = lossy || kept_id.is_none();
        let message_id_header = match (&kept_id, keep_original) {
            (Some(kept), false) => kept.clone(),
            _ => mail_content::new_message_id(&sender_email, random_bytes()),
        };
        // The copy in the folder is replaced by the first save when the draft took it over
        // completely and it is identifiable by its Message-ID; otherwise it stays.
        let server_copy = match (&kept_id, keep_original) {
            (Some(kept), false) => store
                .message_locations(message)
                .await?
                .into_iter()
                .filter(|l| l.selectable)
                .find_map(|l| {
                    Some(ServerCopy {
                        mailbox: mail_model::TargetMailbox {
                            mailbox_id: l.mailbox_id,
                            name: l.mailbox_name.clone(),
                        },
                        uid_validity: l.uid_validity?,
                        uid: l.uid,
                        message_id_header: kept.clone(),
                    })
                }),
            _ => None,
        };
        let in_reply_to = headers.in_reply_to.first().cloned();
        let row = NewDraftRow {
            id,
            account,
            identity: Some(identity),
            kind: if in_reply_to.is_some() {
                DraftKind::Reply
            } else {
                DraftKind::New
            },
            message_id_header,
            original: None,
            original_message_id_header: in_reply_to.clone(),
            in_reply_to,
            references: headers.references.clone(),
            content: DraftContent {
                identity: Some(identity),
                to: headers.to.clone(),
                cc: headers.cc.clone(),
                bcc: headers.bcc.clone(),
                subject: headers.subject.clone().unwrap_or_default(),
                format,
                body_html,
                body_text,
                include_quote: false,
                quote_position: QuotePosition::Above,
                // The signature is part of the text already.
                include_signature: false,
                signature: None,
            },
            quote_html: None,
            quote_text: None,
            attachments,
            server_copy,
            kept_original: keep_original.then(|| kept_id.clone().unwrap_or_default()),
            now: now_ms(),
        };
        let draft = store.create_draft(row).await?;
        drop(held);
        tracing::info!(
            %account,
            inline_images,
            original_kept = keep_original,
            "draft opened from the Drafts folder"
        );
        self.drafts_changed(account, vec![draft.id.clone()], Vec::new());
        self.info(draft).await
    }

    /// The body and attachments of a parsed message as a draft's content (stage 9 drafts of
    /// other programs, stage 19 "edit as new"): HTML converted for the editor with the inline
    /// images it shows kept as parts (raster images within the size limit; ids the editor does
    /// not accept get a new one in the sender's domain), plain text otherwise, every other part
    /// copied as an attachment of draft `id`. `lossy` when something could not be taken over.
    /// The caller holds the compose files lock.
    async fn copy_content(
        &self,
        id: &DraftId,
        raw: &[u8],
        parsed: &mail_content::ParsedMessage,
        sender_email: &str,
    ) -> Result<CopiedContent, CoreError> {
        let mut lossy = false;
        // The draft's own inline images the editor can show keep their part: a raster image
        // within the size limit. Ids other programs write that the editor does not accept (or
        // the reader's own for unnamed body images) get a new one in the sender's domain.
        let domain = inline::content_id_domain(sender_email);
        let mut images: HashMap<String, String> = HashMap::new();
        let mut candidates: HashMap<String, &mail_content::InlinePart> = HashMap::new();
        for part in &parsed.inline {
            let fits = usize::try_from(MAX_INLINE_IMAGE_BYTES)
                .is_ok_and(|max| part.bytes.len() <= max)
                && inline::TYPES.contains(&part.mime);
            if !fits || images.contains_key(&part.content_id) {
                continue;
            }
            // The reader names body images without a Content-ID itself; only a header's id
            // is the draft's own.
            let own = mail_content::is_valid_content_id(&part.content_id)
                && parsed.attachments.iter().any(|a| {
                    a.part_id == part.part_id && a.content_id.as_deref() == Some(&part.content_id)
                });
            let content_id = if own {
                part.content_id.clone()
            } else {
                format!("{:032x}@{domain}", random_u128())
            };
            images.insert(part.content_id.clone(), content_id);
            candidates.insert(part.content_id.clone(), part);
        }
        let (format, body_html, body_text, shown) = match &parsed.html {
            Some(html) => {
                let html = html.clone();
                let map = images.clone();
                let converted =
                    blocking(move || mail_content::foreign_draft_html(&html, &map)).await?;
                lossy |= converted.dropped > 0;
                (
                    ComposeFormat::Html,
                    converted.editor_html,
                    String::new(),
                    converted.shown,
                )
            }
            None => (
                ComposeFormat::Text,
                String::new(),
                parsed.display_text().into_owned(),
                Vec::new(),
            ),
        };
        let shown_parts: Vec<&mail_content::InlinePart> = shown
            .iter()
            .filter_map(|original| candidates.get(original).copied())
            .collect();
        let inline_part_ids: Vec<u32> = shown_parts.iter().map(|p| p.part_id).collect();

        let mut attachments = Vec::new();
        let mut next = 0u32;
        for part in &parsed.attachments {
            // An inline image the HTML shows is a part of its own below, not an attachment.
            if inline_part_ids.contains(&part.part_id) {
                continue;
            }
            let Some(bytes) = mail_content::attachment_bytes(raw, part.part_id) else {
                // Cannot be copied: the draft would lose it, so the original stays.
                lossy = true;
                continue;
            };
            next += 1;
            let file_id = next.to_string();
            let root = self.inner.root.clone();
            let draft = id.clone();
            let fid = file_id.clone();
            let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let sha256 =
                blocking(move || files::write_attachment(&root, &draft, &fid, &bytes)).await??;
            attachments.push(DraftFile {
                id: file_id.clone(),
                file_name: part.file_name.clone(),
                mime: part.mime.clone(),
                size,
                sha256,
                file: files::attachment_path(id, &file_id),
                content_id: None,
            });
        }
        for (n, part) in shown_parts.iter().enumerate() {
            let Some(content_id) = images.get(&part.content_id).cloned() else {
                continue;
            };
            next += 1;
            let file_id = next.to_string();
            let bytes = part.bytes.to_vec();
            let root = self.inner.root.clone();
            let draft = id.clone();
            let fid = file_id.clone();
            let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let sha256 =
                blocking(move || files::write_attachment(&root, &draft, &fid, &bytes)).await??;
            let file_name = parsed
                .attachments
                .iter()
                .find(|a| a.part_id == part.part_id)
                .map(|a| a.file_name.clone())
                .unwrap_or_else(|| {
                    let extension = part.mime.strip_prefix("image/").unwrap_or("png");
                    format!("image-{}.{extension}", n + 1)
                });
            attachments.push(DraftFile {
                id: file_id.clone(),
                file_name,
                mime: part.mime.to_owned(),
                size,
                sha256,
                file: files::attachment_path(id, &file_id),
                content_id: Some(content_id),
            });
        }
        Ok(CopiedContent {
            format,
            body_html,
            body_text,
            attachments,
            inline_images: shown_parts.len(),
            lossy,
        })
    }

    // -- editing ---------------------------------------------------------------------------

    /// Saves the composer's fields locally (one transaction) and restarts the server timer.
    pub(crate) async fn save(&self, id: DraftId, update: DraftUpdate) -> Result<Draft, CoreError> {
        let content = content_of(&update)?;
        let draft = self.inner.store.save_draft(id.clone(), content).await?;
        self.schedule(id.clone(), self.inner.config.draft_save_delay);
        self.drafts_changed(draft.account, vec![id], Vec::new());
        self.info(draft).await
    }

    /// The composer closed: the newest version goes to the server now.
    pub(crate) async fn close(&self, id: DraftId) -> Result<Draft, CoreError> {
        let draft = self.stored(&id).await?;
        self.unschedule(&id);
        self.save_to_server(&id).await;
        let draft = self.inner.store.draft(id).await?.unwrap_or(draft);
        self.info(draft).await
    }

    /// Deletes the draft, its files and (through the outbox, with writes on) its server copy.
    pub(crate) async fn delete(&self, id: DraftId) -> Result<(), CoreError> {
        self.unschedule(&id);
        let (draft, change) = self
            .inner
            .store
            .delete_draft(id.clone(), uuid::Uuid::new_v4().to_string())
            .await?;
        self.inner.outbox.changed(&change);
        let root = self.inner.root.clone();
        blocking(move || files::remove_draft_dir(&root, &id)).await?;
        tracing::info!(account = %draft.account, "draft deleted");
        Ok(())
    }

    /// Copies files into the draft (paths from the system dialog or a drop on the window).
    pub(crate) async fn add_attachments(
        &self,
        id: DraftId,
        paths: Vec<PathBuf>,
    ) -> Result<Draft, CoreError> {
        let draft = self.stored(&id).await?;
        if draft.state != DraftState::Editing {
            return Err(CoreError::Draft(DraftError::Sending(id)));
        }
        if draft.attachments.len() + paths.len() > MAX_ATTACHMENTS {
            return Err(CoreError::InvalidArgument(format!(
                "at most {MAX_ATTACHMENTS} attachments"
            )));
        }
        let mut list = draft.attachments.clone();
        let mut used: u64 = list.iter().map(|a| a.size).sum();
        let mut next = list
            .iter()
            .filter_map(|a| a.id.parse::<u64>().ok())
            .max()
            .unwrap_or(0);
        let mut written: Vec<String> = Vec::new();
        for path in paths {
            next += 1;
            let file_id = next.to_string();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let limit = MAX_ATTACHMENT_BYTES.saturating_sub(used);
            let root = self.inner.root.clone();
            let draft_id = id.clone();
            let fid = file_id.clone();
            let copied =
                blocking(move || files::copy_in(&root, &draft_id, &fid, &path, limit)).await?;
            let (size, sha256) = match copied {
                Ok(done) => done,
                Err(err) => {
                    for relative in &written {
                        files::remove_file(&self.inner.root, relative);
                    }
                    return Err(match err {
                        CoreError::Draft(DraftError::AttachmentTooLarge { size, .. }) => {
                            CoreError::Draft(DraftError::AttachmentTooLarge {
                                size: used.saturating_add(size),
                                limit: MAX_ATTACHMENT_BYTES,
                            })
                        }
                        other => other,
                    });
                }
            };
            used = used.saturating_add(size);
            let relative = files::attachment_path(&id, &file_id);
            written.push(relative.clone());
            let file_name = mail_content::sanitize_filename(&name);
            list.push(DraftFile {
                id: file_id,
                mime: mime_of(&file_name).to_owned(),
                file_name,
                size,
                sha256,
                file: relative,
                content_id: None,
            });
        }
        let draft = match self
            .inner
            .store
            .set_draft_attachments(id.clone(), list)
            .await
        {
            Ok(draft) => draft,
            Err(err) => {
                for relative in &written {
                    files::remove_file(&self.inner.root, relative);
                }
                return Err(err.into());
            }
        };
        self.schedule(id.clone(), self.inner.config.draft_save_delay);
        self.drafts_changed(draft.account, vec![id], Vec::new());
        self.info(draft).await
    }

    pub(crate) async fn remove_attachment(
        &self,
        id: DraftId,
        attachment: String,
    ) -> Result<Draft, CoreError> {
        let draft = self.stored(&id).await?;
        let Some(removed) = draft
            .attachments
            .iter()
            .find(|a| a.id == attachment)
            .cloned()
        else {
            return Err(CoreError::NotFound(format!("attachment {attachment}")));
        };
        let list: Vec<DraftFile> = draft
            .attachments
            .iter()
            .filter(|a| a.id != attachment)
            .cloned()
            .collect();
        let draft = self
            .inner
            .store
            .set_draft_attachments(id.clone(), list)
            .await?;
        files::remove_file(&self.inner.root, &removed.file);
        self.schedule(id.clone(), self.inner.config.draft_save_delay);
        self.drafts_changed(draft.account, vec![id], Vec::new());
        self.info(draft).await
    }

    // -- sending ---------------------------------------------------------------------------

    async fn send_delay(&self) -> Result<u32, CoreError> {
        Ok(self.compose_settings().await?.send_delay_seconds)
    }

    /// Freezes the draft and queues its send after the undo delay (see module docs).
    pub(crate) async fn send(
        &self,
        id: DraftId,
        request: String,
    ) -> Result<OutgoingMail, CoreError> {
        self.send_at(id, request, None).await
    }

    /// Like [`Compose::send`]; with `at` (Unix ms) the send waits until then ("send later",
    /// stage 10): the message is dated `at`, and it goes out only if the app runs and is
    /// online at that time.
    pub(crate) async fn send_at(
        &self,
        id: DraftId,
        request: String,
        at: Option<i64>,
    ) -> Result<OutgoingMail, CoreError> {
        valid_request_id(&request)?;
        let store = &self.inner.store;
        let draft = self.stored(&id).await?;
        if draft.state != DraftState::Editing {
            if let Some(op) = draft.outgoing
                && let Some(op) = store.operation(op).await?
                && let Some(mail) = outgoing::outgoing_mail(&op, self.now())
            {
                // The same press of "send" twice: the send that is already queued.
                return Ok(mail);
            }
            return Err(CoreError::Draft(DraftError::Sending(id)));
        }
        let account = store
            .account(draft.account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {}", draft.account)))?;
        if !account.writes_enabled {
            return Err(CoreError::Action(ActionError::ReadOnly(account.id)));
        }
        let identities = store.compose_identities(draft.account).await?;
        let sender = build::sender_of(&draft, &identities).ok_or_else(|| {
            CoreError::Draft(DraftError::NoIdentity("choose a sender address".into()))
        })?;
        let sender = snippets::with_signature(store, &draft, sender).await?;
        for address in draft.to.iter().chain(&draft.cc).chain(&draft.bcc) {
            if mail_content::normalize_email(&address.email).is_err() {
                return Err(CoreError::Draft(DraftError::InvalidAddress(
                    address.email.clone(),
                )));
            }
        }
        if draft.to.is_empty() && draft.cc.is_empty() && draft.bcc.is_empty() {
            return Err(CoreError::Draft(DraftError::NoRecipients));
        }
        let size: u64 = draft.attachments.iter().map(|a| a.size).sum();
        if size > MAX_ATTACHMENT_BYTES {
            return Err(CoreError::Draft(DraftError::AttachmentTooLarge {
                size,
                limit: MAX_ATTACHMENT_BYTES,
            }));
        }
        let mode = store.sent_copy_mode(draft.account).await?;
        let server_saves_sent = match mode {
            SentCopyMode::Server => true,
            SentCopyMode::App => false,
            // Stage 14 (decision 0034): Gmail by host name or by X-GM-EXT-1 at the last login.
            SentCopyMode::Auto => {
                files_sent_itself(&account.imap.host)
                    || store.account_is_gmail(draft.account).await?
            }
        };
        let delay = self.send_delay().await?;
        let now = self.now();
        if let Some(at) = at {
            schedule::check_time(at, now)?;
        }

        // Built once: the bytes are what every attempt sends. A scheduled message is dated
        // with its send time.
        let date = at.unwrap_or(now) / 1000;
        let root = self.inner.root.clone();
        let for_build = draft.clone();
        let built = blocking(move || {
            let files = build::files_of(&root, &for_build)?;
            let message = build::outgoing(&for_build, &sender, files, date);
            mail_content::build_message(&message).map_err(build::compose_error)
        })
        .await??;
        self.refuse_too_large(draft.account, built.for_sending.len())?;
        let to: Vec<Address> = draft
            .to
            .iter()
            .chain(&draft.cc)
            .chain(&draft.bcc)
            .take(5)
            .cloned()
            .collect();
        let original = match (draft.kind, &draft.original_message_id_header) {
            (DraftKind::Reply | DraftKind::ReplyAll, Some(id)) => Some(OriginalRef {
                message_id_header: id.clone(),
                forwarded: false,
            }),
            (DraftKind::Forward, Some(id)) => Some(OriginalRef {
                message_id_header: id.clone(),
                forwarded: true,
            }),
            _ => None,
        };
        let archive_original = draft.archive_original && original.is_some();
        let job = SendJob {
            draft: id.clone(),
            message_id_header: draft.message_id_header.clone(),
            from: built.envelope.from.clone(),
            recipients: built.envelope.recipients.clone(),
            file: files::outgoing_path(&id, files::MESSAGE_FILE),
            sent_file: files::outgoing_path(&id, files::SENT_FILE),
            subject: draft.subject.clone(),
            to,
            recipient_count: u32::try_from(built.envelope.recipients.len()).unwrap_or(u32::MAX),
            server_saves_sent,
            original,
            phase: SendPhase::Submit,
            checks: 0,
            scheduled: at.is_some(),
            archive_original,
            archive_missing: false,
            receipt_for: None,
            follow_up_at: draft.follow_up_at,
            redirect: false,
        };
        let not_before = match at {
            Some(at) => Some(at),
            None => (delay > 0).then(|| now + i64::from(delay) * 1000),
        };
        let lock = self.inner.outbox.files_lock();
        let held = lock.lock().await;
        let root = self.inner.root.clone();
        let (message_file, sent_file) = (job.file.clone(), job.sent_file.clone());
        blocking(move || {
            files::write_atomically(&files::resolve(&root, &message_file)?, &built.for_sending)?;
            files::write_atomically(&files::resolve(&root, &sent_file)?, &built.for_sent_folder)
        })
        .await??;
        let change = match store
            .enqueue_send(id.clone(), job, not_before, request)
            .await
        {
            Ok(change) => change,
            Err(err) => {
                files::remove_outgoing_dir(&self.inner.root, &id);
                return Err(err.into());
            }
        };
        drop(held);
        self.unschedule(&id);
        self.inner.outbox.changed(&change);
        let op = match change
            .operations
            .iter()
            .find(|op| matches!(op.payload, OperationPayload::Send(_)))
        {
            Some(op) => op.clone(),
            None => {
                let draft = self.stored(&id).await?;
                let op = draft
                    .outgoing
                    .ok_or_else(|| CoreError::Internal("the send was not recorded".into()))?;
                store
                    .operation(op)
                    .await?
                    .ok_or_else(|| CoreError::Internal("the send was not recorded".into()))?
            }
        };
        tracing::info!(account = %draft.account, operation = %op.id, delay, scheduled = at.is_some(), "send queued");
        outgoing::outgoing_mail(&op, self.now())
            .ok_or_else(|| CoreError::Internal("the send was not recorded".into()))
    }

    /// Takes the draft out of the outbox (within the undo delay, or a failed or unclear send
    /// to edit it). `send_in_progress` once the submission started.
    pub(crate) async fn cancel_send(&self, id: DraftId) -> Result<Draft, CoreError> {
        let change = self.inner.store.cancel_send(id.clone()).await?;
        self.inner.outbox.changed(&change);
        if !change.removed.is_empty() {
            let root = self.inner.root.clone();
            let draft = id.clone();
            blocking(move || files::remove_outgoing_dir(&root, &draft)).await?;
            tracing::info!(account = %change.account, "send cancelled; back to the draft");
        }
        self.get(id).await
    }

    /// Messages of the outbox view, newest first.
    pub(crate) async fn outgoing(
        &self,
        account: AccountId,
        before: Option<OperationId>,
        limit: u32,
    ) -> Result<Vec<OutgoingMail>, CoreError> {
        let now = self.now();
        Ok(self
            .inner
            .store
            .sends(account, before, limit)
            .await?
            .iter()
            .filter_map(|op| outgoing::outgoing_mail(op, now))
            .collect())
    }

    /// The user's decision on an unclear send.
    pub(crate) async fn resolve_unclear(
        &self,
        operation: OperationId,
        decision: UnclearDecision,
    ) -> Result<OutgoingMail, CoreError> {
        let change = self
            .inner
            .store
            .resolve_unclear_send(operation, decision)
            .await?;
        self.inner.outbox.changed(&change);
        tracing::info!(account = %change.account, %operation, ?decision, "unclear send decided");
        let op = match change.operations.iter().find(|op| op.id == operation) {
            Some(op) => op.clone(),
            None => self
                .inner
                .store
                .operation(operation)
                .await?
                .ok_or_else(|| CoreError::NotFound(format!("operation {operation}")))?,
        };
        outgoing::outgoing_mail(&op, self.now())
            .ok_or_else(|| CoreError::NotFound(format!("send {operation}")))
    }

    // -- settings, identities, contacts ----------------------------------------------------

    pub(crate) async fn compose_settings(&self) -> Result<ComposeSettings, CoreError> {
        let settings = self.inner.store.settings(GLOBAL_SCOPE).await?;
        let mut out = ComposeSettings::default();
        if let Some(stored) = settings.iter().find(|s| s.key == SEND_DELAY_KEY) {
            match serde_json::from_str::<u32>(&stored.value) {
                Ok(secs) => out.send_delay_seconds = secs.min(MAX_SEND_DELAY_SECS),
                Err(err) => tracing::warn!(%err, "stored send delay unreadable; default used"),
            }
        }
        if let Some(stored) = settings.iter().find(|s| s.key == QUOTE_POSITION_KEY) {
            match serde_json::from_str::<QuotePosition>(&stored.value) {
                Ok(position) => out.default_quote_position = position,
                Err(err) => tracing::warn!(%err, "stored quote position unreadable; default used"),
            }
        }
        if let Some(stored) = settings.iter().find(|s| s.key == ARCHIVE_ON_REPLY_KEY) {
            match serde_json::from_str::<bool>(&stored.value) {
                Ok(on) => out.archive_on_reply = on,
                Err(err) => tracing::warn!(%err, "stored archive setting unreadable; default used"),
            }
        }
        if let Some(stored) = settings.iter().find(|s| s.key == READ_RECEIPTS_KEY) {
            match serde_json::from_str::<mail_model::ReadReceiptPolicy>(&stored.value) {
                Ok(policy) => out.read_receipts = policy,
                Err(err) => {
                    tracing::warn!(%err, "stored read receipt setting unreadable; default used");
                }
            }
        }
        if let Some(stored) = settings.iter().find(|s| s.key == ATTACHMENT_REMINDER_KEY) {
            match serde_json::from_str::<bool>(&stored.value) {
                Ok(on) => out.attachment_reminder = on,
                Err(err) => {
                    tracing::warn!(%err, "stored attachment reminder unreadable; default used");
                }
            }
        }
        Ok(out)
    }

    /// Stage 20 (decision 0058): the word of the draft's own text that speaks of an attachment
    /// while the draft has none (inline images count as attached); `None` with the reminder off
    /// or nothing to remind of. Quote and signature are kept apart by the composer and are not
    /// read; quoted lines and quoted messages inside the text are left out by
    /// `mail_content::attachment_mention`.
    pub(crate) async fn attachment_reminder(
        &self,
        id: DraftId,
    ) -> Result<Option<String>, CoreError> {
        if !self.compose_settings().await?.attachment_reminder {
            return Ok(None);
        }
        let draft = self.get(id).await?;
        if !draft.attachments.is_empty() {
            return Ok(None);
        }
        let own_text = match draft.format {
            mail_model::ComposeFormat::Html => mail_content::html_to_text(&draft.body_html),
            mail_model::ComposeFormat::Text => draft.body_text,
        };
        Ok(mail_content::attachment_mention(&own_text))
    }

    pub(crate) async fn set_compose_settings(
        &self,
        settings: ComposeSettings,
    ) -> Result<ComposeSettings, CoreError> {
        if settings.send_delay_seconds > MAX_SEND_DELAY_SECS {
            return Err(CoreError::InvalidArgument(format!(
                "the send delay is 0 to {MAX_SEND_DELAY_SECS} seconds"
            )));
        }
        self.inner
            .store
            .put_settings(
                GLOBAL_SCOPE,
                vec![
                    (
                        SEND_DELAY_KEY.to_owned(),
                        settings.send_delay_seconds.to_string(),
                    ),
                    (
                        QUOTE_POSITION_KEY.to_owned(),
                        format!("\"{}\"", settings.default_quote_position.as_str()),
                    ),
                    (
                        ARCHIVE_ON_REPLY_KEY.to_owned(),
                        settings.archive_on_reply.to_string(),
                    ),
                    (
                        READ_RECEIPTS_KEY.to_owned(),
                        format!("\"{}\"", settings.read_receipts.as_str()),
                    ),
                    (
                        ATTACHMENT_REMINDER_KEY.to_owned(),
                        settings.attachment_reminder.to_string(),
                    ),
                ],
            )
            .await?;
        Ok(settings)
    }

    pub(crate) async fn send_settings(
        &self,
        account: AccountId,
    ) -> Result<AccountSendSettings, CoreError> {
        let store = &self.inner.store;
        let stored = store
            .account(account)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {account}")))?;
        let mode = store.sent_copy_mode(account).await?;
        let gmail = files_sent_itself(&stored.imap.host) || store.account_is_gmail(account).await?;
        Ok(AccountSendSettings {
            account_id: account,
            sent_copy: mode,
            server_saves_sent: match mode {
                SentCopyMode::Server => true,
                SentCopyMode::App => false,
                SentCopyMode::Auto => gmail,
            },
            gmail,
        })
    }

    pub(crate) async fn set_sent_copy(
        &self,
        account: AccountId,
        mode: SentCopyMode,
    ) -> Result<AccountSendSettings, CoreError> {
        self.inner.store.set_sent_copy_mode(account, mode).await?;
        self.send_settings(account).await
    }

    pub(crate) async fn identities(
        &self,
        account: AccountId,
    ) -> Result<Vec<ComposeIdentity>, CoreError> {
        let store = &self.inner.store;
        if store.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        let mut out = Vec::new();
        for (identity, signature) in store.compose_identities(account).await? {
            let defaults = store.signature_defaults(identity.id).await?;
            out.push(ComposeIdentity {
                id: identity.id,
                account_id: identity.account_id,
                email: identity.email,
                name: identity.name,
                reply_to: identity.reply_to,
                is_default: identity.is_default,
                signature,
                signature_new: defaults.new,
                signature_reply: defaults.reply,
            });
        }
        Ok(out)
    }

    pub(crate) async fn set_signature(
        &self,
        identity: IdentityId,
        signature: Option<String>,
    ) -> Result<ComposeIdentity, CoreError> {
        let account = self
            .inner
            .store
            .set_identity_signature(identity, signature)
            .await?;
        self.emit(CoreEvent::AccountsChanged);
        self.emit(CoreEvent::SignaturesChanged);
        self.identities(account)
            .await?
            .into_iter()
            .find(|i| i.id == identity)
            .ok_or_else(|| CoreError::NotFound(format!("identity {}", identity.0)))
    }

    pub(crate) async fn search_contacts(
        &self,
        query: String,
        limit: u32,
    ) -> Result<Vec<ContactSuggestion>, CoreError> {
        let limit = limit.clamp(1, MAX_CONTACT_SUGGESTIONS);
        contacts::search(&self.inner.store, &query, limit).await
    }
}

/// The contact directory's job: reads messages not read yet, batch by batch; then waits for
/// new mail. Holds the store only while it works (the database closes with the core).
fn contacts_job<C: Connector>(
    weak: Weak<Inner<C>>,
    kick: Arc<Notify>,
    delay: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // The first run waits too: a core that is dropped right after it started must not find
        // its database held open by this task.
        tokio::time::sleep(delay).await;
        let mut first = true;
        // Stage 17: the kinds of the rows of stage 10 are recomputed once per run, when the
        // backfill first has work.
        let mut kinds_done = false;
        loop {
            if !first {
                tokio::select! {
                    () = kick.notified() => tokio::time::sleep(delay).await,
                    () = tokio::time::sleep(Duration::from_secs(600)) => {}
                }
            }
            first = false;
            loop {
                let Some(store) = weak.upgrade().map(|inner| inner.store.clone()) else {
                    return;
                };
                let own = match contacts::own_addresses(&store).await {
                    Ok(own) => own,
                    Err(err) => {
                        tracing::warn!(error = %err, "contacts: own addresses unreadable");
                        break;
                    }
                };
                match contacts::run_batch(&store, &own).await {
                    Ok((0, _)) => break,
                    Ok((n, backfill)) => {
                        tracing::debug!(messages = n, backfill, "contacts read");
                        if backfill && !kinds_done {
                            kinds_done = true;
                            if let Err(err) = contacts::recompute_kinds(&store).await {
                                tracing::warn!(error = %err, "contact kinds not recomputed");
                            }
                        }
                        tokio::task::yield_now().await;
                    }
                    Err(err) => {
                        tracing::warn!(error = %err, "contacts could not be read");
                        break;
                    }
                }
            }
        }
    })
}

/// New mail (and new identities) wake the contact job.
fn contacts_listener(
    mut events: broadcast::Receiver<CoreEvent>,
    kick: Arc<Notify>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(CoreEvent::MessagesChanged { .. } | CoreEvent::AccountsChanged) => {
                    kick.notify_one();
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    })
}
