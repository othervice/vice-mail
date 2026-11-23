//! The calendar parts of a whole message: `text/calendar` (the inline alternative of an
//! invitation), `application/ics`, and attachments named `*.ics`. Outlook sends the same file
//! twice (alternative part and `invite.ics`); equal content is kept once.

use std::panic::{self, AssertUnwindSafe};
use std::sync::LazyLock;

use mail_parser::{MessageParser, MessagePart, PartKind};

use super::MAX_CALENDAR_BYTES;

/// Calendar parts returned per message; further ones are ignored.
pub const MAX_CALENDAR_PARTS: usize = 4;

static PARSER: LazyLock<MessageParser> = LazyLock::new(MessageParser::new);

/// A calendar file found in a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarPart {
    /// Part number, as `AttachmentPart::part_id` (for [`crate::attachment_bytes`], e.g. to hand
    /// the whole file to the system calendar).
    pub part_id: u32,
    /// The content for [`crate::read_calendar`]: transfer encoding undone, a text part converted
    /// from its charset to UTF-8. At most [`MAX_CALENDAR_BYTES`] + 1 bytes, so a larger file is
    /// still reported as `too_large`.
    pub data: Vec<u8>,
}

/// The calendar parts of a raw RFC 5322 message, in message order (parts of attached messages
/// excluded), distinct by content, at most [`MAX_CALENDAR_PARTS`]. Never panics; unreadable
/// input gives none.
pub fn calendar_parts(raw_message: &[u8]) -> Vec<CalendarPart> {
    panic::catch_unwind(AssertUnwindSafe(|| find(raw_message))).unwrap_or_default()
}

fn find(raw: &[u8]) -> Vec<CalendarPart> {
    let Some(message) = PARSER.parse(raw) else {
        return Vec::new();
    };
    let mut out: Vec<CalendarPart> = Vec::new();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    for part in message.root().parts() {
        if out.len() >= MAX_CALENDAR_PARTS {
            break;
        }
        if !is_calendar(&part) {
            continue;
        }
        let data = content(&part);
        let key = comparable(&data);
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(CalendarPart {
            part_id: part.id(),
            data,
        });
    }
    out
}

fn is_calendar(part: &MessagePart<'_>) -> bool {
    if matches!(part.kind(), PartKind::Multipart | PartKind::Message(_)) {
        return false;
    }
    let declared = part.content_type().is_some_and(|ct| {
        let subtype = ct.subtype().unwrap_or_default();
        let is = |t: &str, s: &str| {
            ct.ctype().eq_ignore_ascii_case(t) && subtype.eq_ignore_ascii_case(s)
        };
        is("text", "calendar")
            || is("text", "x-vcalendar")
            || is("application", "ics")
            || is("application", "x-ics")
    });
    declared
        || part.attachment_name().is_some_and(|name| {
            let name = name.trim();
            name.len() > 4
                && name
                    .get(name.len() - 4..)
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(".ics"))
        })
}

fn content(part: &MessagePart<'_>) -> Vec<u8> {
    let limit = MAX_CALENDAR_BYTES + 1;
    let mut bytes = match part.text_prefix(limit) {
        Some(text) => text.into_owned().into_bytes(),
        None => {
            let decoded = part.decoded();
            decoded.get(..limit).unwrap_or(&decoded).to_vec()
        }
    };
    bytes.truncate(limit);
    bytes
}

/// For comparing parts: without `\r`, trimmed.
fn comparable(data: &[u8]) -> Vec<u8> {
    let bytes: Vec<u8> = data.iter().copied().filter(|&b| b != b'\r').collect();
    bytes.trim_ascii().to_vec()
}
