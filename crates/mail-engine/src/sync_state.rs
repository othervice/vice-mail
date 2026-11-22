//! State machine of an account's sync task. Pure logic, no IO and no clock: the task feeds in
//! events with the current time and a jitter source, and does what the returned
//! [`SyncAction`] says. That keeps reconnect and backoff behaviour deterministic in tests.
//!
//! ```text
//!   Stopped ──Start──▶ Connecting ──Connected──▶ Syncing ──PassCompleted──▶ Idle
//!                          ▲  │                    ▲  │                      │
//!                RetryDue  │  │ transient error    │  └──── Changed ─────────┘
//!                Wake      │  ▼                    │
//!                RetryNow  Backoff ◀── transient error (from Connecting, Syncing, Idle)
//!                          Failed  ◀── permanent error (password, certificate, ...)
//!                             └── RetryNow (any error) / Wake (all but auth_failed) ──▶ Connecting
//! ```
//!
//! Consecutive failures count up until a sync pass completes; the backoff grows with them.
//! `Wake` (resume from sleep, network change) reconnects at once and resets the count.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use mail_model::{AccountState, AccountStatus, CoreError, ErrorCode, IpcError, SyncProgress};

/// Exponential backoff with jitter and a cap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackoffPolicy {
    /// Delay before the first retry.
    pub base: Duration,
    /// Upper bound of any delay.
    pub max: Duration,
    /// Share of the delay that is randomised, `0.0..=1.0`. With 0.5 a delay `d` becomes a
    /// value in `(d/2, d]`, so retries of many clients spread out but never come early.
    pub jitter: f64,
}

impl Default for BackoffPolicy {
    /// 1 s, doubling, at most 2 min, half of it random.
    fn default() -> Self {
        Self {
            base: Duration::from_secs(1),
            max: Duration::from_secs(120),
            jitter: 0.5,
        }
    }
}

impl BackoffPolicy {
    /// Delay before retry number `attempt` (1 = first retry): `min(max, base * 2^(attempt-1))`,
    /// reduced by up to `jitter` of itself. `unit` is a random number in `[0, 1)`.
    pub fn delay(&self, attempt: u32, unit: f64) -> Duration {
        let doublings = attempt.saturating_sub(1).min(31);
        let full = self.base.saturating_mul(1u32 << doublings).min(self.max);
        let unit = if unit.is_finite() {
            unit.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let jitter = if self.jitter.is_finite() {
            self.jitter.clamp(0.0, 1.0)
        } else {
            0.0
        };
        full.mul_f64(1.0 - jitter * unit)
    }
}

/// Source of random numbers in `[0, 1)` for the jitter. Tests pass a fixed sequence.
pub trait Jitter {
    fn unit(&mut self) -> f64;
}

impl<F: FnMut() -> f64> Jitter for F {
    fn unit(&mut self) -> f64 {
        self()
    }
}

/// Small, fast generator (SplitMix64). Good enough to spread retries; not for cryptography.
#[derive(Debug, Clone)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub const fn seeded(seed: u64) -> Self {
        Self(seed)
    }

    /// Seeded from the per-process random keys of the standard library's hash maps.
    pub fn from_entropy() -> Self {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(0x5eed);
        Self(hasher.finish())
    }
}

impl Jitter for SplitMix64 {
    fn unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        // 53 random bits as a fraction in [0, 1).
        (z >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Where the sync task of one account is.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState {
    /// Not running (before `Start`, after `Stop`).
    Stopped,
    /// Opening a connection. `attempt` is the number of failures before this try.
    Connecting { attempt: u32 },
    /// A sync pass runs. `progress` is set while a mailbox has its first sync.
    Syncing { progress: Option<SyncProgress> },
    /// Up to date, waiting for changes (IDLE or poll interval).
    Idle,
    /// Waiting before the next connection attempt.
    Backoff {
        attempt: u32,
        retry_in: Duration,
        /// Unix milliseconds, for the UI.
        retry_at_ms: i64,
    },
    /// Stopped by an error only the user can fix.
    Failed,
}

/// What happened, as reported by the sync task (or the app, for `Wake` and `RetryNow`).
#[derive(Debug, Clone, PartialEq)]
pub enum SyncEvent {
    /// The account was added or the app started.
    Start,
    /// Login succeeded.
    Connected,
    /// The first sync of a mailbox made progress.
    Progress(SyncProgress),
    /// A sync pass over all mailboxes finished.
    PassCompleted,
    /// The server reported changes (IDLE) or the poll interval elapsed.
    Changed,
    /// Connecting or syncing failed.
    Failed(CoreError),
    /// The backoff delay elapsed.
    RetryDue,
    /// Resume from sleep or a network change: existing connections are suspect.
    Wake,
    /// The user asked to retry (button, or a new password was stored).
    RetryNow,
    /// The account is being removed or the app quits.
    Stop,
}

/// What the sync task does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    /// Nothing changes; keep doing what you do.
    None,
    /// Drop any connection and connect (again).
    Connect,
    /// Run a sync pass on the current connection.
    Sync,
    /// Wait for changes (IDLE or poll).
    Wait,
    /// Drop connections, wait this long, then report `RetryDue` (or earlier `Wake`/`RetryNow`).
    RetryAfter(Duration),
    /// Drop connections and stay disconnected.
    Disconnect,
}

/// The state machine of one account.
#[derive(Debug, Clone)]
pub struct SyncMachine {
    state: SyncState,
    policy: BackoffPolicy,
    failures: u32,
    last_error: Option<CoreError>,
    last_sync_at_ms: Option<i64>,
    /// A change was reported while a pass was running: run another pass afterwards.
    pending_change: bool,
}

impl SyncMachine {
    pub fn new(policy: BackoffPolicy) -> Self {
        Self {
            state: SyncState::Stopped,
            policy,
            failures: 0,
            last_error: None,
            last_sync_at_ms: None,
            pending_change: false,
        }
    }

    pub fn state(&self) -> &SyncState {
        &self.state
    }

    /// Consecutive failures since the last completed pass.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    pub fn last_error(&self) -> Option<&CoreError> {
        self.last_error.as_ref()
    }

    /// Applies `event` at `now_ms` (Unix milliseconds) and returns what to do next. Events
    /// that make no sense in the current state are ignored ([`SyncAction::None`]).
    pub fn handle(
        &mut self,
        event: SyncEvent,
        now_ms: i64,
        jitter: &mut impl Jitter,
    ) -> SyncAction {
        use SyncEvent as E;
        use SyncState as S;

        match (&self.state, event) {
            (_, E::Stop) => {
                self.state = S::Stopped;
                self.pending_change = false;
                SyncAction::Disconnect
            }
            (S::Stopped, E::Start) => self.connect(0),
            (S::Stopped, _) => SyncAction::None,

            (S::Connecting { .. }, E::Connected) => {
                self.state = S::Syncing { progress: None };
                SyncAction::Sync
            }
            (S::Syncing { .. }, E::Progress(progress)) => {
                self.state = S::Syncing {
                    progress: Some(progress),
                };
                SyncAction::None
            }
            (S::Syncing { .. }, E::PassCompleted) => {
                self.failures = 0;
                self.last_error = None;
                self.last_sync_at_ms = Some(now_ms);
                if std::mem::take(&mut self.pending_change) {
                    self.state = S::Syncing { progress: None };
                    SyncAction::Sync
                } else {
                    self.state = S::Idle;
                    SyncAction::Wait
                }
            }
            (S::Syncing { .. }, E::Changed) => {
                self.pending_change = true;
                SyncAction::None
            }
            (S::Idle, E::Changed) => {
                self.state = S::Syncing { progress: None };
                SyncAction::Sync
            }

            (S::Connecting { .. } | S::Syncing { .. } | S::Idle, E::Failed(error)) => {
                self.pending_change = false;
                if error.is_transient() {
                    self.failures = self.failures.saturating_add(1);
                    let retry_in = self.policy.delay(self.failures, jitter.unit());
                    let retry_at_ms = now_ms
                        .saturating_add(i64::try_from(retry_in.as_millis()).unwrap_or(i64::MAX));
                    self.last_error = Some(error);
                    self.state = S::Backoff {
                        attempt: self.failures,
                        retry_in,
                        retry_at_ms,
                    };
                    SyncAction::RetryAfter(retry_in)
                } else {
                    self.last_error = Some(error);
                    self.state = S::Failed;
                    SyncAction::Disconnect
                }
            }

            (S::Backoff { .. }, E::RetryDue) => self.connect(self.failures),
            (S::Backoff { .. } | S::Idle | S::Syncing { .. }, E::Wake) => {
                self.failures = 0;
                self.connect(0)
            }
            (S::Backoff { .. }, E::RetryNow) => self.connect(self.failures),
            (S::Failed, E::RetryNow) => {
                self.failures = 0;
                self.connect(0)
            }
            (S::Failed, E::Wake) => {
                // A network change can heal a certificate error (captive portal) but not a
                // wrong password; repeating a rejected login may lock the account.
                let auth = self
                    .last_error
                    .as_ref()
                    .is_some_and(|e| e.code() == ErrorCode::AuthFailed);
                if auth {
                    SyncAction::None
                } else {
                    self.failures = 0;
                    self.connect(0)
                }
            }

            _ => SyncAction::None,
        }
    }

    fn connect(&mut self, attempt: u32) -> SyncAction {
        self.pending_change = false;
        self.state = SyncState::Connecting { attempt };
        SyncAction::Connect
    }

    /// The account's status for the UI.
    pub fn status(&self) -> AccountStatus {
        let error = || self.last_error.as_ref().map(IpcError::from);
        let (state, progress, error, retry_at) = match &self.state {
            SyncState::Stopped => (AccountState::Offline, None, None, None),
            SyncState::Connecting { attempt: 0 } if self.last_error.is_none() => {
                (AccountState::Connecting, None, None, None)
            }
            // A reconnect after failures still reads as offline, so the indicator does not
            // flicker between "offline" and "connecting" on every attempt.
            SyncState::Connecting { .. } => (AccountState::Offline, None, error(), None),
            SyncState::Syncing { progress } => (AccountState::Syncing, *progress, None, None),
            SyncState::Idle => (AccountState::Idle, None, None, None),
            SyncState::Backoff { retry_at_ms, .. } => {
                (AccountState::Offline, None, error(), Some(*retry_at_ms))
            }
            SyncState::Failed => (AccountState::Error, None, error(), None),
        };
        AccountStatus {
            state,
            progress,
            error,
            retry_at,
            last_sync_at: self.last_sync_at_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use mail_model::{BackendError, MailboxId};

    use super::*;

    const NOW: i64 = 1_764_000_000_000;

    fn policy() -> BackoffPolicy {
        BackoffPolicy {
            base: Duration::from_millis(100),
            max: Duration::from_secs(5),
            jitter: 0.5,
        }
    }

    fn lost() -> CoreError {
        BackendError::ConnectionLost("eof".into()).into()
    }

    fn auth() -> CoreError {
        BackendError::AuthFailed("[AUTHENTICATIONFAILED]".into()).into()
    }

    /// Jitter that always returns the same value.
    fn fixed(value: f64) -> impl FnMut() -> f64 {
        move || value
    }

    fn running() -> SyncMachine {
        let mut m = SyncMachine::new(policy());
        let j = &mut fixed(0.0);
        assert_eq!(m.handle(SyncEvent::Start, NOW, j), SyncAction::Connect);
        assert_eq!(m.handle(SyncEvent::Connected, NOW, j), SyncAction::Sync);
        assert_eq!(m.handle(SyncEvent::PassCompleted, NOW, j), SyncAction::Wait);
        m
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let p = policy();
        let delays: Vec<_> = (1..=8).map(|n| p.delay(n, 0.0)).collect();
        let ms = |v: u64| Duration::from_millis(v);
        assert_eq!(
            delays,
            [
                ms(100),
                ms(200),
                ms(400),
                ms(800),
                ms(1600),
                ms(3200),
                ms(5000),
                ms(5000)
            ]
        );
        assert_eq!(p.delay(0, 0.0), ms(100));
        assert_eq!(p.delay(u32::MAX, 0.0), ms(5000));
        let default = BackoffPolicy::default();
        assert_eq!(default.delay(1, 0.0), Duration::from_secs(1));
        assert_eq!(default.delay(100, 0.0), Duration::from_secs(120));
    }

    #[test]
    fn jitter_shortens_by_at_most_its_share() {
        let p = policy();
        assert_eq!(p.delay(3, 0.5), Duration::from_millis(300));
        assert!(p.delay(3, 0.999_999) > Duration::from_millis(200));
        // Out-of-range or broken random values are clamped, never panic.
        assert_eq!(p.delay(3, -1.0), Duration::from_millis(400));
        assert_eq!(p.delay(3, f64::NAN), Duration::from_millis(400));
        assert_eq!(p.delay(3, 7.0), Duration::from_millis(200));
        let none = BackoffPolicy { jitter: 0.0, ..p };
        assert_eq!(none.delay(3, 0.9), Duration::from_millis(400));

        let mut rng = SplitMix64::seeded(42);
        for attempt in 1..200 {
            let unit = rng.unit();
            assert!((0.0..1.0).contains(&unit));
            let d = p.delay(attempt % 10 + 1, unit);
            let full = p.delay(attempt % 10 + 1, 0.0);
            assert!(d > full / 2 && d <= full, "{d:?} {full:?}");
        }
        // Same seed, same sequence.
        let (mut a, mut b) = (SplitMix64::seeded(7), SplitMix64::seeded(7));
        assert!((0..10).all(|_| a.unit() == b.unit()));
        let _ = SplitMix64::from_entropy().unit();
    }

    #[test]
    fn first_sync_reports_progress_then_idles() {
        let mut m = SyncMachine::new(policy());
        let j = &mut fixed(0.0);
        assert_eq!(m.status().state, AccountState::Offline);
        assert_eq!(m.handle(SyncEvent::Start, NOW, j), SyncAction::Connect);
        assert_eq!(m.status().state, AccountState::Connecting);
        m.handle(SyncEvent::Connected, NOW, j);
        let progress = SyncProgress {
            mailbox_id: MailboxId(1),
            done: 500,
            total: 10_000,
        };
        assert_eq!(
            m.handle(SyncEvent::Progress(progress), NOW, j),
            SyncAction::None
        );
        let status = m.status();
        assert_eq!(status.state, AccountState::Syncing);
        assert_eq!(status.progress, Some(progress));
        assert_eq!(
            m.handle(SyncEvent::PassCompleted, NOW + 5, j),
            SyncAction::Wait
        );
        let status = m.status();
        assert_eq!(status.state, AccountState::Idle);
        assert_eq!(status.last_sync_at, Some(NOW + 5));
        assert!(status.progress.is_none() && status.error.is_none());
    }

    #[test]
    fn changes_trigger_a_pass_and_are_not_lost_during_one() {
        let mut m = running();
        let j = &mut fixed(0.0);
        assert_eq!(m.handle(SyncEvent::Changed, NOW, j), SyncAction::Sync);
        // Another change arrives while the pass runs: one more pass afterwards.
        assert_eq!(m.handle(SyncEvent::Changed, NOW, j), SyncAction::None);
        assert_eq!(m.handle(SyncEvent::PassCompleted, NOW, j), SyncAction::Sync);
        assert_eq!(m.handle(SyncEvent::PassCompleted, NOW, j), SyncAction::Wait);
        assert_eq!(m.state(), &SyncState::Idle);
    }

    #[test]
    fn transient_errors_back_off_exponentially_until_a_pass_completes() {
        let mut m = running();
        let j = &mut fixed(0.0);
        // The idle connection drops.
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, j),
            SyncAction::RetryAfter(Duration::from_millis(100))
        );
        let status = m.status();
        assert_eq!(status.state, AccountState::Offline);
        assert_eq!(status.retry_at, Some(NOW + 100));
        assert_eq!(
            status.error.map(|e| e.code),
            Some(ErrorCode::ConnectionLost)
        );
        assert_eq!(
            status.last_sync_at,
            Some(NOW),
            "last sync survives the failure"
        );

        // Reconnects keep failing: 200, 400, 800 ms. While connecting, still "offline".
        for expected in [200, 400, 800] {
            assert_eq!(m.handle(SyncEvent::RetryDue, NOW, j), SyncAction::Connect);
            assert_eq!(m.status().state, AccountState::Offline);
            assert!(m.status().retry_at.is_none());
            let refused = BackendError::ConnectionRefused.into();
            assert_eq!(
                m.handle(SyncEvent::Failed(refused), NOW, j),
                SyncAction::RetryAfter(Duration::from_millis(expected))
            );
        }
        assert_eq!(m.failures(), 4);

        // Login works, but the pass fails again: the count keeps growing (no tight loop).
        m.handle(SyncEvent::RetryDue, NOW, j);
        m.handle(SyncEvent::Connected, NOW, j);
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, j),
            SyncAction::RetryAfter(Duration::from_millis(1600))
        );

        // A completed pass resets everything.
        m.handle(SyncEvent::RetryDue, NOW, j);
        m.handle(SyncEvent::Connected, NOW, j);
        m.handle(SyncEvent::PassCompleted, NOW, j);
        assert_eq!(m.failures(), 0);
        assert!(m.last_error().is_none());
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, j),
            SyncAction::RetryAfter(Duration::from_millis(100))
        );
    }

    #[test]
    fn jitter_is_injected() {
        let mut m = running();
        let mut values = [0.5, 0.25].into_iter();
        let mut j = move || values.next().unwrap_or(0.0);
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, &mut j),
            SyncAction::RetryAfter(Duration::from_millis(75))
        );
        m.handle(SyncEvent::RetryDue, NOW, &mut j);
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, &mut j),
            SyncAction::RetryAfter(Duration::from_millis(175))
        );
    }

    #[test]
    fn permanent_errors_stop_until_the_user_acts() {
        let mut m = SyncMachine::new(policy());
        let j = &mut fixed(0.0);
        m.handle(SyncEvent::Start, NOW, j);
        assert_eq!(
            m.handle(SyncEvent::Failed(auth()), NOW, j),
            SyncAction::Disconnect
        );
        let status = m.status();
        assert_eq!(status.state, AccountState::Error);
        assert_eq!(status.error.map(|e| e.code), Some(ErrorCode::AuthFailed));
        // Neither timers nor network changes retry a rejected login.
        assert_eq!(m.handle(SyncEvent::RetryDue, NOW, j), SyncAction::None);
        assert_eq!(m.handle(SyncEvent::Wake, NOW, j), SyncAction::None);
        assert_eq!(m.state(), &SyncState::Failed);
        // The user stored a new password.
        assert_eq!(m.handle(SyncEvent::RetryNow, NOW, j), SyncAction::Connect);
        assert_eq!(m.state(), &SyncState::Connecting { attempt: 0 });
        // Old error still shown until the new attempt resolves.
        assert_eq!(m.status().state, AccountState::Offline);
        m.handle(SyncEvent::Connected, NOW, j);
        m.handle(SyncEvent::PassCompleted, NOW, j);
        assert_eq!(m.status().state, AccountState::Idle);
        assert!(m.status().error.is_none());
    }

    #[test]
    fn wake_reconnects_at_once_and_resets_the_count() {
        let mut m = running();
        let j = &mut fixed(0.0);
        for _ in 0..5 {
            m.handle(SyncEvent::Failed(lost()), NOW, j);
            m.handle(SyncEvent::RetryDue, NOW, j);
        }
        m.handle(SyncEvent::Failed(lost()), NOW, j);
        assert!(matches!(m.state(), SyncState::Backoff { attempt: 6, .. }));
        assert_eq!(m.handle(SyncEvent::Wake, NOW, j), SyncAction::Connect);
        assert_eq!(m.failures(), 0);
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, j),
            SyncAction::RetryAfter(Duration::from_millis(100))
        );

        // From idle: the connection may be dead after sleep, reconnect instead of waiting
        // for the read limit.
        let mut idle = running();
        assert_eq!(idle.handle(SyncEvent::Wake, NOW, j), SyncAction::Connect);

        // A certificate error may heal after a network change (captive portal).
        let mut cert = SyncMachine::new(policy());
        cert.handle(SyncEvent::Start, NOW, j);
        cert.handle(
            SyncEvent::Failed(BackendError::CertUntrusted("UnknownIssuer".into()).into()),
            NOW,
            j,
        );
        assert_eq!(cert.handle(SyncEvent::Wake, NOW, j), SyncAction::Connect);
    }

    #[test]
    fn retry_now_cuts_the_backoff_short() {
        let mut m = running();
        let j = &mut fixed(0.0);
        m.handle(SyncEvent::Failed(lost()), NOW, j);
        m.handle(SyncEvent::RetryDue, NOW, j);
        m.handle(SyncEvent::Failed(lost()), NOW, j);
        assert_eq!(m.handle(SyncEvent::RetryNow, NOW, j), SyncAction::Connect);
        // Unlike Wake, the count stays: the next failure waits longer.
        assert_eq!(
            m.handle(SyncEvent::Failed(lost()), NOW, j),
            SyncAction::RetryAfter(Duration::from_millis(400))
        );
    }

    #[test]
    fn stop_ends_everything_and_ignores_later_events() {
        let mut m = running();
        let j = &mut fixed(0.0);
        m.handle(SyncEvent::Changed, NOW, j);
        assert_eq!(m.handle(SyncEvent::Stop, NOW, j), SyncAction::Disconnect);
        assert_eq!(m.state(), &SyncState::Stopped);
        for event in [
            SyncEvent::Connected,
            SyncEvent::PassCompleted,
            SyncEvent::Changed,
            SyncEvent::RetryDue,
            SyncEvent::Wake,
            SyncEvent::RetryNow,
            SyncEvent::Failed(lost()),
        ] {
            assert_eq!(m.handle(event, NOW, j), SyncAction::None);
        }
        assert_eq!(m.handle(SyncEvent::Start, NOW, j), SyncAction::Connect);
    }

    #[test]
    fn out_of_place_events_are_ignored() {
        let mut m = running();
        let j = &mut fixed(0.0);
        assert_eq!(m.handle(SyncEvent::Connected, NOW, j), SyncAction::None);
        assert_eq!(m.handle(SyncEvent::PassCompleted, NOW, j), SyncAction::None);
        assert_eq!(m.handle(SyncEvent::RetryDue, NOW, j), SyncAction::None);
        assert_eq!(m.handle(SyncEvent::Start, NOW, j), SyncAction::None);
        let p = SyncProgress {
            mailbox_id: MailboxId(1),
            done: 1,
            total: 2,
        };
        assert_eq!(m.handle(SyncEvent::Progress(p), NOW, j), SyncAction::None);
        assert_eq!(m.state(), &SyncState::Idle);
    }

    #[test]
    fn database_errors_are_permanent() {
        let mut m = running();
        let j = &mut fixed(0.0);
        m.handle(SyncEvent::Changed, NOW, j);
        assert_eq!(
            m.handle(
                SyncEvent::Failed(CoreError::Database("disk full".into())),
                NOW,
                j
            ),
            SyncAction::Disconnect
        );
        assert_eq!(m.status().state, AccountState::Error);
    }
}
