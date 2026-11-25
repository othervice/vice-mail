//! Decision 0037 with the real updater plugin on Tauri's mock runtime, against a local HTTP
//! server that counts every TCP connection:
//!
//! - no connection at all while the switch is off and nobody clicked "Check now" (and the
//!   positive control: the click connects);
//! - a newer, correctly signed update downloads and verifies;
//! - a manipulated update file, and a manifest that announces another version than the file was
//!   signed for, are rejected; a lower or equal version is not offered;
//! - a manifest pointing its download elsewhere is refused without connecting there;
//! - a download larger than the cap (announced or not) or slower than the time limit stops
//!   (finding N-3).
//!
//! The fixtures were signed once with a throwaway key whose private half was never stored;
//! only its public key is here.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mail_engine::credentials::{Credentials, MemorySecretStore};
use mail_engine::{AppCore, BuildInfo, CoreConfig, CoreServices, UpdateSchedule};
use mail_model::{ErrorCode, UpdateStatus};
use serde_json::{Value, json};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri::{App, Url};
use tempfile::TempDir;

use super::PluginSource;

const TEST_PUBKEY: &str = include_str!("fixtures/test-key.pub");
const PAYLOAD: &[u8] = include_bytes!("fixtures/update.bin");
/// Signature of `update.bin`, bound to version 0.2.0 (`tauri signer sign --app-version 0.2.0`).
const SIGNATURE_0_2_0: &str = include_str!("fixtures/update-0.2.0.sig");

/// The mock app's version: `tauri::test::mock_context` always reports 0.1.0, whatever the
/// package's version is.
const RUNNING: &str = "0.1.0";

const FAST: UpdateSchedule = UpdateSchedule {
    first_delay: Duration::from_millis(0),
    interval: Duration::from_millis(20),
};

/// What the local server answers.
struct Routes {
    manifest: Value,
    payload: Vec<u8>,
    /// The update file without `Content-Length` (the body ends when the connection closes).
    no_length: bool,
    /// The update file in 1 KiB pieces with this pause between them.
    trickle: Option<Duration>,
    /// Bytes of the update file written before the client went away.
    sent: usize,
}

/// A local HTTP/1.1 server on 127.0.0.1 that counts accepted TCP connections.
struct CountingServer {
    base: String,
    connections: Arc<AtomicUsize>,
    routes: Arc<std::sync::Mutex<Routes>>,
}

impl CountingServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let connections = Arc::new(AtomicUsize::new(0));
        let routes = Arc::new(std::sync::Mutex::new(Routes {
            manifest: json!({}),
            payload: PAYLOAD.to_vec(),
            no_length: false,
            trickle: None,
            sent: 0,
        }));
        let (count, shared) = (Arc::clone(&connections), Arc::clone(&routes));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                count.fetch_add(1, Ordering::SeqCst);
                let shared = Arc::clone(&shared);
                std::thread::spawn(move || serve(stream, &shared));
            }
        });
        Self {
            base,
            connections,
            routes,
        }
    }

    fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    fn endpoint(&self) -> Url {
        Url::parse(&format!("{}/latest.json", self.base)).expect("url")
    }

    /// A static manifest for this platform with `version`, `signature` and a download URL.
    fn announce(&self, version: &str, signature: &str, url: Option<String>) {
        let url = url.unwrap_or_else(|| format!("{}/update.bin", self.base));
        let target = tauri_plugin_updater::target().expect("supported platform");
        self.routes.lock().expect("lock").manifest = json!({
            "version": version,
            "notes": "Test release.",
            "pub_date": "2026-11-23T12:00:00Z",
            "platforms": { target: { "url": url, "signature": signature } },
        });
    }

    fn serve_payload(&self, payload: Vec<u8>) {
        self.routes.lock().expect("lock").payload = payload;
    }

    fn routes(&self) -> std::sync::MutexGuard<'_, Routes> {
        self.routes.lock().expect("lock")
    }
}

fn serve(mut stream: TcpStream, routes: &std::sync::Mutex<Routes>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    // Skip the headers.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => {}
        }
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (no_length, trickle) = {
        let routes = routes.lock().expect("lock");
        (routes.no_length, routes.trickle)
    };
    let (status, body, kind) = {
        let routes = routes.lock().expect("lock");
        match path {
            "/latest.json" => (
                "200 OK",
                serde_json::to_vec(&routes.manifest).expect("json"),
                "application/json",
            ),
            "/update.bin" => ("200 OK", routes.payload.clone(), "application/octet-stream"),
            _ => ("404 Not Found", Vec::new(), "text/plain"),
        }
    };
    let payload = path == "/update.bin";
    let length = if payload && no_length {
        String::new()
    } else {
        format!("Content-Length: {}\r\n", body.len())
    };
    let head =
        format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\n{length}Connection: close\r\n\r\n");
    let _ = stream.write_all(head.as_bytes());
    if payload {
        // In pieces, counting what got out before the client closed the connection.
        for piece in body.chunks(1024) {
            if stream.write_all(piece).is_err() || stream.flush().is_err() {
                break;
            }
            routes.lock().expect("lock").sent += piece.len();
            if let Some(pause) = trickle {
                std::thread::sleep(pause);
            }
        }
        let _ = stream.shutdown(std::net::Shutdown::Write);
        return;
    }
    let _ = stream.write_all(&body);
    let _ = stream.flush();
    let mut rest = Vec::new();
    let _ = reader.read_to_end(&mut rest);
}

struct Setup {
    _dir: TempDir,
    _app: App<MockRuntime>,
    core: AppCore,
    server: CountingServer,
}

/// The mock app with the updater plugin (test key, `requireSignedVersion` as shipped), a core
/// with a fresh data directory, and the plugin source pointed at the local server.
fn setup() -> Setup {
    setup_with(None)
}

/// [`setup`] with other download limits (size cap, time limit).
fn setup_with(limits: Option<(u64, Duration)>) -> Setup {
    let server = CountingServer::start();
    let mut context = mock_context(noop_assets());
    assert_eq!(context.package_info().version.to_string(), RUNNING);
    context.config_mut().plugins.0.insert(
        "updater".into(),
        json!({ "pubkey": TEST_PUBKEY.trim(), "requireSignedVersion": true }),
    );
    let app = mock_builder()
        .plugin(super::plugin())
        .build(context)
        .expect("mock app");
    let dir = TempDir::new().expect("dir");
    let core = tauri::async_runtime::block_on(AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: RUNNING.into(),
                commit: None,
                build_date: None,
            },
        },
        CoreServices::new(Credentials::new(MemorySecretStore::new())),
    ));
    let mut source = PluginSource::new(app.handle().clone(), server.endpoint(), || {
        Err("tests never install".into())
    })
    .expect("source");
    if let Some((max_bytes, timeout)) = limits {
        source = source.with_download_limits(max_bytes, timeout);
    }
    tauri::async_runtime::block_on(core.start_updates(Arc::new(source), FAST));
    Setup {
        _dir: dir,
        _app: app,
        core,
        server,
    }
}

fn status_after_download(core: &AppCore) -> UpdateStatus {
    tauri::async_runtime::block_on(async {
        core.download_update().await.expect("download started");
        for _ in 0..1000 {
            let status = core.get_update_info().await.expect("info").status;
            if !matches!(status, UpdateStatus::Downloading { .. }) {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the download did not finish");
    })
}

fn failure_code(status: &UpdateStatus) -> Option<ErrorCode> {
    match status {
        UpdateStatus::Failed { error, .. } => Some(error.code),
        _ => None,
    }
}

#[test]
fn no_connection_without_consent_and_one_for_check_now() {
    let s = setup();
    s.server.announce("0.2.0", SIGNATURE_0_2_0, None);
    // The background task looks every 20 ms; with the switch off it never connects.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        s.server.connections(),
        0,
        "switch off, no click: no connection"
    );
    let info = tauri::async_runtime::block_on(s.core.get_update_info()).expect("info");
    assert!(info.supported && !info.auto_check);

    // Positive control: the counter counts, and "Check now" reaches the server.
    let info = tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    assert_eq!(s.server.connections(), 1, "one connection for the manifest");
    let UpdateStatus::Available { update } = info.status else {
        panic!("expected an update, got {:?}", info.status)
    };
    assert_eq!(update.version, "0.2.0");
    assert_eq!(update.notes.as_deref(), Some("Test release."));
    assert_eq!(update.pub_date.as_deref(), Some("2026-11-23T12:00:00Z"));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        s.server.connections(),
        1,
        "nothing downloads without a click"
    );
}

#[test]
fn a_correctly_signed_newer_update_downloads_and_verifies() {
    let s = setup();
    s.server.announce("0.2.0", SIGNATURE_0_2_0, None);
    tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    let status = status_after_download(&s.core);
    assert_eq!(
        status,
        UpdateStatus::Ready {
            version: "0.2.0".into()
        }
    );
    // Installing is refused by the test guard, so nothing on this machine changes.
    let info = tauri::async_runtime::block_on(s.core.install_update()).expect("install ran");
    assert_eq!(
        failure_code(&info.status),
        Some(ErrorCode::UpdateInstallFailed)
    );
}

/// N-3: a release file over the size cap stops the download, whether the server announces its
/// length or not, long before the whole file is in memory.
#[test]
fn a_download_over_the_size_cap_stops() {
    const CAP: u64 = 256 * 1024;
    for no_length in [false, true] {
        let s = setup_with(Some((CAP, Duration::from_secs(60))));
        s.server.announce("0.2.0", SIGNATURE_0_2_0, None);
        // 64 MiB the signature never covers; a slow pace so the server notices the client go.
        s.server.serve_payload(vec![0x5a; 64 * 1024 * 1024]);
        {
            let mut routes = s.server.routes();
            routes.no_length = no_length;
            routes.trickle = Some(Duration::from_micros(200));
        }
        tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
        let status = status_after_download(&s.core);
        let UpdateStatus::Failed { error, .. } = &status else {
            panic!("expected a failed download, got {status:?}");
        };
        assert_eq!(error.code, ErrorCode::UpdateDownloadFailed, "{status:?}");
        let text = format!(
            "{} {}",
            error.message,
            error.detail.as_deref().unwrap_or_default()
        );
        assert!(text.contains("larger than"), "{error:?}");
        // The server stops writing once the connection is gone.
        std::thread::sleep(Duration::from_millis(300));
        let sent = s.server.routes().sent;
        assert!(
            sent < 16 * 1024 * 1024,
            "no length: {no_length}; {sent} bytes went out"
        );
        let err = tauri::async_runtime::block_on(s.core.install_update()).expect_err("not ready");
        assert_eq!(err.code(), ErrorCode::UpdateNotReady);
    }
}

/// N-3: a download slower than the time limit stops (the plugin's request timeout or the limit
/// around the whole download, whichever notices first).
#[test]
fn a_download_over_the_time_limit_stops() {
    let s = setup_with(Some((300 * 1024 * 1024, Duration::from_millis(400))));
    s.server.announce("0.2.0", SIGNATURE_0_2_0, None);
    // 512 pieces of 1 KiB, 20 ms apart: about ten seconds for the whole file.
    s.server.serve_payload(vec![0x5a; 512 * 1024]);
    s.server.routes().trickle = Some(Duration::from_millis(20));
    tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    let started = std::time::Instant::now();
    let status = status_after_download(&s.core);
    let UpdateStatus::Failed { error, .. } = &status else {
        panic!("expected a failed download, got {status:?}");
    };
    assert_eq!(error.code, ErrorCode::UpdateDownloadFailed, "{status:?}");
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        s.server.routes().sent < 512 * 1024,
        "the connection was dropped"
    );
}

#[test]
fn a_manipulated_update_file_is_rejected() {
    let s = setup();
    s.server.announce("0.2.0", SIGNATURE_0_2_0, None);
    let mut tampered = PAYLOAD.to_vec();
    tampered[0] ^= 0x20;
    s.server.serve_payload(tampered);
    tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    let status = status_after_download(&s.core);
    assert_eq!(
        failure_code(&status),
        Some(ErrorCode::UpdateSignatureInvalid),
        "{status:?}"
    );
    let err = tauri::async_runtime::block_on(s.core.install_update()).expect_err("not ready");
    assert_eq!(err.code(), ErrorCode::UpdateNotReady);
}

#[test]
fn a_damaged_signature_is_rejected() {
    let s = setup();
    // The signature text with a few characters of its last line changed.
    let mut forged = SIGNATURE_0_2_0.trim().to_owned();
    forged.replace_range(forged.len() - 8..forged.len() - 4, "AAAA");
    s.server.announce("0.2.0", &forged, None);
    tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    let status = status_after_download(&s.core);
    assert_eq!(
        failure_code(&status),
        Some(ErrorCode::UpdateSignatureInvalid),
        "{status:?}"
    );
}

#[test]
fn a_manifest_announcing_another_version_than_signed_is_rejected() {
    let s = setup();
    // The genuine 0.2.0 file, announced as 0.3.0 (a replayed old release under a new number).
    s.server.announce("0.3.0", SIGNATURE_0_2_0, None);
    tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    let status = status_after_download(&s.core);
    assert_eq!(
        failure_code(&status),
        Some(ErrorCode::UpdateSignatureInvalid),
        "{status:?}"
    );
}

#[test]
fn a_lower_or_equal_version_is_not_offered() {
    let s = setup();
    for version in ["0.0.9", RUNNING] {
        s.server.announce(version, SIGNATURE_0_2_0, None);
        let info = tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
        assert!(
            matches!(info.status, UpdateStatus::UpToDate { .. }),
            "{version}: {:?}",
            info.status
        );
        let err = tauri::async_runtime::block_on(s.core.download_update()).expect_err("none");
        assert_eq!(err.code(), ErrorCode::UpdateNotReady);
    }
}

#[test]
fn a_download_address_outside_the_update_hosts_is_refused_unfetched() {
    let s = setup();
    let elsewhere = CountingServer::start();
    s.server.announce(
        "0.2.0",
        SIGNATURE_0_2_0,
        Some(format!(
            "{}/update.bin",
            elsewhere.base.replace("127.0.0.1", "localhost")
        )),
    );
    let info = tauri::async_runtime::block_on(s.core.check_for_updates()).expect("check");
    assert_eq!(
        failure_code(&info.status),
        Some(ErrorCode::UpdateCheckFailed),
        "{:?}",
        info.status
    );
    assert_eq!(
        elsewhere.connections(),
        0,
        "the other host was never contacted"
    );
}

#[test]
fn the_shipped_config_has_the_key_endpoint_and_passive_windows_install() {
    let conf: Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).expect("tauri.conf.json");
    let updater = &conf["plugins"]["updater"];
    assert_eq!(
        updater["endpoints"],
        json!(["https://github.com/othervice/vice-mail/releases/latest/download/latest.json"])
    );
    assert_eq!(updater["requireSignedVersion"], true);
    assert_eq!(updater["windows"]["installMode"], "passive");
    assert!(updater.get("dangerousInsecureTransportProtocol").is_none());
    assert!(updater.get("allowDowngrades").is_none());
    let pubkey = updater["pubkey"].as_str().expect("pubkey");
    assert_ne!(
        pubkey,
        TEST_PUBKEY.trim(),
        "the test key is not the shipped key"
    );
    // No capability grants the plugin's JavaScript commands.
    for file in ["default.json", "secondary-windows.json"] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capabilities")
            .join(file);
        let text = std::fs::read_to_string(&path).expect("capability");
        assert!(
            !text.contains("updater"),
            "{} grants updater access",
            path.display()
        );
    }
}

#[test]
fn only_an_installed_app_replaces_itself() {
    use std::path::Path;
    let problem = super::install_location_problem;
    if cfg!(target_os = "macos") {
        assert!(
            problem(Path::new(
                "/Applications/Vice Mail.app/Contents/MacOS/mail-app"
            ))
            .is_none()
        );
        assert!(
            problem(Path::new(
                "/Users/a/Applications/Vice Mail.app/Contents/MacOS/mail-app"
            ))
            .is_none()
        );
        assert!(
            problem(Path::new("/repo/target/debug/mail-app")).is_some(),
            "dev build"
        );
        assert!(
            problem(Path::new(
                "/Volumes/Vice Mail/Vice Mail.app/Contents/MacOS/mail-app"
            ))
            .is_some()
        );
        assert!(
            problem(Path::new(
                "/private/var/folders/x/AppTranslocation/1/d/Vice Mail.app/Contents/MacOS/mail-app"
            ))
            .is_some()
        );
    }
    assert!(
        problem(&std::env::current_exe().expect("exe")).is_some(),
        "the test binary"
    );
}
