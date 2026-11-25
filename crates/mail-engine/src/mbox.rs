//! Stage 19, block C (decision 0057): a folder exported as an mbox file, an mbox file imported
//! into a folder, as background jobs with progress (`mboxJobChanged`) and cancel.
//!
//! **Export** (mboxrd, `mail_content::write_mbox_message`) writes every message of the folder
//! as the server has it (the stored body when complete, else fetched whole over the reading
//! connection, not stored) to the file the user chose in the save dialog. Messages no longer on
//! the server are skipped and counted. Cancelled or failed, the partial file is removed.
//!
//! **Import** needs writes on the target's account (checked before the file is read). The file
//! is read line by line on the blocking pool (`mail_content::MboxReader`, bounded memory: one
//! message at a time, at most [`mail_content::MAX_MBOX_MESSAGE_BYTES`]); every message becomes
//! one `append` of the outbox with frozen bytes, its flags (Thunderbird's `X-Mozilla-Status`,
//! else `Status`/`X-Status`, else read) and its Message-ID. A message without one gets
//! `Message-ID: <random@mbox-import.invalid>` in front, so the outbox can tell whether an append
//! arrived before a crash. Messages Thunderbird marked as deleted are skipped. At most
//! [`MBOX_IMPORT_WINDOW`] appends wait at once (and their frozen copies on disk): reading goes
//! on as the server takes them. Cancel stops reading and takes the waiting appends out of the
//! outbox; what the server took stays.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use mail_content::{MboxEntry, MboxReader, MozillaStatus};
use mail_model::{
    AccountId, CoreError, CoreEvent, DraftId, Flags, IpcError, MAX_MBOX_JOB_MESSAGES,
    MBOX_IMPORT_WINDOW, MailboxId, MboxJob, MboxJobId, MboxJobKind, MboxJobState, OperationId,
    OperationState,
};
use mail_store::{ImportItem, Store};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::AppCore;
use crate::compose::files;
use crate::core::{Connector, WeakCore};

/// Messages read per page of the folder (export).
const EXPORT_PAGE: u32 = 500;

/// Progress events at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// How often a waiting import looks at its appends.
const IMPORT_POLL: Duration = Duration::from_millis(200);

/// Finished jobs kept for the UI.
const MAX_KEPT_JOBS: usize = 16;

struct Job {
    info: Mutex<MboxJob>,
    cancel: AtomicBool,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Job {
    fn snapshot(&self) -> MboxJob {
        self.info
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, f: impl FnOnce(&mut MboxJob)) -> MboxJob {
        let mut info = self.info.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut info);
        info.clone()
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// The jobs of this session. Tasks are aborted when the core goes.
#[derive(Default)]
pub(crate) struct MboxJobs {
    jobs: Mutex<Vec<Arc<Job>>>,
}

impl Drop for MboxJobs {
    fn drop(&mut self) {
        for job in self
            .jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            if let Some(task) = job
                .task
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                task.abort();
            }
        }
    }
}

impl MboxJobs {
    fn add(&self, job: Arc<Job>) {
        let mut jobs = self.jobs.lock().unwrap_or_else(PoisonError::into_inner);
        jobs.push(job);
        while jobs.len() > MAX_KEPT_JOBS {
            let Some(index) = jobs
                .iter()
                .position(|j| j.snapshot().state != MboxJobState::Running)
            else {
                break;
            };
            jobs.remove(index);
        }
    }

    fn find(&self, id: &MboxJobId) -> Option<Arc<Job>> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|j| j.snapshot().id == *id)
            .cloned()
    }

    fn all(&self) -> Vec<MboxJob> {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|j| j.snapshot())
            .collect()
    }

    fn running_on(&self, mailbox: MailboxId) -> bool {
        self.jobs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|j| {
                let info = j.snapshot();
                info.mailbox_id == mailbox && info.state == MboxJobState::Running
            })
    }
}

fn random_u128() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Flags of an imported message: Thunderbird's status, else `Status`/`X-Status` (mutt, procmail),
/// else read (an archive should not fill the unread count).
fn flags_of(mozilla: Option<MozillaStatus>, header: &[mail_content::HeaderField]) -> Flags {
    if let Some(status) = mozilla {
        return Flags {
            seen: status.read,
            answered: status.replied,
            flagged: status.flagged,
            ..Flags::default()
        };
    }
    let value = |name: &str| {
        header
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
            .map(|f| f.value.clone())
    };
    let status = value("Status");
    let x_status = value("X-Status");
    if status.is_none() && x_status.is_none() {
        return Flags {
            seen: true,
            ..Flags::default()
        };
    }
    let x = x_status.unwrap_or_default();
    Flags {
        seen: status.unwrap_or_default().contains('R'),
        answered: x.contains('A'),
        flagged: x.contains('F'),
        ..Flags::default()
    }
}

/// The Message-ID of a header, if usable for finding the append again.
fn message_id_of(header: &[mail_content::HeaderField]) -> Option<String> {
    let value = header
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case("Message-ID"))?
        .value
        .trim()
        .to_owned();
    let id = value.strip_prefix('<')?.strip_suffix('>')?.trim();
    (id.len() <= 250
        && id.contains('@')
        && id
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>' | '"' | '\\')))
    .then(|| id.to_owned())
}

/// One line with its line ending, at most `cap` bytes; `Ok(false)` at the end of the file.
fn read_line_capped(
    reader: &mut impl BufRead,
    line: &mut Vec<u8>,
    cap: usize,
) -> Result<bool, CoreError> {
    line.clear();
    loop {
        let available = reader.fill_buf().map_err(|err| {
            CoreError::InvalidArgument(format!("the file could not be read: {err}"))
        })?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let (take, done) = match available.iter().position(|b| *b == b'\n') {
            Some(end) => (end + 1, true),
            None => (available.len(), false),
        };
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if line.len() > cap {
            return Err(CoreError::InvalidArgument(
                "the file has a line longer than a message may be: not an mbox file".into(),
            ));
        }
        if done {
            return Ok(true);
        }
    }
}

/// A message read from the file, with what the import needs.
struct Read {
    bytes: Vec<u8>,
    flags: Flags,
    message_id: Option<String>,
}

enum FromFile {
    Message(Read),
    Skipped,
    Failed(CoreError),
}

/// Reads the file on the blocking pool and hands messages over one at a time.
fn read_file(path: PathBuf, cancel: Arc<AtomicBool>, sender: mpsc::Sender<FromFile>) {
    let file = match std::fs::File::open(&path) {
        Ok(file) if file.metadata().is_ok_and(|m| m.is_file()) => file,
        Ok(_) => {
            let _ = sender.blocking_send(FromFile::Failed(CoreError::InvalidArgument(
                "not a file".into(),
            )));
            return;
        }
        Err(err) => {
            let _ = sender.blocking_send(FromFile::Failed(CoreError::InvalidArgument(format!(
                "the file could not be read: {err}"
            ))));
            return;
        }
    };
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut mbox = MboxReader::new();
    let mut line = Vec::new();
    let mut count = 0u32;
    let hand_over = |entry: MboxEntry, count: &mut u32| -> bool {
        *count += 1;
        if *count > MAX_MBOX_JOB_MESSAGES {
            let _ = sender.blocking_send(FromFile::Failed(CoreError::InvalidArgument(format!(
                "an import takes at most {MAX_MBOX_JOB_MESSAGES} messages"
            ))));
            return false;
        }
        let item = match entry {
            MboxEntry::TooLarge { .. } => FromFile::Skipped,
            MboxEntry::Message { mozilla, .. } if mozilla.is_some_and(|m| m.deleted) => {
                FromFile::Skipped
            }
            MboxEntry::Message { bytes, mozilla } => {
                let header = mail_content::header_fields(&bytes, 200, 1000);
                FromFile::Message(Read {
                    flags: flags_of(mozilla, &header),
                    message_id: message_id_of(&header),
                    bytes,
                })
            }
        };
        sender.blocking_send(item).is_ok()
    };
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        match read_line_capped(&mut reader, &mut line, mail_content::MAX_MBOX_MESSAGE_BYTES) {
            Ok(true) => {
                if let Some(entry) = mbox.push_line(&line)
                    && !hand_over(entry, &mut count)
                {
                    return;
                }
            }
            Ok(false) => break,
            Err(err) => {
                let _ = sender.blocking_send(FromFile::Failed(err));
                return;
            }
        }
    }
    if let Some(entry) = mbox.finish() {
        hand_over(entry, &mut count);
    }
    if count == 0 && mbox.lines_before_first() > 0 {
        let _ = sender.blocking_send(FromFile::Failed(CoreError::InvalidArgument(
            "the file is not an mbox file".into(),
        )));
    }
}

impl<C: Connector> AppCore<C> {
    fn mbox_emit(&self, job: MboxJob) {
        self.emit_event(CoreEvent::MboxJobChanged { job });
    }

    async fn mbox_target(&self, mailbox: MailboxId) -> Result<(AccountId, String), CoreError> {
        let store = self.store_ref()?;
        let account: AccountId = store
            .mailbox_account(mailbox)
            .await?
            .and_then(|uuid| uuid.parse().ok())
            .ok_or_else(|| CoreError::NotFound(format!("mailbox {mailbox}")))?;
        let name = store
            .mailboxes(account)
            .await?
            .into_iter()
            .find(|m| m.id == mailbox)
            .map(|m| m.path)
            .ok_or_else(|| CoreError::NotFound(format!("mailbox {mailbox}")))?;
        Ok((account, name))
    }

    fn new_job(
        &self,
        kind: MboxJobKind,
        account: AccountId,
        mailbox: MailboxId,
        name: String,
        path: &Path,
        total: Option<u32>,
    ) -> Result<Arc<Job>, CoreError> {
        if self.mbox_jobs().running_on(mailbox) {
            return Err(CoreError::InvalidArgument(
                "an export or import of this folder is running already".into(),
            ));
        }
        let job = Arc::new(Job {
            info: Mutex::new(MboxJob {
                id: MboxJobId::from_random(random_u128()),
                kind,
                account_id: account,
                mailbox_id: mailbox,
                mailbox_name: name,
                file_name: file_name(path),
                state: MboxJobState::Running,
                total,
                done: 0,
                queued: 0,
                skipped: 0,
                error: None,
            }),
            cancel: AtomicBool::new(false),
            task: Mutex::new(None),
        });
        self.mbox_jobs().add(Arc::clone(&job));
        Ok(job)
    }

    /// The file name the save dialog suggests for a folder's export.
    pub async fn mbox_export_name(&self, mailbox: MailboxId) -> Result<String, CoreError> {
        let (_, path) = self.mbox_target(mailbox).await?;
        let base = path.rsplit(['/', '.']).next().unwrap_or("mail");
        let base: String = base.chars().take(120).collect();
        Ok(mail_content::sanitize_filename(&format!("{base}.mbox")))
    }

    /// Stage 19: exports the folder's messages to `path` (from the save dialog) in the
    /// background; `mboxJobChanged` reports the progress.
    pub async fn start_mbox_export(
        &self,
        mailbox: MailboxId,
        path: PathBuf,
    ) -> Result<MboxJob, CoreError> {
        let (account, name) = self.mbox_target(mailbox).await?;
        let total = self
            .store_ref()?
            .mailboxes(account)
            .await?
            .into_iter()
            .find(|m| m.id == mailbox)
            .map(|m| m.total);
        let job = self.new_job(MboxJobKind::Export, account, mailbox, name, &path, total)?;
        let weak = self.downgrade();
        let running = Arc::clone(&job);
        let task = tokio::spawn(async move {
            let outcome = export(&weak, &running, mailbox, &path).await;
            finish(&weak, &running, outcome, Some(&path));
        });
        *job.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
        let snapshot = job.snapshot();
        tracing::info!(job = snapshot.id.as_str(), "mbox export started");
        self.mbox_emit(snapshot.clone());
        Ok(snapshot)
    }

    /// Stage 19: imports the messages of the mbox file at `path` (from the open dialog) into
    /// the folder through the outbox, in the background. `read_only` without writes on the
    /// folder's account, before anything is read.
    pub async fn start_mbox_import(
        &self,
        mailbox: MailboxId,
        path: PathBuf,
    ) -> Result<MboxJob, CoreError> {
        let store = self.store_ref()?;
        store.check_import_target(mailbox).await?;
        self.outbox_ref()?;
        let (account, name) = self.mbox_target(mailbox).await?;
        let job = self.new_job(MboxJobKind::Import, account, mailbox, name, &path, None)?;
        let weak = self.downgrade();
        let running = Arc::clone(&job);
        let task = tokio::spawn(async move {
            let outcome = import(&weak, &running, mailbox, path).await;
            finish(&weak, &running, outcome, None);
        });
        *job.task.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
        let snapshot = job.snapshot();
        tracing::info!(job = snapshot.id.as_str(), "mbox import started");
        self.mbox_emit(snapshot.clone());
        Ok(snapshot)
    }

    /// Stage 19: whether the folder can take an import (`read_only`, `invalid_target`), asked
    /// before the open dialog.
    pub async fn check_mbox_import(&self, mailbox: MailboxId) -> Result<(), CoreError> {
        Ok(self.store_ref()?.check_import_target(mailbox).await?)
    }

    /// Stage 19: stops a running job (an import also takes its waiting appends out of the
    /// outbox). The job reports `cancelled` once it stopped. `not_found` for an unknown job.
    pub fn cancel_mbox_job(&self, id: MboxJobId) -> Result<MboxJob, CoreError> {
        let job = self
            .mbox_jobs()
            .find(&id)
            .ok_or_else(|| CoreError::NotFound(format!("mbox job {}", id.as_str())))?;
        job.cancel.store(true, Ordering::SeqCst);
        Ok(job.snapshot())
    }

    /// Stage 19: the jobs of this session, running and finished (the newest 16).
    pub fn list_mbox_jobs(&self) -> Vec<MboxJob> {
        self.mbox_jobs().all()
    }
}

/// Ends a job: its state from the outcome; a partial export file is removed.
fn finish<C: Connector>(
    weak: &WeakCore<C>,
    job: &Job,
    outcome: Result<(), CoreError>,
    partial: Option<&Path>,
) {
    let cancelled = job.cancelled();
    if (cancelled || outcome.is_err())
        && let Some(path) = partial
    {
        let _ = std::fs::remove_file(path);
    }
    let snapshot = job.update(|info| {
        info.state = match &outcome {
            Ok(()) if cancelled => MboxJobState::Cancelled,
            Ok(()) => MboxJobState::Done,
            Err(_) if cancelled => MboxJobState::Cancelled,
            Err(err) => {
                info.error = Some(IpcError::from(err.clone()));
                MboxJobState::Failed
            }
        };
    });
    tracing::info!(
        job = snapshot.id.as_str(),
        state = ?snapshot.state,
        done = snapshot.done,
        skipped = snapshot.skipped,
        "mbox job ended"
    );
    if let Some(core) = weak.upgrade() {
        core.mbox_emit(snapshot);
    }
}

/// Emits the job's state when the last report is older than [`PROGRESS_EVERY`].
struct Progress(Instant);

impl Progress {
    fn tick<C: Connector>(&mut self, weak: &WeakCore<C>, job: &Job) {
        if self.0.elapsed() >= PROGRESS_EVERY {
            self.0 = Instant::now();
            if let Some(core) = weak.upgrade() {
                core.mbox_emit(job.snapshot());
            }
        }
    }
}

fn gone() -> CoreError {
    CoreError::Internal("the app is closing".into())
}

async fn export<C: Connector>(
    weak: &WeakCore<C>,
    job: &Job,
    mailbox: MailboxId,
    path: &Path,
) -> Result<(), CoreError> {
    let file = {
        let path = path.to_owned();
        tokio::task::spawn_blocking(move || std::fs::File::create(path))
            .await
            .map_err(|err| CoreError::Internal(format!("mbox task failed: {err}")))?
            .map_err(|err| {
                CoreError::InvalidArgument(format!("the file could not be written: {err}"))
            })?
    };
    let mut out = Some(std::io::BufWriter::new(file));
    let mut cursor = None;
    let mut progress = Progress(Instant::now());
    loop {
        if job.cancelled() {
            return Ok(());
        }
        let core = weak.upgrade().ok_or_else(gone)?;
        let page = core
            .store_ref()?
            .list_messages(mailbox, cursor, EXPORT_PAGE)
            .await?;
        let reading = Arc::clone(core.reading_ref()?);
        drop(core);
        for item in &page.items {
            if job.cancelled() {
                return Ok(());
            }
            let raw = match reading.raw_message(item.id).await {
                Ok(raw) => raw,
                Err(CoreError::NotFound(_)) => {
                    job.update(|info| info.skipped += 1);
                    continue;
                }
                Err(err) => return Err(err),
            };
            let sender = item.from.as_ref().map(|a| a.email.clone());
            let date = item.date / 1000;
            let mut writer = out
                .take()
                .ok_or_else(|| CoreError::Internal("no file".into()))?;
            writer = tokio::task::spawn_blocking(move || {
                let mut buffer = Vec::with_capacity(raw.len() + 128);
                mail_content::write_mbox_message(&mut buffer, sender.as_deref(), date, &raw);
                writer.write_all(&buffer).map(|()| writer)
            })
            .await
            .map_err(|err| CoreError::Internal(format!("mbox task failed: {err}")))?
            .map_err(|err| {
                CoreError::InvalidArgument(format!("the file could not be written: {err}"))
            })?;
            out = Some(writer);
            job.update(|info| info.done += 1);
            progress.tick(weak, job);
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    if let Some(mut writer) = out {
        tokio::task::spawn_blocking(move || {
            writer.flush().and_then(|()| writer.get_ref().sync_all())
        })
        .await
        .map_err(|err| CoreError::Internal(format!("mbox task failed: {err}")))?
        .map_err(|err| {
            CoreError::InvalidArgument(format!("the file could not be written: {err}"))
        })?;
    }
    Ok(())
}

/// Counts what of `pending` the server took or refused; returns those still waiting.
async fn settle(
    store: &Store,
    job: &Job,
    pending: Vec<OperationId>,
) -> Result<Vec<OperationId>, CoreError> {
    let states = store.operation_states(pending).await?;
    let mut waiting = Vec::new();
    let (mut done, mut skipped) = (0, 0);
    for (id, state) in states {
        match state {
            // Confirmed appends leave the outbox after a while (`linger`): gone is done.
            Some(OperationState::Confirmed) | None => done += 1,
            Some(OperationState::Failed | OperationState::Conflict) => skipped += 1,
            Some(OperationState::Queued | OperationState::InFlight) => waiting.push(id),
        }
    }
    let queued = u32::try_from(waiting.len()).unwrap_or(u32::MAX);
    job.update(|info| {
        info.done += done;
        info.skipped += skipped;
        info.queued = queued;
    });
    Ok(waiting)
}

async fn import<C: Connector>(
    weak: &WeakCore<C>,
    job: &Job,
    mailbox: MailboxId,
    path: PathBuf,
) -> Result<(), CoreError> {
    let (sender, mut receiver) = mpsc::channel::<FromFile>(2);
    let stop = Arc::new(AtomicBool::new(false));
    let reader = {
        let stop = Arc::clone(&stop);
        tokio::task::spawn_blocking(move || read_file(path, stop, sender))
    };
    let mut pending: Vec<OperationId> = Vec::new();
    let outcome = import_loop(weak, job, mailbox, &mut receiver, &mut pending).await;
    stop.store(true, Ordering::SeqCst);
    drop(receiver);
    let _ = reader.await;
    if job.cancelled()
        && !pending.is_empty()
        && let Some(core) = weak.upgrade()
    {
        let store = core.store_ref()?;
        if let Some(change) = store.cancel_import_appends(pending.clone()).await? {
            pending.retain(|id| !change.removed.contains(id));
            if let Ok(outbox) = core.outbox_ref() {
                outbox.changed(&change);
            }
        }
        // What was sent meanwhile counts; what was taken out is not imported.
        settle(store, job, pending).await?;
        job.update(|info| info.queued = 0);
    }
    outcome
}

/// Reads messages from the reader task, freezes and queues them, keeps the window, waits for
/// the rest at the end. `pending` holds the appends not settled yet (for cancelling).
async fn import_loop<C: Connector>(
    weak: &WeakCore<C>,
    job: &Job,
    mailbox: MailboxId,
    receiver: &mut mpsc::Receiver<FromFile>,
    pending: &mut Vec<OperationId>,
) -> Result<(), CoreError> {
    let request_base = format!("mbox-{}", job.snapshot().id.as_str());
    let mut progress = Progress(Instant::now());
    let mut batch = 0u64;
    let window = usize::try_from(MBOX_IMPORT_WINDOW).unwrap_or(usize::MAX);
    {
        loop {
            if job.cancelled() {
                return Ok(());
            }
            // Wait while the window is full.
            while pending.len() >= window {
                if job.cancelled() {
                    return Ok(());
                }
                let core = weak.upgrade().ok_or_else(gone)?;
                *pending = settle(core.store_ref()?, job, std::mem::take(pending)).await?;
                drop(core);
                progress.tick(weak, job);
                if pending.len() >= window {
                    tokio::time::sleep(IMPORT_POLL).await;
                }
            }
            let Some(next) = receiver.recv().await else {
                break;
            };
            let read = match next {
                FromFile::Message(read) => read,
                FromFile::Skipped => {
                    job.update(|info| info.skipped += 1);
                    continue;
                }
                FromFile::Failed(err) => return Err(err),
            };
            let core = weak.upgrade().ok_or_else(gone)?;
            let store = core.store_ref()?;
            let (message_id, bytes) = match read.message_id {
                Some(id) => (id, read.bytes),
                None => {
                    let id = format!("{:032x}@mbox-import.invalid", random_u128());
                    let mut bytes = format!("Message-ID: <{id}>\r\n").into_bytes();
                    bytes.extend_from_slice(&read.bytes);
                    (id, bytes)
                }
            };
            let root = files::compose_dir(store);
            let dir = DraftId::from_random(random_u128());
            let relative = files::outgoing_path(&dir, "import.eml");
            let target = files::resolve(&root, &relative)?;
            let outbox = core.outbox_ref()?;
            let lock = outbox.files_lock();
            let held = lock.lock().await;
            tokio::task::spawn_blocking(move || files::write_atomically(&target, &bytes))
                .await
                .map_err(|err| CoreError::Internal(format!("writing failed: {err}")))??;
            batch += 1;
            let change = store
                .enqueue_import_appends(
                    mailbox,
                    vec![ImportItem {
                        file: relative,
                        flags: read.flags,
                        message_id_header: message_id,
                    }],
                    format!("{request_base}-{batch}"),
                )
                .await?;
            drop(held);
            outbox.changed(&change);
            pending.extend(change.operations.iter().map(|op| op.id));
            let queued = u32::try_from(pending.len()).unwrap_or(u32::MAX);
            job.update(|info| info.queued = queued);
            progress.tick(weak, job);
        }
        // The file is read: wait for the rest.
        while !pending.is_empty() {
            if job.cancelled() {
                return Ok(());
            }
            let core = weak.upgrade().ok_or_else(gone)?;
            *pending = settle(core.store_ref()?, job, std::mem::take(pending)).await?;
            drop(core);
            progress.tick(weak, job);
            if !pending.is_empty() {
                tokio::time::sleep(IMPORT_POLL).await;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
