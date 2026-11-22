//! Notices a resume from sleep without OS notifications: the wall clock keeps running during
//! sleep, Tokio's monotonic clock does not (macOS: `CLOCK_UPTIME_RAW`). When the two drift apart
//! by more than the threshold between two checks, every account reconnects at once instead of
//! waiting for its dead connections to time out.
//!
//! Limits: a large manual clock change also counts as a resume (harmless: one reconnect). On
//! Windows the monotonic clock may count sleep too; then only the time limits and the
//! `AppCore::resume` hook notice it (unchecked, stage 6).

use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::account::Registry;
use crate::clock::Clock;

/// Starts the detector. It ends when the registry is gone.
pub(crate) fn spawn(
    registry: Weak<Registry>,
    clock: Arc<dyn Clock>,
    check: Duration,
    threshold: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let check = check.max(Duration::from_millis(10));
        let threshold = i64::try_from(threshold.as_millis()).unwrap_or(i64::MAX);
        let mut wall = clock.now_ms();
        let mut mono = Instant::now();
        loop {
            tokio::time::sleep(check).await;
            let (wall_now, mono_now) = (clock.now_ms(), Instant::now());
            let wall_elapsed = wall_now.saturating_sub(wall);
            let mono_elapsed =
                i64::try_from(mono_now.duration_since(mono).as_millis()).unwrap_or(i64::MAX);
            let drift = wall_elapsed.saturating_sub(mono_elapsed);
            (wall, mono) = (wall_now, mono_now);
            let Some(registry) = registry.upgrade() else {
                return;
            };
            if drift.abs() > threshold {
                tracing::info!(
                    drift_ms = drift,
                    "wall clock jumped (resume from sleep?); reconnecting all accounts"
                );
                registry.wake_all();
            }
        }
    })
}
