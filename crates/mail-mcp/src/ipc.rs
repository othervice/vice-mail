//! The channel between the helper and the app (decision 0015): JSON, one message per line
//! (`\n`), every line bounded. Version 1.
//!
//! Handshake (both sides prove the client key without sending it; HMAC-SHA-256 over fixed-size
//! fields, compared in constant time):
//!
//! ```text
//! helper → app   {"type":"hello","protocol":1,"client_id":"<32 hex>","nonce":"<64 hex>"}
//! app → helper   {"type":"challenge","protocol":1,"nonce":"<64 hex>","proof":"<64 hex>"}
//!                proof = HMAC(key, "mail-mcp ipc 1 app" ‖ client id ‖ helper nonce ‖ app nonce)
//! helper → app   {"type":"auth","proof":"<64 hex>"}
//!                proof = HMAC(key, "mail-mcp ipc 1 helper" ‖ client id ‖ helper nonce ‖ app nonce)
//! app → helper   {"type":"ready"}
//! ```
//!
//! Instead of a challenge or `ready` the app may answer `{"type":"refused","reason":…}`
//! (`unsupported_protocol` carries `"supported":[1]`). The helper checks the app's proof before
//! it sends its own, so a process that only pretends to be the app learns nothing it could use
//! and cannot feed the client fake mail.
//!
//! After `ready` the helper sends requests and the app answers each, in order:
//!
//! ```text
//! {"type":"list_tools","id":1}                       → {"type":"tools","id":1,"tools":["list_accounts",…]}
//! {"type":"call_tool","id":2,"name":…,"arguments":{…}} → {"type":"result","id":2,"text":…,"is_error":false}
//!                                                     or {"type":"error","id":2,"code":"unknown_tool","message":…}
//! ```
//!
//! The app may send `{"type":"closed","reason":"disabled"|"unknown_client"}` at any time and
//! close the connection (MCP switched off, client revoked).

use std::io;

use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::Sha256;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::permissions::{ClientId, ClientKey, parse_hex};

/// The only protocol version so far.
pub const PROTOCOL: u32 = 1;

/// Why the app refused or closed a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    UnsupportedProtocol,
    /// MCP is switched off.
    Disabled,
    /// No such client (never registered, or revoked).
    UnknownClient,
    /// The client's proof was wrong: wrong key.
    BadProof,
    /// Not a handshake message.
    Malformed,
    /// The handshake took too long.
    Timeout,
    TooManyConnections,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Handshake {
    Hello {
        protocol: u32,
        client_id: ClientId,
        nonce: String,
    },
    Challenge {
        protocol: u32,
        nonce: String,
        proof: String,
    },
    Auth {
        proof: String,
    },
    Ready,
    Refused {
        reason: Refusal,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        supported: Vec<u32>,
    },
}

/// Requests of the helper after the handshake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Request {
    ListTools {
        id: u64,
    },
    CallTool {
        id: u64,
        name: String,
        #[serde(default)]
        arguments: Map<String, Value>,
    },
}

/// Answers of the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum Reply {
    Tools {
        id: u64,
        tools: Vec<String>,
    },
    Result {
        id: u64,
        text: String,
        is_error: bool,
    },
    Error {
        id: u64,
        code: CallError,
        message: String,
    },
    Closed {
        reason: Refusal,
    },
}

/// Why a request got no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CallError {
    /// No such tool, or not allowed for this client now (`create_draft` without permission).
    UnknownTool,
    /// Not a request of this protocol, or too long.
    Malformed,
    RateLimited,
    Timeout,
}

/// Which side a proof is from; part of the MAC input, so a proof cannot be reflected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    App,
    Helper,
}

type HmacSha256 = Hmac<Sha256>;

fn mac(
    key: &ClientKey,
    side: Side,
    client: &ClientId,
    helper_nonce: &[u8; 32],
    app_nonce: &[u8; 32],
) -> HmacSha256 {
    let mut mac = match HmacSha256::new_from_slice(key.expose_secret()) {
        Ok(mac) => mac,
        // HMAC takes keys of every length (as in settings_sync::crypto).
        Err(_) => unreachable!("HMAC accepts keys of every length"),
    };
    mac.update(match side {
        Side::App => b"mail-mcp ipc 1 app\0",
        Side::Helper => b"mail-mcp ipc 1 helper\0",
    });
    mac.update(client.as_bytes());
    mac.update(helper_nonce);
    mac.update(app_nonce);
    mac
}

pub(crate) fn proof(
    key: &ClientKey,
    side: Side,
    client: &ClientId,
    helper_nonce: &[u8; 32],
    app_nonce: &[u8; 32],
) -> String {
    let tag: [u8; 32] = mac(key, side, client, helper_nonce, app_nonce)
        .finalize()
        .into_bytes()
        .into();
    crate::permissions::hex(&tag)
}

/// Constant-time check of a proof in hex.
pub(crate) fn verify(
    key: &ClientKey,
    side: Side,
    client: &ClientId,
    helper_nonce: &[u8; 32],
    app_nonce: &[u8; 32],
    given: &str,
) -> bool {
    let Some(given) = parse_hex::<32>(given) else {
        return false;
    };
    mac(key, side, client, helper_nonce, app_nonce)
        .verify_slice(&given)
        .is_ok()
}

/// One line read with a bound.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Line {
    Data(Vec<u8>),
    /// Longer than allowed; it was read to its end and dropped.
    TooLong,
    Eof,
}

/// Reads up to `\n` (removed, also a `\r` before it). At most `max` bytes are kept; a longer
/// line is consumed and reported as [`Line::TooLong`]. Not cancel-safe: run it in its own task
/// or drop the stream after cancelling.
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> io::Result<Line> {
    let mut line = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(match (too_long, line.is_empty()) {
                (true, _) => Line::TooLong,
                (false, true) => Line::Eof,
                (false, false) => Line::Data(line),
            });
        }
        let (chunk, used, end) = match available.iter().position(|b| *b == b'\n') {
            Some(pos) => (&available[..pos], pos + 1, true),
            None => (available, available.len(), false),
        };
        if !too_long {
            if line.len() + chunk.len() > max {
                too_long = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(chunk);
            }
        }
        reader.consume(used);
        if end {
            if too_long {
                return Ok(Line::TooLong);
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Line::Data(line));
        }
    }
}

/// Writes `message` as one JSON line and flushes.
pub(crate) async fn write_line<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    message: &T,
) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(message).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use tokio::io::BufReader;

    use super::*;
    use crate::permissions::ClientToken;

    #[tokio::test]
    async fn lines_are_bounded() {
        let input: &[u8] = b"short\r\nexactly10!\nthis one is far too long\nlast";
        let mut reader = BufReader::with_capacity(4, input);
        assert_eq!(
            read_line(&mut reader, 10).await.expect("read"),
            Line::Data(b"short".to_vec())
        );
        assert_eq!(
            read_line(&mut reader, 10).await.expect("read"),
            Line::Data(b"exactly10!".to_vec())
        );
        assert_eq!(
            read_line(&mut reader, 10).await.expect("read"),
            Line::TooLong
        );
        assert_eq!(
            read_line(&mut reader, 10).await.expect("read"),
            Line::Data(b"last".to_vec())
        );
        assert_eq!(read_line(&mut reader, 10).await.expect("read"), Line::Eof);
    }

    #[test]
    fn proofs_bind_side_client_and_nonces() {
        let token = ClientToken::generate().expect("random");
        let other = ClientToken::generate().expect("random");
        let (h, a) = ([1_u8; 32], [2_u8; 32]);
        let app = proof(&token.key, Side::App, &token.id, &h, &a);
        assert!(verify(&token.key, Side::App, &token.id, &h, &a, &app));
        assert!(
            !verify(&token.key, Side::Helper, &token.id, &h, &a, &app),
            "reflected"
        );
        assert!(
            !verify(&other.key, Side::App, &token.id, &h, &a, &app),
            "other key"
        );
        assert!(
            !verify(&token.key, Side::App, &other.id, &h, &a, &app),
            "other client"
        );
        assert!(
            !verify(&token.key, Side::App, &token.id, &a, &h, &app),
            "swapped nonces"
        );
        assert!(!verify(
            &token.key,
            Side::App,
            &token.id,
            &h,
            &a,
            &app.to_uppercase()
        ));
        assert!(!verify(&token.key, Side::App, &token.id, &h, &a, ""));
    }

    #[test]
    fn wire_format() {
        let hello: Handshake = serde_json::from_str(
            r#"{"type":"hello","protocol":1,"client_id":"000102030405060708090a0b0c0d0e0f","nonce":"ab"}"#,
        )
        .expect("hello");
        assert!(matches!(hello, Handshake::Hello { protocol: 1, .. }));
        assert_eq!(
            serde_json::to_string(&Handshake::Refused {
                reason: Refusal::UnsupportedProtocol,
                supported: vec![PROTOCOL]
            })
            .expect("json"),
            r#"{"type":"refused","reason":"unsupported_protocol","supported":[1]}"#
        );
        let call: Request =
            serde_json::from_str(r#"{"type":"call_tool","id":3,"name":"read_message"}"#)
                .expect("call");
        assert_eq!(
            call,
            Request::CallTool {
                id: 3,
                name: "read_message".into(),
                arguments: Map::new()
            }
        );
    }
}
