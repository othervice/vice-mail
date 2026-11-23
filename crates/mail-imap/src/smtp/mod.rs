//! SMTP submission (stage 5): [`SmtpSender`] hands a built message to the user's SMTP server.
//!
//! - **Transport** as for IMAP: TCP, then TLS from the first byte (port 465) or STARTTLS (port
//!   587, never continues unencrypted), certificates checked by the operating system plus the
//!   test CA from [`ConnectConfig::extra_root_ca`], read and write limits on every byte.
//! - **Protocol:** lettre's low-level `AsyncSmtpConnection` on that stream (EHLO, AUTH PLAIN or
//!   LOGIN, MAIL, RCPT, DATA, dot-stuffing, reply parsing). The transaction is driven step by
//!   step, so every recipient gets its own answer, and a refused recipient abandons the whole
//!   message (nobody gets it; the user fixes the address and sends once more).
//! - **Unclear outcome:** if the connection ends after the last byte of the message went out
//!   but before the server's reply, the result is [`SendOutcome::Unclear`]: the message may
//!   have been accepted and must not be sent again automatically. If it ends earlier, nothing
//!   was delivered and the call fails with a transient [`BackendError`]. See `stream` for how
//!   the two are told apart. The sender itself never retries.
//! - **Privacy:** EHLO sends the address literal `[127.0.0.1]`, not the computer's name.
//!   lettre's `tracing` feature stays off (it would log AUTH lines and message data); this
//!   module logs reply codes and sizes only.
//! - **Accounts that sign in with Microsoft or Google** (stage 14, decision 0042) log in with
//!   `AUTH XOAUTH2`, or `AUTH OAUTHBEARER` where only that is offered; see [`bearer_auth`].
//! - **Read-only accounts** cannot send: [`SmtpSender::send`] refuses a configuration with
//!   [`Access::ReadOnly`] before connecting.

mod plain;
mod stream;

#[cfg(test)]
mod tests;

use std::error::Error as _;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::AsyncSmtpConnection;
use lettre::transport::smtp::commands::{Data, Ehlo, Mail, Rcpt};
use lettre::transport::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter};
use lettre::transport::smtp::response::{Response, Severity};
use mail_model::{
    Access, BackendError, ConnectConfig, Envelope, LoginAuth, RecipientRefusal, Secret, Security,
    SendOutcome, SendRejection, SmtpLimits, SmtpReply, SmtpStep,
};
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout, timeout_at};
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use self::stream::{Probe, STARTTLS_GREETING, SmtpStream};
use crate::error::{from_connect, from_io, short};
use crate::timeout_io::{IoTimeout, TimeoutHandle};
use crate::tls::client_config;

/// EHLO argument: an address literal instead of the computer's name (RFC 5321, 4.1.4).
const HELLO: ClientId = ClientId::Ipv4(Ipv4Addr::LOCALHOST);
/// How long a polite QUIT may take after the outcome is known.
const QUIT_LIMIT: Duration = Duration::from_secs(2);

/// Sends messages over SMTP (see module docs). Cheap to clone; holds no connection, every call
/// opens its own.
#[derive(Debug, Clone, Default)]
pub struct SmtpSender {
    #[cfg(feature = "test-support")]
    hooks: crate::test_support::Hooks,
}

/// One authenticated SMTP connection.
struct Session {
    conn: AsyncSmtpConnection,
    probe: Arc<Probe>,
}

impl SmtpSender {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records every byte the connections of this sender write (inside TLS; the AUTH line too,
    /// so only with test credentials). Tests only.
    #[cfg(feature = "test-support")]
    pub fn with_wire_tap(mut self, tap: crate::WireTap) -> Self {
        self.hooks.wire_tap = Some(tap);
        self
    }

    /// Verifies the certificate against `name` instead of the host. Tests only.
    #[cfg(feature = "test-support")]
    pub fn with_tls_server_name(mut self, name: &str) -> Self {
        self.hooks.tls_server_name = Some(name.to_owned());
        self
    }

    fn tls_name<'a>(&'a self, host: &'a str) -> &'a str {
        #[cfg(feature = "test-support")]
        if let Some(name) = &self.hooks.tls_server_name {
            return name;
        }
        host
    }

    #[cfg_attr(not(feature = "test-support"), allow(clippy::let_and_return))]
    fn limited<S>(&self, inner: S, timeouts: &TimeoutHandle) -> IoTimeout<S> {
        let stream = IoTimeout::new(inner, timeouts.clone());
        #[cfg(feature = "test-support")]
        let stream = stream.with_tap(self.hooks.wire_tap.clone());
        stream
    }

    /// Connects and logs in, then says goodbye: checks server, certificate and password
    /// without sending anything (account setup). Also allowed for read-only accounts.
    pub async fn verify(&self, config: &ConnectConfig) -> Result<(), BackendError> {
        let mut session = self.open(config).await?;
        let _ = timeout(QUIT_LIMIT, session.conn.quit()).await;
        Ok(())
    }

    /// Connects and logs in, asks EHLO once more and reads what the server announces about
    /// message sizes (SIZE, RFC 1870), then says goodbye. Nothing is sent; also allowed for
    /// read-only accounts (the composer warns before the user presses send).
    pub async fn limits(&self, config: &ConnectConfig) -> Result<SmtpLimits, BackendError> {
        let mut session = self.open(config).await?;
        let reply = session
            .conn
            .command(Ehlo::new(HELLO))
            .await
            .map_err(|err| match reply_of(&err) {
                Some(reply) => BackendError::Rejected(format!("EHLO: {reply}")),
                None => from_lettre(&err),
            })?;
        let limits = SmtpLimits::from_ehlo(reply.message());
        tracing::debug!(max = ?limits.max_message_bytes, "SMTP size limit");
        let _ = timeout(QUIT_LIMIT, session.conn.quit()).await;
        Ok(limits)
    }

    /// Submits `message` (the bytes for sending, without `Bcc`) to the envelope's recipients.
    ///
    /// `Err` means nothing was delivered: connecting, TLS, login, or the connection broke before
    /// the end of the message went out ([`BackendError::is_transient`] says whether trying
    /// again later can help). `Ok` holds what the server did with the message, including
    /// [`SendOutcome::Unclear`], which must never be retried automatically.
    pub async fn send(
        &self,
        config: &ConnectConfig,
        envelope: &Envelope,
        message: &[u8],
    ) -> Result<SendOutcome, BackendError> {
        if config.access == Access::ReadOnly {
            return Err(BackendError::ReadOnly("sending mail".into()));
        }
        let from = envelope_address(&envelope.from)?;
        let recipients = envelope
            .recipients
            .iter()
            .map(|r| envelope_address(r).map(|address| (r.as_str(), address)))
            .collect::<Result<Vec<_>, _>>()?;
        if recipients.is_empty() {
            return Err(BackendError::InvalidState(
                "a message needs at least one recipient".into(),
            ));
        }
        let mut session = self.open(config).await?;
        let outcome = session
            .transaction(from, &recipients, envelope, message)
            .await;
        match &outcome {
            Ok(SendOutcome::Accepted(reply)) => {
                tracing::info!(code = reply.code, bytes = message.len(), "message accepted");
            }
            Ok(SendOutcome::Rejected(rejection)) => {
                tracing::info!(
                    code = rejection.reply.code,
                    step = ?rejection.step,
                    refused = rejection.recipients.len(),
                    "message refused"
                );
            }
            Ok(SendOutcome::Unclear(_)) => {
                tracing::warn!("connection ended before the server confirmed the message");
            }
            Err(err) => tracing::info!(code = ?err.code(), "message not sent"),
        }
        // Say goodbye where the connection is still in step; after a cut or an unclear end
        // it is just dropped.
        if matches!(
            outcome,
            Ok(SendOutcome::Accepted(_) | SendOutcome::Rejected(_))
                | Err(BackendError::Unsupported(_))
        ) {
            let _ = timeout(QUIT_LIMIT, session.conn.quit()).await;
        }
        outcome
    }

    /// TCP, TLS or STARTTLS, greeting, EHLO, AUTH.
    async fn open(&self, config: &ConnectConfig) -> Result<Session, BackendError> {
        let tls = client_config(config.extra_root_ca.as_ref())?;
        // `[::1]` as typed into the host field.
        let host = config
            .host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(&config.host);
        let server_name = ServerName::try_from(self.tls_name(host).to_owned())
            .map_err(|_| BackendError::Dns(format!("not a valid host name: {}", short(host))))?;
        let timeouts = TimeoutHandle::new(config.timeouts.read, config.timeouts.write);
        let limit = config.timeouts.connect;
        let deadline = Instant::now() + limit;
        let tcp = timeout_at(deadline, tcp_connect(host, config.port))
            .await
            .map_err(|_| BackendError::ConnectTimeout)??;
        let peer = tcp
            .peer_addr()
            .unwrap_or_else(|_| SocketAddr::from((Ipv4Addr::UNSPECIFIED, config.port)));

        let (tls_stream, greeting) = match config.security {
            Security::Tls => {
                let tls_stream = timeout_at(deadline, handshake(tls, server_name, tcp))
                    .await
                    .map_err(|_| BackendError::ConnectTimeout)??;
                (tls_stream, &[][..])
            }
            Security::Starttls => {
                let plain = self.limited(tcp, &timeouts);
                let tcp = plain::starttls(plain, &HELLO.to_string()).await?;
                let deadline = Instant::now() + limit;
                let tls_stream = timeout_at(deadline, handshake(tls, server_name, tcp))
                    .await
                    .map_err(|_| BackendError::ConnectTimeout)??;
                (tls_stream, STARTTLS_GREETING)
            }
        };
        let probe = Arc::new(Probe::default());
        let stream = SmtpStream::new(
            self.limited(tls_stream, &timeouts),
            peer,
            greeting,
            Arc::clone(&probe),
        );
        let mut conn = AsyncSmtpConnection::connect_with_transport(Box::new(stream), &HELLO)
            .await
            .map_err(|err| match reply_of(&err) {
                Some(reply) => {
                    BackendError::Rejected(format!("server refused the connection: {reply}"))
                }
                None => from_lettre(&err),
            })?;
        if config.auth == LoginAuth::Bearer {
            bearer_auth(&mut conn, config).await?;
            return Ok(Session { conn, probe });
        }
        let credentials = Credentials::new(
            config.username.clone(),
            config.password.expose_secret().clone(),
        );
        conn.auth(&[Mechanism::Plain, Mechanism::Login], &credentials)
            .await
            .map_err(|err| match reply_of(&err) {
                Some(reply) if reply.is_permanent() => BackendError::AuthFailed(reply.to_string()),
                Some(reply) => BackendError::Rejected(format!("login: {reply}")),
                None if err.is_client() => {
                    BackendError::Unsupported("AUTH PLAIN or AUTH LOGIN".into())
                }
                None => from_lettre(&err),
            })?;
        Ok(Session { conn, probe })
    }
}

/// One SASL line of the bearer login. `Debug` leaves the line out (it holds the token).
struct SaslLine(Secret<String>);

impl std::fmt::Display for SaslLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.expose_secret())?;
        f.write_str("\r\n")
    }
}

/// Stage 14 (decision 0042): logs in with the OAuth 2.0 access token in `config.password`.
/// `XOAUTH2` when the server announces it (Microsoft, Google), otherwise `OAUTHBEARER`
/// (RFC 7628) if a second EHLO shows it (lettre does not parse that name). The token is sent
/// as the initial response; a `334` is the server's error challenge, answered as the
/// mechanism prescribes, after which the server's final reply is the error.
async fn bearer_auth(
    conn: &mut AsyncSmtpConnection,
    config: &ConnectConfig,
) -> Result<(), BackendError> {
    use crate::sasl::Bearer;
    let mechanism = if conn
        .server_info()
        .supports_auth_mechanism(Mechanism::Xoauth2)
    {
        Bearer::XOauth2
    } else {
        let reply = conn
            .command(Ehlo::new(HELLO))
            .await
            .map_err(|err| match reply_of(&err) {
                Some(reply) => BackendError::Rejected(format!("EHLO: {reply}")),
                None => from_lettre(&err),
            })?;
        let offers_oauthbearer = reply.message().any(|line| {
            let mut words = line.split_ascii_whitespace();
            words
                .next()
                .is_some_and(|first| first.eq_ignore_ascii_case("AUTH"))
                && words.any(|word| word.eq_ignore_ascii_case("OAUTHBEARER"))
        });
        if !offers_oauthbearer {
            return Err(BackendError::Unsupported(
                "AUTH XOAUTH2 or OAUTHBEARER".into(),
            ));
        }
        Bearer::OAuthBearer
    };
    let message = mechanism.message(
        &config.username,
        &config.host,
        config.port,
        &config.password,
    );
    let line = SaslLine(Secret::new(format!(
        "AUTH {} {}",
        mechanism.name(),
        crate::sasl::base64(message.expose_secret())
    )));
    let refused = |err: lettre::transport::smtp::Error| match reply_of(&err) {
        Some(reply) if reply.is_permanent() => BackendError::AuthFailed(reply.to_string()),
        Some(reply) => BackendError::Rejected(format!("login: {reply}")),
        None => from_lettre(&err),
    };
    let response = conn.command(line).await.map_err(refused)?;
    if !response.has_code(334) {
        return Ok(());
    }
    // The error challenge (a base64 JSON status, nothing secret); the final reply follows.
    let ack = SaslLine(Secret::new(crate::sasl::base64(mechanism.error_ack())));
    match conn.command(ack).await {
        Err(err) => Err(refused(err)),
        Ok(response) => Err(BackendError::AuthFailed(format!(
            "the server rejected the access token ({})",
            u16::from(response.code())
        ))),
    }
}

impl Session {
    async fn transaction(
        &mut self,
        from: lettre::Address,
        recipients: &[(&str, lettre::Address)],
        envelope: &Envelope,
        message: &[u8],
    ) -> Result<SendOutcome, BackendError> {
        let info = self.conn.server_info().clone();
        let mut parameters = Vec::new();
        let non_ascii_address =
            !envelope.from.is_ascii() || envelope.recipients.iter().any(|r| !r.is_ascii());
        if non_ascii_address {
            if !info.supports_feature(Extension::SmtpUtfEight) {
                return Err(BackendError::Unsupported(
                    "SMTPUTF8 (an address with non-ASCII characters)".into(),
                ));
            }
            parameters.push(MailParameter::SmtpUtfEight);
        }
        if !message.is_ascii() {
            if !info.supports_feature(Extension::EightBitMime) {
                return Err(BackendError::Unsupported("8BITMIME".into()));
            }
            parameters.push(MailParameter::Body(MailBodyParameter::EightBitMime));
        }

        if let Err(err) = self.conn.command(Mail::new(Some(from), parameters)).await {
            return refused(&err, SmtpStep::MailFrom, Vec::new());
        }
        let mut refusals = Vec::new();
        for (address, recipient) in recipients {
            match self
                .conn
                .command(Rcpt::new(recipient.clone(), Vec::new()))
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    let Some(reply) = reply_of(&err) else {
                        return Err(from_lettre(&err));
                    };
                    // 421: the server is closing the connection; no further answers come.
                    let closing = reply.code == 421;
                    refusals.push(RecipientRefusal {
                        address: (*address).to_owned(),
                        reply,
                    });
                    if closing {
                        break;
                    }
                }
            }
        }
        if let Some(first) = refusals.first() {
            return Ok(SendOutcome::Rejected(SendRejection {
                step: SmtpStep::Recipients,
                reply: first.reply.clone(),
                recipients: refusals,
            }));
        }
        match self.conn.command(Data).await {
            Ok(reply) if reply.code().severity == Severity::PositiveIntermediate => {}
            Ok(reply) => {
                return Err(BackendError::Protocol(format!(
                    "unexpected reply to DATA: {}",
                    u16::from(reply.code())
                )));
            }
            Err(err) => return refused(&err, SmtpStep::Data, Vec::new()),
        }

        self.probe.start_data();
        match self.conn.message(message).await {
            Ok(reply) if reply.code().severity == Severity::PositiveCompletion => {
                Ok(SendOutcome::Accepted(reply_of_response(&reply)))
            }
            // A positive reply that is no confirmation: out of step with the server.
            Ok(reply) => Ok(SendOutcome::Unclear(format!(
                "unexpected reply to the end of the message: {}",
                reply_of_response(&reply)
            ))),
            Err(err) => match reply_of(&err) {
                Some(reply) => Ok(SendOutcome::Rejected(SendRejection {
                    step: SmtpStep::Message,
                    reply,
                    recipients: Vec::new(),
                })),
                None if self.probe.reply_awaited() => {
                    let err = from_lettre(&err);
                    Ok(SendOutcome::Unclear(
                        err.detail().unwrap_or_else(|| err.to_string()),
                    ))
                }
                None => Err(from_lettre(&err)),
            },
        }
    }
}

/// The engine sends through this trait (decision 0018), so tests can hand it an instrumented
/// sender.
impl mail_model::MailSender for SmtpSender {
    fn send<'a>(
        &'a self,
        config: &'a ConnectConfig,
        envelope: &'a Envelope,
        message: &'a [u8],
    ) -> mail_model::SendFuture<'a, SendOutcome> {
        Box::pin(SmtpSender::send(self, config, envelope, message))
    }

    fn verify<'a>(&'a self, config: &'a ConnectConfig) -> mail_model::SendFuture<'a, ()> {
        Box::pin(SmtpSender::verify(self, config))
    }

    fn limits<'a>(&'a self, config: &'a ConnectConfig) -> mail_model::SendFuture<'a, SmtpLimits> {
        Box::pin(SmtpSender::limits(self, config))
    }
}

/// The outcome of a refused command, or the connection error if there was no reply.
fn refused(
    err: &lettre::transport::smtp::Error,
    step: SmtpStep,
    recipients: Vec<RecipientRefusal>,
) -> Result<SendOutcome, BackendError> {
    match reply_of(err) {
        Some(reply) => Ok(SendOutcome::Rejected(SendRejection {
            step,
            reply,
            recipients,
        })),
        None => Err(from_lettre(err)),
    }
}

fn envelope_address(address: &str) -> Result<lettre::Address, BackendError> {
    address.parse().map_err(|err| {
        BackendError::InvalidState(format!(
            "envelope address {:?} is not valid: {err}",
            short(address)
        ))
    })
}

/// The server's reply carried by a lettre error (a 4xx or 5xx answer).
fn reply_of(err: &lettre::transport::smtp::Error) -> Option<SmtpReply> {
    let code = err.status()?;
    let text = err.source().map(ToString::to_string).unwrap_or_default();
    Some(reply(u16::from(code), &text))
}

fn reply_of_response(response: &Response) -> SmtpReply {
    let lines: Vec<&str> = response.message().collect();
    let (enhanced, _) = split_enhanced(lines.first().copied().unwrap_or_default());
    // Later lines repeat the enhanced code; keep it once.
    let joined = lines
        .iter()
        .enumerate()
        .map(|(i, line)| match (&enhanced, i) {
            (Some(code), 1..) => line
                .strip_prefix(code.as_str())
                .map_or(*line, str::trim_start),
            _ => line,
        })
        .collect::<Vec<_>>()
        .join(" ");
    reply(u16::from(response.code()), &joined)
}

fn reply(code: u16, text: &str) -> SmtpReply {
    let (enhanced, rest) = split_enhanced(text.trim());
    SmtpReply {
        code,
        enhanced,
        text: short(rest),
    }
}

/// An enhanced status code (RFC 3463, `class.subject.detail`) at the start of a reply text.
fn split_enhanced(text: &str) -> (Option<String>, &str) {
    let (first, rest) = text.split_once(' ').unwrap_or((text, ""));
    let parts: Vec<&str> = first.split('.').collect();
    let valid = parts.len() == 3
        && matches!(parts.first(), Some(&("2" | "4" | "5")))
        && parts
            .iter()
            .skip(1)
            .all(|p| (1..=3).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()));
    if valid {
        (Some(first.to_owned()), rest.trim_start())
    } else {
        (None, text)
    }
}

/// A lettre error without a server reply: IO and TLS errors through the cause chain (lettre
/// wraps them), a connection that ended inside or before a reply, anything else as protocol.
fn from_lettre(err: &lettre::transport::smtp::Error) -> BackendError {
    let mut source = err.source();
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>() {
            return from_io(io);
        }
        if let Some(tls) = cause.downcast_ref::<rustls::Error>() {
            return crate::error::from_tls(tls);
        }
        source = cause.source();
    }
    let text = err.to_string();
    if let Some(reply) = reply_of(err) {
        return BackendError::Rejected(reply.to_string());
    }
    if err.is_response() && text.contains("incomplete response") {
        return BackendError::ConnectionLost("server closed the connection".into());
    }
    BackendError::Protocol(short(&text))
}

async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, BackendError> {
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| BackendError::Dns(short(&format!("{host}: {err}"))))?;
    let mut last = None;
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                if let Err(err) = stream.set_nodelay(true) {
                    tracing::debug!(error = %err, "TCP_NODELAY not set");
                }
                return Ok(stream);
            }
            Err(err) => last = Some(err),
        }
    }
    Err(match last {
        Some(err) => from_connect(&err),
        None => BackendError::Dns(format!("{} has no address", short(host))),
    })
}

async fn handshake(
    config: Arc<rustls::ClientConfig>,
    name: ServerName<'static>,
    tcp: TcpStream,
) -> Result<TlsStream<TcpStream>, BackendError> {
    TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|err| from_io(&err))
}
