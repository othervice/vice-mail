//! Typed errors as a sketch for `mail-model`: every failure the spike provokes ends up in one
//! of these variants, so the UI (stage 2) can show a specific, understandable message.

use std::io;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("connection attempt timed out")]
    ConnectTimeout,
    #[error("connection refused")]
    Refused,
    #[error("server certificate is not trusted ({0})")]
    CertUntrusted(String),
    #[error("server certificate does not match the host name")]
    CertNameMismatch,
    #[error("server certificate has expired or is not yet valid")]
    CertExpired,
    #[error("TLS failure: {0}")]
    Tls(String),
    #[error("server does not offer STARTTLS")]
    StartTlsUnavailable,
    #[error("no data from the server within the time limit ({0})")]
    Timeout(String),
    #[error("connection lost ({0})")]
    ConnectionLost(String),
    #[error("login rejected")]
    AuthFailed,
    #[error("server refused the command: {0}")]
    No(String),
    #[error("server rejected the command as invalid: {0}")]
    Bad(String),
    #[error("could not understand the server response: {0}")]
    Protocol(String),
    #[error("writes are disabled for this account")]
    ReadOnly,
}

impl NetError {
    pub fn kind(&self) -> &'static str {
        match self {
            NetError::ConnectTimeout => "ConnectTimeout",
            NetError::Refused => "Refused",
            NetError::CertUntrusted(_) => "CertUntrusted",
            NetError::CertNameMismatch => "CertNameMismatch",
            NetError::CertExpired => "CertExpired",
            NetError::Tls(_) => "Tls",
            NetError::StartTlsUnavailable => "StartTlsUnavailable",
            NetError::Timeout(_) => "Timeout",
            NetError::ConnectionLost(_) => "ConnectionLost",
            NetError::AuthFailed => "AuthFailed",
            NetError::No(_) => "No",
            NetError::Bad(_) => "Bad",
            NetError::Protocol(_) => "Protocol",
            NetError::ReadOnly => "ReadOnly",
        }
    }
    /// Worth retrying with backoff (network trouble), as opposed to configuration or server errors.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            NetError::ConnectTimeout | NetError::Refused | NetError::Timeout(_) | NetError::ConnectionLost(_)
        )
    }
}

pub fn from_tls(e: &rustls::Error) -> NetError {
    use rustls::CertificateError as C;
    match e {
        rustls::Error::InvalidCertificate(ce) => match ce {
            C::NotValidForName | C::NotValidForNameContext { .. } => NetError::CertNameMismatch,
            C::Expired | C::ExpiredContext { .. } | C::NotValidYet | C::NotValidYetContext { .. } => {
                NetError::CertExpired
            }
            // UnknownIssuer comes from webpki; the Apple verifier reports an untrusted root as
            // Other("... -67843" = errSecNotTrusted) or UnknownIssuer depending on the chain.
            other => NetError::CertUntrusted(format!("{other:?}")),
        },
        other => NetError::Tls(other.to_string()),
    }
}

pub fn from_io(e: &io::Error) -> NetError {
    if let Some(te) = e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()) {
        return from_tls(te);
    }
    use io::ErrorKind as K;
    match e.kind() {
        K::TimedOut => NetError::Timeout(e.to_string()),
        K::ConnectionRefused => NetError::Refused,
        K::UnexpectedEof | K::ConnectionReset | K::ConnectionAborted | K::BrokenPipe | K::NotConnected => {
            NetError::ConnectionLost(e.to_string())
        }
        _ if e.to_string().contains("during parsing") => NetError::Protocol(truncate(&e.to_string())),
        _ => NetError::ConnectionLost(format!("{:?}: {}", e.kind(), truncate(&e.to_string()))),
    }
}

pub fn from_imap(e: async_imap::error::Error) -> NetError {
    use async_imap::error::Error as E;
    match e {
        E::Io(io) => from_io(&io),
        E::ConnectionLost => NetError::ConnectionLost("server closed the connection".into()),
        E::No(s) => NetError::No(s),
        E::Bad(s) => NetError::Bad(s),
        E::Parse(p) => NetError::Protocol(truncate(&p.to_string())),
        E::Validate(v) => NetError::Protocol(v.to_string()),
        E::Append => NetError::Protocol("APPEND was not accepted".into()),
        other => NetError::Protocol(truncate(&other.to_string())),
    }
}

fn truncate(s: &str) -> String {
    if s.len() > 200 {
        let mut end = 200;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    } else {
        s.to_string()
    }
}

impl From<async_imap::error::Error> for NetError {
    fn from(e: async_imap::error::Error) -> Self {
        from_imap(e)
    }
}
impl From<io::Error> for NetError {
    fn from(e: io::Error) -> Self {
        from_io(&e)
    }
}
