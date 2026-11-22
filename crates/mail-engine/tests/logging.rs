//! Installs the global subscriber, so it runs in its own test binary.

use std::time::Duration;

use mail_engine::logging::{self, LOG_FILE_PREFIX, LOG_FILE_SUFFIX, LogConfig};
use mail_model::Secret;

#[test]
fn init_writes_a_daily_log_file_without_secrets() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let log_dir = dir.path().join("logs");
    let guard = logging::init(&LogConfig {
        dir: log_dir.clone(),
        console: false,
        verbose: false,
    })
    .expect("init logging");

    let secret = Secret::new("s3cr3t-token-value".to_owned());
    // This test crate's own target is not one of ours, so name one explicitly.
    tracing::info!(target: "mail_engine", token = ?secret, "logging-test-marker");
    tracing::debug!(target: "mail_engine", "debug-is-filtered-out");
    tracing::info!(target: "some_dependency", "dependency-info-is-filtered-out");
    tracing::warn!(target: "some_dependency", "dependency-warning-is-kept");
    // Dropping the guard flushes the background writer.
    drop(guard);
    std::thread::sleep(Duration::from_millis(50));

    let files: Vec<_> = std::fs::read_dir(&log_dir)
        .expect("log dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    let name = &files[0];
    assert!(
        name.starts_with(&format!("{LOG_FILE_PREFIX}."))
            && name.ends_with(&format!(".{LOG_FILE_SUFFIX}")),
        "daily file app.YYYY-MM-DD.log, got {name}"
    );

    let text = std::fs::read_to_string(log_dir.join(name)).expect("read log");
    assert!(text.contains("logging-test-marker"), "{text}");
    assert!(text.contains("[redacted]"), "{text}");
    assert!(!text.contains("s3cr3t-token-value"), "{text}");
    assert!(!text.contains("debug-is-filtered-out"), "{text}");
    assert!(!text.contains("dependency-info-is-filtered-out"), "{text}");
    assert!(text.contains("dependency-warning-is-kept"), "{text}");
    assert!(!text.contains('\u{1b}'), "no ANSI colours in the file");

    // A second subscriber cannot be installed; that is an error, not a panic.
    assert!(
        logging::init(&LogConfig {
            dir: log_dir,
            console: false,
            verbose: false
        })
        .is_err()
    );
}
