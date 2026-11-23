//! The diagnostics file through [`AppCore`] with the real store: a real-looking account with a
//! password in the (in-memory) keychain, a message with subject, body, addresses and an
//! attachment, a folder named by the user, and log lines that carry an address and tokens.
//! None of it may reach the file; the size of the log part is bounded.

use mail_model::{
    AccountId, Address, ErrorCode, Flags, MailboxRole, MessageHeader, MessageId, RemoteMailbox,
    Secret, Security, ServerSettings, UidValidity,
};
use mail_store::{MailboxBaseline, NewAccount};
use tempfile::TempDir;

use super::*;
use crate::credentials::{CredentialRef, Credentials, MemorySecretStore};
use crate::{BuildInfo, CoreConfig, CoreServices};

const UV: UidValidity = UidValidity(9);
const DATE: i64 = 1_790_000_000;

const PASSWORD: &str = "Babbage-1842-Analytical!";
const EMAIL: &str = "ada.lovelace@analytical-engine.example";
const DISPLAY_NAME: &str = "Ada Lovelace";
const USER_NAME: &str = "ada.l1815";
const IMAP_HOST: &str = "imap.analytical-engine.example";
const SMTP_HOST: &str = "smtp.analytical-engine.example";
const FOLDER: &str = "Geheimprojekt Differenzmaschine";
const SUBJECT: &str = "Kontoauszug Oktober für Ada";
const BODY: &str = "Die Überweisung über 1.234,56 EUR ist eingegangen";
const ATTACHMENT: &str = "Kontoauszug_2026-10.pdf";
const CONTACT: &str = "charles@babbage.example";
const CONTACT_NAME: &str = "Charles Babbage";
const LOGGED_TOKEN: &str = "ya29.a0AfH6SMBx3pQ-very-secret-oauth-token";
const LOGGED_KEY: &str = "sk-live-0123456789abcdefABCDEF";

fn env(log_dir: Option<PathBuf>) -> DiagnosticsEnv {
    DiagnosticsEnv {
        product_name: "Test Mail".into(),
        app_identifier: "org.example.testmail".into(),
        log_dir,
        webview_version: Some("621.1.15".into()),
        locale: "de-DE".into(),
    }
}

async fn start(dir: &TempDir) -> AppCore {
    AppCore::start(
        CoreConfig {
            data_dir: dir.path().join("data"),
            build: BuildInfo {
                version: "1.2.3".into(),
                commit: Some("abc1234".into()),
                build_date: Some("2026-11-23T08:00:00Z".into()),
            },
        },
        CoreServices::new(Credentials::new(MemorySecretStore::new())),
    )
    .await
}

fn header(uid: u32) -> MessageHeader {
    MessageHeader {
        uid,
        modseq: None,
        flags: Flags::default(),
        size: 2_000,
        internal_date: DATE,
        date: Some(DATE),
        subject: SUBJECT.into(),
        from: vec![Address {
            name: Some(CONTACT_NAME.into()),
            email: CONTACT.into(),
        }],
        to: vec![Address {
            name: Some(DISPLAY_NAME.into()),
            email: EMAIL.into(),
        }],
        cc: Vec::new(),
        message_id: Some("1842@babbage.example".into()),
        in_reply_to: None,
        references: Vec::new(),
        list_id: None,
        list_unsubscribe: None,
        auto_submitted: None,
        precedence: None,
        has_attachments: true,
        category_headers: Vec::new(),
        gmail: None,
    }
}

/// The account, its password in the keychain, a user folder and one message with its body.
async fn seed(core: &AppCore) -> AccountId {
    let id: AccountId = "00000000-0000-4000-8000-000000001815".parse().expect("id");
    let server = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: USER_NAME.into(),
    };
    let store = core.store_for_tests();
    store
        .insert_account(NewAccount {
            id,
            email: EMAIL.into(),
            display_name: DISPLAY_NAME.into(),
            imap: server(IMAP_HOST, 993),
            smtp: server(SMTP_HOST, 465),
            credential_ref: CredentialRef::for_account(id).to_string(),
        })
        .await
        .expect("account");
    core.credentials_ref()
        .set(
            &CredentialRef::for_account(id),
            Secret::new(PASSWORD.to_owned()),
        )
        .await
        .expect("keychain");
    let remote = |name: &str, attrs: &[&str]| RemoteMailbox {
        name: name.into(),
        path: name.into(),
        delimiter: Some('/'),
        attributes: attrs.iter().map(|a| (*a).to_owned()).collect(),
        status: None,
    };
    store
        .sync_mailboxes(id, vec![remote("INBOX", &[]), remote(FOLDER, &[])])
        .await
        .expect("mailboxes");
    let boxes = store.mailboxes(id).await.expect("mailboxes");
    let inbox = boxes
        .iter()
        .find(|m| m.role == MailboxRole::Inbox)
        .expect("inbox")
        .id;
    store
        .reset_mailbox(
            inbox,
            MailboxBaseline {
                uid_validity: UV,
                uid_next: 1,
                highest_modseq: None,
            },
        )
        .await
        .expect("reset");
    store
        .store_headers(inbox, UV, vec![header(1)], None)
        .await
        .expect("header");
    let raw = format!(
        "From: {CONTACT_NAME} <{CONTACT}>\r\nTo: {DISPLAY_NAME} <{EMAIL}>\r\nSubject: {SUBJECT}\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b\r\n\r\n--b\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\r\n{BODY}\r\n--b\r\n\
         Content-Type: application/pdf\r\n\
         Content-Disposition: attachment; filename=\"{ATTACHMENT}\"\r\n\r\nJVBERi0=\r\n--b--\r\n"
    );
    store
        .store_body(MessageId(DATE), raw.into_bytes())
        .await
        .expect("body");
    id
}

/// Log lines as a careless call site might write them: addresses, tokens, a password, the
/// user name, the server, a folder and an attachment's path. (Subjects, message text and contact
/// names are never logged and cannot be told from other text; they are only in the database,
/// and the test checks that the file takes nothing from there.)
fn write_log(dir: &Path, name: &str, lines: &[String]) {
    std::fs::create_dir_all(dir).expect("log dir");
    std::fs::write(dir.join(name), lines.join("\n") + "\n").expect("log file");
}

/// The home directory as the logs would show it (a stand-in where none is set).
fn home() -> String {
    system::home_dir().unwrap_or_else(|| "/Users/ada".to_owned())
}

fn leaky_lines() -> Vec<String> {
    let home = home();
    vec![
        // Finding V-07: error texts with a path in the home directory and with an address.
        format!("2026-11-23T07:59:58.000001Z  WARN main mail_app::commands: command failed command=add_dropped_attachments code=Internal error=internal error: copy {home}/Documents/{ATTACHMENT}: denied"),
        format!("2026-11-23T07:59:59.000001Z  WARN main mail_app::commands: command failed command=send_draft code=InvalidAddress error=the address {CONTACT} is not valid"),
        "2026-11-23T08:00:00.000001Z  INFO main mail_engine::core: database ready schema_version=15".into(),
        format!("2026-11-23T08:00:01.000001Z  WARN main mail_app::commands: command failed command=add_account code=AccountExists error=an account for {EMAIL} exists already"),
        format!("2026-11-23T08:00:02.000001Z DEBUG main mail_engine::ai: request sent authorization=\"Bearer {LOGGED_TOKEN}\" key={LOGGED_KEY}"),
        format!("2026-11-23T08:00:03.000001Z  WARN main mail_imap: login rejected: LOGIN failed for {USER_NAME} on {IMAP_HOST}"),
        format!("2026-11-23T08:00:04.000001Z  INFO main mail_engine::compose: reply queued to=<{CONTACT}> cc={CONTACT}"),
        format!("2026-11-23T08:00:05.000001Z  WARN main mail_engine::reading::files: copy could not be removed path=/tmp/attachments/{ATTACHMENT} err=denied"),
        format!("2026-11-23T08:00:06.000001Z  WARN main mail_engine::sync: mailbox not found: {FOLDER}"),
        format!("2026-11-23T08:00:07.000001Z  WARN main mail_engine::credentials: login password={PASSWORD}"),
        "2026-11-23T08:00:08.000001Z  INFO main mail_engine::sync: sync-pass-marker account=00000000-0000-4000-8000-000000001815".into(),
    ]
}

#[tokio::test]
async fn the_file_holds_facts_and_counts_but_no_secret_and_no_mail_content() {
    let dir = TempDir::new().expect("temp dir");
    let core = start(&dir).await;
    assert!(core.startup_error().is_none());
    let account = seed(&core).await;
    let log_dir = dir.path().join("logs");
    write_log(&log_dir, "app.2026-11-23.log", &leaky_lines());

    let target = dir.path().join("out").join("diagnostics.txt");
    std::fs::create_dir_all(target.parent().expect("parent")).expect("out dir");
    let written = core
        .export_diagnostics(env(Some(log_dir)), target.clone())
        .await
        .expect("export");
    assert_eq!(written, target);
    let text = std::fs::read_to_string(&target).expect("read the file");

    for secret in [
        PASSWORD,
        EMAIL,
        DISPLAY_NAME,
        USER_NAME,
        IMAP_HOST,
        SMTP_HOST,
        "analytical-engine",
        FOLDER,
        "Geheimprojekt",
        SUBJECT,
        "Kontoauszug",
        BODY,
        "Überweisung",
        ATTACHMENT,
        CONTACT,
        CONTACT_NAME,
        "babbage",
        LOGGED_TOKEN,
        "a0AfH6SMBx3pQ",
        LOGGED_KEY,
        "0123456789abcdef",
    ] {
        assert!(
            !text.to_lowercase().contains(&secret.to_lowercase()),
            "{secret:?} leaked into the diagnostics file:\n{text}"
        );
    }

    // What it is for.
    for expected in [
        "Test Mail diagnostics",
        "version: 1.2.3",
        "commit: abc1234",
        "build date: 2026-11-23T08:00:00Z",
        &format!("operating system: {}", std::env::consts::OS),
        &format!("architecture: {}", std::env::consts::ARCH),
        "locale: de-DE",
        "web view: 621.1.15",
        "count: 1",
        &format!("account 1 ({account}):"),
        "provider: other",
        "sign-in: password (system keychain)",
        "incoming: tls, port 993",
        "outgoing: tls, port 465",
        "writing: off (read-only)",
        "folders: 2 (1 without a role, 1 fully synced)",
        "messages: 1",
        "stored bodies: 1",
        "  account: 1",
        "  message: 1",
        "theme: system",
        "settings sync: off",
        "AI in the app: off",
        "--- app.2026-11-23.log: ",
        "sync-pass-marker account=00000000-0000-4000-8000-000000001815",
        "database ready schema_version=15",
        "<email>",
        "[redacted]",
        "error=internal error: copy ~/…",
        "error=the address <email> is not valid",
    ] {
        assert!(text.contains(expected), "{expected:?} missing:\n{text}");
    }
    let home = home();
    assert!(
        home.len() < 2 || !text.contains(&home),
        "the home directory {home:?} leaked:\n{text}"
    );
    // The temporary file is gone.
    let left: Vec<_> = std::fs::read_dir(target.parent().expect("parent"))
        .expect("list")
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left.len(), 1, "{left:?}");
}

#[tokio::test]
async fn only_the_newest_log_files_and_only_their_ends_are_included() {
    let dir = TempDir::new().expect("temp dir");
    let core = start(&dir).await;
    let log_dir = dir.path().join("logs");
    // Five days of about 1 MiB each; every line names its file and number.
    for day in 1..=5 {
        let name = format!("app.2026-11-0{day}.log");
        let lines: Vec<String> = (0..12_000)
            .map(|n| format!("2026-11-0{day}T10:00:00.000000Z  INFO main mail_engine: day {day} line {n:05} {}", "x".repeat(60)))
            .collect();
        write_log(&log_dir, &name, &lines);
        assert!(std::fs::metadata(log_dir.join(&name)).expect("size").len() > 1_000_000);
    }
    std::fs::write(log_dir.join("notes.txt"), "not a log").expect("other file");

    let report = core.diagnostics_report(&env(Some(log_dir))).await;
    let bound = MAX_LOG_FILES * MAX_LOG_BYTES_PER_FILE + 64 * 1024;
    assert!(
        report.len() <= bound,
        "{} bytes, more than {bound}",
        report.len()
    );
    for day in [5, 4, 3] {
        assert!(
            report.contains(&format!("--- app.2026-11-0{day}.log: ")),
            "day {day}"
        );
        assert!(
            report.contains(&format!("day {day} line 11999 ")),
            "end of day {day}"
        );
        assert!(
            !report.contains(&format!("day {day} line 00000 ")),
            "start of day {day}"
        );
    }
    for day in [2, 1] {
        assert!(
            !report.contains(&format!("app.2026-11-0{day}.log")),
            "day {day}"
        );
    }
    assert!(!report.contains("not a log"));
    // Newest first, each line whole.
    let newest = report.find("--- app.2026-11-05.log: ").expect("newest");
    let older = report.find("--- app.2026-11-04.log: ").expect("older");
    assert!(newest < older);
    assert!(
        report
            .lines()
            .filter(|l| l.contains(" line "))
            .all(|l| l.ends_with(&"x".repeat(60)))
    );
}

#[tokio::test]
async fn a_broken_database_or_missing_logs_still_give_a_file() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("data")).expect("data dir");
    std::fs::write(
        dir.path().join("data").join(crate::DATABASE_FILE),
        b"this is not an SQLite database, it is long enough to have a header....",
    )
    .expect("broken database");
    let core = start(&dir).await;
    assert!(core.startup_error().is_some());

    let report = core
        .diagnostics_report(&env(Some(dir.path().join("no-logs-here"))))
        .await;
    assert!(
        report.contains("database: unavailable (database)"),
        "{report}"
    );
    assert!(
        report.contains("accounts: unavailable (database)"),
        "{report}"
    );
    assert!(
        report.contains("logs: could not be read (NotFound)"),
        "{report}"
    );
    assert!(report.contains("version: 1.2.3"), "{report}");

    let report = core.diagnostics_report(&env(None)).await;
    assert!(report.contains("The log directory is not known to this build."));
}

#[tokio::test]
async fn a_file_that_cannot_be_written_is_a_typed_error_and_leaves_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let core = start(&dir).await;
    let missing = dir.path().join("gone").join("diagnostics.txt");
    let err = core
        .export_diagnostics(env(None), missing)
        .await
        .expect_err("no folder");
    assert_eq!(err.code(), ErrorCode::DiagnosticsWriteFailed);
    assert_eq!(err.detail().as_deref(), Some("NotFound"));

    // An existing file is replaced as a whole.
    let target = dir.path().join("diagnostics.txt");
    std::fs::write(&target, "old").expect("old file");
    core.export_diagnostics(env(None), target.clone())
        .await
        .expect("export");
    let text = std::fs::read_to_string(&target).expect("read");
    assert!(text.starts_with("Test Mail diagnostics\n"), "{text}");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect();
    assert!(names.is_empty(), "{names:?}");
}

#[tokio::test]
async fn the_proposed_name_has_the_product_and_the_date() {
    let dir = TempDir::new().expect("temp dir");
    let core = start(&dir).await;
    let name = core.diagnostics_file_name("Vice  Mail!");
    assert!(name.starts_with("vice-mail-diagnostics-20"), "{name}");
    assert!(name.ends_with(".txt") && name.len() == "vice-mail-diagnostics-2026-11-23.txt".len());
    assert!(
        core.diagnostics_file_name("")
            .starts_with("app-diagnostics-")
    );
}

#[test]
fn provider_kinds_come_from_the_table_never_from_the_host() {
    let server = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: "u".into(),
    };
    assert_eq!(
        provider_kind(&server("imap.gmail.com", 993), false),
        "gmail"
    );
    assert_eq!(
        provider_kind(&server("IMAP.mail.me.com.", 993), false),
        "icloud"
    );
    assert_eq!(
        provider_kind(&server("127.0.0.1", 1143), false),
        "proton-bridge"
    );
    assert_eq!(provider_kind(&server("127.0.0.1", 3143), false), "local");
    assert_eq!(
        provider_kind(&server("mail.firma.example", 993), true),
        "gmail"
    );
    assert_eq!(
        provider_kind(&server("mail.firma.example", 993), false),
        "other"
    );
    for provider in crate::discovery::known::all() {
        assert_ne!(
            provider_slug(provider.name),
            "known-other",
            "{}",
            provider.name
        );
    }
}
