//! Ready-made client configurations for the setup help in the app's settings (stage 12
//! "Einrichtungshilfe"). Each starts the helper with the endpoint and the token in its
//! environment (not in the arguments: other users can read a process's arguments).
//!
//! Formats as documented on 23.11.2026:
//! - Claude Desktop: `claude_desktop_config.json`, key `mcpServers`, entries with `command`,
//!   `args`, `env`; macOS `~/Library/Application Support/Claude/`, Windows `%APPDATA%\Claude\`.
//!   <https://modelcontextprotocol.io/docs/develop/connect-local-servers>
//! - Claude Code: `claude mcp add --transport stdio --scope user --env KEY=value <name> --
//!   <command>`; user scope is stored in `~/.claude.json` (not in a project's `.mcp.json`,
//!   which is meant to be committed). <https://code.claude.com/docs/en/mcp>
//! - Cursor: `~/.cursor/mcp.json` (global) or `.cursor/mcp.json` (project), `mcpServers` with
//!   `type: "stdio"`, `command`, `args`, `env`. <https://cursor.com/docs/context/mcp>
//! - LM Studio: Program tab, Install > Edit mcp.json; "follows Cursor's mcp.json notation".
//!   <https://lmstudio.ai/docs/app/mcp>

use serde_json::{Value, json};

use crate::endpoint::Endpoint;
use crate::helper::{ENV_ENDPOINT, ENV_TOKEN};

/// Shown instead of the token until the user creates a client.
pub const TOKEN_PLACEHOLDER: &str = "<token>";

/// The name the server gets in the client's list.
pub const SERVER_KEY: &str = "mail";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKind {
    ClaudeDesktop,
    ClaudeCode,
    Cursor,
    LmStudio,
}

impl ClientKind {
    pub const ALL: [ClientKind; 4] = [
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::Cursor,
        Self::LmStudio,
    ];
}

/// One configuration to copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub kind: ClientKind,
    /// Where it goes: a file path (with `~` or `%APPDATA%`), or a terminal for Claude Code.
    pub location: &'static str,
    /// The text to copy: JSON for the config files, a command line for Claude Code.
    pub content: String,
    /// The client's documentation of the format.
    pub source: &'static str,
}

/// What every configuration contains.
#[derive(Debug, Clone, Copy)]
pub struct Setup<'a> {
    /// Absolute path of the shipped helper binary.
    pub helper: &'a str,
    pub endpoint: &'a Endpoint,
    /// The client token, or [`TOKEN_PLACEHOLDER`].
    pub token: &'a str,
}

fn env(setup: &Setup<'_>) -> Value {
    json!({ ENV_ENDPOINT: setup.endpoint.as_str(), ENV_TOKEN: setup.token })
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Single quotes for POSIX shells and PowerShell (both take the text literally); a `'` inside
/// is written as `'\''` (POSIX) or `''` (PowerShell).
fn quote(text: &str) -> String {
    let inner = if cfg!(windows) {
        text.replace('\'', "''")
    } else {
        text.replace('\'', r"'\''")
    };
    format!("'{inner}'")
}

pub fn client_config(kind: ClientKind, setup: &Setup<'_>) -> ClientConfig {
    match kind {
        ClientKind::ClaudeDesktop => ClientConfig {
            kind,
            location: if cfg!(windows) {
                r"%APPDATA%\Claude\claude_desktop_config.json"
            } else {
                "~/Library/Application Support/Claude/claude_desktop_config.json"
            },
            content: pretty(&json!({
                "mcpServers": {
                    SERVER_KEY: { "command": setup.helper, "args": [], "env": env(setup) }
                }
            })),
            source: "https://modelcontextprotocol.io/docs/develop/connect-local-servers",
        },
        ClientKind::ClaudeCode => ClientConfig {
            kind,
            location: "terminal",
            content: format!(
                "claude mcp add --transport stdio --scope user --env {} --env {} {SERVER_KEY} -- {}",
                quote(&format!("{ENV_ENDPOINT}={}", setup.endpoint.as_str())),
                quote(&format!("{ENV_TOKEN}={}", setup.token)),
                quote(setup.helper),
            ),
            source: "https://code.claude.com/docs/en/mcp",
        },
        ClientKind::Cursor => ClientConfig {
            kind,
            location: "~/.cursor/mcp.json",
            content: pretty(&json!({
                "mcpServers": {
                    SERVER_KEY: {
                        "type": "stdio",
                        "command": setup.helper,
                        "args": [],
                        "env": env(setup)
                    }
                }
            })),
            source: "https://cursor.com/docs/context/mcp",
        },
        ClientKind::LmStudio => ClientConfig {
            kind,
            location: "LM Studio: Program > Install > Edit mcp.json",
            content: pretty(&json!({
                "mcpServers": {
                    SERVER_KEY: { "command": setup.helper, "args": [], "env": env(setup) }
                }
            })),
            source: "https://lmstudio.ai/docs/app/mcp",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configurations_carry_path_endpoint_and_token() {
        let endpoint = if cfg!(windows) {
            Endpoint::new(r"\\.\pipe\mail-mcp-0123").expect("endpoint")
        } else {
            Endpoint::new("/Users/me/Library/Application Support/x/mcp/mail-mcp.sock")
                .expect("endpoint")
        };
        let helper = "/Applications/Mail App.app/Contents/MacOS/mail-mcp";
        let setup = Setup {
            helper,
            endpoint: &endpoint,
            token: TOKEN_PLACEHOLDER,
        };
        for kind in ClientKind::ALL {
            let config = client_config(kind, &setup);
            assert!(config.source.starts_with("https://"));
            if kind == ClientKind::ClaudeCode {
                assert!(
                    config
                        .content
                        .starts_with("claude mcp add --transport stdio --scope user")
                );
                assert!(
                    config.content.contains(&format!("'{helper}'")),
                    "{}",
                    config.content
                );
                assert!(
                    config
                        .content
                        .contains(&format!("'MAIL_MCP_TOKEN={TOKEN_PLACEHOLDER}'"))
                );
                continue;
            }
            let json: Value = serde_json::from_str(&config.content).expect("valid JSON");
            let server = &json["mcpServers"][SERVER_KEY];
            assert_eq!(server["command"], helper);
            assert_eq!(server["env"][ENV_ENDPOINT], endpoint.as_str());
            assert_eq!(server["env"][ENV_TOKEN], TOKEN_PLACEHOLDER);
            assert_eq!(server["args"], json!([]), "nothing secret in the arguments");
        }
    }

    #[test]
    fn shell_quoting_keeps_text_literal() {
        if cfg!(windows) {
            assert_eq!(quote("it's"), "'it''s'");
        } else {
            assert_eq!(quote("it's $HOME"), r"'it'\''s $HOME'");
        }
    }
}
