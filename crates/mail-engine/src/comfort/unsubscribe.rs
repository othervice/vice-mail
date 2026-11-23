//! Unsubscribing (RFC 2369 `List-Unsubscribe`, RFC 8058 one-click), only after the user's
//! click (architecture, "Erlaubte Netzverbindungen").
//!
//! - **One-click**: an HTTPS `POST` with the body `List-Unsubscribe=One-Click` to the `https`
//!   URI of `List-Unsubscribe`, only if the message also had
//!   `List-Unsubscribe-Post: List-Unsubscribe=One-Click`. Like external images: reqwest over
//!   the shared TLS configuration (`mail_imap::tls`), HTTP/1.1, no cookies, no `Referer`, no
//!   proxy, no retries, the neutral `User-Agent`, **never to private, loopback or link-local
//!   addresses** (IP literals before, names after resolution), no redirects followed (RFC 8058
//!   asks the receiver for one request; a redirect status counts as answered), 5 s to connect,
//!   15 s in total. Nothing else of the message is sent.
//! - **mailto**: the fields of the `mailto:` URI for the composer; the user sends the request.
//! - **Web**: an `http`/`https` page, handed to the system browser after the UI asked.

use std::sync::Arc;
use std::time::Duration;

use mail_content::parse_mailto;
use mail_model::{BackendError, CoreError, ExtraRootCa, UnsubscribeMethod, UnsubscribeOptions};
use reqwest::Url;

use crate::discovery::autoconfig::NEUTRAL_USER_AGENT;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest URI considered.
const MAX_URI: usize = 2_048;

/// `List-Unsubscribe-Post: List-Unsubscribe=One-Click` in a raw header block.
pub(crate) fn one_click_in(block: &[u8]) -> bool {
    let text = String::from_utf8_lossy(block);
    let mut current: Option<String> = None;
    let mut found = false;
    let mut check = |field: &str| {
        if let Some((name, value)) = field.split_once(':')
            && name.trim().eq_ignore_ascii_case("List-Unsubscribe-Post")
            && value
                .split_whitespace()
                .collect::<String>()
                .eq_ignore_ascii_case("List-Unsubscribe=One-Click")
        {
            found = true;
        }
    };
    for line in text.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(field) = current.as_mut() {
                field.push(' ');
                field.push_str(line.trim());
            }
            continue;
        }
        if let Some(field) = current.take() {
            check(&field);
        }
        current = Some(line.to_owned());
    }
    if let Some(field) = current.take() {
        check(&field);
    }
    found
}

/// The URIs of a `List-Unsubscribe` value (`<uri>, <uri>`), as written.
pub(crate) fn uris_of(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find('<') {
        let Some(len) = rest.get(start + 1..).and_then(|r| r.find('>')) else {
            break;
        };
        if let Some(uri) = rest.get(start + 1..start + 1 + len) {
            let uri: String = uri.chars().filter(|c| !c.is_whitespace()).collect();
            if !uri.is_empty() && uri.len() <= MAX_URI {
                out.push(uri);
            }
        }
        rest = rest.get(start + 1 + len + 1..).unwrap_or_default();
    }
    out
}

/// A usable `http`/`https` URL: absolute, a host, no user name or password.
fn web_url(uri: &str) -> Option<Url> {
    let url = Url::parse(uri).ok()?;
    let fine = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none();
    fine.then_some(url)
}

/// What a message offers, from its `List-Unsubscribe` URIs and whether
/// `List-Unsubscribe-Post: List-Unsubscribe=One-Click` was present.
pub(crate) fn options(uris: &[String], one_click: bool) -> UnsubscribeOptions {
    let https = uris
        .iter()
        .filter_map(|u| web_url(u))
        .find(|u| u.scheme() == "https");
    let web = uris.iter().find_map(|u| web_url(u));
    let mailto = uris.iter().find_map(|u| {
        let request = parse_mailto(u).ok()?;
        request.to.first().map(|a| a.email.clone())
    });
    UnsubscribeOptions {
        one_click_host: one_click
            .then(|| https.as_ref().and_then(|u| u.host_str().map(str::to_owned)))
            .flatten(),
        mailto,
        web: web.map(|u| u.to_string()),
    }
}

/// The target of `method` among `uris`.
pub(crate) fn target(
    uris: &[String],
    one_click: bool,
    method: UnsubscribeMethod,
) -> Result<String, CoreError> {
    let missing = || {
        CoreError::NotFound(format!(
            "the message offers no {method:?} unsubscribe (List-Unsubscribe)"
        ))
    };
    match method {
        UnsubscribeMethod::OneClick => {
            if !one_click {
                return Err(missing());
            }
            uris.iter()
                .filter_map(|u| web_url(u))
                .find(|u| u.scheme() == "https")
                .map(|u| u.to_string())
                .ok_or_else(missing)
        }
        UnsubscribeMethod::Web => uris
            .iter()
            .find_map(|u| web_url(u))
            .map(|u| u.to_string())
            .ok_or_else(missing),
        UnsubscribeMethod::Mailto => uris
            .iter()
            .find(|u| parse_mailto(u).is_ok_and(|r| !r.to.is_empty()))
            .cloned()
            .ok_or_else(missing),
    }
}

/// Sends one-click unsubscribe requests.
#[derive(Clone)]
pub(crate) struct OneClick {
    client: Result<reqwest::Client, String>,
    allow_loopback: bool,
}

impl OneClick {
    /// `extra_root` and `allow_loopback` only in tests (the test CA, a local counter server).
    pub(crate) fn new(extra_root: Option<&ExtraRootCa>, allow_loopback: bool) -> Self {
        let client = mail_imap::tls::client_config(extra_root)
            .map_err(|err| err.to_string())
            .and_then(|tls| {
                reqwest::Client::builder()
                    .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
                    .http1_only()
                    .redirect(reqwest::redirect::Policy::none())
                    .referer(false)
                    .no_proxy()
                    .retry(reqwest::retry::never())
                    .user_agent(NEUTRAL_USER_AGENT)
                    .connect_timeout(CONNECT_TIMEOUT)
                    .timeout(REQUEST_TIMEOUT)
                    .dns_resolver(crate::reading::remote::PublicResolver::new(allow_loopback))
                    .build()
                    .map_err(|err| format!("HTTP client: {err}"))
            });
        Self {
            client,
            allow_loopback,
        }
    }

    /// POSTs `List-Unsubscribe=One-Click` to `url`; the HTTP status on a 2xx or 3xx answer.
    pub(crate) async fn post(&self, url: &str) -> Result<u16, CoreError> {
        let url = Url::parse(url)
            .map_err(|err| CoreError::InvalidArgument(format!("unsubscribe URL: {err}")))?;
        if url.scheme() != "https" {
            return Err(CoreError::InvalidArgument(
                "one-click unsubscribe needs an https URL".into(),
            ));
        }
        crate::reading::remote::check_url(&url, self.allow_loopback)
            .map_err(CoreError::InvalidArgument)?;
        let client = self
            .client
            .clone()
            .map_err(|err| CoreError::Internal(format!("no HTTP client: {err}")))?;
        let host = url.host_str().unwrap_or_default().to_owned();
        let response = client
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body("List-Unsubscribe=One-Click")
            .send()
            .await
            .map_err(http_error)?;
        let status = response.status();
        tracing::info!(host, status = status.as_u16(), "one-click unsubscribe sent");
        if status.is_success() || status.is_redirection() {
            Ok(status.as_u16())
        } else {
            Err(CoreError::Mail(BackendError::Rejected(format!(
                "the list's server answered {status}"
            ))))
        }
    }
}

fn http_error(err: reqwest::Error) -> CoreError {
    let (timeout, connect) = (err.is_timeout(), err.is_connect());
    let detail = err.without_url().to_string();
    if timeout {
        CoreError::Mail(BackendError::Timeout(detail))
    } else if connect {
        if detail.contains("refused: ") {
            CoreError::InvalidArgument(detail)
        } else {
            CoreError::Mail(BackendError::ConnectionRefused)
        }
    } else {
        CoreError::Mail(BackendError::ConnectionLost(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_click_header() {
        assert!(one_click_in(
            b"List-Unsubscribe: <https://x/u>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n\r\n"
        ));
        assert!(one_click_in(
            b"list-unsubscribe-post:\r\n  List-Unsubscribe=One-Click\r\n"
        ));
        assert!(!one_click_in(b"List-Unsubscribe: <https://x/u>\r\n\r\n"));
        assert!(!one_click_in(b"List-Unsubscribe-Post: Other=1\r\n"));
        // Only the header block counts.
        assert!(!one_click_in(
            b"Subject: x\r\n\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n"
        ));
    }

    #[test]
    fn options_and_targets() {
        let uris = uris_of(
            "<mailto:leave@lists.example?subject=unsubscribe>, <https://news.example/u?id=1>, \
             <http://old.example/u>",
        );
        assert_eq!(uris.len(), 3);
        let options = options(&uris, true);
        assert_eq!(options.one_click_host.as_deref(), Some("news.example"));
        assert_eq!(options.mailto.as_deref(), Some("leave@lists.example"));
        assert_eq!(options.web.as_deref(), Some("https://news.example/u?id=1"));
        // Without the Post header there is no one-click.
        assert_eq!(super::options(&uris, false).one_click_host, None);
        assert!(target(&uris, false, UnsubscribeMethod::OneClick).is_err());
        assert_eq!(
            target(&uris, true, UnsubscribeMethod::OneClick).expect("https"),
            "https://news.example/u?id=1"
        );
        // Only http: a page for the browser, never a POST.
        let http = uris_of("<http://old.example/u>");
        assert_eq!(super::options(&http, true).one_click_host, None);
        assert_eq!(
            super::options(&http, true).web.as_deref(),
            Some("http://old.example/u")
        );
        // Credentials in the URL, other schemes, junk: refused.
        let bad =
            uris_of("<https://user:pw@x.example/u>, <javascript:alert(1)>, <ftp://x/u>, garbage");
        assert!(super::options(&bad, true).is_empty());
        assert!(uris_of("no brackets at all").is_empty());
        assert!(uris_of("<https://x.example/u").is_empty());
    }

    #[tokio::test]
    async fn private_targets_are_refused_before_any_connection() {
        let client = OneClick::new(None, false);
        for url in [
            "https://127.0.0.1/u",
            "https://10.0.0.1/u",
            "https://192.168.1.1/u",
            "https://[::1]/u",
            "https://169.254.1.1/u",
        ] {
            let err = client.post(url).await.expect_err(url);
            assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument, "{url}");
        }
        let err = client
            .post("http://news.example/u")
            .await
            .expect_err("http");
        assert_eq!(err.code(), mail_model::ErrorCode::InvalidArgument);
    }
}
