//! Access tokens in memory, one lock per account: a connection gets a token that stays valid
//! for a while yet, otherwise the refresh token from the keychain buys a new one (once, even if
//! several connections ask at the same time). A login the server rejected marks the token
//! stale, so the next connection refreshes even before the token's own end.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use mail_model::{CoreError, OAuthError, OAuthProvider, Secret};

use super::{OAuthService, store, token_http};
use crate::credentials::{CredentialRef, Credentials};

/// A token is renewed this long before its end at the latest.
const MARGIN_MS: i64 = 5 * 60 * 1000;

#[derive(Default)]
struct Slot {
    /// The access token and when to renew it (Unix milliseconds).
    cached: tokio::sync::Mutex<Option<(Secret<String>, i64)>>,
    stale: AtomicBool,
}

/// The access tokens of all accounts, by keychain reference.
#[derive(Default)]
pub(crate) struct TokenCache {
    slots: Mutex<HashMap<String, Arc<Slot>>>,
}

impl TokenCache {
    fn slot(&self, key: &CredentialRef) -> Arc<Slot> {
        Arc::clone(
            self.slots
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entry(key.as_str().to_owned())
                .or_default(),
        )
    }

    /// A server rejected the current token: the next login refreshes.
    pub(crate) fn reject(&self, key: &CredentialRef) {
        if let Some(slot) = self
            .slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key.as_str())
        {
            slot.stale.store(true, Ordering::SeqCst);
        }
    }

    /// Forgets the account's token (signed in again, or removed).
    pub(crate) fn forget(&self, key: &CredentialRef) {
        self.slots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key.as_str());
    }

    /// Keeps a token that just came from a sign-in, so the first connection uses it.
    pub(crate) async fn put(&self, key: &CredentialRef, token: Secret<String>, renew_at: i64) {
        let slot = self.slot(key);
        slot.stale.store(false, Ordering::SeqCst);
        *slot.cached.lock().await = Some((token, renew_at));
    }
}

/// When to renew a token that is valid for `lifetime_secs` from `now`: five minutes before its
/// end, or halfway for short-lived tokens.
pub(crate) fn renew_at(now: i64, lifetime_secs: u64) -> i64 {
    let lifetime = i64::try_from(lifetime_secs.saturating_mul(1000)).unwrap_or(i64::MAX / 2);
    now.saturating_add((lifetime - MARGIN_MS).max(lifetime / 2))
}

/// A valid access token for the account behind `key`.
pub(crate) async fn access_token(
    service: &OAuthService,
    credentials: &Credentials,
    key: &CredentialRef,
    provider: OAuthProvider,
) -> Result<Secret<String>, CoreError> {
    let profile = service.profile(provider)?;
    let slot = service.tokens.slot(key);
    let mut cached = slot.cached.lock().await;
    let stale = slot.stale.swap(false, Ordering::SeqCst);
    let now = service.now_ms();
    if !stale
        && let Some((token, renew)) = cached.as_ref()
        && now < *renew
    {
        return Ok(token.clone());
    }
    let result = async {
        let refresh_token = store::load(credentials, key).await?.ok_or_else(|| {
            OAuthError::SignInRequired("this device has no sign-in for the account".into())
        })?;
        let response = token_http::refresh(profile, &refresh_token).await?;
        if let Some(new) = &response.refresh_token
            && new.expose_secret() != refresh_token.expose_secret()
            && let Err(err) = store::save(credentials, key, new).await
        {
            // The old refresh token usually stays valid for a while; the next refresh retries.
            tracing::warn!(code = ?err.code(), error = %err, "the renewed refresh token could not be stored");
        }
        Ok::<_, CoreError>(response)
    }
    .await;
    match result {
        Ok(response) => {
            tracing::debug!(provider = provider.as_str(), "access token renewed");
            let renew = renew_at(service.now_ms(), response.expires_in_secs);
            *cached = Some((response.access_token.clone(), renew));
            Ok(response.access_token)
        }
        Err(err) => {
            if matches!(err, CoreError::OAuth(OAuthError::SignInRequired(_))) {
                *cached = None;
            } else if stale {
                slot.stale.store(true, Ordering::SeqCst);
            }
            tracing::warn!(provider = provider.as_str(), code = ?err.code(), "access token not renewed");
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_renewed_before_their_end() {
        assert_eq!(renew_at(0, 3600), 3_300_000);
        assert_eq!(renew_at(1000, 60), 31_000);
        assert_eq!(renew_at(0, 0), 0);
        assert!(renew_at(0, u64::MAX) > 0);
    }
}
