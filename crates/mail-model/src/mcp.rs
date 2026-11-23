//! AI access over MCP (stage 12, decisions 0010, 0015 and 0021) as the UI sees it: the master
//! switch, what AI clients may do with each account, the registered clients, the setup texts,
//! the access log ("Zugriffe") and the state of the local endpoint.
//!
//! The MCP server itself lives in `mail-mcp`; these are only the types that cross the bridge.
//! Nothing here carries mail content: the access log holds metadata only (client, tool, time,
//! number of messages).

use std::fmt;

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::AccountId;

/// Most entries one `listMcpAccess` page returns.
pub const MAX_MCP_ACCESS_PAGE: u32 = 500;
/// The access log keeps entries this many days ...
pub const MCP_ACCESS_RETENTION_DAYS: u32 = 30;
/// ... and at most this many entries (the oldest go first).
pub const MCP_ACCESS_MAX_ENTRIES: u32 = 10_000;
/// Longest client name in characters ("Claude Desktop", "Cursor at work").
pub const MAX_MCP_CLIENT_NAME_CHARS: u32 = 100;
/// Most clients that can be registered at the same time (revoked ones do not count).
pub const MAX_MCP_CLIENTS: u32 = 50;

/// What AI clients may do with one account. Every account starts `hidden`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum McpAccountAccess {
    /// Invisible to every tool, also to the search.
    #[default]
    Hidden,
    /// Listed, searchable, readable (bodies are read with `BODY.PEEK`: nothing becomes read).
    Read,
    /// Also "create draft". Without writes on for the account the draft stays in the app on
    /// this device (`localOnly`).
    ReadAndDrafts,
}

/// The access level of one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpAccountShare {
    pub account_id: AccountId,
    pub access: McpAccountAccess,
}

/// A registered AI client. Its key lives in the system keychain; the app shows the token only
/// once, right after creating the client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpClientInfo {
    /// 32 lowercase hex digits; not secret (it names the client in the access log).
    pub client_id: String,
    /// Chosen by the user.
    pub name: String,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub created_at: i64,
    /// Set once revoked: the token no longer works, the key is gone from the keychain.
    #[specta(type = Option<Number>)]
    pub revoked_at: Option<i64>,
    /// The last tool call or connection attempt of this client.
    #[specta(type = Option<Number>)]
    pub last_used_at: Option<i64>,
}

/// What the local endpoint is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum McpHostState {
    /// MCP is off: no socket or pipe exists, nothing can connect.
    Stopped,
    /// Waiting for the helper program of an AI client.
    Listening,
    /// MCP is on, but the endpoint could not be opened (`McpStatus::error` says why). Tried
    /// again at the next settings change.
    Failed,
}

/// The helper program `mail-mcp` that AI clients start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpHelperInfo {
    /// Absolute path, as the client configurations contain it.
    pub path: Option<String>,
    /// The file exists.
    pub found: bool,
    /// The app runs from a place that changes (a disk image, macOS App Translocation): the
    /// path in a client configuration would stop working. The UI asks to move the app first.
    pub unstable_location: bool,
}

/// State of AI access, for the settings page and `mcpStatusChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    /// The master switch.
    pub enabled: bool,
    pub state: McpHostState,
    /// Where the app listens (socket path on macOS, pipe name on Windows); null until known.
    pub endpoint: Option<String>,
    /// Why the endpoint could not be opened (`failed`); technical text from the app, never
    /// from a mail.
    pub error: Option<String>,
    pub helper: McpHelperInfo,
}

/// Everything the settings page "AI over MCP" shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpSettingsInfo {
    pub enabled: bool,
    /// Every account, oldest first, with its access level (`hidden` when never shared).
    pub accounts: Vec<McpAccountShare>,
    /// Registered clients, newest first; revoked ones stay listed with `revokedAt`.
    pub clients: Vec<McpClientInfo>,
    pub status: McpStatus,
}

/// The AI clients with a ready-made configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum McpClientKind {
    ClaudeDesktop,
    ClaudeCode,
    Cursor,
    LmStudio,
}

/// One configuration to copy.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpClientConfig {
    pub kind: McpClientKind,
    /// Where it goes: a file path with `~` or `%APPDATA%`, `terminal` for Claude Code, or a
    /// short hint for LM Studio (shown as is).
    pub location: String,
    /// JSON for the configuration files, a command line for Claude Code. Contains the token
    /// when it comes from `createMcpClient`, the placeholder `<token>` otherwise.
    pub content: String,
    /// Claude Code only: the same command with `MAIL_MCP_TOKEN_FILE` (a file in the home
    /// directory that holds the token) instead of the token, so the token ends up in no shell
    /// history and no process list. Never contains the token.
    pub token_file_content: Option<String>,
    /// The client's documentation of the format.
    pub source: String,
}

impl fmt::Debug for McpClientConfig {
    /// Never prints `content`: it may contain a token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpClientConfig")
            .field("kind", &self.kind)
            .field("location", &self.location)
            .field("content", &"[redacted]")
            .finish_non_exhaustive()
    }
}

/// The setup help without a token (`getMcpSetup`): helper path, endpoint and the
/// configurations with the placeholder `<token>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpSetup {
    pub helper: McpHelperInfo,
    pub endpoint: Option<String>,
    pub configs: Vec<McpClientConfig>,
}

/// A new client (`createMcpClient`): its token and the configurations with the token filled
/// in. Shown once; the app cannot show the token again (a lost token means a new client).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpClientCreated {
    pub client: McpClientInfo,
    /// `mcp1_<client id>_<key>`; the environment variable `MAIL_MCP_TOKEN` of the client.
    pub token: String,
    pub configs: Vec<McpClientConfig>,
}

impl fmt::Debug for McpClientCreated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpClientCreated")
            .field("client", &self.client)
            .field("token", &"[redacted]")
            .field("configs", &self.configs)
            .finish()
    }
}

/// The tools of the MCP server (fixed in `mail-mcp`; nothing sends, deletes, moves or marks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum McpTool {
    ListAccounts,
    ListMailboxes,
    SearchMessages,
    ListMessages,
    ReadMessage,
    ReadThread,
    CreateDraft,
}

impl McpTool {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ListAccounts => "list_accounts",
            Self::ListMailboxes => "list_mailboxes",
            Self::SearchMessages => "search_messages",
            Self::ListMessages => "list_messages",
            Self::ReadMessage => "read_message",
            Self::ReadThread => "read_thread",
            Self::CreateDraft => "create_draft",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::ListAccounts,
            Self::ListMailboxes,
            Self::SearchMessages,
            Self::ListMessages,
            Self::ReadMessage,
            Self::ReadThread,
            Self::CreateDraft,
        ]
        .into_iter()
        .find(|tool| tool.as_str() == name)
    }
}

/// How a logged access ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum McpAccessOutcome {
    /// The tool ran and answered.
    Ok,
    /// The tool ran and reported an error (bad argument, unknown id, app error).
    Error,
    /// Not run (not allowed, rate limit, timeout), or a connection was refused.
    Refused,
}

/// Why the app refused a connection of a helper program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum McpRefusal {
    /// The helper speaks another IPC version (app and helper from different releases).
    UnsupportedProtocol,
    /// MCP is off.
    Disabled,
    /// The client was never registered, or was revoked.
    UnknownClient,
    /// The token's key is wrong.
    BadProof,
    /// The helper sent something unreadable.
    Malformed,
    /// The handshake took too long.
    Timeout,
    /// Too many AI clients at once.
    TooManyConnections,
}

/// Id of an access log entry; grows with every entry. The cursor of `listMcpAccess`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct McpAccessId(#[specta(type = Number)] pub i64);

/// One entry of the access log: metadata only, never arguments or content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpAccessEntry {
    pub id: McpAccessId,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub at: i64,
    /// Null for a connection refused before the client was known.
    pub client_id: Option<String>,
    /// The client's name at that moment.
    pub client_name: Option<String>,
    /// The tool called; null for a refused connection.
    pub tool: Option<McpTool>,
    /// Messages whose data the answer contained (a created draft counts as one).
    pub messages: u32,
    pub outcome: McpAccessOutcome,
    /// Set for a refused connection.
    pub refusal: Option<McpRefusal>,
}

/// One page of the access log, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpAccessPage {
    pub entries: Vec<McpAccessEntry>,
    /// Pass as `before` for the next page; null on the last page.
    pub next_cursor: Option<McpAccessId>,
}

/// What went wrong in "test connection".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum McpTestProblem {
    /// MCP is off; turn it on first.
    Disabled,
    /// The helper program is not where the configurations say (a development build without
    /// `cargo build -p mail-mcp`, or a damaged installation).
    HelperMissing,
    /// The helper program did not start, crashed or did not speak MCP.
    HelperFailed,
    /// The app could not open its endpoint (`McpStatus::error`).
    EndpointFailed,
    /// The helper started but could not reach the app.
    Unreachable,
    /// The app refused the helper (`refusal`).
    Refused,
    /// No answer in time.
    Timeout,
}

/// Result of "test connection": the app starts its helper program like an AI client would,
/// with a temporary client of its own, and asks it for the tools and the shared accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpConnectionTest {
    pub ok: bool,
    pub problem: Option<McpTestProblem>,
    pub refusal: Option<McpRefusal>,
    /// The helper that was started.
    pub helper_path: Option<String>,
    /// MCP protocol version the helper agreed on.
    pub protocol_version: Option<String>,
    /// Tools the helper listed (from the app when it was reached).
    pub tools: Vec<McpTool>,
    /// Accounts visible to AI clients right now.
    pub shared_accounts: u32,
    /// Technical detail for a failure (English, from the app or the helper; never mail data).
    pub detail: Option<String>,
    pub duration_ms: u32,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn wire_names() {
        assert_eq!(
            serde_json::to_value(McpAccountAccess::ReadAndDrafts).expect("json"),
            json!("read_and_drafts")
        );
        assert_eq!(
            serde_json::to_value(McpRefusal::TooManyConnections).expect("json"),
            json!("too_many_connections")
        );
        for tool in [McpTool::ListAccounts, McpTool::CreateDraft] {
            assert_eq!(McpTool::parse(tool.as_str()), Some(tool));
            assert_eq!(
                serde_json::to_value(tool).expect("json"),
                json!(tool.as_str())
            );
        }
    }

    #[test]
    fn tokens_never_reach_debug_output() {
        let created = McpClientCreated {
            client: McpClientInfo {
                client_id: "00".repeat(16),
                name: "Claude".into(),
                created_at: 1,
                revoked_at: None,
                last_used_at: None,
            },
            token: "mcp1_secret".into(),
            configs: vec![McpClientConfig {
                kind: McpClientKind::Cursor,
                location: "~/.cursor/mcp.json".into(),
                content: "{\"MAIL_MCP_TOKEN\": \"mcp1_secret\"}".into(),
                token_file_content: None,
                source: "https://cursor.com/docs/context/mcp".into(),
            }],
        };
        let debug = format!("{created:?}");
        assert!(!debug.contains("secret"), "{debug}");
    }
}
