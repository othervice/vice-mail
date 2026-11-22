//! Helpers for integration tests against the local test server (`scripts/testmail.sh`). Only
//! with the feature `test-support`; never part of the product. `mail-engine` tests can use
//! them too.
//!
//! - [`TestEnv`]: ports, accounts and the test CA from `testmail/accounts.env`, and ready
//!   [`mail_model::ConnectConfig`]s.
//! - [`Proxy`]: a TCP proxy between client and server that cuts or stalls connections.
//! - [`crate::WireTap`]: records what the adapter writes inside TLS.
//! - [`crate::ImapBackend::raw_command`]: any command through the command layer, for test
//!   setup in read-write mode (it still refuses writes in read-only mode).
//! - [`send_mail`]: SMTP submission with STARTTLS and AUTH, to deliver mail during a test.

mod env;
mod proxy;
mod smtp;

pub use async_imap::imap_proto;
pub use env::{TestAccount, TestEnv};
pub use proxy::{Fault, Proxy};
pub use smtp::send_mail;

/// Test hooks of an [`crate::ImapConnector`].
#[derive(Debug, Clone, Default)]
pub(crate) struct Hooks {
    pub(crate) wire_tap: Option<crate::WireTap>,
    pub(crate) tls_server_name: Option<String>,
}

/// Result of [`crate::ImapBackend::raw_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawReply {
    /// The tagged status was OK.
    pub ok: bool,
    /// Response code and text of the tagged status.
    pub text: String,
}
