//! Stage 11: [`ImapSyncFolder`], the connection of the settings sync (decision 0014, 0022).
//!
//! It logs in like a read-only connection and is then restricted to one folder by the command
//! layer ([`crate::command::FolderScope`]): it may create, select, append to and expunge from
//! the settings sync folder, and nothing else. Every other writing command, in any folder,
//! fails with `read_only` before it is sent, also when the account has `writes_enabled =
//! false`. The folder is created without subscribing to it (and unsubscribed in case the
//! server subscribes new folders itself), so most mail clients do not show it.
//!
//! Only messages with both settings sync header fields are of interest to the caller; this
//! type reads any header fields the caller names and leaves the meaning to it.

use async_imap::imap_proto::{AttributeValue, MailboxDatum, Response};
use mail_model::{
    Access, BackendError, ConnectConfig, MailBackend, MailConnector, SelectedMailbox, SyncFolder,
    SyncFolderConnector, SyncFolderEntry, SyncFolderState, Uid, UidSet,
};

use crate::backend::{ImapBackend, TlsTransport};
use crate::command::{FolderScope, TaggedStatus, Transport, quote};
use crate::connector::ImapConnector;
use crate::convert::uid_of;
use crate::error::short;

/// Longest header field name the caller may ask for (they go into one command line).
const MAX_FIELD_NAME: usize = 64;

/// The settings sync connection of one account. Built by
/// [`ImapConnector::connect_sync_folder`].
pub struct ImapSyncFolder<S: Transport = TlsTransport> {
    backend: ImapBackend<S>,
    /// Wire name of the folder.
    folder: String,
    open: Option<SelectedMailbox>,
}

impl<S: Transport> std::fmt::Debug for ImapSyncFolder<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapSyncFolder")
            .field("folder", &self.folder)
            .field("open", &self.open.is_some())
            .finish_non_exhaustive()
    }
}

impl<S: Transport> ImapSyncFolder<S> {
    /// Restricts a freshly logged-in read-only backend to `folder`.
    pub(crate) fn new(mut backend: ImapBackend<S>, folder: &str) -> Result<Self, BackendError> {
        if backend.access() != Access::ReadOnly {
            return Err(BackendError::InvalidState(
                "the settings sync folder connection starts read-only".into(),
            ));
        }
        backend.conn.restrict_to_folder(FolderScope::new(folder)?);
        Ok(Self {
            backend,
            folder: folder.to_owned(),
            open: None,
        })
    }

    /// The underlying connection (tests use its `raw_command` to prove the restriction).
    #[cfg(feature = "test-support")]
    pub fn backend_mut(&mut self) -> &mut ImapBackend<S> {
        &mut self.backend
    }

    fn opened(&self) -> Result<&SelectedMailbox, BackendError> {
        match &self.open {
            Some(open) if self.backend.selected.as_ref() == Some(open) => Ok(open),
            _ => Err(BackendError::InvalidState(format!(
                "{} is not open for writing",
                short(&self.folder)
            ))),
        }
    }

    async fn select(&mut self) -> Result<SelectedMailbox, BackendError> {
        self.open = None;
        let selected = self.backend.open_as(&self.folder, "SELECT").await?;
        if selected.read_only {
            return Err(BackendError::Rejected(format!(
                "{} can only be opened read-only",
                short(&self.folder)
            )));
        }
        self.open = Some(selected.clone());
        Ok(selected)
    }

    async fn create(&mut self) -> Result<(), BackendError> {
        let quoted = quote(&self.folder)?;
        let created = self
            .backend
            .conn
            .run(&format!("CREATE {quoted}"), |_| false)
            .await?;
        if created.status != TaggedStatus::Ok {
            // `[ALREADYEXISTS]` (RFC 5530), or another client created it meanwhile: fine if it
            // can be selected now (the caller selects next).
            tracing::debug!(response = %created.text, "CREATE of the settings sync folder refused");
        }
        // Some servers subscribe new folders themselves; the folder stays unsubscribed.
        let unsubscribed = self
            .backend
            .conn
            .run(&format!("UNSUBSCRIBE {quoted}"), |_| false)
            .await?;
        if unsubscribed.status != TaggedStatus::Ok {
            tracing::trace!(response = %unsubscribed.text, "UNSUBSCRIBE refused (not subscribed)");
        }
        Ok(())
    }
}

fn state_of(selected: &SelectedMailbox) -> SyncFolderState {
    SyncFolderState {
        uid_validity: selected.uid_validity,
        uid_next: selected.uid_next,
        exists: selected.exists,
        highest_modseq: selected.highest_modseq,
    }
}

impl<S: Transport> SyncFolder for ImapSyncFolder<S> {
    async fn open(&mut self, create: bool) -> Result<Option<SyncFolderState>, BackendError> {
        match self.select().await {
            Ok(selected) => Ok(Some(state_of(&selected))),
            Err(BackendError::MailboxNotFound(_)) if create => {
                self.create().await?;
                Ok(Some(state_of(&self.select().await?)))
            }
            Err(BackendError::MailboxNotFound(_)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn list(&mut self, fields: &[&str]) -> Result<Vec<SyncFolderEntry>, BackendError> {
        let open = self.opened()?.clone();
        if fields.iter().any(|f| {
            f.is_empty()
                || f.len() > MAX_FIELD_NAME
                || !f.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }) {
            return Err(BackendError::InvalidState(
                "header field names are letters, digits and '-'".into(),
            ));
        }
        // An empty folder has no `1:*` some servers would accept.
        let all = self.backend.search_uids(&open).await?;
        if all.is_empty() {
            return Ok(Vec::new());
        }
        let section = if fields.is_empty() {
            String::new()
        } else {
            format!(" BODY.PEEK[HEADER.FIELDS ({})]", fields.join(" "))
        };
        let line = format!("UID FETCH {all} (UID RFC822.SIZE{section})");
        let mut entries: Vec<SyncFolderEntry> = Vec::new();
        self.backend
            .conn
            .run(&line, |resp| {
                let Response::Fetch(_, attrs) = resp else {
                    return false;
                };
                let Some(uid) = uid_of(attrs).filter(|uid| all.contains(*uid)) else {
                    return false;
                };
                let mut size = None;
                let mut header = None;
                for attr in attrs {
                    match attr {
                        AttributeValue::Rfc822Size(s) => size = Some(*s),
                        AttributeValue::BodySection { data, .. } => {
                            header = Some(data.as_deref().unwrap_or_default().to_vec());
                        }
                        _ => {}
                    }
                }
                if size.is_none() && header.is_none() {
                    // An unsolicited flag update for the UID.
                    return false;
                }
                match entries.iter_mut().find(|e| e.uid == uid) {
                    Some(entry) => {
                        if let Some(size) = size {
                            entry.size = size;
                        }
                        if let Some(header) = header {
                            entry.header = header;
                        }
                    }
                    None => entries.push(SyncFolderEntry {
                        uid,
                        size: size.unwrap_or_default(),
                        header: header.unwrap_or_default(),
                    }),
                }
                true
            })
            .await?
            .check("UID FETCH")?;
        entries.sort_by_key(|e| e.uid);
        Ok(entries)
    }

    async fn read(&mut self, uids: &UidSet) -> Result<Vec<(Uid, Vec<u8>)>, BackendError> {
        self.opened()?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let mut bodies: Vec<(Uid, Vec<u8>)> = Vec::new();
        self.backend
            .conn
            .run(&format!("UID FETCH {uids} (UID BODY.PEEK[TEXT])"), |resp| {
                let Response::Fetch(_, attrs) = resp else {
                    return false;
                };
                let Some(uid) = uid_of(attrs).filter(|uid| uids.contains(*uid)) else {
                    return false;
                };
                let body = attrs.iter().find_map(|attr| match attr {
                    AttributeValue::BodySection { data, .. } => {
                        Some(data.as_deref().unwrap_or_default().to_vec())
                    }
                    _ => None,
                });
                let Some(body) = body else {
                    return false;
                };
                match bodies.iter_mut().find(|(u, _)| *u == uid) {
                    Some((_, existing)) => *existing = body,
                    None => bodies.push((uid, body)),
                }
                true
            })
            .await?
            .check("UID FETCH")?;
        bodies.sort_by_key(|(uid, _)| *uid);
        Ok(bodies)
    }

    async fn append(&mut self, message: &[u8]) -> Result<Option<Uid>, BackendError> {
        if message.is_empty() || message.contains(&0) {
            return Err(BackendError::InvalidState(
                "APPEND of an empty message or one with NUL bytes".into(),
            ));
        }
        let line = format!("APPEND {} (\\Seen)", quote(&self.folder)?);
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
            return Err(BackendError::MailboxNotFound(self.folder.clone()));
        }
        let tagged = tagged.check("APPEND")?;
        Ok(tagged
            .append_uid
            .filter(|_| self.backend.caps.uidplus)
            .map(|appended| appended.uid))
    }

    async fn delete(&mut self, uids: &UidSet) -> Result<(), BackendError> {
        self.opened()?;
        if uids.is_empty() {
            return Ok(());
        }
        self.backend
            .conn
            .run(
                &format!("UID STORE {uids} +FLAGS.SILENT (\\Deleted)"),
                |resp| match resp {
                    // With CONDSTORE the server reports the new MODSEQ even for .SILENT.
                    Response::Fetch(_, attrs) => {
                        uid_of(attrs).is_some_and(|uid| uids.contains(uid))
                    }
                    _ => false,
                },
            )
            .await?
            .check("UID STORE")?;
        if self.backend.caps.uidplus {
            self.backend
                .conn
                .run(&format!("UID EXPUNGE {uids}"), |_| false)
                .await?
                .check("UID EXPUNGE")?;
            return Ok(());
        }
        // Without UIDPLUS a plain EXPUNGE removes every message marked deleted: only when the
        // folder has no others (another client's), else the marked ones wait.
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
        if UidSet::from_uids(deleted).difference(uids).is_empty() {
            self.backend
                .conn
                .run("EXPUNGE", |_| false)
                .await?
                .check("EXPUNGE")?;
        } else {
            tracing::info!("no UIDPLUS and foreign messages are marked deleted; not expunging");
        }
        Ok(())
    }

    async fn logout(self) -> Result<(), BackendError> {
        self.backend.logout().await
    }
}

impl SyncFolderConnector for ImapConnector {
    type Folder = ImapSyncFolder<TlsTransport>;

    async fn connect_sync_folder(
        &self,
        config: &ConnectConfig,
        folder: &str,
    ) -> Result<Self::Folder, BackendError> {
        // Login and everything before the restriction run under the read-only allowlist.
        let mut config = config.clone();
        config.access = Access::ReadOnly;
        let backend = self.connect(&config).await?;
        ImapSyncFolder::new(backend, folder)
    }
}
