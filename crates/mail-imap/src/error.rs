//! Maps IO, TLS and async-imap errors onto [`BackendError`]. Error texts are shortened and never
//! contain the password; server texts and response prefixes are kept short because they can
//! carry message data.

use std::io;

use mail_model::BackendError;

/// Longest detail text taken over from a server response or a library error.
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

/// TLS errors. The platform verifier reports an unknown CA as `UnknownIssuer` (or `Other` with a
/// system code); both mean "not trusted".
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

/// IO errors of an established or establishing connection.
pub(crate) fn from_io(err: &io::Error) -> BackendError {
    if let Some(tls) = err
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
    {
        return from_tls(tls);
    }
    use io::ErrorKind as K;
    let text = err.to_string();
    match err.kind() {
        K::TimedOut => BackendError::Timeout(short(&text)),
        K::ConnectionRefused => BackendError::ConnectionRefused,
        K::UnexpectedEof
        | K::ConnectionReset
        | K::ConnectionAborted
        | K::BrokenPipe
        | K::NotConnected => BackendError::ConnectionLost(short(&text)),
        // async-imap reports what imap-proto could not parse as "... during parsing of <data>".
        _ if text.contains("during parsing") => {
            let data = text
                .split_once("during parsing of ")
                .map_or("", |(_, data)| data);
            BackendError::Protocol(format!("unparseable response: {}", short_prefix(data, 60)))
        }
        kind => BackendError::ConnectionLost(short(&format!("{kind:?}: {text}"))),
    }
}

fn short_prefix(text: &str, chars: usize) -> String {
    let prefix: String = text.chars().take(chars).collect();
    short(&prefix)
}

/// Errors of TCP connect (after name resolution).
pub(crate) fn from_connect(err: &io::Error) -> BackendError {
    match err.kind() {
        io::ErrorKind::ConnectionRefused => BackendError::ConnectionRefused,
        io::ErrorKind::TimedOut => BackendError::ConnectTimeout,
        _ => BackendError::ConnectionLost(short(&format!("could not connect: {err}"))),
    }
}

/// Errors of async-imap calls the adapter still makes (LOGIN, STARTTLS).
pub(crate) fn from_imap(err: async_imap::error::Error) -> BackendError {
    use async_imap::error::Error as E;
    match err {
        E::Io(io) => from_io(&io),
        E::ConnectionLost => BackendError::ConnectionLost("server closed the connection".into()),
        E::No(text) => BackendError::Rejected(format!("NO {}", status_text(&text))),
        E::Bad(text) => BackendError::Rejected(format!("BAD {}", status_text(&text))),
        other => BackendError::Protocol(short(&other.to_string())),
    }
}

/// async-imap formats a refusal as `code: Some(..), info: Some("text")`; keeps the readable part.
pub(crate) fn status_text(formatted: &str) -> String {
    let Some((code, info)) = formatted
        .strip_prefix("code: ")
        .and_then(|rest| rest.split_once(", info: "))
    else {
        return short(formatted);
    };
    let info = info
        .strip_prefix("Some(\"")
        .and_then(|s| s.strip_suffix("\")"))
        .map(|s| s.replace("\\\"", "\""))
        .unwrap_or_default();
    match code {
        "None" => short(&info),
        code => short(&format!("{code} {info}")),
    }
}

#[cfg(test)]
mod tests {
    use rustls::CertificateError;

    use super::*;

    #[test]
    fn certificate_errors_are_typed() {
        let cases = [
            (CertificateError::UnknownIssuer, "untrusted"),
            (CertificateError::BadSignature, "untrusted"),
            (CertificateError::NotValidForName, "name"),
            (CertificateError::Expired, "expired"),
            (CertificateError::NotValidYet, "expired"),
        ];
        for (cert, want) in cases {
            let mapped = from_tls(&rustls::Error::InvalidCertificate(cert));
            let got = match mapped {
                BackendError::CertUntrusted(_) => "untrusted",
                BackendError::CertNameMismatch => "name",
                BackendError::CertExpired => "expired",
                _ => "other",
            };
            assert_eq!(got, want);
        }
        // Wrapped in an io::Error, as tokio-rustls reports it.
        let io = io::Error::new(
            io::ErrorKind::InvalidData,
            rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer),
        );
        assert!(matches!(from_io(&io), BackendError::CertUntrusted(_)));
        let other = io::Error::new(io::ErrorKind::InvalidData, rustls::Error::DecryptError);
        assert!(matches!(from_io(&other), BackendError::Tls(_)));
    }

    #[test]
    fn io_errors_are_typed() {
        let kinds = [
            (io::ErrorKind::TimedOut, "timeout"),
            (io::ErrorKind::UnexpectedEof, "lost"),
            (io::ErrorKind::ConnectionReset, "lost"),
            (io::ErrorKind::BrokenPipe, "lost"),
            (io::ErrorKind::ConnectionRefused, "refused"),
        ];
        for (kind, want) in kinds {
            let got = match from_io(&io::Error::new(kind, "x")) {
                BackendError::Timeout(_) => "timeout",
                BackendError::ConnectionLost(_) => "lost",
                BackendError::ConnectionRefused => "refused",
                _ => "other",
            };
            assert_eq!(got, want, "{kind:?}");
        }
        let huge = format!(
            "Error(..) during parsing of \"* 1 FETCH (BODY[] {{{}}}\r\n{}\"",
            9999,
            "Subject: private ".repeat(1000)
        );
        let BackendError::Protocol(detail) = from_io(&io::Error::other(huge)) else {
            panic!("parse errors are protocol errors");
        };
        assert!(detail.len() < 120, "{detail}");
        assert!(!detail.contains('\r'));
    }

    #[test]
    fn refusal_texts_are_cleaned() {
        assert_eq!(
            status_text(
                r#"code: None, info: Some("[AUTHENTICATIONFAILED] Authentication failed.")"#
            ),
            "[AUTHENTICATIONFAILED] Authentication failed."
        );
        assert_eq!(
            status_text(r#"code: Some(Alert), info: Some("say \"hi\"")"#),
            "Some(Alert) say \"hi\""
        );
        assert_eq!(status_text("anything else"), "anything else");
        assert_eq!(short(&"x".repeat(500)).chars().count(), MAX_DETAIL + 1);
        assert_eq!(short("a\r\nb"), "a  b");
    }
}
