//! Logging: `tracing` into a daily rotating file in the app's log directory, plus the console
//! in development builds. Secrets never reach the log because [`mail_model::Secret`] formats
//! as `[redacted]`.

use std::path::PathBuf;

use mail_model::CoreError;
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, Registry};

/// Log files are `app.YYYY-MM-DD.log`.
pub const LOG_FILE_PREFIX: &str = "app";
pub const LOG_FILE_SUFFIX: &str = "log";
/// Daily files kept before the oldest is deleted.
pub const LOG_FILES_KEPT: usize = 14;

/// Our crates; everything else only logs warnings and errors.
const OWN_TARGETS: &[&str] = &[
    "mail_app",
    "mail_engine",
    "mail_store",
    "mail_imap",
    "mail_model",
];

#[derive(Debug, Clone)]
pub struct LogConfig {
    /// The app's log directory.
    pub dir: PathBuf,
    /// Also log to stderr (development builds).
    pub console: bool,
    /// Log our own crates at `debug` instead of `info`.
    pub verbose: bool,
}

/// Keeps the background log writer alive. Drop it last: dropping flushes buffered lines.
#[must_use = "dropping the guard stops file logging"]
pub struct LogGuard {
    _file: WorkerGuard,
}

/// Installs the global subscriber. Call once, as early as possible.
pub fn init(config: &LogConfig) -> Result<LogGuard, CoreError> {
    std::fs::create_dir_all(&config.dir)
        .map_err(|err| CoreError::Internal(format!("create log directory: {err}")))?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(LOG_FILES_KEPT)
        .build(&config.dir)
        .map_err(|err| CoreError::Internal(format!("open log file: {err}")))?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let console = config.console.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_filter(filter(config.verbose))
    });

    tracing_subscriber::registry()
        .with(file_layer(writer).with_filter(filter(config.verbose)))
        .with(console)
        .try_init()
        .map_err(|err| CoreError::Internal(format!("install log subscriber: {err}")))?;

    Ok(LogGuard { _file: guard })
}

/// The formatting layer used for the log file. Public so tests can point it at a buffer and
/// check exactly what would be written.
pub fn file_layer<S, W>(writer: W) -> impl Layer<S> + Send + Sync
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_thread_names(true)
        .with_writer(writer)
}

/// Level filter: our crates at `info` (or `debug` when verbose), others at `warn`.
pub fn filter(verbose: bool) -> Targets {
    let own = if verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };
    OWN_TARGETS
        .iter()
        .fold(Targets::new().with_default(LevelFilter::WARN), |t, name| {
            t.with_target(*name, own)
        })
}

/// Lets tests build a subscriber with the production layers without installing it globally.
pub fn test_subscriber<W>(writer: W, verbose: bool) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    Registry::default().with(file_layer(writer).with_filter(filter(verbose)))
}
