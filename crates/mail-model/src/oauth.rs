//! Stage 14, blocks B and C (decision 0042): sign-in with Microsoft and Google (OAuth 2.0 with
//! PKCE in the system browser) for IMAP and SMTP. The types here tell the adapter how to log in.

/// How a connection proves who it is. With [`LoginAuth::Bearer`] the `password` field of
/// [`crate::ConnectConfig`] holds an OAuth 2.0 access token, sent with SASL `XOAUTH2` or
/// `OAUTHBEARER` (RFC 7628), never with LOGIN or PLAIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoginAuth {
    #[default]
    Password,
    Bearer,
}
