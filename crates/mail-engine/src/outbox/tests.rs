//! The outbox engine without a server: an in-memory IMAP-like server behind [`MailWriter`],
//! with fault injection ("the connection breaks right after this command was applied"), and
//! the real store in a temporary directory. The same rules as the server tests in
//! `server_tests`, but these run in `scripts/check.sh`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mail_model::{
    Access, AccountId, ActionError, Address, BackendError, Capabilities, ConflictReason,
    ConnectConfig, CopyUid, CoreError, ExpungeOutcome, Flag, FlagUpdate, Flags, IdleOutcome,
    MailAction, MailBackend, MailWriter, MailboxRole, MailboxStatus, MessageHeader, MessageRef,
    ModSeq, MoveMethod, MoveOutcome, OperationState, OutboxEvent, RemoteMailbox, Security,
    SelectedMailbox, ServerSettings, StoreMode, StoreOutcome, Uid, UidSet, UidValidity,
    WriteConnector,
};
use mail_store::{MailboxBaseline, NewAccount, Store, StoreOptions, StoredOperation};
use tempfile::TempDir;
use tokio::time::Instant;

use super::{Outbox, OutboxConfig};
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore, SecretStore};
use crate::sync_state::BackoffPolicy;

// ---------------------------------------------------------------------------------------------
// In-memory server

#[derive(Debug, Clone)]
struct Msg {
    flags: Flags,
    modseq: u64,
    message_id: String,
}

#[derive(Debug, Clone, Default)]
struct FakeBox {
    uid_validity: u32,
    uid_next: Uid,
    messages: BTreeMap<Uid, Msg>,
}

#[derive(Debug, Default)]
struct Server {
    boxes: BTreeMap<String, FakeBox>,
    modseq: u64,
    /// Every command name, in order.
    log: Vec<String>,
    /// Apply the next command with this name, then break the connection.
    cut_after: Option<&'static str>,
}

impl Server {
    fn next_modseq(&mut self) -> u64 {
        self.modseq += 1;
        self.modseq
    }

    fn add(&mut self, mailbox: &str, message_id: &str) -> Uid {
        let modseq = self.next_modseq();
        let b = self
            .boxes
            .entry(mailbox.to_owned())
            .or_insert_with(|| FakeBox {
                uid_validity: 7,
                uid_next: 1,
                messages: BTreeMap::new(),
            });
        let uid = b.uid_next;
        b.uid_next += 1;
        b.messages.insert(
            uid,
            Msg {
                flags: Flags::default(),
                modseq,
                message_id: message_id.to_owned(),
            },
        );
        uid
    }

    /// Where a Message-ID lies: (mailbox, UID).
    fn find(&self, message_id: &str) -> Vec<(String, Uid)> {
        self.boxes
            .iter()
            .flat_map(|(name, b)| {
                b.messages
                    .iter()
                    .filter(|(_, m)| m.message_id == message_id)
                    .map(move |(uid, _)| (name.clone(), *uid))
            })
            .collect()
    }

    fn count(&self, verb: &str) -> usize {
        self.log.iter().filter(|v| *v == verb).count()
    }
}

type Shared = Arc<Mutex<Server>>;

fn lock(server: &Shared) -> MutexGuard<'_, Server> {
    server.lock().unwrap_or_else(PoisonError::into_inner)
}

struct FakeConnector {
    server: Shared,
    caps: Capabilities,
    offline: Arc<AtomicBool>,
}

impl WriteConnector for FakeConnector {
    type Writer = FakeWriter;

    async fn connect_writer(&self, config: &ConnectConfig) -> Result<FakeWriter, BackendError> {
        assert_eq!(config.access, Access::ReadWrite);
        if self.offline.load(Ordering::SeqCst) {
            return Err(BackendError::ConnectionRefused);
        }
        Ok(FakeWriter {
            server: Arc::clone(&self.server),
            caps: self.caps.clone(),
            offline: Arc::clone(&self.offline),
            selected: None,
            broken: false,
        })
    }
}

struct FakeWriter {
    server: Shared,
    caps: Capabilities,
    /// Set when a cut happens: like a crash, nothing reconnects until the test says so.
    offline: Arc<AtomicBool>,
    selected: Option<String>,
    broken: bool,
}

impl FakeWriter {
    /// Logs the command; fails if the connection broke before.
    fn begin(&mut self, verb: &str) -> Result<MutexGuard<'_, Server>, BackendError> {
        if self.broken {
            return Err(BackendError::ConnectionLost("broken".into()));
        }
        let mut server = lock(&self.server);
        server.log.push(verb.to_owned());
        Ok(server)
    }

    /// After a command was applied: break now if the test asked for it.
    fn end(&mut self, verb: &str) -> Result<(), BackendError> {
        let mut server = lock(&self.server);
        if server.cut_after == Some(verb) {
            server.cut_after = None;
            drop(server);
            self.offline.store(true, Ordering::SeqCst);
            self.broken = true;
            return Err(BackendError::ConnectionLost(format!("cut after {verb}")));
        }
        Ok(())
    }

    fn open(&self, mailbox: &SelectedMailbox) -> Result<(), BackendError> {
        if self.selected.as_deref() == Some(mailbox.name.as_str()) {
            Ok(())
        } else {
            Err(BackendError::InvalidState("not selected".into()))
        }
    }
}

impl MailBackend for FakeWriter {
    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn access(&self) -> Access {
        Access::ReadWrite
    }

    async fn list_mailboxes(&mut self) -> Result<Vec<RemoteMailbox>, BackendError> {
        let server = self.begin("LIST")?;
        Ok(server.boxes.keys().map(|name| remote(name)).collect())
    }

    async fn open_mailbox(&mut self, name: &str) -> Result<SelectedMailbox, BackendError> {
        self.selected = None;
        let server = self.begin("SELECT")?;
        let b = server
            .boxes
            .get(name)
            .ok_or_else(|| BackendError::MailboxNotFound(name.to_owned()))?;
        let selected = SelectedMailbox {
            name: name.to_owned(),
            uid_validity: UidValidity(b.uid_validity),
            uid_next: b.uid_next,
            exists: u32::try_from(b.messages.len()).unwrap_or(u32::MAX),
            highest_modseq: Some(ModSeq(server.modseq)),
            read_only: false,
            keywords_allowed: true,
        };
        drop(server);
        self.selected = Some(name.to_owned());
        Ok(selected)
    }

    async fn search_uids(&mut self, mailbox: &SelectedMailbox) -> Result<UidSet, BackendError> {
        self.open(mailbox)?;
        let server = self.begin("UID SEARCH")?;
        Ok(UidSet::from_uids(
            server.boxes[&mailbox.name].messages.keys().copied(),
        ))
    }

    async fn fetch_headers(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<Vec<MessageHeader>, BackendError> {
        self.open(mailbox)?;
        let server = self.begin("UID FETCH")?;
        Ok(server.boxes[&mailbox.name]
            .messages
            .iter()
            .filter(|(uid, _)| uids.contains(**uid))
            .map(|(uid, m)| header(*uid, m))
            .collect())
    }

    async fn fetch_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        _changed_since: Option<ModSeq>,
    ) -> Result<Vec<FlagUpdate>, BackendError> {
        self.open(mailbox)?;
        let server = self.begin("UID FETCH")?;
        Ok(server.boxes[&mailbox.name]
            .messages
            .iter()
            .filter(|(uid, _)| uids.contains(**uid))
            .map(|(uid, m)| FlagUpdate {
                uid: *uid,
                flags: m.flags.clone(),
                modseq: Some(ModSeq(m.modseq)),
            })
            .collect())
    }

    async fn idle<S>(
        &mut self,
        _mailbox: &SelectedMailbox,
        _max: Duration,
        _stop: S,
    ) -> Result<IdleOutcome, BackendError>
    where
        S: std::future::Future<Output = ()> + Send,
    {
        Err(BackendError::Unsupported("IDLE".into()))
    }

    async fn noop(&mut self) -> Result<(), BackendError> {
        drop(self.begin("NOOP")?);
        Ok(())
    }

    async fn logout(self) -> Result<(), BackendError> {
        Ok(())
    }
}

impl MailWriter for FakeWriter {
    async fn mailbox_status(&mut self, name: &str) -> Result<MailboxStatus, BackendError> {
        let server = self.begin("STATUS")?;
        let b = server
            .boxes
            .get(name)
            .ok_or_else(|| BackendError::MailboxNotFound(name.to_owned()))?;
        Ok(MailboxStatus {
            messages: u32::try_from(b.messages.len()).unwrap_or(u32::MAX),
            unseen: None,
            uid_next: Some(b.uid_next),
            uid_validity: Some(UidValidity(b.uid_validity)),
            highest_modseq: None,
        })
    }

    async fn store_flags(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<ModSeq>,
    ) -> Result<StoreOutcome, BackendError> {
        self.open(mailbox)?;
        let mut server = self.begin("UID STORE")?;
        let mut outcome = StoreOutcome::default();
        let mut modified = Vec::new();
        let candidates: Vec<Uid> = server.boxes[&mailbox.name]
            .messages
            .keys()
            .copied()
            .filter(|uid| uids.contains(*uid))
            .collect();
        for uid in candidates {
            let modseq = server.next_modseq();
            let Some(m) = server
                .boxes
                .get_mut(&mailbox.name)
                .and_then(|b| b.messages.get_mut(&uid))
            else {
                continue;
            };
            if unchanged_since.is_some_and(|since| m.modseq > since.0) {
                modified.push(uid);
                continue;
            }
            for flag in flags {
                flag.apply(&mut m.flags, mode == StoreMode::Add);
            }
            m.modseq = modseq;
            outcome.updates.push(FlagUpdate {
                uid,
                flags: m.flags.clone(),
                modseq: Some(ModSeq(modseq)),
            });
        }
        outcome.modified = UidSet::from_uids(modified);
        drop(server);
        self.end("UID STORE")?;
        Ok(outcome)
    }

    async fn move_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
        target: &str,
    ) -> Result<MoveOutcome, BackendError> {
        self.open(mailbox)?;
        let with_move = self.caps.move_;
        let verb = if with_move { "UID MOVE" } else { "UID COPY" };
        let mut server = self.begin(verb)?;
        if !server.boxes.contains_key(target) {
            return Err(BackendError::Rejected("UID MOVE: NO [TRYCREATE]".into()));
        }
        let moving: Vec<(Uid, Msg)> = server.boxes[&mailbox.name]
            .messages
            .iter()
            .filter(|(uid, _)| uids.contains(**uid))
            .map(|(uid, m)| (*uid, m.clone()))
            .collect();
        let mut pairs = Vec::new();
        for (uid, mut msg) in moving {
            msg.modseq = server.next_modseq();
            let Some(t) = server.boxes.get_mut(target) else {
                continue;
            };
            let new = t.uid_next;
            t.uid_next += 1;
            t.messages.insert(new, msg);
            pairs.push((uid, new));
            if with_move && let Some(source) = server.boxes.get_mut(&mailbox.name) {
                source.messages.remove(&uid);
            }
        }
        let validity = UidValidity(server.boxes[target].uid_validity);
        drop(server);
        self.end(verb)?;
        if !with_move {
            // The COPY fallback: \Deleted and UID EXPUNGE (the fake has UIDPLUS).
            let mut server = self.begin("UID EXPUNGE")?;
            if let Some(source) = server.boxes.get_mut(&mailbox.name) {
                for (uid, _) in &pairs {
                    source.messages.remove(uid);
                }
            }
        }
        Ok(MoveOutcome {
            method: if with_move {
                MoveMethod::Move
            } else {
                MoveMethod::CopyExpunge
            },
            copy_uid: Some(CopyUid {
                uid_validity: validity,
                pairs,
            }),
        })
    }

    async fn expunge_messages(
        &mut self,
        mailbox: &SelectedMailbox,
        uids: &UidSet,
    ) -> Result<ExpungeOutcome, BackendError> {
        self.open(mailbox)?;
        let mut server = self.begin("UID EXPUNGE")?;
        if let Some(b) = server.boxes.get_mut(&mailbox.name) {
            b.messages.retain(|uid, _| !uids.contains(*uid));
        }
        drop(server);
        self.end("UID EXPUNGE")?;
        Ok(ExpungeOutcome::Expunged)
    }

    async fn create_mailbox(
        &mut self,
        path: &str,
        _role: Option<MailboxRole>,
    ) -> Result<String, BackendError> {
        let mut server = self.begin("CREATE")?;
        server
            .boxes
            .entry(path.to_owned())
            .or_insert_with(|| FakeBox {
                uid_validity: 9,
                uid_next: 1,
                messages: BTreeMap::new(),
            });
        Ok(path.to_owned())
    }
}

fn remote(name: &str) -> RemoteMailbox {
    let special = match name {
        "Archive" => Some("\\Archive"),
        "Trash" => Some("\\Trash"),
        "Junk" => Some("\\Junk"),
        _ => None,
    };
    RemoteMailbox {
        name: name.to_owned(),
        path: name.to_owned(),
        delimiter: Some('/'),
        attributes: special.into_iter().map(str::to_owned).collect(),
        status: None,
    }
}

fn header(uid: Uid, m: &Msg) -> MessageHeader {
    MessageHeader {
        uid,
        modseq: Some(ModSeq(m.modseq)),
        flags: m.flags.clone(),
        size: 100,
        internal_date: 1_764_000_000 + i64::from(uid),
        date: None,
        subject: m.message_id.clone(),
        from: vec![Address {
            name: None,
            email: "a@example.org".into(),
        }],
        to: Vec::new(),
        cc: Vec::new(),
        message_id: Some(m.message_id.clone()),
        in_reply_to: None,
        references: Vec::new(),
        list_id: None,
        list_unsubscribe: None,
        auto_submitted: None,
        precedence: None,
        has_attachments: false,
        category_headers: Vec::new(),
    }
}

// ---------------------------------------------------------------------------------------------
// Test bed

struct Bed {
    _dir: TempDir,
    path: std::path::PathBuf,
    store: Store,
    account: AccountId,
    credentials: Credentials,
    server: Shared,
    offline: Arc<AtomicBool>,
    outbox: Outbox<FakeConnector>,
}

fn caps(with_move: bool) -> Capabilities {
    let mut names = vec!["IMAP4rev1", "CONDSTORE", "UIDPLUS"];
    if with_move {
        names.push("MOVE");
    }
    Capabilities::from_names(names)
}

fn outbox_config() -> OutboxConfig {
    OutboxConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(5),
            max: Duration::from_millis(20),
            jitter: 0.0,
        },
        linger: Duration::from_millis(50),
        ..OutboxConfig::default()
    }
}

impl Bed {
    async fn new(messages: usize, with_move: bool) -> Self {
        let mut server = Server::default();
        for name in ["INBOX", "Archive", "Trash", "Junk"] {
            server.boxes.insert(
                name.to_owned(),
                FakeBox {
                    uid_validity: 7,
                    uid_next: 1,
                    messages: BTreeMap::new(),
                },
            );
        }
        for n in 1..=messages {
            server.add("INBOX", &format!("m{n}@fake"));
        }
        let server = Arc::new(Mutex::new(server));
        let dir = TempDir::new().expect("dir");
        let path = dir.path().join("mail.sqlite3");
        let store = Store::open(&path, StoreOptions::default()).expect("store");
        let account = AccountId::from_uuid(uuid::Uuid::new_v4());
        let key = CredentialRef::for_account(account);
        let keychain = MemorySecretStore::new();
        keychain
            .set(&key, &mail_model::Secret::new("pw".into()))
            .expect("keychain");
        let settings = ServerSettings {
            host: "fake.test".into(),
            port: 993,
            security: Security::Tls,
            username: "u".into(),
        };
        store
            .insert_account(NewAccount {
                id: account,
                email: "u@fake.test".into(),
                display_name: "U".into(),
                imap: settings.clone(),
                smtp: settings,
                credential_ref: key.to_string(),
            })
            .await
            .expect("account");
        store
            .set_writes_enabled(account, true)
            .await
            .expect("writes on");
        let offline = Arc::new(AtomicBool::new(false));
        let credentials = Credentials::new(keychain);
        let outbox = Outbox::new(
            store.clone(),
            Arc::new(FakeConnector {
                server: Arc::clone(&server),
                caps: caps(with_move),
                offline: Arc::clone(&offline),
            }),
            credentials.clone(),
            outbox_config(),
        );
        let bed = Self {
            _dir: dir,
            path,
            store,
            account,
            credentials,
            server,
            offline,
            outbox,
        };
        bed.sync().await;
        bed
    }

    /// The server's state into the store (what the sync does).
    async fn sync(&self) {
        let boxes = lock(&self.server).boxes.clone();
        self.store
            .sync_mailboxes(self.account, boxes.keys().map(|n| remote(n)).collect())
            .await
            .expect("mailboxes");
        for state in self
            .store
            .mailbox_sync_states(self.account)
            .await
            .expect("states")
        {
            let b = &boxes[&state.name];
            let uv = UidValidity(b.uid_validity);
            self.store
                .reset_mailbox(
                    state.mailbox_id,
                    MailboxBaseline {
                        uid_validity: uv,
                        uid_next: b.uid_next,
                        highest_modseq: None,
                    },
                )
                .await
                .expect("reset");
            let headers = b.messages.iter().map(|(uid, m)| header(*uid, m)).collect();
            self.store
                .store_headers(state.mailbox_id, uv, headers, Some(1))
                .await
                .expect("headers");
        }
    }

    async fn restart(&mut self, with_move: bool) {
        self.outbox.shutdown().await;
        self.store = Store::open(&self.path, StoreOptions::default()).expect("reopen");
        self.outbox = Outbox::new(
            self.store.clone(),
            Arc::new(FakeConnector {
                server: Arc::clone(&self.server),
                caps: caps(with_move),
                offline: Arc::clone(&self.offline),
            }),
            self.credentials.clone(),
            outbox_config(),
        );
    }

    /// Message `n` in the INBOX, as the list shows it.
    async fn inbox_ref(&self, n: usize) -> MessageRef {
        let inbox = self.mailbox("INBOX").await;
        let subject = format!("m{n}@fake");
        let item = self
            .store
            .list_messages(inbox, None, 500)
            .await
            .expect("list")
            .items
            .into_iter()
            .find(|i| i.subject == subject)
            .unwrap_or_else(|| panic!("m{n} not in the inbox"));
        MessageRef {
            mailbox_id: inbox,
            message_id: item.id,
        }
    }

    async fn mailbox(&self, name: &str) -> mail_model::MailboxId {
        self.store
            .mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == name)
            .map(|m| m.id)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    async fn op_of(&self, batch: Option<mail_model::BatchId>) -> StoredOperation {
        let batch = batch.expect("batch");
        self.store
            .operations(self.account, None, 100)
            .await
            .expect("ops")
            .into_iter()
            .find(|op| op.batch == batch)
            .expect("operation")
    }

    async fn wait(&self, id: mail_model::OperationId, state: OperationState) -> StoredOperation {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let op = self.store.operation(id).await.expect("read").expect("op");
            if op.state == state {
                return op;
            }
            assert!(Instant::now() < deadline, "stays {:?}: {op:?}", op.state);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests

#[tokio::test]
async fn offline_actions_wait_and_run_after_connecting() {
    let bed = Bed::new(2, true).await;
    let mut events = bed.outbox.subscribe();
    bed.offline.store(true, Ordering::SeqCst);
    let at = bed.inbox_ref(1).await;
    let result = bed
        .outbox
        .apply(MailAction::Archive, vec![at])
        .await
        .expect("apply");
    let op = bed.op_of(result.batch).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let waiting = bed.store.operation(op.id).await.expect("read").expect("op");
    assert_eq!(
        (waiting.state, waiting.attempts),
        (OperationState::Queued, 0)
    );
    assert_eq!(lock(&bed.server).find("m1@fake"), [("INBOX".to_owned(), 1)]);

    bed.offline.store(false, Ordering::SeqCst);
    bed.outbox.wake(bed.account);
    let done = bed.wait(op.id, OperationState::Confirmed).await;
    assert_eq!(done.attempts, 1);
    assert_eq!(
        lock(&bed.server).find("m1@fake"),
        [("Archive".to_owned(), 1)]
    );
    // Known in the target through COPYUID.
    let archive = bed.mailbox("Archive").await;
    assert!(
        bed.store
            .known_uids(archive)
            .await
            .expect("uids")
            .contains(1)
    );
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let OutboxEvent::OperationChanged { operation } = event {
            seen.push(operation.state);
        }
    }
    assert_eq!(seen.first(), Some(&OperationState::Queued));
    assert_eq!(seen.last(), Some(&OperationState::Confirmed));
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn a_move_cut_after_the_command_is_checked_not_repeated() {
    for with_move in [true, false] {
        let mut bed = Bed::new(1, with_move).await;
        let verb = if with_move { "UID MOVE" } else { "UID COPY" };
        lock(&bed.server).cut_after = Some(verb);
        // After the cut nothing reconnects (the fake goes offline), as after a crash.
        let at = bed.inbox_ref(1).await;
        let result = bed
            .outbox
            .apply(MailAction::Archive, vec![at])
            .await
            .expect("apply");
        let op = bed.op_of(result.batch).await;
        bed.wait(op.id, OperationState::InFlight).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(bed.offline.load(Ordering::SeqCst), "the cut happened");

        bed.restart(with_move).await;
        bed.offline.store(false, Ordering::SeqCst);
        bed.outbox.start().await.expect("start");
        let done = bed.wait(op.id, OperationState::Confirmed).await;
        {
            let server = lock(&bed.server);
            assert_eq!(
                server.find("m1@fake"),
                [("Archive".to_owned(), 1)],
                "exactly once ({verb})"
            );
            assert_eq!(
                server.count(verb),
                1,
                "{verb} not sent again: {:?}",
                server.log
            );
        }
        assert!(done.attempts <= 2, "{done:?}");
        bed.outbox.shutdown().await;
    }
}

#[tokio::test]
async fn a_flag_cut_after_the_store_is_confirmed_by_the_check() {
    let mut bed = Bed::new(1, true).await;
    lock(&bed.server).cut_after = Some("UID STORE");
    let at = bed.inbox_ref(1).await;
    let result = bed
        .outbox
        .apply(MailAction::Flag, vec![at])
        .await
        .expect("apply");
    let op = bed.op_of(result.batch).await;
    bed.wait(op.id, OperationState::InFlight).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    bed.restart(true).await;
    bed.offline.store(false, Ordering::SeqCst);
    bed.outbox.start().await.expect("start");
    bed.wait(op.id, OperationState::Confirmed).await;
    assert_eq!(lock(&bed.server).count("UID STORE"), 1);
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn changes_from_elsewhere_win_and_are_conflicts() {
    let bed = Bed::new(3, true).await;
    bed.offline.store(true, Ordering::SeqCst);
    let one = bed.inbox_ref(1).await;
    let two = bed.inbox_ref(2).await;
    let three = bed.inbox_ref(3).await;
    let moved = bed
        .outbox
        .apply(MailAction::Archive, vec![one])
        .await
        .expect("apply");
    let flagged = bed
        .outbox
        .apply(MailAction::MarkRead, vec![two])
        .await
        .expect("apply");
    let deleted = bed
        .outbox
        .apply(MailAction::DeletePermanently, vec![three])
        .await
        .expect("apply");
    {
        // Another client: moves 1 away, flags 2 (MODSEQ grows), deletes 3.
        let mut server = lock(&bed.server);
        let msg = server
            .boxes
            .get_mut("INBOX")
            .and_then(|b| b.messages.remove(&1))
            .expect("m1");
        if let Some(junk) = server.boxes.get_mut("Junk") {
            junk.messages.insert(9, msg);
        }
        let modseq = server.next_modseq();
        if let Some(m) = server
            .boxes
            .get_mut("INBOX")
            .and_then(|b| b.messages.get_mut(&2))
        {
            m.flags.flagged = true;
            m.modseq = modseq;
        }
        if let Some(inbox) = server.boxes.get_mut("INBOX") {
            inbox.messages.remove(&3);
        }
    }
    bed.offline.store(false, Ordering::SeqCst);
    bed.outbox.wake(bed.account);
    let op = bed.op_of(moved.batch).await;
    let op = bed.wait(op.id, OperationState::Conflict).await;
    assert_eq!(op.conflict, Some(ConflictReason::Gone));
    assert_eq!(lock(&bed.server).find("m1@fake"), [("Junk".to_owned(), 9)]);
    let op = bed.op_of(flagged.batch).await;
    let op = bed.wait(op.id, OperationState::Conflict).await;
    assert_eq!(op.conflict, Some(ConflictReason::Modified));
    let server_flags = lock(&bed.server).boxes["INBOX"].messages[&2].flags.clone();
    assert!(
        server_flags.flagged && !server_flags.seen,
        "server state kept"
    );
    let op = bed.op_of(deleted.batch).await;
    let op = bed.wait(op.id, OperationState::Conflict).await;
    assert_eq!(op.conflict, Some(ConflictReason::Gone));
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn refusals_fail_and_revert_and_can_be_retried() {
    let bed = Bed::new(1, true).await;
    let scratch_missing = {
        // The target exists locally but not on the server any more.
        lock(&bed.server).boxes.insert(
            "Gone".into(),
            FakeBox {
                uid_validity: 3,
                uid_next: 1,
                messages: BTreeMap::new(),
            },
        );
        bed.sync().await;
        lock(&bed.server).boxes.remove("Gone");
        bed.mailbox("Gone").await
    };
    let at = bed.inbox_ref(1).await;
    let result = bed
        .outbox
        .apply(
            MailAction::Move {
                target: scratch_missing,
            },
            vec![at],
        )
        .await
        .expect("apply");
    let op = bed.op_of(result.batch).await;
    let failed = bed.wait(op.id, OperationState::Failed).await;
    assert_eq!(
        failed.last_error.map(|e| e.code),
        Some(mail_model::ErrorCode::NotFound)
    );
    assert_eq!(bed.inbox_ref(1).await, at, "back in the inbox locally");
    // Retry once the folder is back: it goes through.
    lock(&bed.server).boxes.insert(
        "Gone".into(),
        FakeBox {
            uid_validity: 3,
            uid_next: 1,
            messages: BTreeMap::new(),
        },
    );
    let retried = bed.outbox.retry(op.id).await.expect("retry");
    assert_eq!((retried.batch, retried.queued), (result.batch, 1));
    let again = bed.op_of(retried.batch).await;
    assert!(again.id > op.id, "queued again at the end");
    bed.wait(again.id, OperationState::Confirmed).await;
    assert_eq!(lock(&bed.server).find("m1@fake"), [("Gone".to_owned(), 1)]);
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn read_only_accounts_record_nothing_and_never_connect() {
    let bed = Bed::new(1, true).await;
    bed.outbox
        .set_writes_enabled(bed.account, false)
        .await
        .expect("off");
    let at = bed.inbox_ref(1).await;
    let err = bed
        .outbox
        .apply(MailAction::MarkRead, vec![at])
        .await
        .expect_err("read-only");
    assert_eq!(err, CoreError::Action(ActionError::ReadOnly(bed.account)));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(lock(&bed.server).log.is_empty(), "no command at all");
    bed.outbox.shutdown().await;
}

#[tokio::test]
async fn undo_and_opposites_cancel_before_anything_is_sent() {
    let bed = Bed::new(1, true).await;
    bed.offline.store(true, Ordering::SeqCst);
    let at = bed.inbox_ref(1).await;
    let read = bed
        .outbox
        .apply(MailAction::MarkRead, vec![at])
        .await
        .expect("read");
    let unread = bed
        .outbox
        .apply(MailAction::MarkUnread, vec![at])
        .await
        .expect("unread");
    assert_eq!((unread.queued, unread.cancelled), (0, 1));
    let archived = bed
        .outbox
        .apply(MailAction::Archive, vec![at])
        .await
        .expect("archive");
    let undone = bed
        .outbox
        .undo(archived.batch.expect("batch"))
        .await
        .expect("undo");
    assert_eq!((undone.queued, undone.cancelled), (0, 1));
    assert!(read.batch.is_some());
    bed.offline.store(false, Ordering::SeqCst);
    bed.outbox.wake(bed.account);
    tokio::time::sleep(Duration::from_millis(100)).await;
    {
        let server = lock(&bed.server);
        assert_eq!(
            server.count("UID STORE") + server.count("UID MOVE"),
            0,
            "{:?}",
            server.log
        );
        assert_eq!(server.find("m1@fake"), [("INBOX".to_owned(), 1)]);
    }
    bed.outbox.shutdown().await;
}
