//! One writer thread and a small pool of reader threads, each owning its own connection.
//!
//! Callers submit closures over a channel and await the result on a `oneshot`, so no database
//! call ever runs on (or blocks) a Tokio worker. Readers see a consistent WAL snapshot while
//! the writer works; writes are serialised by the single writer thread.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

use rusqlite::{Connection, Transaction, TransactionBehavior};
use tokio::sync::oneshot;

use crate::StoreError;

type WriteJob = Box<dyn FnOnce(&mut Connection) + Send>;
type ReadJob = Box<dyn FnOnce(&Connection) + Send>;

pub(crate) struct Workers {
    write_tx: Option<mpsc::Sender<WriteJob>>,
    read_tx: Option<mpsc::Sender<ReadJob>>,
    writer: Option<JoinHandle<()>>,
    readers: Vec<JoinHandle<()>>,
}

impl Workers {
    pub(crate) fn start(writer: Connection, readers: Vec<Connection>) -> Result<Self, StoreError> {
        let (write_tx, write_rx) = mpsc::channel::<WriteJob>();
        let (read_tx, read_rx) = mpsc::channel::<ReadJob>();
        let read_rx = Arc::new(Mutex::new(read_rx));

        let spawn = |name: String, f: Box<dyn FnOnce() + Send>| {
            thread::Builder::new()
                .name(name)
                .spawn(f)
                .map_err(StoreError::io("start database thread"))
        };

        let writer = spawn(
            "db-writer".into(),
            Box::new(move || writer_loop(writer, &write_rx)),
        )?;
        let readers = readers
            .into_iter()
            .enumerate()
            .map(|(i, conn)| {
                let rx = Arc::clone(&read_rx);
                spawn(
                    format!("db-reader-{i}"),
                    Box::new(move || reader_loop(conn, &rx)),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            write_tx: Some(write_tx),
            read_tx: Some(read_tx),
            writer: Some(writer),
            readers,
        })
    }

    /// Runs `f` inside a `BEGIN IMMEDIATE` transaction on the writer thread. Commits when `f`
    /// returns `Ok`, rolls back otherwise.
    pub(crate) async fn write<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job: WriteJob = Box::new(move |conn| {
            let result = guarded(|| {
                let txn = conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .map_err(StoreError::sqlite("begin write transaction"))?;
                let value = f(&txn)?;
                txn.commit()
                    .map_err(StoreError::sqlite("commit write transaction"))?;
                Ok(value)
            });
            // The caller may have stopped waiting; nothing else to do then.
            let _ = tx.send(result);
        });
        self.write_tx
            .as_ref()
            .ok_or(StoreError::WorkerGone("writer"))?
            .send(job)
            .map_err(|_| StoreError::WorkerGone("writer"))?;
        rx.await.map_err(|_| StoreError::WorkerGone("writer"))?
    }

    /// Runs `f` on one of the reader connections (`query_only`).
    pub(crate) async fn read<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let job: ReadJob = Box::new(move |conn| {
            let _ = tx.send(guarded(|| f(conn)));
        });
        self.read_tx
            .as_ref()
            .ok_or(StoreError::WorkerGone("reader"))?
            .send(job)
            .map_err(|_| StoreError::WorkerGone("reader"))?;
        rx.await.map_err(|_| StoreError::WorkerGone("reader"))?
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        // Closing a channel ends its loop once the queued jobs are done. Readers close first and
        // the writer last: only the last connection to close checkpoints and removes the WAL,
        // and two connections closing at the same moment can each leave that to the other.
        self.read_tx.take();
        for handle in self.readers.drain(..) {
            join(handle);
        }
        self.write_tx.take();
        if let Some(handle) = self.writer.take() {
            join(handle);
        }
    }
}

fn join(handle: JoinHandle<()>) {
    // A job holding the last `Store` clone would otherwise join its own thread.
    if handle.thread().id() != thread::current().id() && handle.join().is_err() {
        tracing::error!("database thread panicked during shutdown");
    }
}

fn writer_loop(mut conn: Connection, jobs: &mpsc::Receiver<WriteJob>) {
    while let Ok(job) = jobs.recv() {
        job(&mut conn);
    }
    // Recommended before closing: lets SQLite refresh statistics where they are stale.
    if let Err(err) = conn.execute_batch("PRAGMA optimize;") {
        tracing::warn!(%err, "PRAGMA optimize failed on close");
    }
    if let Err((_, err)) = conn.close() {
        tracing::warn!(%err, "closing the write connection failed");
    }
}

fn reader_loop(conn: Connection, jobs: &Mutex<mpsc::Receiver<ReadJob>>) {
    loop {
        // Hold the lock only while waiting for the next job, not while running it.
        let next = jobs.lock().unwrap_or_else(PoisonError::into_inner).recv();
        match next {
            Ok(job) => job(&conn),
            Err(_) => break,
        }
    }
    if let Err((_, err)) = conn.close() {
        tracing::warn!(%err, "closing a read connection failed");
    }
}

/// Turns a panic inside a job into an error, so one bad job cannot stop a database thread.
fn guarded<T>(f: impl FnOnce() -> Result<T, StoreError>) -> Result<T, StoreError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let msg = panic_message(payload.as_ref());
        tracing::error!(panic = %msg, "database job panicked");
        Err(StoreError::JobPanicked(msg))
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned())
}
