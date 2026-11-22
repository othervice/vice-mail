//! IMAP adapter: [`ImapConnector`] and [`ImapBackend`] implement `MailConnector` and
//! `MailBackend` from `mail-model` (decision 0002). Knows neither database nor UI.
//!
//! - **Transport:** TCP, then TLS (port 993) or STARTTLS (port 143, never continues
//!   unencrypted), certificates checked by the operating system (`tls`), read and write
//!   limits on every connection (`timeout_io`).
//! - **Library use:** async-imap provides the connection, buffer, parser and LOGIN. Everything
//!   after LOGIN runs through the own command layer in `command`, which checks the tagged
//!   status, keeps response codes, records unsolicited changes and enforces the read-only
//!   allowlist before a byte is sent.
//! - **Data:** responses are turned into model types while they are read (`convert`);
//!   mailbox names are decoded from modified UTF-7 ([`mutf7`]); header text is decoded with
//!   mail-parser. Broken messages never panic.
//!
//! The feature `test-support` adds helpers for integration tests against the local test
//! server: a fault-injecting TCP proxy, a wire tap, raw commands and a small SMTP sender.

mod backend;
mod command;
mod connector;
mod convert;
mod error;
pub mod mutf7;
mod timeout_io;
mod tls;

#[cfg(any(test, feature = "test-support"))]
mod wire_tap;

#[cfg(feature = "test-support")]
pub mod test_support;

#[cfg(test)]
mod tests;

pub use backend::{ImapBackend, MAX_IDLE, TlsTransport};
pub use command::Transport;
pub use connector::ImapConnector;
#[cfg(feature = "test-support")]
pub use wire_tap::WireTap;
