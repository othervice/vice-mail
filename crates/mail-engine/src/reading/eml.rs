//! Stage 9 (decision 0027): `.eml` files shown read-only, outside every account.
//!
//! The shell hands the core a path: from the system (file association, launch argument, a
//! second start on Windows) or from the open dialog. The core reads the file once (at most
//! [`MAX_EML_FILE_BYTES`], checked before and while reading), checks that it is a mail (a
//! readable header with at least one address, subject, date or message id) and keeps its bytes
//! under a random [`EmlFileId`] while its window is open ([`MAX_OPEN_EML_FILES`] at most; one
//! more forgets the oldest). Nothing goes into the store: no account, no folder, no search
//! index, no contacts.
//!
//! Showing it is the reading pipeline of stored mail: the same parser, the same sanitiser and
//! render options (external content blocked, loaded once by the core only on request, never
//! "always for this sender": there is no account), link placeholders under a fresh render
//! token (`describeLink`/`openLink` check them as usual), and attachments decoded by the core,
//! saved where the user picks or opened as read-only copies (executables only after
//! confirmation).
//!
//! A file from the system that fails the checks is remembered with its error, so its window
//! can say why (there is no other place to show it at launch); a file from the dialog that
//! fails is answered with the error and remembered not at all.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use mail_content::{ParseProblem, ParsedMessage};
use mail_model::{
    BodyFormat, CoreError, EmlError, EmlFile, EmlFileId, EmlMessageView, MAX_EML_FILE_BYTES,
    MAX_OPEN_EML_FILES, MailConnector, MailScheme, MessageViewHeader, OpenAttachmentResult,
};

use super::links::{RenderOwner, Renderings};
use super::view::{AttachmentSource, Content};
use super::{Reading, blocking, files};
use crate::core::{AppCore, Connector};

/// One opened file: its name and its bytes, or why it could not be opened.
struct EmlEntry {
    name: String,
    raw: Result<Arc<Vec<u8>>, CoreError>,
}

/// The opened files, oldest first.
#[derive(Default)]
pub(crate) struct EmlFiles {
    entries: Mutex<VecDeque<(EmlFileId, Arc<EmlEntry>)>>,
}

impl EmlFiles {
    fn insert(&self, name: String, raw: Result<Arc<Vec<u8>>, CoreError>) -> EmlFile {
        let id = new_id();
        let mut entries = self.lock();
        entries.push_back((
            id.clone(),
            Arc::new(EmlEntry {
                name: name.clone(),
                raw,
            }),
        ));
        while entries.len() > MAX_OPEN_EML_FILES as usize {
            entries.pop_front();
        }
        EmlFile { id, name }
    }

    fn get(&self, id: &EmlFileId) -> Result<Arc<EmlEntry>, CoreError> {
        self.lock()
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, entry)| Arc::clone(entry))
            .ok_or_else(|| {
                CoreError::NotFound(format!("opened file {} (open it again)", id.as_str()))
            })
    }

    fn remove(&self, id: &EmlFileId) -> bool {
        let mut entries = self.lock();
        let before = entries.len();
        entries.retain(|(i, _)| i != id);
        entries.len() != before
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(EmlFileId, Arc<EmlEntry>)>> {
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A random id (the render tokens' source and format).
fn new_id() -> EmlFileId {
    let token = Renderings::new_token();
    EmlFileId::parse(token.as_str()).unwrap_or_else(|| EmlFileId::from_random(0))
}

/// The file's name for the window title, sanitised like an attachment name.
pub(crate) fn display_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    mail_content::sanitize_filename(&name)
}

/// Reads at most [`MAX_EML_FILE_BYTES`] of `path` (the size is checked before and while
/// reading, so a file that grows meanwhile is refused too).
pub(crate) fn read_file(path: &Path) -> Result<Vec<u8>, EmlError> {
    let unreadable = |err: std::io::Error| EmlError::Unreadable(err.to_string());
    let limit = u64::from(MAX_EML_FILE_BYTES);
    let meta = std::fs::metadata(path).map_err(unreadable)?;
    if !meta.is_file() {
        return Err(EmlError::Unreadable("not a file".into()));
    }
    if meta.len() > limit {
        return Err(EmlError::TooLarge {
            bytes: meta.len(),
            limit: MAX_EML_FILE_BYTES,
        });
    }
    let file = std::fs::File::open(path).map_err(unreadable)?;
    let mut raw = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.take(limit + 1)
        .read_to_end(&mut raw)
        .map_err(unreadable)?;
    if raw.len() as u64 > limit {
        return Err(EmlError::TooLarge {
            bytes: raw.len() as u64,
            limit: MAX_EML_FILE_BYTES,
        });
    }
    Ok(raw)
}

/// Whether `parsed` is a mail: a header the parser could read, with at least one address,
/// subject, date or message id. Plain text, binaries and empty files are not.
pub(crate) fn is_message(parsed: &ParsedMessage) -> bool {
    let headerless = parsed
        .problems
        .iter()
        .any(|p| matches!(p, ParseProblem::NoHeaders | ParseProblem::Internal));
    let h = &parsed.headers;
    let known = !h.from.is_empty()
        || !h.sender.is_empty()
        || !h.to.is_empty()
        || !h.cc.is_empty()
        || h.subject.as_deref().is_some_and(|s| !s.is_empty())
        || h.date.is_some()
        || h.message_id.is_some();
    !headerless && known
}

/// Reads and checks a file (blocking).
fn load(path: &Path) -> Result<Vec<u8>, EmlError> {
    let raw = read_file(path)?;
    if raw.is_empty() || !is_message(&mail_content::parse(&raw)) {
        return Err(EmlError::NotAMessage);
    }
    Ok(raw)
}

/// The header of the view: everything from the file (there is no stored copy). `date` is 0
/// without a readable `Date`.
fn file_header(parsed: &ParsedMessage) -> MessageViewHeader {
    let h = &parsed.headers;
    MessageViewHeader {
        from: h.from.clone(),
        to: h.to.clone(),
        cc: h.cc.clone(),
        bcc: h.bcc.clone(),
        reply_to: h.reply_to.clone(),
        date: h.date.map_or(0, |d| d.saturating_mul(1000)),
        subject: h.subject.clone().unwrap_or_default(),
    }
}

impl<C: MailConnector + 'static> Reading<C> {
    /// Opens a file the user picked: the error comes back, nothing is remembered then.
    pub(crate) async fn open_eml_file(&self, path: PathBuf) -> Result<EmlFile, CoreError> {
        let name = display_name(&path);
        let raw = blocking(move || load(&path)).await??;
        tracing::info!(bytes = raw.len(), "eml file opened");
        Ok(self.eml.insert(name, Ok(Arc::new(raw))))
    }

    /// Opens a file the system handed over. Always answers a file: one that failed keeps its
    /// error, which its window shows.
    pub(crate) async fn open_eml_from_system(&self, path: PathBuf) -> EmlFile {
        let name = display_name(&path);
        let raw = match blocking(move || load(&path)).await {
            Ok(Ok(raw)) => {
                tracing::info!(bytes = raw.len(), "eml file from the system opened");
                Ok(Arc::new(raw))
            }
            Ok(Err(err)) => {
                tracing::warn!(code = ?err.code(), "eml file from the system refused");
                Err(CoreError::from(err))
            }
            Err(err) => Err(err),
        };
        self.eml.insert(name, raw)
    }

    /// The bytes of an opened file, or the error it was opened with.
    fn eml_raw(&self, id: &EmlFileId) -> Result<(String, Arc<Vec<u8>>), CoreError> {
        let entry = self.eml.get(id)?;
        let raw = entry.raw.clone()?;
        Ok((entry.name.clone(), raw))
    }

    async fn eml_content(
        &self,
        id: &EmlFileId,
    ) -> Result<(String, Arc<Vec<u8>>, Content), CoreError> {
        let (name, raw) = self.eml_raw(id)?;
        let bytes = Arc::clone(&raw);
        let content = blocking(move || Content::complete(&bytes)).await?;
        Ok((name, raw, content))
    }

    /// The view of an opened file; external content only when `load_remote` (the user asked,
    /// for this rendering).
    pub(crate) async fn eml_view(
        &self,
        id: &EmlFileId,
        format: BodyFormat,
        load_remote: bool,
        scheme: MailScheme,
    ) -> Result<EmlMessageView, CoreError> {
        let (file_name, _, content) = self.eml_content(id).await?;
        let body = self
            .render_body(
                &content,
                format,
                load_remote,
                RenderOwner::File(id.clone()),
                scheme,
            )
            .await?;
        Ok(EmlMessageView {
            file_id: id.clone(),
            file_name,
            header: file_header(&content.parsed),
            body,
            formats: content.formats(),
            attachments: content.attachments.iter().map(|a| a.info.clone()).collect(),
            problems: content.problems(),
        })
    }

    /// Attachment `index` of an opened file: its sanitised name and decoded bytes.
    async fn eml_attachment(
        &self,
        id: &EmlFileId,
        index: u32,
    ) -> Result<(mail_model::AttachmentInfo, Vec<u8>), CoreError> {
        let (_, raw, content) = self.eml_content(id).await?;
        let attachment = content
            .attachments
            .into_iter()
            .find(|a| a.info.index == index)
            .ok_or_else(|| CoreError::NotFound(format!("attachment {index} of the file")))?;
        let AttachmentSource::Whole { part_id } = attachment.source else {
            return Err(CoreError::Internal(
                "an opened file has no partial attachments".into(),
            ));
        };
        let bytes = blocking(move || mail_content::attachment_bytes(&raw, part_id))
            .await?
            .ok_or_else(|| CoreError::Internal("the attachment could not be decoded".into()))?;
        Ok((attachment.info, bytes))
    }

    pub(crate) async fn eml_attachment_name(
        &self,
        id: &EmlFileId,
        index: u32,
    ) -> Result<String, CoreError> {
        Ok(self.eml_attachment(id, index).await?.0.name)
    }

    pub(crate) async fn save_eml_attachment_to(
        &self,
        id: &EmlFileId,
        index: u32,
        path: PathBuf,
    ) -> Result<PathBuf, CoreError> {
        let (_, bytes) = self.eml_attachment(id, index).await?;
        let saved = blocking(move || files::save_atomically(&path, &bytes)).await??;
        tracing::info!(index, "attachment of an opened file saved");
        Ok(saved)
    }

    /// Stage 16 (decision 0036): the preview of an attachment of an opened file.
    pub(crate) async fn preview_eml_attachment(
        &self,
        id: &EmlFileId,
        index: u32,
        page: u32,
    ) -> Result<mail_model::AttachmentPreview, CoreError> {
        let (_, _, content) = self.eml_content(id).await?;
        let info = content
            .attachments
            .iter()
            .find(|a| a.info.index == index)
            .map(|a| a.info.clone())
            .ok_or_else(|| CoreError::NotFound(format!("attachment {index} of the file")))?;
        let kind = super::preview_kind_of(&info)?;
        let (_, bytes) = self.eml_attachment(id, index).await?;
        self.render_preview(kind, bytes, page).await
    }

    /// Stage 16 (decision 0036): every attachment of an opened file into `folder`.
    pub(crate) async fn save_all_eml_attachments_to(
        &self,
        id: &EmlFileId,
        folder: PathBuf,
    ) -> Result<(u32, u32), CoreError> {
        let (_, _, content) = self.eml_content(id).await?;
        if content.attachments.is_empty() {
            return Err(CoreError::InvalidArgument(
                "the file has no attachments".into(),
            ));
        }
        let (mut saved, mut renamed) = (0u32, 0u32);
        for index in content.attachments.iter().map(|a| a.info.index) {
            let (info, bytes) = self.eml_attachment(id, index).await?;
            let name = mail_content::sanitize_filename(&info.name);
            if super::save_into(&folder, name, bytes).await? {
                renamed += 1;
            }
            saved += 1;
        }
        tracing::info!(saved, renamed, "all attachments of an opened file saved");
        Ok((saved, renamed))
    }

    pub(crate) async fn open_eml_attachment(
        &self,
        id: &EmlFileId,
        index: u32,
        confirmed_executable: bool,
    ) -> Result<OpenAttachmentResult, CoreError> {
        let (info, bytes) = self.eml_attachment(id, index).await?;
        if info.executable && !confirmed_executable {
            return Ok(OpenAttachmentResult::NeedsConfirmation);
        }
        let name = mail_content::sanitize_filename(&info.name);
        self.open_file_copy(name, bytes).await?;
        tracing::info!(
            index,
            executable = info.executable,
            "attachment of an opened file opened"
        );
        Ok(OpenAttachmentResult::Opened)
    }

    /// Forgets an opened file and its renderings (its window closed).
    pub(crate) fn close_eml_file(&self, id: &EmlFileId) -> bool {
        self.renderings.forget_file(id);
        self.eml.remove(id)
    }
}

/// Stage 9 (decision 0027): `.eml` files, read-only and outside every account.
impl<C: Connector> AppCore<C> {
    /// Opens a file the user picked in the open dialog; typed errors (`eml_too_large`,
    /// `eml_invalid`, `eml_unreadable`) when it cannot be shown.
    pub async fn open_eml_file(&self, path: PathBuf) -> Result<EmlFile, CoreError> {
        self.reading_ref()?.open_eml_file(path).await
    }

    /// Opens a file the system handed over (file association, launch argument). A file that
    /// cannot be shown is remembered with its error, which `getEmlMessage` answers.
    pub async fn open_eml_from_system(&self, path: PathBuf) -> Result<EmlFile, CoreError> {
        Ok(self.reading_ref()?.open_eml_from_system(path).await)
    }

    /// The opened file as the reading pane shows it; `not_found` once its window closed.
    /// Colours as in the light theme ([`Self::get_eml_message_for`]).
    pub async fn get_eml_message(
        &self,
        id: EmlFileId,
        format: BodyFormat,
    ) -> Result<EmlMessageView, CoreError> {
        self.get_eml_message_for(id, format, MailScheme::Light)
            .await
    }

    /// Stage 21 (decision 0066): [`Self::get_eml_message`] for the theme the window shows.
    pub async fn get_eml_message_for(
        &self,
        id: EmlFileId,
        format: BodyFormat,
        scheme: MailScheme,
    ) -> Result<EmlMessageView, CoreError> {
        self.reading_ref()?
            .eml_view(&id, format, false, scheme)
            .await
    }

    /// The HTML again, with its external images loaded by the core (this rendering only).
    pub async fn load_eml_remote_content(
        &self,
        id: EmlFileId,
    ) -> Result<EmlMessageView, CoreError> {
        self.load_eml_remote_content_for(id, MailScheme::Light)
            .await
    }

    /// Stage 21: [`Self::load_eml_remote_content`] for the theme the window shows.
    pub async fn load_eml_remote_content_for(
        &self,
        id: EmlFileId,
        scheme: MailScheme,
    ) -> Result<EmlMessageView, CoreError> {
        self.reading_ref()?
            .eml_view(&id, BodyFormat::Html, true, scheme)
            .await
    }

    /// The sanitised name of an attachment, for the save dialog.
    pub async fn eml_attachment_name(
        &self,
        id: EmlFileId,
        index: u32,
    ) -> Result<String, CoreError> {
        self.reading_ref()?.eml_attachment_name(&id, index).await
    }

    /// Writes an attachment to the path the user chose in the save dialog.
    pub async fn save_eml_attachment_to(
        &self,
        id: EmlFileId,
        index: u32,
        path: PathBuf,
    ) -> Result<PathBuf, CoreError> {
        self.reading_ref()?
            .save_eml_attachment_to(&id, index, path)
            .await
    }

    /// Opens a read-only copy of an attachment; executables only with `confirmed_executable`.
    pub async fn open_eml_attachment(
        &self,
        id: EmlFileId,
        index: u32,
        confirmed_executable: bool,
    ) -> Result<OpenAttachmentResult, CoreError> {
        self.reading_ref()?
            .open_eml_attachment(&id, index, confirmed_executable)
            .await
    }

    /// Stage 16 (decision 0036): the preview of an attachment of an opened file.
    pub async fn preview_eml_attachment(
        &self,
        id: EmlFileId,
        index: u32,
        page: u32,
    ) -> Result<mail_model::AttachmentPreview, CoreError> {
        self.reading_ref()?
            .preview_eml_attachment(&id, index, page)
            .await
    }

    /// Stage 16 (decision 0036): every attachment of an opened file into the folder the user
    /// chose; how many were written, and how many of them under another name.
    pub async fn save_all_eml_attachments_to(
        &self,
        id: EmlFileId,
        folder: PathBuf,
    ) -> Result<(u32, u32), CoreError> {
        self.reading_ref()?
            .save_all_eml_attachments_to(&id, folder)
            .await
    }

    /// Forgets an opened file (the shell calls it when its window closes).
    pub fn close_eml_file(&self, id: &EmlFileId) -> bool {
        self.reading_ref()
            .map(|reading| reading.close_eml_file(id))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests;
