//! The local MCP server (stage 12, decisions 0010 and 0015).
//!
//! Two programs share this crate:
//!
//! - **The helper** `mail-mcp` (binary, [`helper`]): an AI client (Claude Desktop, Claude Code,
//!   Cursor, LM Studio, ...) starts it and speaks MCP with it over stdin/stdout ([`mcp`]). It
//!   forwards `tools/list` and `tools/call` over a local IPC channel ([`ipc`]) to the app and
//!   knows nothing else.
//! - **The host** ([`McpHost`]) inside the app: listens on a Unix domain socket (macOS, 0600) or
//!   a named pipe (Windows) only while MCP is on ([`Endpoint`]), authenticates each helper by its
//!   client key (HMAC challenge-response, both directions), checks the permissions
//!   ([`McpSettings`]) on every request and runs the tools ([`ToolName`]) against [`MailAccess`],
//!   which `AppCore` implements. Every call is reported to an [`AccessLog`].
//!
//! Security properties (stage 12, decision 0010): off until switched on; nothing network-facing;
//! hidden accounts invisible to every tool; six read tools and `create_draft` only (listed only
//! when an account allows drafts); tool descriptions fixed in code; mail content cleaned (no
//! HTML, no image addresses, no invisible characters) and handed over only inside a delimited
//! untrusted-data block ([`output`]); limits per call ([`limits`]); revocation and emergency stop
//! take effect immediately.

pub mod access;
pub mod config;
pub mod endpoint;
mod exec;
pub mod helper;
pub mod host;
pub mod ipc;
pub mod limits;
pub mod mcp;
pub mod output;
pub mod permissions;
pub mod preview;
pub mod tools;

pub use access::{
    AttachmentMeta, BodyParts, DraftCreated, ListFilter, ListQuery, MailAccess, MessageContent,
    NewDraft, SearchQuery, SearchResults, ThreadContent, readable_in,
};
pub use endpoint::{Endpoint, EndpointError, ListenError};
pub use host::{AccessEvent, AccessKind, AccessLog, CallOutcome, HostStatus, McpHost};
pub use ipc::Refusal;
pub use permissions::{
    AccountAccess, ClientId, ClientKey, ClientRecord, ClientToken, Denied, Grant, McpSettings,
    RandomError, TokenError,
};
pub use tools::{ToolName, tools_for};
