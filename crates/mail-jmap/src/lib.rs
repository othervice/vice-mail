//! JMAP adapter (stage 14, block D, decision 0041): [`JmapConnector`] implements
//! `MailConnector` and `WriteConnector`, [`JmapSender`] implements `MailSender`, behind the
//! IMAP-shaped traits of `mail-model`. Knows neither database nor UI.
//!
//! - **Transport:** HTTPS only (RFC 8620 over HTTP/1.1, reqwest), with the app's rustls
//!   configuration, which the engine hands in ([`TlsConfigFn`], built by `mail_imap::tls`): no
//!   proxy, no cookies, no redirects except for the session resource (same host or its
//!   subdomains, at most three). The session resource is
//!   `https://<host>[:<port>]/.well-known/jmap`; the API, download, upload and event source
//!   addresses are the ones it names, and must be HTTPS too.
//! - **Login:** HTTP Basic with the account's user name and the password from the keychain,
//!   or `Authorization: Bearer` with an API token (Fastmail) when the configuration says
//!   `LoginAuth::Bearer`; one place (`client::Auth`).
//! - **UIDs:** JMAP has string ids and state strings; the engine wants UIDs, UIDVALIDITY and
//!   mod-sequences. The adapter numbers the messages of each mailbox itself (`map`) and keeps
//!   that numbering in the engine's store ([`mail_model::UidMapStore`]). Changes come from
//!   `Email/changes`; the emulated HIGHESTMODSEQ is a counter that grows with each new state.
//! - **Read-only protection:** a read-only connection only sends the methods on an allowlist
//!   (`*/get`, `*/query`, `*/changes`, ...). `Email/set`, `Email/import`, `Mailbox/set`,
//!   uploads and `EmailSubmission/set` fail with `BackendError::ReadOnly` before a byte is sent.
//!   Writers come only from `connect_writer`, which refuses a read-only configuration.
//! - **Push:** `idle` reads the server's event source (RFC 8620 section 7.3); without one the
//!   capabilities say "no IDLE" and the engine polls.
//! - **Settings sync folder:** [`JmapSyncFolder`] keeps the encrypted records as messages in
//!   one mailbox, on a read-only connection whose only writes are into and out of that mailbox.
//! - **Sending:** the message is uploaded, imported into Drafts and submitted with
//!   `EmailSubmission/set`, which destroys the imported copy on success. The engine files its
//!   Sent copy as for IMAP.

mod backend;
mod client;
mod connector;
mod email;
mod error;
mod mailbox;
mod map;
mod push;
mod sender;
mod session;
mod sync_folder;
mod writer;

#[cfg(test)]
mod tests;

pub use backend::JmapBackend;
pub use client::MethodLog;
pub use connector::{JmapConnector, TlsConfigFn};
pub use sender::JmapSender;
pub use session::{SessionProbe, probe};
pub use sync_folder::JmapSyncFolder;
pub use writer::JmapWriter;

/// Capability URIs (RFC 8620, RFC 8621).
pub mod capability {
    pub const CORE: &str = "urn:ietf:params:jmap:core";
    pub const MAIL: &str = "urn:ietf:params:jmap:mail";
    pub const SUBMISSION: &str = "urn:ietf:params:jmap:submission";
    /// Stage 20 (decision 0060): quotas (RFC 9425).
    pub const QUOTA: &str = "urn:ietf:params:jmap:quota";
}
