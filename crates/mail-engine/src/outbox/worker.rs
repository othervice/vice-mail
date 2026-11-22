//! The outbox worker of one account: its own writer connection (independent of the sync's
//! connections and IDLE), connects when there is work, executes the queue in order, waits out
//! a backoff when the server is unreachable, and logs out after a quiet `linger`.
//!
//! Every step runs in its own Tokio task with the connection moved in and back out, so a panic
//! (imap-proto can panic on exotic responses) costs the connection, never the app; the
//! operations it was sending stay in flight and are checked after reconnecting.

use std::sync::Arc;
use std::time::Duration;

use mail_model::{
    Access, AccountId, BackendError, ConnectConfig, CoreError, MailWriter, OutboxEvent,
    WriteConnector,
};
use mail_store::{StoreError, StoredAccount};
use tokio::sync::{Notify, watch};

use super::exec::{self, Connection, StepError};
use super::{Shared, emit_change};
use crate::credentials::CredentialRef;
use crate::sync_state::{Jitter, SplitMix64};

/// Longest wait for LOGOUT when the connection is closed on purpose.
const LOGOUT_LIMIT: Duration = Duration::from_secs(2);

/// What woke the worker.
enum Wake {
    Kick,
    Timer,
    Stop,
}

pub(crate) async fn run<C: WriteConnector + 'static>(
    shared: Arc<Shared<C>>,
    account: AccountId,
    kick: Arc<Notify>,
    mut stop: watch::Receiver<bool>,
) {
    let mut conn: Option<Connection<C::Writer>> = None;
    let mut failures: u32 = 0;
    let mut jitter = SplitMix64::from_entropy();
    let mut purged = false;
    let backoff = |failures: u32, jitter: &mut SplitMix64| {
        shared.config.backoff.delay(failures, jitter.unit())
    };

    loop {
        if *stop.borrow() {
            break;
        }
        let pending = match shared.store.pending_operations(account).await {
            Ok(pending) => pending,
            Err(StoreError::NotFound(_)) => {
                tracing::debug!(%account, "account removed; outbox worker ends");
                break;
            }
            Err(err) => {
                tracing::error!(%account, error = %err, "outbox could not read its queue");
                failures = failures.saturating_add(1);
                match wait(&kick, &mut stop, Some(backoff(failures, &mut jitter))).await {
                    Wake::Stop => break,
                    Wake::Kick | Wake::Timer => continue,
                }
            }
        };

        if pending.is_empty() {
            if !purged {
                purged = true;
                purge(&shared, account).await;
            }
            // Keep the connection for a moment (the next action is often seconds away).
            let linger = conn.is_some().then_some(shared.config.linger);
            match wait(&kick, &mut stop, linger).await {
                Wake::Stop => break,
                Wake::Kick => {}
                Wake::Timer => {
                    if let Some(open) = conn.take() {
                        logout(open.writer).await;
                    }
                }
            }
            continue;
        }
        purged = false;

        let stored = match shared.store.account(account).await {
            Ok(Some(stored)) => stored,
            Ok(None) => break,
            Err(err) => {
                tracing::error!(%account, error = %err, "outbox could not read the account");
                match wait(
                    &kick,
                    &mut stop,
                    Some(backoff(failures.max(1), &mut jitter)),
                )
                .await
                {
                    Wake::Stop => break,
                    Wake::Kick | Wake::Timer => continue,
                }
            }
        };
        if !stored.writes_enabled {
            // Queued operations fail (`read_only`) and are reverted; in-flight ones wait until
            // writes are on again and are then checked against the server.
            match shared.store.set_writes_enabled(account, false).await {
                Ok(change) => emit_change(&shared.events, &change),
                Err(err) => tracing::error!(%account, error = %err, "failing queued operations"),
            }
            if let Some(open) = conn.take() {
                logout(open.writer).await;
            }
            match wait(&kick, &mut stop, None).await {
                Wake::Stop => break,
                Wake::Kick | Wake::Timer => continue,
            }
        }

        let open = match conn.take() {
            Some(open) => open,
            None => match connect(&shared, &stored).await {
                Ok(writer) => {
                    tracing::info!(%account, "outbox connected");
                    Connection::new(writer)
                }
                Err(err) => {
                    failures = failures.saturating_add(1);
                    let delay = backoff(failures, &mut jitter);
                    tracing::info!(%account, code = ?err.code(), error = %err, ?delay, "outbox cannot connect; waiting");
                    match wait(&kick, &mut stop, Some(delay)).await {
                        Wake::Stop => break,
                        Wake::Kick => failures = 0,
                        Wake::Timer => {}
                    }
                    continue;
                }
            },
        };

        let group = exec::next_group(&pending);
        let store = shared.store.clone();
        let max_attempts = shared.config.max_attempts;
        let task = tokio::spawn(async move {
            let mut open = open;
            let result = exec::run_step(&mut open, &store, account, max_attempts, group).await;
            (open, result)
        });
        let error = match task.await {
            Ok((open, Ok(outcome))) => {
                failures = 0;
                conn = Some(open);
                if let Some(change) = &outcome.change {
                    emit_change(&shared.events, change);
                }
                if outcome.list_changed {
                    let _ = shared.events.send(OutboxEvent::MailboxesChanged {
                        account_id: account,
                    });
                }
                if !outcome.server_mailboxes.is_empty() {
                    shared
                        .hook()
                        .sync_requested(account, &outcome.server_mailboxes);
                }
                continue;
            }
            Ok((open, Err(StepError::Store(err)))) => {
                conn = Some(open);
                tracing::error!(%account, error = %err, "outbox could not record an outcome");
                CoreError::from(err)
            }
            Ok((_broken, Err(StepError::Connection(err)))) => {
                tracing::info!(%account, code = ?err.code(), error = %err, "outbox connection lost");
                CoreError::Mail(err)
            }
            Err(join) => {
                tracing::error!(%account, panicked = join.is_panic(), "outbox step ended abnormally; reconnecting");
                CoreError::Mail(BackendError::Protocol(
                    "the outbox could not process a server response".into(),
                ))
            }
        };
        failures = failures.saturating_add(1);
        let delay = backoff(failures, &mut jitter);
        tracing::debug!(%account, code = ?error.code(), ?delay, "outbox retries later");
        match wait(&kick, &mut stop, Some(delay)).await {
            Wake::Stop => break,
            Wake::Kick => failures = 0,
            Wake::Timer => {}
        }
    }
    if let Some(open) = conn.take() {
        logout(open.writer).await;
    }
}

async fn wait(kick: &Notify, stop: &mut watch::Receiver<bool>, timer: Option<Duration>) -> Wake {
    let sleep = async {
        match timer {
            Some(d) => tokio::time::sleep(d).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        biased;
        _ = stop.wait_for(|stop| *stop) => Wake::Stop,
        () = kick.notified() => Wake::Kick,
        () = sleep => Wake::Timer,
    }
}

async fn connect<C: WriteConnector>(
    shared: &Shared<C>,
    account: &StoredAccount,
) -> Result<C::Writer, CoreError> {
    let key = CredentialRef::parse(&account.credential_ref)?;
    let password = shared.credentials.get(&key).await?.ok_or_else(|| {
        CoreError::Keychain("no password stored for this account; enter it again".into())
    })?;
    let mut config = ConnectConfig::new(&account.imap, password, Access::ReadWrite);
    config.timeouts = shared.config.timeouts;
    config.extra_root_ca = shared.config.extra_root_ca.clone();
    Ok(shared.connector.connect_writer(&config).await?)
}

async fn logout<W: MailWriter>(writer: W) {
    match tokio::time::timeout(LOGOUT_LIMIT, writer.logout()).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::debug!(error = %err, "outbox LOGOUT failed"),
        Err(_) => tracing::debug!("outbox LOGOUT timed out"),
    }
}

async fn purge<C>(shared: &Shared<C>, account: AccountId) {
    let keep = i64::try_from(shared.config.keep_confirmed.as_millis()).unwrap_or(i64::MAX);
    let before = shared.clock.now_ms().saturating_sub(keep);
    match shared.store.purge_operations(account, before).await {
        Ok(0) => {}
        Ok(n) => tracing::debug!(%account, purged = n, "confirmed operations purged"),
        Err(err) => tracing::warn!(%account, error = %err, "purging confirmed operations failed"),
    }
}
