//! A connected account: the worker with the main connection, the IDLE watcher with its own
//! connection, and the poller, each in its own Tokio task (see the parent module).

use std::any::Any;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    AccountId, BackendError, ConnectConfig, CoreError, CoreEvent, IdleOutcome, MailBackend,
    MailConnector, MailboxId, SyncProgress,
};
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::{JoinError, JoinSet};
use tokio::time::{Instant, MissedTickBehavior};

use super::pass::{self, Scope};
use super::{Env, emit};

/// Longest wait for LOGOUT when a session ends on purpose.
const LOGOUT_LIMIT: Duration = Duration::from_secs(2);

/// What a session reports to the account's supervisor.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SessionEvent {
    /// The worker logged in. `idle`: the server can IDLE.
    Connected {
        idle: bool,
    },
    Progress(SyncProgress),
    PassCompleted,
    /// IDLE saw a change, or a poll interval passed.
    Changed,
}

/// Either an event or the end of the session.
#[derive(Debug)]
pub(crate) enum SessionOutput {
    Event(SessionEvent),
    /// A task failed or panicked; the session is finished.
    Ended(CoreError),
}

/// Sends what a pass does: progress to the supervisor, list changes to the UI (coalesced by the
/// caller: one `messagesChanged` + `mailboxesChanged` per stored window, not per message).
#[derive(Debug, Clone)]
pub(crate) struct Reporter {
    account: AccountId,
    events: broadcast::Sender<CoreEvent>,
    session: mpsc::UnboundedSender<SessionEvent>,
    /// Stage 9: new-mail windows for local rules and notifications.
    arrival: crate::comfort::ArrivalHook,
    /// Stage 11: the settings sync folder seen (or not) in the folder list.
    sync_signals: SignalsHook,
}

/// [`crate::settings_sync::service::SyncSignals`] for the `Debug` reporter.
#[derive(Clone)]
pub(crate) struct SignalsHook(pub Arc<crate::settings_sync::service::SyncSignals>);

impl std::fmt::Debug for SignalsHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignalsHook")
    }
}

impl Reporter {
    pub(crate) fn account(&self) -> AccountId {
        self.account
    }

    /// Messages of `mailbox` changed; its counters with them.
    pub(crate) fn messages_changed(&self, mailbox: MailboxId) {
        emit(
            &self.events,
            CoreEvent::MessagesChanged {
                account_id: self.account,
                mailbox_id: mailbox,
            },
        );
        self.mailboxes_changed();
    }

    pub(crate) fn mailboxes_changed(&self) {
        emit(
            &self.events,
            CoreEvent::MailboxesChanged {
                account_id: self.account,
            },
        );
    }

    pub(crate) fn new_mail(&self, mailbox: MailboxId, count: u32) {
        emit(
            &self.events,
            CoreEvent::NewMail {
                account_id: self.account,
                mailbox_id: mailbox,
                count,
            },
        );
    }

    /// Stage 9: a new-mail window of `mailbox` (outside the first sync) was stored.
    pub(crate) fn arrived(&self, mailbox: MailboxId, uids: mail_model::UidSet) {
        (self.arrival.0)(crate::comfort::Arrival {
            account: self.account,
            mailbox,
            uids,
        });
    }

    /// Stage 11: the folder list was read; `folder` is the settings sync folder in it.
    pub(crate) fn sync_folder_seen(&self, folder: Option<String>) {
        self.sync_signals.0.folder_seen(self.account, folder);
    }

    pub(crate) fn progress(&self, progress: SyncProgress) {
        self.send(SessionEvent::Progress(progress));
    }

    fn send(&self, event: SessionEvent) {
        // Fails only when the supervisor dropped the session, which aborts this task anyway.
        let _ = self.session.send(event);
    }
}

/// Mailboxes that need a pass, set by the IDLE watcher, the poller and the outbox (through the
/// supervisor), taken by the worker.
#[derive(Debug)]
struct Dirty {
    full: AtomicBool,
    inbox: AtomicBool,
    mailboxes: Mutex<BTreeSet<MailboxId>>,
}

impl Dirty {
    fn new() -> Self {
        // A new session starts with a full pass.
        Self {
            full: AtomicBool::new(true),
            inbox: AtomicBool::new(false),
            mailboxes: Mutex::new(BTreeSet::new()),
        }
    }

    fn mark_full(&self) {
        self.full.store(true, Ordering::SeqCst);
    }

    fn mark_inbox(&self) {
        self.inbox.store(true, Ordering::SeqCst);
    }

    fn mark_mailboxes(&self, ids: &[MailboxId]) {
        self.mailboxes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend(ids.iter().copied());
    }

    fn take(&self) -> Scope {
        let full = self.full.swap(false, Ordering::SeqCst);
        let inbox = self.inbox.swap(false, Ordering::SeqCst);
        let mailboxes = std::mem::take(
            &mut *self
                .mailboxes
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        if full || (!inbox && mailboxes.is_empty()) {
            // Nothing marked cannot happen (every `Changed` marks something); a full pass is
            // the safe answer anyway.
            if !full {
                tracing::debug!("pass requested without a marked scope");
            }
            Scope::Full
        } else {
            Scope::Partial { inbox, mailboxes }
        }
    }
}

/// The tasks of one connected account. Dropping it aborts them at once (connections are
/// closed without LOGOUT); [`Session::shutdown`] gives them time to log out.
pub(crate) struct Session {
    tasks: JoinSet<Result<(), CoreError>>,
    events: mpsc::UnboundedReceiver<SessionEvent>,
    reporter: Reporter,
    passes: Option<mpsc::UnboundedSender<()>>,
    shutdown: watch::Sender<bool>,
    dirty: Arc<Dirty>,
    /// For the IDLE watcher's connection.
    connect: ConnectConfig,
    watchers_started: bool,
}

impl Session {
    /// Starts the worker, which connects and reports [`SessionEvent::Connected`].
    pub(crate) fn start<C: MailConnector + 'static>(
        env: &Env<C>,
        account: AccountId,
        connect: ConnectConfig,
    ) -> Self {
        let (events_tx, events) = mpsc::unbounded_channel();
        let reporter = Reporter {
            account,
            events: env.events.clone(),
            session: events_tx,
            arrival: crate::comfort::ArrivalHook(Arc::clone(&env.on_arrival)),
            sync_signals: SignalsHook(Arc::clone(&env.sync_signals)),
        };
        let (passes_tx, passes_rx) = mpsc::unbounded_channel();
        let (shutdown, shutdown_rx) = watch::channel(false);
        let dirty = Arc::new(Dirty::new());
        let mut tasks = JoinSet::new();
        tasks.spawn(worker(
            env.clone(),
            connect.clone(),
            reporter.clone(),
            passes_rx,
            shutdown_rx,
            Arc::clone(&dirty),
        ));
        Self {
            tasks,
            events,
            reporter,
            passes: Some(passes_tx),
            shutdown,
            dirty,
            connect,
            watchers_started: false,
        }
    }

    /// Starts the IDLE watcher (servers with IDLE) and the poller, once per session.
    pub(crate) fn start_watchers<C: MailConnector + 'static>(&mut self, env: &Env<C>, idle: bool) {
        if std::mem::replace(&mut self.watchers_started, true) {
            return;
        }
        let config = &env.config;
        if idle {
            self.tasks.spawn(idle_watcher(
                env.clone(),
                self.connect.clone(),
                self.reporter.clone(),
                Arc::clone(&self.dirty),
                self.shutdown.subscribe(),
            ));
        }
        let inbox_poll = (!idle).then_some(config.inbox_poll_without_idle);
        self.tasks.spawn(poller(
            self.reporter.clone(),
            Arc::clone(&self.dirty),
            config.poll_interval,
            inbox_poll,
            self.shutdown.subscribe(),
        ));
    }

    /// Marks mailboxes for the next pass (the outbox changed them on the server). The pass
    /// itself is asked for by the supervisor, through the state machine.
    pub(crate) fn mark_mailboxes(&self, ids: &[MailboxId]) {
        self.dirty.mark_mailboxes(ids);
    }

    /// Asks the worker for a pass over what is marked dirty.
    pub(crate) fn request_pass(&self) {
        if let Some(passes) = &self.passes {
            // The worker only goes away together with the session.
            let _ = passes.send(());
        }
    }

    /// The next event, or the end of the session. Events sent before a task failed come first.
    pub(crate) async fn next(&mut self) -> SessionOutput {
        loop {
            tokio::select! {
                biased;
                Some(event) = self.events.recv() => return SessionOutput::Event(event),
                Some(joined) = self.tasks.join_next() => match joined {
                    Ok(Ok(())) => {
                        return SessionOutput::Ended(CoreError::Mail(BackendError::ConnectionLost(
                            "a sync connection ended unexpectedly".into(),
                        )));
                    }
                    Ok(Err(err)) => return SessionOutput::Ended(err),
                    Err(err) if err.is_panic() => return SessionOutput::Ended(panic_error(err)),
                    Err(_) => continue,
                },
                else => {
                    return SessionOutput::Ended(CoreError::Mail(BackendError::ConnectionLost(
                        "no sync task left".into(),
                    )));
                }
            }
        }
    }

    /// Ends the session on purpose: idle connections end IDLE and log out, a running pass is
    /// dropped; after `grace` whatever still runs is aborted.
    pub(crate) async fn shutdown(mut self, grace: Duration) {
        self.passes = None;
        let _ = self.shutdown.send(true);
        let drained = tokio::time::timeout(grace, async {
            while let Some(joined) = self.tasks.join_next().await {
                if let Err(err) = joined
                    && err.is_panic()
                {
                    tracing::warn!(error = %panic_error(err), "sync task panicked while stopping");
                }
            }
        })
        .await;
        if drained.is_err() {
            tracing::debug!("sync tasks did not stop in time; aborting them");
            self.tasks.shutdown().await;
        }
    }
}

/// A panic in a session task: a `protocol` error, so the account backs off and retries instead
/// of the app going down. imap-proto panics on some malformed responses (for example
/// `INTERNALDATE NIL`).
fn panic_error(err: JoinError) -> CoreError {
    let payload: Box<dyn Any + Send> = err.into_panic();
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned());
    let text: String = text.chars().take(200).collect();
    tracing::error!(panic = %text, "a sync task panicked; treating it as a protocol error");
    CoreError::Mail(BackendError::Protocol(format!(
        "the app could not process a server response ({text})"
    )))
}

async fn logout<B: MailBackend>(backend: B) {
    match tokio::time::timeout(LOGOUT_LIMIT, backend.logout()).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::debug!(error = %err, "LOGOUT failed"),
        Err(_) => tracing::debug!("LOGOUT timed out"),
    }
}

/// Main connection: connects, then runs a pass per request until the session ends.
async fn worker<C: MailConnector>(
    env: Env<C>,
    connect: ConnectConfig,
    reporter: Reporter,
    mut passes: mpsc::UnboundedReceiver<()>,
    mut shutdown: watch::Receiver<bool>,
    dirty: Arc<Dirty>,
) -> Result<(), CoreError> {
    let mut backend = env.connector.connect(&connect).await?;
    let idle = backend.capabilities().idle;
    tracing::info!(account = %reporter.account(), idle, "connected");
    reporter.send(SessionEvent::Connected { idle });
    loop {
        let request = tokio::select! {
            biased;
            _ = shutdown.wait_for(|stop| *stop) => None,
            request = passes.recv() => request,
        };
        if request.is_none() {
            logout(backend).await;
            return Ok(());
        }
        let scope = dirty.take();
        let started = Instant::now();
        let outcome = tokio::select! {
            biased;
            // Dropping the pass mid-command leaves the connection unusable: no LOGOUT.
            _ = shutdown.wait_for(|stop| *stop) => return Ok(()),
            outcome = pass::run(&mut backend, &env.store, &env.config, &reporter, scope.clone()) => outcome,
        }
        .map_err(CoreError::from)?;
        tracing::debug!(
            account = %reporter.account(),
            ?scope,
            ?outcome,
            ms = started.elapsed().as_millis() as u64,
            "pass completed"
        );
        reporter.send(SessionEvent::PassCompleted);
    }
}

/// Second connection: IDLE on the inbox in sections, reports every change.
async fn idle_watcher<C: MailConnector>(
    env: Env<C>,
    connect: ConnectConfig,
    reporter: Reporter,
    dirty: Arc<Dirty>,
    shutdown: watch::Receiver<bool>,
) -> Result<(), CoreError> {
    let mut backend = env.connector.connect(&connect).await?;
    let inbox = backend.open_mailbox("INBOX").await?;
    // Mail that arrived after the worker last looked at the inbox but before this EXAMINE
    // would not wake IDLE: look once more.
    dirty.mark_inbox();
    reporter.send(SessionEvent::Changed);
    loop {
        let mut stop_rx = shutdown.clone();
        let stop = async move {
            let _ = stop_rx.wait_for(|stop| *stop).await;
        };
        match backend.idle(&inbox, env.config.idle_section, stop).await? {
            IdleOutcome::Changed => {
                dirty.mark_inbox();
                reporter.send(SessionEvent::Changed);
            }
            IdleOutcome::TimedOut => {}
            IdleOutcome::Stopped => {
                logout(backend).await;
                return Ok(());
            }
        }
    }
}

/// Asks for a full pass every `full`, and for an inbox pass every `inbox` (servers without IDLE).
async fn poller(
    reporter: Reporter,
    dirty: Arc<Dirty>,
    full: Duration,
    inbox: Option<Duration>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), CoreError> {
    let ticker = |period: Duration| {
        let period = period.max(Duration::from_millis(10));
        let mut interval = tokio::time::interval_at(Instant::now() + period, period);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        interval
    };
    let mut full_tick = ticker(full);
    let mut inbox_tick = inbox.map(ticker);
    loop {
        tokio::select! {
            _ = shutdown.wait_for(|stop| *stop) => return Ok(()),
            _ = full_tick.tick() => dirty.mark_full(),
            _ = async {
                match inbox_tick.as_mut() {
                    Some(tick) => { tick.tick().await; }
                    None => std::future::pending::<()>().await,
                }
            } => dirty.mark_inbox(),
        }
        reporter.send(SessionEvent::Changed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_scopes() {
        let dirty = Dirty::new();
        assert_eq!(
            dirty.take(),
            Scope::Full,
            "a new session starts with a full pass"
        );
        dirty.mark_inbox();
        assert_eq!(dirty.take(), Scope::inbox());
        dirty.mark_inbox();
        dirty.mark_full();
        assert_eq!(dirty.take(), Scope::Full);
        assert_eq!(dirty.take(), Scope::Full);

        // The outbox's mailboxes add up until the next pass takes them; a full pass covers them.
        dirty.mark_mailboxes(&[MailboxId(4), MailboxId(2)]);
        dirty.mark_mailboxes(&[MailboxId(4)]);
        dirty.mark_inbox();
        assert_eq!(
            dirty.take(),
            Scope::Partial {
                inbox: true,
                mailboxes: BTreeSet::from([MailboxId(2), MailboxId(4)]),
            }
        );
        dirty.mark_mailboxes(&[MailboxId(7)]);
        assert_eq!(
            dirty.take(),
            Scope::Partial {
                inbox: false,
                mailboxes: BTreeSet::from([MailboxId(7)]),
            }
        );
        dirty.mark_mailboxes(&[MailboxId(7)]);
        dirty.mark_full();
        assert_eq!(dirty.take(), Scope::Full);
        assert_eq!(dirty.take(), Scope::Full, "nothing left over");
    }
}
