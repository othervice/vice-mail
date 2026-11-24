//! Stage 15: "Export diagnostics". One plain-text file a person can read before sending it:
//! app and system facts, the data directory's size, counts from the database, each account's
//! kind and state, settings that are not personal, and the end of the recent log files.
//!
//! Never in the file: passwords, keys, tokens or OAuth data (the keychain is not read at all),
//! email addresses, names, user names, server host names of the user's own servers, subjects,
//! message text, attachment names, contacts, folder names. Provider servers from the built-in
//! table (`imap.gmail.com`) count as a provider kind, not as personal data. The log lines are
//! redacted once more ([`redact`]), because a log is written by many hands. What the file holds
//! is listed for users in `docs/PRIVACY.md` ("Diagnostics").
//!
//! Collecting never fails: a part that cannot be read is described in the file instead (the
//! export matters most when something is broken). Only writing the file can fail
//! ([`mail_model::DiagnosticsError`]).

mod redact;
mod system;
#[cfg(test)]
mod tests;

/// The user's home directory (also for the paths the MCP settings show as `~/…`).
pub(crate) use system::home_dir;

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use mail_model::{
    Account, AccountState, CoreError, DiagnosticsError, ErrorCode, IpcError, MailboxRole,
    ServerSettings, UpdateStatus,
};
use mail_store::DatabaseStats;
use serde::Serialize;

use self::redact::{EMAIL, HOST, REMOVED, Redactor};
use crate::AppCore;
use crate::core::Connector;

use self::system::{MAX_LOG_BYTES_PER_FILE, MAX_LOG_FILES};

/// Placeholder of a known user name and of a known folder name in the logs.
const USER_NAME: &str = "<user name>";
const FOLDER: &str = "<folder>";

/// Hosts of the app's own update source (decision 0037): not personal.
const UPDATE_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// What only the shell (or the UI) knows about the running app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticsEnv {
    /// The product name from the app's configuration: the file's title and name.
    pub product_name: String,
    /// The app identifier (bundle id). Its folders appear in paths; it is not a host name.
    pub app_identifier: String,
    /// The app's log directory; `None` when unknown.
    pub log_dir: Option<PathBuf>,
    /// Version of the system's web view (WKWebView, WebView2), if known.
    pub webview_version: Option<String>,
    /// The language the web view reports (`navigator.language`), as the system locale.
    pub locale: String,
}

impl<C: Connector> AppCore<C> {
    /// The name the save dialog proposes: `<product>-diagnostics-YYYY-MM-DD.txt` (local date).
    pub fn diagnostics_file_name(&self, product_name: &str) -> String {
        let mut slug = String::new();
        for c in product_name.trim().chars() {
            if c.is_ascii_alphanumeric() {
                slug.push(c.to_ascii_lowercase());
            } else if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        let slug = if slug.is_empty() { "app" } else { slug };
        let date = jiff::Timestamp::from_millisecond(self.now_ms())
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_zoned_datetime(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%d")
            .to_string();
        format!("{slug}-diagnostics-{date}.txt")
    }

    /// Collects the report and writes it to `path` (chosen by the user), atomically. Returns
    /// the path. `diagnostics_write_failed` if the file cannot be written; nothing is left
    /// behind then.
    pub async fn export_diagnostics(
        &self,
        env: DiagnosticsEnv,
        path: PathBuf,
    ) -> Result<PathBuf, CoreError> {
        let report = self.diagnostics_report(&env).await;
        let bytes = report.len();
        let written = tokio::task::spawn_blocking(move || {
            write_atomically(&path, report.as_bytes()).map(|()| path)
        })
        .await
        .map_err(|err| CoreError::Internal(format!("diagnostics write task failed: {err}")))??;
        tracing::info!(bytes, "diagnostics exported");
        Ok(written)
    }

    /// The text of the diagnostics file.
    pub async fn diagnostics_report(&self, env: &DiagnosticsEnv) -> String {
        let redactor = self.diagnostics_redactor(env).await;
        let data_dir = self.data_dir().to_path_buf();
        let log_dir = env.log_dir.clone();
        let files = tokio::task::spawn_blocking(move || Files {
            os_version: system::os_version(),
            data: system::folder_size(&data_dir),
            data_dir: redactor.app_path(&data_dir.display().to_string()),
            log_dir: log_dir
                .as_deref()
                .map(|dir| redactor.app_path(&dir.display().to_string())),
            logs: log_dir
                .as_deref()
                .map(|dir| system::recent_logs(dir, &redactor)),
        })
        .await
        .unwrap_or_else(|err| {
            tracing::error!(%err, "diagnostics file task failed");
            Files::default()
        });

        let stats = match self.store_ref() {
            Ok(store) => store.diagnostics_stats().await.map_err(CoreError::from),
            Err(err) => Err(err),
        };

        let mut out = String::new();
        self.write_header(&mut out, env);
        self.write_system(&mut out, env, &files).await;
        self.write_data(&mut out, &files, &stats).await;
        self.write_accounts(&mut out, &stats).await;
        self.write_states(&mut out).await;
        self.write_settings(&mut out).await;
        write_logs(&mut out, &files);
        out
    }

    /// The redactor for the log lines: knows the accounts' personal strings and the hosts that
    /// may stay.
    async fn diagnostics_redactor(&self, env: &DiagnosticsEnv) -> Redactor {
        let mut redactor = base_redactor();
        if !env.app_identifier.trim().is_empty() {
            redactor.allow_host(&env.app_identifier);
        }
        let Ok(store) = self.store_ref() else {
            return redactor;
        };
        let accounts = store.accounts().await.unwrap_or_default();
        for account in &accounts {
            redactor.add_known(&account.email, EMAIL);
            redactor.add_known(&account.display_name, REMOVED);
            for server in [&account.imap, &account.smtp] {
                redactor.add_known(&server.username, USER_NAME);
                redactor.add_known(&server.host, HOST);
            }
            for identity in store.identities(account.id).await.unwrap_or_default() {
                redactor.add_known(&identity.email, EMAIL);
                redactor.add_known(&identity.name, REMOVED);
                if let Some(reply_to) = &identity.reply_to {
                    redactor.add_known(reply_to, EMAIL);
                }
            }
            for mailbox in store.mailboxes(account.id).await.unwrap_or_default() {
                if mailbox.role != MailboxRole::Custom {
                    continue;
                }
                redactor.add_known(&mailbox.path, FOLDER);
                redactor.add_known(&mailbox.name, FOLDER);
                if let Some(delimiter) = mailbox.delimiter.as_deref().filter(|d| !d.is_empty()) {
                    for segment in mailbox.path.split(delimiter) {
                        redactor.add_known(segment, FOLDER);
                    }
                }
            }
        }
        redactor
    }

    fn write_header(&self, out: &mut String, env: &DiagnosticsEnv) {
        let product = env.product_name.trim();
        let product = if product.is_empty() { "App" } else { product };
        line(out, &format!("{product} diagnostics"));
        line(out, &format!("Created: {}", time(self.now_ms())));
        out.push('\n');
        for text in [
            "This file helps to find the cause of a problem. Read it before you send it to anyone.",
            "It contains: the app and system versions, the size of the app's data, counts from the",
            "database, the kind and state of each account, settings that are not personal, and the",
            "end of the recent log files.",
            "It never contains: passwords, keys or tokens, email addresses, names, user names, the",
            "names of your own servers, subjects, message text, attachment names, contacts or folder",
            "names. The log lines were checked once more: anything that looked like one of these was",
            "replaced by a placeholder such as <email>, <host>, <path>, <removed> or [redacted].",
        ] {
            line(out, text);
        }
    }

    async fn write_system(&self, out: &mut String, env: &DiagnosticsEnv, files: &Files) {
        let info = self.app_info();
        section(out, "App");
        field(out, "version", &info.version);
        field(
            out,
            "commit",
            info.commit.as_deref().map_or("unknown", plain),
        );
        field(
            out,
            "build date",
            info.build_date.as_deref().map_or("unknown", plain),
        );
        field(out, "platform", &wire(&info.platform));
        section(out, "System");
        field(out, "operating system", std::env::consts::OS);
        field(
            out,
            "version",
            files.os_version.as_deref().unwrap_or("unknown"),
        );
        field(out, "architecture", std::env::consts::ARCH);
        field(out, "locale", &locale(&env.locale));
        match self.get_preferences().await {
            Ok(preferences) => field(out, "app language", &wire(&preferences.language)),
            Err(err) => unavailable(out, "app language", &err),
        }
        field(
            out,
            "web view",
            env.webview_version
                .as_deref()
                .filter(|v| system::is_version_text(v))
                .unwrap_or("unknown"),
        );
    }

    async fn write_data(
        &self,
        out: &mut String,
        files: &Files,
        stats: &Result<DatabaseStats, CoreError>,
    ) {
        section(out, "Data");
        field(out, "data directory", &files.data_dir);
        match &files.data {
            Some(size) => {
                field(
                    out,
                    "total",
                    &format!(
                        "{} in {} files{}",
                        bytes(size.bytes),
                        size.files,
                        if size.incomplete {
                            " (counting stopped early)"
                        } else {
                            ""
                        }
                    ),
                );
                for (name, b, f) in &size.entries {
                    field(
                        out,
                        &format!("  {name}"),
                        &format!("{} in {f} files", bytes(*b)),
                    );
                }
            }
            None => field(out, "total", "the folder could not be read"),
        }
        section(out, "Database");
        let store = match self.store_ref() {
            Ok(store) => store,
            Err(err) => {
                unavailable(out, "database", &err);
                return;
            }
        };
        let report = store.open_report();
        field(out, "schema version", &report.version.to_string());
        if report.previous_version != report.version {
            field(
                out,
                "migrated at this start from",
                &report.previous_version.to_string(),
            );
        }
        match stats {
            Ok(stats) => {
                field(
                    out,
                    "size",
                    &format!(
                        "{} ({} pages of {} bytes, {} free)",
                        bytes(
                            u64::try_from(stats.page_count.saturating_mul(stats.page_size))
                                .unwrap_or(0)
                        ),
                        stats.page_count,
                        stats.page_size,
                        stats.freelist_count
                    ),
                );
                line(out, "rows per table:");
                for (table, count) in &stats.tables {
                    field(out, &format!("  {table}"), &count.to_string());
                }
                line(out, "outbox operations by state:");
                if stats.operations.is_empty() {
                    line(out, "  none");
                }
                for (state, count) in &stats.operations {
                    field(out, &format!("  {state}"), &count.to_string());
                }
                if !stats.operation_errors.is_empty() {
                    line(out, "failed or conflicting operations by error code:");
                    for (code, count) in &stats.operation_errors {
                        field(out, &format!("  {code}"), &count.to_string());
                    }
                }
            }
            Err(err) => unavailable(out, "counts", err),
        }
        match self.get_search_index_status().await {
            Ok(index) => field(
                out,
                "search index",
                &format!("{} ({}%)", wire(&index.state), index.percent),
            ),
            Err(err) => unavailable(out, "search index", &err),
        }
    }

    async fn write_accounts(&self, out: &mut String, stats: &Result<DatabaseStats, CoreError>) {
        section(out, "Accounts");
        let accounts = match self.list_accounts().await {
            Ok(accounts) => accounts,
            Err(err) => {
                unavailable(out, "accounts", &err);
                return;
            }
        };
        let per_account = stats.as_ref().map_or(&[][..], |s| s.accounts.as_slice());
        field(out, "count", &accounts.len().to_string());
        for (n, account) in accounts.iter().enumerate() {
            let counts = per_account.iter().find(|s| s.account == account.id);
            let gmail = counts.is_some_and(|c| c.gmail);
            line(out, &format!("account {} ({}):", n + 1, account.id));
            field(out, "  provider", provider_kind(&account.imap, gmail));
            field(
                out,
                "  sign-in",
                match account.oauth {
                    // Stage 14: the refresh token is in the keychain, the access token in memory.
                    Some(mail_model::OAuthProvider::Microsoft) => {
                        "Microsoft sign-in (refresh token in the system keychain)"
                    }
                    Some(mail_model::OAuthProvider::Google) => {
                        "Google sign-in (refresh token in the system keychain)"
                    }
                    None => "password (system keychain)",
                },
            );
            field(out, "  incoming", &server(&account.imap));
            field(out, "  outgoing", &server(&account.smtp));
            field(
                out,
                "  writing",
                if account.writes_enabled {
                    "on"
                } else {
                    "off (read-only)"
                },
            );
            write_status(out, account);
            if let Some(c) = counts {
                field(
                    out,
                    "  folders",
                    &format!(
                        "{} ({} without a role, {} fully synced)",
                        c.mailboxes, c.custom_mailboxes, c.synced_mailboxes
                    ),
                );
                field(out, "  messages", &c.messages.to_string());
                field(out, "  stored bodies", &c.bodies.to_string());
            }
        }
    }

    async fn write_states(&self, out: &mut String) {
        section(out, "Background services");
        match self.get_settings_sync_status().await {
            Ok(sync) => {
                field(out, "settings sync", &wire(&sync.state));
                if let Some(error) = &sync.last_error {
                    field(out, "settings sync last error", &code(error.code));
                }
                if let Some(at) = sync.last_sync_at {
                    field(out, "settings sync last pass", &time(at));
                }
                field(
                    out,
                    "settings sync pending changes",
                    &sync.pending_changes.to_string(),
                );
                field(
                    out,
                    "settings sync rejected entries",
                    &sync.rejected_entries.to_string(),
                );
                field(
                    out,
                    "settings sync devices",
                    &sync.devices.len().to_string(),
                );
            }
            Err(err) => unavailable(out, "settings sync", &err),
        }
        match self.get_update_info().await {
            Ok(info) => {
                field(out, "updates supported", yes(info.supported));
                field(out, "updates checked automatically", yes(info.auto_check));
                if let Some(at) = info.last_check_ms {
                    field(out, "updates last check", &time(at));
                }
                field(out, "update state", &update_state(&info.status));
            }
            Err(err) => unavailable(out, "updates", &err),
        }
        match self.get_ai_settings().await {
            Ok(ai) => {
                field(out, "AI in the app", on(ai.enabled));
                let mut kinds: Vec<String> = ai.profiles.iter().map(|p| wire(&p.kind)).collect();
                kinds.sort();
                field(
                    out,
                    "AI profiles",
                    &if kinds.is_empty() {
                        "none".to_owned()
                    } else {
                        format!("{} ({})", kinds.len(), kinds.join(", "))
                    },
                );
            }
            Err(err) => unavailable(out, "AI in the app", &err),
        }
        match self.get_mcp_settings().await {
            Ok(mcp) => {
                field(out, "AI over MCP", on(mcp.enabled));
                field(out, "MCP host", &wire(&mcp.status.state));
                field(
                    out,
                    "MCP clients",
                    &mcp.clients
                        .iter()
                        .filter(|c| c.revoked_at.is_none())
                        .count()
                        .to_string(),
                );
                let shared = mcp
                    .accounts
                    .iter()
                    .filter(|a| a.access != mail_model::McpAccountAccess::Hidden)
                    .count();
                field(out, "MCP shared accounts", &shared.to_string());
            }
            Err(err) => unavailable(out, "AI over MCP", &err),
        }
    }

    async fn write_settings(&self, out: &mut String) {
        section(out, "Settings");
        match self.get_preferences().await {
            Ok(p) => {
                field(out, "theme", &wire(&p.theme));
                field(out, "language", &wire(&p.language));
            }
            Err(err) => unavailable(out, "appearance", &err),
        }
        match self.get_ui_preferences().await {
            Ok(ui) => {
                field(out, "text size", &format!("{}%", ui.text_size));
                field(out, "accent colours", &wire(&ui.accent_theme));
                field(out, "conversations", on(ui.conversations));
                field(out, "AI panel", &wire(&ui.ai_panel));
                field(
                    out,
                    "changed shortcuts",
                    &(ui.shortcuts.macos.len() + ui.shortcuts.windows.len()).to_string(),
                );
            }
            Err(err) => unavailable(out, "interface", &err),
        }
        match self.get_reading_preferences().await {
            Ok(r) => field(out, "mark as read on open", &wire(&r.mark_read_on_open)),
            Err(err) => unavailable(out, "reading", &err),
        }
        match self.get_prefetch_settings().await {
            Ok(p) => {
                field(out, "prefetch period", &wire(&p.period));
                field(
                    out,
                    "prefetch size limit",
                    &bytes(u64::from(p.max_message_size)),
                );
            }
            Err(err) => unavailable(out, "prefetch", &err),
        }
        match self.get_compose_settings().await {
            Ok(c) => {
                field(out, "send delay", &format!("{} s", c.send_delay_seconds));
                field(out, "quote position", &wire(&c.default_quote_position));
                field(out, "archive on reply", on(c.archive_on_reply));
                field(out, "read receipts", &wire(&c.read_receipts));
            }
            Err(err) => unavailable(out, "composing", &err),
        }
        match self.get_inbox_settings().await {
            Ok(i) => {
                field(out, "inbox categories", on(i.categories));
                field(out, "bundle notifications", on(i.bundle_notifications));
                field(out, "bundle newsletters", on(i.bundle_newsletters));
            }
            Err(err) => unavailable(out, "inbox", &err),
        }
        match self.get_notification_settings().await {
            Ok(n) => {
                field(out, "notifications", on(n.enabled));
                field(out, "notify new mail", on(n.new_mail));
                field(out, "notify failed sends", on(n.failed_sends));
                field(out, "notify snoozes", on(n.snoozes));
                field(out, "quiet hours", on(n.quiet_hours.enabled));
                field(out, "hide content", on(n.hide_content));
                field(out, "badge", &wire(&n.badge));
                field(out, "sound", on(n.sound.enabled));
                field(out, "muted accounts", &n.muted_accounts.len().to_string());
            }
            Err(err) => unavailable(out, "notifications", &err),
        }
    }
}

/// A redactor without account knowledge: the home directory, the system's user name and the
/// hosts that may stay (provider table, update source).
fn base_redactor() -> Redactor {
    let mut redactor = Redactor::new(
        system::home_dir().as_deref(),
        system::user_name().as_deref(),
    );
    for provider in crate::discovery::known::all() {
        redactor.allow_host(provider.imap.host);
        redactor.allow_host(provider.smtp.host);
    }
    for host in UPDATE_HOSTS {
        redactor.allow_host(host);
    }
    redactor
}

/// Redacts one text before it is written to the log (finding V-07): addresses, host names,
/// paths, secret-looking values, as in a diagnostics export but without the accounts' names.
pub(crate) fn redact_for_log(text: &str) -> String {
    base_redactor().line(text)
}

/// What the blocking part gathered.
#[derive(Default)]
struct Files {
    os_version: Option<String>,
    data: Option<system::FolderSize>,
    data_dir: String,
    log_dir: Option<String>,
    logs: Option<Result<Vec<system::LogTail>, String>>,
}

fn write_status(out: &mut String, account: &Account) {
    let status = &account.status;
    field(out, "  state", &wire(&status.state));
    if let Some(error) = &status.error {
        field(out, "  last error", &code(error.code));
    } else if status.state == AccountState::Error || status.state == AccountState::Offline {
        field(out, "  last error", "none recorded");
    }
    if let Some(at) = status.last_sync_at {
        field(out, "  last complete sync", &time(at));
    }
    if let Some(at) = status.retry_at {
        field(out, "  next attempt", &time(at));
    }
}

fn write_logs(out: &mut String, files: &Files) {
    section(out, "Logs");
    let Some(dir) = &files.log_dir else {
        line(out, "The log directory is not known to this build.");
        return;
    };
    field(out, "log directory", dir);
    let tails = match &files.logs {
        Some(Ok(tails)) => tails,
        Some(Err(kind)) => {
            field(out, "logs", &format!("could not be read ({kind})"));
            return;
        }
        None => return,
    };
    if tails.is_empty() {
        line(out, "No log files yet.");
    }
    field(
        out,
        "included",
        &format!(
            "the newest {MAX_LOG_FILES} files at most, the last {} of each",
            bytes(MAX_LOG_BYTES_PER_FILE as u64)
        ),
    );
    for tail in tails {
        out.push('\n');
        line(
            out,
            &format!(
                "--- {}: {}{} ---",
                tail.name,
                bytes(tail.size),
                if tail.cut { ", end only" } else { "" }
            ),
        );
        for text in &tail.lines {
            line(out, text);
        }
    }
}

/// Writes `bytes` to `path` through a temporary file next to it; removes it on failure.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), CoreError> {
    let failed =
        |err: &std::io::Error| CoreError::from(DiagnosticsError::WriteFailed(system::io_kind(err)));
    let (Some(dir), Some(name)) = (
        path.parent().filter(|d| !d.as_os_str().is_empty()),
        path.file_name(),
    ) else {
        return Err(CoreError::InvalidArgument(
            "the diagnostics file needs a folder and a name".into(),
        ));
    };
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(name);
    temp_name.push(format!(".{}.part", std::process::id()));
    let temp = dir.join(temp_name);
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    result.map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        tracing::warn!(kind = ?err.kind(), "diagnostics file could not be written");
        failed(&err)
    })
}

/// The provider from the built-in table whose IMAP server the account uses, `gmail` if the
/// server said so at login, `local` for a server on this computer, else `other`.
fn provider_kind(imap: &ServerSettings, gmail: bool) -> &'static str {
    let host = imap.host.trim().trim_end_matches('.');
    let known = crate::discovery::known::all().iter().find(|p| {
        p.imap.host.eq_ignore_ascii_case(host) && (!is_loopback(host) || p.imap.port == imap.port)
    });
    if let Some(provider) = known {
        return provider_slug(provider.name);
    }
    if gmail || mail_model::is_gmail_host(host) {
        "gmail"
    } else if is_loopback(host) {
        "local"
    } else {
        "other"
    }
}

/// A fixed lowercase name per provider of the table.
fn provider_slug(name: &str) -> &'static str {
    match name {
        "Gmail" => "gmail",
        "iCloud" => "icloud",
        "Outlook.com" => "outlook",
        "Yahoo" => "yahoo",
        "GMX" => "gmx",
        "WEB.DE" => "webde",
        "Posteo" => "posteo",
        "mailbox.org" => "mailbox-org",
        "Fastmail" => "fastmail",
        "Proton Mail" => "proton-bridge",
        "Telekom" => "telekom",
        "Infomaniak" => "infomaniak",
        _ => "known-other",
    }
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.starts_with("127.") || host == "[::1]"
}

/// Security and port of a server; never its host or user name.
fn server(settings: &ServerSettings) -> String {
    format!("{}, port {}", wire(&settings.security), settings.port)
}

fn update_state(status: &UpdateStatus) -> String {
    let state = serde_json::to_value(status)
        .ok()
        .and_then(|v| v.get("state").and_then(|s| s.as_str().map(str::to_owned)))
        .unwrap_or_else(|| "unknown".to_owned());
    match status {
        UpdateStatus::Failed { error, .. } => format!("{state} ({})", code(error.code)),
        _ => state,
    }
}

/// The wire name of an enum value (`afterDelay`, `macos`), or its compact JSON.
fn wire<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        Ok(other) => other.to_string(),
        Err(_) => "unknown".to_owned(),
    }
}

fn code(code: ErrorCode) -> String {
    wire(&code)
}

fn unavailable(out: &mut String, name: &str, err: &CoreError) {
    let error = IpcError::from(err);
    field(out, name, &format!("unavailable ({})", code(error.code)));
}

/// RFC 3339 in UTC, to the second.
fn time(ms: i64) -> String {
    jiff::Timestamp::from_millisecond(ms)
        .map(|ts| ts.strftime("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} bytes");
    }
    #[allow(clippy::cast_precision_loss)] // a rounded size for people
    let mut value = n as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!(
        "{value:.1} {} ({n} bytes)",
        UNITS.get(unit).unwrap_or(&"TiB")
    )
}

fn on(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn yes(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// Only a short language tag (`de-DE`, `en`) passes; anything else is `unknown`.
fn locale(value: &str) -> String {
    let value = value.trim();
    let ok = !value.is_empty()
        && value.len() <= 35
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        value.to_owned()
    } else {
        "unknown".to_owned()
    }
}

/// Text from the build that is printed as is only if it is plain (commit, build date).
fn plain(value: &str) -> &str {
    if value.len() <= 64
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '+'))
    {
        value
    } else {
        "unknown"
    }
}

fn section(out: &mut String, title: &str) {
    let _ = write!(out, "\n[{title}]\n");
}

fn field(out: &mut String, name: &str, value: &str) {
    let _ = writeln!(out, "{name}: {value}");
}

fn line(out: &mut String, text: &str) {
    out.push_str(text);
    out.push('\n');
}
