//! The only module that talks to the network. One HTTP client per provider profile:
//!
//! - TLS from the caller (`mail_imap::tls::client_config`: the system's certificate check and
//!   the app's one crypto provider), HTTP/1.1;
//! - no cookie store (reqwest is built without cookies), no `Referer`, no proxy (a system proxy
//!   would see the mail and break the "local" label), no redirects (a redirect is an error, so
//!   a local server cannot hand the request to the internet), no automatic retries (a request
//!   with mail is never sent twice without the user), a neutral `User-Agent` without product
//!   name or version;
//! - for endpoints labelled local whose host is a name, a resolver that only accepts addresses
//!   of that kind ([`crate::endpoint::address_allowed`]);
//! - time limits for connecting, for the response headers, between two chunks of the stream
//!   and for the whole request; size limits for the request, every event and the whole answer;
//!   error bodies are read only up to a small limit and their text is cleaned
//!   ([`crate::error::server_message`]: the key never appears in an error).
//!
//! Requests go only to URLs built from the validated base URL and a path fixed by the
//! provider kind ([`Endpoint::url`]); nothing the model writes is ever a request target.

use std::net::SocketAddr;
use std::time::{Duration, SystemTime};

use mail_model::Secret;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{StatusCode, Url};
use tokio::time::Instant;

use crate::endpoint::{Endpoint, Locality, address_allowed};
use crate::error::server_message;
use crate::sse::{SseEvent, SseParser};
use crate::{AiError, CancelToken, TimeoutPhase};

/// The `User-Agent` of every request: what browsers send first, nothing that names the app
/// (as for external images, `mail-engine/src/reading/remote.rs`).
pub const NEUTRAL_USER_AGENT: &str = "Mozilla/5.0";

/// Time and size limits of one provider profile. The defaults suit local models that load
/// slowly as well as cloud providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpLimits {
    /// TCP connect and TLS handshake.
    pub connect_timeout: Duration,
    /// Until the response headers arrive (a local server may load the model first).
    pub response_timeout: Duration,
    /// Longest pause between two chunks of a stream (Anthropic sends `ping` events, local
    /// servers stream tokens).
    pub idle_timeout: Duration,
    /// The whole request, stream included.
    pub total_timeout: Duration,
    /// Largest request body.
    pub max_request_bytes: usize,
    /// Largest response (whole stream or body).
    pub max_response_bytes: usize,
    /// Largest single event of a stream.
    pub max_event_bytes: usize,
    /// Most characters of answer text a stream may deliver.
    pub max_output_chars: usize,
    /// Largest error body that is read (for its message).
    pub max_error_body_bytes: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            response_timeout: Duration::from_secs(180),
            idle_timeout: Duration::from_secs(90),
            total_timeout: Duration::from_secs(600),
            max_request_bytes: 4 << 20,
            max_response_bytes: 16 << 20,
            max_event_bytes: 1 << 20,
            max_output_chars: 256 * 1024,
            max_error_body_bytes: 64 * 1024,
        }
    }
}

/// The client of one profile. Cheap to clone.
#[derive(Clone)]
pub(crate) struct HttpClient {
    client: reqwest::Client,
    limits: HttpLimits,
}

impl HttpClient {
    pub fn new(
        tls: &rustls::ClientConfig,
        limits: HttpLimits,
        endpoint: &Endpoint,
    ) -> Result<Self, AiError> {
        let mut builder = reqwest::Client::builder()
            .tls_backend_preconfigured(tls.clone())
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .no_proxy()
            .retry(reqwest::retry::never())
            .user_agent(NEUTRAL_USER_AGENT)
            .connect_timeout(limits.connect_timeout)
            .tcp_keepalive(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(30));
        if endpoint.locality().is_local() && endpoint.is_name() {
            builder = builder.dns_resolver(LocalResolver {
                locality: endpoint.locality(),
            });
        }
        let client = builder
            .build()
            .map_err(|err| AiError::Internal(format!("HTTP client: {}", err.without_url())))?;
        Ok(Self { client, limits })
    }

    pub fn limits(&self) -> &HttpLimits {
        &self.limits
    }

    /// POSTs a JSON body and returns the successful response (2xx); any other status becomes
    /// its typed error. The returned [`Body`] carries the remaining time of the request.
    pub async fn post_json(
        &self,
        url: Url,
        headers: HeaderMap,
        body: Vec<u8>,
        key: Option<&Secret<String>>,
        cancel: &CancelToken,
    ) -> Result<Body, AiError> {
        if body.len() > self.limits.max_request_bytes {
            return Err(AiError::RequestTooLarge);
        }
        let request = self
            .client
            .post(url)
            .headers(headers)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        self.send(request, key, cancel).await
    }

    /// GETs a JSON document (model lists): the whole body, up to the response limit.
    pub async fn get_json(
        &self,
        url: Url,
        headers: HeaderMap,
        key: Option<&Secret<String>>,
        cancel: &CancelToken,
    ) -> Result<serde_json::Value, AiError> {
        let request = self
            .client
            .get(url)
            .headers(headers)
            .header(reqwest::header::ACCEPT, "application/json");
        let mut body = self.send(request, key, cancel).await?;
        let bytes = body.read_all(cancel).await?;
        serde_json::from_slice(&bytes)
            .map_err(|err| AiError::Protocol(format!("invalid JSON: {err}")))
    }

    async fn send(
        &self,
        request: reqwest::RequestBuilder,
        key: Option<&Secret<String>>,
        cancel: &CancelToken,
    ) -> Result<Body, AiError> {
        let deadline = Instant::now() + self.limits.total_timeout;
        let response_deadline = deadline.min(Instant::now() + self.limits.response_timeout);
        let response = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(AiError::Cancelled),
            sent = tokio::time::timeout_at(response_deadline, request.send()) => match sent {
                Ok(Ok(response)) => response,
                Ok(Err(err)) => return Err(transport_error(&err)),
                Err(_) => return Err(AiError::Timeout(TimeoutPhase::Response)),
            },
        };
        let mut body = Body {
            response,
            deadline,
            idle: self.limits.idle_timeout,
            received: 0,
            max: self.limits.max_response_bytes,
        };
        let status = body.response.status();
        if status.is_success() {
            return Ok(body);
        }
        body.max = self.limits.max_error_body_bytes;
        let headers = body.response.headers().clone();
        // The message is only a help; a body that does not arrive is not an error of its own.
        let raw = body.read_all(cancel).await.unwrap_or_default();
        Err(status_error(status, &headers, &raw, key, SystemTime::now()))
    }
}

/// A response body being read, with the request's time and size limits.
pub(crate) struct Body {
    response: reqwest::Response,
    deadline: Instant,
    idle: Duration,
    received: usize,
    max: usize,
}

impl Body {
    pub fn headers(&self) -> &HeaderMap {
        self.response.headers()
    }

    /// The next chunk; `None` at the end. Cancelling, the idle limit, the total limit and the
    /// size limit end the read with an error.
    pub async fn chunk(&mut self, cancel: &CancelToken) -> Result<Option<Vec<u8>>, AiError> {
        let idle_deadline = self.deadline.min(Instant::now() + self.idle);
        let chunk = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(AiError::Cancelled),
            chunk = tokio::time::timeout_at(idle_deadline, self.response.chunk()) => match chunk {
                Ok(Ok(chunk)) => chunk,
                Ok(Err(err)) => return Err(transport_error(&err)),
                Err(_) if Instant::now() >= self.deadline => {
                    return Err(AiError::Timeout(TimeoutPhase::Total));
                }
                Err(_) => return Err(AiError::Timeout(TimeoutPhase::Idle)),
            },
        };
        let Some(chunk) = chunk else {
            return Ok(None);
        };
        self.received += chunk.len();
        if self.received > self.max {
            return Err(AiError::ResponseTooLarge);
        }
        Ok(Some(chunk.to_vec()))
    }

    async fn read_all(&mut self, cancel: &CancelToken) -> Result<Vec<u8>, AiError> {
        let mut all = Vec::new();
        while let Some(chunk) = self.chunk(cancel).await? {
            all.extend_from_slice(&chunk);
        }
        Ok(all)
    }
}

/// Events of a server-sent event stream, read from a [`Body`].
pub(crate) struct EventReader {
    body: Body,
    parser: SseParser,
    ended: bool,
}

impl EventReader {
    pub fn new(body: Body, max_event_bytes: usize) -> Self {
        Self {
            body,
            parser: SseParser::new(max_event_bytes),
            ended: false,
        }
    }

    pub fn headers(&self) -> &HeaderMap {
        self.body.headers()
    }

    /// The next event; `None` when the stream ended (an unterminated last event is dropped).
    pub async fn next(&mut self, cancel: &CancelToken) -> Result<Option<SseEvent>, AiError> {
        loop {
            if let Some(event) = self.parser.next_event() {
                return Ok(Some(event));
            }
            if self.ended {
                return Ok(None);
            }
            match self.body.chunk(cancel).await? {
                Some(chunk) => self.parser.feed(&chunk)?,
                None => self.ended = true,
            }
        }
    }
}

/// A transport failure as an error, without the URL.
fn transport_error(err: &reqwest::Error) -> AiError {
    if err.is_timeout() {
        return AiError::Timeout(if err.is_connect() {
            TimeoutPhase::Connect
        } else {
            TimeoutPhase::Response
        });
    }
    let mut text = err.to_string();
    let mut source = std::error::Error::source(err);
    while let Some(inner) = source {
        text.push_str(": ");
        text.push_str(&inner.to_string());
        source = inner.source();
    }
    // reqwest's own text names the URL; the base URL holds no secret, but keep errors short.
    let text = text.replace(
        &format!(" for url ({})", err.url().map_or("", Url::as_str)),
        "",
    );
    if err.is_connect() || err.is_request() {
        AiError::Connect(crate::text::clean_line(&text, 300))
    } else {
        AiError::Protocol(crate::text::clean_line(&text, 300))
    }
}

/// A non-2xx status as an error. `raw` is the (limited) error body.
pub(crate) fn status_error(
    status: StatusCode,
    headers: &HeaderMap,
    raw: &[u8],
    key: Option<&Secret<String>>,
    now: SystemTime,
) -> AiError {
    let message = error_body_message(raw).and_then(|text| server_message(&text, key));
    let retry_after = retry_after(headers, now);
    match status.as_u16() {
        401 | 403 => AiError::Unauthorized {
            status: status.as_u16(),
            message,
        },
        404 => AiError::NotFound { message },
        413 => AiError::RequestTooLarge,
        429 => AiError::RateLimited { retry_after },
        503 | 529 => AiError::Overloaded { retry_after },
        500..=599 => AiError::Server {
            status: status.as_u16(),
            message,
        },
        code => AiError::Rejected {
            status: code,
            message,
        },
    }
}

/// The message of an error body: `{"error": {"message": …}}` (OpenAI, Anthropic, OpenRouter),
/// `{"error": "…"}` (Ollama), `{"message": …}`, else the text itself.
pub(crate) fn error_body_message(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
        let found = json
            .pointer("/error/message")
            .or_else(|| json.get("error").filter(|value| value.is_string()))
            .or_else(|| json.get("message"))
            .and_then(serde_json::Value::as_str);
        return found.map(str::to_owned);
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Longest wait honoured from a `retry-after` header.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(3600);

/// `retry-after-ms` (milliseconds, OpenAI), else `retry-after` in seconds or as an HTTP date
/// (RFC 9110). Capped at an hour.
pub(crate) fn retry_after(headers: &HeaderMap, now: SystemTime) -> Option<Duration> {
    let value = |name: &str| {
        headers
            .get(HeaderName::from_bytes(name.as_bytes()).ok()?)
            .and_then(|value: &HeaderValue| value.to_str().ok())
            .map(str::trim)
    };
    let parsed = if let Some(ms) = value("retry-after-ms").and_then(|v| v.parse::<f64>().ok()) {
        (ms.is_finite() && ms >= 0.0).then(|| Duration::from_secs_f64(ms.min(3.6e6) / 1000.0))
    } else {
        let raw = value("retry-after")?;
        if let Ok(seconds) = raw.parse::<u64>() {
            Some(Duration::from_secs(seconds))
        } else {
            let at = parse_http_date(raw)?;
            let now = now.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
            let at = u64::try_from(at).ok()?;
            Some(Duration::from_secs(at.saturating_sub(now)))
        }
    };
    parsed.map(|wait| wait.min(MAX_RETRY_AFTER))
}

/// The preferred HTTP date format (IMF-fixdate, `Sun, 06 Nov 1994 08:49:37 GMT`) as Unix
/// seconds. The obsolete formats are not accepted (servers must not send them).
fn parse_http_date(text: &str) -> Option<i64> {
    let (_weekday, rest) = text.split_once(", ")?;
    let mut parts = rest.split(' ');
    let day: u8 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i32 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    if parts.next()? != "GMT" || parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let date = mail_model::search::CivilDate::new(year, month, day)?;
    Some(date.days() * 86_400 + hour * 3600 + minute * 60 + second)
}

/// For local endpoints given by name: the system resolver, keeping only addresses that match
/// the label; none left is an error (nothing is sent).
struct LocalResolver {
    locality: Locality,
}

impl reqwest::dns::Resolve for LocalResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        let locality = self.locality;
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let allowed: Vec<SocketAddr> = resolved
                .filter(|addr| address_allowed(locality, addr.ip()))
                .collect();
            if allowed.is_empty() {
                return Err(
                    "refused: the name of this local endpoint does not resolve to a local address"
                        .into(),
                );
            }
            Ok(Box::new(allowed.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// A header for a key or other sensitive value (marked sensitive, so hyper never logs it).
pub(crate) fn header(
    name: &'static str,
    value: &str,
) -> Result<(HeaderName, HeaderValue), AiError> {
    let mut value = HeaderValue::from_str(value)
        .map_err(|_| AiError::InvalidConfig(format!("invalid characters for header {name}")))?;
    value.set_sensitive(true);
    Ok((HeaderName::from_static(name), value))
}

/// Keeps the HTTP client out of `Debug` output of providers.
impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_static(name),
                HeaderValue::from_str(value).expect("value"),
            );
        }
        map
    }

    #[test]
    fn retry_after_forms() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(784_111_717); // 06 Nov 1994 08:48:37
        assert_eq!(
            retry_after(&headers(&[("retry-after", "7")]), now),
            Some(Duration::from_secs(7))
        );
        assert_eq!(
            retry_after(
                &headers(&[("retry-after", "Sun, 06 Nov 1994 08:49:37 GMT")]),
                now
            ),
            Some(Duration::from_secs(60))
        );
        assert_eq!(
            retry_after(
                &headers(&[("retry-after-ms", "1500"), ("retry-after", "9")]),
                now
            ),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            retry_after(&headers(&[("retry-after", "999999")]), now),
            Some(MAX_RETRY_AFTER)
        );
        // A date in the past means "now".
        assert_eq!(
            retry_after(
                &headers(&[("retry-after", "Sat, 05 Nov 1994 08:49:37 GMT")]),
                now
            ),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_after(&headers(&[("retry-after", "soon")]), now), None);
        assert_eq!(retry_after(&HeaderMap::new(), now), None);
    }

    #[test]
    fn http_dates() {
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT"), None);
        assert_eq!(parse_http_date("Sun, 31 Feb 1994 08:49:37 GMT"), None);
        assert_eq!(parse_http_date("Sun, 06 Nov 1994 08:49:37 CET"), None);
    }

    #[test]
    fn statuses_map_to_typed_errors() {
        let now = SystemTime::UNIX_EPOCH;
        let key = Secret::new("sk-test-0123456789abcdef".to_owned());
        let echoed = br#"{"error":{"message":"Incorrect API key provided: sk-test-0123456789abcdef","type":"invalid_request_error"}}"#;
        let err = status_error(
            StatusCode::UNAUTHORIZED,
            &HeaderMap::new(),
            echoed,
            Some(&key),
            now,
        );
        let AiError::Unauthorized { status, message } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(*status, 401);
        let message = message.as_deref().expect("message");
        assert!(!message.contains("sk-test-0123456789abcdef"), "{message}");
        assert!(message.contains("[redacted]"), "{message}");
        for text in [err.to_string(), format!("{err:?}")] {
            assert!(!text.contains("0123456789abcdef"), "{text}");
        }

        let rate = status_error(
            StatusCode::TOO_MANY_REQUESTS,
            &headers(&[("retry-after", "3")]),
            b"",
            None,
            now,
        );
        assert_eq!(
            rate,
            AiError::RateLimited {
                retry_after: Some(Duration::from_secs(3))
            }
        );
        assert!(rate.is_transient());
        assert_eq!(rate.retry_after(), Some(Duration::from_secs(3)));
        assert!(matches!(
            status_error(
                StatusCode::from_u16(529).expect("529"),
                &HeaderMap::new(),
                b"",
                None,
                now
            ),
            AiError::Overloaded { .. }
        ));
        assert!(matches!(
            status_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &HeaderMap::new(),
                b"oops",
                None,
                now
            ),
            AiError::Server { status: 500, .. }
        ));
        let ollama = status_error(
            StatusCode::NOT_FOUND,
            &HeaderMap::new(),
            br#"{"error":"model 'llama9' not found"}"#,
            None,
            now,
        );
        assert_eq!(
            ollama,
            AiError::NotFound {
                message: Some("model 'llama9' not found".into())
            }
        );
        assert!(matches!(
            status_error(StatusCode::BAD_REQUEST, &HeaderMap::new(), b"", None, now),
            AiError::Rejected {
                status: 400,
                message: None
            }
        ));
        assert_eq!(
            status_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                &HeaderMap::new(),
                b"",
                None,
                now
            ),
            AiError::RequestTooLarge
        );
    }
}
