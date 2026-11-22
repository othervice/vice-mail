//! Read and write limits for any `AsyncRead + AsyncWrite` stream (decision 0002).
//!
//! async-imap has no time limits of its own: on a dead connection (sleep, network change, a
//! silent middlebox) every pending read would wait forever. [`IoTimeout`] turns "no byte for the
//! limit" into `io::ErrorKind::TimedOut`, which async-imap passes through unchanged. A limit
//! counts time without progress, so a large literal that keeps arriving never times out.
//!
//! The limits live in a shared [`TimeoutHandle`] and can change at runtime: during IDLE the
//! adapter turns the read limit off and waits with its own timer. Changing a limit restarts any
//! armed deadline, so a deadline left over from a cancelled read never fires on a later one.
//!
//! `tokio-io-timeout` does the same, but its last release is older than twelve months, so the
//! project rules exclude it.

use std::fmt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Sleep;

#[cfg(any(test, feature = "test-support"))]
use crate::wire_tap::WireTap;

/// Shared, changeable read and write limits of one connection. `Duration::ZERO` turns a limit off.
#[derive(Clone)]
pub(crate) struct TimeoutHandle(Arc<Limits>);

struct Limits {
    read_ms: AtomicU64,
    write_ms: AtomicU64,
    /// Bumped on every change; deadlines armed under an older epoch are discarded.
    epoch: AtomicU64,
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl TimeoutHandle {
    pub(crate) fn new(read: Duration, write: Duration) -> Self {
        Self(Arc::new(Limits {
            read_ms: AtomicU64::new(millis(read)),
            write_ms: AtomicU64::new(millis(write)),
            epoch: AtomicU64::new(0),
        }))
    }

    pub(crate) fn set_read(&self, limit: Duration) {
        self.0.read_ms.store(millis(limit), Ordering::Relaxed);
        self.0.epoch.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn read(&self) -> Duration {
        Duration::from_millis(self.0.read_ms.load(Ordering::Relaxed))
    }

    pub(crate) fn write(&self) -> Duration {
        Duration::from_millis(self.0.write_ms.load(Ordering::Relaxed))
    }

    fn epoch(&self) -> u64 {
        self.0.epoch.load(Ordering::Relaxed)
    }
}

impl fmt::Debug for TimeoutHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimeoutHandle")
            .field("read", &self.read())
            .field("write", &self.write())
            .finish()
    }
}

type Deadline = Option<(u64, Pin<Box<Sleep>>)>;

/// A stream with read and write limits (see module docs).
pub(crate) struct IoTimeout<S> {
    inner: S,
    limits: TimeoutHandle,
    read_deadline: Deadline,
    write_deadline: Deadline,
    #[cfg(any(test, feature = "test-support"))]
    tap: Option<WireTap>,
}

impl<S> IoTimeout<S> {
    pub(crate) fn new(inner: S, limits: TimeoutHandle) -> Self {
        Self {
            inner,
            limits,
            read_deadline: None,
            write_deadline: None,
            #[cfg(any(test, feature = "test-support"))]
            tap: None,
        }
    }

    /// Records every byte written to the inner stream (tests only).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_tap(mut self, tap: Option<WireTap>) -> Self {
        self.tap = tap;
        self
    }

    pub(crate) fn into_inner(self) -> S {
        self.inner
    }
}

impl<S> fmt::Debug for IoTimeout<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IoTimeout")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

/// Called after the inner stream returned `Pending`: arms the deadline if needed and checks it.
fn poll_deadline(
    slot: &mut Deadline,
    limit: Duration,
    epoch: u64,
    cx: &mut Context<'_>,
    what: &str,
) -> Poll<io::Result<()>> {
    if limit.is_zero() {
        *slot = None;
        return Poll::Pending;
    }
    if slot.as_ref().is_some_and(|(armed, _)| *armed != epoch) {
        *slot = None;
    }
    let (_, sleep) = slot.get_or_insert_with(|| (epoch, Box::pin(tokio::time::sleep(limit))));
    match sleep.as_mut().poll(cx) {
        Poll::Ready(()) => {
            *slot = None;
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("nothing {what} for {} s", limit.as_secs_f32()),
            )))
        }
        Poll::Pending => Poll::Pending,
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for IoTimeout<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(result) => {
                this.read_deadline = None;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let (limit, epoch) = (this.limits.read(), this.limits.epoch());
                poll_deadline(&mut this.read_deadline, limit, epoch, cx, "received")
            }
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for IoTimeout<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_write(cx, buf) {
            Poll::Ready(result) => {
                this.write_deadline = None;
                #[cfg(any(test, feature = "test-support"))]
                if let (Ok(n), Some(tap)) = (&result, &this.tap) {
                    tap.record(buf.get(..*n).unwrap_or(buf));
                }
                Poll::Ready(result)
            }
            Poll::Pending => {
                let (limit, epoch) = (this.limits.write(), this.limits.epoch());
                match poll_deadline(&mut this.write_deadline, limit, epoch, cx, "sent") {
                    Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                    Poll::Ready(Ok(())) | Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_flush(cx) {
            Poll::Ready(result) => {
                this.write_deadline = None;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let (limit, epoch) = (this.limits.write(), this.limits.epoch());
                poll_deadline(&mut this.write_deadline, limit, epoch, cx, "sent")
            }
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_shutdown(cx) {
            Poll::Ready(result) => {
                this.write_deadline = None;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let (limit, epoch) = (this.limits.write(), this.limits.epoch());
                poll_deadline(&mut this.write_deadline, limit, epoch, cx, "sent")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[tokio::test]
    async fn silent_stream_times_out_but_a_slow_one_does_not() {
        let (client, mut server) = tokio::io::duplex(64);
        let limits = TimeoutHandle::new(Duration::from_millis(150), Duration::from_secs(5));
        let mut stream = IoTimeout::new(client, limits.clone());

        // Bytes every 100 ms keep the read alive although the whole read takes longer.
        let writer = tokio::spawn(async move {
            for _ in 0..4 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                server.write_all(b"x").await.ok();
            }
            server
        });
        let mut buf = [0u8; 4];
        let start = Instant::now();
        stream
            .read_exact(&mut buf)
            .await
            .expect("slow but steady data");
        assert!(start.elapsed() >= Duration::from_millis(350));
        let _server = writer.await.expect("writer");

        // Now nothing comes: TimedOut after the limit.
        let start = Instant::now();
        let err = stream.read(&mut buf).await.expect_err("silent");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        let waited = start.elapsed();
        assert!(waited >= Duration::from_millis(140) && waited < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn zero_turns_the_limit_off_and_changing_it_restarts_the_deadline() {
        let (client, mut server) = tokio::io::duplex(64);
        let limits = TimeoutHandle::new(Duration::from_millis(100), Duration::from_secs(5));
        let mut stream = IoTimeout::new(client, limits.clone());

        // A read that is cancelled after arming its deadline ...
        let mut buf = [0u8; 1];
        let cancelled =
            tokio::time::timeout(Duration::from_millis(50), stream.read(&mut buf)).await;
        assert!(cancelled.is_err(), "nothing was sent");
        // ... and the limit turned off: no timeout, however long it takes.
        limits.set_read(Duration::ZERO);
        let writer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            server.write_all(b"y").await.ok();
            server
        });
        stream.read_exact(&mut buf).await.expect("no limit");
        assert_eq!(&buf, b"y");
        let _server = writer.await.expect("writer");

        // Back on: a fresh deadline, not the stale one from the cancelled read.
        limits.set_read(Duration::from_millis(100));
        let start = Instant::now();
        let err = stream.read(&mut buf).await.expect_err("silent");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() >= Duration::from_millis(90));
    }

    #[tokio::test]
    async fn blocked_write_times_out() {
        // The peer never reads: the 16-byte pipe fills up and the write stalls.
        let (client, _server) = tokio::io::duplex(16);
        let limits = TimeoutHandle::new(Duration::from_secs(5), Duration::from_millis(100));
        let mut stream = IoTimeout::new(client, limits);
        let err = stream
            .write_all(&[0u8; 1024])
            .await
            .expect_err("peer does not read");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn tap_records_written_bytes() {
        let (client, mut server) = tokio::io::duplex(64);
        let tap = WireTap::new();
        let limits = TimeoutHandle::new(Duration::from_secs(1), Duration::from_secs(1));
        let mut stream = IoTimeout::new(client, limits).with_tap(Some(tap.clone()));
        stream.write_all(b"A1 NOOP\r\n").await.expect("write");
        let mut buf = [0u8; 9];
        server.read_exact(&mut buf).await.expect("read");
        assert_eq!(tap.bytes(), b"A1 NOOP\r\n");
    }
}
