//! [`ImapBackend`]: one authenticated IMAP connection implementing [`MailBackend`].

use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use async_imap::Client;
use async_imap::imap_proto::{MailboxDatum, ResponseCode, StatusAttribute};
use mail_model::{
    Access, BackendError, BodySection, Capabilities, ConnectConfig, FetchedSection, FlagUpdate,
    IdleOutcome, MailBackend, MailboxStatus, MessageHeader, MessageStructure, ModSeq, RawMessage,
    RemoteMailbox, Secret, SelectedMailbox, Uid, UidSet, UidValidity,
};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::bodies::{
    MESSAGE_ITEMS, MessageCollector, STRUCTURE_ITEMS, SectionCollector, StructureCollector,
    section_items,
};
use crate::command::{Conn, Response, TaggedStatus, Transport, permit, quote};
use crate::convert::{
    FlagCollector, HeaderCollector, capability_names, header_items, name_attribute, uid_of,
};
use crate::error::{from_imap, short, status_text};
use crate::mutf7;
use crate::timeout_io::{IoTimeout, TimeoutHandle};

/// The transport of product connections: TLS over TCP.
pub type TlsTransport = TlsStream<TcpStream>;

/// Response codes of a refused EXAMINE/SELECT that do not mean "no such mailbox" (RFC 5530).
const NOT_A_MISSING_MAILBOX: &[&str] = &[
    "NOPERM",
    "UNAVAILABLE",
    "SERVERBUG",
    "LIMIT",
    "INUSE",
    "CONTACTADMIN",
    "OVERQUOTA",
    "EXPUNGEISSUED",
    "CLIENTBUG",
];

/// Longest IDLE before it is renewed, whatever the caller asks for. RFC 2177 lets servers end an
/// idle connection after 30 minutes; the engine uses shorter sections (about 5 minutes) so a
/// silent connection is noticed early (decision 0002).
pub const MAX_IDLE: Duration = Duration::from_secs(25 * 60);

/// One authenticated IMAP connection. Built by [`crate::ImapConnector`].
pub struct ImapBackend<S: Transport = TlsTransport> {
    conn: Conn<S>,
    caps: Capabilities,
    access: Access,
    /// Extensions the server confirmed with `* ENABLED` (upper case).
    enabled: Vec<String>,
    selected: Option<SelectedMailbox>,
}

impl<S: Transport> std::fmt::Debug for ImapBackend<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImapBackend")
            .field("access", &self.access)
            .field("enabled", &self.enabled)
            .field("selected", &self.selected.as_ref().map(|m| &m.name))
            .finish_non_exhaustive()
    }
}

impl<S: Transport> ImapBackend<S> {
    /// Login, CAPABILITY and ENABLE on a connection whose greeting was read. `known_caps` are
    /// the capabilities the server announced on the encrypted connection before login (the
    /// greeting on an implicit-TLS port), `None` if unknown (STARTTLS: the plaintext greeting's
    /// list must be discarded, RFC 3501 section 6.2.1).
    ///
    /// ASCII user name and password: `LOGIN`. Otherwise `AUTHENTICATE PLAIN` (UTF-8 by
    /// definition, RFC 4616), with the initial response on the command line when the server
    /// announced SASL-IR (RFC 4959). Only a server known to lack `AUTH=PLAIN` gets the non-ASCII
    /// credentials as a quoted LOGIN string, which many servers read as UTF-8.
    pub(crate) async fn establish(
        client: Client<IoTimeout<S>>,
        timeouts: TimeoutHandle,
        config: &ConnectConfig,
        known_caps: Option<&[String]>,
    ) -> Result<Self, BackendError> {
        let (session, login_caps) = login(client, config, known_caps).await?;
        let mut conn = Conn::new(session, timeouts, config.access);

        let names = match login_caps {
            Some(caps) => caps
                .iter()
                .map(|cap| match cap {
                    async_imap::types::Capability::Imap4rev1 => "IMAP4REV1".to_owned(),
                    async_imap::types::Capability::Auth(m) => format!("AUTH={m}"),
                    async_imap::types::Capability::Atom(a) => a.clone(),
                })
                .collect(),
            None => {
                let mut names = Vec::new();
                conn.run("CAPABILITY", |resp| match resp {
                    Response::Capabilities(caps) => {
                        names.extend(capability_names(caps));
                        true
                    }
                    _ => false,
                })
                .await
                .map_err(|err| match err {
                    // imap-proto cannot parse a capability list without IMAP4rev1.
                    BackendError::Protocol(detail) => BackendError::Unsupported(format!(
                        "IMAP4rev1 (unreadable CAPABILITY response: {detail})"
                    )),
                    other => other,
                })?
                .check("CAPABILITY")?;
                names
            }
        };
        let caps = Capabilities::from_names(names);
        if !caps.imap4rev1 {
            return Err(BackendError::Unsupported("IMAP4rev1".into()));
        }

        let mut enabled = Vec::new();
        let enable = if caps.qresync {
            Some("ENABLE CONDSTORE QRESYNC")
        } else if caps.condstore {
            Some("ENABLE CONDSTORE")
        } else {
            None
        };
        if let Some(line) = enable.filter(|_| caps.enable) {
            let tagged = conn
                .run(line, |resp| match resp {
                    // imap-proto parses `* ENABLED ...` as a capability list.
                    Response::Capabilities(list) => {
                        enabled.extend(
                            capability_names(list)
                                .iter()
                                .map(|n| n.to_ascii_uppercase()),
                        );
                        true
                    }
                    _ => false,
                })
                .await?;
            if tagged.status != TaggedStatus::Ok {
                // Not fatal: EXAMINE ... (CONDSTORE) enables CONDSTORE per mailbox anyway.
                tracing::warn!(response = %tagged.text, "server refused ENABLE");
            }
        }
        tracing::debug!(capabilities = ?caps.names, ?enabled, "logged in");
        Ok(Self {
            conn,
            caps,
            access: config.access,
            enabled,
            selected: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn conn_mut(&mut self) -> &mut Conn<S> {
        &mut self.conn
    }

    /// Extensions the server confirmed after `ENABLE` (`CONDSTORE`, `QRESYNC`), upper case.
    pub fn enabled(&self) -> &[String] {
        &self.enabled
    }

    fn ensure_open(&self, mailbox: &SelectedMailbox) -> Result<(), BackendError> {
        match &self.selected {
            Some(open)
                if open.name == mailbox.name && open.uid_validity == mailbox.uid_validity =>
            {
                Ok(())
            }
            Some(open) => Err(BackendError::InvalidState(format!(
                "{} (UIDVALIDITY {}) is not open; {} (UIDVALIDITY {}) is",
                mailbox.name, mailbox.uid_validity, open.name, open.uid_validity
            ))),
            None => Err(BackendError::InvalidState(format!(
                "{} is not open; no mailbox is",
                mailbox.name
            ))),
        }
    }

    fn status_items(&self) -> &'static str {
        if self.caps.condstore {
            "MESSAGES UNSEEN UIDNEXT UIDVALIDITY HIGHESTMODSEQ"
        } else {
            "MESSAGES UNSEEN UIDNEXT UIDVALIDITY"
        }
    }

    /// UIDNEXT for a server that did not send it: highest UID + 1, or 1 for an empty mailbox.
    async fn derive_uid_next(&mut self, exists: u32) -> Result<Uid, BackendError> {
        if exists == 0 {
            return Ok(1);
        }
        let mut highest = None;
        self.conn
            .run("FETCH * (UID)", |resp| match resp {
                Response::Fetch(_, attrs) => {
                    highest = highest.max(uid_of(attrs));
                    true
                }
                _ => false,
            })
            .await?
            .check("FETCH * (UID)")?;
        Ok(highest.map_or(1, |uid| uid.saturating_add(1)))
    }

    /// Runs any command line through the command layer (read-only allowlist included) and
    /// hands untagged responses to `on_untagged`. Tests only.
    #[cfg(feature = "test-support")]
    pub async fn raw_command<F>(
        &mut self,
        line: &str,
        on_untagged: F,
    ) -> Result<crate::test_support::RawReply, BackendError>
    where
        F: FnMut(&Response<'_>) -> bool + Send,
    {
        let tagged = self.conn.run(line, on_untagged).await?;
        // The command was sent: a mailbox command changes what is selected.
        let verb = line
            .split_ascii_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if matches!(verb.as_str(), "SELECT" | "EXAMINE" | "CLOSE" | "UNSELECT") {
            self.selected = None;
        }
        Ok(crate::test_support::RawReply {
            ok: tagged.status == TaggedStatus::Ok,
            text: tagged.text,
        })
    }
}

/// How [`login`] authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginMethod {
    Login,
    /// `AUTHENTICATE PLAIN`, then the credentials after the server's continuation.
    Plain,
    /// `AUTHENTICATE PLAIN <initial response>` in one line (SASL-IR).
    PlainInitialResponse,
}

fn login_method(config: &ConnectConfig, known_caps: Option<&[String]>) -> LoginMethod {
    if config.username.is_ascii() && config.password.expose_secret().is_ascii() {
        return LoginMethod::Login;
    }
    let has = |name: &str| {
        known_caps.is_some_and(|caps| caps.iter().any(|c| c.trim().eq_ignore_ascii_case(name)))
    };
    match known_caps {
        // Unknown capabilities: every IMAP4rev1 server must implement AUTH=PLAIN.
        None => LoginMethod::Plain,
        Some(_) if has("AUTH=PLAIN") && has("SASL-IR") => LoginMethod::PlainInitialResponse,
        Some(_) if has("AUTH=PLAIN") => LoginMethod::Plain,
        Some(_) => {
            tracing::debug!(
                "server announced no AUTH=PLAIN; sending non-ASCII credentials with LOGIN"
            );
            LoginMethod::Login
        }
    }
}

type LoggedIn<S> = (
    async_imap::Session<IoTimeout<S>>,
    Option<async_imap::types::Capabilities>,
);

/// Authenticates with the method [`login_method`] picks. The password only ever goes into the
/// command line or the SASL response; error texts are the server's.
async fn login<S: Transport>(
    client: Client<IoTimeout<S>>,
    config: &ConnectConfig,
    known_caps: Option<&[String]>,
) -> Result<LoggedIn<S>, BackendError> {
    match login_method(config, known_caps) {
        LoginMethod::Login => {
            permit(config.access, "LOGIN")?;
            client
                .login_with_capabilities(&config.username, config.password.expose_secret())
                .await
                .map_err(|(err, _client)| login_error(err))
        }
        LoginMethod::Plain => {
            permit(config.access, "AUTHENTICATE PLAIN")?;
            let auth = crate::sasl::PlainAuthenticator::new(&config.username, &config.password);
            let session = client
                .authenticate("PLAIN", auth)
                .await
                .map_err(|(err, _client)| login_error(err))?;
            Ok((session, None))
        }
        LoginMethod::PlainInitialResponse => {
            permit(config.access, "AUTHENTICATE PLAIN")?;
            let response = crate::sasl::plain_response(&config.username, &config.password);
            // A server that still sends a continuation gets the same message again.
            let auth = crate::sasl::PlainAuthenticator::new(&config.username, &config.password);
            let line = Secret::new(format!("PLAIN {}", response.expose_secret()));
            let session = client
                .authenticate(line.expose_secret(), auth)
                .await
                .map_err(|(err, _client)| login_error(err))?;
            Ok((session, None))
        }
    }
}

fn login_error(err: async_imap::error::Error) -> BackendError {
    use async_imap::error::Error as E;
    match err {
        // LOGIN refused (NO or BAD): the user has to fix something; a temporary refusal
        // (`[UNAVAILABLE]`, RFC 5530) is worth retrying.
        E::No(text) | E::Bad(text) => {
            let text = status_text(&text);
            if text.to_ascii_uppercase().contains("[UNAVAILABLE]") {
                BackendError::Rejected(format!("LOGIN: {text}"))
            } else {
                BackendError::AuthFailed(text)
            }
        }
        E::Validate(_) => {
            BackendError::AuthFailed("user name or password contains a line break".into())
        }
        other => from_imap(other),
    }
}

/// State collected from the untagged responses of EXAMINE/SELECT.
#[derive(Default)]
struct OpenState {
    exists: Option<u32>,
    uid_validity: Option<u32>,
    uid_next: Option<u32>,
    highest_modseq: Option<u64>,
    no_modseq: bool,
    keywords_allowed: bool,
}

impl OpenState {
    fn take(&mut self, resp: &Response<'_>) -> bool {
        match resp {
            Response::MailboxData(MailboxDatum::Exists(n)) => self.exists = Some(*n),
            Response::MailboxData(MailboxDatum::Flags(_) | MailboxDatum::Recent(_)) => {}
            Response::Data { outcome, .. } => match &outcome.code {
                Some(ResponseCode::UidValidity(v)) => self.uid_validity = Some(*v),
                Some(ResponseCode::UidNext(n)) => self.uid_next = Some(*n),
                Some(ResponseCode::HighestModSeq(m)) => self.highest_modseq = Some(*m),
                Some(ResponseCode::PermanentFlags(flags)) => {
                    self.keywords_allowed = flags.iter().any(|f| f == "\\*");
                }
                Some(_) => {}
                // imap-proto does not know `[NOMODSEQ]` (RFC 7162) and leaves it in the text.
                None => {
                    let text = outcome.information.as_deref().unwrap_or_default();
                    if text.to_ascii_uppercase().starts_with("[NOMODSEQ]") {
                        self.no_modseq = true;
                    }
                }
            },
            _ => return false,
        }
        true
    }
}

impl<S: Transport> MailBackend for ImapBackend<S> {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn access(&self) -> Access {
        self.access
    }

    async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
        let caps = &self.caps;
        let status_items = self.status_items();
        let mut options = Vec::new();
        if caps.special_use && (caps.list_status || caps.has("LIST-EXTENDED")) {
            options.push("SPECIAL-USE".to_owned());
        }
        if caps.list_status {
            options.push(format!("STATUS ({status_items})"));
        }
        let line = if options.is_empty() {
            "LIST \"\" \"*\"".to_owned()
        } else {
            format!("LIST \"\" \"*\" RETURN ({})", options.join(" "))
        };

        let mut mailboxes: Vec<RemoteMailbox> = Vec::new();
        let mut statuses: HashMap<String, MailboxStatus> = HashMap::new();
        self.conn
            .run(&line, |resp| match resp {
                Response::MailboxData(MailboxDatum::List(entry)) => {
                    let name = entry.name.to_string();
                    let path = mutf7::decode(&name).unwrap_or_else(|_| name.clone());
                    mailboxes.push(RemoteMailbox {
                        path,
                        delimiter: entry.delimiter.as_deref().and_then(|d| d.chars().next()),
                        attributes: entry.name_attributes.iter().map(name_attribute).collect(),
                        status: None,
                        name,
                    });
                    true
                }
                Response::MailboxData(MailboxDatum::Status { mailbox, status }) => {
                    statuses.insert(mailbox.to_string(), mailbox_status(status));
                    true
                }
                _ => false,
            })
            .await?
            .check("LIST")?;

        for mailbox in &mut mailboxes {
            if !mailbox.is_selectable() {
                continue;
            }
            if let Some(status) = statuses.remove(&mailbox.name) {
                mailbox.status = Some(status);
                continue;
            }
            if self.caps.list_status {
                // The server answered LIST-STATUS without STATUS for this one; leave it empty.
                continue;
            }
            let Ok(quoted) = quote(&mailbox.name) else {
                continue;
            };
            let line = format!("STATUS {quoted} ({status_items})");
            let mut found = None;
            let tagged = self
                .conn
                .run(&line, |resp| match resp {
                    Response::MailboxData(MailboxDatum::Status { status, .. }) => {
                        found = Some(mailbox_status(status));
                        true
                    }
                    _ => false,
                })
                .await?;
            if tagged.status == TaggedStatus::Ok {
                mailbox.status = found;
            } else {
                tracing::debug!(response = %tagged.text, "server refused STATUS for one mailbox");
            }
        }
        Ok(mailboxes)
    }

    async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        let verb = match self.access {
            Access::ReadOnly => "EXAMINE",
            Access::ReadWrite => "SELECT",
        };
        let condstore = if self.caps.condstore {
            " (CONDSTORE)"
        } else {
            ""
        };
        let line = format!("{verb} {}{condstore}", quote(name)?);
        // A failed EXAMINE/SELECT leaves no mailbox selected (RFC 3501).
        self.selected = None;
        let mut state = OpenState::default();
        let tagged = self.conn.run(&line, |resp| state.take(resp)).await?;
        match tagged.status {
            TaggedStatus::Ok => {}
            TaggedStatus::No => {
                let code = tagged.bracket_code().unwrap_or_default();
                return Err(if NOT_A_MISSING_MAILBOX.contains(&code.as_str()) {
                    BackendError::Rejected(format!("{verb}: NO {}", tagged.text))
                } else {
                    BackendError::MailboxNotFound(name.to_owned())
                });
            }
            TaggedStatus::Bad => {
                return Err(BackendError::Rejected(format!(
                    "{verb}: BAD {}",
                    tagged.text
                )));
            }
        }
        let uid_validity = state.uid_validity.ok_or_else(|| {
            BackendError::Protocol(format!("{verb} {} without UIDVALIDITY", short(name)))
        })?;
        let exists = state.exists.unwrap_or(0);
        let uid_next = match state.uid_next {
            Some(next) => next,
            None => self.derive_uid_next(exists).await?,
        };
        let highest_modseq = if state.no_modseq || !self.caps.condstore {
            None
        } else {
            state.highest_modseq.map(ModSeq)
        };
        self.conn.watch.reset(exists);
        let mailbox = SelectedMailbox {
            name: name.to_owned(),
            uid_validity: UidValidity(uid_validity),
            uid_next,
            exists,
            highest_modseq,
            read_only: self.access == Access::ReadOnly || tagged.read_only,
            keywords_allowed: state.keywords_allowed,
        };
        self.selected = Some(mailbox.clone());
        Ok(mailbox)
    }

    async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
        self.ensure_open(mailbox)?;
        let mut uids = Vec::new();
        self.conn
            .run("UID SEARCH ALL", |resp| match resp {
                Response::MailboxData(MailboxDatum::Search(found)) => {
                    uids.extend_from_slice(found);
                    true
                }
                _ => false,
            })
            .await?
            .check("UID SEARCH")?;
        Ok(UidSet::from_uids(uids))
    }

    async fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<MessageHeader>, BackendError> {
        self.ensure_open(mailbox)?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let modseq = if mailbox.highest_modseq.is_some() {
            " MODSEQ"
        } else {
            ""
        };
        let line = format!("UID FETCH {uids} ({}{modseq})", header_items());
        let mut collector = HeaderCollector::new(uids);
        self.conn
            .run(&line, |resp| collector.take(resp))
            .await?
            .check("UID FETCH")?;
        let (headers, changed) = collector.finish();
        if changed {
            self.conn.watch.changed = true;
        }
        Ok(headers)
    }

    async fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        changed_since: Option<ModSeq>,
    ) -> Result<Vec<FlagUpdate>, BackendError> {
        self.ensure_open(mailbox)?;
        if changed_since.is_some() && mailbox.highest_modseq.is_none() {
            return Err(BackendError::Unsupported(
                "CONDSTORE (CHANGEDSINCE) for this mailbox".into(),
            ));
        }
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let modseq = if mailbox.highest_modseq.is_some() {
            " MODSEQ"
        } else {
            ""
        };
        let since = changed_since
            .map(|m| format!(" (CHANGEDSINCE {m})"))
            .unwrap_or_default();
        let line = format!("UID FETCH {uids} (UID FLAGS{modseq}){since}");
        let mut collector = FlagCollector::new(uids);
        self.conn
            .run(&line, |resp| collector.take(resp))
            .await?
            .check("UID FETCH")?;
        Ok(collector.updates)
    }

    async fn fetch_structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<MessageStructure>, BackendError> {
        self.ensure_open(mailbox)?;
        if uid == 0 {
            return Ok(None);
        }
        let line = format!("UID FETCH {uid} ({STRUCTURE_ITEMS})");
        let mut collector = StructureCollector::new(uid);
        self.conn
            .run(&line, |resp| collector.take(resp))
            .await?
            .check("UID FETCH")?;
        if collector.unsolicited {
            self.conn.watch.changed = true;
        }
        Ok(collector.finish())
    }

    async fn fetch_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<RawMessage>, BackendError> {
        self.ensure_open(mailbox)?;
        if uids.is_empty() {
            return Ok(Vec::new());
        }
        let line = format!("UID FETCH {uids} ({MESSAGE_ITEMS})");
        let mut collector = MessageCollector::new(uids);
        self.conn
            .run(&line, |resp| collector.take(resp))
            .await?
            .check("UID FETCH")?;
        if collector.unsolicited {
            self.conn.watch.changed = true;
        }
        Ok(collector.messages)
    }

    async fn fetch_sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
        self.ensure_open(mailbox)?;
        if uid == 0 {
            return Ok(None);
        }
        if sections.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let line = format!("UID FETCH {uid} ({})", section_items(sections));
        let mut collector = SectionCollector::new(uid, sections);
        self.conn
            .run(&line, |resp| collector.take(resp))
            .await?
            .check("UID FETCH")?;
        if collector.unsolicited {
            self.conn.watch.changed = true;
        }
        Ok(collector.sections)
    }

    async fn idle<St>(
        &mut self,
        mailbox: &SelectedMailbox,
        max: Duration,
        stop: St,
    ) -> Result<IdleOutcome, BackendError>
    where
        St: Future<Output = ()> + Send,
    {
        self.ensure_open(mailbox)?;
        if !self.caps.idle {
            return Err(BackendError::Unsupported("IDLE".into()));
        }
        if self.conn.watch.changed {
            self.conn.watch.changed = false;
            return Ok(IdleOutcome::Changed);
        }
        self.conn.idle(max.min(MAX_IDLE), stop).await
    }

    async fn noop(&mut self) -> Result<(), BackendError> {
        self.conn.run("NOOP", |_| false).await?.check("NOOP")?;
        Ok(())
    }

    async fn logout(self) -> Result<(), BackendError> {
        self.conn.logout().await
    }
}

fn mailbox_status(attrs: &[StatusAttribute]) -> MailboxStatus {
    let mut status = MailboxStatus {
        messages: 0,
        unseen: None,
        uid_next: None,
        uid_validity: None,
        highest_modseq: None,
    };
    for attr in attrs {
        match attr {
            StatusAttribute::Messages(n) => status.messages = *n,
            StatusAttribute::Unseen(n) => status.unseen = Some(*n),
            StatusAttribute::UidNext(n) => status.uid_next = Some(*n),
            StatusAttribute::UidValidity(v) => status.uid_validity = Some(UidValidity(*v)),
            StatusAttribute::HighestModSeq(m) => status.highest_modseq = Some(ModSeq(*m)),
            _ => {}
        }
    }
    status
}
