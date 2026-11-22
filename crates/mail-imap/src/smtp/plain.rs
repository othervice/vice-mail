//! The plaintext part of STARTTLS (RFC 3207): greeting, EHLO, STARTTLS, nothing else. Nothing
//! secret is sent before TLS, and bytes that arrive after the `220` to STARTTLS (a man in the
//! middle injecting replies) end the connection.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use mail_model::BackendError;

use crate::error::{from_io, short};
use crate::timeout_io::IoTimeout;

/// Longest reply accepted in the plaintext phase.
const MAX_REPLY_BYTES: u64 = 64 * 1024;

/// One reply: code and text lines (without code and separator).
#[derive(Debug)]
pub(super) struct PlainReply {
    pub(super) code: u16,
    pub(super) lines: Vec<String>,
}

impl PlainReply {
    fn text(&self) -> String {
        short(&self.lines.join(" "))
    }
}

/// Reads one (possibly multi-line) reply.
pub(super) async fn read_reply<S: AsyncBufRead + Unpin>(
    stream: &mut S,
) -> Result<PlainReply, BackendError> {
    let mut lines = Vec::new();
    let mut total: u64 = 0;
    loop {
        let mut line = Vec::new();
        let left = MAX_REPLY_BYTES.saturating_sub(total);
        let n = (&mut *stream)
            .take(left)
            .read_until(b'\n', &mut line)
            .await
            .map_err(|err| from_io(&err))?;
        if n == 0 {
            return Err(BackendError::ConnectionLost(
                "server closed the connection".into(),
            ));
        }
        total += n as u64;
        if !line.ends_with(b"\n") {
            return Err(BackendError::Protocol(if total >= MAX_REPLY_BYTES {
                "SMTP reply too long".into()
            } else {
                "connection ended inside a reply".into()
            }));
        }
        let text = String::from_utf8_lossy(&line);
        let text = text.trim_end_matches(['\r', '\n']);
        let code = text
            .get(..3)
            .filter(|c| c.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|c| c.parse::<u16>().ok())
            .ok_or_else(|| BackendError::Protocol(format!("not an SMTP reply: {}", short(text))))?;
        let separator = text.as_bytes().get(3).copied();
        lines.push(text.get(4..).unwrap_or_default().to_owned());
        match separator {
            Some(b'-') => continue,
            None | Some(b' ') => return Ok(PlainReply { code, lines }),
            Some(_) => {
                return Err(BackendError::Protocol(format!(
                    "not an SMTP reply: {}",
                    short(text)
                )));
            }
        }
    }
}

async fn command(
    stream: &mut BufReader<IoTimeout<TcpStream>>,
    line: &str,
) -> Result<PlainReply, BackendError> {
    stream
        .get_mut()
        .write_all(line.as_bytes())
        .await
        .map_err(|err| from_io(&err))?;
    stream
        .get_mut()
        .flush()
        .await
        .map_err(|err| from_io(&err))?;
    read_reply(stream).await
}

/// Greeting, `EHLO hello`, `STARTTLS`; returns the TCP stream ready for the TLS handshake.
pub(super) async fn starttls(
    plain: IoTimeout<TcpStream>,
    hello: &str,
) -> Result<TcpStream, BackendError> {
    let mut stream = BufReader::new(plain);
    let greeting = read_reply(&mut stream).await?;
    if greeting.code != 220 {
        return Err(BackendError::Rejected(format!(
            "server refused the connection: {} {}",
            greeting.code,
            greeting.text()
        )));
    }
    let ehlo = command(&mut stream, &format!("EHLO {hello}\r\n")).await?;
    if ehlo.code != 250 {
        return Err(BackendError::Rejected(format!(
            "EHLO: {} {}",
            ehlo.code,
            ehlo.text()
        )));
    }
    let offered = ehlo.lines.iter().skip(1).any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|keyword| keyword.eq_ignore_ascii_case("STARTTLS"))
    });
    if !offered {
        return Err(BackendError::StarttlsUnavailable);
    }
    let reply = command(&mut stream, "STARTTLS\r\n").await?;
    if reply.code != 220 {
        return Err(BackendError::StarttlsUnavailable);
    }
    // Anything after the 220 came unencrypted and would be read as if it came over TLS.
    if !stream.buffer().is_empty() {
        return Err(BackendError::Protocol(
            "unexpected data after the STARTTLS reply".into(),
        ));
    }
    Ok(stream.into_inner().into_inner())
}
