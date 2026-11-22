//! [`ImapConnector`]: TCP, TLS or STARTTLS, greeting, then login in [`ImapBackend::establish`].

use std::sync::Arc;

use async_imap::Client;
use async_imap::imap_proto::{ResponseCode, Status};
use mail_model::{BackendError, ConnectConfig, MailConnector, Security};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout_at};
use tokio_rustls::TlsConnector;

use crate::backend::{ImapBackend, TlsTransport};
use crate::command::{Response, Transport, permit};
use crate::convert::capability_names;
use crate::error::{from_connect, from_imap, from_io, short};
use crate::timeout_io::{IoTimeout, TimeoutHandle};
use crate::tls::client_config;

/// Opens IMAP connections (see [`MailConnector`]). Cheap to clone; holds no connection.
#[derive(Debug, Clone, Default)]
pub struct ImapConnector {
    #[cfg(feature = "test-support")]
    hooks: crate::test_support::Hooks,
}

impl ImapConnector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records every byte the connections of this connector write (inside TLS). Tests only.
    #[cfg(feature = "test-support")]
    pub fn with_wire_tap(mut self, tap: crate::WireTap) -> Self {
        self.hooks.wire_tap = Some(tap);
        self
    }

    /// Verifies the certificate against `name` instead of the host. Tests only (to provoke a
    /// name mismatch on a server that answers on 127.0.0.1).
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
    fn stream<S>(&self, inner: S, timeouts: &TimeoutHandle) -> IoTimeout<S> {
        let stream = IoTimeout::new(inner, timeouts.clone());
        #[cfg(feature = "test-support")]
        let stream = stream.with_tap(self.hooks.wire_tap.clone());
        stream
    }
}

impl MailConnector for ImapConnector {
    type Backend = ImapBackend<TlsTransport>;

    async fn connect(&self, config: &ConnectConfig) -> Result<Self::Backend, BackendError> {
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

        let client = match config.security {
            Security::Tls => {
                let tls = timeout_at(deadline, handshake(tls, server_name, tcp))
                    .await
                    .map_err(|_| BackendError::ConnectTimeout)??;
                let mut client = Client::new(self.stream(tls, &timeouts));
                read_greeting(&mut client).await?;
                client
            }
            Security::Starttls => {
                // Plaintext phase: greeting and STARTTLS, nothing else.
                let mut plain = Client::new(self.stream(tcp, &timeouts));
                let caps = read_greeting(&mut plain).await?;
                if caps
                    .as_ref()
                    .is_some_and(|caps| !caps.iter().any(|c| c.eq_ignore_ascii_case("STARTTLS")))
                {
                    return Err(BackendError::StarttlsUnavailable);
                }
                permit(config.access, "STARTTLS")?;
                plain
                    .run_command_and_check_ok("STARTTLS", None)
                    .await
                    .map_err(|err| match from_imap(err) {
                        BackendError::Rejected(_) => BackendError::StarttlsUnavailable,
                        other => other,
                    })?;
                // `into_inner` drops async-imap's read buffer: bytes a man in the middle
                // injected before the handshake are discarded.
                let tcp = plain.into_inner().into_inner();
                let deadline = Instant::now() + limit;
                let tls = timeout_at(deadline, handshake(tls, server_name, tcp))
                    .await
                    .map_err(|_| BackendError::ConnectTimeout)??;
                Client::new(self.stream(tls, &timeouts))
            }
        };
        ImapBackend::establish(client, timeouts, config).await
    }
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
    config: Arc<ClientConfig>,
    name: ServerName<'static>,
    tcp: TcpStream,
) -> Result<TlsTransport, BackendError> {
    TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|err| from_io(&err))
}

/// Reads the greeting; returns the capabilities it carried, if any.
pub(crate) async fn read_greeting<S: Transport>(
    client: &mut Client<IoTimeout<S>>,
) -> Result<Option<Vec<String>>, BackendError> {
    let greeting = match client.read_response().await {
        Ok(Some(resp)) => resp,
        Ok(None) => {
            return Err(BackendError::ConnectionLost(
                "server closed the connection before the greeting".into(),
            ));
        }
        Err(err) => return Err(from_io(&err)),
    };
    match greeting.parsed() {
        Response::Data {
            status: Status::Ok,
            outcome,
        } => Ok(match &outcome.code {
            Some(ResponseCode::Capabilities(caps)) => Some(capability_names(caps)),
            _ => None,
        }),
        Response::Data {
            status: Status::Bye,
            outcome,
        } => Err(BackendError::Rejected(format!(
            "server refused the connection: {}",
            short(outcome.information.as_deref().unwrap_or_default())
        ))),
        Response::Data {
            status: Status::PreAuth,
            ..
        } => Err(BackendError::Protocol(
            "server greeted with PREAUTH; login is not possible".into(),
        )),
        _ => Err(BackendError::Protocol("unexpected greeting".into())),
    }
}
