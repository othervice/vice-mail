//! Stage 9 (decision 0027): `.eml` files shown read-only, outside every account. The system
//! hands the app a file (file association, launch argument) or the user picks one in the
//! core's open dialog; the core reads and checks it, keeps its bytes under an unguessable id
//! while its window is open, and renders it through the same pipeline as stored mail.

use serde::{Deserialize, Serialize};

use crate::{
    AttachmentInfo, BodyFormat, ErrorCode, MessageBody, MessageProblem, MessageViewHeader,
};

/// Largest `.eml` file the app opens (64 MiB: a 45 MB attachment in base64 still fits).
pub const MAX_EML_FILE_BYTES: u32 = 64 * 1024 * 1024;

/// Most `.eml` files kept open at once; opening one more forgets the oldest.
pub const MAX_OPEN_EML_FILES: u32 = 16;

/// An opened `.eml` file: 32 lowercase hex digits from the operating system's random source,
/// so one window cannot name another window's file by guessing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct EmlFileId(String);

impl EmlFileId {
    pub const LEN: usize = 32;

    /// `None` unless `text` is exactly 32 lowercase hex digits.
    pub fn parse(text: &str) -> Option<Self> {
        (text.len() == Self::LEN
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(text.to_owned()))
    }

    /// From 128 random bits (the engine draws them; this crate has no randomness).
    pub fn from_random(bits: u128) -> Self {
        Self(format!("{bits:032x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What opening a file answers: its id and its name (sanitised, for the window title).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EmlFile {
    pub id: EmlFileId,
    pub name: String,
}

/// "Open .eml file…": the user cancelled the dialog, or the file opened in its own window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OpenEmlFileResult {
    Cancelled,
    Opened { file: EmlFile },
}

/// The view of an opened file, like `MessageView` without account and message id: the same
/// header, body (sanitised HTML with a fresh render token, or text), formats, attachments and
/// problems. `header.date` is `0` when the file has no readable `Date`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EmlMessageView {
    pub file_id: EmlFileId,
    pub file_name: String,
    pub header: MessageViewHeader,
    pub body: MessageBody,
    pub formats: Vec<BodyFormat>,
    pub attachments: Vec<AttachmentInfo>,
    pub problems: Vec<MessageProblem>,
}

/// Why a file could not be opened. Nothing was imported or changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EmlError {
    /// Larger than [`MAX_EML_FILE_BYTES`].
    #[error("the file has {bytes} bytes, at most {limit} are opened")]
    TooLarge { bytes: u64, limit: u32 },
    /// Empty, or no mail header the app could read.
    #[error("the file is not an email message (no readable header)")]
    NotAMessage,
    /// Missing, a folder, or the system refused to read it; holds the system's text.
    #[error("the file could not be read: {0}")]
    Unreadable(String),
}

impl EmlError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::TooLarge { .. } => ErrorCode::EmlTooLarge,
            Self::NotAMessage => ErrorCode::EmlInvalid,
            Self::Unreadable(_) => ErrorCode::EmlUnreadable,
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::TooLarge { .. } => "The file is too large to open.",
            Self::NotAMessage => "The file is not an email message.",
            Self::Unreadable(_) => "The file could not be read.",
        }
    }

    pub fn detail(&self) -> Option<String> {
        Some(self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_32_lowercase_hex_digits() {
        let id = EmlFileId::from_random(0xabc);
        assert_eq!(id.as_str().len(), EmlFileId::LEN);
        assert_eq!(EmlFileId::parse(id.as_str()), Some(id));
        assert!(EmlFileId::parse("ABC").is_none());
        assert!(EmlFileId::parse(&"G".repeat(32)).is_none());
        assert!(EmlFileId::parse(&"a".repeat(33)).is_none());
        assert!(EmlFileId::parse("../../../../etc/passwd/aaaaaaaaaa").is_none());
    }

    #[test]
    fn errors_have_their_codes_and_details() {
        let large = EmlError::TooLarge {
            bytes: 70_000_000,
            limit: MAX_EML_FILE_BYTES,
        };
        assert_eq!(large.code(), ErrorCode::EmlTooLarge);
        assert!(large.detail().is_some_and(|d| d.contains("70000000")));
        assert_eq!(EmlError::NotAMessage.code(), ErrorCode::EmlInvalid);
        assert_eq!(
            EmlError::Unreadable("denied".into()).code(),
            ErrorCode::EmlUnreadable
        );
        let core: crate::CoreError = EmlError::NotAMessage.into();
        assert_eq!(core.code(), ErrorCode::EmlInvalid);
        assert_eq!(core.message(), "The file is not an email message.");
    }

    #[test]
    fn wire_format() {
        let opened = OpenEmlFileResult::Opened {
            file: EmlFile {
                id: EmlFileId::from_random(1),
                name: "a.eml".into(),
            },
        };
        let json = serde_json::to_value(&opened).expect("json");
        assert_eq!(json["kind"], "opened");
        assert_eq!(json["file"]["id"], "00000000000000000000000000000001");
        assert_eq!(
            serde_json::to_value(OpenEmlFileResult::Cancelled).expect("json")["kind"],
            "cancelled"
        );
    }
}
