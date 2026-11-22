//! Fault-injecting TCP proxy (127.0.0.1, random port) between client and test server.
//! Faults apply to the next accepted connection: close after N bytes server→client,
//! stall after N bytes (socket stays open, nothing flows), refuse the next K connections.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, Notify};

#[derive(Clone, Copy, Debug)]
pub enum Fault {
    None,
    CloseAfter(u64),
    StallAfter(u64),
    /// Close / stall once this many ms passed since the connection was accepted (e.g. during IDLE).
    CloseAfterMs(u64),
    StallAfterMs(u64),
}

pub struct Proxy {
    pub port: u16,
    next_fault: Mutex<Fault>,
    refuse: AtomicUsize,
    pub accepted: AtomicUsize,
    pub bytes_s2c: AtomicU64,
    fired_at: Mutex<Option<Instant>>,
    release: Notify,
}

impl Proxy {
    pub async fn start(upstream_port: u16) -> std::io::Result<Arc<Proxy>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let p = Arc::new(Proxy {
            port: listener.local_addr()?.port(),
            next_fault: Mutex::new(Fault::None),
            refuse: AtomicUsize::new(0),
            accepted: AtomicUsize::new(0),
            bytes_s2c: AtomicU64::new(0),
            fired_at: Mutex::new(None),
            release: Notify::new(),
        });
        let p2 = p.clone();
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    break;
                };
                p2.accepted.fetch_add(1, Ordering::SeqCst);
                if p2.refuse.load(Ordering::SeqCst) > 0 {
                    p2.refuse.fetch_sub(1, Ordering::SeqCst);
                    drop(client);
                    continue;
                }
                let fault = std::mem::replace(&mut *p2.next_fault.lock().await, Fault::None);
                let p3 = p2.clone();
                tokio::spawn(async move {
                    if let Ok(server) = TcpStream::connect(("127.0.0.1", upstream_port)).await {
                        p3.pump(client, server, fault).await;
                    }
                });
            }
        });
        Ok(p)
    }

    pub async fn arm(&self, f: Fault) {
        *self.next_fault.lock().await = f;
        *self.fired_at.lock().await = None;
    }
    pub fn refuse_next(&self, n: usize) {
        self.refuse.store(n, Ordering::SeqCst);
    }
    pub async fn fired_at(&self) -> Option<Instant> {
        *self.fired_at.lock().await
    }
    /// Ends stalled connections (closes them).
    pub fn release_stalled(&self) {
        self.release.notify_waiters();
    }

    async fn fire(&self) {
        *self.fired_at.lock().await = Some(Instant::now());
    }

    async fn pump(self: Arc<Self>, client: TcpStream, server: TcpStream, fault: Fault) {
        client.set_nodelay(true).ok();
        server.set_nodelay(true).ok();
        let (mut cr, mut cw) = client.into_split();
        let (mut sr, mut sw) = server.into_split();
        let c2s = async {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match cr.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sw.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            }
        };
        let me = self.clone();
        let s2c = async move {
            let mut buf = vec![0u8; 16 * 1024];
            let mut sent: u64 = 0;
            let started = Instant::now();
            loop {
                let read = async {
                    match fault {
                        Fault::CloseAfterMs(ms) | Fault::StallAfterMs(ms) => {
                            let left = ms.saturating_sub(started.elapsed().as_millis() as u64);
                            tokio::time::timeout(std::time::Duration::from_millis(left), sr.read(&mut buf))
                                .await
                                .ok()
                        }
                        _ => Some(sr.read(&mut buf).await),
                    }
                };
                let n = match read.await {
                    None => {
                        me.fire().await;
                        if let Fault::StallAfterMs(_) = fault {
                            me.release.notified().await;
                        }
                        return; // time limit reached
                    }
                    Some(Ok(0)) | Some(Err(_)) => return,
                    Some(Ok(n)) => n,
                };
                let limit = match fault {
                    Fault::CloseAfter(l) | Fault::StallAfter(l) => Some(l),
                    _ => None,
                };
                let n_send = match limit {
                    Some(l) if sent + n as u64 >= l => (l - sent) as usize,
                    _ => n,
                };
                if cw.write_all(&buf[..n_send]).await.is_err() {
                    return;
                }
                sent += n_send as u64;
                me.bytes_s2c.fetch_add(n_send as u64, Ordering::Relaxed);
                if n_send < n || limit.is_some_and(|l| sent >= l) {
                    cw.flush().await.ok();
                    me.fire().await;
                    if let Fault::StallAfter(_) = fault {
                        me.release.notified().await; // keep both sockets open, move nothing
                    }
                    return;
                }
            }
        };
        // Whichever direction ends first closes both sockets (drop = FIN).
        tokio::select! {
            _ = c2s => {},
            _ = s2c => {},
        }
    }
}
