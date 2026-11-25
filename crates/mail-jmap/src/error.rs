//! Maps HTTP, TLS and JMAP errors onto [`BackendError`]. Texts are shortened and never contain
//! the password; server texts are kept short because they can carry message data.

use std::error::Error as _;
use std::io;

use mail_model::BackendError;
use serde_json::Value;

/// Longest detail text taken over from a server answer or a library error.
const MAX_DETAIL: usize = 160;

/// Shortens `text` to [`MAX_DETAIL`] characters and replaces control characters.
pub(crate) fn short(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_DETAIL + 3));
    for (i, ch) in text.chars().enumerate() {
        if i == MAX_DETAIL {
            out.push('…');
            break;
        }
        out.push(if ch.is_control() { ' ' } else { ch });
    }
    out.trim().to_owned()
}

/// TLS errors, as the IMAP adapter types them.
pub(crate) fn from_tls(err: &rustls::Error) -> BackendError {
    use rustls::CertificateError as C;
    match err {
        rustls::Error::InvalidCertificate(cert) => match cert {
            C::NotValidForName | C::NotValidForNameContext { .. } => BackendError::CertNameMismatch,
            C::Expired
            | C::ExpiredContext { .. }
            | C::NotValidYet
            | C::NotValidYetContext { .. } => BackendError::CertExpired,
            other => BackendError::CertUntrusted(short(&format!("{other:?}"))),
        },
        other => BackendError::Tls(short(&other.to_string())),
    }
}

/// A failed HTTP exchange (no status): TLS, DNS, connect, timeouts, broken connections.
pub(crate) fn from_http(err: &reqwest::Error) -> BackendError {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = err.source();
    let mut io_kind = None;
    let mut text = err.to_string();
    while let Some(current) = cause {
        if let Some(tls) = current.downcast_ref::<rustls::Error>() {
            return from_tls(tls);
        }
        if let Some(io) = current.downcast_ref::<io::Error>() {
            if let Some(tls) = io
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            {
                return from_tls(tls);
            }
            io_kind.get_or_insert(io.kind());
        }
        text.push_str(": ");
        text.push_str(&current.to_string());
        cause = current.source();
    }
    let lower = text.to_ascii_lowercase();
    if err.is_connect() {
        if lower.contains("dns error") || lower.contains("failed to lookup") {
            return BackendError::Dns(short(&text));
        }
        return match io_kind {
            Some(io::ErrorKind::ConnectionRefused) => BackendError::ConnectionRefused,
            Some(io::ErrorKind::TimedOut) => BackendError::ConnectTimeout,
            _ if err.is_timeout() => BackendError::ConnectTimeout,
            _ => BackendError::ConnectionLost(short(&format!("could not connect: {text}"))),
        };
    }
    if err.is_timeout() || io_kind == Some(io::ErrorKind::TimedOut) {
        return BackendError::Timeout(short(&text));
    }
    if err.is_builder() {
        return BackendError::InvalidState(short(&text));
    }
    BackendError::ConnectionLost(short(&text))
}

/// An HTTP answer that is not a success. `body` may hold an RFC 7807 problem (RFC 8620 section
/// 3.6.1).
pub(crate) fn from_status(status: u16, body: &[u8]) -> BackendError {
    let problem = serde_json::from_slice::<Value>(body).ok();
    let kind = problem
        .as_ref()
        .and_then(|p| p.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .to_owned();
    let detail = problem
        .as_ref()
        .and_then(|p| p.get("detail").or_else(|| p.get("title")))
        .and_then(Value::as_str)
        .map(short)
        .unwrap_or_default();
    let what = if kind.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status} {kind}")
    };
    let text = if detail.is_empty() {
        what
    } else {
        format!("{what}: {detail}")
    };
    match status {
        401 => BackendError::AuthFailed(text),
        404 => BackendError::JmapUnavailable(text),
        // "unknownCapability", "notJSON", "notRequest": the app sent something the server does
        // not understand; "limit": a limit of the session was exceeded.
        400 if kind == "limit" => BackendError::Rejected(text),
        400 => BackendError::Protocol(text),
        _ => BackendError::Rejected(text),
    }
}

/// A method-level error (`["error", {"type": ...}, id]`, RFC 8620 section 3.6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MethodError {
    pub kind: String,
    pub description: Option<String>,
}

impl MethodError {
    pub(crate) fn from_args(args: &Value) -> Self {
        Self {
            kind: args
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("serverFail")
                .to_owned(),
            description: args.get("description").and_then(Value::as_str).map(short),
        }
    }

    pub(crate) fn into_backend(self, method: &str) -> BackendError {
        let text = match &self.description {
            Some(d) if !d.is_empty() => format!("{method}: {} ({d})", self.kind),
            _ => format!("{method}: {}", self.kind),
        };
        match self.kind.as_str() {
            "unknownMethod" | "unsupportedSort" | "unsupportedFilter" => {
                BackendError::Unsupported(text)
            }
            "accountNotFound" | "accountNotSupportedByMethod" => {
                BackendError::JmapUnavailable(text)
            }
            "accountReadOnly" => BackendError::ReadOnly(text),
            "invalidArguments" | "invalidResultReference" | "unknownCapability" => {
                BackendError::Protocol(text)
            }
            // serverUnavailable, serverFail, serverPartialFail, forbidden, requestTooLarge,
            // stateMismatch, cannotCalculateChanges (handled where it matters), ...
            _ => BackendError::Rejected(text),
        }
    }
}

/// A per-object error of a `/set` or `/import` (RFC 8620 section 5.3).
pub(crate) fn set_error_text(method: &str, error: &Value) -> String {
    let kind = error
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match error.get("description").and_then(Value::as_str) {
        Some(d) if !d.is_empty() => short(&format!("{method}: {kind} ({d})")),
        _ => format!("{method}: {kind}"),
    }
}

/// A set error as a backend error.
pub(crate) fn from_set_error(method: &str, error: &Value) -> BackendError {
    let text = set_error_text(method, error);
    match error.get("type").and_then(Value::as_str).unwrap_or("") {
        "forbidden" if method.starts_with("Email") => BackendError::Rejected(text),
        // Stage 20 (decision 0060): the mailbox is full (RFC 8620 `overQuota`).
        "overQuota" => BackendError::OverQuota(text),
        "invalidPatch" | "invalidProperties" => BackendError::Protocol(text),
        _ => BackendError::Rejected(text),
    }
}

#[cfg(test)]
mod tests {
    use rustls::CertificateError;

    use super::*;

    #[test]
    fn statuses_and_problems_are_typed() {
        assert!(matches!(from_status(401, b""), BackendError::AuthFailed(_)));
        assert!(matches!(
            from_status(404, b"not found"),
            BackendError::JmapUnavailable(_)
        ));
        let problem = br#"{"type":"urn:ietf:params:jmap:error:limit","limit":"maxSizeRequest","status":400,"detail":"too big"}"#;
        match from_status(400, problem) {
            BackendError::Rejected(text) => {
                assert_eq!(text, "HTTP 400 limit: too big");
            }
            other => panic!("{other:?}"),
        }
        let problem = br#"{"type":"urn:ietf:params:jmap:error:notJSON","status":400}"#;
        assert!(matches!(
            from_status(400, problem),
            BackendError::Protocol(_)
        ));
        let busy = from_status(503, b"");
        assert!(busy.is_transient(), "{busy:?}");
        assert!(from_status(429, b"").is_transient());
    }

    #[test]
    fn method_errors_are_typed() {
        let error = |kind: &str| MethodError {
            kind: kind.into(),
            description: Some("x".into()),
        };
        assert!(matches!(
            error("unknownMethod").into_backend("Email/get"),
            BackendError::Unsupported(_)
        ));
        assert!(matches!(
            error("accountReadOnly").into_backend("Email/set"),
            BackendError::ReadOnly(_)
        ));
        assert!(matches!(
            error("accountNotFound").into_backend("Email/get"),
            BackendError::JmapUnavailable(_)
        ));
        let busy = error("serverUnavailable").into_backend("Email/get");
        assert!(busy.is_transient());
        assert_eq!(
            MethodError::from_args(&serde_json::json!({"type": "forbidden"})).kind,
            "forbidden"
        );
    }

    #[test]
    fn tls_errors_are_typed_like_imap() {
        assert!(matches!(
            from_tls(&rustls::Error::InvalidCertificate(
                CertificateError::UnknownIssuer
            )),
            BackendError::CertUntrusted(_)
        ));
        assert!(matches!(
            from_tls(&rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName
            )),
            BackendError::CertNameMismatch
        ));
        assert!(matches!(
            from_tls(&rustls::Error::InvalidCertificate(
                CertificateError::Expired
            )),
            BackendError::CertExpired
        ));
        assert_eq!(short(&"x".repeat(200)).chars().count(), MAX_DETAIL + 1);
    }
}
