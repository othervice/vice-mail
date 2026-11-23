//! The AI settings: one JSON value in `setting` (`global`, `ai.settings`). A device setting:
//! the key is not registered for the settings sync (`settings_sync::record`), so neither the
//! profiles nor the consent leave this device; keys are in the keychain only.

use mail_ai::{ProviderConfig, ProviderKind};
use mail_model::{
    AccountId, AiAccountCloud, AiFailure, AiFunction, AiFunctionChoice, AiKeyChange, AiLocality,
    AiPreset, AiProfileInfo, AiProfileInput, AiProviderKind, AiSettingsInfo, CoreError,
    MAX_AI_PROFILE_NAME_CHARS, MAX_AI_PROFILES, OpenReplySettings, Secret,
};
use mail_store::{GLOBAL_SCOPE, Store, StoredAccount};
use serde::{Deserialize, Serialize};

use super::locality;

const KEY: &str = "ai.settings";

/// One provider profile as stored (without its key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredProfile {
    pub id: String,
    pub name: String,
    pub kind: AiProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "yes")]
    pub structured_output: bool,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    /// A key is in the keychain under `CredentialRef::for_ai_profile(id)`.
    #[serde(default)]
    pub has_key: bool,
    /// Consent to send data to `consent_host` (Unix ms).
    #[serde(default)]
    pub consent_at: Option<i64>,
    #[serde(default)]
    pub consent_host: Option<String>,
}

const fn yes() -> bool {
    true
}

impl StoredProfile {
    pub(crate) fn host(&self) -> String {
        host_of(&self.base_url)
    }

    pub(crate) fn locality(&self) -> AiLocality {
        mail_ai::locality_of(&self.base_url).map_or(AiLocality::Internet, locality)
    }

    /// Consent was given for the host the profile points at now.
    pub(crate) fn consented(&self) -> bool {
        self.consent_at.is_some() && self.consent_host.as_deref() == Some(self.host().as_str())
    }

    pub(crate) fn config(&self, key: Option<Secret<String>>) -> ProviderConfig {
        let kind = match self.kind {
            AiProviderKind::OpenaiCompatible => ProviderKind::OpenAiCompatible,
            AiProviderKind::Anthropic => ProviderKind::AnthropicMessages,
        };
        let mut config = ProviderConfig::new(kind, self.base_url.clone(), key);
        config.structured_output = self.structured_output;
        config.max_output_tokens = self.max_output_tokens;
        config
    }

    fn info(&self) -> AiProfileInfo {
        let locality = self.locality();
        let local = locality.is_local();
        AiProfileInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            kind: self.kind,
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            structured_output: self.structured_output,
            max_output_tokens: self.max_output_tokens,
            has_key: self.has_key,
            host: self.host(),
            locality,
            consent_at: if local || !self.consented() {
                None
            } else {
                self.consent_at
            },
            needs_consent: !local && !self.consented(),
        }
    }
}

/// Host and port of a base URL (the default port of the scheme when none is given), as the
/// settings and the consent name it.
pub(crate) fn host_of(base_url: &str) -> String {
    match url::Url::parse(base_url.trim()) {
        Ok(url) => {
            let host = url.host_str().unwrap_or_default();
            match url.port_or_known_default() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            }
        }
        Err(_) => String::new(),
    }
}

/// Everything stored.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct StoredAi {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub profiles: Vec<StoredProfile>,
    /// Function name → profile id; missing = off.
    #[serde(default)]
    pub functions: Vec<(String, String)>,
    #[serde(default)]
    pub cloud_accounts: Vec<AccountId>,
    #[serde(default)]
    pub open_replies: OpenReplySettings,
}

/// What happens to the key when a profile is saved.
pub(crate) enum KeyAction {
    Keep,
    Remove,
    Set(Secret<String>),
}

fn off() -> CoreError {
    CoreError::Ai(AiFailure::Off)
}

fn profile_missing(id: &str) -> CoreError {
    CoreError::NotFound(format!("AI profile {id}"))
}

impl StoredAi {
    fn profile(&self, id: &str) -> Option<&StoredProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    fn profile_of(&self, function: AiFunction) -> Option<&str> {
        self.functions
            .iter()
            .find(|(name, _)| name == function.as_str())
            .map(|(_, id)| id.as_str())
    }

    /// The profile `function` uses, if AI is on and it has a model; `ai_off` otherwise.
    pub(crate) fn usable_profile(&self, function: AiFunction) -> Result<&StoredProfile, CoreError> {
        if !self.enabled {
            return Err(off());
        }
        let profile = self
            .profile_of(function)
            .and_then(|id| self.profile(id))
            .ok_or_else(off)?;
        if profile.model.trim().is_empty() {
            return Err(off());
        }
        Ok(profile)
    }

    /// A profile for the model list or the connection test: AI must be on.
    pub(crate) fn enabled_profile(&self, id: &str) -> Result<&StoredProfile, CoreError> {
        if !self.enabled {
            return Err(off());
        }
        self.profile(id).ok_or_else(|| profile_missing(id))
    }

    pub(crate) fn info(&self, accounts: &[StoredAccount]) -> AiSettingsInfo {
        AiSettingsInfo {
            enabled: self.enabled,
            profiles: self.profiles.iter().map(StoredProfile::info).collect(),
            functions: AiFunction::ALL
                .into_iter()
                .map(|function| AiFunctionChoice {
                    function,
                    profile_id: self
                        .profile_of(function)
                        .filter(|id| self.profile(id).is_some())
                        .map(str::to_owned),
                })
                .collect(),
            accounts: accounts
                .iter()
                .map(|account| AiAccountCloud {
                    account_id: account.id,
                    cloud_allowed: self.cloud_accounts.contains(&account.id),
                })
                .collect(),
            presets: mail_ai::PRESETS
                .iter()
                .map(|preset| AiPreset {
                    name: preset.name.to_owned(),
                    kind: match preset.kind {
                        ProviderKind::OpenAiCompatible => AiProviderKind::OpenaiCompatible,
                        ProviderKind::AnthropicMessages => AiProviderKind::Anthropic,
                    },
                    base_url: preset.base_url.to_owned(),
                    needs_key: preset.needs_key,
                    locality: mail_ai::locality_of(preset.base_url)
                        .map_or(AiLocality::Internet, locality),
                })
                .collect(),
            open_replies: self.open_replies,
        }
    }

    pub(crate) fn remove_profile(&mut self, id: &str) -> Result<(), CoreError> {
        if self.profile(id).is_none() {
            return Err(profile_missing(id));
        }
        self.profiles.retain(|p| p.id != id);
        self.functions.retain(|(_, profile)| profile != id);
        Ok(())
    }

    pub(crate) fn set_function(
        &mut self,
        function: AiFunction,
        profile: Option<String>,
    ) -> Result<(), CoreError> {
        if let Some(id) = &profile
            && self.profile(id).is_none()
        {
            return Err(profile_missing(id));
        }
        self.functions.retain(|(name, _)| name != function.as_str());
        if let Some(id) = profile {
            self.functions.push((function.as_str().to_owned(), id));
        }
        Ok(())
    }

    pub(crate) fn set_consent(
        &mut self,
        id: &str,
        granted: bool,
        now: i64,
    ) -> Result<(), CoreError> {
        let profile = self
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| profile_missing(id))?;
        if granted {
            profile.consent_at = Some(now);
            profile.consent_host = Some(profile.host());
        } else {
            profile.consent_at = None;
            profile.consent_host = None;
        }
        Ok(())
    }
}

fn check_text(what: &str, text: &str, max: usize, empty_ok: bool) -> Result<String, CoreError> {
    let text = text.trim();
    let chars = text.chars().count();
    if (!empty_ok && chars == 0) || chars > max || text.chars().any(char::is_control) {
        return Err(CoreError::InvalidArgument(format!(
            "{what} has {} to {max} characters and no control characters",
            usize::from(!empty_ok)
        )));
    }
    Ok(text.to_owned())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Applies a profile from the UI to `stored`: validated, a new id for a new profile, consent
/// withdrawn when the host changes. Returns the profile and what to do with its key.
pub(crate) fn apply_input(
    stored: &mut StoredAi,
    input: AiProfileInput,
    _now: i64,
) -> Result<(StoredProfile, KeyAction), CoreError> {
    let name = check_text(
        "a profile name",
        &input.name,
        usize::try_from(MAX_AI_PROFILE_NAME_CHARS).unwrap_or(usize::MAX),
        false,
    )?;
    let model = check_text("a model name", &input.model, 200, true)?;
    let base_url = input.base_url.trim().to_owned();
    mail_ai::locality_of(&base_url).map_err(|err| CoreError::InvalidArgument(err.to_string()))?;
    if input.max_output_tokens == Some(0) {
        return Err(CoreError::InvalidArgument(
            "the output limit is at least 1 token".into(),
        ));
    }
    let key = match input.key {
        AiKeyChange::Keep => KeyAction::Keep,
        AiKeyChange::Remove => KeyAction::Remove,
        AiKeyChange::Set { key } => match super::key_secret(key) {
            Some(secret) => KeyAction::Set(secret),
            None => KeyAction::Remove,
        },
    };
    let index = match &input.id {
        Some(id) => Some(
            stored
                .profiles
                .iter()
                .position(|p| &p.id == id)
                .ok_or_else(|| profile_missing(id))?,
        ),
        None => {
            if stored.profiles.len() >= usize::try_from(MAX_AI_PROFILES).unwrap_or(usize::MAX) {
                return Err(CoreError::InvalidArgument(format!(
                    "at most {MAX_AI_PROFILES} AI profiles"
                )));
            }
            None
        }
    };
    let mut profile = match index {
        Some(i) => stored.profiles[i].clone(),
        None => StoredProfile {
            id: new_id(),
            name: String::new(),
            kind: input.kind,
            base_url: String::new(),
            model: String::new(),
            structured_output: true,
            max_output_tokens: None,
            has_key: false,
            consent_at: None,
            consent_host: None,
        },
    };
    profile.name = name;
    profile.kind = input.kind;
    profile.base_url = base_url;
    profile.model = model;
    profile.structured_output = input.structured_output;
    profile.max_output_tokens = input.max_output_tokens;
    match &key {
        KeyAction::Keep => {}
        KeyAction::Remove => profile.has_key = false,
        KeyAction::Set(_) => profile.has_key = true,
    }
    if !profile.consented() {
        profile.consent_at = None;
        profile.consent_host = None;
    }
    match index {
        Some(i) => stored.profiles[i] = profile.clone(),
        None => stored.profiles.push(profile.clone()),
    }
    Ok((profile, key))
}

pub(crate) async fn load(store: &Store) -> Result<StoredAi, CoreError> {
    let settings = store.settings(GLOBAL_SCOPE).await?;
    let Some(setting) = settings.into_iter().find(|s| s.key == KEY) else {
        return Ok(StoredAi::default());
    };
    match serde_json::from_str(&setting.value) {
        Ok(stored) => Ok(stored),
        Err(err) => {
            // Unreadable settings are AI off, never a guess.
            tracing::error!(error = %err, "stored AI settings are not readable; AI stays off");
            Ok(StoredAi::default())
        }
    }
}

pub(crate) async fn save(store: &Store, stored: &StoredAi) -> Result<(), CoreError> {
    let json = serde_json::to_string(stored)
        .map_err(|err| CoreError::Internal(format!("AI settings: {err}")))?;
    store
        .put_settings(GLOBAL_SCOPE, vec![(KEY.to_owned(), json)])
        .await?;
    Ok(())
}
