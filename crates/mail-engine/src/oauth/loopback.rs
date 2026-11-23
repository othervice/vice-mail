//! The loopback redirect of the sign-in (RFC 8252, section 7.3): a minimal HTTP listener on
//! `127.0.0.1` (and `::1` for `localhost` redirects) on a port the system picks.
//!
//! It accepts exactly one answer: `GET /` whose query carries the `state` this sign-in sent.
//! Everything else (another path such as `/favicon.ico`, another method, a missing or other
//! `state`, repeated parameters) gets an error page and the wait goes on, so an answer injected
//! by another program or an old browser tab can neither finish nor break the sign-in. Requests
//! are read with a size and time limit; nothing of them is logged.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use mail_model::{OAuthError, OAuthPageTexts, Secret};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::Instant;

use super::pkce::same;

/// Longest request accepted (request line and headers).
const MAX_REQUEST: usize = 16 * 1024;
/// How long one connection may take to send its request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Longest `error_description` kept from the provider's answer.
const MAX_DESCRIPTION: usize = 300;

/// What the provider sent back through the browser (with the right `state`).
#[derive(Debug)]
pub(crate) enum Callback {
    Code(Secret<String>),
    /// `error` and, if given, a shortened `error_description` (RFC 6749, section 4.1.2.1).
    Error {
        error: String,
        description: Option<String>,
    },
}

/// The listening sockets of one sign-in.
pub(crate) struct Listener {
    v4: TcpListener,
    v6: Option<TcpListener>,
    port: u16,
}

impl Listener {
    /// Binds `127.0.0.1` on a free port; with `also_v6` also `::1` on the same port, where
    /// the system allows it (`localhost` may resolve to either).
    pub(crate) async fn bind(also_v6: bool) -> Result<Self, OAuthError> {
        let v4 = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .map_err(|err| OAuthError::Failed(format!("no local port for the sign-in: {err}")))?;
        let port = v4
            .local_addr()
            .map_err(|err| OAuthError::Failed(format!("no local port for the sign-in: {err}")))?
            .port();
        let v6 = if also_v6 {
            match TcpListener::bind(SocketAddr::from((Ipv6Addr::LOCALHOST, port))).await {
                Ok(listener) => Some(listener),
                Err(err) => {
                    tracing::debug!(error = %err, "sign-in listener on ::1 not available");
                    None
                }
            }
        } else {
            None
        };
        Ok(Self { v4, v6, port })
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// Waits for the answer with `state` until `deadline` or until `cancel` turns true.
    pub(crate) async fn wait(
        self,
        state: &Secret<String>,
        texts: &OAuthPageTexts,
        mut cancel: watch::Receiver<bool>,
        deadline: Instant,
    ) -> Result<Callback, OAuthError> {
        loop {
            let accepted = tokio::select! {
                biased;
                _ = cancel.wait_for(|cancelled| *cancelled) => return Err(OAuthError::Cancelled),
                () = tokio::time::sleep_until(deadline) => return Err(OAuthError::TimedOut),
                accepted = self.v4.accept() => accepted,
                accepted = accept_v6(self.v6.as_ref()) => accepted,
            };
            let Ok((stream, _)) = accepted else {
                continue;
            };
            if let Some(callback) = answer(stream, state, texts).await {
                return Ok(callback);
            }
        }
    }
}

async fn accept_v6(listener: Option<&TcpListener>) -> std::io::Result<(TcpStream, SocketAddr)> {
    match listener {
        Some(listener) => listener.accept().await,
        None => std::future::pending().await,
    }
}

/// Reads one request and answers it. `Some` only for the real answer of this sign-in.
async fn answer(
    mut stream: TcpStream,
    state: &Secret<String>,
    texts: &OAuthPageTexts,
) -> Option<Callback> {
    let request = match tokio::time::timeout(REQUEST_TIMEOUT, read_request(&mut stream)).await {
        Ok(Some(request)) => request,
        _ => {
            respond(&mut stream, 400, &texts.title, &texts.failed).await;
            return None;
        }
    };
    match judge(&request, state) {
        Verdict::NotFound => {
            respond(&mut stream, 404, &texts.title, &texts.failed).await;
            None
        }
        Verdict::Refused => {
            tracing::warn!("sign-in: refused an answer without the expected state");
            respond(&mut stream, 400, &texts.title, &texts.failed).await;
            None
        }
        Verdict::Accepted(callback) => {
            let text = match callback {
                Callback::Code(_) => &texts.done,
                Callback::Error { .. } => &texts.failed,
            };
            respond(&mut stream, 200, &texts.title, text).await;
            Some(callback)
        }
    }
}

/// The request line and headers, or `None` if the request is too long or broken.
async fn read_request(stream: &mut TcpStream) -> Option<String> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(chunk.get(..n)?);
        if buffer.windows(4).any(|w| w == b"\r\n\r\n") || buffer.windows(2).any(|w| w == b"\n\n") {
            break;
        }
        if buffer.len() > MAX_REQUEST {
            return None;
        }
    }
    String::from_utf8(buffer).ok()
}

#[derive(Debug)]
enum Verdict {
    /// Another path or method: not for the sign-in at all.
    NotFound,
    /// The sign-in's path, but not its answer (no or another `state`, repeated parameters).
    Refused,
    Accepted(Callback),
}

/// Decides about one request (its request line; the headers do not matter).
fn judge(request: &str, state: &Secret<String>) -> Verdict {
    let line = request.lines().next().unwrap_or_default();
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Verdict::NotFound;
    };
    if method != "GET" || !version.starts_with("HTTP/1.") {
        return Verdict::NotFound;
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/" {
        return Verdict::NotFound;
    }
    let mut code = None;
    let mut error = None;
    let mut description = None;
    let mut got_state = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        let slot = match key.as_ref() {
            "code" => &mut code,
            "error" => &mut error,
            "error_description" => &mut description,
            "state" => &mut got_state,
            _ => continue,
        };
        if slot.is_some() {
            // RFC 6749, section 3.1: parameters must not repeat.
            return Verdict::Refused;
        }
        *slot = Some(value.into_owned());
    }
    match got_state {
        Some(got) if same(&got, state.expose_secret()) => {}
        _ => return Verdict::Refused,
    }
    match (code, error) {
        (_, Some(error)) => Verdict::Accepted(Callback::Error {
            error: clean(&error, 64),
            description: description.map(|d| clean(&d, MAX_DESCRIPTION)),
        }),
        (Some(code), None) if !code.is_empty() => {
            Verdict::Accepted(Callback::Code(Secret::new(code)))
        }
        _ => Verdict::Accepted(Callback::Error {
            error: "no_code".into(),
            description: None,
        }),
    }
}

/// Printable text of at most `max` characters (it ends up in an error detail).
fn clean(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect()
}

async fn respond(stream: &mut TcpStream, status: u16, title: &str, text: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Bad Request",
    };
    let body = format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width, initial-scale=1\"><title>{title}</title></head>\
         <body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;\
         padding:0 1rem\"><h1>{title}</h1><p>{text}</p></body></html>",
        title = escape(title),
        text = escape(text),
    );
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\n\
         X-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let write = async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body.as_bytes()).await?;
        stream.shutdown().await
    };
    if tokio::time::timeout(REQUEST_TIMEOUT, write).await.is_err() {
        tracing::debug!("sign-in page not delivered in time");
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Secret<String> {
        Secret::new("s3cr3t-state".to_owned())
    }

    fn verdict(line: &str) -> Verdict {
        judge(&format!("{line}\r\nHost: 127.0.0.1\r\n\r\n"), &state())
    }

    #[test]
    fn only_get_on_the_root_with_the_state_is_the_answer() {
        assert!(matches!(
            verdict("GET /?code=abc&state=s3cr3t-state HTTP/1.1"),
            Verdict::Accepted(Callback::Code(code)) if code.expose_secret() == "abc"
        ));
        assert!(matches!(
            verdict("GET /?state=s3cr3t-state&error=access_denied&error_description=No%0Athanks HTTP/1.1"),
            Verdict::Accepted(Callback::Error { error, description })
                if error == "access_denied" && description.as_deref() == Some("Nothanks")
        ));
        for refused in [
            "GET /?code=abc&state=other HTTP/1.1",
            "GET /?code=abc HTTP/1.1",
            "GET /?code=abc&state=s3cr3t-stat HTTP/1.1",
            "GET /?code=abc&state=s3cr3t-state&state=s3cr3t-state HTTP/1.1",
            "GET /?code=abc&code=evil&state=s3cr3t-state HTTP/1.1",
        ] {
            assert!(matches!(verdict(refused), Verdict::Refused), "{refused}");
        }
        for not_found in [
            "GET /favicon.ico HTTP/1.1",
            "GET /callback?code=abc&state=s3cr3t-state HTTP/1.1",
            "GET //?code=abc&state=s3cr3t-state HTTP/1.1",
            "POST /?code=abc&state=s3cr3t-state HTTP/1.1",
            "GET /?code=abc&state=s3cr3t-state",
            "nonsense",
        ] {
            assert!(
                matches!(verdict(not_found), Verdict::NotFound),
                "{not_found}"
            );
        }
    }

    #[test]
    fn page_texts_are_escaped() {
        assert_eq!(escape("<b>&\"'"), "&lt;b&gt;&amp;&quot;&#39;");
    }
}
