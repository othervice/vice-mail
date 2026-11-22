//! A minimal SMTP submission client for tests: STARTTLS (with the test CA), AUTH PLAIN, one
//! message. Stage 5 brings the real SMTP adapter (lettre); this only delivers test mail.

use std::time::Duration;

use rustls::pki_types::ServerName;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

use super::{TestAccount, TestEnv};

/// Sends a plain-text message from `from` to `to` through the submission port (STARTTLS and
/// AUTH required). Returns once the server accepted the message for delivery.
pub async fn send_mail(
    env: &TestEnv,
    from: &TestAccount,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(20), send(env, from, to, subject, body))
        .await
        .map_err(|_| "SMTP timed out".to_owned())?
}

async fn send(
    env: &TestEnv,
    from: &TestAccount,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<(), String> {
    let tcp = TcpStream::connect((env.host.as_str(), env.submission_port))
        .await
        .map_err(|err| format!("connect: {err}"))?;
    let mut plain = BufReader::new(tcp);
    expect(&mut plain, "220").await?;
    command(&mut plain, "EHLO vicemail.test", "250").await?;
    command(&mut plain, "STARTTLS", "220").await?;
    let tcp = plain.into_inner();

    let config =
        crate::tls::client_config(Some(&env.extra_root())).map_err(|err| err.to_string())?;
    let name = ServerName::try_from(env.tls_name.clone()).map_err(|err| err.to_string())?;
    let tls = TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|err| format!("TLS: {err}"))?;
    let mut s = BufReader::new(tls);
    command(&mut s, "EHLO vicemail.test", "250").await?;
    let credentials = format!("\0{}\0{}", from.address, from.password.expose_secret());
    let auth = format!("AUTH PLAIN {}", base64(credentials.as_bytes()));
    command(&mut s, &auth, "235").await?;
    command(&mut s, &format!("MAIL FROM:<{}>", from.address), "250").await?;
    command(&mut s, &format!("RCPT TO:<{to}>"), "250").await?;
    command(&mut s, "DATA", "354").await?;
    let message_id = format!(
        "<test-{}-{}@vicemail.test>",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    );
    let mut data = format!(
        "From: {}\r\nTo: {to}\r\nSubject: {subject}\r\nMessage-ID: {message_id}\r\n\
         MIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n",
        from.address
    );
    for line in body.lines() {
        // Dot-stuffing (RFC 5321, section 4.5.2).
        if line.starts_with('.') {
            data.push('.');
        }
        data.push_str(line);
        data.push_str("\r\n");
    }
    data.push('.');
    command(&mut s, &data, "250").await?;
    command(&mut s, "QUIT", "221").await.ok();
    Ok(())
}

async fn command<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut BufReader<S>,
    line: &str,
    code: &str,
) -> Result<(), String> {
    s.get_mut()
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .map_err(|err| format!("write: {err}"))?;
    s.get_mut()
        .flush()
        .await
        .map_err(|err| format!("flush: {err}"))?;
    let verb = line.split(' ').next().unwrap_or_default();
    expect(s, code)
        .await
        .map_err(|err| format!("{verb}: {err}"))
}

/// Reads one (possibly multi-line) reply and checks its code.
async fn expect<S: AsyncRead + Unpin>(s: &mut BufReader<S>, code: &str) -> Result<(), String> {
    loop {
        let mut line = String::new();
        let n = s
            .read_line(&mut line)
            .await
            .map_err(|err| format!("read: {err}"))?;
        if n == 0 {
            return Err("connection closed".into());
        }
        if !line.starts_with(code) {
            return Err(format!("expected {code}, got {}", line.trim_end()));
        }
        // `250-...` continues, `250 ...` ends the reply.
        if line.as_bytes().get(3) != Some(&b'-') {
            return Ok(());
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk.first().copied().unwrap_or(0),
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc_4648() {
        for (input, output) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(super::base64(input.as_bytes()), output);
        }
    }
}
