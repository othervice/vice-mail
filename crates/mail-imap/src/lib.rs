//! IMAP adapter: [`ImapConnector`] and [`ImapBackend`] implement `MailConnector` and
//! `MailBackend` from `mail-model` (decision 0002). Knows neither database nor UI.
//!
//! - **Transport:** TCP, then TLS (port 993) or STARTTLS (port 143, never continues
//!   unencrypted), certificates checked by the operating system ([`tls`], which also sets up
//!   the HTTPS clients of `mail-engine`), read and write limits on every connection
//!   (`timeout_io`).
//! - **Library use:** async-imap provides the connection, buffer, parser and LOGIN. Everything
//!   after LOGIN runs through the own command layer in `command`, which checks the tagged
//!   status, keeps response codes, records unsolicited changes and enforces the read-only
//!   allowlist before a byte is sent.
//! - **Data:** responses are turned into model types while they are read (`convert`, and
//!   `bodies` for BODYSTRUCTURE and `BODY.PEEK[...]` sections);
//!   mailbox names are decoded from modified UTF-7 ([`mutf7`]); header text is decoded with
//!   mail-parser. Broken messages never panic.
//!
//! - **Writing** (stage 4): [`ImapWriter`] implements `MailWriter` for connections opened with
//!   `Access::ReadWrite` only (`ImapConnector::connect_writer` refuses read-only
//!   configurations before connecting); STORE with `UNCHANGEDSINCE`, MOVE with its COPY/EXPUNGE
//!   fallback, CREATE.
//!
//! - **Sending** (stage 5): [`SmtpSender`] submits a message over SMTP with TLS or STARTTLS
//!   (lettre's protocol layer on the adapter's own TLS streams) and reports accepted, refused
//!   (per recipient) or unclear (connection ended after the data went out).
//!
//! The feature `test-support` adds helpers for integration tests against the local test
//! server: a fault-injecting TCP proxy, a wire tap, raw commands and a small SMTP sender.

mod backend;
mod bodies;
mod command;
mod connector;
mod convert;
mod error;
pub mod mutf7;
mod sasl;
mod search;
mod smtp;
mod timeout_io;
pub mod tls;
mod writer;

#[cfg(any(test, feature = "test-support"))]
mod wire_tap;

#[cfg(feature = "test-support")]
pub mod test_support;

#[cfg(test)]
mod tests;

pub use backend::{ImapBackend, MAX_IDLE, TlsTransport};
pub use command::Transport;
pub use connector::ImapConnector;
pub use smtp::SmtpSender;
#[cfg(feature = "test-support")]
pub use wire_tap::WireTap;
pub use writer::ImapWriter;
