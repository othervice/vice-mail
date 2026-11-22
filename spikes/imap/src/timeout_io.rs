//! Read/write inactivity timeouts for any AsyncRead + AsyncWrite stream.
//!
//! async-imap has no timeouts of its own. A dead TCP connection (sleep, network change, silent
//! middlebox) would make every pending read hang forever. This wrapper turns "no byte for N
//! seconds" into `io::ErrorKind::TimedOut`, which async-imap passes through unchanged.
//! The timeout can be changed at runtime through the shared handle (e.g. longer during IDLE).
//!
//! `tokio-io-timeout` does the same, but its last release is older than 12 months, so the
//! project rules exclude it; the wrapper is small enough to own.

use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{Instant, Sleep};

#[derive(Clone, Debug)]
pub struct TimeoutHandle(Arc<(AtomicU64, AtomicU64)>);

impl TimeoutHandle {
    pub fn new(read: Duration, write: Duration) -> Self {
        Self(Arc::new((
            AtomicU64::new(read.as_millis() as u64),
            AtomicU64::new(write.as_millis() as u64),
        )))
    }
    /// `Duration::ZERO` disables the read timeout.
    pub fn set_read(&self, d: Duration) {
        self.0.0.store(d.as_millis() as u64, Ordering::Relaxed);
    }
    pub fn read(&self) -> Duration {
        Duration::from_millis(self.0.0.load(Ordering::Relaxed))
    }
    fn write(&self) -> Duration {
        Duration::from_millis(self.0.1.load(Ordering::Relaxed))
    }
}

pub struct IoTimeout<S> {
    inner: S,
    handle: TimeoutHandle,
    read_deadline: Option<Pin<Box<Sleep>>>,
    write_deadline: Option<Pin<Box<Sleep>>>,
}

impl<S> IoTimeout<S> {
    pub fn new(inner: S, handle: TimeoutHandle) -> Self {
        Self {
            inner,
            handle,
            read_deadline: None,
            write_deadline: None,
        }
    }
    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> fmt::Debug for IoTimeout<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IoTimeout").field("read", &self.handle.read()).finish()
    }
}

/// Arms (or keeps) a deadline after the inner stream returned Pending and checks it.
fn poll_deadline(
    slot: &mut Option<Pin<Box<Sleep>>>,
    limit: Duration,
    cx: &mut Context<'_>,
    what: &str,
) -> Poll<io::Result<()>> {
    if limit.is_zero() {
        *slot = None;
        return Poll::Pending;
    }
    let sleep = slot.get_or_insert_with(|| Box::pin(tokio::time::sleep_until(Instant::now() + limit)));
    match sleep.as_mut().poll(cx) {
        Poll::Ready(()) => {
            *slot = None;
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("no data {what} for {} ms", limit.as_millis()),
            )))
        }
        Poll::Pending => Poll::Pending,
    }
}

use std::future::Future;

impl<S: AsyncRead + Unpin> AsyncRead for IoTimeout<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(r) => {
                this.read_deadline = None;
                Poll::Ready(r)
            }
            Poll::Pending => {
                let limit = this.handle.read();
                poll_deadline(&mut this.read_deadline, limit, cx, "received")
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for IoTimeout<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(r) => {
                this.write_deadline = None;
                Poll::Ready(r)
            }
            Poll::Pending => {
                let limit = this.handle.write();
                poll_deadline(&mut this.write_deadline, limit, cx, "sent").map_ok(|()| 0)
            }
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Ready(r) => {
                this.write_deadline = None;
                Poll::Ready(r)
            }
            Poll::Pending => {
                let limit = this.handle.write();
                poll_deadline(&mut this.write_deadline, limit, cx, "flushed")
            }
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}
