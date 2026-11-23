use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, Transaction};
use rusqlite_migration::{Migrations, SchemaVersion};

use crate::workers::Workers;
use crate::{StoreError, backup, migrations};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Directory of the body files, next to the database file (decision 0009).
pub const BODIES_DIR: &str = "bodies";

/// Prepared statements kept per connection. The sync path alone uses about ten; rusqlite's
/// default of 16 would evict statements between windows.
const STATEMENT_CACHE: usize = 64;

/// Pragmas of the single write connection (decision 0003). `page_size` only takes effect before
/// the first table exists; `journal_mode` is set separately because its result must be checked.
const WRITER_PRAGMAS: &str = "
    PRAGMA synchronous = NORMAL;
    PRAGMA foreign_keys = ON;
    PRAGMA cache_size = -65536;
    PRAGMA temp_store = MEMORY;
    PRAGMA mmap_size = 268435456;
    PRAGMA journal_size_limit = 67108864;
    PRAGMA wal_autocheckpoint = 1000;
";

/// Pragmas of the reader connections (decision 0003).
const READER_PRAGMAS: &str = "
    PRAGMA query_only = 1;
    PRAGMA cache_size = -8192;
    PRAGMA mmap_size = 268435456;
    PRAGMA temp_store = MEMORY;
";

#[derive(Debug, Clone, Copy)]
pub struct StoreOptions {
    /// Number of reader threads (each with its own connection). Decision 0003: 2–4.
    pub readers: usize,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self { readers: 2 }
    }
}

/// What happened while opening the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenReport {
    /// Schema version found in the file (0 for a new database).
    pub previous_version: usize,
    /// Schema version after migrations.
    pub version: usize,
    /// Backup written before migrating, if any. A new, empty database is not backed up.
    pub backup: Option<PathBuf>,
}

/// Handle to the database. Cheap to clone; the threads stop when the last clone is dropped.
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}

struct Inner {
    workers: Workers,
    path: PathBuf,
    bodies: PathBuf,
    report: OpenReport,
    /// Makes index text from stored bodies (stage 7); set by the engine after opening.
    body_text: std::sync::RwLock<Option<Arc<dyn crate::BodyTextSource>>>,
    /// Classifies new messages (stage 9); set by the engine after opening.
    classifier: std::sync::RwLock<Option<Arc<dyn crate::Classifier>>>,
    /// Versions captured changes of synchronised tables (stage 11); set by the engine.
    pub(crate) sync_capture: std::sync::RwLock<Option<Arc<dyn crate::SyncCapture>>>,
}

/// A set of migrations and the schema version they lead to.
pub(crate) struct Schema {
    migrations: Migrations<'static>,
    version: usize,
}

impl Schema {
    pub(crate) fn current() -> Self {
        Self {
            migrations: migrations::migrations(),
            version: migrations::SCHEMA_VERSION,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_vec(ms: Vec<rusqlite_migration::M<'static>>) -> Self {
        let version = ms.len();
        Self {
            migrations: Migrations::new(ms),
            version,
        }
    }
}

impl Store {
    /// Opens (or creates) the database at `path` and migrates it to the newest schema.
    ///
    /// Blocking: does file IO and migrations. Call it from a blocking context
    /// (`tokio::task::spawn_blocking`), not from an async task.
    ///
    /// - A database newer than this build knows is rejected with [`StoreError::TooNew`] and
    ///   left untouched.
    /// - Before migrating an existing database, a backup is written to `backups/` next to it;
    ///   the newest [`backup::BACKUPS_KEPT`] backups are kept.
    pub fn open(path: &Path, options: StoreOptions) -> Result<Self, StoreError> {
        Self::open_with(path, options, &Schema::current())
    }

    pub(crate) fn open_with(
        path: &Path,
        options: StoreOptions,
        schema: &Schema,
    ) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(StoreError::io("create data directory"))?;
        }

        // Check the version through a read-only connection first, so a database from a newer
        // app version is not touched at all (no journal-mode change, no checkpoint on close).
        if fs::metadata(path).is_ok_and(|m| m.len() > 0) {
            let probe = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .map_err(StoreError::sqlite("open database for version check"))?;
            probe
                .busy_timeout(BUSY_TIMEOUT)
                .map_err(StoreError::sqlite("set busy timeout"))?;
            check_version(&probe, schema)?;
        }

        let mut writer = Connection::open(path).map_err(StoreError::sqlite("open database"))?;
        configure_writer(&writer)?;
        let previous_version = check_version(&writer, schema)?;

        let mut backup = None;
        if previous_version < schema.version {
            if !is_empty(&writer)? {
                let written = backup::create(&writer, path, previous_version)?;
                tracing::info!(path = %written.display(), version = previous_version, "database backup written");
                backup = Some(written);
            }
            schema.migrations.to_latest(&mut writer)?;
            tracing::info!(
                from = previous_version,
                to = schema.version,
                "database migrated"
            );
        }
        let version = check_version(&writer, schema)?;

        let readers = (0..options.readers.max(1))
            .map(|_| open_reader(path))
            .collect::<Result<Vec<_>, _>>()?;
        let workers = Workers::start(writer, readers)?;

        let bodies = path
            .parent()
            .map_or_else(|| PathBuf::from(BODIES_DIR), |dir| dir.join(BODIES_DIR));
        Ok(Self {
            inner: Arc::new(Inner {
                workers,
                path: path.to_owned(),
                bodies,
                report: OpenReport {
                    previous_version,
                    version,
                    backup,
                },
                body_text: std::sync::RwLock::new(None),
                classifier: std::sync::RwLock::new(None),
                sync_capture: std::sync::RwLock::new(None),
            }),
        })
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Directory of the message body files: [`BODIES_DIR`] next to the database file. Created
    /// with the first body.
    pub fn bodies_dir(&self) -> &Path {
        &self.inner.bodies
    }

    pub fn open_report(&self) -> &OpenReport {
        &self.inner.report
    }

    /// Sets what makes the full-text index's body text from stored bodies (stage 7). Without
    /// one, bodies are stored but only their headers are indexed. The engine sets it right
    /// after opening, before anything stores a body.
    pub fn set_body_text_source(&self, source: Arc<dyn crate::BodyTextSource>) {
        *self
            .inner
            .body_text
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(source);
    }

    pub(crate) fn body_text_source(&self) -> Option<Arc<dyn crate::BodyTextSource>> {
        self.inner
            .body_text
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Sets the classifier of new messages (stage 9). Without one, messages are stored
    /// without a category and classified once one is set ([`Store::category_step`]).
    pub fn set_classifier(&self, classifier: Arc<dyn crate::Classifier>) {
        *self
            .inner
            .classifier
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(classifier);
    }

    /// Sets the hook that assigns versions to captured changes (stage 11). The engine sets it
    /// right after opening; until then marks wait in `sync_dirty`.
    pub fn set_sync_capture(&self, capture: Arc<dyn crate::SyncCapture>) {
        *self
            .inner
            .sync_capture
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(capture);
    }

    pub(crate) fn sync_capture(&self) -> Option<Arc<dyn crate::SyncCapture>> {
        self.inner
            .sync_capture
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn classifier(&self) -> Option<Arc<dyn crate::Classifier>> {
        self.inner
            .classifier
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Runs `f` in a `BEGIN IMMEDIATE` transaction on the writer thread. Before the
    /// transaction commits, the changes it made to synchronised tables are captured for the
    /// settings sync (stage 11, [`crate::settings_sync`]): one central place for every write.
    pub(crate) async fn write<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Transaction<'_>) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let capture = self.sync_capture();
        self.inner
            .workers
            .write(move |tx| {
                let value = f(tx)?;
                if let Some(capture) = &capture {
                    crate::settings_sync::flush(tx, capture.as_ref())?;
                }
                Ok(value)
            })
            .await
    }

    /// Runs `f` on a reader thread.
    pub(crate) async fn read<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Connection) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        self.inner.workers.read(f).await
    }
}

/// Returns the schema version of the database, or [`StoreError::TooNew`].
fn check_version(conn: &Connection, schema: &Schema) -> Result<usize, StoreError> {
    match schema.migrations.current_version(conn)? {
        SchemaVersion::NoneSet => Ok(0),
        SchemaVersion::Inside(v) => Ok(v.get()),
        SchemaVersion::Outside(v) => Err(StoreError::TooNew {
            found: v.get(),
            supported: schema.version,
        }),
    }
}

fn is_empty(conn: &Connection) -> Result<bool, StoreError> {
    let objects: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
        .map_err(StoreError::sqlite("inspect schema"))?;
    Ok(objects == 0)
}

fn configure_writer(conn: &Connection) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(StoreError::sqlite("set busy timeout"))?;
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE);
    conn.execute_batch("PRAGMA page_size = 4096;")
        .map_err(StoreError::sqlite("set page size"))?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .map_err(StoreError::sqlite("enable WAL"))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Config(format!(
            "journal_mode is {mode}, WAL could not be enabled"
        )));
    }
    conn.execute_batch(WRITER_PRAGMAS)
        .map_err(StoreError::sqlite("configure write connection"))
}

fn open_reader(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(StoreError::sqlite("open read connection"))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(StoreError::sqlite("set busy timeout"))?;
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE);
    conn.execute_batch(READER_PRAGMAS)
        .map_err(StoreError::sqlite("configure read connection"))?;
    Ok(conn)
}
