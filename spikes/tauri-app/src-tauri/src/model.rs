//! IPC types.
//!
//! Every type derives both `specta::Type` (for tauri-specta) and `ts_rs::TS` (for ts-rs) so the
//! two generators can be compared on identical input. The product will keep only one of them.
//!
//! Large integers: JavaScript `number` is an IEEE 754 double, so integers above 2^53 lose
//! precision in `JSON.parse`. Every `u64`/`i64` field therefore states its mapping explicitly:
//! either `number` (value is known to stay below 2^53) or `string` (value may exceed 2^53).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Arguments of `get_message`.
#[derive(Debug, Clone, Deserialize, specta::Type, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageQuery {
    pub account_id: String,
    pub uid: u32,
    pub include_body: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
    pub account_id: String,
    pub mailbox: String,
    pub uid: u32,
    /// CONDSTORE mod-sequence: up to 2^63 - 1, so it crosses the bridge as a decimal string.
    #[serde(with = "u64_as_string")]
    #[specta(type = String)]
    #[ts(type = "string")]
    pub mod_seq: u64,
    /// Message size. Mail sizes stay far below 2^53, so `number` is safe.
    #[specta(type = specta_typescript::Number)]
    #[ts(type = "number")]
    pub size_bytes: u64,
    /// IMAP INTERNALDATE as Unix milliseconds. Safe as `number` until the year 287396.
    #[specta(type = specta_typescript::Number)]
    #[ts(type = "number")]
    pub internal_date_ms: i64,
    /// `Date` header, RFC 3339 string on the wire.
    pub sent_at: Option<DateTime<Utc>>,
    pub subject: Option<String>,
    pub from: Address,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub flags: Vec<Flag>,
    pub body: Body,
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS)]
pub struct Address {
    pub name: Option<String>,
    pub email: String,
}

/// Adjacently tagged: `{ "kind": "seen" }` or `{ "kind": "keyword", "value": "$Label1" }`.
#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Flag {
    Seen,
    Answered,
    Flagged,
    Draft,
    Keyword(String),
}

/// Internally tagged enum with data.
#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Body {
    NotLoaded,
    Plain {
        text: String,
    },
    Html {
        sanitized_html: String,
        blocked_remote_images: u32,
    },
}

#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub part_id: String,
    pub file_name: Option<String>,
    pub mime_type: String,
    #[specta(type = specta_typescript::Number)]
    #[ts(type = "number")]
    pub size_bytes: u64,
    pub content_id: Option<String>,
}

/// Typed error that crosses the bridge as `{ code, message, detail }`.
#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: ErrorCode,
    /// Human-readable, safe to show.
    pub message: String,
    /// Technical detail for logs and an expandable section, never secrets.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    InvalidArgument,
    ReadOnly,
    Internal,
}

impl IpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>, detail: Option<String>) -> Self {
        Self {
            code,
            message: message.into(),
            detail,
        }
    }
}

/// The one event stream from the core to the UI. Internally tagged by `type`.
#[derive(Debug, Clone, Serialize, specta::Type, ts_rs::TS, tauri_specta::Event)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CoreEvent {
    SyncProgress {
        account_id: String,
        mailbox: String,
        done: u32,
        total: u32,
    },
    MessagesChanged {
        account_id: String,
        mailbox: String,
        uids: Vec<u32>,
    },
    AccountError {
        account_id: String,
        error: IpcError,
    },
}

/// Event name used on the wire. tauri-specta derives it from the type name (kebab-case);
/// ts-rs has no notion of events, so the ts-rs path repeats this literal in TypeScript.
pub const CORE_EVENT_NAME: &str = <CoreEvent as tauri_specta::Event>::NAME;

mod u64_as_string {
    use serde::Serializer;

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(value)
    }
}
