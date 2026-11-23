//! Public API of the store for stage 3: message bodies, previews, the data of the reading pane,
//! prefetch candidates and settings, and the remote-content allowlist (decision 0009).
//!
//! **Bodies are files, the database holds the reference.** A write compresses (zstd) and
//! writes the file atomically on Tokio's blocking pool first, then records it in one writer
//! transaction, then removes files nothing refers to any more. A read looks the reference up on
//! a reader and reads and decompresses on the blocking pool. Neither blocks a database thread
//! or a Tokio worker with file work.
//!
//! **A body is a cache.** A missing or damaged file (checksum, size) is logged, its reference
//! is forgotten and the read reports "not stored", so the engine fetches it again. Files left
//! behind by a crash or a failed removal are found by [`Store::sweep_body_files`].

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use mail_model::{
    AccountId, BodySection, FetchedSection, MessageId, MessageStructure, PrefetchSettings,
    normalize_sender,
};

use crate::bodies::{self, BodyInfo, NewPart, Placement};
use crate::body_files::{self, SweepReport};
use crate::fts::{self, BodyText};
use crate::reading::{self, MessageLocation, PrefetchCandidate, StoredMessage};
use crate::settings::now_millis;
use crate::{GLOBAL_SCOPE, Store, StoreError};

/// Runs file work on Tokio's blocking pool.
async fn blocking<T, F>(f: F) -> Result<T, StoreError>
where
    F: FnOnce() -> Result<T, StoreError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| StoreError::JobPanicked(format!("body file task: {err}")))?
}

impl Store {
    // ---------------------------------------------------------------------------------------
    // Bodies
    // ---------------------------------------------------------------------------------------

    /// Stores the whole raw message (`BODY[]`) of `message`, replacing whatever was stored
    /// (also a partial body). [`StoreError::NotFound`] if the message does not exist (any
    /// more); then nothing stays on disk.
    pub async fn store_body(
        &self,
        message: MessageId,
        raw: Vec<u8>,
    ) -> Result<BodyInfo, StoreError> {
        self.store_bodies(vec![(message, raw)])
            .await?
            .pop()
            .ok_or_else(|| StoreError::NotFound(format!("message {message}")))
    }

    /// Stores several whole messages, as one FETCH of the prefetch returns them, in one
    /// transaction. Bodies of one account are packed into files of up to
    /// [`BODY_PACK_LIMIT`] raw bytes, so a batch of small mails costs one `fsync` instead of
    /// one each (decision 0009); a larger body gets a file of its own. Messages that do not
    /// exist (any more) are skipped; the result has the stored ones, in input order.
    pub async fn store_bodies(
        &self,
        bodies: Vec<(MessageId, Vec<u8>)>,
    ) -> Result<Vec<BodyInfo>, StoreError> {
        let ids: Vec<MessageId> = bodies.iter().map(|(id, _)| *id).collect();
        let accounts = self
            .read(move |c| {
                ids.iter()
                    .map(|id| bodies::account_of(c, *id))
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
        let now = now_millis();
        let packs = pack(bodies, accounts, now);
        if packs.is_empty() {
            return Ok(Vec::new());
        }
        let root = self.bodies_dir().to_owned();
        let source = self.body_text_source();
        let (written, texts) = {
            let root = root.clone();
            blocking(move || {
                // Stage 7: the text for the full-text index, made before the bytes are packed.
                let texts: Vec<(MessageId, BodyText)> = match &source {
                    Some(source) => packs
                        .iter()
                        .flat_map(|p| p.entries.iter())
                        .map(|(id, raw)| (*id, source.complete(raw)))
                        .collect(),
                    None => Vec::new(),
                };
                let written = packs
                    .into_iter()
                    .map(|p| write_pack(&root, p))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((written, texts))
            })
            .await?
        };
        let written: Vec<WrittenPack> = written;
        let files: Vec<String> = written.iter().map(|p| p.file.clone()).collect();
        let recorded = self
            .write(move |tx| {
                let mut infos = Vec::new();
                let mut unreferenced = Vec::new();
                for pack in &written {
                    for (message, placement) in &pack.entries {
                        let recorded =
                            bodies::record_complete(tx, *message, &pack.file, *placement, now)?;
                        infos.extend(recorded.info);
                        unreferenced.extend(recorded.unreferenced);
                    }
                }
                let stored: HashSet<MessageId> =
                    infos.iter().map(|i: &BodyInfo| i.message).collect();
                let mut indexed: BTreeMap<i64, BodyText> = BTreeMap::new();
                for (message, text) in texts {
                    if stored.contains(&message) {
                        indexed.insert(message.0, text);
                    }
                }
                let indexed: Vec<(i64, BodyText)> = indexed.into_iter().collect();
                fts::index_bodies(tx, &indexed)?;
                // A pack whose messages all vanished meanwhile.
                unreferenced.extend(bodies::unreferenced(
                    tx,
                    written.iter().map(|p| p.file.clone()),
                )?);
                Ok((infos, unreferenced))
            })
            .await;
        let (infos, remove) = match recorded {
            Ok((infos, unreferenced)) => (Ok(infos), unreferenced),
            Err(err) => (Err(err), files),
        };
        self.remove_body_files(remove).await;
        let mut infos = infos?;
        // Input order (a message given twice: the last copy won).
        infos.dedup_by_key(|i| i.message);
        Ok(infos)
    }

    /// Adds sections of a large message (from `MailBackend::fetch_sections`) to its partial
    /// body; sections stored before are replaced. All sections of one call go into one file.
    /// If a complete body is stored already, it stays and the sections are discarded. Empty
    /// `sections` store just the structure. [`StoreError::NotFound`] if the message does not
    /// exist (any more).
    pub async fn store_body_sections(
        &self,
        message: MessageId,
        structure: MessageStructure,
        sections: Vec<FetchedSection>,
    ) -> Result<BodyInfo, StoreError> {
        let account = self
            .read(move |c| bodies::account_of(c, message))
            .await?
            .ok_or_else(|| StoreError::NotFound(format!("message {message}")))?;
        let now = now_millis();
        let file = body_files::new_file_name(&account, message, now);
        let root = self.bodies_dir().to_owned();
        let source = self.body_text_source();
        let text_structure = structure.clone();
        let (file, parts, text) = {
            let root = root.clone();
            blocking(move || {
                // Stage 7: index text from the sections of this call (the text parts).
                let text = source.map(|s| s.partial(&text_structure, &sections));
                let names: Vec<BodySection> = sections.iter().map(|s| s.section.clone()).collect();
                let pieces: Vec<Vec<u8>> = sections.into_iter().map(|s| s.data).collect();
                let written = write_file(&root, file, &pieces)?;
                let parts = names
                    .into_iter()
                    .zip(written.placements)
                    .map(|(section, placement)| NewPart { section, placement })
                    .collect::<Vec<_>>();
                Ok((written.file, parts, text))
            })
            .await?
        };
        let recorded = {
            let file = file.clone();
            self.write(move |tx| {
                let first = bodies::info(tx, message)?.is_none();
                let recorded =
                    bodies::record_sections(tx, message, &structure, &file, &parts, now)?;
                // Sections without text (an attachment fetched on request) keep the indexed
                // text of the earlier call; the first call always indexes (attachment names).
                if recorded.info.as_ref().is_some_and(|i| !i.complete)
                    && let Some(text) = text
                    && (first || !text.body.trim().is_empty())
                {
                    fts::index_bodies(tx, &[(message.0, text)])?;
                }
                Ok(recorded)
            })
            .await
        };
        let (info, remove) = match recorded {
            Ok(recorded) => (Ok(recorded.info), recorded.unreferenced),
            Err(err) => (Err(err), vec![file]),
        };
        self.remove_body_files(remove).await;
        info?.ok_or_else(|| StoreError::NotFound(format!("message {message}")))
    }

    /// What is stored of the body of `message`; `None` if nothing (or the message is unknown).
    pub async fn body_info(&self, message: MessageId) -> Result<Option<BodyInfo>, StoreError> {
        self.read(move |c| bodies::info(c, message)).await
    }

    /// The whole raw message, if a complete body is stored; `None` otherwise (nothing stored,
    /// only sections, or the file was missing or damaged; then its reference is forgotten).
    pub async fn load_message(&self, message: MessageId) -> Result<Option<Vec<u8>>, StoreError> {
        let parts = self.read(move |c| bodies::parts(c, message)).await?;
        let Some(whole) = parts.into_iter().find(|p| p.section.is_none()) else {
            return Ok(None);
        };
        let mut loaded = self.load_files(message, vec![whole.file.clone()]).await?;
        let Some(data) = loaded.remove(&whole.file) else {
            return Ok(None);
        };
        match data.get(whole.offset..whole.offset.saturating_add(whole.length)) {
            Some(bytes) if bytes.len() == data.len() => Ok(Some(data)),
            Some(bytes) => Ok(Some(bytes.to_vec())),
            None => {
                self.forget_damaged(message, whole.file, "shorter than recorded".into())
                    .await?;
                Ok(None)
            }
        }
    }

    /// Stored sections of a partial body: `wanted`, or all with `None`. Sections not stored are
    /// missing from the result (a complete body has none; parse the whole message instead).
    /// Damaged files are forgotten like in [`Store::load_message`].
    pub async fn load_sections(
        &self,
        message: MessageId,
        wanted: Option<Vec<BodySection>>,
    ) -> Result<Vec<FetchedSection>, StoreError> {
        let parts = self.read(move |c| bodies::parts(c, message)).await?;
        let parts: Vec<_> = parts
            .into_iter()
            .filter_map(|p| {
                let section = p.section.clone()?;
                let keep = wanted.as_ref().is_none_or(|w| w.contains(&section));
                keep.then_some((section, p))
            })
            .collect();
        let files: Vec<String> = parts
            .iter()
            .map(|(_, p)| p.file.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let loaded = self.load_files(message, files).await?;
        let mut out = Vec::with_capacity(parts.len());
        let mut short = Vec::new();
        for (section, part) in parts {
            let Some(data) = loaded.get(&part.file) else {
                continue;
            };
            match data.get(part.offset..part.offset.saturating_add(part.length)) {
                Some(bytes) => out.push(FetchedSection {
                    section,
                    data: bytes.to_vec(),
                }),
                None => short.push(part.file),
            }
        }
        short.sort();
        short.dedup();
        for file in short {
            self.forget_damaged(message, file, "shorter than recorded".into())
                .await?;
        }
        Ok(out)
    }

    /// Reads and decompresses `files` of `message`. Missing or damaged files are left out of the
    /// result and forgotten.
    async fn load_files(
        &self,
        message: MessageId,
        files: Vec<String>,
    ) -> Result<BTreeMap<String, Vec<u8>>, StoreError> {
        let root = self.bodies_dir().to_owned();
        let results = blocking(move || {
            Ok(files
                .into_iter()
                .map(|file| {
                    let data = body_files::read(&root, &file)
                        .map_err(|err| err.to_string())
                        .and_then(|bytes| body_files::decompress(&bytes));
                    (file, data)
                })
                .collect::<Vec<_>>())
        })
        .await?;
        let mut loaded = BTreeMap::new();
        for (file, data) in results {
            match data {
                Ok(data) => {
                    loaded.insert(file, data);
                }
                Err(reason) => self.forget_damaged(message, file, reason).await?,
            }
        }
        Ok(loaded)
    }

    async fn forget_damaged(
        &self,
        message: MessageId,
        file: String,
        reason: String,
    ) -> Result<(), StoreError> {
        let (forgotten, removable) = {
            let file = file.clone();
            self.write(move |tx| bodies::forget_file(tx, message, &file))
                .await?
        };
        if forgotten {
            tracing::warn!(message = message.0, %file, %reason, "body file missing or damaged; it will be fetched again");
        }
        if removable {
            self.remove_body_files(vec![file]).await;
        }
        Ok(())
    }

    /// Deletes the stored body of `message`. Returns `false` if there was none.
    pub async fn delete_body(&self, message: MessageId) -> Result<bool, StoreError> {
        let (deleted, files) = self.write(move |tx| bodies::delete(tx, message)).await?;
        self.remove_body_files(files).await;
        Ok(deleted)
    }

    /// Removes body files and account directories nothing in the database refers to, last
    /// modified more than `min_age` ago (crash leftovers, failed removals). The engine runs it
    /// at start with a few minutes, so files of writes in flight are never touched.
    pub async fn sweep_body_files(&self, min_age: Duration) -> Result<SweepReport, StoreError> {
        let (accounts, referenced) = self
            .read(|c| {
                let accounts: HashSet<String> = crate::accounts::all(c)?
                    .into_iter()
                    .map(|a| a.id.to_string())
                    .collect();
                let referenced: HashSet<String> = bodies::all_files(c)?.into_iter().collect();
                Ok((accounts, referenced))
            })
            .await?;
        let root = self.bodies_dir().to_owned();
        let report = blocking(move || {
            body_files::sweep(&root, &accounts, &referenced, min_age)
                .map_err(StoreError::io("sweep body files"))
        })
        .await?;
        if report != SweepReport::default() {
            tracing::info!(?report, "removed unreferenced body files");
        }
        Ok(report)
    }

    /// Removes the body directory of a deleted account; failures are logged and left to the
    /// sweep (the account itself is gone either way).
    pub(crate) async fn remove_account_bodies(&self, account: AccountId) {
        let root = self.bodies_dir().to_owned();
        let uuid = account.to_string();
        let result = blocking(move || {
            body_files::remove_account_dir(&root, &uuid)
                .map_err(StoreError::io("remove account body directory"))
        })
        .await;
        if let Err(err) = result {
            tracing::error!(account = %account, error = %err, "body files of a removed account stay until the next sweep");
        }
    }

    /// Removes body files of deleted messages after their rows are gone.
    pub(crate) async fn remove_body_files(&self, files: Vec<String>) {
        if files.is_empty() {
            return;
        }
        let root = self.bodies_dir().to_owned();
        let result = blocking(move || {
            body_files::remove_files(&root, &files);
            Ok(())
        })
        .await;
        if let Err(err) = result {
            tracing::error!(error = %err, "body files of deleted messages stay until the next sweep");
        }
    }

    // ---------------------------------------------------------------------------------------
    // Reading pane
    // ---------------------------------------------------------------------------------------

    /// Header data of a message as stored with the list (shown while the body is missing);
    /// `None` if the message does not exist.
    pub async fn message_summary(
        &self,
        message: MessageId,
    ) -> Result<Option<StoredMessage>, StoreError> {
        self.read(move |c| reading::summary(c, message)).await
    }

    /// Every mailbox the message lies in, with UID and UIDVALIDITY, by mailbox id.
    pub async fn message_locations(
        &self,
        message: MessageId,
    ) -> Result<Vec<MessageLocation>, StoreError> {
        self.read(move |c| reading::locations(c, message)).await
    }

    /// Stores list previews made by the parser: white space collapsed, cut to
    /// [`crate::MAX_PREVIEW_CHARS`] characters, empty text clears. Unknown messages are skipped;
    /// returns how many were written.
    pub async fn set_previews(
        &self,
        previews: Vec<(MessageId, String)>,
    ) -> Result<usize, StoreError> {
        self.write(move |tx| reading::set_previews(tx, &previews))
            .await
    }

    // ---------------------------------------------------------------------------------------
    // Prefetch
    // ---------------------------------------------------------------------------------------

    /// Messages of the account to prefetch, newest first, at most `limit`
    /// (1..=[`crate::MAX_PREFETCH_BATCH`]): every message in Inbox, Sent and Drafts and every
    /// flagged message elsewhere whose sort date is at least `since` (Unix seconds, `None` for
    /// all) and whose id is below `before` (to continue past messages that failed), without a
    /// stored body (complete or partial). One location per message. Per mailbox a keyset range
    /// on the location key; a test checks the plans.
    pub async fn prefetch_candidates(
        &self,
        account: AccountId,
        since: Option<i64>,
        before: Option<MessageId>,
        limit: u32,
    ) -> Result<Vec<PrefetchCandidate>, StoreError> {
        self.read(move |c| reading::prefetch_candidates(c, account, since, before, limit))
            .await
    }

    /// Prefetch settings; defaults (30 days, 5 MiB) for values never set or unreadable.
    pub async fn prefetch_settings(&self) -> Result<PrefetchSettings, StoreError> {
        self.read(reading::prefetch_settings).await
    }

    /// Stores the settings ([`StoreError::InvalidArgument`] if the size limit is too large) and
    /// returns them as stored.
    pub async fn set_prefetch_settings(
        &self,
        settings: PrefetchSettings,
    ) -> Result<PrefetchSettings, StoreError> {
        let settings = settings
            .validated()
            .map_err(|err| StoreError::InvalidArgument(err.to_string()))?;
        self.put_settings(GLOBAL_SCOPE, reading::prefetch_entries(settings)?)
            .await?;
        self.prefetch_settings().await
    }

    // ---------------------------------------------------------------------------------------
    // Remote content allowlist
    // ---------------------------------------------------------------------------------------

    /// "Always load remote content from this sender" for the account. The address is
    /// normalised with [`mail_model::normalize_sender`] ([`StoreError::InvalidArgument`] if it
    /// is not one address). Returns `false` if it was allowed already.
    pub async fn allow_remote_content(
        &self,
        account: AccountId,
        sender: &str,
    ) -> Result<bool, StoreError> {
        let sender = sender_key(sender)?;
        self.write(move |tx| reading::allow_sender(tx, account, &sender))
            .await
    }

    /// Takes the permission back. Returns `false` if there was none.
    pub async fn revoke_remote_content(
        &self,
        account: AccountId,
        sender: &str,
    ) -> Result<bool, StoreError> {
        let sender = sender_key(sender)?;
        self.write(move |tx| reading::revoke_sender(tx, account, &sender))
            .await
    }

    /// Whether remote content of `sender` may load without asking. An address that is not one
    /// address is never allowed.
    pub async fn remote_content_allowed(
        &self,
        account: AccountId,
        sender: &str,
    ) -> Result<bool, StoreError> {
        let Some(sender) = normalize_sender(sender) else {
            return Ok(false);
        };
        self.read(move |c| reading::sender_allowed(c, account, &sender))
            .await
    }

    /// Allowed senders of the account, sorted (for the settings).
    pub async fn remote_content_senders(
        &self,
        account: AccountId,
    ) -> Result<Vec<String>, StoreError> {
        self.read(move |c| reading::senders(c, account)).await
    }
}

/// Raw bytes up to which [`Store::store_bodies`] packs complete bodies into one file. Reading
/// one body decompresses its whole file: 1 MiB takes well below a millisecond.
pub const BODY_PACK_LIMIT: usize = 1024 * 1024;

/// Bodies of one account to be written into one file.
struct Pack {
    file: String,
    entries: Vec<(MessageId, Vec<u8>)>,
}

struct WrittenPack {
    file: String,
    entries: Vec<(MessageId, Placement)>,
}

/// Groups bodies into files: per account, consecutive bodies while the raw total stays within
/// [`BODY_PACK_LIMIT`]; a larger body alone. Bodies of unknown messages are dropped.
fn pack(
    bodies: Vec<(MessageId, Vec<u8>)>,
    accounts: Vec<Option<String>>,
    now_ms: i64,
) -> Vec<Pack> {
    let mut packs: Vec<Pack> = Vec::new();
    let mut open: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new(); // account -> (pack index, raw bytes)
    for ((message, data), account) in bodies.into_iter().zip(accounts) {
        let Some(account) = account else {
            continue;
        };
        let len = data.len();
        if len <= BODY_PACK_LIMIT
            && let Some((index, total)) = open.get_mut(&account)
            && *total + len <= BODY_PACK_LIMIT
            && let Some(pack) = packs.get_mut(*index)
        {
            pack.entries.push((message, data));
            *total += len;
            continue;
        }
        let file = body_files::new_file_name(&account, message, now_ms);
        packs.push(Pack {
            file,
            entries: vec![(message, data)],
        });
        if len <= BODY_PACK_LIMIT {
            open.insert(account, (packs.len() - 1, len));
        }
    }
    packs
}

struct WrittenFile {
    file: String,
    placements: Vec<Placement>,
}

/// Concatenates `pieces`, compresses and writes them atomically to `file`; each piece gets its
/// place and its share of the compressed size.
fn write_file(
    root: &std::path::Path,
    file: String,
    pieces: &[Vec<u8>],
) -> Result<WrittenFile, StoreError> {
    let total: usize = pieces.iter().map(Vec::len).sum();
    let mut data = Vec::with_capacity(total);
    let mut placements = Vec::with_capacity(pieces.len());
    for piece in pieces {
        placements.push(Placement {
            offset: data.len(),
            length: piece.len(),
            stored_bytes: 0,
        });
        data.extend_from_slice(piece);
    }
    let compressed =
        body_files::compress(&data).map_err(StoreError::io("compress message body"))?;
    let size = body_files::write_atomically(root, &file, &compressed)
        .map_err(StoreError::io("write message body"))?;
    let count = u64::try_from(placements.len().max(1)).unwrap_or(1);
    let total = u64::try_from(total).unwrap_or(u64::MAX);
    for placement in &mut placements {
        let length = u64::try_from(placement.length).unwrap_or(0);
        placement.stored_bytes = size
            .saturating_mul(length)
            .checked_div(total)
            .unwrap_or(size / count);
    }
    // Rounding remainder to the last piece, so the shares add up to the file size.
    let shared: u64 = placements.iter().map(|p| p.stored_bytes).sum();
    if let Some(last) = placements.last_mut() {
        last.stored_bytes += size.saturating_sub(shared);
    }
    Ok(WrittenFile { file, placements })
}

fn write_pack(root: &std::path::Path, pack: Pack) -> Result<WrittenPack, StoreError> {
    let (messages, pieces): (Vec<MessageId>, Vec<Vec<u8>>) = pack.entries.into_iter().unzip();
    let written = write_file(root, pack.file, &pieces)?;
    Ok(WrittenPack {
        file: written.file,
        entries: messages.into_iter().zip(written.placements).collect(),
    })
}

fn sender_key(sender: &str) -> Result<String, StoreError> {
    normalize_sender(sender)
        .ok_or_else(|| StoreError::InvalidArgument(format!("sender address {sender:?}")))
}

/// How long [`Store::sweep_body_files`] leaves young files alone by default.
pub const SWEEP_MIN_AGE: Duration = Duration::from_secs(10 * 60);
