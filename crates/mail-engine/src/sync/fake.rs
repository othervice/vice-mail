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
    Access, Address, BackendError, Capabilities, ConnectConfig, FlagUpdate, Flags, IdleOutcome,
    MailBackend, MailConnector, MailboxStatus, MessageHeader, ModSeq, RemoteMailbox,
    SelectedMailbox, Uid, UidSet, UidValidity,
};
use tokio::sync::watch;

pub(crate) const PASSWORD: &str = "fake-pw";

#[derive(Debug, Clone)]
pub(crate) struct FakeMessage {
    pub flags: Flags,
    pub modseq: u64,
    pub serial: u64,
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
                    },
                );
                uids.push(uid);
            }
            uids
        })
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
            size: 1_000,
            internal_date: date,
            date: Some(date),
            subject: format!("Message {serial}"),
            from: vec![Address {
                name: Some("Sender".into()),
                email: "sender@fake.test".into(),
            }],
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

    async fn noop(&mut self) -> Result<(), BackendError> {
        self.usable()
    }

    async fn logout(self) -> Result<(), BackendError> {
        self.usable()
    }
}
