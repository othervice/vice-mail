//! The commands of the sign-in on [`AppCore`]: which providers this build offers, adding an
//! account that signs in with one, signing in again, and cancelling the browser step.

use mail_model::{
    Account, AccountId, AccountSettings, AccountStatus, CoreError, CoreEvent, LoginAuth,
    OAuthAvailability, OAuthError, OAuthPageTexts, OAuthProvider,
};
use mail_store::NewAccount;

use super::{SignInGrant, store};
use crate::core::{AppCore, Connector};
use crate::credentials::CredentialRef;

impl<C: Connector> AppCore<C> {
    /// Which providers this build can sign in with (a client ID was configured at build time).
    pub fn oauth_availability(&self) -> OAuthAvailability {
        self.credentials_ref()
            .oauth()
            .map(|service| service.availability())
            .unwrap_or_default()
    }

    /// Ends the running sign-in; its command fails with `oauth_cancelled`. `false` if none ran.
    pub fn cancel_oauth_sign_in(&self) -> bool {
        self.credentials_ref()
            .oauth()
            .is_some_and(|service| service.cancel())
    }

    /// Adds an account that signs in with `provider`: checks the settings (the servers must be
    /// the provider's), signs in in the system browser, logs in to IMAP with the access token,
    /// stores the refresh token in the keychain and the account read-only
    /// (`writes_enabled = false`), then starts its sync. Emits `accountsChanged`.
    pub async fn add_oauth_account(
        &self,
        settings: AccountSettings,
        provider: OAuthProvider,
        texts: OAuthPageTexts,
    ) -> Result<Account, CoreError> {
        let settings = settings.validated()?;
        let service = self
            .credentials_ref()
            .oauth()
            .ok_or(OAuthError::Unavailable(provider))?
            .clone();
        let profile = service.profile(provider)?;
        for (server, imap) in [(&settings.imap, true), (&settings.smtp, false)] {
            if !profile.allows_host(&server.host, imap) {
                return Err(CoreError::InvalidArgument(format!(
                    "{} is not a {} server; sign-in with {} works only with its own servers",
                    server.host,
                    if imap { "IMAP" } else { "SMTP" },
                    provider.name()
                )));
            }
        }
        let store = self.store_ref()?;
        if store
            .accounts()
            .await?
            .iter()
            .any(|a| a.email.eq_ignore_ascii_case(&settings.email))
        {
            return Err(CoreError::AccountExists(settings.email));
        }
        let grant = service.sign_in(provider, &settings.email, texts).await?;
        self.check_login_with(
            &settings.imap,
            grant.access_token.clone(),
            LoginAuth::Bearer,
        )
        .await?;

        let _admin = self.admin_lock().await;
        let id = AccountId::from_uuid(uuid::Uuid::new_v4());
        let key = CredentialRef::for_oauth_account(provider, id);
        self.keep_grant(&key, &grant).await?;
        let new = NewAccount {
            id,
            email: settings.email,
            display_name: settings.display_name,
            imap: settings.imap,
            smtp: settings.smtp,
            credential_ref: key.to_string(),
        };
        let stored = match store.insert_account(new).await {
            Ok(stored) => stored,
            Err(err) => {
                if let Err(cleanup) = self.credentials_ref().delete(&key).await {
                    tracing::error!(account = %id, error = %cleanup, "keychain entry of a failed account insert could not be removed");
                }
                return Err(err.into());
            }
        };
        tracing::info!(account = %id, provider = provider.as_str(), "account added (sign-in)");
        self.spawn_sync(stored.clone(), None);
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(stored.into_account(AccountStatus::connecting()))
    }

    /// Signs in again for an account that signs in with its provider (its permission expired
    /// or was revoked, or the account came from another device through the settings sync):
    /// browser, IMAP login with the new token, new refresh token in the keychain, reconnect.
    pub async fn sign_in_again(
        &self,
        id: AccountId,
        texts: OAuthPageTexts,
    ) -> Result<(), CoreError> {
        let store = self.store_ref()?;
        let account = store
            .account(id)
            .await?
            .ok_or_else(|| CoreError::NotFound(format!("account {id}")))?;
        let key = CredentialRef::parse(&account.credential_ref)?;
        let provider = key.oauth_provider().ok_or_else(|| {
            CoreError::InvalidArgument("this account uses a password, not a sign-in".into())
        })?;
        let service = self
            .credentials_ref()
            .oauth()
            .ok_or(OAuthError::Unavailable(provider))?
            .clone();
        let profile = service.profile(provider)?;
        if !profile.allows_host(&account.imap.host, true) {
            return Err(OAuthError::Failed(format!(
                "{} is not a {} server",
                account.imap.host,
                provider.name()
            ))
            .into());
        }
        let grant = service.sign_in(provider, &account.email, texts).await?;
        self.check_login_with(&account.imap, grant.access_token.clone(), LoginAuth::Bearer)
            .await?;
        {
            let _admin = self.admin_lock().await;
            self.keep_grant(&key, &grant).await?;
        }
        if let Ok(reading) = self.reading_ref() {
            reading.credentials_changed(id);
        }
        if let Ok(outbox) = self.outbox_ref() {
            outbox.wake(id);
        }
        // Stage 11: an account taken over from another device has its sign-in now.
        if let Err(err) = store.clear_sync_adopted(id).await {
            tracing::warn!(account = %id, error = %err, "adopted mark not cleared");
        }
        if self.retry_account(id).await.is_err() {
            self.spawn_sync(account, None);
        }
        tracing::info!(account = %id, provider = provider.as_str(), "signed in again");
        self.emit_event(CoreEvent::AccountsChanged);
        Ok(())
    }

    /// The refresh token to the keychain, the access token to memory.
    async fn keep_grant(&self, key: &CredentialRef, grant: &SignInGrant) -> Result<(), CoreError> {
        let credentials = self.credentials_ref();
        store::save(credentials, key, &grant.refresh_token).await?;
        if let Some(service) = credentials.oauth() {
            service
                .tokens
                .put(key, grant.access_token.clone(), grant.renew_at)
                .await;
        }
        Ok(())
    }
}
