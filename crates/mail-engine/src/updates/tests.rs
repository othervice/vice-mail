//! Decision 0037: the update setting, the one-time question, the daily throttle and the steps,
//! through `AppCore` with the real store and a counting fake source. The shell's tests repeat
//! the "nothing without consent" proof against a local HTTP server with the real plugin.

use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use mail_model::{ErrorCode, Security, ServerSettings};
use tempfile::TempDir;

use super::*;
use crate::clock::{Clock, SystemClock};
use crate::credentials::{Credentials, MemorySecretStore};
use crate::{BuildInfo, CoreConfig, CoreServices};

const DAY_MS: i64 = mail_model::UPDATE_CHECK_INTERVAL_MS;

/// Fast looks for the tests: the switch and the throttle decide, not the timer.
const FAST: UpdateSchedule = UpdateSchedule {
    first_delay: Duration::from_millis(0),
    interval: Duration::from_millis(20),
};

#[derive(Debug, Default)]
struct OffsetClock(AtomicI64);

impl Clock for OffsetClock {
    fn now_ms(&self) -> i64 {
        SystemClock.now_ms() + self.0.load(Ordering::SeqCst)
    }
}

/// What the fake answers, and how often each step ran.
#[derive(Default)]
struct FakeSource {
    checks: AtomicUsize,
    downloads: AtomicUsize,
    installs: AtomicUsize,
    answer: Mutex<Option<Result<Option<AvailableUpdate>, UpdateError>>>,
    download_result: Mutex<Option<UpdateError>>,
    install_result: Mutex<Option<UpdateError>>,
}

impl FakeSource {
    fn answering(answer: Result<Option<AvailableUpdate>, UpdateError>) -> Arc<Self> {
        let source = Self::default();
        *source.answer.lock().expect("lock") = Some(answer);
        Arc::new(source)
    }
}

impl UpdateSource for FakeSource {
    fn check(&self) -> UpdateFuture<'_, Option<AvailableUpdate>> {
        self.checks.fetch_add(1, Ordering::SeqCst);
        let answer = self
            .answer
            .lock()
            .expect("lock")
            .clone()
            .unwrap_or(Ok(None));
        Box::pin(async move { answer })
    }

    fn download(&self, progress: DownloadProgress) -> UpdateFuture<'_, ()> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        let failure = self.download_result.lock().expect("lock").clone();
        Box::pin(async move {
            for done in [0, 50, 100] {
                progress(done, Some(100));
            }
            match failure {
                Some(err) => Err(err),
                None => Ok(()),
            }
        })
    }

    fn install(&self) -> Result<(), UpdateError> {
        self.installs.fetch_add(1, Ordering::SeqCst);
        match self.install_result.lock().expect("lock").clone() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
}

fn update(version: &str) -> AvailableUpdate {
    AvailableUpdate {
        version: version.into(),
        notes: Some("Fixes.\n\u{7}Bell gone.".into()),
        pub_date: Some("2026-11-23T12:00:00Z".into()),
    }
}

async fn start(dir: &TempDir, clock: Arc<OffsetClock>) -> AppCore {
    let mut services = CoreServices::new(Credentials::new(MemorySecretStore::new()));
    services.clock = clock;
    let core = AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "0.1.0".into(),
                commit: None,
                build_date: None,
            },
        },
        services,
    )
    .await;
    assert!(core.startup_error().is_none());
    core
}

async fn add_account(core: &AppCore) {
    let server = ServerSettings {
        host: "mail.example.org".into(),
        port: 993,
        security: Security::Tls,
        username: "alice".into(),
    };
    core.store_for_tests()
        .insert_account(mail_store::NewAccount {
            id: "00000000-0000-4000-8000-0000000000aa".parse().expect("id"),
            email: "alice@example.org".into(),
            display_name: "Alice".into(),
            imap: server.clone(),
            smtp: server,
            credential_ref: "x".into(),
        })
        .await
        .expect("account");
}

/// Waits long enough for many looks of the background task (`FAST`: one every 20 ms).
async fn let_the_schedule_run() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

async fn wait_for(
    core: &AppCore,
    what: &str,
    done: impl Fn(&UpdateStatus) -> bool,
) -> UpdateStatus {
    for _ in 0..200 {
        let status = core.updates().status();
        if done(&status) {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{what}: status stayed {:?}", core.updates().status());
}

#[tokio::test]
async fn without_a_source_nothing_is_offered_or_requested() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir, Arc::default()).await;
    add_account(&core).await;
    let info = core.get_update_info().await.expect("info");
    assert!(!info.supported);
    assert!(!info.auto_check);
    assert!(!info.ask_consent, "no question without an updater");
    assert_eq!(info.current_version, "0.1.0");
    assert_eq!(info.status, UpdateStatus::Idle);
    let err = core.check_for_updates().await.expect_err("no source");
    assert_eq!(err.code(), ErrorCode::UpdateNotReady);
}

#[tokio::test]
async fn nothing_is_requested_while_the_switch_is_off_until_check_now() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir, Arc::default()).await;
    let source = FakeSource::answering(Ok(None));
    core.start_updates(source.clone(), FAST).await;
    let_the_schedule_run().await;
    assert_eq!(
        source.checks.load(Ordering::SeqCst),
        0,
        "off by default: no request"
    );
    let info = core.get_update_info().await.expect("info");
    assert!(info.supported && !info.auto_check && info.last_check_ms.is_none());

    // Positive control: "Check now" works with the switch off and reaches the source.
    let info = core.check_for_updates().await.expect("check");
    assert_eq!(source.checks.load(Ordering::SeqCst), 1);
    assert!(matches!(info.status, UpdateStatus::UpToDate { .. }));
    assert!(info.last_check_ms.is_some());
    assert!(
        !info.auto_check,
        "a manual check does not turn the switch on"
    );
    let_the_schedule_run().await;
    assert_eq!(
        source.checks.load(Ordering::SeqCst),
        1,
        "still off: no automatic check"
    );
}

#[tokio::test]
async fn automatic_checks_run_at_most_once_a_day() {
    let dir = TempDir::new().expect("dir");
    let clock = Arc::new(OffsetClock::default());
    let core = start(&dir, clock.clone()).await;
    let source = FakeSource::answering(Ok(None));
    core.start_updates(source.clone(), FAST).await;

    core.set_update_auto_check(true).await.expect("on");
    wait_for(&core, "first automatic check", |s| {
        matches!(s, UpdateStatus::UpToDate { .. })
    })
    .await;
    let_the_schedule_run().await;
    assert_eq!(
        source.checks.load(Ordering::SeqCst),
        1,
        "one check, then the throttle"
    );

    clock.0.fetch_add(DAY_MS - 60_000, Ordering::SeqCst);
    let_the_schedule_run().await;
    assert_eq!(source.checks.load(Ordering::SeqCst), 1, "not yet a day");

    clock.0.fetch_add(60_000, Ordering::SeqCst);
    let_the_schedule_run().await;
    assert_eq!(
        source.checks.load(Ordering::SeqCst),
        2,
        "a day later: the next check"
    );

    // A failing server is not asked again sooner either.
    *source.answer.lock().expect("lock") = Some(Err(UpdateError::CheckFailed("offline".into())));
    clock.0.fetch_add(DAY_MS, Ordering::SeqCst);
    let failed = wait_for(&core, "failed check", |s| {
        matches!(s, UpdateStatus::Failed { .. })
    })
    .await;
    let UpdateStatus::Failed { version, error } = failed else {
        unreachable!()
    };
    assert_eq!(version, None);
    assert_eq!(error.code, ErrorCode::UpdateCheckFailed);
    let_the_schedule_run().await;
    assert_eq!(source.checks.load(Ordering::SeqCst), 3);

    // Off again: no more checks, even when one would be due.
    core.set_update_auto_check(false).await.expect("off");
    clock.0.fetch_add(10 * DAY_MS, Ordering::SeqCst);
    let_the_schedule_run().await;
    assert_eq!(source.checks.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn the_setting_survives_a_restart_and_stays_on_the_device() {
    let dir = TempDir::new().expect("dir");
    {
        let core = start(&dir, Arc::default()).await;
        core.set_update_auto_check(true).await.expect("on");
        core.shutdown().await;
    }
    let core = start(&dir, Arc::default()).await;
    let source = FakeSource::answering(Ok(None));
    core.start_updates(source, FAST).await;
    let info = core.get_update_info().await.expect("info");
    assert!(info.auto_check);
    assert!(
        !crate::settings_sync::SYNCED_SETTINGS.contains(&SETTINGS_KEY),
        "the update setting is a device setting"
    );
}

#[tokio::test]
async fn the_question_comes_once_after_the_first_account() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir, Arc::default()).await;
    let source = FakeSource::answering(Ok(None));
    core.start_updates(source.clone(), FAST).await;
    assert!(
        !core.get_update_info().await.expect("info").ask_consent,
        "no account yet"
    );

    add_account(&core).await;
    let info = core.get_update_info().await.expect("info");
    assert!(info.ask_consent, "first account: ask");
    assert!(!info.auto_check, "asking turns nothing on");

    // "No, thanks": answered, stays off, never asked again.
    let info = core.set_update_auto_check(false).await.expect("answer");
    assert!(!info.ask_consent && !info.auto_check);
    let_the_schedule_run().await;
    assert_eq!(
        source.checks.load(Ordering::SeqCst),
        0,
        "declined: no request"
    );

    // The switch in Settings still works later.
    let info = core.set_update_auto_check(true).await.expect("on");
    assert!(info.auto_check && !info.ask_consent);
}

#[tokio::test]
async fn download_and_install_only_on_request_and_in_order() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir, Arc::default()).await;
    let source = FakeSource::answering(Ok(Some(update("0.2.0"))));
    core.start_updates(source.clone(), FAST).await;

    let err = core.download_update().await.expect_err("nothing found yet");
    assert_eq!(err.code(), ErrorCode::UpdateNotReady);
    let err = core.install_update().await.expect_err("nothing downloaded");
    assert_eq!(err.code(), ErrorCode::UpdateNotReady);

    let info = core.check_for_updates().await.expect("check");
    let UpdateStatus::Available { update } = info.status else {
        panic!("expected an update, got {:?}", info.status)
    };
    assert_eq!(update.version, "0.2.0");
    assert_eq!(
        update.notes.as_deref(),
        Some("Fixes.\nBell gone."),
        "plain text only"
    );
    let_the_schedule_run().await;
    assert_eq!(
        source.downloads.load(Ordering::SeqCst),
        0,
        "found is not downloaded"
    );

    let mut events = core.subscribe();
    core.download_update().await.expect("download");
    let ready = wait_for(&core, "download", |s| {
        matches!(s, UpdateStatus::Ready { .. })
    })
    .await;
    assert_eq!(
        ready,
        UpdateStatus::Ready {
            version: "0.2.0".into()
        }
    );
    let mut progress = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let CoreEvent::UpdateStatusChanged {
            status: UpdateStatus::Downloading {
                downloaded, total, ..
            },
        } = event
        {
            progress.push((downloaded, total));
        }
    }
    assert!(
        progress.contains(&(100, Some(100))),
        "progress reported: {progress:?}"
    );
    assert_eq!(
        source.installs.load(Ordering::SeqCst),
        0,
        "ready is not installed"
    );

    // An install failure leaves the version and shows up in the state.
    *source.install_result.lock().expect("lock") =
        Some(UpdateError::InstallFailed("read-only volume".into()));
    let info = core.install_update().await.expect("install ran");
    let UpdateStatus::Failed { version, error } = info.status else {
        panic!("expected a failure, got {:?}", info.status)
    };
    assert_eq!(version.as_deref(), Some("0.2.0"));
    assert_eq!(error.code, ErrorCode::UpdateInstallFailed);
    assert_eq!(source.installs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_rejected_download_can_be_retried_and_installs_nothing() {
    let dir = TempDir::new().expect("dir");
    let core = start(&dir, Arc::default()).await;
    let source = FakeSource::answering(Ok(Some(update("0.2.0"))));
    *source.download_result.lock().expect("lock") =
        Some(UpdateError::SignatureInvalid("signature mismatch".into()));
    core.start_updates(source.clone(), FAST).await;
    core.check_for_updates().await.expect("check");
    core.download_update().await.expect("download started");
    let failed = wait_for(&core, "rejected", |s| {
        matches!(s, UpdateStatus::Failed { .. })
    })
    .await;
    let UpdateStatus::Failed { error, .. } = failed else {
        unreachable!()
    };
    assert_eq!(error.code, ErrorCode::UpdateSignatureInvalid);
    let err = core.install_update().await.expect_err("nothing verified");
    assert_eq!(err.code(), ErrorCode::UpdateNotReady);
    assert_eq!(source.installs.load(Ordering::SeqCst), 0);

    *source.download_result.lock().expect("lock") = None;
    core.download_update().await.expect("retry");
    wait_for(&core, "retry", |s| matches!(s, UpdateStatus::Ready { .. })).await;
    assert_eq!(source.downloads.load(Ordering::SeqCst), 2);
}

#[test]
fn notes_are_plain_text_and_bounded() {
    assert_eq!(clean_notes(None), None);
    assert_eq!(clean_notes(Some("  \n ".into())), None);
    assert_eq!(
        clean_notes(Some("a\u{1b}[31mb\r\nc".into())).as_deref(),
        Some("a[31mb\nc")
    );
    let long = "x".repeat(MAX_UPDATE_NOTES_CHARS + 10);
    assert_eq!(
        clean_notes(Some(long)).map(|n| n.chars().count()),
        Some(MAX_UPDATE_NOTES_CHARS)
    );
}
