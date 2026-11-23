//! Turns parsed IMAP responses (imap-proto types) into model types, right where they are read.
//!
//! Nothing here may panic on server data: broken messages (the seed has 26) and odd servers
//! must yield best-effort values. Header text is decoded with mail-parser (RFC 2047 encoded
//! words, charsets, dates), which Stalwart Labs fuzzes.

use std::collections::HashMap;
use std::sync::LazyLock;

use async_imap::imap_proto::{
    self, AttributeValue, BodyContentCommon, BodyParams, BodyStructure, Capability, Envelope,
    MessageSection, NameAttribute, Response, SectionPath,
};
use mail_model::{Address, FlagUpdate, Flags, GmailInfo, MessageHeader, ModSeq, Uid, UidSet};
use mail_parser::{DateTime, HeaderForm, MessageParser};

/// Header fields fetched with every message besides the ENVELOPE (stage 2 task list).
pub(crate) const HEADER_FIELDS: &str =
    "MESSAGE-ID REFERENCES IN-REPLY-TO LIST-ID LIST-UNSUBSCRIBE AUTO-SUBMITTED PRECEDENCE";

/// Stage 9: the fields the category rules read (`mail_content::CLASSIFY_HEADER_FIELDS`,
/// decision 0016; a test keeps both lists equal) and `List-Unsubscribe-Post` (RFC 8058). The
/// raw block goes to [`MessageHeader::category_headers`].
pub(crate) const CATEGORY_FIELDS: &[&str] = &[
    "From",
    "Sender",
    "Reply-To",
    "Subject",
    "Return-Path",
    "Content-Type",
    "List-Id",
    "List-Unsubscribe",
    "List-Post",
    "Auto-Submitted",
    "Precedence",
    "X-Autoreply",
    "X-Autorespond",
    "X-Mailer",
    "Feedback-ID",
    "X-CSA-Complaints",
    "X-Report-Abuse",
    "X-Campaign",
    "X-CampaignID",
    "X-Campaign-Id",
    "X-Newsletter",
    "X-MC-User",
    "X-Mailjet-Campaign",
    "X-Mailin-Campaign",
    "X-SES-Outgoing",
    "X-SG-EID",
    "X-Mailgun-Sid",
    "X-PM-Message-Id",
    "X-Mandrill-User",
    "X-MSFBL",
    "X-MJ-Mid",
    "X-SFMC-Stack",
    "X-GitHub-Reason",
    "X-GitLab-Project",
    "X-GitLab-NotificationReason",
    "X-Gitea-Reason",
    "X-Forgejo-Reason",
    "X-JIRA-FingerPrint",
    "X-Bugzilla-Reason",
    "X-Redmine-Project",
    "X-Phabricator-Sent-This-Message",
    "X-Gerrit-MessageType",
    "X-Jenkins-Job",
    "X-Discourse-Post-Id",
    "X-Launchpad-Notification-Type",
    "X-Facebook-Notify",
    "List-Unsubscribe-Post",
];

/// FETCH items of [`mail_model::MailBackend::fetch_headers`] (without MODSEQ): one
/// `HEADER.FIELDS` block with the stage 2 fields and the category fields.
pub(crate) fn header_items() -> String {
    let mut fields = HEADER_FIELDS.to_owned();
    for field in CATEGORY_FIELDS {
        if !HEADER_FIELDS
            .split(' ')
            .any(|f| f.eq_ignore_ascii_case(field))
        {
            fields.push(' ');
            fields.push_str(&field.to_ascii_uppercase());
        }
    }
    format!(
        "UID FLAGS RFC822.SIZE INTERNALDATE ENVELOPE BODYSTRUCTURE BODY.PEEK[HEADER.FIELDS ({fields})]"
    )
}

/// Stage 14 (decision 0034): the FETCH items Gmail (`X-GM-EXT-1`) adds to header and flag
/// fetches. Both are plain data items, allowed on a read-only (EXAMINE) connection.
pub(crate) const GMAIL_HEADER_ITEMS: &str = "X-GM-MSGID X-GM-LABELS";
pub(crate) const GMAIL_FLAG_ITEMS: &str = "X-GM-LABELS";

/// `X-GM-LABELS` decoded: modified UTF-7 like mailbox names (Gmail's documentation), system
/// labels in Gmail's spelling, sorted and without duplicates. A name that is not valid
/// modified UTF-7 is kept as sent rather than dropped.
pub(crate) fn gmail_labels(labels: &[std::borrow::Cow<'_, str>]) -> Vec<String> {
    mail_model::normalize_labels(labels.iter().map(|label| {
        if label.starts_with('\\') {
            label.to_string()
        } else {
            crate::mutf7::decode(label).unwrap_or_else(|_| label.to_string())
        }
    }))
}

/// Longest raw header block kept for the category rules; a larger one is cut at a line end.
const MAX_CATEGORY_BLOCK: usize = 16 * 1024;

// ---------------------------------------------------------------------------------------------
// FETCH: header data

/// What one or more FETCH responses said about one message. Servers may split the data of one
/// message over several responses; the parts are merged by UID.
#[derive(Debug, Default)]
struct Partial {
    uid: Uid,
    modseq: Option<u64>,
    flags: Option<Flags>,
    size: Option<u32>,
    internal_date: Option<i64>,
    envelope: Option<EnvelopeData>,
    has_attachments: Option<bool>,
    fields: Option<HeaderFields>,
    /// The raw `HEADER.FIELDS` block (stage 9).
    raw_fields: Option<Vec<u8>>,
    /// Stage 14: `X-GM-MSGID` and `X-GM-LABELS` (Gmail only).
    gmail_msgid: Option<u64>,
    gmail_labels: Option<Vec<String>>,
}

#[derive(Debug, Default)]
struct EnvelopeData {
    date: Option<i64>,
    subject: String,
    from: Vec<Address>,
    to: Vec<Address>,
    cc: Vec<Address>,
    message_id: Option<String>,
    in_reply_to: Option<String>,
}

/// Values from the `BODY[HEADER.FIELDS (...)]` block.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct HeaderFields {
    pub(crate) message_id: Option<String>,
    pub(crate) in_reply_to: Option<String>,
    pub(crate) references: Vec<String>,
    pub(crate) list_id: Option<String>,
    pub(crate) list_unsubscribe: Option<String>,
    pub(crate) auto_submitted: Option<String>,
    pub(crate) precedence: Option<String>,
}

/// Collects the FETCH responses of one `fetch_headers` call.
pub(crate) struct HeaderCollector<'a> {
    wanted: &'a UidSet,
    index: HashMap<Uid, usize>,
    partials: Vec<Partial>,
    /// A FETCH that is not part of the answer (another client changed flags).
    pub(crate) unsolicited: bool,
}

impl<'a> HeaderCollector<'a> {
    pub(crate) fn new(wanted: &'a UidSet) -> Self {
        Self {
            wanted,
            index: HashMap::new(),
            partials: Vec::new(),
            unsolicited: false,
        }
    }

    /// Takes a FETCH response for a requested UID; returns `false` for everything else.
    pub(crate) fn take(&mut self, resp: &Response<'_>) -> bool {
        let Response::Fetch(_, attrs) = resp else {
            return false;
        };
        let Some(uid) = uid_of(attrs).filter(|uid| self.wanted.contains(*uid)) else {
            return false;
        };
        let slot = *self.index.entry(uid).or_insert_with(|| {
            self.partials.push(Partial {
                uid,
                ..Partial::default()
            });
            self.partials.len() - 1
        });
        let Some(partial) = self.partials.get_mut(slot) else {
            return false;
        };
        for attr in attrs {
            match attr {
                AttributeValue::ModSeq(m) => partial.modseq = Some(*m),
                AttributeValue::Flags(flags) => partial.flags = Some(Flags::from_imap(flags)),
                AttributeValue::Rfc822Size(size) => partial.size = Some(*size),
                AttributeValue::InternalDate(date) => {
                    partial.internal_date = parse_internal_date(date);
                }
                AttributeValue::Envelope(env) => partial.envelope = Some(envelope(env)),
                AttributeValue::BodyStructure(body) => {
                    partial.has_attachments = Some(has_attachments(body));
                }
                AttributeValue::BodySection {
                    section: Some(SectionPath::Full(MessageSection::Header)),
                    data,
                    ..
                } => {
                    let block = data.as_deref().unwrap_or_default();
                    partial.fields = Some(header_fields(block));
                    partial.raw_fields = Some(category_block(block));
                }
                AttributeValue::GmailMsgId(id) => partial.gmail_msgid = Some(*id),
                AttributeValue::GmailLabels(labels) => {
                    partial.gmail_labels = Some(gmail_labels(labels));
                }
                _ => {}
            }
        }
        true
    }

    /// The header data, and whether a requested message got only flags (it changed or
    /// vanished while being fetched; the next sync pass looks again).
    pub(crate) fn finish(self) -> (Vec<MessageHeader>, bool) {
        let mut flags_only = self.unsolicited;
        let mut out = Vec::with_capacity(self.partials.len());
        for p in self.partials {
            let Some(env) = p.envelope else {
                flags_only = true;
                continue;
            };
            let fields = p.fields.unwrap_or_default();
            out.push(MessageHeader {
                uid: p.uid,
                modseq: p.modseq.map(ModSeq),
                flags: p.flags.unwrap_or_default(),
                size: p.size.unwrap_or(0),
                internal_date: p.internal_date.or(env.date).unwrap_or(0),
                date: env.date,
                subject: env.subject,
                from: env.from,
                to: env.to,
                cc: env.cc,
                message_id: fields.message_id.or(env.message_id),
                in_reply_to: fields.in_reply_to.or(env.in_reply_to),
                references: fields.references,
                list_id: fields.list_id,
                list_unsubscribe: fields.list_unsubscribe,
                auto_submitted: fields.auto_submitted,
                precedence: fields.precedence,
                has_attachments: p.has_attachments.unwrap_or(false),
                category_headers: p.raw_fields.unwrap_or_default(),
                gmail: p.gmail_msgid.map(|msgid| GmailInfo {
                    msgid,
                    labels: p.gmail_labels.unwrap_or_default(),
                }),
            });
        }
        (out, flags_only)
    }
}

/// Collects the FETCH responses of one `fetch_flags` call (last response per UID wins).
pub(crate) struct FlagCollector<'a> {
    wanted: &'a UidSet,
    index: HashMap<Uid, usize>,
    pub(crate) updates: Vec<FlagUpdate>,
}

impl<'a> FlagCollector<'a> {
    pub(crate) fn new(wanted: &'a UidSet) -> Self {
        Self {
            wanted,
            index: HashMap::new(),
            updates: Vec::new(),
        }
    }

    pub(crate) fn take(&mut self, resp: &Response<'_>) -> bool {
        let Response::Fetch(_, attrs) = resp else {
            return false;
        };
        let Some(uid) = uid_of(attrs).filter(|uid| self.wanted.contains(*uid)) else {
            return false;
        };
        let mut flags = None;
        let mut modseq = None;
        let mut gmail_labels = None;
        for attr in attrs {
            match attr {
                AttributeValue::Flags(f) => flags = Some(Flags::from_imap(f)),
                AttributeValue::ModSeq(m) => modseq = Some(ModSeq(*m)),
                AttributeValue::GmailLabels(labels) => {
                    gmail_labels = Some(self::gmail_labels(labels));
                }
                _ => {}
            }
        }
        let Some(flags) = flags else {
            return false;
        };
        let update = FlagUpdate {
            uid,
            flags,
            modseq,
            gmail_labels,
        };
        match self.index.get(&uid).and_then(|i| self.updates.get_mut(*i)) {
            Some(existing) => *existing = update,
            None => {
                self.index.insert(uid, self.updates.len());
                self.updates.push(update);
            }
        }
        true
    }
}

pub(crate) fn uid_of(attrs: &[AttributeValue<'_>]) -> Option<Uid> {
    attrs.iter().find_map(|attr| match attr {
        AttributeValue::Uid(uid) => Some(*uid),
        _ => None,
    })
}

fn envelope(env: &Envelope<'_>) -> EnvelopeData {
    EnvelopeData {
        date: env.date.as_deref().and_then(parse_date),
        subject: env.subject.as_deref().map(decode_text).unwrap_or_default(),
        from: addresses(env.from.as_deref()),
        to: addresses(env.to.as_deref()),
        cc: addresses(env.cc.as_deref()),
        message_id: env.message_id.as_deref().and_then(last_message_id),
        in_reply_to: env.in_reply_to.as_deref().and_then(first_message_id),
    }
}

/// Decodes unstructured header text: RFC 2047 encoded words in any charset, raw UTF-8.
pub(crate) fn decode_text(raw: &[u8]) -> String {
    // A header value never contains line breaks after unfolding; a literal might.
    let cleaned: Vec<u8> = raw
        .iter()
        .map(|&b| {
            if matches!(b, b'\r' | b'\n' | 0) {
                b' '
            } else {
                b
            }
        })
        .collect();
    let has_encoded_word = cleaned.windows(2).any(|w| w == b"=?");
    if !has_encoded_word && let Ok(text) = std::str::from_utf8(&cleaned) {
        return text.trim().to_owned();
    }
    let parsed = HeaderForm::Text.parse(&cleaned);
    match parsed.value().as_text() {
        Some(text) => text.trim().to_owned(),
        None => String::from_utf8_lossy(&cleaned).trim().to_owned(),
    }
}

/// RFC 5322 date as Unix seconds; `None` for missing, unparseable or impossible dates.
pub(crate) fn parse_date(raw: &[u8]) -> Option<i64> {
    let text = String::from_utf8_lossy(raw);
    DateTime::parse_rfc822(text.trim())
        .filter(DateTime::is_valid)
        .map(|date| date.to_timestamp())
}

fn message_ids(raw: &[u8]) -> Vec<String> {
    let mut value = raw.to_vec();
    value.extend_from_slice(b"\r\n");
    let parsed = HeaderForm::MessageIds.parse(&value);
    parsed
        .value()
        .as_text_list()
        .map(|list| list.iter().filter_map(clean_message_id).collect())
        .unwrap_or_default()
}

fn first_message_id(raw: &[u8]) -> Option<String> {
    message_ids(raw).into_iter().next()
}

fn last_message_id(raw: &[u8]) -> Option<String> {
    message_ids(raw).into_iter().next_back()
}

fn clean_message_id(id: &str) -> Option<String> {
    let id = id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// Mailboxes of an ENVELOPE address list; group markers (host NIL) are dropped, so groups are
/// flattened.
fn addresses(list: Option<&[imap_proto::Address<'_>]>) -> Vec<Address> {
    let lossy = |b: &[u8]| String::from_utf8_lossy(b).trim().to_owned();
    list.unwrap_or_default()
        .iter()
        .filter(|a| a.host.is_some())
        .filter_map(|a| {
            let mailbox = a.mailbox.as_deref().map(lossy).unwrap_or_default();
            let host = a.host.as_deref().map(lossy).unwrap_or_default();
            let email = match (mailbox.is_empty(), host.is_empty()) {
                (false, false) => format!("{mailbox}@{host}"),
                (false, true) => mailbox,
                (true, _) => String::new(),
            };
            let name = a.name.as_deref().map(decode_text).filter(|n| !n.is_empty());
            (!email.is_empty() || name.is_some()).then_some(Address { name, email })
        })
        .collect()
}

/// The block as fetched, at most [`MAX_CATEGORY_BLOCK`] bytes (cut after a line end).
fn category_block(block: &[u8]) -> Vec<u8> {
    if block.len() <= MAX_CATEGORY_BLOCK {
        return block.to_vec();
    }
    let head = block.get(..MAX_CATEGORY_BLOCK).unwrap_or_default();
    let end = head.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    head.get(..end).unwrap_or_default().to_vec()
}

static FIELD_PARSER: LazyLock<MessageParser> = LazyLock::new(MessageParser::new);

/// Parses the `HEADER.FIELDS` block. List and auto-reply headers stay raw (unfolded, trimmed).
pub(crate) fn header_fields(block: &[u8]) -> HeaderFields {
    let Some(message) = FIELD_PARSER.parse_headers(block) else {
        return HeaderFields::default();
    };
    let headers = message.headers();
    let raw = |name: &str| {
        headers
            .all(name)
            .next()
            .map(|h| unfold(h.raw_value()))
            .filter(|v| !v.is_empty())
    };
    let ids = |name: &str| -> Vec<String> {
        headers
            .all(name)
            .next()
            .map(|h| message_ids(h.raw_value()))
            .unwrap_or_default()
    };
    HeaderFields {
        message_id: ids("Message-ID").into_iter().next_back(),
        in_reply_to: ids("In-Reply-To").into_iter().next(),
        references: ids("References"),
        list_id: raw("List-Id"),
        list_unsubscribe: raw("List-Unsubscribe"),
        auto_submitted: raw("Auto-Submitted"),
        precedence: raw("Precedence"),
    }
}

/// RFC 5322 unfolding (CRLF followed by white space becomes that white space), then trim.
fn unfold(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch != '\r' && ch != '\n' {
            out.push(if ch.is_control() && ch != '\t' {
                ' '
            } else {
                ch
            });
        }
    }
    out.trim().to_owned()
}

/// Whether some part is an attachment: `Content-Disposition: attachment`, or a non-text part
/// with a file name. Walks without recursion (the seed nests 40 levels deep).
pub(crate) fn has_attachments(root: &BodyStructure<'_>) -> bool {
    let mut stack = vec![root];
    while let Some(part) = stack.pop() {
        match part {
            BodyStructure::Multipart { bodies, .. } => stack.extend(bodies.iter()),
            BodyStructure::Text { common, .. } => {
                if is_disposition_attachment(common) {
                    return true;
                }
            }
            BodyStructure::Basic { common, .. } => {
                if is_disposition_attachment(common) || has_file_name(common) {
                    return true;
                }
            }
            BodyStructure::Message { common, body, .. } => {
                if is_disposition_attachment(common) || has_file_name(common) {
                    return true;
                }
                stack.push(body);
            }
        }
    }
    false
}

fn is_disposition_attachment(common: &BodyContentCommon<'_>) -> bool {
    common
        .disposition
        .as_ref()
        .is_some_and(|d| d.ty.eq_ignore_ascii_case("attachment"))
}

fn has_file_name(common: &BodyContentCommon<'_>) -> bool {
    fn named(params: &BodyParams<'_>) -> bool {
        params.as_ref().is_some_and(|params| {
            params.iter().any(|(key, value)| {
                let key = key.to_ascii_lowercase();
                let is_name = key == "name" || key.starts_with("name*");
                let is_file = key == "filename" || key.starts_with("filename*");
                (is_name || is_file) && !value.trim().is_empty()
            })
        })
    }
    named(&common.ty.params)
        || common
            .disposition
            .as_ref()
            .is_some_and(|d| named(&d.params))
}

/// IMAP INTERNALDATE (`17-Jul-1996 02:44:25 -0700`, day may be space-padded) as Unix seconds.
pub(crate) fn parse_internal_date(text: &str) -> Option<i64> {
    let mut words = text.split_whitespace();
    let (date, time, zone) = (words.next()?, words.next()?, words.next()?);
    let mut date_parts = date.split('-');
    let day: i64 = date_parts.next()?.parse().ok()?;
    let month_name = date_parts.next()?;
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month = MONTHS
        .iter()
        .position(|m| m.eq_ignore_ascii_case(month_name))
        .and_then(|i| i64::try_from(i).ok())?
        + 1;
    let mut time_parts = time.split(':');
    let hour: i64 = time_parts.next()?.parse().ok()?;
    let minute: i64 = time_parts.next()?.parse().ok()?;
    let second: i64 = time_parts.next()?.parse().ok()?;
    let (sign, digits) = match zone.as_bytes().first()? {
        b'+' => (1, zone.get(1..)?),
        b'-' => (-1, zone.get(1..)?),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let zone_hours: i64 = digits.get(..2)?.parse().ok()?;
    let zone_minutes: i64 = digits.get(2..)?.parse().ok()?;
    let valid = (1..=31).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..=60).contains(&second)
        && (0..60).contains(&zone_minutes)
        && (1..=9999).contains(&year);
    if !valid {
        return None;
    }
    let offset = sign * (zone_hours * 3600 + zone_minutes * 60);
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

// ---------------------------------------------------------------------------------------------
// CAPABILITY, LIST

/// Capability names as the server sent them (`AUTH=` entries included).
pub(crate) fn capability_names(caps: &[Capability<'_>]) -> Vec<String> {
    caps.iter()
        .map(|cap| match cap {
            Capability::Imap4rev1 => "IMAP4REV1".to_owned(),
            Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
            Capability::Atom(atom) => atom.to_string(),
        })
        .collect()
}

/// LIST attribute in wire form (`\Sent`, `\Noselect`, `\HasNoChildren`).
pub(crate) fn name_attribute(attr: &NameAttribute<'_>) -> String {
    match attr {
        NameAttribute::NoInferiors => "\\Noinferiors".to_owned(),
        NameAttribute::NoSelect => "\\Noselect".to_owned(),
        NameAttribute::Marked => "\\Marked".to_owned(),
        NameAttribute::Unmarked => "\\Unmarked".to_owned(),
        NameAttribute::All => "\\All".to_owned(),
        NameAttribute::Archive => "\\Archive".to_owned(),
        NameAttribute::Drafts => "\\Drafts".to_owned(),
        NameAttribute::Flagged => "\\Flagged".to_owned(),
        NameAttribute::Junk => "\\Junk".to_owned(),
        NameAttribute::Sent => "\\Sent".to_owned(),
        NameAttribute::Trash => "\\Trash".to_owned(),
        NameAttribute::Extension(text) => text.to_string(),
        other => format!("\\{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses one untagged response line the way async-imap's stream does.
    fn parse(line: &[u8]) -> Response<'_> {
        match Response::parse(line) {
            Ok(([], resp)) => resp,
            other => panic!("test response does not parse: {other:?}"),
        }
    }

    #[test]
    fn category_fields_are_the_classifier_s() {
        // Every field the category rules read is fetched, and nothing else besides
        // `List-Unsubscribe-Post` (decision 0016).
        let ours: Vec<&str> = CATEGORY_FIELDS
            .iter()
            .copied()
            .filter(|f| *f != "List-Unsubscribe-Post")
            .collect();
        assert_eq!(ours, mail_content::CLASSIFY_HEADER_FIELDS);
        let items = header_items();
        for field in CATEGORY_FIELDS {
            assert!(
                items.contains(&format!(" {}", field.to_ascii_uppercase())),
                "{field} is fetched"
            );
        }
        assert_eq!(items.matches("LIST-ID").count(), 1, "no field twice");
        let long = [&b"X-Mailer: a\r\n"[..]; 2_000].concat();
        let cut = category_block(&long);
        assert!(cut.len() <= MAX_CATEGORY_BLOCK && cut.ends_with(b"\n"));
    }

    #[test]
    fn internal_dates() {
        assert_eq!(
            parse_internal_date("17-Jul-1996 02:44:25 -0700"),
            Some(837_596_665)
        );
        assert_eq!(parse_internal_date(" 1-Jan-1970 00:00:00 +0000"), Some(0));
        assert_eq!(
            parse_internal_date("29-Feb-2024 23:59:59 +0100"),
            Some(1_709_247_599)
        );
        assert_eq!(
            parse_internal_date("01-Sep-2026 12:00:00 +0000"),
            Some(1_788_264_000)
        );
        for bad in [
            "",
            "32-Jan-2020 00:00:00 +0000",
            "01-Foo-2020 00:00:00 +0000",
            "01-Jan-2020 24:00:00 +0000",
            "01-Jan-2020 00:00:00 0000",
            "01-Jan-2020 00:00:00 +00",
            "01-Jan-2020 00:00",
        ] {
            assert_eq!(parse_internal_date(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn encoded_words_charsets_and_raw_utf8_are_decoded() {
        assert_eq!(
            decode_text(b"=?utf-8?q?caf=C3=A9?= au lait"),
            "café au lait"
        );
        assert_eq!(decode_text(b"=?ISO-8859-1?Q?Gr=FC=DFe?="), "Grüße");
        assert_eq!(decode_text(b"=?KOI8-R?B?8NLJ18XU?="), "Привет");
        assert_eq!(decode_text(b"=?ISO-2022-JP?B?GyRCRnxLXDhsGyhC?="), "日本語");
        assert_eq!(decode_text("Grüße roh".as_bytes()), "Grüße roh");
        assert_eq!(decode_text(b"  plain  "), "plain");
        assert_eq!(decode_text(b"line\r\nbreak"), "line  break");
        // Broken words never panic and keep something readable.
        let broken = decode_text(b"=?utf-8?Q?unterminated =?unknown-cs?B?Zm9v?= =?utf-8?b?8J+R?=");
        assert!(!broken.is_empty());
        let latin1 = decode_text(b"Stra\xdfe");
        assert!(latin1.starts_with("Stra"), "{latin1}");
    }

    #[test]
    fn dates_from_the_envelope() {
        assert_eq!(
            parse_date(b"Sat, 20 Nov 2021 14:22:01 -0800"),
            Some(1_637_446_921)
        );
        assert_eq!(parse_date(b"Mon, 32 Foo 2026 25:61:99 +9999"), None);
        assert_eq!(parse_date(b""), None);
        assert_eq!(parse_date(b"garbage"), None);
        assert_eq!(
            parse_date(b"Tue, 1 Sep 26 12:00:00 -0500"),
            Some(1_788_282_000)
        );
        // Obsolete RFC 822 forms (no seconds, named zones) are not parsed; the sort date then
        // falls back to INTERNALDATE.
        assert_eq!(parse_date(b"1 Sep 26 12:00 EST"), None);
    }

    #[test]
    fn header_field_block() {
        let block = b"Message-ID: <a@b.example>\r\nReferences: <r1@x> <r2@x>\r\n\t<r3@x>\r\n\
In-Reply-To: <r3@x> <other@x>\r\nList-Id: Rust users\r\n discussion <rust-users.lists.example>\r\n\
List-Unsubscribe: <mailto:leave@lists.example>\r\nAuto-Submitted: auto-replied\r\n\
Precedence: bulk\r\n\r\n";
        let fields = header_fields(block);
        assert_eq!(fields.message_id.as_deref(), Some("a@b.example"));
        assert_eq!(fields.references, ["r1@x", "r2@x", "r3@x"]);
        assert_eq!(fields.in_reply_to.as_deref(), Some("r3@x"));
        assert_eq!(
            fields.list_id.as_deref(),
            Some("Rust users discussion <rust-users.lists.example>")
        );
        assert_eq!(
            fields.list_unsubscribe.as_deref(),
            Some("<mailto:leave@lists.example>")
        );
        assert_eq!(fields.auto_submitted.as_deref(), Some("auto-replied"));
        assert_eq!(fields.precedence.as_deref(), Some("bulk"));

        assert_eq!(header_fields(b"\r\n"), HeaderFields::default());
        assert_eq!(header_fields(b""), HeaderFields::default());
        let odd = header_fields(b"Message-ID: no-angle-brackets@seed\r\nMessage-ID:\r\n\r\n");
        assert!(odd.references.is_empty());
        let empty = header_fields(b"Message-ID: <>\r\nList-Id:   \r\n\r\n");
        assert_eq!(empty.message_id, None);
        assert_eq!(empty.list_id, None);
    }

    #[test]
    fn fetch_response_becomes_a_message_header() {
        let line = b"* 12 FETCH (UID 4711 MODSEQ (99) FLAGS (\\Seen $Forwarded projekt-x) RFC822.SIZE 2048 \
INTERNALDATE \"01-Sep-2026 12:00:00 +0000\" ENVELOPE (\"Tue, 01 Sep 2026 13:59:00 +0200\" \
\"=?utf-8?q?Gr=C3=BC=C3=9Fe?=\" ((\"=?utf-8?q?J=C3=BCrgen?=\" NIL \"juergen\" \"firma.example\")) NIL NIL \
((NIL NIL \"alice\" \"vicemail.test\")(\"Group\" NIL \"team\" NIL)(NIL NIL \"bob\" \"vicemail.test\")(NIL NIL NIL NIL)) \
NIL NIL \"<parent@x>\" \"<self@x>\") BODYSTRUCTURE ((\"TEXT\" \"PLAIN\" (\"CHARSET\" \"utf-8\") NIL NIL \"7BIT\" 10 1 NIL NIL NIL NIL)\
(\"APPLICATION\" \"PDF\" (\"NAME\" \"a.pdf\") NIL NIL \"BASE64\" 100 NIL (\"ATTACHMENT\" (\"FILENAME\" \"a.pdf\")) NIL NIL) \
\"MIXED\" (\"BOUNDARY\" \"b\") NIL NIL NIL) BODY[HEADER.FIELDS (MESSAGE-ID LIST-ID)] {40}\r\n\
Message-ID: <self@x>\r\nList-Id: <l.x>\r\n\r\n)\r\n";
        let wanted = UidSet::range(4700, 4800);
        let mut collector = HeaderCollector::new(&wanted);
        assert!(collector.take(&parse(line)));
        // An unrelated response is not taken.
        assert!(!collector.take(&parse(b"* 13 EXISTS\r\n")));
        // A FETCH for a UID that was not asked for is not taken either.
        assert!(!collector.take(&parse(b"* 1 FETCH (UID 3 FLAGS (\\Seen))\r\n")));
        let (headers, flags_only) = collector.finish();
        assert!(!flags_only);
        let [h] = headers.as_slice() else {
            panic!("one header: {headers:?}");
        };
        assert_eq!(h.uid, 4711);
        assert_eq!(h.modseq, Some(ModSeq(99)));
        assert!(
            h.flags.seen && h.flags.has_keyword("$forwarded") && h.flags.has_keyword("PROJEKT-X")
        );
        assert_eq!(h.size, 2048);
        assert_eq!(h.internal_date, 1_788_264_000);
        assert_eq!(h.date, Some(1_788_263_940));
        assert_eq!(h.subject, "Grüße");
        assert_eq!(h.from.len(), 1);
        assert_eq!(h.from[0].name.as_deref(), Some("Jürgen"));
        assert_eq!(h.from[0].email, "juergen@firma.example");
        let to: Vec<&str> = h.to.iter().map(|a| a.email.as_str()).collect();
        assert_eq!(to, ["alice@vicemail.test", "bob@vicemail.test"]);
        assert_eq!(h.message_id.as_deref(), Some("self@x"));
        assert_eq!(h.in_reply_to.as_deref(), Some("parent@x"));
        assert_eq!(h.list_id.as_deref(), Some("<l.x>"));
        assert!(h.has_attachments);
    }

    #[test]
    fn split_and_flags_only_responses() {
        let wanted = UidSet::from_uids([5, 6]);
        let mut collector = HeaderCollector::new(&wanted);
        assert!(collector.take(&parse(b"* 1 FETCH (UID 5 FLAGS (\\Flagged))\r\n")));
        assert!(collector.take(&parse(
            b"* 1 FETCH (UID 5 RFC822.SIZE 7 ENVELOPE (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL))\r\n"
        )));
        // UID 6 only got flags: it changed or vanished meanwhile.
        assert!(collector.take(&parse(b"* 2 FETCH (UID 6 FLAGS ())\r\n")));
        let (headers, flags_only) = collector.finish();
        assert!(flags_only);
        assert_eq!(headers.len(), 1);
        let h = &headers[0];
        assert!(h.flags.flagged);
        assert_eq!((h.size, h.internal_date, h.date), (7, 0, None));
        assert!(h.subject.is_empty() && h.from.is_empty() && h.message_id.is_none());
    }

    #[test]
    fn flag_updates() {
        let wanted = UidSet::range(1, 10);
        let mut collector = FlagCollector::new(&wanted);
        assert!(collector.take(&parse(b"* 1 FETCH (UID 2 MODSEQ (5) FLAGS (\\Seen))\r\n")));
        assert!(collector.take(&parse(
            b"* 1 FETCH (FLAGS (\\Seen \\Answered) UID 2 MODSEQ (6))\r\n"
        )));
        assert!(!collector.take(&parse(b"* 1 FETCH (FLAGS (\\Seen))\r\n")));
        assert!(!collector.take(&parse(b"* 9 FETCH (UID 11 FLAGS (\\Seen))\r\n")));
        let [update] = collector.updates.as_slice() else {
            panic!("one update");
        };
        assert_eq!(update.uid, 2);
        assert!(update.flags.answered);
        assert_eq!(update.modseq, Some(ModSeq(6)));
    }

    #[test]
    fn attachments_in_body_structures() {
        let cases: [(&[u8], bool); 5] = [
            (b"(\"TEXT\" \"PLAIN\" (\"CHARSET\" \"us-ascii\") NIL NIL \"7BIT\" 3 1)", false),
            // Text with disposition attachment (a CSV file).
            (b"(\"TEXT\" \"CSV\" NIL NIL NIL \"7BIT\" 3 1 NIL (\"attachment\" NIL) NIL NIL)", true),
            // Inline image without a name (multipart/related): not an attachment.
            (b"((\"TEXT\" \"HTML\" NIL NIL NIL \"7BIT\" 3 1)(\"IMAGE\" \"PNG\" NIL \"<cid1>\" NIL \"BASE64\" 9 NIL (\"inline\" NIL) NIL NIL) \"RELATED\")", false),
            // RFC 2231 continuation in a name parameter.
            (b"((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 3 1)(\"APPLICATION\" \"OCTET-STREAM\" (\"name*0\" \"long\" \"name*1\" \".bin\") NIL NIL \"BASE64\" 9) \"MIXED\")", true),
            // Forwarded message with an attachment inside.
            (b"((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 3 1)(\"MESSAGE\" \"RFC822\" NIL NIL NIL \"7BIT\" 50 (NIL NIL NIL NIL NIL NIL NIL NIL NIL NIL) ((\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 3 1)(\"APPLICATION\" \"PDF\" NIL NIL NIL \"BASE64\" 9 NIL (\"ATTACHMENT\" NIL) NIL NIL) \"MIXED\") 5) \"MIXED\")", true),
        ];
        for (structure, want) in cases {
            let mut line = b"* 1 FETCH (UID 1 BODYSTRUCTURE ".to_vec();
            line.extend_from_slice(structure);
            line.extend_from_slice(b")\r\n");
            let resp = parse(&line);
            let Response::Fetch(_, attrs) = &resp else {
                panic!("fetch");
            };
            let found = attrs.iter().find_map(|a| match a {
                AttributeValue::BodyStructure(b) => Some(has_attachments(b)),
                _ => None,
            });
            assert_eq!(found, Some(want), "{}", String::from_utf8_lossy(structure));
        }
    }

    #[test]
    fn deep_nesting_does_not_recurse() {
        let mut structure = b"(\"TEXT\" \"PLAIN\" NIL NIL NIL \"7BIT\" 3 1)".to_vec();
        for _ in 0..40 {
            let mut outer = b"(".to_vec();
            outer.extend_from_slice(&structure);
            outer.extend_from_slice(b" \"MIXED\")");
            structure = outer;
        }
        let mut line = b"* 1 FETCH (UID 1 BODYSTRUCTURE ".to_vec();
        line.extend_from_slice(&structure);
        line.extend_from_slice(b")\r\n");
        let resp = parse(&line);
        let Response::Fetch(_, attrs) = &resp else {
            panic!("fetch");
        };
        assert!(
            attrs
                .iter()
                .any(|a| matches!(a, AttributeValue::BodyStructure(b) if !has_attachments(b)))
        );
    }

    #[test]
    fn list_attributes_and_capabilities_in_wire_form() {
        let resp = parse(b"* LIST (\\HasNoChildren \\Sent \\Noselect) \"/\" \"Gesendet\"\r\n");
        let Response::MailboxData(imap_proto::MailboxDatum::List(list)) = &resp else {
            panic!("list");
        };
        let attrs: Vec<String> = list.name_attributes.iter().map(name_attribute).collect();
        assert_eq!(attrs, ["\\HasNoChildren", "\\Sent", "\\Noselect"]);

        let resp = parse(b"* CAPABILITY IMAP4rev1 IDLE AUTH=PLAIN condstore\r\n");
        let Response::Capabilities(caps) = &resp else {
            panic!("caps");
        };
        assert_eq!(
            capability_names(caps),
            ["IMAP4REV1", "IDLE", "AUTH=PLAIN", "condstore"]
        );
    }
}
