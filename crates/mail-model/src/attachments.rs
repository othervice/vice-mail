//! Stage 16 (decision 0036): attachment preview, "save all" and pinned attachments.
//!
//! - **Preview.** The core decodes the attachment and hands the web view only what it renders
//!   itself: images re-encoded as PNG or JPEG (at most [`MAX_PREVIEW_EDGE`] pixels per side),
//!   PDF pages rasterised to PNG one at a time, plain text (at most [`MAX_PREVIEW_TEXT_BYTES`]).
//!   Nothing of the attachment runs or loads anything; HTML and SVG are never previewed.
//! - **Save all**: one folder from the system dialog, every attachment under its sanitised name,
//!   an existing file is never replaced (`name (2).ext` instead).
//! - **Pinned attachments**: a local mark by account UUID, Message-ID and attachment index,
//!   synced as kind `attachmentPin`; the pin keeps name, type and size, so the list shows them
//!   without the body.

use serde::{Deserialize, Serialize};
use specta_typescript::Number;

use crate::{ErrorCode, MessageListItem};

/// Largest attachment the core previews (decoded bytes). Larger ones open in the system.
pub const MAX_PREVIEW_INPUT_BYTES: u32 = 32 * 1024 * 1024;

/// Longest side of a previewed image or PDF page, in pixels.
pub const MAX_PREVIEW_EDGE: u32 = 2048;

/// Largest image the core decodes for a preview, in pixels (width × height).
pub const MAX_PREVIEW_PIXELS: u64 = 64 * 1024 * 1024;

/// Most bytes of text a preview shows; longer text is cut at a character boundary.
pub const MAX_PREVIEW_TEXT_BYTES: u32 = 512 * 1024;

/// Most pages of a PDF the preview pages through.
pub const MAX_PREVIEW_PDF_PAGES: u32 = 200;

/// Most pinned attachments; the view lists at most this many, the newest first.
pub const MAX_PINNED_ATTACHMENTS: u32 = 1_000;

/// What the preview of an attachment shows. Null on an attachment the core does not preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PreviewKind {
    /// PNG, JPEG, GIF (first frame) or WebP, recognised by its bytes.
    Image,
    /// A PDF document, page by page.
    Pdf,
    /// Plain text (`text/plain`, CSV, Markdown, logs; never HTML or XML-based images).
    Text,
}

/// `previewAttachment`: what the preview pane shows. Images are `data:` URIs of bytes the core
/// encoded itself; text is shown as text, never as markup.
// `Deserialize`: the preview helper sends it back to the app (decision 0043).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AttachmentPreview {
    Image {
        /// `data:image/png;base64,…` or `data:image/jpeg;base64,…`, encoded by the core.
        data_url: String,
        width: u32,
        height: u32,
        /// The image was scaled down to fit [`MAX_PREVIEW_EDGE`].
        scaled: bool,
    },
    Pdf {
        /// `data:image/png;base64,…` of the page, rasterised by the core.
        data_url: String,
        width: u32,
        height: u32,
        /// 0-based page shown.
        page: u32,
        /// Pages the preview can show (at most [`MAX_PREVIEW_PDF_PAGES`]).
        pages: u32,
        /// The document has more pages than the preview shows.
        more_pages: bool,
    },
    Text {
        text: String,
        /// Cut after [`MAX_PREVIEW_TEXT_BYTES`].
        truncated: bool,
    },
}

/// Why an attachment could not be previewed. Nothing was shown or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreviewError {
    /// Not an image, PDF or text the core previews (HTML, SVG, programs, other formats), or
    /// its bytes are not what its type says.
    #[error("the core does not preview this attachment ({0})")]
    Unsupported(String),
    /// Larger than [`MAX_PREVIEW_INPUT_BYTES`] or [`MAX_PREVIEW_PIXELS`].
    #[error("the attachment is too large for a preview: {0}")]
    TooLarge(String),
    /// The image or PDF could not be decoded (damaged, encrypted, a page out of range).
    #[error("the attachment could not be read for a preview: {0}")]
    Damaged(String),
}

impl PreviewError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Unsupported(_) => ErrorCode::PreviewUnsupported,
            Self::TooLarge(_) => ErrorCode::PreviewTooLarge,
            Self::Damaged(_) => ErrorCode::PreviewDamaged,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "There is no preview for this attachment.",
            Self::TooLarge(_) => "The attachment is too large for a preview.",
            Self::Damaged(_) => "The attachment could not be read for a preview.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        Some(self.to_string())
    }
}

/// `saveAllAttachments`: where the attachments went, or nothing when the dialog was cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SaveAllAttachmentsResult {
    Saved {
        /// The folder the user chose.
        folder: String,
        /// Files written.
        saved: u32,
        /// Of those, written under another name because the name was taken.
        renamed: u32,
    },
    Cancelled,
}

/// A row of "Pinned attachments": the attachment as it was pinned, and its message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PinnedAttachment {
    /// The message, as a list row (in its inbox where it lies there).
    pub message: MessageListItem,
    /// The attachment's position in the message (`AttachmentInfo.index`).
    pub index: u32,
    /// Sanitised file name at pinning time.
    pub name: String,
    pub mime_type: String,
    pub size: u32,
    /// What its preview shows; null when there is none.
    pub preview: Option<PreviewKind>,
    /// Unix milliseconds.
    #[specta(type = Number)]
    pub pinned_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_and_codes() {
        let image = AttachmentPreview::Image {
            data_url: "data:image/png;base64,AA==".into(),
            width: 1,
            height: 2,
            scaled: false,
        };
        let json = serde_json::to_value(&image).expect("json");
        assert_eq!(json["kind"], "image");
        assert_eq!(json["dataUrl"], "data:image/png;base64,AA==");
        let pdf = AttachmentPreview::Pdf {
            data_url: String::new(),
            width: 1,
            height: 1,
            page: 0,
            pages: 3,
            more_pages: false,
        };
        assert_eq!(
            serde_json::to_value(&pdf).expect("json")["morePages"],
            false
        );
        assert_eq!(
            serde_json::to_value(SaveAllAttachmentsResult::Cancelled).expect("json")["kind"],
            "cancelled"
        );
        assert_eq!(
            serde_json::to_value(PreviewKind::Pdf).expect("json"),
            serde_json::json!("pdf")
        );
        let core: crate::CoreError = PreviewError::TooLarge("x".into()).into();
        assert_eq!(core.code(), ErrorCode::PreviewTooLarge);
        assert_eq!(core.message(), "The attachment is too large for a preview.");
        assert_eq!(
            PreviewError::Unsupported("text/html".into()).code(),
            ErrorCode::PreviewUnsupported
        );
        assert_eq!(
            PreviewError::Damaged("x".into()).code(),
            ErrorCode::PreviewDamaged
        );
    }
}
