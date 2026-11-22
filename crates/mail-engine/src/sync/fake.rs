//! An in-memory mail server behind [`MailConnector`]/[`MailBackend`], for fast and exact engine
//! tests: mailboxes with UIDVALIDITY, UIDNEXT and mod-sequences, IDLE that wakes on changes, and
//! fault injection (refused or failing connects, a connection cut after some fetches, slow
//! fetches, a panic like imap-proto's on `INTERNALDATE NIL`). The test server tests in
//! `tests/server.rs` cover the real adapter.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mail_model::{
    Access, Address, BackendError, BodySection, Capabilities, ConnectConfig, FetchedSection,
    FlagUpdate, Flags, IdleOutcome, MailBackend, MailConnector, MailboxStatus, MessageHeader,
    MessageStructure, MimePart, ModSeq, RawMessage, RemoteMailbox, SelectedMailbox, Uid, UidSet,
    UidValidity,
};
use tokio::sync::watch;

pub(crate) const PASSWORD: &str = "fake-pw";

#[derive(Debug, Clone)]
pub(crate) struct FakeMessage {
    pub flags: Flags,
    pub modseq: u64,
    pub serial: u64,
    /// The raw message; `None` for the generated plain-text one ([`FakeServer::raw_of`]).
    pub raw: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub(crate) struct FakeMailbox {
    pub uid_validity: u32,
    pub uid_next: Uid,
    pub modseq: u64,
    pub attributes: Vec<String>,
    pub messages: BTreeMap<Uid, FakeMessage>,
    /// Bumped on every change, for IDLE.
    pub changes: u64,
}

/// Faults the next operations run into.
#[derive(Debug, Default)]
pub(crate) struct Faults {
    /// Every connect fails with this.
    pub connect: Option<BackendError>,
    /// Every `fetch_headers` panics.
    pub panic_on_fetch: bool,
    /// The connection breaks after this many more `fetch_headers` calls (once).
    pub cut_after_fetches: Option<usize>,
    /// Each `fetch_headers` takes this long.
    pub fetch_delay: Duration,
    /// Every body fetch (structure, messages, sections) fails with this.
    pub bodies: Option<BackendError>,
    /// Each body fetch takes this long.
    pub body_delay: Duration,
}

#[derive(Debug)]
pub(crate) struct ServerState {
    pub mailboxes: BTreeMap<String, FakeMailbox>,
    pub password: String,
    pub capabilities: Vec<&'static str>,
    pub faults: Faults,
    /// Successful logins.
    pub logins: usize,
    /// UIDs returned by `fetch_headers`, in total.
    pub fetched: usize,
    /// Access mode of every connect attempt.
    pub access: Vec<Access>,
    /// Body fetch commands (`fetch_structure`, `fetch_messages`, `fetch_sections`), and the
    /// UIDs whose whole body or sections were returned.
    pub body_fetches: usize,
    pub bodies_sent: Vec<Uid>,
    next_validity: u32,
    next_serial: u64,
}

impl ServerState {
    fn mailbox(&mut self, name: &str) -> &mut FakeMailbox {
        self.mailboxes
            .get_mut(name)
            .unwrap_or_else(|| panic!("fake server has no mailbox {name}"))
    }
}

/// The server. Mutations through [`FakeServer::with`] wake idling connections.
#[derive(Debug)]
pub(crate) struct FakeServer {
    state: Mutex<ServerState>,
    version: watch::Sender<u64>,
    /// Backends alive right now (connections open).
    open: AtomicUsize,
}

impl FakeServer {
    /// INBOX, Sent (`\Sent`) and Archive, empty; IMAP4rev1, IDLE, CONDSTORE.
    pub(crate) fn new() -> Arc<Self> {
        let mut state = ServerState {
            mailboxes: BTreeMap::new(),
            password: PASSWORD.to_owned(),
            capabilities: vec!["IMAP4rev1", "IDLE", "CONDSTORE", "ENABLE", "SPECIAL-USE"],
            faults: Faults::default(),
            logins: 0,
            fetched: 0,
            access: Vec::new(),
            body_fetches: 0,
            bodies_sent: Vec::new(),
            next_validity: 100,
            next_serial: 1,
        };
        for (name, attributes) in [
            ("INBOX", vec![]),
            ("Sent", vec!["\\Sent"]),
            ("Archive", vec![]),
        ] {
            state.next_validity += 1;
            state.mailboxes.insert(
                name.to_owned(),
                FakeMailbox {
                    uid_validity: state.next_validity,
                    uid_next: 1,
                    modseq: 1,
                    attributes: attributes.into_iter().map(str::to_owned).collect(),
                    messages: BTreeMap::new(),
                    changes: 0,
                },
            );
        }
        Arc::new(Self {
            state: Mutex::new(state),
            version: watch::channel(0).0,
            open: AtomicUsize::new(0),
        })
    }

    fn lock(&self) -> MutexGuard<'_, ServerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads or changes the state; wakes IDLE afterwards.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut ServerState) -> R) -> R {
        let result = f(&mut self.lock());
        self.version.send_modify(|v| *v += 1);
        result
    }

    pub(crate) fn read<R>(&self, f: impl FnOnce(&ServerState) -> R) -> R {
        f(&self.lock())
    }

    pub(crate) fn open_connections(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }

    /// Delivers `n` messages to `mailbox`; returns their UIDs.
    pub(crate) fn deliver(&self, mailbox: &str, n: usize, seen: bool) -> Vec<Uid> {
        self.with(|s| {
            let mut uids = Vec::with_capacity(n);
            for _ in 0..n {
                let serial = s.next_serial;
                s.next_serial += 1;
                let mb = s.mailbox(mailbox);
                let uid = mb.uid_next;
                mb.uid_next += 1;
                mb.modseq += 1;
                mb.changes += 1;
                let flags = Flags {
                    seen,
                    ..Flags::default()
                };
                let modseq = mb.modseq;
                mb.messages.insert(
                    uid,
                    FakeMessage {
                        flags,
                        modseq,
                        serial,
                        raw: None,
                    },
                );
                uids.push(uid);
            }
            uids
        })
    }

    /// Delivers one message with this raw content; returns its UID.
    pub(crate) fn deliver_raw(&self, mailbox: &str, raw: &[u8]) -> Uid {
        let uid = self.deliver(mailbox, 1, false)[0];
        self.with(|s| {
            if let Some(message) = s.mailbox(mailbox).messages.get_mut(&uid) {
                message.raw = Some(raw.to_vec());
            }
        });
        uid
    }

    /// The raw message: the stored one, or a small generated text mail with a link.
    pub(crate) fn raw_of(message: &FakeMessage) -> Vec<u8> {
        if let Some(raw) = &message.raw {
            return raw.clone();
        }
        let serial = message.serial;
        format!(
            "From: Sender <sender@fake.test>\r\nTo: alice@fake.test\r\n\
             Subject: Message {serial}\r\nMessage-ID: <{serial}@fake.test>\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\n\
             Body of message {serial}, see https://example.org/{serial} \u{2013} gr\u{fc}\u{df}e.\r\n"
        )
        .into_bytes()
    }

    pub(crate) fn set_flagged(&self, mailbox: &str, uid: Uid, flagged: bool) {
        self.with(|s| {
            let mb = s.mailbox(mailbox);
            mb.modseq += 1;
            mb.changes += 1;
            let modseq = mb.modseq;
            let message = mb.messages.get_mut(&uid).expect("message");
            message.flags.flagged = flagged;
            message.modseq = modseq;
        });
    }

    pub(crate) fn expunge(&self, mailbox: &str, uid: Uid) {
        self.with(|s| {
            let mb = s.mailbox(mailbox);
            mb.messages.remove(&uid);
            mb.modseq += 1;
            mb.changes += 1;
        });
    }

    /// Deletes and creates the mailbox again with the same messages under new UIDs and a new
    /// UIDVALIDITY (what servers do after a rebuild).
    pub(crate) fn recreate(&self, mailbox: &str) {
        self.with(|s| {
            s.next_validity += 1;
            let validity = s.next_validity;
            let mb = s.mailbox(mailbox);
            let old: Vec<FakeMessage> = mb.messages.values().cloned().collect();
            mb.uid_validity = validity;
            mb.messages.clear();
            mb.uid_next = 1;
            mb.modseq += 1;
            mb.changes += 1;
            for message in old {
                let uid = mb.uid_next;
                mb.uid_next += 1;
                mb.messages.insert(uid, message);
            }
        });
    }

    fn header(serial: u64, uid: Uid, message: &FakeMessage) -> MessageHeader {
        let date = 1_700_000_000 + i64::try_from(serial).unwrap_or(0) * 60;
        MessageHeader {
            uid,
            modseq: Some(ModSeq(message.modseq)),
            flags: message.flags.clone(),
            size: u32::try_from(FakeServer::raw_of(message).len()).unwrap_or(u32::MAX),
            internal_date: date,
            date: Some(date),
            subject: message
                .raw
                .as_deref()
                .and_then(|raw| raw_header(raw, "Subject"))
                .unwrap_or_else(|| format!("Message {serial}")),
            from: vec![
                message
                    .raw
                    .as_deref()
                    .and_then(|raw| raw_header(raw, "From"))
                    .map_or_else(
                        || Address {
                            name: Some("Sender".into()),
                            email: "sender@fake.test".into(),
                        },
                        |from| match from.split_once('<') {
                            Some((name, rest)) => Address {
                                name: Some(name.trim().to_owned()).filter(|n| !n.is_empty()),
                                email: rest.trim_end_matches('>').trim().to_owned(),
                            },
                            None => Address {
                                name: None,
                                email: from.trim().to_owned(),
                            },
                        },
                    ),
            ],
            to: vec![Address {
                name: None,
                email: "alice@fake.test".into(),
            }],
            cc: Vec::new(),
            message_id: Some(format!("{serial}@fake.test")),
            in_reply_to: None,
            references: Vec::new(),
            list_id: None,
            list_unsubscribe: None,
            auto_submitted: None,
            precedence: None,
            has_attachments: false,
        }
    }
}

/// A header field of a raw message (one unfolded line), for the header the fake reports.
fn raw_header(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let prefix = format!("{name}: ");
    text.lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix(&prefix))
        .map(str::to_owned)
}

#[derive(Debug, Clone)]
pub(crate) struct FakeConnector(pub Arc<FakeServer>);

impl MailConnector for FakeConnector {
    type Backend = FakeBackend;

    async fn connect(&self, config: &ConnectConfig) -> Result<FakeBackend, BackendError> {
        let server = Arc::clone(&self.0);
        let capabilities = server.with(|s| {
            s.access.push(config.access);
            if let Some(err) = &s.faults.connect {
                return Err(err.clone());
            }
            if config.password.expose_secret() != &s.password {
                return Err(BackendError::AuthFailed(
                    "[AUTHENTICATIONFAILED] Authentication failed.".into(),
                ));
            }
            s.logins += 1;
            Ok(Capabilities::from_names(s.capabilities.iter().copied()))
        })?;
        server.open.fetch_add(1, Ordering::SeqCst);
        Ok(FakeBackend {
            server,
            capabilities,
            access: config.access,
            selected: None,
            idle_seen: 0,
            poisoned: false,
        })
    }
}

#[derive(Debug)]
pub(crate) struct FakeBackend {
    server: Arc<FakeServer>,
    capabilities: Capabilities,
    access: Access,
    selected: Option<SelectedMailbox>,
    /// `changes` of the open mailbox that IDLE has reported.
    idle_seen: u64,
    poisoned: bool,
}

impl Drop for FakeBackend {
    fn drop(&mut self) {
        self.server.open.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FakeBackend {
    /// One body fetch: counted, delayed and failed as the faults say.
    async fn body_of(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<Vec<u8>>, BackendError> {
        self.check(mailbox)?;
        let (fault, delay) = self.server.with(|s| {
            s.body_fetches += 1;
            (s.faults.bodies.clone(), s.faults.body_delay)
        });
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if let Some(err) = fault {
            if matches!(err, BackendError::ConnectionLost(_)) {
                self.poisoned = true;
            }
            return Err(err);
        }
        let raw = self.server.read(|s| {
            s.mailboxes
                .get(&mailbox.name)
                .filter(|mb| mb.uid_validity == mailbox.uid_validity.0)
                .and_then(|mb| mb.messages.get(&uid))
                .map(FakeServer::raw_of)
        });
        if raw.is_some() {
            self.server.with(|s| s.bodies_sent.push(uid));
        }
        Ok(raw)
    }

    fn usable(&self) -> Result<(), BackendError> {
        if self.poisoned {
            Err(BackendError::ConnectionLost(
                "fake connection was cut".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn check(&self, mailbox: &SelectedMailbox) -> Result<(), BackendError> {
        self.usable()?;
        match &self.selected {
            Some(open) if open == mailbox => Ok(()),
            _ => Err(BackendError::InvalidState(format!(
                "{} is not open",
                mailbox.name
            ))),
        }
    }

    fn condstore(&self) -> bool {
        self.capabilities.condstore
    }
}

impl MailBackend for FakeBackend {
    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn access(&self) -> Access {
        self.access
    }

    async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
        self.usable()?;
        let condstore = self.condstore();
        Ok(self.server.read(|s| {
            s.mailboxes
                .iter()
                .map(|(name, mb)| RemoteMailbox {
                    name: name.clone(),
                    path: name.clone(),
                    delimiter: Some('/'),
                    attributes: mb.attributes.clone(),
                    status: Some(MailboxStatus {
                        messages: u32::try_from(mb.messages.len()).unwrap_or(u32::MAX),
                        unseen: None,
                        uid_next: Some(mb.uid_next),
                        uid_validity: Some(UidValidity(mb.uid_validity)),
                        highest_modseq: condstore.then_some(ModSeq(mb.modseq)),
                    }),
                })
                .collect()
        }))
    }

    async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        self.usable()?;
        self.selected = None;
        let condstore = self.condstore();
        let (selected, changes) = self
            .server
            .read(|s| {
                s.mailboxes.get(name).map(|mb| {
                    (
                        SelectedMailbox {
                            name: name.to_owned(),
                            uid_validity: UidValidity(mb.uid_validity),
                            uid_next: mb.uid_next,
                            exists: u32::try_from(mb.messages.len()).unwrap_or(u32::MAX),
                            highest_modseq: condstore.then_some(ModSeq(mb.modseq)),
                            read_only: true,
                            keywords_allowed: false,
                        },
                        mb.changes,
                    )
                })
            })
            .ok_or_else(|| BackendError::MailboxNotFound(name.to_owned()))?;
        self.idle_seen = changes;
        self.selected = Some(selected.clone());
        Ok(selected)
    }

    async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
        self.check(mailbox)?;
        Ok(self.server.read(|s| {
            s.mailboxes
                .get(&mailbox.name)
                .filter(|mb| mb.uid_validity == mailbox.uid_validity.0)
                .map(|mb| UidSet::from_uids(mb.messages.keys().copied()))
                .unwrap_or_default()
        }))
    }

    async fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<MessageHeader>, BackendError> {
        self.check(mailbox)?;
        let (panic, cut, delay) = self.server.with(|s| {
            let cut = match s.faults.cut_after_fetches.as_mut() {
                Some(0) => {
                    s.faults.cut_after_fetches = None;
                    true
                }
                Some(n) => {
                    *n -= 1;
                    false
                }
                None => false,
            };
            (s.faults.panic_on_fetch, cut, s.faults.fetch_delay)
        });
        assert!(!panic, "INTERNALDATE NIL: imap-proto would panic here");
        if cut {
            self.poisoned = true;
            return Err(BackendError::ConnectionLost("fake: cut mid-fetch".into()));
        }
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        let headers: Vec<MessageHeader> = self.server.read(|s| {
            let Some(mb) = s
                .mailboxes
                .get(&mailbox.name)
                .filter(|mb| mb.uid_validity == mailbox.uid_validity.0)
            else {
                return Vec::new();
            };
            mb.messages
                .iter()
                .filter(|(uid, _)| uids.contains(**uid))
                .map(|(uid, m)| FakeServer::header(m.serial, *uid, m))
                .collect()
        });
        self.server.with(|s| s.fetched += headers.len());
        Ok(headers)
    }

    async fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        changed_since: Option<ModSeq>,
    ) -> Result<Vec<FlagUpdate>, BackendError> {
        self.check(mailbox)?;
        if changed_since.is_some() && mailbox.highest_modseq.is_none() {
            return Err(BackendError::Unsupported("CONDSTORE".into()));
        }
        let since = changed_since.map_or(0, |m| m.0);
        Ok(self.server.read(|s| {
            s.mailboxes
                .get(&mailbox.name)
                .map(|mb| {
                    mb.messages
                        .iter()
                        .filter(|(uid, m)| uids.contains(**uid) && m.modseq > since)
                        .map(|(uid, m)| FlagUpdate {
                            uid: *uid,
                            flags: m.flags.clone(),
                            modseq: Some(ModSeq(m.modseq)),
                        })
                        .collect()
                })
                .unwrap_or_default()
        }))
    }

    async fn idle<S>(
        &mut self,
        mailbox: &SelectedMailbox,
        max: Duration,
        stop: S,
    ) -> Result<IdleOutcome, BackendError>
    where
        S: Future<Output = ()> + Send,
    {
        self.check(mailbox)?;
        let mut version = self.server.version.subscribe();
        let deadline = tokio::time::Instant::now() + max;
        let mut stop = std::pin::pin!(stop);
        loop {
            let changes = self.server.read(|s| {
                s.mailboxes
                    .get(&mailbox.name)
                    .map(|mb| (mb.changes, mb.uid_validity))
            });
            match changes {
                Some((_, validity)) if validity != mailbox.uid_validity.0 => {
                    // Recreated under the same name: a real server ends the connection.
                    self.poisoned = true;
                    return Err(BackendError::ConnectionLost(
                        "fake: mailbox was recreated".into(),
                    ));
                }
                Some((changes, _)) if changes != self.idle_seen => {
                    self.idle_seen = changes;
                    return Ok(IdleOutcome::Changed);
                }
                _ => {}
            }
            tokio::select! {
                biased;
                () = &mut stop => return Ok(IdleOutcome::Stopped),
                () = tokio::time::sleep_until(deadline) => return Ok(IdleOutcome::TimedOut),
                changed = version.changed() => {
                    if changed.is_err() {
                        return Err(BackendError::ConnectionLost("fake server gone".into()));
                    }
                }
            }
        }
    }

    async fn fetch_structure(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
    ) -> Result<Option<MessageStructure>, BackendError> {
        let raw = self.body_of(mailbox, uid).await?;
        Ok(raw.map(|raw| MessageStructure {
            size: u32::try_from(raw.len()).unwrap_or(u32::MAX),
            parts: vec![MimePart {
                path: "1".parse().ok(),
                depth: 0,
                mime_type: "text/plain".into(),
                charset: Some("utf-8".into()),
                file_name: None,
                disposition: None,
                content_id: None,
                encoding: "8bit".into(),
                size: u32::try_from(raw.len()).unwrap_or(u32::MAX),
            }],
        }))
    }

    async fn fetch_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<RawMessage>, BackendError> {
        let mut out = Vec::new();
        for uid in uids.iter() {
            if let Some(data) = self.body_of(mailbox, uid).await? {
                out.push(RawMessage { uid, data });
            }
        }
        Ok(out)
    }

    async fn fetch_sections(
        &mut self,
        mailbox: &SelectedMailbox,
        uid: Uid,
        sections: &[BodySection],
    ) -> Result<Option<Vec<FetchedSection>>, BackendError> {
        let Some(raw) = self.body_of(mailbox, uid).await? else {
            return Ok(None);
        };
        // A single-part message: the header and part 1 (the body).
        let split = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map_or(raw.len(), |p| p + 4);
        let (header, body) = raw.split_at(split);
        Ok(Some(
            sections
                .iter()
                .filter_map(|section| {
                    let data = match section {
                        BodySection::Header => header.to_vec(),
                        BodySection::Part(path) if path.to_string() == "1" => body.to_vec(),
                        _ => return None,
                    };
                    Some(FetchedSection {
                        section: section.clone(),
                        data,
                    })
                })
                .collect(),
        ))
    }

    async fn noop(&mut self) -> Result<(), BackendError> {
        self.usable()
    }

    async fn logout(self) -> Result<(), BackendError> {
        self.usable()
    }
}
