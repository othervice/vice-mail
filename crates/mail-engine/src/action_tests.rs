//! Stage 4 through `AppCore` against the in-memory fake server (`sync::fake`), with the real
//! store in a temporary directory: actions change the list at once, the outbox sends them on
//! its own connection, the sync confirms them right after (the outbox's `SyncHook`), undo,
//! failures, missing folders, write access, wake-ups, and the read-only sync.
//!
//! The poll interval is long and IDLE only watches the inbox, so a pass over another folder
//! within the wait can only come from the hook.

use std::sync::Arc;
use std::time::Duration;

use mail_model::{
    Access, AccountId, AccountSettings, AccountState, BackendError, CoreError, CoreEvent,
    ErrorCode, MailAction, Mailbox, MailboxId, MailboxRole, MarkReadOnOpen, MessageActivity,
    MessageListItem, MessageRef, OperationInfo, OperationState, OutboxCounts, ReadingPreferences,
    Secret, Security, ServerSettings,
};
use tempfile::TempDir;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::outbox::OutboxConfig;
use crate::sync::SyncConfig;
use crate::sync::fake::{FakeConnector, FakeServer, PASSWORD};
use crate::sync_state::BackoffPolicy;
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, ReadingServices};

const WAIT: Duration = Duration::from_secs(10);

fn sync_config() -> SyncConfig {
    SyncConfig {
        backoff: BackoffPolicy {
            base: Duration::from_millis(50),
            max: Duration::from_millis(400),
            jitter: 0.5,
        },
        idle_section: Duration::from_secs(2),
        // Long: only IDLE (inbox) and the outbox's hook start passes.
        poll_interval: Duration::from_secs(600),
        inbox_poll_without_idle: Duration::from_secs(600),
        wake_check: Duration::from_millis(50),
        wake_threshold: Duration::from_secs(5),
        shutdown_grace: Duration::from_millis(500),
        ..SyncConfig::default()
    }
}

/// The outbox with a backoff far longer than any wait here: after a failed connect it only
/// runs again when something wakes it.
fn sleepy_outbox() -> OutboxConfig {
    OutboxConfig {
        backoff: BackoffPolicy {
            base: Duration::from_secs(120),
            max: Duration::from_secs(120),
            jitter: 0.0,
        },
        linger: Duration::from_millis(200),
        ..OutboxConfig::default()
    }
}

fn settings() -> AccountSettings {
    let server = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "alice".into(),
    };
    AccountSettings {
        credential: Default::default(),
        protocol: Default::default(),
        email: "alice@fake.test".into(),
        display_name: "Alice".into(),
        imap: server("imap.fake.test", 993),
        smtp: server("smtp.fake.test", 465),
    }
}

struct Harness {
    _dir: TempDir,
    server: Arc<FakeServer>,
    core: AppCore<FakeConnector>,
    events: broadcast::Receiver<CoreEvent>,
    account: AccountId,
}

impl Harness {
    /// An account on a fresh fake server with `inbox` unread messages in the INBOX and
    /// `archived` in Archive, synced and idle. Writes are off, as for every new account.
    async fn new(inbox: usize, archived: usize, outbox: Option<OutboxConfig>) -> Self {
        let server = FakeServer::new();
        server.deliver("INBOX", inbox, false);
        server.deliver("Archive", archived, false);
        Self::with_server(server, outbox).await
    }

    async fn with_server(server: Arc<FakeServer>, outbox: Option<OutboxConfig>) -> Self {
        let dir = TempDir::new().expect("temp dir");
        let services = CoreServices {
            credentials: Credentials::new(MemorySecretStore::new()),
            connector: FakeConnector(Arc::clone(&server)),
            clock: Arc::new(crate::clock::SystemClock),
            sync: sync_config(),
            reading: ReadingServices::default(),
            outbox,
        };
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0.0.0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            services,
        )
        .await;
        let events = core.subscribe();
        let account = core
            .add_account(settings(), Secret::new(PASSWORD.into()))
            .await
            .expect("add account")
            .id;
        let mut h = Self {
            _dir: dir,
            server,
            core,
            events,
            account,
        };
        h.idle().await;
        h
    }

    async fn event(&mut self, pred: impl Fn(&CoreEvent) -> bool) -> (CoreEvent, Vec<CoreEvent>) {
        let mut before = Vec::new();
        let deadline = Instant::now() + WAIT;
        loop {
            let event = tokio::time::timeout_at(deadline, self.events.recv())
                .await
                .unwrap_or_else(|_| panic!("no matching event within {WAIT:?}; saw {before:#?}"))
                .expect("event stream");
            if pred(&event) {
                return (event, before);
            }
            before.push(event);
        }
    }

    async fn idle(&mut self) {
        let accounts = self.core.list_accounts().await.expect("accounts");
        if accounts
            .iter()
            .any(|a| a.id == self.account && a.status.state == AccountState::Idle)
        {
            return;
        }
        self.event(|e| {
            matches!(e, CoreEvent::AccountStatusChanged { status, .. } if status.state == AccountState::Idle)
        })
        .await;
    }

    /// Waits for the operations of `batch`'s first operation id to reach `state`, from the
    /// `operationsChanged` events; returns them.
    async fn operation_state(
        &mut self,
        op: mail_model::OperationId,
        state: OperationState,
    ) -> OperationInfo {
        let (event, _) = self
            .event(|e| match e {
                CoreEvent::OperationsChanged { operations, .. } => {
                    operations.iter().any(|o| o.id == op && o.state == state)
                }
                _ => false,
            })
            .await;
        let CoreEvent::OperationsChanged { operations, .. } = event else {
            unreachable!()
        };
        operations
            .into_iter()
            .find(|o| o.id == op)
            .expect("the operation")
    }

    async fn writes(&mut self, enabled: bool) {
        let account = self
            .core
            .set_writes_enabled(self.account, enabled)
            .await
            .expect("set writes");
        assert_eq!(account.writes_enabled, enabled);
        self.event(|e| matches!(e, CoreEvent::AccountsChanged))
            .await;
    }

    async fn mailbox(&self, path: &str) -> Mailbox {
        self.core
            .list_mailboxes(self.account)
            .await
            .expect("mailboxes")
            .into_iter()
            .find(|m| m.path == path)
            .unwrap_or_else(|| panic!("no mailbox {path}"))
    }

    async fn items(&self, mailbox: MailboxId) -> Vec<MessageListItem> {
        self.core
            .list_messages(mailbox, None, 500)
            .await
            .expect("list")
            .items
    }

    /// The list of `path`, oldest first (UID order of the fake), as references.
    async fn refs(&self, path: &str) -> Vec<MessageRef> {
        let mailbox = self.mailbox(path).await;
        let mut items = self.items(mailbox.id).await;
        items.reverse();
        items
            .into_iter()
            .map(|i| MessageRef {
                mailbox_id: mailbox.id,
                message_id: i.id,
            })
            .collect()
    }

    async fn item(&self, at: MessageRef) -> Option<MessageListItem> {
        self.items(at.mailbox_id)
            .await
            .into_iter()
            .find(|i| i.id == at.message_id)
    }

    /// Polls the list until `pred` holds (lists change through the sync in the background).
    async fn until(&self, what: &str, pred: impl AsyncFnPred) {
        let deadline = Instant::now() + WAIT;
        while !pred.check(self).await {
            assert!(Instant::now() < deadline, "{what} within {WAIT:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn act(
        &self,
        action: MailAction,
        at: &[MessageRef],
    ) -> Result<mail_model::ActionResult, CoreError> {
        self.core
            .apply_action(action, at.to_vec(), uuid::Uuid::new_v4().to_string())
            .await
    }

    async fn counts(&self) -> OutboxCounts {
        self.core.outbox_counts(self.account).await.expect("counts")
    }

    fn counts_of(&self, pending: u32, failed: u32, conflict: u32) -> OutboxCounts {
        OutboxCounts {
            account_id: self.account,
            pending,
            failed,
            conflict,
        }
    }

    fn examined(&self, path: &str) -> usize {
        self.server
            .read(|s| s.examined.iter().filter(|m| *m == path).count())
    }

    /// Every connection the sync, IDLE and reading opened was read-only.
    fn sync_stayed_read_only(&self) {
        let access = self.server.read(|s| s.access.clone());
        assert!(!access.is_empty());
        assert!(access.iter().all(|a| *a == Access::ReadOnly), "{access:?}");
    }
}

/// A list condition for [`Harness::until`].
trait AsyncFnPred {
    async fn check(&self, h: &Harness) -> bool;
}

/// The message is in `path`'s list (`true`) or not (`false`).
struct InList(&'static str, MessageListItemKey, bool);

/// Identifies a message across folders: its subject (the fake's `Message <serial>`).
type MessageListItemKey = String;

impl AsyncFnPred for InList {
    async fn check(&self, h: &Harness) -> bool {
        let mailbox = h.mailbox(self.0).await;
        h.items(mailbox.id)
            .await
            .iter()
            .any(|i| i.subject == self.1)
            == self.2
    }
}

fn code(result: Result<impl std::fmt::Debug, CoreError>) -> ErrorCode {
    result.expect_err("must be refused").code()
}

// ---------------------------------------------------------------------------------------------

/// An account without write access refuses every action through `AppCore` with `read_only`,
/// before anything changes or a writer connection exists. Turned on, the same action shows in
/// the list at once and reaches the server on the outbox's own connection; the sync, IDLE and
/// reading stay read-only (EXAMINE) throughout.
#[tokio::test]
async fn actions_need_write_access_and_the_sync_never_writes() {
    let mut h = Harness::new(3, 0, None).await;
    let at = h.refs("INBOX").await;
    assert_eq!(at.len(), 3);

    for action in [
        MailAction::MarkRead,
        MailAction::Flag,
        MailAction::Archive,
        MailAction::Delete,
        MailAction::DeletePermanently,
    ] {
        let err = h.act(action.clone(), &at).await.expect_err("read-only");
        assert_eq!(err.code(), ErrorCode::ReadOnly, "{action:?}: {err}");
        assert_eq!(mail_model::IpcError::from(&err).code, ErrorCode::ReadOnly);
    }
    let err = h
        .core
        .create_mailbox(h.account, "Trash".into(), Some(MailboxRole::Trash))
        .await
        .expect_err("read-only");
    assert_eq!(err.code(), ErrorCode::ReadOnly);
    assert!(
        h.item(at[0]).await.expect("listed").unread,
        "nothing changed"
    );
    assert_eq!(h.counts().await, h.counts_of(0, 0, 0));
    assert_eq!(
        h.server.read(|s| s.writer_attempts),
        0,
        "no writer connection"
    );
    let accounts = h.core.list_accounts().await.expect("accounts");
    assert!(!accounts[0].writes_enabled, "new accounts start read-only");

    h.writes(true).await;
    let accounts = h.core.list_accounts().await.expect("accounts");
    assert!(accounts[0].writes_enabled);

    let result = h
        .act(MailAction::MarkRead, &at[..2])
        .await
        .expect("mark read");
    assert_eq!(result.queued, 2);
    let batch = result.batch.expect("a batch");
    // Local at once (same transaction as the recorded operations).
    for m in &at[..2] {
        assert!(!h.item(*m).await.expect("listed").unread);
    }
    let (event, _) = h
        .event(|e| matches!(e, CoreEvent::OperationsChanged { .. }))
        .await;
    let CoreEvent::OperationsChanged {
        account_id,
        operations,
        removed,
    } = event
    else {
        unreachable!()
    };
    assert_eq!(account_id, h.account);
    assert!(removed.is_empty());
    assert_eq!(operations.len(), 2, "one event for the whole action");
    assert!(operations.iter().all(|o| o.batch == batch));
    assert!(operations.iter().all(|o| o.state == OperationState::Queued));
    let last = operations[1].id;
    h.operation_state(last, OperationState::Confirmed).await;

    let serial = h.server.serial_of("INBOX", 1);
    let places = h.server.find(serial);
    assert_eq!(places.len(), 1);
    assert!(places[0].2.seen, "the server has it");
    assert!(h.server.read(|s| s.writer_logins) >= 1);
    assert!(h.server.read(|s| s.writes.contains(&"STORE")));
    h.until(
        "rows without the pending symbol",
        RowsQuiet(at[..2].to_vec()),
    )
    .await;
    h.sync_stayed_read_only();
    assert_eq!(h.counts().await, h.counts_of(0, 0, 0));

    // Off again: refused again.
    h.writes(false).await;
    assert_eq!(
        code(h.act(MailAction::Flag, &at).await),
        ErrorCode::ReadOnly
    );
    h.sync_stayed_read_only();
    h.core.shutdown().await;
}

/// The rows show no outbox symbol any more.
struct RowsQuiet(Vec<MessageRef>);

impl AsyncFnPred for RowsQuiet {
    async fn check(&self, h: &Harness) -> bool {
        for at in &self.0 {
            if h.item(*at).await.and_then(|i| i.activity).is_some() {
                return false;
            }
        }
        true
    }
}

/// Archiving takes the message out of the inbox at once; the server moves it; the sync pass
/// the outbox asks for brings it into the archive's list (the fake has no UIDPLUS, so only
/// that pass can, and nothing else syncs the archive within the wait). Undo moves it back.
#[tokio::test]
async fn archive_shows_at_once_the_hook_confirms_it_and_undo_moves_it_back() {
    let mut h = Harness::new(2, 0, None).await;
    h.writes(true).await;
    let at = h.refs("INBOX").await;
    let subject = h.item(at[0]).await.expect("listed").subject;
    let serial = h.server.serial_of("INBOX", 1);
    let archive_passes = h.examined("Archive");

    let result = h.act(MailAction::Archive, &at[..1]).await.expect("archive");
    let batch = result.batch.expect("batch");
    assert!(h.item(at[0]).await.is_none(), "gone from the inbox at once");
    let (event, before) = h
        .event(|e| matches!(e, CoreEvent::OperationsChanged { .. }))
        .await;
    let CoreEvent::OperationsChanged { operations, .. } = event else {
        unreachable!()
    };
    let op = operations[0].id;
    assert_eq!(operations[0].action, mail_model::ActionKind::Archive);
    let inbox = h.mailbox("INBOX").await.id;
    let (_, _) = h
        .event(
            |e| matches!(e, CoreEvent::MessagesChanged { mailbox_id, .. } if *mailbox_id == inbox),
        )
        .await;
    drop(before);
    h.operation_state(op, OperationState::Confirmed).await;
    assert_eq!(
        h.server
            .find(serial)
            .iter()
            .map(|p| p.0.as_str())
            .collect::<Vec<_>>(),
        ["Archive"]
    );
    h.until(
        "the archive lists it",
        InList("Archive", subject.clone(), true),
    )
    .await;
    assert!(
        h.examined("Archive") > archive_passes,
        "the outbox's hook synced the archive"
    );
    assert_eq!(h.counts().await, h.counts_of(0, 0, 0));

    let undone = h.core.undo_action(batch).await.expect("undo");
    assert_eq!(undone.queued, 1, "a move back");
    h.until("back in the inbox", InList("INBOX", subject.clone(), true))
        .await;
    h.until("gone from the archive", InList("Archive", subject, false))
        .await;
    let places = h.server.find(serial);
    assert_eq!(
        places.iter().map(|p| p.0.as_str()).collect::<Vec<_>>(),
        ["INBOX"],
        "{places:?}"
    );
    h.sync_stayed_read_only();
    h.core.shutdown().await;
}

/// Without a trash folder, deleting answers `missing_mailbox`; creating one is an operation of
/// its own, after which deleting works. In the trash, deleting asks for the explicit permanent
/// delete (`confirmation_required`), which cannot be undone (`not_undoable`). A bad tag is
/// `invalid_keyword`.
#[tokio::test]
async fn missing_folders_confirmation_and_bad_tags_have_their_own_codes() {
    let mut h = Harness::new(2, 0, None).await;
    h.writes(true).await;
    let at = h.refs("INBOX").await;
    let subject = h.item(at[0]).await.expect("listed").subject;
    let serial = h.server.serial_of("INBOX", 1);

    assert_eq!(
        code(h.act(MailAction::Delete, &at[..1]).await),
        ErrorCode::MissingMailbox
    );
    assert_eq!(
        code(h.act(MailAction::Spam, &at[..1]).await),
        ErrorCode::MissingMailbox
    );
    assert_eq!(
        code(
            h.act(
                MailAction::AddKeyword {
                    keyword: "two words".into()
                },
                &at[..1]
            )
            .await
        ),
        ErrorCode::InvalidKeyword
    );
    assert!(h.item(at[0]).await.is_some(), "nothing changed");

    let created = h
        .core
        .create_mailbox(h.account, "Trash".into(), Some(MailboxRole::Trash))
        .await
        .expect("create");
    assert_eq!(created.queued, 1);
    h.event(|e| matches!(e, CoreEvent::MailboxesChanged { .. }))
        .await;
    h.until("the trash exists", HasRole(MailboxRole::Trash))
        .await;
    assert_eq!(
        code(h.core.undo_action(created.batch.expect("batch")).await),
        ErrorCode::NotUndoable
    );

    h.act(MailAction::Delete, &at[..1]).await.expect("delete");
    h.until("in the trash", InList("Trash", subject.clone(), true))
        .await;
    let in_trash = h.refs("Trash").await;
    assert_eq!(
        code(h.act(MailAction::Delete, &in_trash).await),
        ErrorCode::ConfirmationRequired
    );
    let gone = h
        .act(MailAction::DeletePermanently, &in_trash)
        .await
        .expect("delete for good");
    h.until("gone from the trash", InList("Trash", subject, false))
        .await;
    let deadline = Instant::now() + WAIT;
    while !h.server.find(serial).is_empty() {
        assert!(Instant::now() < deadline, "expunged on the server");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        code(h.core.undo_action(gone.batch.expect("batch")).await),
        ErrorCode::NotUndoable
    );
    h.core.shutdown().await;
}

struct HasRole(MailboxRole);

impl AsyncFnPred for HasRole {
    async fn check(&self, h: &Harness) -> bool {
        h.core
            .list_mailboxes(h.account)
            .await
            .expect("mailboxes")
            .iter()
            .any(|m| m.role == self.0)
    }
}

/// A refused STORE fails the operation, reverts the row and marks it in the list; the counts
/// and the activity view show it; retry sends it again, discard removes it.
#[tokio::test]
async fn failed_operations_show_in_the_list_and_can_be_retried_or_discarded() {
    let mut h = Harness::new(2, 0, None).await;
    h.writes(true).await;
    let at = h.refs("INBOX").await;
    h.server.with(|s| {
        s.faults.writes = Some(BackendError::Rejected("NO [CANNOT] not today".into()));
    });

    let result = h.act(MailAction::Flag, &at[..1]).await.expect("flag");
    let batch = result.batch.expect("batch");
    let ops = h
        .core
        .list_operations(h.account, None, 50)
        .await
        .expect("activity");
    let op = ops[0].id;
    assert_eq!(ops[0].batch, batch);
    let failed = h.operation_state(op, OperationState::Failed).await;
    assert_eq!(
        failed.error.as_ref().map(|e| e.code),
        Some(ErrorCode::ServerRejected)
    );
    let row = h.item(at[0]).await.expect("listed");
    assert!(!row.flagged, "reverted");
    assert_eq!(row.activity, Some(MessageActivity::Failed));
    assert_eq!(h.counts().await, h.counts_of(0, 1, 0));

    h.server.with(|s| s.faults.writes = None);
    let retried = h.core.retry_operation(op).await.expect("retry");
    assert_eq!(retried.batch, Some(batch));
    assert!(
        h.item(at[0]).await.expect("listed").flagged,
        "applied again"
    );
    let ops = h
        .core
        .list_operations(h.account, None, 50)
        .await
        .expect("activity");
    assert!(ops.iter().all(|o| o.id != op), "replaced by the retry");
    h.operation_state(ops[0].id, OperationState::Confirmed)
        .await;
    h.until("row quiet", RowsQuiet(at[..1].to_vec())).await;
    assert_eq!(h.counts().await, h.counts_of(0, 0, 0));

    h.server.with(|s| {
        s.faults.writes = Some(BackendError::Rejected("NO".into()));
    });
    h.act(MailAction::MarkRead, &at[1..]).await.expect("read");
    let op = h
        .core
        .list_operations(h.account, None, 1)
        .await
        .expect("activity")[0]
        .id;
    h.operation_state(op, OperationState::Failed).await;
    h.core.discard_operation(op).await.expect("discard");
    let (event, _) = h
        .event(
            |e| matches!(e, CoreEvent::OperationsChanged { removed, .. } if removed.contains(&op)),
        )
        .await;
    assert!(matches!(event, CoreEvent::OperationsChanged { .. }));
    assert!(h.item(at[1]).await.expect("listed").activity.is_none());
    assert_eq!(h.counts().await, h.counts_of(0, 0, 0));
    assert_eq!(
        code(h.core.discard_operation(op).await),
        ErrorCode::NotFound
    );
    h.core.shutdown().await;
}

/// Offline the action waits (row marked pending); the outbox, deep in its backoff, runs as soon
/// as the sync has reconnected, and again on `network_changed`.
#[tokio::test]
async fn queued_actions_run_when_the_sync_reconnects_or_the_network_changes() {
    let mut h = Harness::new(0, 3, Some(sleepy_outbox())).await;
    h.writes(true).await;
    let at = h.refs("Archive").await;
    assert_eq!(at.len(), 3);

    // 1. The writer cannot connect; the action is local and pending.
    h.server.with(|s| {
        s.faults.writer_connect = Some(BackendError::ConnectionRefused);
    });
    h.act(MailAction::MarkRead, &at[..1]).await.expect("read");
    let row = h.item(at[0]).await.expect("listed");
    assert!(!row.unread);
    assert_eq!(row.activity, Some(MessageActivity::Pending));
    assert_eq!(h.counts().await, h.counts_of(1, 0, 0));
    wait_for(
        || h.server.read(|s| s.writer_attempts) >= 1,
        "a connect attempt",
    )
    .await;
    h.server.with(|s| s.faults.writer_connect = None);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.counts().await,
        h.counts_of(1, 0, 0),
        "still in its backoff"
    );

    // The sync loses its connections (the inbox was rebuilt) and reconnects: the outbox
    // follows at once.
    h.server.recreate("INBOX");
    h.until(
        "sent after the sync reconnected",
        RowsQuiet(at[..1].to_vec()),
    )
    .await;
    assert_eq!(h.counts().await, h.counts_of(0, 0, 0));
    let serial = h.server.serial_of("Archive", 1);
    assert!(h.server.find(serial)[0].2.seen);

    // 2. Same, woken by a network change.
    h.idle().await;
    let attempts = h.server.read(|s| s.writer_attempts);
    h.server.with(|s| {
        s.faults.writer_connect = Some(BackendError::ConnectionRefused);
    });
    // The writer still lingers on its connection: let it log out first.
    tokio::time::sleep(Duration::from_millis(400)).await;
    h.act(MailAction::Flag, &at[1..2]).await.expect("flag");
    wait_for(
        || h.server.read(|s| s.writer_attempts) > attempts,
        "a connect attempt",
    )
    .await;
    h.server.with(|s| s.faults.writer_connect = None);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.counts().await,
        h.counts_of(1, 0, 0),
        "still in its backoff"
    );
    h.core.network_changed();
    h.until(
        "sent after the network change",
        RowsQuiet(at[1..2].to_vec()),
    )
    .await;
    let serial = h.server.serial_of("Archive", 2);
    assert!(h.server.find(serial)[0].2.flagged);
    h.sync_stayed_read_only();
    h.core.shutdown().await;
}

async fn wait_for(pred: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + WAIT;
    while !pred() {
        assert!(Instant::now() < deadline, "{what} within {WAIT:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// One action on many messages is one `operationsChanged` when recorded and one per command
/// when confirmed, not one event per message.
#[tokio::test]
async fn an_action_on_many_messages_is_a_few_events() {
    let mut h = Harness::new(300, 0, None).await;
    h.writes(true).await;
    let at = h.refs("INBOX").await;
    assert_eq!(at.len(), 300);
    let result = h.act(MailAction::MarkRead, &at).await.expect("read all");
    assert_eq!(result.queued, 300);
    let mut events = 0;
    let mut confirmed = std::collections::BTreeSet::new();
    let deadline = Instant::now() + WAIT;
    while confirmed.len() < 300 {
        let event = tokio::time::timeout_at(deadline, h.events.recv())
            .await
            .expect("events within the wait")
            .expect("event");
        if let CoreEvent::OperationsChanged { operations, .. } = event {
            events += 1;
            confirmed.extend(
                operations
                    .iter()
                    .filter(|o| o.state == OperationState::Confirmed)
                    .map(|o| o.id),
            );
        }
    }
    assert!(
        events <= 3,
        "{events} operationsChanged events for one action"
    );
    let too_many = vec![at[0]; mail_model::MAX_ACTION_MESSAGES + 1];
    assert_eq!(
        code(h.act(MailAction::Flag, &too_many).await),
        ErrorCode::InvalidArgument
    );
    let err = h
        .core
        .apply_action(MailAction::Flag, at[..1].to_vec(), "not a uuid!".into())
        .await
        .expect_err("bad request id");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    h.core.shutdown().await;
}

/// The same request id twice (an IPC call repeated) records the action once.
#[tokio::test]
async fn the_same_request_id_changes_nothing_the_second_time() {
    let mut h = Harness::new(1, 0, Some(sleepy_outbox())).await;
    h.writes(true).await;
    h.server.with(|s| {
        s.faults.writer_connect = Some(BackendError::ConnectionRefused);
    });
    let at = h.refs("INBOX").await;
    let id = uuid::Uuid::new_v4().to_string();
    let first = h
        .core
        .apply_action(MailAction::Flag, at.clone(), id.clone())
        .await
        .expect("first");
    let second = h
        .core
        .apply_action(MailAction::Flag, at, id)
        .await
        .expect("second");
    assert_eq!(first.batch, second.batch);
    assert_eq!((first.queued, second.queued), (1, 0));
    assert_eq!(h.counts().await, h.counts_of(1, 0, 0));
    h.core.shutdown().await;
}

/// Removing an account stops its outbox; nothing is left to count.
#[tokio::test]
async fn removing_an_account_stops_its_outbox() {
    let mut h = Harness::new(1, 0, Some(sleepy_outbox())).await;
    h.writes(true).await;
    h.server.with(|s| {
        s.faults.writer_connect = Some(BackendError::ConnectionRefused);
    });
    let at = h.refs("INBOX").await;
    h.act(MailAction::Flag, &at).await.expect("flag");
    h.core.remove_account(h.account).await.expect("remove");
    assert_eq!(
        code(h.core.outbox_counts(h.account).await),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(h.core.set_writes_enabled(h.account, true).await),
        ErrorCode::NotFound
    );
    h.core.shutdown().await;
}

/// "Mark as read when opened" defaults to after two seconds and survives a restart.
#[tokio::test]
async fn reading_preferences_default_to_the_delay_and_persist() {
    let dir = TempDir::new().expect("temp dir");
    let config = || CoreConfig {
        data_dir: dir.path().join("data"),
        build: BuildInfo {
            version: "0.0.0".into(),
            commit: None,
            build_date: None,
        },
    };
    let services = || {
        CoreServices::with_connector(
            Credentials::new(MemorySecretStore::new()),
            FakeConnector(FakeServer::new()),
        )
    };
    let core = AppCore::start(config(), services()).await;
    assert_eq!(
        core.get_reading_preferences().await.expect("read"),
        ReadingPreferences {
            mark_read_on_open: MarkReadOnOpen::AfterDelay
        }
    );
    let never = ReadingPreferences {
        mark_read_on_open: MarkReadOnOpen::Never,
    };
    assert_eq!(
        core.set_reading_preferences(never).await.expect("write"),
        never
    );
    // The appearance preferences are untouched.
    assert_eq!(
        core.get_preferences().await.expect("prefs"),
        mail_model::Preferences::default()
    );
    core.shutdown().await;
    drop(core);
    let core = AppCore::start(config(), services()).await;
    assert_eq!(core.get_reading_preferences().await.expect("read"), never);
    core.shutdown().await;
}
