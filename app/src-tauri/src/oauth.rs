//! Stage 14 (decision 0042): the sign-in providers of this build. Client IDs are build
//! configuration, not secrets: set when the app is built, empty in a build without them, in which
//! case the setup says that this build cannot sign in with that provider.
//!
//! - `MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID`: application (client) ID of an app registered in
//!   Microsoft Entra as a public client ("Mobile and desktop applications", redirect URI
//!   `http://localhost`), with the delegated permissions `IMAP.AccessAsUser.All`, `SMTP.Send`
//!   and `offline_access`. No client secret.
//! - `MAIL_APP_OAUTH_GOOGLE_CLIENT_ID` and `MAIL_APP_OAUTH_GOOGLE_CLIENT_SECRET`: an OAuth client
//!   of type "Desktop app" in a Google Cloud project. Google issues a "secret" with it and states
//!   that it cannot be kept confidential in an installed app; its token endpoint still asks for
//!   it. It is treated as public configuration, like the client ID (decision 0042).

use mail_engine::OAuthConfig;

pub fn config() -> OAuthConfig {
    OAuthConfig::from_build(
        option_env!("MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID"),
        option_env!("MAIL_APP_OAUTH_GOOGLE_CLIENT_ID"),
        option_env!("MAIL_APP_OAUTH_GOOGLE_CLIENT_SECRET"),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_build_without_client_ids_offers_no_sign_in() {
        // The check builds have no client IDs; release builds set them in the environment.
        let availability = super::config().availability();
        assert_eq!(
            availability.microsoft,
            option_env!("MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID").is_some_and(|v| !v.trim().is_empty())
        );
    }
}
