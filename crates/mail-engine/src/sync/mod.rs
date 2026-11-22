//! Synchronisation of accounts with their servers (`docs/ARCHITECTURE.md`, "Regeln der
//! Mail-Engine", and decision 0002).
//!
//! One supervisor task per account ([`account`]) owns the account's [`SyncMachine`] and reacts
//! to what it says. A connected account has a session ([`session`]) of up to three tasks:
//!
//! - the **worker** holds the main connection and runs sync passes ([`pass`]) when told to;
//! - the **IDLE watcher** holds a second connection, idles on the inbox in sections of
//!   [`IDLE_SECTION`] and reports changes at once (servers with IDLE);
//! - the **poller** asks for a full pass every [`POLL_INTERVAL`] (other folders, and the inbox
//!   every [`INBOX_POLL_WITHOUT_IDLE`] on servers without IDLE).
//!
//! Every session task runs in its own Tokio task, so a panic (imap-proto can panic on exotic
//! server responses) ends only that session: the supervisor turns it into a `protocol` error
//! with backoff. A failing connection ends the session the same way; the machine decides when
//! to reconnect.
//!
//! **Sleep and network changes** ([`wake`]): without OS notifications (they would need `unsafe`
//! or platform crates), the engine notices them by
//! - a jump between the wall clock and the monotonic clock (resume from sleep; macOS stops the
//!   monotonic clock during sleep, so the difference is the time slept);
//! - short time limits: reads time out after 30 s without progress, IDLE is renewed every
//!   [`IDLE_SECTION`] (a dead IDLE connection is noticed within that plus the read limit), and
//!   the poller's pass every [`POLL_INTERVAL`] touches the main connection;
//! - and the public hooks `AppCore::resume` and `AppCore::network_changed`, which a shell with
//!   an OS signal can call.

pub(crate) mod account;
pub(crate) mod pass;
pub(crate) mod session;
pub(crate) mod wake;

#[cfg(test)]
mod fake;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use mail_model::{Access, ConnectConfig, CoreEvent, ExtraRootCa, Secret, ServerSettings, Timeouts};
use mail_store::Store;
use tokio::sync::broadcast;

use crate::clock::Clock;
use crate::credentials::Credentials;
use crate::sync_state::BackoffPolicy;

pub use crate::sync_state::SyncMachine;

/// Length of one IDLE section before it is renewed with DONE and IDLE (decision 0002: short
/// sections find a dead connection early; RFC 2177 allows up to 29 minutes).
pub const IDLE_SECTION: Duration = Duration::from_secs(5 * 60);

/// Interval of full passes (all folders) while connected.
pub const POLL_INTERVAL: Duration = Duration::from_secs(3 * 60);

/// Interval of inbox passes on servers without IDLE.
pub const INBOX_POLL_WITHOUT_IDLE: Duration = Duration::from_secs(60);

/// Messages per window and transaction in the first sync (decision 0002).
pub const WINDOW: u32 = 500;

/// The first window of an empty mailbox is smaller, so the list has content at once.
pub const FIRST_WINDOW: u32 = 100;

/// UIDs per `UID FETCH (FLAGS)` when flags are compared window by window (no CONDSTORE).
pub const FLAG_WINDOW: u32 = 2_000;

/// How often the sleep detector compares the clocks.
pub const WAKE_CHECK: Duration = Duration::from_secs(5);

/// A difference between wall clock and monotonic clock above this counts as a resume.
pub const WAKE_THRESHOLD: Duration = Duration::from_secs(20);

/// Time the session tasks get to log out before they are aborted (account removed, app quits).
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Tunables of the sync engine. The defaults are the product values; tests shorten them.
#[derive(Debug, Clone)]
pub struct SyncConfig {
    pub backoff: BackoffPolicy,
    pub idle_section: Duration,
    pub poll_interval: Duration,
    pub inbox_poll_without_idle: Duration,
    pub window: u32,
    pub first_window: u32,
    pub flag_window: u32,
    pub wake_check: Duration,
    pub wake_threshold: Duration,
    pub shutdown_grace: Duration,
    /// Limits of every server connection.
    pub timeouts: Timeouts,
    /// Additional trusted root (tests against the local test server and the desktop test
    /// build only). `None` in the product.
    pub extra_root_ca: Option<ExtraRootCa>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            backoff: BackoffPolicy::default(),
            idle_section: IDLE_SECTION,
            poll_interval: POLL_INTERVAL,
            inbox_poll_without_idle: INBOX_POLL_WITHOUT_IDLE,
            window: WINDOW,
            first_window: FIRST_WINDOW,
            flag_window: FLAG_WINDOW,
            wake_check: WAKE_CHECK,
            wake_threshold: WAKE_THRESHOLD,
            shutdown_grace: SHUTDOWN_GRACE,
            timeouts: Timeouts::default(),
            extra_root_ca: None,
        }
    }
}

impl SyncConfig {
    /// Connection settings for `server`: the configured limits and extra root, the access mode
    /// from the account's `writes_enabled` (always read-only in stage 2).
    pub(crate) fn connect_config(
        &self,
        server: &ServerSettings,
        password: Secret<String>,
        writes_enabled: bool,
    ) -> ConnectConfig {
        let access = if writes_enabled {
            Access::ReadWrite
        } else {
            Access::ReadOnly
        };
        let mut config = ConnectConfig::new(server, password, access);
        config.timeouts = self.timeouts;
        config.extra_root_ca = self.extra_root_ca.clone();
        config
    }
}

/// What the sync tasks of all accounts share. Cheap to clone.
pub(crate) struct Env<C> {
    pub store: Store,
    pub connector: Arc<C>,
    pub credentials: Credentials,
    pub clock: Arc<dyn Clock>,
    pub config: Arc<SyncConfig>,
    pub events: broadcast::Sender<CoreEvent>,
}

impl<C> Clone for Env<C> {
    fn clone(&self) -> Self {
        Self {
            store: self.store.clone(),
            connector: Arc::clone(&self.connector),
            credentials: self.credentials.clone(),
            clock: Arc::clone(&self.clock),
            config: Arc::clone(&self.config),
            events: self.events.clone(),
        }
    }
}

/// Sends an event to every subscriber. No subscriber is not an error: the state is stored.
pub(crate) fn emit(events: &broadcast::Sender<CoreEvent>, event: CoreEvent) {
    if events.send(event).is_err() {
        tracing::trace!("core event dropped: no subscribers");
    }
}
