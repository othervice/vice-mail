//! Connecting: implicit TLS or STARTTLS, read/write timeouts, greeting, login.

use std::sync::Arc;
use std::time::Duration;

use async_imap::imap_proto::{Response, ResponseCode, Status};
use async_imap::{Client, Session};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

use crate::env::Env;
use crate::errors::{NetError, from_imap, from_io};
use crate::timeout_io::{IoTimeout, TimeoutHandle};

pub type Stream = IoTimeout<TlsStream<TcpStream>>;
pub type Sess = Session<Stream>;

#[derive(Clone)]
pub struct Opts {
    pub host: String,
    pub port: u16,
    pub tls_name: String,
    pub tls: Arc<ClientConfig>,
    pub starttls: bool,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}

impl Opts {
    pub fn imaps(env: &Env, tls: Arc<ClientConfig>) -> Self {
        Opts {
            host: env.host.clone(),
            port: env.imaps_port,
            tls_name: env.tls_name.clone(),
            tls,
            starttls: false,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            write_timeout: Duration::from_secs(30),
        }
    }
    pub fn starttls(env: &Env, tls: Arc<ClientConfig>) -> Self {
        Opts {
            port: env.imap_port,
            starttls: true,
            ..Self::imaps(env, tls)
        }
    }
}

pub struct Connected {
    pub client: Client<Stream>,
    pub timeouts: TimeoutHandle,
    /// Capabilities from the greeting (and, for STARTTLS, from the plain-text greeting).
    pub greeting_caps: Vec<String>,
    pub tls_info: String,
}

fn caps_of(resp: &Response<'_>) -> Option<Vec<String>> {
    match resp {
        Response::Data { outcome, .. } => match &outcome.code {
            Some(ResponseCode::Capabilities(c)) => Some(c.iter().map(|c| format!("{c:?}")).collect()),
            _ => None,
        },
        Response::Capabilities(c) => Some(c.iter().map(|c| format!("{c:?}")).collect()),
        _ => None,
    }
}

async fn read_greeting<S>(client: &mut Client<S>) -> Result<Vec<String>, NetError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + std::fmt::Debug + Send,
{
    let g = client
        .read_response()
        .await
        .map_err(|e| from_io(&e))?
        .ok_or_else(|| NetError::ConnectionLost("closed before greeting".into()))?;
    match g.parsed() {
        Response::Data { status: Status::Ok, .. } => Ok(caps_of(g.parsed()).unwrap_or_default()),
        Response::Data {
            status: Status::Bye,
            outcome,
        } => Err(NetError::ConnectionLost(format!(
            "BYE in greeting: {:?}",
            outcome.information
        ))),
        other => Err(NetError::Protocol(format!("unexpected greeting {other:?}"))),
    }
}

async fn tls_handshake(o: &Opts, tcp: TcpStream) -> Result<(TlsStream<TcpStream>, String), NetError> {
    let name = ServerName::try_from(o.tls_name.clone()).map_err(|e| NetError::Tls(e.to_string()))?;
    let tls = tokio::time::timeout(o.connect_timeout, TlsConnector::from(o.tls.clone()).connect(name, tcp))
        .await
        .map_err(|_| NetError::ConnectTimeout)?
        .map_err(|e| from_io(&e))?;
    let (_, conn) = tls.get_ref();
    let info = format!(
        "{:?} {:?}",
        conn.protocol_version().map(|v| format!("{v:?}")).unwrap_or_default(),
        conn.negotiated_cipher_suite()
            .map(|c| c.suite())
            .map(|s| format!("{s:?}"))
            .unwrap_or_default()
    );
    Ok((tls, info))
}

pub async fn connect(o: &Opts) -> Result<Connected, NetError> {
    let tcp = tokio::time::timeout(o.connect_timeout, TcpStream::connect((o.host.as_str(), o.port)))
        .await
        .map_err(|_| NetError::ConnectTimeout)?
        .map_err(|e| from_io(&e))?;
    tcp.set_nodelay(true).ok();
    let timeouts = TimeoutHandle::new(o.read_timeout, o.write_timeout);

    if o.starttls {
        // Plain-text phase: read greeting, require STARTTLS, upgrade. Nothing else is sent in clear.
        let mut plain = Client::new(IoTimeout::new(tcp, timeouts.clone()));
        let caps = read_greeting(&mut plain).await?;
        if !caps.iter().any(|c| c.contains("STARTTLS")) {
            return Err(NetError::StartTlsUnavailable);
        }
        plain
            .run_command_and_check_ok("STARTTLS", None)
            .await
            .map_err(from_imap)?;
        // into_inner drops async-imap's read buffer: bytes injected before the handshake are discarded.
        let tcp = plain.into_inner().into_inner();
        let (tls, info) = tls_handshake(o, tcp).await?;
        let client = Client::new(IoTimeout::new(tls, timeouts.clone()));
        Ok(Connected {
            client,
            timeouts,
            greeting_caps: caps,
            tls_info: info,
        })
    } else {
        let (tls, info) = tls_handshake(o, tcp).await?;
        let mut client = Client::new(IoTimeout::new(tls, timeouts.clone()));
        let caps = read_greeting(&mut client).await?;
        Ok(Connected {
            client,
            timeouts,
            greeting_caps: caps,
            tls_info: info,
        })
    }
}

pub async fn login(o: &Opts, user: &str, password: &str) -> Result<(Sess, TimeoutHandle), NetError> {
    let c = connect(o).await?;
    let sess = c
        .client
        .login(user, password)
        .await
        .map_err(|(e, _)| match from_imap(e) {
            NetError::No(_) => NetError::AuthFailed,
            other => other,
        })?;
    Ok((sess, c.timeouts))
}

/// Drains everything async-imap parked in the bounded (100) unsolicited channel.
pub fn drain_unsolicited(s: &Sess) -> Vec<async_imap::types::UnsolicitedResponse> {
    let mut v = Vec::new();
    while let Ok(r) = s.unsolicited_responses.try_recv() {
        v.push(r);
    }
    v
}
