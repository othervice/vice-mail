//! Stage 20, task C (decision 0060): the "Attachments" view across accounts.
//!
//! Every attachment of a message whose body is stored on this computer (opened, or downloaded
//! in advance; large messages with their attachment list from the structure) is listed once in
//! an index of the store, with the index the reading pane uses: open, save and preview go
//! through the same commands. Attachments not downloaded yet are fetched when opened.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{AccountId, Address, MailboxId, MarkScope, MessageId, PreviewKind};

/// The kinds the view filters by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentKind {
    Image,
    Pdf,
    /// Text documents, presentations, plain text.
    Document,
    /// Spreadsheets and CSV.
    Spreadsheet,
    /// Zip, tar, 7z, rar, gzip …
    Archive,
    Other,
}

const DOCUMENT_EXTENSIONS: &[&str] = &[
    "doc", "docx", "odt", "rtf", "txt", "md", "pages", "ppt", "pptx", "odp", "key", "tex", "epub",
];
const SPREADSHEET_EXTENSIONS: &[&str] = &["xls", "xlsx", "xlsm", "ods", "csv", "tsv", "numbers"];
const ARCHIVE_EXTENSIONS: &[&str] = &[
    "zip", "tar", "gz", "tgz", "bz2", "xz", "7z", "rar", "zst", "lz", "cab",
];
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "bmp", "tif", "tiff", "svg", "avif",
];

impl AttachmentKind {
    pub const ALL: [Self; 6] = [
        Self::Image,
        Self::Pdf,
        Self::Document,
        Self::Spreadsheet,
        Self::Archive,
        Self::Other,
    ];

    /// The kind by MIME type, and by the file name's extension where the type is generic.
    pub fn classify(name: &str, mime_type: &str) -> Self {
        let mime = mime_type.trim().to_ascii_lowercase();
        let extension = name
            .rsplit_once('.')
            .map(|(_, ext)| ext.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let ext = extension.as_str();
        if mime == "application/pdf" || ext == "pdf" {
            return Self::Pdf;
        }
        if mime.starts_with("image/") || IMAGE_EXTENSIONS.contains(&ext) {
            return Self::Image;
        }
        if mime.contains("spreadsheet")
            || mime.contains("ms-excel")
            || mime == "text/csv"
            || SPREADSHEET_EXTENSIONS.contains(&ext)
        {
            return Self::Spreadsheet;
        }
        if mime.contains("zip")
            || mime.contains("compressed")
            || mime.contains("x-tar")
            || mime.contains("gzip")
            || mime.contains("x-7z")
            || mime.contains("x-rar")
            || ARCHIVE_EXTENSIONS.contains(&ext)
        {
            return Self::Archive;
        }
        if mime.contains("wordprocessing")
            || mime.contains("msword")
            || mime.contains("presentation")
            || mime.contains("powerpoint")
            || mime.contains("opendocument.text")
            || mime == "application/rtf"
            || mime == "text/plain"
            || mime == "text/markdown"
            || DOCUMENT_EXTENSIONS.contains(&ext)
        {
            return Self::Document;
        }
        Self::Other
    }

    /// The stored code (`attachment.kind`).
    pub const fn code(self) -> i64 {
        match self {
            Self::Image => 0,
            Self::Pdf => 1,
            Self::Document => 2,
            Self::Spreadsheet => 3,
            Self::Archive => 4,
            Self::Other => 5,
        }
    }

    pub fn from_code(code: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.code() == code)
    }
}

/// What the view lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFilter {
    /// The accounts: all visible ones, a group's, or one.
    pub scope: MarkScope,
    /// Null: every kind.
    pub kind: Option<AttachmentKind>,
    /// Part of the sender's name or address (any case); null or empty: every sender.
    pub sender: Option<String>,
    /// Part of the file name (any case); null or empty: every name.
    pub query: Option<String>,
}

/// Keyset position in the view: below this attachment. Opaque; pass it back unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentCursor {
    pub message_id: MessageId,
    pub index: u32,
}

/// One attachment of the view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentEntry {
    pub message_id: MessageId,
    pub account_id: AccountId,
    /// Where the message is shown ("Go to message"): its inbox location if it has one, else
    /// another folder (never Trash or Junk).
    pub mailbox_id: MailboxId,
    /// The attachment's index in the reading pane (open, save, preview).
    pub index: u32,
    /// Sanitised file name.
    pub name: String,
    pub mime_type: String,
    pub kind: AttachmentKind,
    /// Decoded bytes (estimated for attachments not downloaded yet).
    pub size: u32,
    /// The bytes are on this computer; otherwise opening downloads them first.
    pub local: bool,
    /// What the preview shows; null: no preview, open it in the system.
    pub preview: Option<PreviewKind>,
    /// A program or script: opening asks first.
    pub executable: bool,
    pub from: Option<Address>,
    pub subject: String,
    /// The message's sort date, Unix milliseconds.
    #[specta(type = Number)]
    pub date: i64,
}

/// One page of the view, newest message first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPage {
    pub items: Vec<AttachmentEntry>,
    /// Pass to the next call to continue; null on the last page.
    pub next_cursor: Option<AttachmentCursor>,
}

/// Most attachments per page.
pub const MAX_ATTACHMENT_PAGE: u32 = 500;

/// Longest filter text (sender or name), in characters.
pub const MAX_ATTACHMENT_FILTER_CHARS: usize = 200;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_type_and_name() {
        let cases = [
            (
                "Invoice.pdf",
                "application/octet-stream",
                AttachmentKind::Pdf,
            ),
            ("scan", "application/pdf", AttachmentKind::Pdf),
            (
                "photo.JPG",
                "application/octet-stream",
                AttachmentKind::Image,
            ),
            ("x", "image/png", AttachmentKind::Image),
            (
                "Budget.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                AttachmentKind::Spreadsheet,
            ),
            ("data.csv", "text/csv", AttachmentKind::Spreadsheet),
            (
                "Letter.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                AttachmentKind::Document,
            ),
            ("notes.txt", "text/plain", AttachmentKind::Document),
            (
                "slides.pptx",
                "application/octet-stream",
                AttachmentKind::Document,
            ),
            ("backup.zip", "application/zip", AttachmentKind::Archive),
            ("logs.tar.gz", "application/gzip", AttachmentKind::Archive),
            ("invite.ics", "text/calendar", AttachmentKind::Other),
            (
                "setup.exe",
                "application/x-msdownload",
                AttachmentKind::Other,
            ),
        ];
        for (name, mime, kind) in cases {
            assert_eq!(AttachmentKind::classify(name, mime), kind, "{name} {mime}");
        }
        for kind in AttachmentKind::ALL {
            assert_eq!(AttachmentKind::from_code(kind.code()), Some(kind));
        }
        assert_eq!(AttachmentKind::from_code(9), None);
    }
}
