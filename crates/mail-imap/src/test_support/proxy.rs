//! Fault-injecting TCP proxy on 127.0.0.1 (random port) in front of the test server. A fault
//! applies to the next accepted connection: close or stall after a number of bytes or
//! milliseconds from server to client, or refuse the next connections. Counts the bytes in both
//! directions, so a test can prove that nothing at all left the client.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};

/// What happens to the next connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    /// Close both sockets after this many bytes from the server.
    CloseAfter(u64),
    /// After this many bytes from the server, move nothing more but keep the sockets open.
    StallAfter(u64),
    /// Close both sockets this long after the connection was accepted.
    CloseAfterMs(u64),
    /// Stall this long after the connection was accepted.
    StallAfterMs(u64),
}

/// See module docs. Dropping the last handle stops accepting (open connections continue).
#[derive(Debug)]
pub struct Proxy {
    port: u16,
    next_fault: Mutex<Fault>,
    refuse: AtomicUsize,
    accepted: AtomicUsize,
    bytes_to_server: AtomicU64,
    bytes_to_client: AtomicU64,
    fired_at: Mutex<Option<Instant>>,
    release: Notify,
}

impl Proxy {
    /// Starts a proxy to `upstream` (for example `127.0.0.1:1993`).
    pub async fn start(upstream: SocketAddr) -> std::io::Result<Arc<Self>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let proxy = Arc::new(Self {
            port: listener.local_addr()?.port(),
            next_fault: Mutex::new(Fault::None),
            refuse: AtomicUsize::new(0),
            accepted: AtomicUsize::new(0),
            bytes_to_server: AtomicU64::new(0),
            bytes_to_client: AtomicU64::new(0),
            fired_at: Mutex::new(None),
            release: Notify::new(),
        });
        let weak = Arc::downgrade(&proxy);
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let Some(proxy) = weak.upgrade() else { break };
                proxy.accepted.fetch_add(1, Ordering::SeqCst);
                let mut left = proxy.refuse.load(Ordering::SeqCst);
                let refuse = loop {
                    let Some(next) = left.checked_sub(1) else {
                        break false;
                    };
                    match proxy.refuse.compare_exchange(
                        left,
                        next,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    ) {
                        Ok(_) => break true,
                        Err(actual) => left = actual,
                    }
                };
                if refuse {
                    drop(client);
                    continue;
                }
                let fault = std::mem::replace(&mut *proxy.next_fault.lock().await, Fault::None);
                tokio::spawn(async move {
                    if let Ok(server) = TcpStream::connect(upstream).await {
                        proxy.pump(client, server, fault).await;
                    }
                });
            }
        });
        Ok(proxy)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Sets the fault for the next accepted connection.
    pub async fn arm(&self, fault: Fault) {
        *self.next_fault.lock().await = fault;
        *self.fired_at.lock().await = None;
    }

    /// Closes the next `n` connections right after accepting them.
    pub fn refuse_next(&self, n: usize) {
        self.refuse.store(n, Ordering::SeqCst);
    }

    pub fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }

    /// Bytes from the client to the server so far (TLS records included).
    pub fn bytes_to_server(&self) -> u64 {
        self.bytes_to_server.load(Ordering::SeqCst)
    }

    pub fn bytes_to_client(&self) -> u64 {
        self.bytes_to_client.load(Ordering::SeqCst)
    }

    /// When the armed fault took effect.
    pub async fn fired_at(&self) -> Option<Instant> {
        *self.fired_at.lock().await
    }

    /// Closes stalled connections.
    pub fn release_stalled(&self) {
        self.release.notify_waiters();
    }

    async fn fire(&self) {
        *self.fired_at.lock().await = Some(Instant::now());
    }

    async fn pump(self: Arc<Self>, client: TcpStream, server: TcpStream, fault: Fault) {
        // Best effort: the proxy works without TCP_NODELAY, only slower.
        client.set_nodelay(true).ok();
        server.set_nodelay(true).ok();
        let (mut client_read, mut client_write) = client.into_split();
        let (mut server_read, mut server_write) = server.into_split();
        let to_server = async {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match client_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let Some(chunk) = buf.get(..n) else { break };
                        if server_write.write_all(chunk).await.is_err() {
                            break;
                        }
                        self.bytes_to_server.fetch_add(n as u64, Ordering::SeqCst);
                    }
                }
            }
        };
        let to_client = async {
            let mut buf = vec![0u8; 16 * 1024];
            let mut sent: u64 = 0;
            let started = Instant::now();
            loop {
                let read = match fault {
                    Fault::CloseAfterMs(ms) | Fault::StallAfterMs(ms) => {
                        let left = Duration::from_millis(ms).saturating_sub(started.elapsed());
                        tokio::time::timeout(left, server_read.read(&mut buf))
                            .await
                            .ok()
                    }
                    _ => Some(server_read.read(&mut buf).await),
                };
                let n = match read {
                    None => {
                        self.fire().await;
                        if matches!(fault, Fault::StallAfterMs(_)) {
                            self.release.notified().await;
                        }
                        return;
                    }
                    Some(Ok(0) | Err(_)) => return,
                    Some(Ok(n)) => n,
                };
                let limit = match fault {
                    Fault::CloseAfter(limit) | Fault::StallAfter(limit) => Some(limit),
                    _ => None,
                };
                let allowed = match limit {
                    Some(limit) => usize::try_from(limit.saturating_sub(sent))
                        .unwrap_or(n)
                        .min(n),
                    None => n,
                };
                let Some(chunk) = buf.get(..allowed) else {
                    return;
                };
                if client_write.write_all(chunk).await.is_err() {
                    return;
                }
                sent += allowed as u64;
                self.bytes_to_client
                    .fetch_add(allowed as u64, Ordering::SeqCst);
                if limit.is_some_and(|limit| sent >= limit) {
                    client_write.flush().await.ok();
                    self.fire().await;
                    if matches!(fault, Fault::StallAfter(_)) {
                        // Keep both sockets open and move nothing.
                        self.release.notified().await;
                    }
                    return;
                }
            }
        };
        // Whichever direction ends first closes both sockets (drop sends FIN).
        tokio::select! {
            () = to_server => {},
            () = to_client => {},
        }
    }
}
