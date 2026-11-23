//! A provider profile from the settings, and building the provider for it.

use mail_model::Secret;

use crate::anthropic::AnthropicMessages;
use crate::openai::OpenAiCompatible;
use crate::provider::{AiProvider, ProviderKind};
use crate::{AiError, HttpLimits};

/// One profile. The key comes from the system keychain (the engine reads it; it is never
/// stored in settings or the database) and stays a [`Secret`]: `Debug` shows `[redacted]`.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// OpenAI-compatible: including `/v1` (`http://localhost:11434/v1`). Anthropic: the API
    /// root ([`crate::anthropic::ANTHROPIC_BASE_URL`]); a trailing `/v1` is accepted too.
    pub base_url: String,
    /// `None` for local servers without authentication.
    pub api_key: Option<Secret<String>>,
    /// Ask for the JSON schema of the answer (OpenAI `response_format` with `json_schema`,
    /// Anthropic `output_config.format`). Turn off for servers or models that reject it; the
    /// answer is parsed robustly either way.
    pub structured_output: bool,
    /// Output token limit. OpenAI-compatible: sent as `max_tokens` only when set (servers
    /// differ in what they accept). Anthropic: required, default
    /// [`crate::anthropic::DEFAULT_MAX_TOKENS`].
    pub max_output_tokens: Option<u32>,
    /// Anthropic only: on a classifier refusal, let the API retry on the model Anthropic
    /// recommends (server-side fallback, `fallbacks: "default"`, beta). Only sent to
    /// `api.anthropic.com` and for models that support it; the data goes to the same provider.
    pub refusal_fallback: bool,
    pub limits: HttpLimits,
}

impl ProviderConfig {
    pub fn new(
        kind: ProviderKind,
        base_url: impl Into<String>,
        api_key: Option<Secret<String>>,
    ) -> Self {
        Self {
            kind,
            base_url: base_url.into(),
            api_key: api_key.filter(|key| !key.expose_secret().trim().is_empty()),
            structured_output: true,
            max_output_tokens: None,
            refusal_fallback: true,
            limits: HttpLimits::default(),
        }
    }
}

/// Builds the provider. `tls` is the app's client configuration
/// (`mail_imap::tls::client_config(None)`): the system's certificate check and the one crypto
/// provider of the app.
pub fn build_provider(
    config: ProviderConfig,
    tls: &rustls::ClientConfig,
) -> Result<Box<dyn AiProvider>, AiError> {
    Ok(match config.kind {
        ProviderKind::OpenAiCompatible => Box::new(OpenAiCompatible::new(config, tls)?),
        ProviderKind::AnthropicMessages => Box::new(AnthropicMessages::new(config, tls)?),
    })
}

/// Known endpoints for the settings ("Vorlagen"). The user can enter any other base URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    /// The service needs a key.
    pub needs_key: bool,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Ollama",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:11434/v1",
        needs_key: false,
    },
    Preset {
        name: "LM Studio",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:1234/v1",
        needs_key: false,
    },
    Preset {
        name: "llama.cpp server",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "http://localhost:8080/v1",
        needs_key: false,
    },
    Preset {
        name: "OpenRouter",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://openrouter.ai/api/v1",
        needs_key: true,
    },
    Preset {
        name: "OpenAI",
        kind: ProviderKind::OpenAiCompatible,
        base_url: "https://api.openai.com/v1",
        needs_key: true,
    },
    Preset {
        name: "Anthropic",
        kind: ProviderKind::AnthropicMessages,
        base_url: crate::anthropic::ANTHROPIC_BASE_URL,
        needs_key: true,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_never_show_in_debug_output() {
        let key = "sk-ant-api03-very-secret-value";
        let config = ProviderConfig::new(
            ProviderKind::AnthropicMessages,
            "https://api.anthropic.com",
            Some(Secret::new(key.to_owned())),
        );
        let text = format!("{config:?} {config:#?}");
        assert!(!text.contains("very-secret"), "{text}");
        assert!(text.contains("[redacted]"));
        let tls = crate::test_tls();
        let anthropic = AnthropicMessages::new(config.clone(), &tls).expect("provider");
        let mut openai_config = config.clone();
        openai_config.kind = ProviderKind::OpenAiCompatible;
        openai_config.base_url = "https://api.openai.com/v1".into();
        let openai = OpenAiCompatible::new(openai_config, &tls).expect("provider");
        let text = format!("{anthropic:?} {anthropic:#?} {openai:?} {openai:#?}");
        assert!(!text.contains("very-secret"), "{text}");
        let provider = build_provider(config, &tls).expect("provider");
        assert_eq!(provider.info().host, "api.anthropic.com");
    }

    #[test]
    fn empty_keys_are_no_keys_and_presets_are_valid() {
        let config = ProviderConfig::new(
            ProviderKind::OpenAiCompatible,
            "http://localhost:11434/v1",
            Some(Secret::new("  ".to_owned())),
        );
        assert!(config.api_key.is_none());
        let tls = crate::test_tls();
        for preset in PRESETS {
            let config = ProviderConfig::new(preset.kind, preset.base_url, None);
            let provider = build_provider(config, &tls).expect(preset.name);
            assert_eq!(
                provider.info().locality.is_local(),
                !preset.needs_key,
                "{}",
                preset.name
            );
        }
    }
}
