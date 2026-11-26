//! Learning from existing mail (decision 0077): the newest mail of the junk folders as spam,
//! then read inbox mail (replied first) as ham, in the background with progress
//! (`spamFilterChanged`) and cancel. Bodies that are not stored are fetched read-only like for
//! opening (without refreshing lists); offline, the stored header data is learned instead. A
//! read inbox mail the filter already scores as likely spam is left out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mail_model::{
    AccountId, CoreError, IpcError, MIN_LEARNED_SPAM, MessageId, SpamLearning, SpamLearningState,
};
use mail_store::SpamClass;

use super::{HAM_DOUBT, lock};
use crate::AppCore;
use crate::core::{Connector, WeakCore};
use crate::reading::SpamFetch;

/// Messages learned per transaction.
const CHUNK: usize = 25;
/// Progress events at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// One run of learning from existing mail.
pub(crate) struct Run {
    info: Mutex<SpamLearning>,
    cancel: AtomicBool,
}

impl Run {
    fn new() -> Self {
        Self {
            info: Mutex::new(SpamLearning {
                state: SpamLearningState::Running,
                done: 0,
                total: 0,
                spam: 0,
                ham: 0,
                skipped: 0,
                error: None,
            }),
            cancel: AtomicBool::new(false),
        }
    }

    pub(crate) fn snapshot(&self) -> SpamLearning {
        lock(&self.info).clone()
    }

    fn update(&self, f: impl FnOnce(&mut SpamLearning)) {
        f(&mut lock(&self.info));
    }

    fn running(&self) -> bool {
        lock(&self.info).state == SpamLearningState::Running
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

impl<C: Connector> AppCore<C> {
    /// Starts a run with these limits unless one runs. `invalid_argument` while the filter is
    /// off.
    pub(crate) async fn start_spam_learning(
        &self,
        spam_limit: u32,
        ham_limit: u32,
    ) -> Result<(), CoreError> {
        let settings = self.spam_settings().await?;
        if !settings.enabled {
            return Err(CoreError::InvalidArgument("the spam filter is off".into()));
        }
        let run = {
            let mut slot = lock(&self.spam().learning);
            if slot.as_ref().is_some_and(|run| run.running()) {
                return Ok(());
            }
            let run = Arc::new(Run::new());
            *slot = Some(Arc::clone(&run));
            run
        };
        // The mails to learn from are chosen now, so the first progress already has its total.
        let chosen = self
            .learning_candidates(&settings, spam_limit, ham_limit)
            .await;
        let (spam, ham) = match chosen {
            Ok(chosen) => chosen,
            Err(err) => {
                run.update(|info| {
                    info.state = SpamLearningState::Failed;
                    info.error = Some(IpcError::from(err.clone()));
                });
                self.spam_status_changed().await;
                return Err(err);
            }
        };
        run.update(|info| {
            info.total = u32::try_from(spam.len() + ham.len()).unwrap_or(u32::MAX);
        });
        tracing::info!(
            spam = spam.len(),
            ham = ham.len(),
            "spam filter: learning from existing mail"
        );
        let weak = self.downgrade();
        let task = tokio::spawn(async move {
            let result = learn(&weak, &run, spam, ham).await;
            run.update(|info| {
                info.state = match &result {
                    Ok(()) if run.cancelled() => SpamLearningState::Cancelled,
                    Ok(()) => SpamLearningState::Done,
                    Err(_) => SpamLearningState::Failed,
                };
                info.error = result.as_ref().err().map(IpcError::from);
            });
            let info = run.snapshot();
            match &result {
                Ok(()) => tracing::info!(
                    state = ?info.state,
                    spam = info.spam,
                    ham = info.ham,
                    skipped = info.skipped,
                    "spam filter: learning from existing mail ended"
                ),
                Err(err) => tracing::error!(
                    code = ?err.code(),
                    error = %err,
                    "spam filter: learning from existing mail failed"
                ),
            }
            if let Some(core) = weak.upgrade() {
                core.spam_status_changed().await;
            }
        });
        lock(&self.spam().tasks).push(task);
        self.spam_status_changed().await;
        Ok(())
    }
}

impl<C: Connector> AppCore<C> {
    /// The newest junk mail and read inbox mail (replied first) of the accounts the setting
    /// covers, not learned yet.
    async fn learning_candidates(
        &self,
        settings: &mail_model::SpamFilterSettings,
        spam_limit: u32,
        ham_limit: u32,
    ) -> Result<(Vec<MessageId>, Vec<MessageId>), CoreError> {
        let store = self.store_ref()?;
        let accounts: Vec<AccountId> = store
            .accounts()
            .await?
            .into_iter()
            .map(|a| a.id)
            .filter(|a| settings.applies_to(*a))
            .collect();
        let spam = if spam_limit > 0 {
            store
                .spam_learning_candidates(accounts.clone(), SpamClass::Spam, spam_limit)
                .await?
        } else {
            Vec::new()
        };
        let ham = if ham_limit > 0 {
            store
                .spam_learning_candidates(accounts, SpamClass::Ham, ham_limit)
                .await?
        } else {
            Vec::new()
        };
        Ok((spam, ham))
    }
}

async fn learn<C: Connector>(
    weak: &WeakCore<C>,
    run: &Run,
    spam: Vec<MessageId>,
    ham: Vec<MessageId>,
) -> Result<(), CoreError> {
    let Some(core) = weak.upgrade() else {
        return Ok(());
    };
    let store = core.store_ref()?.clone();
    drop(core);
    let mut last_event = Instant::now();
    for (class, ids) in [(SpamClass::Spam, spam), (SpamClass::Ham, ham)] {
        for chunk in ids.chunks(CHUNK) {
            if run.cancelled() {
                return Ok(());
            }
            let Some(core) = weak.upgrade() else {
                return Ok(());
            };
            // Doubting read inbox mail needs a model that knows some ham already.
            let size = store.spam_model_size().await?;
            let doubt = class == SpamClass::Ham
                && size.spam >= MIN_LEARNED_SPAM
                && size.ham >= MIN_LEARNED_SPAM;
            let mut items = Vec::with_capacity(chunk.len());
            let mut skipped = 0u32;
            let mut looked = 0u32;
            for message in chunk {
                if run.cancelled() {
                    break;
                }
                looked += 1;
                let Some(tokens) = core.spam_tokens_of(*message, SpamFetch::Quiet).await? else {
                    skipped += 1;
                    continue;
                };
                if doubt
                    && let Some(score) = core.spam_score_of(&tokens).await?
                    && score.score >= HAM_DOUBT
                {
                    skipped += 1;
                    continue;
                }
                items.push((*message, tokens.hashes().to_vec()));
            }
            let tried = items.len();
            let learned = store
                .learn_spam(items, class, None, core.now_ms())
                .await?
                .iter()
                .filter(|l| l.is_some())
                .count();
            // Messages deleted meanwhile count as left out.
            skipped += u32::try_from(tried - learned).unwrap_or(u32::MAX);
            let learned = u32::try_from(learned).unwrap_or(u32::MAX);
            run.update(|info| {
                info.done += looked;
                info.skipped += skipped;
                match class {
                    SpamClass::Spam => info.spam += learned,
                    SpamClass::Ham => info.ham += learned,
                }
            });
            if last_event.elapsed() >= PROGRESS_EVERY {
                core.spam_status_changed().await;
                last_event = Instant::now();
            }
        }
    }
    Ok(())
}
