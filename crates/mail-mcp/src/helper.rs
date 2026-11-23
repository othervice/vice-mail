//! The helper program `mail-mcp` that AI clients start (decision 0015): MCP on stdin/stdout,
//! the app over the local IPC channel. It holds no mail data and no permissions; the app decides
//! everything. It connects on the first request that needs the app and reconnects when the app
//! was restarted, so the client never has to be restarted.
//!
//! Configuration, from the environment the client passes:
//! - `MAIL_MCP_ENDPOINT`: socket path (macOS) or pipe name (Windows), from the app's settings
//! - `MAIL_MCP_TOKEN`: the client token (`mcp1_…`), or
//! - `MAIL_MCP_TOKEN_FILE`: the absolute path of a file that holds only the token (finding F5:
//!   a setup command with the token itself ends up in the shell history and, while it runs, in
//!   the process list). On Unix the file must not be accessible to group or others (mode
//!   `& 0o077 == 0`, as `chmod 600` leaves it); on Windows its access list is not checked. When
//!   both variables are set, the file wins and a warning goes to stderr.
//!
//! Without them the helper still answers MCP and explains in every tool result what is missing.

use std::ffi::OsString;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::io::{BufReader, ReadHalf, WriteHalf};
use tokio::time::timeout;

use crate::endpoint::{Endpoint, Stream, connect};
use crate::ipc::{
    CallError, Handshake, Line, PROTOCOL, Refusal, Reply, Request, Side, proof, read_line, verify,
    write_line,
};
use crate::limits;
use crate::mcp::{BackendError, McpServer, ToolBackend, ToolReply};
use crate::permissions::{ClientToken, hex, parse_hex, random_bytes};
use crate::tools::ToolName;

pub const ENV_ENDPOINT: &str = "MAIL_MCP_ENDPOINT";
pub const ENV_TOKEN: &str = "MAIL_MCP_TOKEN";
pub const ENV_TOKEN_FILE: &str = "MAIL_MCP_TOKEN_FILE";

/// Largest token file the helper reads; a token has 107 characters.
const MAX_TOKEN_FILE_BYTES: u64 = 4096;

/// Where to reach the app and as whom.
#[derive(Debug, Clone)]
pub struct HelperConfig {
    pub endpoint: Endpoint,
    pub token: ClientToken,
}

impl HelperConfig {
    /// From `MAIL_MCP_ENDPOINT` and `MAIL_MCP_TOKEN_FILE` or `MAIL_MCP_TOKEN` (an empty value
    /// counts as not set). The error text names what is wrong, never the token.
    pub fn from_env() -> Result<Self, String> {
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        Self::from_values(set(ENV_ENDPOINT), set(ENV_TOKEN), set(ENV_TOKEN_FILE))
    }

    /// [`HelperConfig::from_env`] with the values of the three variables. A token file wins
    /// over `MAIL_MCP_TOKEN`; a token file that cannot be used is an error, never a reason to
    /// fall back to the other variable.
    pub fn from_values(
        endpoint: Option<OsString>,
        token: Option<OsString>,
        token_file: Option<OsString>,
    ) -> Result<Self, String> {
        let endpoint = endpoint
            .ok_or_else(|| format!("{ENV_ENDPOINT} is not set"))
            .and_then(|value| {
                value
                    .into_string()
                    .ok()
                    .and_then(|value| Endpoint::new(value).ok())
                    .ok_or_else(|| format!("{ENV_ENDPOINT} is not a valid endpoint"))
            })?;
        let token = match (token_file, token) {
            (Some(path), token) => {
                if token.is_some() {
                    tracing::warn!(
                        "{ENV_TOKEN_FILE} and {ENV_TOKEN} are both set; using {ENV_TOKEN_FILE}"
                    );
                }
                read_token_file(Path::new(&path))?
            }
            (None, Some(value)) => value
                .to_str()
                .ok_or_else(|| {
                    format!("{ENV_TOKEN}: {}", crate::permissions::TokenError::Malformed)
                })
                .and_then(|value| {
                    ClientToken::parse(value).map_err(|err| format!("{ENV_TOKEN}: {err}"))
                })?,
            (None, None) => return Err(format!("{ENV_TOKEN} (or {ENV_TOKEN_FILE}) is not set")),
        };
        Ok(Self { endpoint, token })
    }
}

/// The token from a file that holds only the token (white space around it is ignored). On Unix
/// the file must not be accessible to group or others. The mode is checked on the opened file,
/// so it is the file that is read. Errors name the file, never its content.
fn read_token_file(path: &Path) -> Result<ClientToken, String> {
    let shown = path.display();
    let failed =
        |what: &str, err: io::Error| format!("{ENV_TOKEN_FILE}: cannot {what} {shown}: {err}");
    // Checked before opening too: opening a FIFO would wait for a writer.
    let before = std::fs::metadata(path).map_err(|err| failed("open", err))?;
    if !before.is_file() {
        return Err(format!("{ENV_TOKEN_FILE}: {shown} is not a file"));
    }
    let file = std::fs::File::open(path).map_err(|err| failed("open", err))?;
    let metadata = file.metadata().map_err(|err| failed("read", err))?;
    if !metadata.is_file() {
        return Err(format!("{ENV_TOKEN_FILE}: {shown} is not a file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!(
                "{ENV_TOKEN_FILE}: other users may access {shown} (mode {mode:03o}); \
                 allow only yourself: chmod 600 {shown}"
            ));
        }
    }
    if metadata.len() > MAX_TOKEN_FILE_BYTES {
        return Err(format!(
            "{ENV_TOKEN_FILE}: {shown} is larger than {MAX_TOKEN_FILE_BYTES} bytes; it must hold \
             only the token"
        ));
    }
    let mut text = String::new();
    file.take(MAX_TOKEN_FILE_BYTES)
        .read_to_string(&mut text)
        .map_err(|err| failed("read", err))?;
    ClientToken::parse(&text).map_err(|err| format!("{ENV_TOKEN_FILE}: {err}"))
}

/// Runs the helper on its stdin and stdout until the client closes stdin.
pub async fn run_stdio(config: Result<HelperConfig, String>) -> io::Result<()> {
    McpServer::new(AppBackend::new(config))
        .serve(tokio::io::stdin(), tokio::io::stdout())
        .await
}

struct Connection {
    reader: BufReader<ReadHalf<Box<dyn Stream>>>,
    writer: WriteHalf<Box<dyn Stream>>,
}

/// [`ToolBackend`] that forwards to the app.
pub struct AppBackend {
    config: Result<HelperConfig, String>,
    connection: Option<Connection>,
    next_id: u64,
}

impl AppBackend {
    pub fn new(config: Result<HelperConfig, String>) -> Self {
        Self {
            config,
            connection: None,
            next_id: 0,
        }
    }

    /// Sends one request and returns the app's reply to it. A broken connection is reopened
    /// once (the app may have restarted since the last request).
    async fn request(&mut self, make: impl Fn(u64) -> Request) -> Result<Reply, BackendError> {
        let config = self
            .config
            .as_ref()
            .map_err(|detail| BackendError::NotConfigured(detail.clone()))?;
        let mut reopened = false;
        loop {
            let connection = match &mut self.connection {
                Some(connection) => connection,
                None => {
                    reopened = true;
                    self.connection.insert(open(config).await?)
                }
            };
            self.next_id += 1;
            let id = self.next_id;
            match exchange(connection, &make(id), id).await {
                Ok(Reply::Closed { reason }) => {
                    self.connection = None;
                    return Err(BackendError::Refused(reason));
                }
                Ok(reply) => return Ok(reply),
                Err(Exchange::Broken) if !reopened => {
                    self.connection = None;
                }
                Err(Exchange::Broken) => {
                    self.connection = None;
                    return Err(BackendError::Unreachable);
                }
                Err(Exchange::Failed(err)) => {
                    self.connection = None;
                    return Err(err);
                }
            }
        }
    }
}

impl ToolBackend for AppBackend {
    async fn list_tools(&mut self) -> Result<Vec<ToolName>, BackendError> {
        match self.request(|id| Request::ListTools { id }).await? {
            Reply::Tools { tools, .. } => Ok(tools
                .iter()
                .filter_map(|name| ToolName::parse(name))
                .collect()),
            other => Err(unexpected(&other)),
        }
    }

    async fn call_tool(
        &mut self,
        tool: ToolName,
        arguments: Map<String, Value>,
    ) -> Result<ToolReply, BackendError> {
        let reply = self
            .request(|id| Request::CallTool {
                id,
                name: tool.as_str().to_owned(),
                arguments: arguments.clone(),
            })
            .await?;
        match reply {
            Reply::Result { text, is_error, .. } => Ok(ToolReply::Result { text, is_error }),
            Reply::Error { code, message, .. } => match code {
                CallError::UnknownTool => Ok(ToolReply::Unavailable),
                CallError::RateLimited => Err(BackendError::RateLimited),
                CallError::Timeout => Err(BackendError::Timeout),
                CallError::Malformed => Err(BackendError::Failed(message)),
            },
            other => Err(unexpected(&other)),
        }
    }
}

fn unexpected(reply: &Reply) -> BackendError {
    BackendError::Failed(format!("unexpected answer {reply:?}"))
}

enum Exchange {
    /// The connection is gone (the app quit or restarted).
    Broken,
    Failed(BackendError),
}

async fn exchange(
    connection: &mut Connection,
    request: &Request,
    id: u64,
) -> Result<Reply, Exchange> {
    if write_line(&mut connection.writer, request).await.is_err() {
        return Err(Exchange::Broken);
    }
    let wait = Duration::from_secs(limits::CALL_TIMEOUT_SECS + 5);
    loop {
        let line = timeout(
            wait,
            read_line(&mut connection.reader, limits::RESPONSE_LINE_BYTES),
        )
        .await
        .map_err(|_| Exchange::Failed(BackendError::Timeout))?;
        let bytes = match line {
            Ok(Line::Data(bytes)) => bytes,
            Ok(Line::TooLong) => {
                return Err(Exchange::Failed(BackendError::Failed(
                    "answer too long".into(),
                )));
            }
            Ok(Line::Eof) | Err(_) => return Err(Exchange::Broken),
        };
        let reply: Reply = serde_json::from_slice(&bytes)
            .map_err(|_| Exchange::Failed(BackendError::Failed("unreadable answer".into())))?;
        let reply_id = match &reply {
            Reply::Tools { id, .. } | Reply::Result { id, .. } | Reply::Error { id, .. } => *id,
            Reply::Closed { .. } => return Ok(reply),
        };
        match reply_id {
            reply_id if reply_id == id => return Ok(reply),
            // An error about an unreadable line (id 0) or an answer to an abandoned request.
            _ => continue,
        }
    }
}

async fn open(config: &HelperConfig) -> Result<Connection, BackendError> {
    let stream = connect(&config.endpoint).await.map_err(|err| {
        tracing::info!(error = %err, "mail app not reachable");
        BackendError::Unreachable
    })?;
    let (read, writer) = tokio::io::split(stream);
    let mut connection = Connection {
        reader: BufReader::new(read),
        writer,
    };
    timeout(
        Duration::from_secs(limits::HANDSHAKE_TIMEOUT_SECS),
        handshake(&mut connection, &config.token),
    )
    .await
    .map_err(|_| BackendError::Failed("the handshake timed out".into()))??;
    Ok(connection)
}

async fn read_handshake(connection: &mut Connection) -> Result<Handshake, BackendError> {
    match read_line(&mut connection.reader, limits::HANDSHAKE_LINE_BYTES).await {
        Ok(Line::Data(bytes)) => serde_json::from_slice(&bytes)
            .map_err(|_| BackendError::Failed("unreadable handshake".into())),
        // The app closed the connection without a word (for example another user's process).
        Ok(Line::Eof) | Err(_) => Err(BackendError::Unreachable),
        Ok(Line::TooLong) => Err(BackendError::Failed("handshake line too long".into())),
    }
}

async fn handshake(connection: &mut Connection, token: &ClientToken) -> Result<(), BackendError> {
    let helper_nonce = random_bytes::<32>().map_err(|err| BackendError::Failed(err.to_string()))?;
    let broken = |_| BackendError::Unreachable;
    write_line(
        &mut connection.writer,
        &Handshake::Hello {
            protocol: PROTOCOL,
            client_id: token.id,
            nonce: hex(&helper_nonce),
        },
    )
    .await
    .map_err(broken)?;
    let (app_nonce, app_proof) = match read_handshake(connection).await? {
        Handshake::Challenge {
            protocol,
            nonce,
            proof,
        } if protocol == PROTOCOL => (nonce, proof),
        Handshake::Refused { reason, .. } => return Err(BackendError::Refused(reason)),
        _ => return Err(BackendError::Failed("unexpected handshake message".into())),
    };
    let verified = parse_hex::<32>(&app_nonce).filter(|app_nonce| {
        verify(
            &token.key,
            Side::App,
            &token.id,
            &helper_nonce,
            app_nonce,
            &app_proof,
        )
    });
    let Some(app_nonce) = verified else {
        // Tell the app why (it records it), then stop: nothing else is sent.
        let _ = write_line(
            &mut connection.writer,
            &Handshake::Refused {
                reason: Refusal::BadProof,
                supported: Vec::new(),
            },
        )
        .await;
        return Err(BackendError::NotVerified);
    };
    write_line(
        &mut connection.writer,
        &Handshake::Auth {
            proof: proof(
                &token.key,
                Side::Helper,
                &token.id,
                &helper_nonce,
                &app_nonce,
            ),
        },
    )
    .await
    .map_err(broken)?;
    match read_handshake(connection).await? {
        Handshake::Ready => Ok(()),
        Handshake::Refused { reason, .. } => Err(BackendError::Refused(reason)),
        _ => Err(BackendError::Failed("unexpected handshake message".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoint() -> Option<OsString> {
        Some(if cfg!(windows) {
            r"\\.\pipe\mail-mcp-test".into()
        } else {
            "/tmp/mail-mcp-test.sock".into()
        })
    }

    fn token() -> ClientToken {
        ClientToken::generate().expect("token")
    }

    /// A token file with `content` and, on Unix, `mode`.
    fn token_file(dir: &tempfile::TempDir, content: &str, mode: u32) -> OsString {
        let path = dir.path().join("token");
        std::fs::write(&path, content).expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("mode");
        }
        #[cfg(not(unix))]
        let _ = mode;
        path.into_os_string()
    }

    fn secret(token: &ClientToken) -> OsString {
        token.expose_secret().into()
    }

    #[test]
    fn the_token_comes_from_the_variable_or_from_a_private_file() {
        let inline = token();
        let config = HelperConfig::from_values(endpoint(), Some(secret(&inline)), None)
            .expect("MAIL_MCP_TOKEN still works");
        assert_eq!(config.token.id, inline.id);

        let dir = tempfile::tempdir().expect("dir");
        let stored = token();
        let file = token_file(&dir, &format!("{}\n", stored.expose_secret()), 0o600);
        let config =
            HelperConfig::from_values(endpoint(), None, Some(file.clone())).expect("from the file");
        assert_eq!(config.token.id, stored.id);
        assert_eq!(config.token.key.expose_secret(), stored.key.expose_secret());

        // Both set: the file wins.
        let config =
            HelperConfig::from_values(endpoint(), Some(secret(&inline)), Some(file)).expect("both");
        assert_eq!(config.token.id, stored.id);

        let err = HelperConfig::from_values(endpoint(), None, None).expect_err("none");
        assert!(
            err.contains(ENV_TOKEN) && err.contains(ENV_TOKEN_FILE),
            "{err}"
        );
        let err =
            HelperConfig::from_values(None, Some(secret(&inline)), None).expect_err("no endpoint");
        assert_eq!(err, "MAIL_MCP_ENDPOINT is not set");
    }

    #[cfg(unix)]
    #[test]
    fn a_token_file_others_may_access_is_refused() {
        let dir = tempfile::tempdir().expect("dir");
        let stored = token();
        let inline = token();
        for mode in [0o644, 0o640, 0o604, 0o620, 0o602, 0o610] {
            let file = token_file(&dir, &stored.expose_secret(), mode);
            // Also when MAIL_MCP_TOKEN would work: no silent fall back.
            let err = HelperConfig::from_values(endpoint(), Some(secret(&inline)), Some(file))
                .expect_err("refused");
            assert!(
                err.starts_with("MAIL_MCP_TOKEN_FILE: other users may access"),
                "{err}"
            );
            assert!(err.contains(&format!("mode {mode:03o}")), "{err}");
            assert!(err.contains("chmod 600"), "{err}");
            assert!(!err.contains(&stored.expose_secret()), "never the token");
        }
        for mode in [0o600, 0o400] {
            let file = token_file(&dir, &stored.expose_secret(), mode);
            HelperConfig::from_values(endpoint(), None, Some(file)).expect("private");
        }
    }

    #[test]
    fn a_bad_token_file_is_explained_without_its_content() {
        let dir = tempfile::tempdir().expect("dir");
        let missing = dir.path().join("missing").into_os_string();
        let err = HelperConfig::from_values(endpoint(), None, Some(missing)).expect_err("missing");
        assert!(
            err.starts_with("MAIL_MCP_TOKEN_FILE: cannot open "),
            "{err}"
        );

        let file = token_file(&dir, "mcp1_not-a-token-but-a-secret", 0o600);
        let err = HelperConfig::from_values(endpoint(), None, Some(file)).expect_err("malformed");
        assert!(
            err.starts_with("MAIL_MCP_TOKEN_FILE: the token is not"),
            "{err}"
        );
        assert!(!err.contains("secret"), "{err}");

        let file = token_file(&dir, "  \n", 0o600);
        let err = HelperConfig::from_values(endpoint(), None, Some(file)).expect_err("empty");
        assert_eq!(err, "MAIL_MCP_TOKEN_FILE: the token is missing");

        let big = format!("{}{}", token().expose_secret(), " ".repeat(5000));
        let file = token_file(&dir, &big, 0o600);
        let err = HelperConfig::from_values(endpoint(), None, Some(file)).expect_err("large");
        assert!(err.contains("larger than 4096 bytes"), "{err}");

        let folder = dir.path().join("folder");
        std::fs::create_dir(&folder).expect("dir");
        let err = HelperConfig::from_values(endpoint(), None, Some(folder.into_os_string()))
            .expect_err("a folder");
        assert!(err.ends_with("is not a file"), "{err}");
    }
}
