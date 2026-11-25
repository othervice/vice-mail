//! Stage 19, block C (decision 0057): taking over a Thunderbird profile.
//!
//! The shell names the place Thunderbird keeps its profiles (`~/Library/Thunderbird` on macOS,
//! `%APPDATA%\Thunderbird` on Windows) or a folder the user chose in the system dialog. The
//! core reads `profiles.ini`, the chosen profile's `prefs.js` (`mail_content::parse_prefs_js`,
//! `thunderbird_accounts`), the signature files the identities name (at most
//! [`MAX_SIGNATURE_FILE_BYTES`], text only) and lists the mbox files of "Local Folders". It
//! opens nothing else: **never `logins.json` or `key4.db`**, so no password is ever read. The
//! parsed profile stays in memory under a random token (four at most) and the UI sees no path.
//!
//! What the user takes over:
//! - an IMAP account: the UI fills the normal account setup with the servers and the first
//!   identity; the user enters the password there and the account starts read-only like every
//!   new one. Afterwards [`AppCore::apply_thunderbird_account`] adds the other identities and
//!   creates the signatures as the identities' defaults;
//! - a local folder: imported into a folder of an account through the outbox like any mbox
//!   file ([`AppCore::import_thunderbird_folder`], `crate::mbox`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use mail_content::{TbAccount, TbSecurity, TbServerKind, TbSignature, ThunderbirdProfileEntry};
use mail_model::{
    AccountId, CoreError, IdentityInput, MAX_SIGNATURE_FILE_BYTES, MAX_TAKEOVER_FOLDERS, MailboxId,
    MboxJob, Security, ServerSettings, SignatureInput, TakeoverAccount, TakeoverAccountKind,
    TakeoverApplied, TakeoverFolder, TakeoverIdentity, TakeoverPreview, TakeoverSignature,
    TakeoverToken, ThunderbirdProfile,
};

use crate::AppCore;
use crate::core::Connector;

/// Largest `profiles.ini` read.
const MAX_PROFILES_INI_BYTES: u64 = 256 * 1024;

/// Largest `prefs.js` read.
const MAX_PREFS_BYTES: u64 = 16 * 1024 * 1024;

/// Profiles kept open at once.
const MAX_OPEN: usize = 4;

/// Folders below "Local Folders" this deep at most.
const MAX_DEPTH: usize = 8;

/// Characters of a signature's preview.
const PREVIEW_CHARS: usize = 2_000;

/// Files of a Thunderbird mail folder that are not mbox files.
const NOT_MBOX: &[&str] = &[
    "msf", "dat", "json", "sqlite", "html", "htm", "js", "log", "bak", "txt", "mozlz4",
];

/// A signature as read: HTML or text, the whole of it.
#[derive(Debug, Clone)]
struct ReadSignature {
    html: bool,
    text: String,
}

#[derive(Debug, Clone)]
struct Opened {
    token: TakeoverToken,
    preview: TakeoverPreview,
    /// Per account, per identity: the signature to create.
    signatures: Vec<Vec<Option<ReadSignature>>>,
    /// Per folder: its file.
    files: Vec<PathBuf>,
}

/// The profiles opened in this session.
#[derive(Debug, Default)]
pub(crate) struct TakeoverState {
    opened: Mutex<VecDeque<Opened>>,
}

impl TakeoverState {
    fn keep(&self, opened: Opened) {
        let mut all = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
        all.push_back(opened);
        while all.len() > MAX_OPEN {
            all.pop_front();
        }
    }

    fn get(&self, token: &TakeoverToken) -> Result<Opened, CoreError> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|o| o.token == *token)
            .cloned()
            .ok_or_else(|| CoreError::NotFound("the Thunderbird profile is no longer open".into()))
    }
}

/// Reads a small text file whole, at most `limit` bytes; `None` when it is missing, larger, or
/// not UTF-8.
fn read_text(path: &Path, limit: u64) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// A path Thunderbird wrote: `[ProfD]` relative to the profile, else as written.
fn profile_path(profile: &Path, written: &str) -> PathBuf {
    match written.strip_prefix("[ProfD]") {
        Some(rest) => rest
            .split(['/', '\\'])
            .filter(|p| !p.is_empty() && *p != "..")
            .fold(profile.to_owned(), |path, part| path.join(part)),
        None => PathBuf::from(written),
    }
}

fn security(security: TbSecurity) -> (Security, bool) {
    match security {
        TbSecurity::Tls => (Security::Tls, false),
        TbSecurity::StartTls => (Security::Starttls, false),
        TbSecurity::None => (Security::Starttls, true),
    }
}

fn signature_of(profile: &Path, signature: &TbSignature) -> (Option<ReadSignature>, bool) {
    match signature {
        TbSignature::None => (None, false),
        TbSignature::Text(text) => (
            Some(ReadSignature {
                html: false,
                text: text.clone(),
            }),
            false,
        ),
        TbSignature::Html(html) => (
            Some(ReadSignature {
                html: true,
                text: html.clone(),
            }),
            false,
        ),
        TbSignature::File { path, html } => {
            match read_text(
                &profile_path(profile, path),
                u64::from(MAX_SIGNATURE_FILE_BYTES),
            ) {
                Some(text) if !text.trim().is_empty() => {
                    (Some(ReadSignature { html: *html, text }), false)
                }
                _ => (None, true),
            }
        }
    }
}

fn preview_of(signature: &ReadSignature, from_file: bool) -> TakeoverSignature {
    let text = if signature.html {
        mail_content::html_to_text(&signature.text)
    } else {
        signature.text.clone()
    };
    TakeoverSignature {
        html: signature.html,
        preview: text.trim().chars().take(PREVIEW_CHARS).collect(),
        from_file,
    }
}

/// The mbox files below `dir` (Thunderbird: a folder is a file, its subfolders are in
/// `<name>.sbd`), in name order, with their paths below "Local Folders".
fn local_folders(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<(String, PathBuf, u64)>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= usize::try_from(MAX_TAKEOVER_FOLDERS).unwrap_or(usize::MAX) {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // Never follow links out of the profile.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if let Some(base) = name.strip_suffix(".sbd") {
                local_folders(&path, &format!("{prefix}{base}/"), depth + 1, out);
            }
            continue;
        }
        if !meta.is_file() || meta.len() == 0 {
            continue;
        }
        let extension = Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        if extension.is_some_and(|e| NOT_MBOX.contains(&e.as_str())) {
            continue;
        }
        if !starts_like_mbox(&path) {
            continue;
        }
        out.push((format!("{prefix}{name}"), path, meta.len()));
    }
}

/// The file's first line is an mbox separator.
fn starts_like_mbox(path: &Path) -> bool {
    use std::io::Read;
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::new();
    if file.take(1024).read_to_end(&mut head).is_err() {
        return false;
    }
    let line = head.split(|b| *b == b'\n').next().unwrap_or_default();
    mail_content::is_mbox_separator(line.strip_suffix(b"\r").unwrap_or(line))
}

fn server(
    host: &str,
    port: Option<u16>,
    security: Security,
    default: (u16, u16),
    user: &str,
) -> ServerSettings {
    ServerSettings {
        host: host.trim().to_owned(),
        port: port.unwrap_or(match security {
            Security::Tls => default.0,
            Security::Starttls => default.1,
        }),
        security,
        username: user.to_owned(),
    }
}

/// The preview of one profile directory, read on the blocking pool.
fn read_profile(dir: &Path, own: &[String], token: TakeoverToken) -> Result<Opened, CoreError> {
    let prefs = dir.join("prefs.js");
    let text = read_text(&prefs, MAX_PREFS_BYTES).ok_or_else(|| {
        CoreError::InvalidArgument(
            "this folder is no Thunderbird profile (no readable prefs.js)".into(),
        )
    })?;
    let accounts: Vec<TbAccount> =
        mail_content::thunderbird_accounts(&mail_content::parse_prefs_js(&text));
    let mut preview_accounts = Vec::new();
    let mut signatures = Vec::new();
    let mut folders = Vec::new();
    for account in &accounts {
        if account.server.kind == TbServerKind::Local {
            let written = account
                .server
                .directory
                .as_deref()
                .unwrap_or("[ProfD]Mail/Local Folders");
            let mut root = profile_path(dir, written);
            if !root.is_dir() {
                root = dir.join("Mail").join("Local Folders");
            }
            local_folders(&root, "", 0, &mut folders);
            continue;
        }
        let Some(first) = account.identities.first() else {
            continue;
        };
        let kind = match account.server.kind {
            TbServerKind::Imap => TakeoverAccountKind::Imap,
            TbServerKind::Pop3 => TakeoverAccountKind::Pop3,
            _ => TakeoverAccountKind::Other,
        };
        let (incoming, incoming_plain) = security(account.server.security);
        let user = if account.server.user_name.trim().is_empty() {
            first.email.clone()
        } else {
            account.server.user_name.clone()
        };
        let imap = (kind == TakeoverAccountKind::Imap).then(|| {
            server(
                &account.server.host,
                account.server.port,
                incoming,
                (993, 143),
                &user,
            )
        });
        let (smtp, smtp_plain) = match &first.smtp {
            Some(smtp) => {
                let (sec, plain) = security(smtp.security);
                let user = if smtp.user_name.trim().is_empty() {
                    first.email.clone()
                } else {
                    smtp.user_name.clone()
                };
                (
                    Some(server(&smtp.host, smtp.port, sec, (465, 587), &user)),
                    plain,
                )
            }
            None => (None, false),
        };
        let mut identities = Vec::new();
        let mut account_signatures = Vec::new();
        for identity in &account.identities {
            let (signature, unreadable) = signature_of(dir, &identity.signature);
            let from_file = matches!(identity.signature, TbSignature::File { .. });
            identities.push(TakeoverIdentity {
                email: identity.email.clone(),
                name: identity.full_name.clone(),
                reply_to: identity.reply_to.clone(),
                signature: signature.as_ref().map(|s| preview_of(s, from_file)),
                signature_unreadable: unreadable,
            });
            account_signatures.push(signature);
        }
        preview_accounts.push(TakeoverAccount {
            index: u32::try_from(preview_accounts.len()).unwrap_or(u32::MAX),
            kind,
            name: account.server.name.clone(),
            email: first.email.clone(),
            display_name: first.full_name.clone(),
            imap,
            smtp,
            was_unencrypted: (kind == TakeoverAccountKind::Imap && incoming_plain) || smtp_plain,
            identities,
            already_set_up: own.iter().any(|e| e.eq_ignore_ascii_case(&first.email)),
        });
        signatures.push(account_signatures);
    }
    let profile_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Opened {
        preview: TakeoverPreview {
            token: token.clone(),
            profile_name,
            accounts: preview_accounts,
            folders: folders
                .iter()
                .enumerate()
                .map(|(index, (path, _, bytes))| TakeoverFolder {
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    path: path.clone(),
                    bytes: *bytes,
                })
                .collect(),
        },
        token,
        signatures,
        files: folders.into_iter().map(|(_, path, _)| path).collect(),
    })
}

fn profiles_of(base: &Path) -> Vec<ThunderbirdProfileEntry> {
    read_text(&base.join("profiles.ini"), MAX_PROFILES_INI_BYTES)
        .map(|text| mail_content::parse_profiles_ini(&text))
        .unwrap_or_default()
}

fn entry_dir(base: &Path, entry: &ThunderbirdProfileEntry) -> PathBuf {
    if entry.is_relative {
        entry
            .path
            .split(['/', '\\'])
            .filter(|p| !p.is_empty() && *p != "..")
            .fold(base.to_owned(), |path, part| path.join(part))
    } else {
        PathBuf::from(&entry.path)
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, CoreError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| CoreError::Internal(format!("takeover task failed: {err}")))
}

impl<C: Connector> AppCore<C> {
    /// Stage 19: the profiles in `profiles.ini` under `base` (the place Thunderbird keeps them);
    /// empty when there is none.
    pub async fn thunderbird_profiles(
        &self,
        base: PathBuf,
    ) -> Result<Vec<ThunderbirdProfile>, CoreError> {
        let entries = blocking(move || profiles_of(&base)).await?;
        Ok(entries
            .into_iter()
            .enumerate()
            .map(|(index, entry)| ThunderbirdProfile {
                index: u32::try_from(index).unwrap_or(u32::MAX),
                name: entry.name,
                is_default: entry.is_default,
            })
            .collect())
    }

    /// Stage 19: opens profile `index` of `profiles.ini` under `base`.
    pub async fn open_thunderbird_profile(
        &self,
        base: PathBuf,
        index: u32,
    ) -> Result<TakeoverPreview, CoreError> {
        let dir = blocking(move || {
            let entries = profiles_of(&base);
            entries
                .get(usize::try_from(index).unwrap_or(usize::MAX))
                .map(|entry| entry_dir(&base, entry))
        })
        .await?
        .ok_or_else(|| CoreError::NotFound(format!("Thunderbird profile {index}")))?;
        self.open_thunderbird_dir(dir).await
    }

    /// Stage 19: opens a folder the user chose: a profile (with `prefs.js`), or Thunderbird's
    /// own folder (with `profiles.ini`: its default profile).
    pub async fn open_thunderbird_dir(&self, dir: PathBuf) -> Result<TakeoverPreview, CoreError> {
        let own: Vec<String> = self
            .store_ref()?
            .accounts()
            .await?
            .into_iter()
            .map(|a| a.email)
            .collect();
        let token = TakeoverToken::from_random(uuid::Uuid::new_v4().as_u128());
        let opened = blocking(move || {
            let dir = if dir.join("prefs.js").is_file() {
                dir
            } else {
                let entries = profiles_of(&dir);
                let entry = entries
                    .iter()
                    .find(|e| e.is_default)
                    .or_else(|| entries.first())
                    .ok_or_else(|| {
                        CoreError::InvalidArgument(
                            "this folder is no Thunderbird profile (no prefs.js or profiles.ini)"
                                .into(),
                        )
                    })?;
                entry_dir(&dir, entry)
            };
            read_profile(&dir, &own, token)
        })
        .await??;
        let preview = opened.preview.clone();
        tracing::info!(
            accounts = preview.accounts.len(),
            folders = preview.folders.len(),
            "Thunderbird profile opened"
        );
        self.takeover().keep(opened);
        Ok(preview)
    }

    /// Stage 19: after the user set up the account (normal setup, with the password, read
    /// only): adds Thunderbird's other identities of account `index` to `account` (its own
    /// address takes Thunderbird's name and Reply-To) and creates their signatures as the
    /// identities' defaults for new mails and replies.
    pub async fn apply_thunderbird_account(
        &self,
        token: TakeoverToken,
        index: u32,
        account: AccountId,
    ) -> Result<TakeoverApplied, CoreError> {
        let opened = self.takeover().get(&token)?;
        let position = usize::try_from(index).unwrap_or(usize::MAX);
        let source = opened
            .preview
            .accounts
            .get(position)
            .ok_or_else(|| CoreError::NotFound(format!("Thunderbird account {index}")))?;
        let signatures = opened.signatures.get(position).cloned().unwrap_or_default();
        let mut applied = TakeoverApplied {
            account_id: Some(account),
            ..TakeoverApplied::default()
        };
        for (identity, signature) in source.identities.iter().zip(signatures) {
            let existing = self
                .list_identities(account)
                .await?
                .into_iter()
                .find(|i| i.email.eq_ignore_ascii_case(&identity.email));
            let input = IdentityInput {
                email: identity.email.clone(),
                name: identity.name.clone(),
                reply_to: identity.reply_to.clone(),
            };
            let id = match existing {
                Some(found) => self.update_identity(found.id, input).await?.id,
                None => {
                    applied.identities += 1;
                    self.add_identity(account, input).await?.id
                }
            };
            let Some(signature) = signature else {
                continue;
            };
            let html = if signature.html {
                signature.text
            } else {
                mail_content::text_to_editor_html(&signature.text)
            };
            let label = if identity.name.trim().is_empty() {
                identity.email.clone()
            } else {
                identity.name.clone()
            };
            let saved = self
                .save_signature(SignatureInput {
                    id: None,
                    name: format!("{label} (Thunderbird)").chars().take(100).collect(),
                    html,
                })
                .await?;
            self.set_identity_signature_defaults(
                id,
                Some(saved.signature.id.clone()),
                Some(saved.signature.id),
            )
            .await?;
            applied.signatures += 1;
        }
        tracing::info!(
            identities = applied.identities,
            signatures = applied.signatures,
            "Thunderbird account taken over"
        );
        Ok(applied)
    }

    /// Stage 19: imports local folder `index` of the profile into `mailbox` through the outbox
    /// (an mbox job; `read_only` without writes).
    pub async fn import_thunderbird_folder(
        &self,
        token: TakeoverToken,
        index: u32,
        mailbox: MailboxId,
    ) -> Result<MboxJob, CoreError> {
        let opened = self.takeover().get(&token)?;
        let path = opened
            .files
            .get(usize::try_from(index).unwrap_or(usize::MAX))
            .cloned()
            .ok_or_else(|| CoreError::NotFound(format!("local folder {index}")))?;
        self.start_mbox_import(mailbox, path).await
    }
}

#[cfg(test)]
mod tests;
