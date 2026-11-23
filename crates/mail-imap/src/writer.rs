//! [`ImapWriter`]: an IMAP connection that may write (stage 4, decision 0012), and
//! [`WriteConnector`] for [`ImapConnector`].
//!
//! A writer is an [`ImapBackend`] opened with [`Access::ReadWrite`]: mailboxes are opened with
//! SELECT, and the command layer lets writing commands through. [`ImapConnector::connect_writer`]
//! refuses a read-only configuration before it connects, so an account without
//! `writes_enabled` never gets a writer; read-only connections keep the allowlist of
//! `command::permit` (proved by `tests/read_only.rs`).
//!
//! Every writing command goes through the own command layer, which checks the tagged status
//! (async-imap's `uid_store` reports a refusal as an empty success, decision 0002) and keeps the
//! response codes: `[MODIFIED ...]` after `STORE (UNCHANGEDSINCE ...)`, `[COPYUID ...]` in the
//! tagged OK of COPY and as an untagged `* OK` during MOVE (RFC 6851).

use async_imap::imap_proto::{MailboxDatum, Status};
use mail_model::{
    Access, AppendUid, BackendError, BodySection, Capabilities, ConnectConfig, CopyUid,
    ExpungeOutcome, FetchedSection, Flag, FlagUpdate, IdleOutcome, MailBackend, MailConnector,
    MailWriter, MailboxRole, MailboxStatus, MessageHeader, MessageStructure, ModSeq, MoveMethod,
    MoveOutcome, RawMessage, RemoteMailbox, SelectedMailbox, StoreMode, StoreOutcome, Uid, UidSet,
    WriteConnector, is_valid_keyword,
};

use crate::backend::{ImapBackend, NOT_A_MISSING_MAILBOX, TlsTransport, mailbox_status};
use crate::command::{Response, TaggedStatus, Transport, copy_uid_of, quote};
use crate::connector::ImapConnector;
use crate::convert::{FlagCollector, uid_of};
use crate::error::short;
use crate::mutf7;

/// An authenticated IMAP connection with write access. Built by
/// [`ImapConnector::connect_writer`]; reading works as on an [`ImapBackend`].
pub struct ImapWriter<S: Transport = TlsTransport> {
    backend: ImapBackend<S>,
}

impl<S: Transport> std::fmt::Debug for ImapWriter<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ImapWriter").field(&self.backend).finish()
    }
}

impl<S: Transport> ImapWriter<S> {
    /// Wraps a read-write backend; a read-only one is refused.
    pub(crate) fn new(backend: ImapBackend<S>) -> Result<Self, BackendError> {
        if backend.access() != Access::ReadWrite {
            return Err(BackendError::ReadOnly(
                "a writer needs a read-write connection".into(),
            ));
        }
        Ok(Self { backend })
    }

    /// The underlying backend (tests use its `raw_command` for setup).
    pub fn backend_mut(&mut self) -> &mut ImapBackend<S> {
        &mut self.backend
    }

    async fn status_of(&mut self, name: &str) -> Result<MailboxStatus, BackendError> {
        let line = format!("STATUS {} ({})", quote(name)?, self.backend.status_items());
        let mut found = None;
        let tagged = self
            .backend
            .conn
            .run(&line, |resp| match resp {
                Response::MailboxData(MailboxDatum::Status { status, .. }) => {
                    found = Some(mailbox_status(status));
                    true
                }
                _ => false,
            })
            .await?;
        match tagged.status {
            TaggedStatus::Ok => found.ok_or_else(|| {
                BackendError::Protocol(format!("STATUS {} without data", short(name)))
            }),
            TaggedStatus::No => {
                let code = tagged.bracket_code().unwrap_or_default();
                Err(if NOT_A_MISSING_MAILBOX.contains(&code.as_str()) {
                    BackendError::Rejected(format!("STATUS: NO {}", tagged.text))
                } else {
                    BackendError::MailboxNotFound(name.to_owned())
                })
            }
            TaggedStatus::Bad => Err(BackendError::Rejected(format!(
                "STATUS: BAD {}",
                tagged.text
            ))),
        }
    }

    /// `\Deleted` on `uids`, then UID EXPUNGE, or a plain EXPUNGE only if no other message of the
    /// mailbox is marked deleted (without UIDPLUS a plain EXPUNGE removes all of them).
    async fn delete_and_expunge(&mut self, uids: &UidSet) -> Result<ExpungeOutcome, BackendError> {
        let line = format!("UID STORE {uids} +FLAGS.SILENT (\\Deleted)");
        self.backend
            .conn
            .run(&line, |resp| match resp {
                // With CONDSTORE the server reports the new MODSEQ even for .SILENT.
                Response::Fetch(_, attrs) => uid_of(attrs).is_some_and(|uid| uids.contains(uid)),
                _ => false,
            })
            .await?
            .check("UID STORE")?;
        if self.backend.caps.uidplus {
            self.backend
                .conn
                .run(&format!("UID EXPUNGE {uids}"), |_| false)
                .await?
                .check("UID EXPUNGE")?;
            return Ok(ExpungeOutcome::Expunged);
        }
        let mut deleted = Vec::new();
        self.backend
            .conn
            .run("UID SEARCH DELETED", |resp| match resp {
                Response::MailboxData(MailboxDatum::Search(found)) => {
                    deleted.extend_from_slice(found);
                    true
                }
                _ => false,
            })
            .await?
            .check("UID SEARCH")?;
        let foreign = UidSet::from_uids(deleted).difference(uids);
        if !foreign.is_empty() {
            tracing::info!(
                others = foreign.len(),
                "no UIDPLUS and other messages are marked deleted; not expunging"
            );
            return Ok(ExpungeOutcome::LeftDeleted);
        }
        self.backend
            .conn
            .run("EXPUNGE", |_| false)
            .await?
            .check("EXPUNGE")?;
        Ok(ExpungeOutcome::Expunged)
    }

    /// Stage 14 (decision 0034): `UID STORE uids ±X-GM-LABELS (labels)` on Gmail. System
    /// labels go as atoms (`\Inbox`), user labels as quoted strings in modified UTF-7, like
    /// mailbox names. Labels cannot be mixed with flags in one call and take no
    /// `UNCHANGEDSINCE` (Gmail does not document it); both fail with
    /// [`BackendError::InvalidState`] before anything is sent, as does an invalid label. A
    /// server without `X-GM-EXT-1` gets [`BackendError::Unsupported`]. The FETCH responses
    /// Gmail sends back (the new label list) are taken as part of the answer.
    async fn store_labels(
        &mut self,
        uids: &UidSet,
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<ModSeq>,
    ) -> Result<StoreOutcome, BackendError> {
        if !self.backend.caps.gmail {
            return Err(BackendError::Unsupported(
                "labels (X-GM-EXT-1, Gmail only)".into(),
            ));
        }
        if unchanged_since.is_some() {
            return Err(BackendError::InvalidState(
                "labels are stored without UNCHANGEDSINCE".into(),
            ));
        }
        let mut list = Vec::with_capacity(flags.len());
        for flag in flags {
            let Flag::Label(label) = flag else {
                return Err(BackendError::InvalidState(
                    "labels and flags in one STORE".into(),
                ));
            };
            list.push(label_argument(label)?);
        }
        let sign = match mode {
            StoreMode::Add => '+',
            StoreMode::Remove => '-',
        };
        let line = format!("UID STORE {uids} {sign}X-GM-LABELS ({})", list.join(" "));
        self.backend
            .conn
            .run(&line, |resp| match resp {
                Response::Fetch(_, attrs) => uid_of(attrs).is_some_and(|uid| uids.contains(uid)),
                _ => false,
            })
            .await?
            .check("UID STORE")?;
        Ok(StoreOutcome::default())
    }

    /// Whether LIST knows a mailbox with exactly this wire name.
    async fn exists(&mut self, name: &str) -> Result<bool, BackendError> {
        let mut found = false;
        self.backend
            .conn
            .run(&format!("LIST \"\" {}", quote(name)?), |resp| match resp {
                Response::MailboxData(MailboxDatum::List(entry)) => {
                    found |= entry.name == name
                        || (name.eq_ignore_ascii_case("INBOX")
                            && entry.name.eq_ignore_ascii_case("INBOX"));
                    true
                }
                _ => false,
            })
            .await?
            .check("LIST")?;
        Ok(found)
    }
}

/// A label as `X-GM-LABELS` takes it: a system label as an atom, a user label encoded like a
/// mailbox name and quoted. Invalid names fail with [`BackendError::InvalidState`].
pub(crate) fn label_argument(label: &str) -> Result<String, BackendError> {
    if !mail_model::is_valid_label(label) {
        return Err(BackendError::InvalidState(format!(
            "{} is not a valid label",
            short(label)
        )));
    }
    if let Some(system) = mail_model::system_label(label) {
        return Ok(system.to_owned());
    }
    quote(&mutf7::encode(label))
}

/// SPECIAL-USE attribute for CREATE (RFC 6154, section 3).
fn special_use(role: MailboxRole) -> Option<&'static str> {
    match role {
        MailboxRole::Archive => Some("\\Archive"),
        MailboxRole::Drafts => Some("\\Drafts"),
        MailboxRole::Junk => Some("\\Junk"),
        MailboxRole::Sent => Some("\\Sent"),
        MailboxRole::Trash => Some("\\Trash"),
        MailboxRole::Inbox | MailboxRole::All | MailboxRole::Flagged | MailboxRole::Custom => None,
    }
}

impl<S: Transport> MailBackend for ImapWriter<S> {
    fn capabilities(&self) -> &Capabilities {
        self.backend.capabilities()
    }

    fn access(&self) -> Access {
        self.backend.access()
    }

    async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
        self.backend.list_mailboxes().await
    }

    async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        self.backend.open_mailbox(name).await
    }

    async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
        self.backend.search_uids(mailbox).await
    }

    async fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<MessageHeader>, BackendError> {
        self.backend.fetch_headers(mailbox, uids).await
    }

    async fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        changed_since: Option<ModSeq>,
    ) -> Result<Vec<FlagUpdate>, BackendError> {
        self.backend.fetch_flags(mailbox, uids, changed_since).await
    }

    async fn idle<St>(
        &mut self,
        mailbox: &SelectedMailbox,
        max: std::time::Duration,
        stop: St,
    ) -> Result<IdleOutcome, BackendError>
    where
        St: std::future::Future<Output = ()> + Send,
    {
        self.backend.idle(mailbox, max, stop).await
    }

    async fn fetch_structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<MessageStructure>, BackendError> {
        self.backend.fetch_structure(mailbox, uid).await
    }

    async fn fetch_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<RawMessage>, BackendError> {
        self.backend.fetch_messages(mailbox, uids).await
    }

    async fn fetch_sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
        self.backend.fetch_sections(mailbox, uid, sections).await
    }

    async fn search_text(
        &mut self,
        mailbox: &SelectedMailbox,
        criteria: &[mail_model::search::ServerCriterion],
    ) -> Result<UidSet, BackendError> {
        self.backend.search_text(mailbox, criteria).await
    }

    async fn noop(&mut self) -> Result<(), BackendError> {
        self.backend.noop().await
    }

    async fn logout(self) -> Result<(), BackendError> {
        self.backend.logout().await
    }
}

/// Newest UIDs [`MailWriter::find_message_id`] reads besides the server's search hits: a fresh
/// APPEND may not be in the search index yet (Stalwart indexes asynchronously, decision 0001).
const RECENT_FOR_MESSAGE_ID: usize = 100;

/// A Message-ID as a quoted SEARCH string: printable ASCII only (the command layer sends one
/// line; a Message-ID with anything else is not searched for, only read from the headers).
fn search_string(message_id: &str) -> Option<String> {
    let id = format!("<{message_id}>");
    id.bytes()
        .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
        .then(|| format!("\"{id}\""))
}

impl<S: Transport> MailWriter for ImapWriter<S> {
    async fn append(
        &mut self,
        mailbox: &str,
        flags: &[Flag],
        message: &[u8],
    ) -> Result<Option<AppendUid>, BackendError> {
        if message.is_empty() || message.contains(&0) {
            return Err(BackendError::InvalidState(
                "APPEND of an empty message or one with NUL bytes".into(),
            ));
        }
        if let Some(keyword) = flags
            .iter()
            .find(|f| matches!(f, Flag::Keyword(_) | Flag::Label(_)))
        {
            return Err(BackendError::InvalidState(format!(
                "APPEND with the keyword {}",
                short(keyword.imap())
            )));
        }
        let list: Vec<&str> = flags.iter().map(Flag::imap).collect();
        let line = format!("APPEND {} ({})", quote(mailbox)?, list.join(" "));
        let non_sync = self.backend.caps.literal_plus;
        let tagged = self
            .backend
            .conn
            .run_literal(&line, message, non_sync, |_| false)
            .await?;
        if tagged.status != TaggedStatus::Ok
            && (tagged.bracket_code().as_deref() == Some("TRYCREATE")
                || tagged.text.starts_with("[TRYCREATE]"))
        {
            return Err(BackendError::MailboxNotFound(mailbox.to_owned()));
        }
        let tagged = tagged.check("APPEND")?;
        Ok(tagged.append_uid.filter(|_| self.backend.caps.uidplus))
    }

    async fn find_message_id(
        &mut self,
        mailbox: &SelectedMailbox,
        message_id: &str,
    ) -> Result<UidSet, BackendError> {
        self.backend.ensure_open(mailbox)?;
        let mut candidates: Vec<Uid> = Vec::new();
        if let Some(wanted) = search_string(message_id) {
            let line = format!("UID SEARCH HEADER Message-ID {wanted}");
            let tagged = self
                .backend
                .conn
                .run(&line, |resp| match resp {
                    Response::MailboxData(MailboxDatum::Search(found)) => {
                        candidates.extend_from_slice(found);
                        true
                    }
                    _ => false,
                })
                .await?;
            if tagged.status != TaggedStatus::Ok {
                tracing::debug!(response = %tagged.text, "SEARCH HEADER refused; reading headers");
            }
        }
        let all = self.backend.search_uids(mailbox).await?;
        candidates.extend(all.iter().rev().take(RECENT_FOR_MESSAGE_ID));
        let candidates = UidSet::from_uids(candidates);
        if candidates.is_empty() {
            return Ok(UidSet::new());
        }
        let headers = self.backend.fetch_headers(mailbox, &candidates).await?;
        Ok(UidSet::from_uids(
            headers
                .iter()
                .filter(|h| h.message_id.as_deref() == Some(message_id))
                .map(|h| h.uid),
        ))
    }

    async fn mailbox_status(&mut self, name: &str) -> Result<MailboxStatus, BackendError> {
        self.status_of(name).await
    }

    async fn store_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<ModSeq>,
    ) -> Result<StoreOutcome, BackendError> {
        self.backend.ensure_open(mailbox)?;
        if uids.is_empty() || flags.is_empty() {
            return Ok(StoreOutcome::default());
        }
        if flags.iter().any(|f| matches!(f, Flag::Label(_))) {
            return self.store_labels(uids, mode, flags, unchanged_since).await;
        }
        if let Some(Flag::Keyword(bad)) = flags
            .iter()
            .find(|f| matches!(f, Flag::Keyword(k) if !is_valid_keyword(k)))
        {
            return Err(BackendError::InvalidState(format!(
                "{} is not a valid keyword",
                short(bad)
            )));
        }
        if unchanged_since.is_some() && mailbox.highest_modseq.is_none() {
            return Err(BackendError::Unsupported(
                "CONDSTORE (UNCHANGEDSINCE) for this mailbox".into(),
            ));
        }
        let sign = match mode {
            StoreMode::Add => '+',
            StoreMode::Remove => '-',
        };
        let condition = unchanged_since
            .map(|m| format!(" (UNCHANGEDSINCE {m})"))
            .unwrap_or_default();
        let list: Vec<&str> = flags.iter().map(Flag::imap).collect();
        let line = format!(
            "UID STORE {uids}{condition} {sign}FLAGS ({})",
            list.join(" ")
        );
        let mut collector = FlagCollector::new(uids);
        let tagged = self
            .backend
            .conn
            .run(&line, |resp| collector.take(resp))
            .await?
            .check("UID STORE")?;
        Ok(StoreOutcome {
            updates: collector.updates,
            modified: tagged.modified.unwrap_or_default(),
        })
    }

    async fn move_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        target: &str,
    ) -> Result<MoveOutcome, BackendError> {
        self.backend.ensure_open(mailbox)?;
        let quoted = quote(target)?;
        // COPYUID only counts from servers that announced UIDPLUS (a masked capability in a
        // test behaves like a server without it).
        let uidplus = self.backend.caps.uidplus;
        if uids.is_empty() {
            return Ok(MoveOutcome {
                method: MoveMethod::Move,
                copy_uid: None,
            });
        }
        if self.backend.caps.move_ {
            let mut untagged: Option<CopyUid> = None;
            let tagged = self
                .backend
                .conn
                .run(&format!("UID MOVE {uids} {quoted}"), |resp| match resp {
                    Response::Data {
                        status: Status::Ok,
                        outcome,
                    } => match outcome.code.as_ref().and_then(copy_uid_of) {
                        Some(copy) => {
                            untagged = Some(copy);
                            true
                        }
                        None => false,
                    },
                    _ => false,
                })
                .await?
                .check("UID MOVE")?;
            return Ok(MoveOutcome {
                method: MoveMethod::Move,
                copy_uid: untagged.or(tagged.copy_uid).filter(|_| uidplus),
            });
        }
        let tagged = self
            .backend
            .conn
            .run(&format!("UID COPY {uids} {quoted}"), |_| false)
            .await?
            .check("UID COPY")?;
        let copy_uid = tagged.copy_uid.filter(|_| uidplus);
        let method = match self.delete_and_expunge(uids).await? {
            ExpungeOutcome::Expunged => MoveMethod::CopyExpunge,
            ExpungeOutcome::LeftDeleted => MoveMethod::CopyLeftDeleted,
        };
        Ok(MoveOutcome { method, copy_uid })
    }

    async fn expunge_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<ExpungeOutcome, BackendError> {
        self.backend.ensure_open(mailbox)?;
        if uids.is_empty() {
            return Ok(ExpungeOutcome::Expunged);
        }
        self.delete_and_expunge(uids).await
    }

    async fn create_mailbox(
        &mut self,
        path: &str,
        role: Option<MailboxRole>,
    ) -> Result<String, BackendError> {
        let name = mutf7::encode(path);
        let quoted = quote(&name)?;
        let attribute = role
            .filter(|_| self.backend.caps.has("CREATE-SPECIAL-USE"))
            .and_then(special_use);
        let line = match attribute {
            Some(attribute) => format!("CREATE {quoted} (USE ({attribute}))"),
            None => format!("CREATE {quoted}"),
        };
        let mut tagged = self.backend.conn.run(&line, |_| false).await?;
        if attribute.is_some()
            && tagged.status == TaggedStatus::No
            && tagged.bracket_code().as_deref() == Some("USEATTR")
        {
            // RFC 6154: the server refuses this special use (for example because another
            // mailbox has it). Create it plainly; the role is then the user's choice locally.
            tracing::debug!(response = %tagged.text, "CREATE with USE refused; creating without");
            tagged = self
                .backend
                .conn
                .run(&format!("CREATE {quoted}"), |_| false)
                .await?;
        }
        if tagged.status != TaggedStatus::Ok {
            // `[ALREADYEXISTS]` (RFC 5530), or a server without the code: success if it exists.
            if !self.exists(&name).await? {
                return Err(BackendError::Rejected(format!("CREATE: {}", tagged.text)));
            }
            tracing::debug!("mailbox existed already");
        }
        let subscribed = self
            .backend
            .conn
            .run(&format!("SUBSCRIBE {quoted}"), |_| false)
            .await?;
        if subscribed.status != TaggedStatus::Ok {
            tracing::debug!(response = %subscribed.text, "SUBSCRIBE after CREATE refused");
        }
        Ok(name)
    }
}

impl WriteConnector for ImapConnector {
    type Writer = ImapWriter<TlsTransport>;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<Self::Writer, BackendError> {
        if config.access != Access::ReadWrite {
            return Err(BackendError::ReadOnly(
                "connect_writer with a read-only configuration".into(),
            ));
        }
        ImapWriter::new(self.connect(config).await?)
    }
}
