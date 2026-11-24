//! The supervisor of one account: owns the [`SyncMachine`], starts and ends sessions, waits
//! out the backoff, and publishes the account's status.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mail_model::{
    AccountId, AccountStatus, CoreError, CoreEvent, MailConnector, MailboxId, Secret,
};
use mail_store::StoredAccount;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::session::{Session, SessionEvent, SessionOutput};
use super::{Env, emit};
use crate::credentials::Login;
use crate::sync_state::{SplitMix64, SyncAction, SyncEvent, SyncMachine};

/// What the app tells an account's supervisor.
pub(crate) enum Control {
    /// The user asked to retry (cuts a backoff short, leaves the error state).
    RetryNow,
    /// Resume from sleep or a network change: reconnect.
    Wake,
    /// A new password was stored; use it and reconnect.
    CredentialsChanged(Secret<String>),
    /// The outbox changed these mailboxes on the server: sync them now (its `SyncHook`).
    /// Ignored while not connected; the next session starts with a full pass anyway.
    SyncMailboxes(Vec<MailboxId>),
    /// Stage 18: the user asked to list the folders on the server again: a full pass now (when
    /// not connected, a reconnect, whose session starts with one).
    ReloadFolders,
    /// The account is removed or the app quits.
    Stop,
}

/// The app's handle to a running supervisor.
pub(crate) struct AccountHandle {
    control: mpsc::UnboundedSender<Control>,
    status: watch::Receiver<AccountStatus>,
    task: JoinHandle<()>,
}

impl AccountHandle {
    pub(crate) fn status(&self) -> AccountStatus {
        self.status.borrow().clone()
    }

    pub(crate) fn send(&self, control: Control) {
        // The supervisor only ends after `Stop` or an abort; then nobody needs the message.
        let _ = self.control.send(control);
    }

    /// Stops the supervisor and waits for it (its session gets `grace` to log out).
    pub(crate) async fn stop(self, grace: Duration) {
        self.send(Control::Stop);
        let mut task = self.task;
        if tokio::time::timeout(grace + Duration::from_secs(1), &mut task)
            .await
            .is_err()
        {
            tracing::warn!("account task did not stop in time; aborting it");
            task.abort();
            let _ = task.await;
        }
    }

    pub(crate) fn abort(&self) {
        self.task.abort();
    }
}

/// The running supervisors, by account.
#[derive(Default)]
pub(crate) struct Registry {
    accounts: Mutex<HashMap<AccountId, AccountHandle>>,
}

impl Registry {
    pub(crate) fn lock(&self) -> MutexGuard<'_, HashMap<AccountId, AccountHandle>> {
        // Map operations cannot leave the map inconsistent when a holder panics.
        self.accounts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn insert(&self, id: AccountId, handle: AccountHandle) {
        if let Some(old) = self.lock().insert(id, handle) {
            old.abort();
        }
    }

    pub(crate) fn remove(&self, id: AccountId) -> Option<AccountHandle> {
        self.lock().remove(&id)
    }

    pub(crate) fn status(&self, id: AccountId) -> Option<AccountStatus> {
        self.lock().get(&id).map(AccountHandle::status)
    }

    /// Sends `control` to the account; `false` if it has no supervisor.
    pub(crate) fn send(&self, id: AccountId, control: Control) -> bool {
        match self.lock().get(&id) {
            Some(handle) => {
                handle.send(control);
                true
            }
            None => false,
        }
    }

    /// Tells every account to reconnect.
    pub(crate) fn wake_all(&self) {
        for handle in self.lock().values() {
            handle.send(Control::Wake);
        }
    }

    pub(crate) fn take_all(&self) -> Vec<AccountHandle> {
        self.lock().drain().map(|(_, handle)| handle).collect()
    }

    pub(crate) fn abort_all(&self) {
        for handle in self.lock().values() {
            handle.abort();
        }
    }
}

/// Starts the supervisor of `account`. `password` is known right after setup; otherwise it is
/// read from the keychain on the first connect.
pub(crate) fn spawn<C: MailConnector + 'static>(
    env: Env<C>,
    account: StoredAccount,
    password: Option<Secret<String>>,
) -> AccountHandle {
    let (control, control_rx) = mpsc::unbounded_channel();
    let (status_tx, status) = watch::channel(AccountStatus::connecting());
    let task = tokio::spawn(
        Supervisor {
            env,
            account,
            password,
            status: status_tx,
        }
        .run(control_rx),
    );
    AccountHandle {
        control,
        status,
        task,
    }
}

struct Supervisor<C> {
    env: Env<C>,
    account: StoredAccount,
    password: Option<Secret<String>>,
    status: watch::Sender<AccountStatus>,
}

impl<C: MailConnector + 'static> Supervisor<C> {
    async fn run(mut self, mut control: mpsc::UnboundedReceiver<Control>) {
        let id = self.account.id;
        let mut machine = SyncMachine::new(self.env.config.backoff);
        let mut jitter = SplitMix64::from_entropy();
        let mut session: Option<Session> = None;
        let mut retry_at: Option<Instant> = None;
        // Stage 14: whether the access token was renewed after a rejection since the last
        // successful connection (only once).
        let mut token_renewed = false;
        let mut action = machine.handle(SyncEvent::Start, self.env.clock.now_ms(), &mut jitter);
        self.publish(&machine);

        loop {
            match action {
                SyncAction::Connect => {
                    retry_at = None;
                    drop(session.take());
                    match self.login().await {
                        Ok(login) => {
                            // Read-only also when writes are on: the outbox writes.
                            let mut config = self
                                .env
                                .config
                                .connect_config(&self.account.imap, login.secret);
                            crate::protocols::for_account(&mut config, &self.account);
                            config.auth = login.auth;
                            session = Some(Session::start(&self.env, id, config));
                        }
                        Err(err) => {
                            action = machine.handle(
                                SyncEvent::Failed(err),
                                self.env.clock.now_ms(),
                                &mut jitter,
                            );
                            self.publish(&machine);
                            continue;
                        }
                    }
                }
                SyncAction::Sync => {
                    if let Some(session) = &session {
                        session.request_pass();
                    }
                }
                SyncAction::Wait | SyncAction::None => {}
                SyncAction::RetryAfter(delay) => {
                    drop(session.take());
                    retry_at = Some(Instant::now() + delay);
                }
                SyncAction::Disconnect => {
                    drop(session.take());
                    retry_at = None;
                }
            }

            let event = tokio::select! {
                biased;
                received = control.recv() => match received {
                    Some(Control::RetryNow) => SyncEvent::RetryNow,
                    Some(Control::Wake) => SyncEvent::Wake,
                    Some(Control::CredentialsChanged(password)) => {
                        self.password = Some(password);
                        SyncEvent::CredentialsChanged
                    }
                    Some(Control::SyncMailboxes(mailboxes)) => {
                        if let Some(session) = &session {
                            session.mark_mailboxes(&mailboxes);
                        }
                        // Idle: a pass now; syncing: one more pass afterwards; not
                        // connected: ignored by the machine.
                        SyncEvent::Changed
                    }
                    Some(Control::ReloadFolders) => {
                        if let Some(session) = &session {
                            session.mark_full();
                            SyncEvent::Changed
                        } else {
                            SyncEvent::RetryNow
                        }
                    }
                    Some(Control::Stop) | None => {
                        if let Some(session) = session.take() {
                            session.shutdown(self.env.config.shutdown_grace).await;
                        }
                        machine.handle(SyncEvent::Stop, self.env.clock.now_ms(), &mut jitter);
                        tracing::info!(account = %id, "sync stopped");
                        return;
                    }
                },
                output = next_output(&mut session) => match output {
                    SessionOutput::Event(SessionEvent::Connected { idle }) => {
                        token_renewed = false;
                        if let Some(session) = session.as_mut() {
                            session.start_watchers(&self.env, idle);
                        }
                        // The server is reachable again: the outbox need not wait out its
                        // backoff.
                        (self.env.on_connected)(id);
                        SyncEvent::Connected
                    }
                    SessionOutput::Event(SessionEvent::Progress(progress)) => {
                        SyncEvent::Progress(progress)
                    }
                    SessionOutput::Event(SessionEvent::PassCompleted) => {
                        // Stage 11: the settings sync runs along with its account's mail sync.
                        self.env.sync_signals.pass_completed(id);
                        SyncEvent::PassCompleted
                    }
                    SessionOutput::Event(SessionEvent::Changed) => SyncEvent::Changed,
                    // Stage 14: a rejected access token is renewed once, at once.
                    SessionOutput::Ended(err)
                        if !token_renewed
                            && err.code() == mail_model::ErrorCode::AuthFailed
                            && self.env.credentials.login_rejected(&self.account.credential_ref) =>
                    {
                        drop(session.take());
                        token_renewed = true;
                        tracing::info!(account = %id, "the server rejected the access token; renewing it");
                        SyncEvent::CredentialsChanged
                    }
                    SessionOutput::Ended(err) => {
                        drop(session.take());
                        tracing::warn!(account = %id, code = ?err.code(), error = %err, "sync connection failed");
                        SyncEvent::Failed(err)
                    }
                },
                () = sleep_until(retry_at) => {
                    retry_at = None;
                    SyncEvent::RetryDue
                }
            };
            action = machine.handle(event, self.env.clock.now_ms(), &mut jitter);
            self.publish(&machine);
        }
    }

    /// The login: the password handed in or the keychain's (kept), or (stage 14) an access
    /// token, asked for at every connect so it is always valid.
    async fn login(&mut self) -> Result<Login, CoreError> {
        // Stage 14: a JMAP account's API token is kept like a password, sent as a bearer token.
        let kept = crate::credentials::secret_auth(&self.account.credential_ref);
        if let Some(password) = &self.password {
            return Ok(Login {
                secret: password.clone(),
                auth: kept,
            });
        }
        let login = self
            .env
            .credentials
            .login(&self.account.credential_ref, &self.account.imap, true)
            .await?;
        if login.auth == kept {
            self.password = Some(login.secret.clone());
        }
        Ok(login)
    }

    /// Publishes the machine's status if it changed (the UI gets the complete status).
    fn publish(&self, machine: &SyncMachine) {
        let status = machine.status();
        let changed = self.status.send_if_modified(|current| {
            if *current == status {
                false
            } else {
                *current = status.clone();
                true
            }
        });
        if changed {
            tracing::debug!(account = %self.account.id, state = ?status.state, "account status");
            emit(
                &self.env.events,
                CoreEvent::AccountStatusChanged {
                    account_id: self.account.id,
                    status,
                },
            );
        }
    }
}

async fn next_output(session: &mut Option<Session>) -> SessionOutput {
    match session {
        Some(session) => session.next().await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
