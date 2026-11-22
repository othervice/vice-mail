use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    CoreError, CoreEvent, ErrorCode, LanguagePreference, Platform, Preferences, Secret,
    ThemePreference,
};
use mail_store::{GLOBAL_SCOPE, Store, StoreOptions};
use tempfile::TempDir;
use tracing_subscriber::fmt::MakeWriter;

use crate::credentials::{Credentials, MemorySecretStore};
use crate::{AppCore, BuildInfo, CoreConfig, CoreServices, DATABASE_FILE, logging};

fn config(dir: &TempDir) -> CoreConfig {
    CoreConfig {
        data_dir: dir.path().join("data"),
        build: BuildInfo {
            version: "1.2.3".into(),
            commit: Some("abc1234".into()),
            build_date: Some("2026-11-22T18:00:00Z".into()),
        },
    }
}

/// Product services with an in-memory keychain (tests never touch the real one).
fn services() -> CoreServices {
    CoreServices::new(Credentials::new(MemorySecretStore::new()))
}

const DARK_DE: Preferences = Preferences {
    theme: ThemePreference::Dark,
    language: LanguagePreference::De,
};

#[tokio::test]
async fn set_preferences_stores_and_emits_an_event() {
    let dir = TempDir::new().expect("temp dir");
    let core = AppCore::start(config(&dir), services()).await;
    assert!(core.startup_error().is_none());
    let mut events = core.subscribe();

    assert_eq!(
        core.get_preferences().await.expect("read"),
        Preferences::default()
    );
    let stored = core.set_preferences(DARK_DE).await.expect("write");
    assert_eq!(stored, DARK_DE);

    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("event within 5 s")
        .expect("event");
    assert_eq!(
        event,
        CoreEvent::PreferencesChanged {
            preferences: DARK_DE
        }
    );
    assert_eq!(core.get_preferences().await.expect("read"), DARK_DE);
    drop(core);

    // Survives a restart.
    let core = AppCore::start(config(&dir), services()).await;
    assert_eq!(core.get_preferences().await.expect("read"), DARK_DE);
}

#[tokio::test]
async fn every_subscriber_receives_events() {
    let dir = TempDir::new().expect("temp dir");
    let core = AppCore::start(config(&dir), services()).await;
    let mut first = core.subscribe();
    let mut second = core.subscribe();
    core.set_preferences(DARK_DE).await.expect("write");
    for rx in [&mut first, &mut second] {
        assert!(matches!(
            rx.recv().await,
            Ok(CoreEvent::PreferencesChanged { .. })
        ));
    }
}

#[tokio::test]
async fn app_info_comes_from_the_build() {
    let dir = TempDir::new().expect("temp dir");
    let info = AppCore::start(config(&dir), services()).await.app_info();
    assert_eq!(info.version, "1.2.3");
    assert_eq!(info.commit.as_deref(), Some("abc1234"));
    assert_eq!(info.build_date.as_deref(), Some("2026-11-22T18:00:00Z"));
    assert_eq!(info.platform, Platform::current());
}

#[tokio::test]
async fn too_new_database_is_reported_by_every_storage_call() {
    let dir = TempDir::new().expect("temp dir");
    let cfg = config(&dir);
    std::fs::create_dir_all(&cfg.data_dir).expect("mkdir");
    let db = cfg.data_dir.join(DATABASE_FILE);
    write_newer_database(&db);

    let core = AppCore::start(cfg, services()).await;
    assert!(
        matches!(core.startup_error(), Some(CoreError::DatabaseTooNew { .. })),
        "{:?}",
        core.startup_error()
    );
    let err = core.get_preferences().await.expect_err("rejected");
    assert_eq!(err.code(), ErrorCode::DatabaseTooNew);
    let err = core.set_preferences(DARK_DE).await.expect_err("rejected");
    assert_eq!(err.code(), ErrorCode::DatabaseTooNew);
    // Not storage-backed: still answers, so the UI can render its About page.
    assert_eq!(core.app_info().version, "1.2.3");
}

/// Creates a current database, then patches its header so it claims schema version 999, as a
/// newer app version would leave it. Bytes 60..64 of an SQLite file hold `user_version`.
fn write_newer_database(path: &std::path::Path) {
    drop(Store::open(path, StoreOptions::default()).expect("create"));
    assert!(
        !path.with_file_name(format!("{DATABASE_FILE}-wal")).exists(),
        "closed cleanly, header is current"
    );
    let mut bytes = std::fs::read(path).expect("read");
    bytes[60..64].copy_from_slice(&999_u32.to_be_bytes());
    std::fs::write(path, bytes).expect("write");
}

#[tokio::test]
async fn unknown_stored_value_falls_back_to_the_default() {
    let dir = TempDir::new().expect("temp dir");
    let cfg = config(&dir);
    {
        let store =
            Store::open(&cfg.data_dir.join(DATABASE_FILE), StoreOptions::default()).expect("open");
        store
            .put_settings(
                GLOBAL_SCOPE,
                vec![
                    ("ui.theme".into(), "\"neon\"".into()),
                    ("ui.language".into(), "\"de\"".into()),
                ],
            )
            .await
            .expect("write");
    }
    let core = AppCore::start(cfg, services()).await;
    let prefs = core.get_preferences().await.expect("read");
    assert_eq!(prefs.theme, ThemePreference::System);
    assert_eq!(prefs.language, LanguagePreference::De);
}

// ---------------------------------------------------------------------------------------------
// Secret redaction in the log
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

impl io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buffer {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Every usual way of getting a value into a log line: field with `?`/`%`, message formatting,
/// a struct's `Debug`, an error carrying the secret, and a span field.
#[test]
fn secret_never_appears_in_the_log() {
    const PLAIN: &str = "hunter2-correct-horse";

    #[derive(Debug)]
    #[allow(dead_code)] // read through `Debug`
    struct Login {
        user: &'static str,
        password: Secret<String>,
    }

    let buffer = Buffer::default();
    tracing::subscriber::with_default(logging::test_subscriber(buffer.clone(), true), || {
        let secret = Secret::new(PLAIN.to_owned());
        let span = tracing::info_span!("login", password = ?secret);
        let _entered = span.enter();
        tracing::info!(password = ?secret, "debug field");
        tracing::info!(password = %secret, "display field");
        tracing::info!("message {secret} {secret:?}");
        tracing::warn!(login = ?Login { user: "alice", password: secret.clone() }, "struct field");
        let err = CoreError::Internal(format!("server rejected password {secret}"));
        tracing::error!(error = %err, debug = ?err, "error");
    });

    let log = buffer.text();
    assert!(
        log.contains("debug field") && log.contains("error"),
        "log was written: {log}"
    );
    assert!(!log.contains(PLAIN), "secret leaked into the log:\n{log}");
    assert!(log.matches("[redacted]").count() >= 8, "{log}");
}
