//! Wall-clock time, injected so tests can move it (for example to simulate a resume from sleep).

use std::time::{SystemTime, UNIX_EPOCH};

/// Source of the wall-clock time. Timers and timeouts use Tokio's monotonic clock; this one is
/// for timestamps the UI shows (`retryAt`, `lastSyncAt`) and for noticing sleep (see
/// `sync::wake`).
pub trait Clock: Send + Sync + 'static {
    /// Unix milliseconds.
    fn now_ms(&self) -> i64;
}

/// The operating system's clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(since) => i64::try_from(since.as_millis()).unwrap_or(i64::MAX),
            // A clock before 1970 is broken; report the epoch rather than a negative time.
            Err(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_after_2026() {
        assert!(SystemClock.now_ms() > 1_764_000_000_000);
    }
}
