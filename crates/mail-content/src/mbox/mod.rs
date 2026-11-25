//! mbox files (stage 19): reading a folder exported by another program (Thunderbird's local
//! folders, mutt, Apple Mail's `mbox` export) message by message, and writing a folder as
//! mboxrd. Pure: the engine reads the file line by line and pushes the lines in, and writes the
//! bytes this module produces; nothing here opens a file or reads a clock.
//!
//! **Reading** ([`MboxReader`]) is a push parser, so a file of any size is read with bounded
//! memory: at most one message ([`MAX_MBOX_MESSAGE_BYTES`]) is held; a larger one is counted and
//! dropped while it is read ([`MboxEntry::TooLarge`]). The work is linear in the input.
//!
//! - A line is a **separator** if it starts with `From `, is the first line of the file or
//!   follows an empty line, and has the shape `From <token> <rest>` where `<rest>` holds a
//!   four-digit year (19xx, 20xx) or a time `hh:mm` (Thunderbird writes
//!   `From - Sat Nov 22 10:00:00 2025`, mutt `From x@y Sat Nov 22 10:00:00 2025`, some writers add a
//!   time zone). Before the first message the empty line is not required. A body line such as
//!   `From here on…` that does not have that shape stays in the body.
//! - The empty line before a separator belongs to the separator and is not part of the message.
//! - Quoting is undone the mboxrd way: one `>` is removed from every line matching `^>+From `.
//!   For mboxo files (Thunderbird) this gives back every escaped `From ` line; a line that was
//!   `>From ` before mboxo quoting comes back as `From ` (mboxo cannot tell them apart).
//! - Every line of a message ends in CRLF (RFC 5322), also the last one when the file did not
//!   end with a line break.
//! - Thunderbird's own status fields `X-Mozilla-Status`, `X-Mozilla-Status2` and
//!   `X-Mozilla-Keys` are removed from the header block (with their continuation lines); the
//!   status is reported as [`MozillaStatus`] (read, replied, flagged, deleted). A deleted message
//!   is one Thunderbird has not compacted away yet; skipping it is the caller's decision.
//!
//! **Writing** ([`write_mbox_message`]) produces mboxrd: the separator from [`mbox_from_line`],
//! the message with LF line endings, every line matching `^>*From ` quoted with one more `>`, and
//! an empty line. Reading it back gives the message with CRLF line endings: byte for byte the
//! original for a message that used CRLF and ended with a line break.

#[cfg(test)]
mod tests;

/// The largest message kept while reading; a larger one is counted and dropped
/// ([`MboxEntry::TooLarge`]).
pub const MAX_MBOX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// Thunderbird's status of a message from its `X-Mozilla-Status` field (four hex digits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MozillaStatus {
    /// `0x0001`.
    pub read: bool,
    /// `0x0002`.
    pub replied: bool,
    /// `0x0004` ("marked", the star).
    pub flagged: bool,
    /// `0x0008` (expunged): deleted in Thunderbird, still in the file until it compacts it.
    pub deleted: bool,
}

impl MozillaStatus {
    fn from_hex(value: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(value).ok()?.trim();
        let digits = text
            .strip_prefix("0x")
            .or_else(|| text.strip_prefix("0X"))
            .unwrap_or(text);
        if digits.is_empty() || digits.len() > 8 {
            return None;
        }
        let bits = u32::from_str_radix(digits, 16).ok()?;
        Some(Self {
            read: bits & 0x1 != 0,
            replied: bits & 0x2 != 0,
            flagged: bits & 0x4 != 0,
            deleted: bits & 0x8 != 0,
        })
    }
}

/// What the reader gives for each message of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MboxEntry {
    /// One message: separator line removed, `>From ` quoting undone, every line ending in CRLF,
    /// the empty line before the next separator removed, Thunderbird's status fields removed
    /// from the header block (`mozilla` holds what `X-Mozilla-Status` said, if it was there).
    Message {
        bytes: Vec<u8>,
        mozilla: Option<MozillaStatus>,
    },
    /// Over [`MAX_MBOX_MESSAGE_BYTES`]: skipped; only its size (with CRLF line endings) was
    /// counted, the bytes were dropped while reading.
    TooLarge { bytes: u64 },
}

/// Header fields of Thunderbird's that are removed from imported messages (lower case, with the
/// colon).
const MOZILLA_FIELDS: [&[u8]; 3] = [
    b"x-mozilla-status:",
    b"x-mozilla-status2:",
    b"x-mozilla-keys:",
];

/// The message being read.
#[derive(Debug, Default)]
struct Current {
    bytes: Vec<u8>,
    /// Bytes of the message so far (with CRLF line endings), also after it got too large.
    size: u64,
    too_large: bool,
    /// Still in the header block (no empty line seen yet).
    in_header: bool,
    /// The previous header line was a removed Mozilla field: its continuation lines go too.
    skipping_field: bool,
    mozilla: Option<MozillaStatus>,
    /// An empty line that is held back: it belongs to the next separator if one follows.
    pending_blank: bool,
}

/// A push parser for mbox files; see the module documentation.
#[derive(Debug)]
pub struct MboxReader {
    limit: usize,
    current: Option<Current>,
    /// Physical lines seen so far.
    lines: u64,
    /// Lines before the first separator.
    lines_before: u64,
    /// The previous line was empty.
    previous_empty: bool,
}

impl Default for MboxReader {
    fn default() -> Self {
        Self::new()
    }
}

impl MboxReader {
    pub fn new() -> Self {
        Self::with_limit(MAX_MBOX_MESSAGE_BYTES)
    }

    /// A reader with another limit per message (tests).
    pub(crate) fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            current: None,
            lines: 0,
            lines_before: 0,
            previous_empty: false,
        }
    }

    /// One line of the file with its line ending (`\n` or `\r\n`; the last line may have none).
    /// Returns the previous message when this line starts a new one.
    pub fn push_line(&mut self, line: &[u8]) -> Option<MboxEntry> {
        let content = line_content(line);
        let first = self.lines == 0;
        self.lines = self.lines.saturating_add(1);
        let may_separate = first || self.previous_empty || self.current.is_none();
        self.previous_empty = content.is_empty();
        if may_separate && is_mbox_separator(content) {
            let finished = self.current.take().map(finish_message);
            self.current = Some(Current {
                in_header: true,
                ..Current::default()
            });
            return finished;
        }
        match self.current.as_mut() {
            None => {
                self.lines_before = self.lines_before.saturating_add(1);
            }
            Some(current) => add_line(current, content, self.limit),
        }
        None
    }

    /// End of the file: the last message, if any.
    pub fn finish(&mut self) -> Option<MboxEntry> {
        self.previous_empty = false;
        self.current.take().map(finish_message)
    }

    /// Lines seen before the first separator (a file that is not an mbox has only these).
    pub fn lines_before_first(&self) -> u64 {
        self.lines_before
    }
}

/// The line without its line ending (`\n`, `\r\n`).
fn line_content(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn add_line(current: &mut Current, content: &[u8], limit: usize) {
    if content.is_empty() {
        current.in_header = false;
        current.skipping_field = false;
        if current.pending_blank {
            append(current, b"", limit);
        }
        current.pending_blank = true;
        return;
    }
    if current.pending_blank {
        current.pending_blank = false;
        append(current, b"", limit);
    }
    if current.in_header {
        let continuation = matches!(content.first(), Some(b' ' | b'\t'));
        if continuation && current.skipping_field {
            return;
        }
        current.skipping_field = false;
        if !continuation && let Some(field) = mozilla_field(content) {
            current.skipping_field = true;
            if field == 0 {
                let value = content.get(MOZILLA_FIELDS[0].len()..).unwrap_or_default();
                if let Some(status) = MozillaStatus::from_hex(value) {
                    current.mozilla = Some(status);
                }
            }
            return;
        }
    }
    append(current, unquote(content), limit);
}

/// Which of [`MOZILLA_FIELDS`] the header line starts, if any.
fn mozilla_field(content: &[u8]) -> Option<usize> {
    MOZILLA_FIELDS.iter().position(|name| {
        content
            .get(..name.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(name))
    })
}

/// mboxrd: one `>` less on lines matching `^>+From `.
fn unquote(content: &[u8]) -> &[u8] {
    let quotes = content.iter().take_while(|&&b| b == b'>').count();
    if quotes > 0
        && content
            .get(quotes..)
            .is_some_and(|rest| rest.starts_with(b"From "))
    {
        content.get(1..).unwrap_or(content)
    } else {
        content
    }
}

/// Appends one line with CRLF, or only counts it once the message is too large.
fn append(current: &mut Current, content: &[u8], limit: usize) {
    let added = content.len().saturating_add(2);
    current.size = current
        .size
        .saturating_add(u64::try_from(added).unwrap_or(u64::MAX));
    if current.too_large {
        return;
    }
    if current.bytes.len().saturating_add(added) > limit {
        current.too_large = true;
        current.bytes = Vec::new();
        return;
    }
    current.bytes.extend_from_slice(content);
    current.bytes.extend_from_slice(b"\r\n");
}

fn finish_message(current: Current) -> MboxEntry {
    // A held-back empty line is the one before the separator (or the end of the file).
    if current.too_large {
        MboxEntry::TooLarge {
            bytes: current.size,
        }
    } else {
        MboxEntry::Message {
            bytes: current.bytes,
            mozilla: current.mozilla,
        }
    }
}

/// Whether a line (with or without its line ending) looks like an mbox separator:
/// `From <token> <rest>` with a four-digit year (19xx, 20xx) or a time `hh:mm` in `<rest>`.
pub fn is_mbox_separator(line: &[u8]) -> bool {
    let line = line_content(line);
    let Some(after) = line.strip_prefix(b"From ") else {
        return false;
    };
    let token_end = after
        .iter()
        .position(|b| b.is_ascii_whitespace())
        .unwrap_or(after.len());
    if token_end == 0 {
        return false;
    }
    let rest = after.get(token_end..).unwrap_or_default();
    if rest.first() != Some(&b' ') && rest.first() != Some(&b'\t') {
        return false;
    }
    has_year(rest) || has_time(rest)
}

/// A run of exactly four digits starting with `19` or `20`.
fn has_year(rest: &[u8]) -> bool {
    let mut start = 0;
    while start < rest.len() {
        if !rest.get(start).is_some_and(u8::is_ascii_digit) {
            start += 1;
            continue;
        }
        let run = rest
            .get(start..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        let digits = rest.get(start..start + run).unwrap_or_default();
        if run == 4 && (digits.starts_with(b"19") || digits.starts_with(b"20")) {
            return true;
        }
        start += run;
    }
    false
}

/// `h:mm` or `hh:mm` (minutes 00–59).
fn has_time(rest: &[u8]) -> bool {
    rest.iter().enumerate().any(|(i, &b)| {
        if b != b':' || i == 0 {
            return false;
        }
        let before = rest.get(i - 1).is_some_and(u8::is_ascii_digit);
        let minutes = match (rest.get(i + 1), rest.get(i + 2)) {
            (Some(tens @ b'0'..=b'5'), Some(ones)) => {
                tens.is_ascii_digit() && ones.is_ascii_digit()
            }
            _ => false,
        };
        let third = rest.get(i + 3).is_some_and(u8::is_ascii_digit);
        before && minutes && !third
    })
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Latest second written (9999-12-31 23:59:59 UTC); later or earlier than 1970 becomes the
/// epoch.
const MAX_DATE_SECS: i64 = 253_402_300_799;

/// `Www Mmm dd hh:mm:ss yyyy` in UTC, the day padded with a space (`Nov  2`).
fn asctime(date_secs: i64) -> String {
    let secs = if (0..=MAX_DATE_SECS).contains(&date_secs) {
        date_secs
    } else {
        0
    };
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // Days to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // 1970-01-01 was a Thursday.
    let weekday = usize::try_from((days + 4).rem_euclid(7)).unwrap_or(0);
    let month_index = usize::try_from(month - 1).unwrap_or(0);
    format!(
        "{} {} {day:>2} {hour:02}:{minute:02}:{second:02} {year}",
        WEEKDAYS.get(weekday).copied().unwrap_or("Thu"),
        MONTHS.get(month_index).copied().unwrap_or("Jan"),
    )
}

/// The longest envelope sender kept in a separator line.
const MAX_SENDER_CHARS: usize = 256;

/// `From <sender> <asctime UTC>\n`; the sender is `MAILER-DAEMON` when it is `None` or unusable
/// (empty, longer than 256 characters, or with whitespace or control characters).
pub fn mbox_from_line(envelope_sender: Option<&str>, date_secs: i64) -> String {
    let sender = envelope_sender
        .filter(|s| {
            !s.is_empty()
                && s.chars().count() <= MAX_SENDER_CHARS
                && s.chars().all(|c| !c.is_whitespace() && !c.is_control())
                && !s.chars().any(mail_model::is_bidi_control)
        })
        .unwrap_or("MAILER-DAEMON");
    format!("From {sender} {}\n", asctime(date_secs))
}

/// Appends one message in mboxrd form: the separator line, the message with LF line endings where
/// every line matching `^>*From ` gets one more `>`, a final line break if it was missing, then
/// one empty line.
pub fn write_mbox_message(
    out: &mut Vec<u8>,
    envelope_sender: Option<&str>,
    date_secs: i64,
    raw: &[u8],
) {
    out.extend_from_slice(mbox_from_line(envelope_sender, date_secs).as_bytes());
    let mut rest = raw;
    while !rest.is_empty() {
        let end = rest.iter().position(|&b| b == b'\n');
        let (line, next) = match end {
            Some(end) => (
                rest.get(..end).unwrap_or_default(),
                rest.get(end + 1..).unwrap_or_default(),
            ),
            None => (rest, &[][..]),
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let quotes = line.iter().take_while(|&&b| b == b'>').count();
        if line
            .get(quotes..)
            .is_some_and(|tail| tail.starts_with(b"From "))
        {
            out.push(b'>');
        }
        out.extend_from_slice(line);
        out.push(b'\n');
        rest = next;
    }
    out.push(b'\n');
}
