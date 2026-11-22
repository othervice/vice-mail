//! The stream lettre's SMTP connection runs on: the adapter's own TLS stream (rustls with the
//! system verifier, read and write limits), plus two things lettre cannot provide.
//!
//! - **Probe:** whether the whole message, end-of-data marker included, has gone out. lettre's
//!   `message()` writes the data and the marker (`write_all` and `flush` each) and only then
//!   reads the reply; before that it never reads. So the first read after
//!   [`Probe::start_data`] means the last byte was handed to the socket. A failure before that
//!   read cannot have delivered the message (the server never saw a complete end marker); a
//!   failure after it can have (the outcome is unclear).
//! - **Greeting after STARTTLS:** the adapter does the plaintext part of STARTTLS itself (so TLS
//!   is set up in one place, as for IMAP) and hands lettre the encrypted stream. lettre always
//!   reads a greeting first and then sends EHLO; after STARTTLS the server sends no new
//!   greeting, but the client must send EHLO again (RFC 3207, section 4.2). The stream
//!   therefore serves one local `220` line before the first byte from the server. It never
//!   reaches the network.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::task::{Context, Poll};

use lettre::transport::smtp::client::AsyncTokioStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::timeout_io::IoTimeout;

/// Served to lettre as the greeting after STARTTLS (see module docs).
pub(super) const STARTTLS_GREETING: &[u8] = b"220 TLS established\r\n";

const COMMANDS: u8 = 0;
const DATA: u8 = 1;
const REPLY_AWAITED: u8 = 2;

/// See module docs. Shared between the sender and the stream inside lettre.
#[derive(Debug)]
pub(super) struct Probe(AtomicU8);

impl Default for Probe {
    fn default() -> Self {
        Self(AtomicU8::new(COMMANDS))
    }
}

impl Probe {
    /// Called right before the message data is handed to lettre.
    pub(super) fn start_data(&self) {
        self.0.store(DATA, Ordering::SeqCst);
    }

    /// The whole message has been written and flushed: a read for the reply has started.
    pub(super) fn reply_awaited(&self) -> bool {
        self.0.load(Ordering::SeqCst) == REPLY_AWAITED
    }

    fn on_read(&self) {
        // Only the step from DATA counts; reads during commands change nothing.
        let _ = self
            .0
            .compare_exchange(DATA, REPLY_AWAITED, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// The encrypted connection as lettre sees it.
pub(super) struct SmtpStream {
    /// `std::sync::Mutex` only to make the stream `Sync`, as lettre requires; it is never
    /// locked, every access goes through `get_mut`.
    inner: std::sync::Mutex<IoTimeout<TlsStream<TcpStream>>>,
    peer: SocketAddr,
    greeting: &'static [u8],
    probe: Arc<Probe>,
}

impl SmtpStream {
    pub(super) fn new(
        inner: IoTimeout<TlsStream<TcpStream>>,
        peer: SocketAddr,
        greeting: &'static [u8],
        probe: Arc<Probe>,
    ) -> Self {
        Self {
            inner: std::sync::Mutex::new(inner),
            peer,
            greeting,
            probe,
        }
    }

    fn inner(&mut self) -> Pin<&mut IoTimeout<TlsStream<TcpStream>>> {
        // A poisoned lock cannot happen (never locked), but would hold a usable stream anyway.
        Pin::new(
            self.inner
                .get_mut()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

impl fmt::Debug for SmtpStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpStream")
            .field("peer", &self.peer)
            .field("greeting_left", &self.greeting.len())
            .finish_non_exhaustive()
    }
}

impl AsyncRead for SmtpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        if !this.greeting.is_empty() {
            let n = this.greeting.len().min(buf.remaining());
            let (now, later) = this.greeting.split_at(n);
            buf.put_slice(now);
            this.greeting = later;
            return Poll::Ready(Ok(()));
        }
        this.probe.on_read();
        this.inner().poll_read(cx, buf)
    }
}

impl AsyncWrite for SmtpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.inner().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner().poll_shutdown(cx)
    }
}

impl AsyncTokioStream for SmtpStream {
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.peer)
    }
}
