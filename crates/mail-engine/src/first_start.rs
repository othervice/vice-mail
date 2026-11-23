//! Stage 15 (decision 0041): whether the guided first start still has to be shown.
//!
//! One device setting, `first_start` (not in `SYNCED_SETTINGS`: a second device gets its own
//! first start), holding how it ended: `{"outcome":"completed"}`, `"skipped"` or `"notNeeded"`.
//!
//! - Nothing stored and no account: the first start shows (a new installation).
//! - Nothing stored but an account: an installation from before the first start existed. It
//!   counts as done and the core stores `notNeeded` the first time it is asked, so removing the
//!   last account later does not bring the first start back.
//! - Something stored: it never shows again, whatever happens to the accounts.
//!
//! The steps of the first start use the commands they always use (add an account, preferences,
//! the update switch, the settings sync); this module only answers "show it?" and records the end.

use mail_model::{CoreError, FirstStart, FirstStartEnd, FirstStartOutcome};
use mail_store::GLOBAL_SCOPE;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::AppCore;
use crate::core::Connector;

/// Key of the device setting.
pub(crate) const FIRST_START_KEY: &str = "first_start";

/// Serialises "read, then maybe store `notNeeded`" against "store the end", so a slow read
/// cannot overwrite how the user ended the first start. One core per process; in tests the cores
/// share it, which only orders their (short) calls.
static LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    outcome: FirstStartOutcome,
}

fn encode(outcome: FirstStartOutcome) -> Result<String, CoreError> {
    serde_json::to_string(&Stored { outcome })
        .map_err(|err| CoreError::Internal(format!("encode first start: {err}")))
}

impl<C: Connector> AppCore<C> {
    /// Whether the main window shows the guided first start (decision 0041).
    pub async fn get_first_start(&self) -> Result<FirstStart, CoreError> {
        let _guard = LOCK.lock().await;
        if let Some(outcome) = self.stored_first_start().await? {
            return Ok(FirstStart::ended(outcome));
        }
        if self.store_ref()?.accounts().await?.is_empty() {
            return Ok(FirstStart::PENDING);
        }
        // An installation that had accounts before the first start existed: never show it,
        // also not after its last account is removed.
        self.put_first_start(FirstStartOutcome::NotNeeded).await?;
        tracing::info!("first start not needed: accounts exist");
        Ok(FirstStart::ended(FirstStartOutcome::NotNeeded))
    }

    /// The user finished the first start (last step) or skipped it; it does not show again.
    /// Calling it again replaces the stored outcome (the UI calls it once).
    pub async fn finish_first_start(&self, end: FirstStartEnd) -> Result<FirstStart, CoreError> {
        let _guard = LOCK.lock().await;
        let outcome = FirstStartOutcome::from(end);
        self.put_first_start(outcome).await?;
        tracing::info!(?outcome, "first start ended");
        Ok(FirstStart::ended(outcome))
    }

    /// The stored outcome. A value this version cannot read (damaged, or from a newer version)
    /// counts as "ended": showing the first start again to someone who already set up the app
    /// would be worse than not showing it to someone who never saw it.
    async fn stored_first_start(&self) -> Result<Option<FirstStartOutcome>, CoreError> {
        let settings = self.store_ref()?.settings(GLOBAL_SCOPE).await?;
        Ok(settings
            .into_iter()
            .find(|setting| setting.key == FIRST_START_KEY)
            .map(|setting| {
                serde_json::from_str::<Stored>(&setting.value)
                    .map(|stored| stored.outcome)
                    .unwrap_or_else(|err| {
                        tracing::warn!(key = FIRST_START_KEY, %err, "stored first start unreadable; counts as completed");
                        FirstStartOutcome::Completed
                    })
            }))
    }

    async fn put_first_start(&self, outcome: FirstStartOutcome) -> Result<(), CoreError> {
        self.store_ref()?
            .put_settings(
                GLOBAL_SCOPE,
                vec![(FIRST_START_KEY.to_owned(), encode(outcome)?)],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
