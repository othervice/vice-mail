//! Stage 13 in the core: AI in the app (decisions 0010, 0019, 0024), over `mail-ai`.
//!
//! - **Settings** ([`settings`]) are one device setting (`ai.settings`): the switch (off until
//!   the user turns it on), provider profiles, the profile per function, the cloud permission
//!   per account and the consent per cloud profile. Keys live in the keychain
//!   (`CredentialRef::for_ai_profile`), never in the database, a log or an IPC answer.
//! - **Every request** goes through [`Session`]: AI on, the function has a profile with a
//!   model, a cloud profile has consent (else `ai_consent_required`, nothing sent), the key is
//!   read, the provider is built (`mail_ai::build_provider` with the app's TLS), the request id
//!   is registered for `cancel_ai_request`. Before each provider request the core emits
//!   `aiRequest` with `sending` (messages, bytes, accounts, host, model), while the answer
//!   arrives `receiving`, at the end `finished`; each provider request is one entry of the
//!   request log (metadata only).
//! - **Cloud permission per account** is checked before any body is loaded and again by
//!   `mail-ai` right before sending ([`mail_ai::CloudPolicy`]); questions leave out the
//!   candidates of accounts without permission.
//! - **Results** are checked data: claims keep only references to messages that were sent.
//!   Nothing here turns model output into an action: a reply suggestion becomes a draft only
//!   through `ai_open_reply_draft`, which the user triggers, and the writing help returns a
//!   preview.
//! - **Open-reply hints** ([`open_replies`]) are rules over stored headers, without AI.

mod open_replies;
mod settings;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use mail_ai::{
    AiError, AiProvider, AnswerLanguage, AnswerOutcome, Budget, CancelToken, ChatRequest, Claim,
    Context, MailInput, ProviderConfig, TransferInfo,
};
use mail_model::{
    AccountId, AiAnswer, AiAvailability, AiClaim, AiConnectionTest, AiDeadline, AiFailure,
    AiFunction, AiLocality, AiModel, AiProfileInput, AiReplySuggestion, AiRequestOutcome,
    AiRequestPage, AiRequestPhase, AiSettingsInfo, AiSummary, AiTargetLanguage, AiTone, AiTransfer,
    AiTrimming, AiWritingResult, AiWritingTask, ComposeFormat, CoreError, CoreEvent, Draft,
    DraftUpdate, LanguagePreference, MAX_AI_QUESTION_CHARS, MAX_AI_REQUEST_ID_CHARS, MessageId,
    NewDraft, OpenReplySettings, QuoteLanguage, Secret,
};
use mail_store::{NewAiRequest, StoredMessage};

use crate::core::{AppCore, Connector};
use crate::credentials::CredentialRef;
use crate::search::LocalZone;

use settings::StoredAi;
pub(crate) use settings::StoredProfile;

/// Builds a provider from a profile. The product uses `mail_ai::build_provider` with the
/// app's TLS configuration; tests wrap it (for example to label a local mock as a cloud
/// provider).
pub(crate) type ProviderFactory =
    dyn Fn(ProviderConfig) -> Result<Box<dyn AiProvider>, AiError> + Send + Sync;

/// Most search candidates a question looks at.
const MAX_CANDIDATES: u32 = 20;
/// `receiving` events at most every this many characters.
const PROGRESS_STEP: usize = 256;

/// The core's part of AI in the app.
pub(crate) struct AiState {
    /// Running requests by the UI's request id.
    requests: Mutex<HashMap<String, CancelToken>>,
    /// Serialises settings changes (settings and keychain change together).
    admin: tokio::sync::Mutex<()>,
    factory: Mutex<Option<Arc<ProviderFactory>>>,
}

impl Default for AiState {
    fn default() -> Self {
        Self {
            requests: Mutex::default(),
            admin: tokio::sync::Mutex::new(()),
            factory: Mutex::default(),
        }
    }
}

impl AiState {
    /// Cancels every running request (shutdown).
    pub(crate) fn cancel_all(&self) {
        for token in lock(&self.requests).values() {
            token.cancel();
        }
    }

    fn factory(&self) -> Arc<ProviderFactory> {
        if let Some(factory) = lock(&self.factory).as_ref() {
            return Arc::clone(factory);
        }
        Arc::new(|config: ProviderConfig| {
            let tls = mail_imap::tls::client_config(None)
                .map_err(|err| AiError::Internal(format!("TLS configuration: {err}")))?;
            mail_ai::build_provider(config, &tls)
        })
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `AiError` as the UI sees it. Texts of `AiError` never contain a key.
pub(crate) fn failure(err: AiError) -> CoreError {
    let detail = err.to_string();
    CoreError::Ai(match err {
        AiError::Cancelled => AiFailure::Cancelled,
        AiError::NothingToSend => return CoreError::InvalidArgument(detail),
        AiError::NotPermitted { accounts } => AiFailure::NotPermitted {
            accounts: u32::try_from(accounts.len()).unwrap_or(u32::MAX),
        },
        AiError::Connect(_) | AiError::Timeout(_) => AiFailure::Unreachable(detail),
        AiError::Unauthorized { .. } => AiFailure::Unauthorized(detail),
        AiError::RateLimited { .. } | AiError::Overloaded { .. } => AiFailure::Busy(detail),
        AiError::Refused { .. } => AiFailure::Refused,
        AiError::InvalidConfig(_)
        | AiError::NotFound { .. }
        | AiError::Server { .. }
        | AiError::Rejected { .. }
        | AiError::Truncated
        | AiError::RequestTooLarge
        | AiError::ResponseTooLarge
        | AiError::Protocol(_)
        | AiError::InvalidOutput(_)
        | AiError::Internal(_) => AiFailure::Failed(detail),
    })
}

pub(crate) const fn locality(locality: mail_ai::Locality) -> AiLocality {
    match locality {
        mail_ai::Locality::ThisDevice => AiLocality::ThisDevice,
        mail_ai::Locality::LocalNetwork => AiLocality::LocalNetwork,
        mail_ai::Locality::Internet => AiLocality::Internet,
    }
}

fn claims(list: Vec<Claim>) -> Vec<AiClaim> {
    list.into_iter()
        .map(|claim| AiClaim {
            text: claim.text,
            sources: claim.sources,
        })
        .collect()
}

const fn writing_task(task: AiWritingTask) -> mail_ai::WritingTask {
    use mail_ai::{TargetLanguage as L, Tone as T, WritingTask as W};
    match task {
        AiWritingTask::Improve => W::Improve,
        AiWritingTask::Shorten => W::Shorten,
        AiWritingTask::Rephrase => W::Rephrase,
        AiWritingTask::Tone { tone } => W::Tone(match tone {
            AiTone::Formal => T::Formal,
            AiTone::Friendly => T::Friendly,
            AiTone::Neutral => T::Neutral,
            AiTone::Direct => T::Direct,
        }),
        AiWritingTask::Translate { language } => W::Translate(match language {
            AiTargetLanguage::En => L::English,
            AiTargetLanguage::De => L::German,
            AiTargetLanguage::Fr => L::French,
            AiTargetLanguage::Es => L::Spanish,
            AiTargetLanguage::It => L::Italian,
            AiTargetLanguage::Nl => L::Dutch,
            AiTargetLanguage::Pt => L::Portuguese,
            AiTargetLanguage::Pl => L::Polish,
        }),
    }
}

fn request_id(id: &str) -> Result<String, CoreError> {
    let id = id.trim();
    if id.is_empty()
        || id.chars().count() > usize::try_from(MAX_AI_REQUEST_ID_CHARS).unwrap_or(usize::MAX)
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(CoreError::InvalidArgument(format!(
            "a request id has 1 to {MAX_AI_REQUEST_ID_CHARS} letters, digits, '-' or '_'"
        )));
    }
    Ok(id.to_owned())
}

/// One request of the UI: the provider, the cancel token, what the log needs. Unregisters
/// its id when dropped.
struct Session<'a, C: Connector> {
    core: &'a AppCore<C>,
    id: String,
    function: AiFunction,
    profile: StoredProfile,
    provider: Box<dyn AiProvider>,
    cancel: CancelToken,
    cloud: Vec<AccountId>,
}

impl<C: Connector> Drop for Session<'_, C> {
    fn drop(&mut self) {
        lock(&self.core.ai().requests).remove(&self.id);
    }
}

impl<C: Connector> Session<'_, C> {
    fn locality(&self) -> mail_ai::Locality {
        self.provider.info().locality
    }

    fn cloud_allowed(&self, account: &AccountId) -> bool {
        self.cloud.contains(account)
    }

    fn emit(&self, phase: AiRequestPhase) {
        self.core.emit_event(CoreEvent::AiRequest {
            request_id: self.id.clone(),
            phase,
        });
    }

    fn transfer(&self, info: &TransferInfo) -> AiTransfer {
        let provider = self.provider.info();
        AiTransfer {
            function: self.function,
            profile_id: self.profile.id.clone(),
            profile_name: self.profile.name.clone(),
            host: provider.host.clone(),
            locality: locality(provider.locality),
            model: info.model.clone(),
            mails: info.mails,
            accounts: info.accounts.clone(),
            bytes: u32::try_from(info.bytes).unwrap_or(u32::MAX),
            attachment_names: info.attachment_names,
            quotes_removed: info.quotes_removed,
            trimming: AiTrimming {
                mails_left_out: info.trimming.mails_left_out,
                mails_shortened: info.trimming.mails_shortened,
                chars_removed: u32::try_from(info.trimming.chars_removed).unwrap_or(u32::MAX),
            },
        }
    }

    /// Announces one provider request and returns what the UI was told.
    fn sending(&self, info: &TransferInfo) -> AiTransfer {
        let transfer = self.transfer(info);
        self.emit(AiRequestPhase::Sending {
            transfer: transfer.clone(),
        });
        transfer
    }

    /// The progress callback for `mail-ai`: a `receiving` event every [`PROGRESS_STEP`]
    /// characters.
    fn progress(&self) -> impl FnMut(usize) + Send + '_ {
        let mut next = 0_usize;
        move |chars| {
            if chars >= next {
                next = chars + PROGRESS_STEP;
                self.emit(AiRequestPhase::Receiving {
                    chars: u32::try_from(chars).unwrap_or(u32::MAX),
                });
            }
        }
    }

    /// Runs one provider request (`run`), logs it and maps the error.
    async fn request<T>(
        &self,
        transfer: &AiTransfer,
        run: impl Future<Output = Result<T, AiError>>,
    ) -> Result<T, CoreError> {
        let started = Instant::now();
        let result = run.await;
        let (outcome, error) = match &result {
            Ok(_) => (AiRequestOutcome::Ok, None),
            Err(AiError::Cancelled) => (AiRequestOutcome::Cancelled, None),
            Err(err) => (AiRequestOutcome::Failed, Some(failure(err.clone()).code())),
        };
        let entry = NewAiRequest {
            at: self.core.now_ms(),
            function: self.function,
            profile_name: transfer.profile_name.clone(),
            host: transfer.host.clone(),
            locality: transfer.locality,
            model: transfer.model.clone(),
            mails: transfer.mails,
            bytes: transfer.bytes,
            outcome,
            error,
            duration_ms: u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX),
        };
        if let Ok(store) = self.core.store_ref()
            && let Err(err) = store.record_ai_request(entry, self.core.now_ms()).await
        {
            tracing::warn!(error = %err, "AI request log could not be written");
        }
        result.map_err(failure)
    }

    /// Ends the session with its outcome event.
    fn finish<T>(&self, result: Result<T, CoreError>) -> Result<T, CoreError> {
        let outcome = match &result {
            Ok(_) => AiRequestOutcome::Ok,
            Err(CoreError::Ai(AiFailure::Cancelled)) => AiRequestOutcome::Cancelled,
            Err(_) => AiRequestOutcome::Failed,
        };
        self.emit(AiRequestPhase::Finished { outcome });
        result
    }
}

impl<C: Connector> AppCore<C> {
    async fn ai_stored(&self) -> Result<StoredAi, CoreError> {
        settings::load(self.store_ref()?).await
    }

    /// The user's language for answers, and the date order of generated queries.
    async fn ai_language(&self) -> AnswerLanguage {
        match self.get_preferences().await.map(|p| p.language) {
            Ok(LanguagePreference::De) => AnswerLanguage::German,
            _ => AnswerLanguage::English,
        }
    }

    /// Checks everything before a request and opens its session. Nothing is sent here.
    async fn ai_session(
        &self,
        id: &str,
        function: AiFunction,
    ) -> Result<Session<'_, C>, CoreError> {
        let id = request_id(id)?;
        let stored = self.ai_stored().await?;
        let profile = stored.usable_profile(function)?.clone();
        let provider = self.ai_provider(&profile).await?;
        if !provider.info().locality.is_local() && !profile.consented() {
            return Err(CoreError::Ai(AiFailure::ConsentRequired {
                profile_id: profile.id.clone(),
            }));
        }
        let cancel = CancelToken::new();
        {
            let mut requests = lock(&self.ai().requests);
            if requests.contains_key(&id) {
                return Err(CoreError::InvalidArgument(format!(
                    "the request id {id} is in use"
                )));
            }
            requests.insert(id.clone(), cancel.clone());
        }
        Ok(Session {
            core: self,
            id,
            function,
            profile,
            provider,
            cancel,
            cloud: stored.cloud_accounts,
        })
    }

    /// Builds the provider of a profile with its key from the keychain.
    async fn ai_provider(&self, profile: &StoredProfile) -> Result<Box<dyn AiProvider>, CoreError> {
        let key = if profile.has_key {
            self.credentials_ref()
                .get(&CredentialRef::for_ai_profile(&profile.id))
                .await?
        } else {
            None
        };
        let config = profile.config(key);
        (self.ai().factory())(config).map_err(failure)
    }

    /// A message as `mail-ai` takes it: the visible text of the body (stored, or fetched
    /// read-only), else the stored headers with the preview.
    async fn ai_mail_input(&self, summary: &StoredMessage) -> MailInput {
        let body = match self.reading_ref() {
            Ok(reading) => match reading.content_for_model(summary).await {
                Ok(body) => body,
                Err(err) => {
                    tracing::warn!(message = summary.id.0, code = ?err.code(), "AI: body not available");
                    None
                }
            },
            Err(_) => None,
        };
        match body {
            Some((parsed, _)) => {
                let mut input = MailInput::from_parsed(summary.id, summary.account_id, &parsed);
                if input.date.is_none() {
                    input.date = Some(summary.date);
                }
                input
            }
            None => MailInput {
                id: summary.id,
                account: summary.account_id,
                from: summary.from.clone(),
                to: summary.to.clone(),
                cc: summary.cc.clone(),
                date: Some(summary.date),
                subject: summary.subject.clone(),
                text: summary.preview.clone().unwrap_or_default(),
                hidden_text_chars: 0,
                attachments: Vec::new(),
            },
        }
    }

    /// The thread of `message`, oldest first, up to and including `message` when `until` is
    /// set. Fails with `ai_not_permitted` before any body is loaded when the provider is not
    /// local and the account has no cloud permission.
    async fn ai_thread(
        &self,
        session: &Session<'_, C>,
        message: MessageId,
        until: bool,
    ) -> Result<Vec<MailInput>, CoreError> {
        let store = self.store_ref()?;
        let Some(summary) = store.message_summary(message).await? else {
            return Err(CoreError::NotFound(format!("message {}", message.0)));
        };
        if !session.locality().is_local() && !session.cloud_allowed(&summary.account_id) {
            return Err(failure(AiError::NotPermitted {
                accounts: vec![summary.account_id],
            }));
        }
        let view = store.thread(message, None).await?;
        let mut inputs = Vec::new();
        for entry in &view.messages {
            if session.cancel.is_cancelled() {
                return Err(CoreError::Ai(AiFailure::Cancelled));
            }
            let Some(member) = store.message_summary(entry.message.id).await? else {
                continue;
            };
            if member.account_id != summary.account_id {
                continue;
            }
            inputs.push(self.ai_mail_input(&member).await);
            if until && member.id == message {
                break;
            }
        }
        if inputs.is_empty() {
            inputs.push(self.ai_mail_input(&summary).await);
        }
        Ok(inputs)
    }

    fn ai_context<'z>(&self, language: AnswerLanguage, zone: &'z LocalZone) -> Context<'z> {
        Context {
            language,
            time_zone: zone,
            now: self.now_ms().div_euclid(1000),
            slash_dates: mail_model::search::SlashDateOrder::DayFirst,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Settings
    // -----------------------------------------------------------------------------------------

    /// The settings page "AI in the app".
    pub async fn get_ai_settings(&self) -> Result<AiSettingsInfo, CoreError> {
        let stored = self.ai_stored().await?;
        let accounts = self.store_ref()?.accounts().await?;
        Ok(stored.info(&accounts))
    }

    /// Which functions are usable now (the AI panel is hidden without any).
    pub async fn get_ai_availability(&self) -> Result<AiAvailability, CoreError> {
        let stored = self.ai_stored().await?;
        let usable = |function| stored.usable_profile(function).is_ok();
        Ok(AiAvailability {
            enabled: stored.enabled,
            summary: usable(AiFunction::Summary),
            reply_suggestion: usable(AiFunction::ReplySuggestion),
            writing_help: usable(AiFunction::WritingHelp),
            search: usable(AiFunction::Search),
        })
    }

    async fn ai_update(
        &self,
        change: impl FnOnce(&mut StoredAi) -> Result<(), CoreError>,
    ) -> Result<AiSettingsInfo, CoreError> {
        {
            let _admin = self.ai().admin.lock().await;
            let store = self.store_ref()?;
            let mut stored = settings::load(store).await?;
            change(&mut stored)?;
            settings::save(store, &stored).await?;
        }
        self.ai_settings_changed().await;
        self.get_ai_settings().await
    }

    /// Decision 0027: every window hears that the AI settings changed (`aiSettingsChanged`,
    /// with what is usable now), so a second window or the panel needs no reload of its own.
    async fn ai_settings_changed(&self) {
        match self.get_ai_availability().await {
            Ok(availability) => self.emit_event(CoreEvent::AiSettingsChanged { availability }),
            Err(err) => tracing::warn!(error = %err, "AI availability unreadable after a change"),
        }
    }

    /// The master switch. Off: no request leaves the device, running requests are cancelled.
    pub async fn set_ai_enabled(&self, enabled: bool) -> Result<AiSettingsInfo, CoreError> {
        let info = self
            .ai_update(|stored| {
                stored.enabled = enabled;
                Ok(())
            })
            .await?;
        if !enabled {
            self.ai().cancel_all();
        }
        tracing::info!(enabled, "AI in the app switched");
        Ok(info)
    }

    /// Creates or changes a provider profile. The key goes to the keychain; a changed host
    /// withdraws the consent.
    pub async fn save_ai_profile(
        &self,
        input: AiProfileInput,
    ) -> Result<AiSettingsInfo, CoreError> {
        let _admin = self.ai().admin.lock().await;
        let store = self.store_ref()?;
        let mut stored = settings::load(store).await?;
        let (profile, key) = settings::apply_input(&mut stored, input, self.now_ms())?;
        // The provider validates the URL (scheme, credentials, `http` only locally) without
        // connecting.
        let check = profile.config(None);
        mail_ai::build_provider(check, rustls_for_check()?.as_ref())
            .map_err(|err| CoreError::InvalidArgument(err.to_string()))?;
        let entry = CredentialRef::for_ai_profile(&profile.id);
        match key {
            settings::KeyAction::Keep => {}
            settings::KeyAction::Remove => {
                self.credentials_ref().delete(&entry).await?;
            }
            settings::KeyAction::Set(key) => {
                self.credentials_ref().set(&entry, key).await?;
            }
        }
        settings::save(store, &stored).await?;
        drop(_admin);
        tracing::info!(profile = %profile.id, "AI profile saved");
        self.ai_settings_changed().await;
        self.get_ai_settings().await
    }

    /// Deletes a profile and its key; functions that used it are off.
    pub async fn delete_ai_profile(&self, profile_id: String) -> Result<AiSettingsInfo, CoreError> {
        let info = self
            .ai_update(|stored| stored.remove_profile(&profile_id))
            .await?;
        if let Err(err) = self
            .credentials_ref()
            .delete(&CredentialRef::for_ai_profile(&profile_id))
            .await
        {
            tracing::error!(error = %err, "keychain entry of a deleted AI profile could not be removed");
        }
        Ok(info)
    }

    /// The profile a function uses, or off (`null`).
    pub async fn set_ai_function(
        &self,
        function: AiFunction,
        profile_id: Option<String>,
    ) -> Result<AiSettingsInfo, CoreError> {
        self.ai_update(|stored| stored.set_function(function, profile_id))
            .await
    }

    /// Whether cloud providers may see an account's mail (default: no).
    pub async fn set_ai_account_cloud(
        &self,
        account: AccountId,
        allowed: bool,
    ) -> Result<AiSettingsInfo, CoreError> {
        if self.store_ref()?.account(account).await?.is_none() {
            return Err(CoreError::NotFound(format!("account {account}")));
        }
        self.ai_update(|stored| {
            stored.cloud_accounts.retain(|a| *a != account);
            if allowed {
                stored.cloud_accounts.push(account);
            }
            Ok(())
        })
        .await
    }

    /// Records (or withdraws) the user's consent to send data to a cloud profile's host.
    pub async fn set_ai_consent(
        &self,
        profile_id: String,
        granted: bool,
    ) -> Result<AiSettingsInfo, CoreError> {
        let now = self.now_ms();
        self.ai_update(|stored| stored.set_consent(&profile_id, granted, now))
            .await
    }

    /// The open-reply hints: on or off, and after how many days a sent question counts.
    pub async fn set_open_reply_settings(
        &self,
        settings: OpenReplySettings,
    ) -> Result<AiSettingsInfo, CoreError> {
        if settings.days == 0 || settings.days > 30 {
            return Err(CoreError::InvalidArgument(
                "the days of open-reply hints are 1 to 30".into(),
            ));
        }
        self.ai_update(|stored| {
            stored.open_replies = settings;
            Ok(())
        })
        .await
    }

    /// The request log, newest first.
    pub async fn list_ai_requests(
        &self,
        before: Option<mail_model::AiRequestId>,
        limit: u32,
    ) -> Result<AiRequestPage, CoreError> {
        Ok(self.store_ref()?.ai_requests(before, limit).await?)
    }

    /// Cancels a running request; `false` if none runs under this id.
    pub fn cancel_ai_request(&self, request_id: &str) -> bool {
        match lock(&self.ai().requests).get(request_id.trim()) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// The models of a profile (sends the key, no mail). Needs AI on.
    pub async fn list_ai_models(&self, profile_id: String) -> Result<Vec<AiModel>, CoreError> {
        let stored = self.ai_stored().await?;
        let profile = stored.enabled_profile(&profile_id)?.clone();
        let provider = self.ai_provider(&profile).await?;
        let models = provider
            .list_models(&CancelToken::new())
            .await
            .map_err(failure)?;
        Ok(models
            .into_iter()
            .map(|model| AiModel {
                id: model.id,
                display_name: model.display_name,
            })
            .collect())
    }

    /// "Test connection": the model list and, with a model chosen, a short request without
    /// any mail content. Needs AI on, not consent (nothing of the user's is sent).
    pub async fn test_ai_connection(
        &self,
        profile_id: String,
    ) -> Result<AiConnectionTest, CoreError> {
        let stored = self.ai_stored().await?;
        let profile = stored.enabled_profile(&profile_id)?.clone();
        let provider = self.ai_provider(&profile).await?;
        let cancel = CancelToken::new();
        let models = provider.list_models(&cancel).await.map_err(failure)?;
        let model = profile.model.trim();
        if model.is_empty() {
            return Ok(AiConnectionTest {
                models: u32::try_from(models.len()).unwrap_or(u32::MAX),
                model_listed: None,
                chat_ok: None,
                chat_error: None,
            });
        }
        let request = ChatRequest::connection_test(model).map_err(failure)?;
        let chat = match provider.chat(&request, &cancel).await {
            Ok(stream) => mail_ai::collect(stream, |_| {})
                .await
                .and_then(mail_ai::Completed::into_text),
            Err(err) => Err(err),
        };
        Ok(AiConnectionTest {
            models: u32::try_from(models.len()).unwrap_or(u32::MAX),
            model_listed: Some(models.iter().any(|m| m.id == model)),
            chat_ok: Some(chat.is_ok()),
            chat_error: chat.err().map(|err| failure(err).code()),
        })
    }

    // -----------------------------------------------------------------------------------------
    // Functions
    // -----------------------------------------------------------------------------------------

    /// Summarises the thread of `message` (key points, open questions, deadlines, each with
    /// the messages it is based on). Progress as `aiRequest` events; `cancel_ai_request`
    /// ends it with `ai_cancelled`.
    pub async fn ai_summarize_thread(
        &self,
        request_id: String,
        message: MessageId,
    ) -> Result<AiSummary, CoreError> {
        let session = self.ai_session(&request_id, AiFunction::Summary).await?;
        let result = self.summarize_in(&session, message).await;
        session.finish(result)
    }

    async fn summarize_in(
        &self,
        session: &Session<'_, C>,
        message: MessageId,
    ) -> Result<AiSummary, CoreError> {
        let thread = self.ai_thread(session, message, false).await?;
        let zone = LocalZone::system();
        let context = self.ai_context(self.ai_language().await, &zone);
        let prepared =
            mail_ai::prepare_summary(&session.profile.model, &thread, Budget::THREAD, &context)
                .map_err(failure)?;
        let transfer = session.sending(prepared.transfer());
        let cloud = |account: &AccountId| session.cloud_allowed(account);
        let summary = session
            .request(
                &transfer,
                mail_ai::summarize(
                    session.provider.as_ref(),
                    &prepared,
                    &cloud,
                    &session.cancel,
                    session.progress(),
                ),
            )
            .await?;
        Ok(AiSummary {
            key_points: claims(summary.key_points),
            open_questions: claims(summary.open_questions),
            deadlines: summary
                .deadlines
                .into_iter()
                .map(|d| AiDeadline {
                    text: d.text,
                    due: d.due,
                    sources: d.sources,
                })
                .collect(),
            dropped: summary.dropped,
            transfer,
        })
    }

    /// A reply suggestion to `message` (with the thread up to it), as text for a card;
    /// `ai_open_reply_draft` turns it into an editable draft.
    pub async fn ai_suggest_reply(
        &self,
        request_id: String,
        message: MessageId,
    ) -> Result<AiReplySuggestion, CoreError> {
        let session = self
            .ai_session(&request_id, AiFunction::ReplySuggestion)
            .await?;
        let result = self.reply_in(&session, message).await;
        session.finish(result)
    }

    async fn reply_in(
        &self,
        session: &Session<'_, C>,
        message: MessageId,
    ) -> Result<AiReplySuggestion, CoreError> {
        let thread = self.ai_thread(session, message, true).await?;
        let account = thread
            .last()
            .map(|mail| mail.account)
            .ok_or_else(|| CoreError::NotFound(format!("message {}", message.0)))?;
        let user = self
            .store_ref()?
            .account(account)
            .await?
            .map(|a| mail_model::Address {
                name: Some(a.display_name),
                email: a.email,
            });
        let zone = LocalZone::system();
        let context = self.ai_context(self.ai_language().await, &zone);
        let prepared = mail_ai::prepare_reply(
            &session.profile.model,
            &thread,
            user.as_ref(),
            Budget::THREAD,
            &context,
        )
        .map_err(failure)?;
        let transfer = session.sending(prepared.transfer());
        let cloud = |account: &AccountId| session.cloud_allowed(account);
        let suggestion = session
            .request(
                &transfer,
                mail_ai::suggest_reply(
                    session.provider.as_ref(),
                    &prepared,
                    &cloud,
                    &session.cancel,
                    session.progress(),
                ),
            )
            .await?;
        Ok(AiReplySuggestion {
            text: suggestion.text,
            in_reply_to: suggestion.in_reply_to,
            transfer,
        })
    }

    /// "Open draft" of a reply suggestion: a reply draft of stage 5 to `message` (identity,
    /// recipients, subject and quote as "Reply" prepares them) with `text` as its body. The
    /// draft stays open for editing; nothing is sent.
    pub async fn ai_open_reply_draft(
        &self,
        message: MessageId,
        text: String,
    ) -> Result<Draft, CoreError> {
        let compose = self.compose_ref()?;
        let language = match self.get_preferences().await.map(|p| p.language) {
            Ok(LanguagePreference::De) => QuoteLanguage::De,
            _ => QuoteLanguage::En,
        };
        let draft = compose
            .create(NewDraft::Reply {
                message_id: message,
                language,
            })
            .await?;
        let update = DraftUpdate {
            identity_id: draft.identity_id,
            to: draft.to.clone(),
            cc: draft.cc.clone(),
            bcc: Vec::new(),
            subject: draft.subject.clone(),
            format: ComposeFormat::Text,
            body_html: String::new(),
            body_text: text,
            include_quote: draft.include_quote,
            quote_position: draft.quote_position,
            include_signature: draft.include_signature,
            signature_id: draft.signature_id.clone(),
        };
        match compose.save(draft.id.clone(), update).await {
            Ok(saved) => Ok(saved),
            Err(err) => {
                if let Err(cleanup) = compose.delete(draft.id).await {
                    tracing::warn!(error = %cleanup, "a reply draft that failed to save could not be removed");
                }
                Err(err)
            }
        }
    }

    /// The writing help on a passage of the composer; returns a preview, changes nothing.
    pub async fn ai_writing_help(
        &self,
        request_id: String,
        passage: String,
        task: AiWritingTask,
    ) -> Result<AiWritingResult, CoreError> {
        let session = self
            .ai_session(&request_id, AiFunction::WritingHelp)
            .await?;
        let result = async {
            let prepared =
                mail_ai::prepare_writing(&session.profile.model, &passage, writing_task(task))
                    .map_err(failure)?;
            let transfer = session.sending(prepared.transfer());
            let result = session
                .request(
                    &transfer,
                    mail_ai::rewrite(
                        session.provider.as_ref(),
                        &prepared,
                        &session.cancel,
                        session.progress(),
                    ),
                )
                .await?;
            Ok(AiWritingResult {
                text: result.text,
                transfer,
            })
        }
        .await;
        session.finish(result)
    }

    /// A question in natural language over `accounts` (`None`: all): question → query of
    /// stage 7 → local candidates → an answer only from the candidates, every statement with
    /// its messages; honestly nothing found when there are none. Candidates of accounts
    /// without cloud permission are left out for a cloud provider (`excluded`).
    pub async fn ai_ask(
        &self,
        request_id: String,
        question: String,
        accounts: Option<Vec<AccountId>>,
    ) -> Result<AiAnswer, CoreError> {
        if question.chars().count() > usize::try_from(MAX_AI_QUESTION_CHARS).unwrap_or(usize::MAX) {
            return Err(CoreError::InvalidArgument(format!(
                "a question has at most {MAX_AI_QUESTION_CHARS} characters"
            )));
        }
        let session = self.ai_session(&request_id, AiFunction::Search).await?;
        let result = self.ask_in(&session, &question, accounts).await;
        session.finish(result)
    }

    async fn ask_in(
        &self,
        session: &Session<'_, C>,
        question: &str,
        accounts: Option<Vec<AccountId>>,
    ) -> Result<AiAnswer, CoreError> {
        let store = self.store_ref()?;
        let accounts = match accounts {
            Some(list) => list,
            None => store.accounts().await?.into_iter().map(|a| a.id).collect(),
        };
        let zone = LocalZone::system();
        let context = self.ai_context(self.ai_language().await, &zone);
        let model = &session.profile.model;
        let prepared = mail_ai::prepare_query(model, question, &context).map_err(failure)?;
        let first = session.sending(prepared.transfer());
        let mut answer = AiAnswer {
            found: false,
            statements: Vec::new(),
            query: String::new(),
            candidates: 0,
            excluded: 0,
            dropped: 0,
            transfers: vec![first.clone()],
        };
        let plan = session
            .request(
                &first,
                mail_ai::send_query(
                    session.provider.as_ref(),
                    &prepared,
                    &context,
                    &session.cancel,
                    session.progress(),
                ),
            )
            .await?;
        answer.query.clone_from(&plan.query);
        if plan.empty || accounts.is_empty() {
            return Ok(answer);
        }
        if session.cancel.is_cancelled() {
            return Err(CoreError::Ai(AiFailure::Cancelled));
        }
        let hits = self
            .search_exactly(&plan.query, &accounts, None, None, MAX_CANDIDATES)
            .await?
            .hits;
        answer.candidates = u32::try_from(hits.len()).unwrap_or(u32::MAX);
        // Permission first, so no body of an excluded account is even loaded.
        let local = session.locality().is_local();
        let mut candidates = Vec::new();
        for hit in hits {
            if !local && !session.cloud_allowed(&hit.account_id) {
                answer.excluded += 1;
                continue;
            }
            if let Some(summary) = store.message_summary(hit.id).await? {
                candidates.push(self.ai_mail_input(&summary).await);
            }
        }
        if candidates.is_empty() {
            return Ok(answer);
        }
        let prepared =
            mail_ai::prepare_answer(model, question, &candidates, Budget::CANDIDATES, &context)
                .map_err(failure)?;
        let second = session.sending(prepared.transfer());
        answer.transfers.push(second.clone());
        let cloud = |account: &AccountId| session.cloud_allowed(account);
        let (outcome, dropped) = session
            .request(
                &second,
                mail_ai::send_answer(
                    session.provider.as_ref(),
                    &prepared,
                    &cloud,
                    &session.cancel,
                    session.progress(),
                ),
            )
            .await?;
        answer.dropped = dropped;
        if let AnswerOutcome::Found(statements) = outcome {
            answer.found = true;
            answer.statements = claims(statements);
        }
        Ok(answer)
    }

    // -----------------------------------------------------------------------------------------
    // Test hooks
    // -----------------------------------------------------------------------------------------

    /// Tests: builds providers with `factory` instead of `mail_ai::build_provider`.
    #[cfg(test)]
    pub(crate) fn set_ai_provider_factory(&self, factory: Arc<ProviderFactory>) {
        *lock(&self.ai().factory) = Some(factory);
    }
}

/// A TLS configuration for validating a profile without connecting.
fn rustls_for_check() -> Result<Arc<rustls::ClientConfig>, CoreError> {
    mail_imap::tls::client_config(None)
        .map_err(|err| CoreError::Internal(format!("TLS configuration: {err}")))
}

/// Keys as they arrive from the UI, wrapped at once.
pub(crate) fn key_secret(key: String) -> Option<Secret<String>> {
    let key = key.trim().to_owned();
    (!key.is_empty()).then(|| Secret::new(key))
}

#[cfg(test)]
mod tests;
