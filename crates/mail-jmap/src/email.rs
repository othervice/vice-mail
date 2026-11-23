//! JMAP `Email` objects (RFC 8621 section 4) as the engine's header data, flags and MIME
//! structure. Nothing here may panic on server data; missing values become defaults.

use mail_model::{
    Address, Disposition, Flag, Flags, MessageHeader, MessageStructure, MimePart, ModSeq, PartPath,
    Uid,
};
use mail_parser::DateTime;
use serde_json::{Map, Value, json};

/// Properties of `Email/get` for header data.
pub(crate) const HEADER_PROPERTIES: &[&str] = &[
    "id",
    "size",
    "receivedAt",
    "sentAt",
    "messageId",
    "inReplyTo",
    "references",
    "from",
    "to",
    "cc",
    "subject",
    "keywords",
    "hasAttachment",
    "headers",
];

/// The fields the category rules read (`mail_content::CLASSIFY_HEADER_FIELDS`, decision 0016;
/// a test keeps both lists equal) and `List-Unsubscribe-Post` (RFC 8058), as the IMAP adapter
/// fetches them.
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

/// Longest raw header block kept for the category rules; a larger one is cut at a line end.
const MAX_CATEGORY_BLOCK: usize = 16 * 1024;

/// Keywords the server derives itself (Stalwart: whether a message has attachments). They are
/// no user data and the engine does not see them.
const DERIVED_KEYWORDS: &[&str] = &["$hasattachment", "$hasnoattachment"];

/// JMAP keywords (`{"$seen": true}`) as IMAP-style flags.
pub(crate) fn flags_of(keywords: Option<&Value>) -> Flags {
    let mut flags = Flags::default();
    let Some(map) = keywords.and_then(Value::as_object) else {
        return flags;
    };
    for (keyword, on) in map {
        if on.as_bool() != Some(true) {
            continue;
        }
        match keyword.to_ascii_lowercase().as_str() {
            "$seen" => flags.seen = true,
            "$answered" => flags.answered = true,
            "$flagged" => flags.flagged = true,
            "$draft" => flags.draft = true,
            "$deleted" => flags.deleted = true,
            lower if DERIVED_KEYWORDS.contains(&lower) => {}
            _ => flags.keywords.push(keyword.clone()),
        }
    }
    flags.normalize_keywords();
    flags
}

/// The JMAP keyword of a flag; `None` for Gmail labels (not on JMAP).
pub(crate) fn keyword_of(flag: &Flag) -> Option<String> {
    match flag {
        Flag::Seen => Some("$seen".into()),
        Flag::Answered => Some("$answered".into()),
        Flag::Flagged => Some("$flagged".into()),
        Flag::Draft => Some("$draft".into()),
        Flag::Deleted => Some("$deleted".into()),
        // JMAP keywords are case-insensitive; servers keep them lower case.
        Flag::Keyword(name) => Some(name.to_ascii_lowercase()),
        Flag::Label(_) => None,
    }
}

/// The keywords of `Email/import` for these flags.
pub(crate) fn keywords_object(flags: &[Flag]) -> Value {
    let mut map = Map::new();
    for keyword in flags.iter().filter_map(keyword_of) {
        map.insert(keyword, Value::Bool(true));
    }
    Value::Object(map)
}

/// RFC 3339 date as Unix seconds.
pub(crate) fn parse_date(text: &str) -> Option<i64> {
    DateTime::parse_rfc3339(text.trim())
        .filter(DateTime::is_valid)
        .map(|date| date.to_timestamp())
}

/// A Unix day as a JMAP `UTCDate` at midnight (search: `after`, `before`).
pub(crate) fn utc_midnight(year: i32, month: u8, day: u8) -> String {
    format!("{year:04}-{month:02}-{day:02}T00:00:00Z")
}

fn clean_text(text: &str) -> String {
    let text = text.trim();
    if text.chars().any(mail_model::is_bidi_control) {
        mail_model::strip_bidi_controls(text).trim().to_owned()
    } else {
        text.to_owned()
    }
}

fn addresses(value: Option<&Value>) -> Vec<Address> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|a| {
                    let email = a
                        .get("email")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    let name = a
                        .get("name")
                        .and_then(Value::as_str)
                        .map(clean_text)
                        .filter(|n| !n.is_empty());
                    (!email.is_empty() || name.is_some()).then_some(Address { name, email })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ids(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(|id| {
                    id.trim()
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .trim()
                })
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// RFC 5322 unfolding of a raw header value, then trim.
fn unfold(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
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

/// The raw header fields (`headers` property): name and raw value, in message order.
fn header_list(email: &Value) -> Vec<(&str, &str)> {
    email
        .get("headers")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|h| {
                    Some((
                        h.get("name").and_then(Value::as_str)?,
                        h.get("value").and_then(Value::as_str)?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The raw header block of these fields, as the IMAP adapter's `HEADER.FIELDS` fetch gives it.
pub(crate) fn raw_block<'a>(
    headers: impl IntoIterator<Item = (&'a str, &'a str)>,
    keep: impl Fn(&str) -> bool,
) -> Vec<u8> {
    let mut block = Vec::new();
    for (name, value) in headers {
        if keep(name) {
            block.extend_from_slice(name.as_bytes());
            block.push(b':');
            block.extend_from_slice(value.as_bytes());
            block.extend_from_slice(b"\r\n");
        }
    }
    block
}

fn category_block(email: &Value) -> Vec<u8> {
    let block = raw_block(header_list(email), |name| {
        CATEGORY_FIELDS.iter().any(|f| f.eq_ignore_ascii_case(name))
    });
    if block.len() <= MAX_CATEGORY_BLOCK {
        return block;
    }
    let head = block.get(..MAX_CATEGORY_BLOCK).unwrap_or_default();
    let end = head.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    head.get(..end).unwrap_or_default().to_vec()
}

/// Header data of one `Email/get` entry for the message with `uid`.
pub(crate) fn header(uid: Uid, email: &Value, modseq: u64) -> MessageHeader {
    let raw = |name: &str| {
        header_list(email)
            .into_iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| unfold(v))
            .filter(|v| !v.is_empty())
    };
    let received = email
        .get("receivedAt")
        .and_then(Value::as_str)
        .and_then(parse_date);
    let date = email
        .get("sentAt")
        .and_then(Value::as_str)
        .and_then(parse_date);
    MessageHeader {
        uid,
        modseq: Some(ModSeq(modseq)),
        flags: flags_of(email.get("keywords")),
        size: email
            .get("size")
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX)),
        internal_date: received.or(date).unwrap_or(0),
        date,
        subject: email
            .get("subject")
            .and_then(Value::as_str)
            .map(clean_text)
            .unwrap_or_default(),
        from: addresses(email.get("from")),
        to: addresses(email.get("to")),
        cc: addresses(email.get("cc")),
        message_id: ids(email.get("messageId")).into_iter().next(),
        in_reply_to: ids(email.get("inReplyTo")).into_iter().next(),
        references: ids(email.get("references")),
        list_id: raw("List-Id"),
        list_unsubscribe: raw("List-Unsubscribe"),
        auto_submitted: raw("Auto-Submitted"),
        precedence: raw("Precedence"),
        has_attachments: email
            .get("hasAttachment")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        category_headers: category_block(email),
        gmail: None,
    }
}

/// The header of the whole message (`BODY.PEEK[HEADER]`), rebuilt from the raw fields, with the
/// empty line that ends it.
pub(crate) fn header_section(email: &Value) -> Vec<u8> {
    let mut block = raw_block(header_list(email), |_| true);
    block.extend_from_slice(b"\r\n");
    block
}

/// Body part properties of `Email/get` for the structure.
pub(crate) const PART_PROPERTIES: &[&str] = &[
    "partId",
    "blobId",
    "size",
    "headers",
    "type",
    "charset",
    "disposition",
    "cid",
    "name",
    "subParts",
];

/// One leaf part with the IMAP part number the adapter gave it, for `fetch_sections`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeafPart {
    pub path: PartPath,
    pub blob_id: String,
    /// The part's MIME header, `Content-Transfer-Encoding` replaced by `binary`: the blob is
    /// the decoded content (RFC 8621 section 4.1.4).
    pub mime_header: Vec<u8>,
    pub mime_type: String,
}

/// The MIME structure of `bodyStructure` with IMAP part numbers (RFC 3501 section 6.4.5): the
/// children of a multipart are numbered from 1; a single-part message's body is part 1.
/// Sizes are those of the decoded parts.
pub(crate) fn structure(size: u32, body: &Value) -> (MessageStructure, Vec<LeafPart>) {
    let mut parts = Vec::new();
    let mut leaves = Vec::new();
    // (part, path, depth); a stack, no recursion (deeply nested messages exist).
    let mut stack: Vec<(&Value, Option<PartPath>, u16)> = Vec::new();
    let root_path = if is_multipart(body) {
        None
    } else {
        PartPath::new(vec![1])
    };
    stack.push((body, root_path, 0));
    while let Some((part, path, depth)) = stack.pop() {
        let mime_type = part
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("text/plain")
            .to_ascii_lowercase();
        let multipart = mime_type.starts_with("multipart/");
        let disposition = part.get("disposition").and_then(Value::as_str).map(|d| {
            if d.eq_ignore_ascii_case("inline") {
                Disposition::Inline
            } else {
                Disposition::Attachment
            }
        });
        let size = part
            .get("size")
            .and_then(Value::as_u64)
            .map_or(0, |n| u32::try_from(n).unwrap_or(u32::MAX));
        parts.push(MimePart {
            path: path.clone(),
            depth,
            mime_type: mime_type.clone(),
            charset: part
                .get("charset")
                .and_then(Value::as_str)
                .map(str::to_owned),
            file_name: part.get("name").and_then(Value::as_str).map(str::to_owned),
            disposition,
            content_id: part
                .get("cid")
                .and_then(Value::as_str)
                .map(|c| {
                    c.trim()
                        .trim_start_matches('<')
                        .trim_end_matches('>')
                        .to_owned()
                })
                .filter(|c| !c.is_empty()),
            encoding: if multipart {
                String::new()
            } else {
                "binary".into()
            },
            size: if multipart { 0 } else { size },
        });
        if multipart {
            let children = part
                .get("subParts")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            // Pushed in reverse, so the stack yields them in order (pre-order).
            for (i, child) in children.iter().enumerate().rev() {
                let n = u32::try_from(i + 1).unwrap_or(u32::MAX);
                let child_path = match &path {
                    Some(p) => p.child(n),
                    None => PartPath::new(vec![n]),
                };
                stack.push((child, child_path, depth.saturating_add(1)));
            }
        } else if let (Some(path), Some(blob_id)) =
            (path, part.get("blobId").and_then(Value::as_str))
        {
            let headers: Vec<(&str, &str)> = part
                .get("headers")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|h| {
                            Some((
                                h.get("name").and_then(Value::as_str)?,
                                h.get("value").and_then(Value::as_str)?,
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut mime_header = raw_block(headers, |name| {
                !name.eq_ignore_ascii_case("Content-Transfer-Encoding")
            });
            if !mime_header
                .windows(13)
                .any(|w| w.eq_ignore_ascii_case(b"content-type:"))
            {
                mime_header.extend_from_slice(format!("Content-Type: {mime_type}\r\n").as_bytes());
            }
            mime_header.extend_from_slice(b"Content-Transfer-Encoding: binary\r\n\r\n");
            leaves.push(LeafPart {
                path,
                blob_id: blob_id.to_owned(),
                mime_header,
                mime_type,
            });
        }
    }
    (MessageStructure { size, parts }, leaves)
}

fn is_multipart(part: &Value) -> bool {
    part.get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("multipart/"))
}

/// An `Email/set` patch that sets or clears keywords.
pub(crate) fn keyword_patch(keywords: &[String], on: bool) -> Value {
    let mut patch = Map::new();
    for keyword in keywords {
        patch.insert(
            format!("keywords/{}", escape_pointer(keyword)),
            if on { Value::Bool(true) } else { Value::Null },
        );
    }
    Value::Object(patch)
}

/// An `Email/set` patch that moves a message from `from` to `to`.
pub(crate) fn move_patch(from: &str, to: &str) -> Value {
    json!({
        format!("mailboxIds/{}", escape_pointer(from)): Value::Null,
        format!("mailboxIds/{}", escape_pointer(to)): true,
    })
}

/// JSON pointer escaping of one path segment (RFC 6901), as JMAP patches use it.
pub(crate) fn escape_pointer(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `Email/get` entry from Stalwart 1.1.1 (the seeded alice, abridged).
    fn stalwart_email() -> Value {
        json!({
            "id": "uaaaaafa", "blobId": "cdgy", "size": 1979,
            "receivedAt": "2025-06-02T11:18:18Z", "sentAt": "2025-06-02T06:18:18-05:00",
            "messageId": ["filler.00036@seed.vicemail.test"], "inReplyTo": null, "references": null,
            "from": [{"name": "Rust Users", "email": "rust-users@lists.example"}],
            "to": [{"name": "Alice Archer", "email": "alice@vicemail.test"}], "cc": null,
            "subject": "Rust Users: Question about the hiring plan",
            "keywords": {"$hasnoattachment": true, "$seen": true, "projekt-x": true},
            "hasAttachment": false, "mailboxIds": {"i": true},
            "headers": [
                {"name": "Date", "value": " Mon, 02 Jun 2025 06:18:18 -0500"},
                {"name": "From", "value": " Rust Users <rust-users@lists.example>"},
                {"name": "To", "value": " Alice Archer <alice@vicemail.test>"},
                {"name": "Subject", "value": " Rust Users: Question about the hiring plan"},
                {"name": "Message-ID", "value": " <filler.00036@seed.vicemail.test>"},
                {"name": "List-Id", "value": " Rust users discussion\r\n <rust-users.lists.example>"},
                {"name": "List-Unsubscribe", "value": " <https://lists.example/u/36>"},
                {"name": "Precedence", "value": " bulk"},
                {"name": "X-Vicemail-Seed", "value": " filler/00036"}
            ]
        })
    }

    #[test]
    fn stalwart_email_becomes_header_data() {
        let header = header(7, &stalwart_email(), 3);
        assert_eq!(header.uid, 7);
        assert_eq!(header.modseq, Some(ModSeq(3)));
        assert_eq!(header.size, 1979);
        assert_eq!(header.internal_date, 1_748_863_098);
        assert_eq!(header.date, Some(1_748_863_098));
        assert_eq!(header.subject, "Rust Users: Question about the hiring plan");
        assert_eq!(header.from[0].email, "rust-users@lists.example");
        assert_eq!(header.from[0].name.as_deref(), Some("Rust Users"));
        assert!(header.cc.is_empty());
        assert_eq!(
            header.message_id.as_deref(),
            Some("filler.00036@seed.vicemail.test")
        );
        assert_eq!(
            header.list_id.as_deref(),
            Some("Rust users discussion <rust-users.lists.example>")
        );
        assert_eq!(header.precedence.as_deref(), Some("bulk"));
        assert!(header.flags.seen && !header.flags.flagged);
        assert_eq!(
            header.flags.keywords,
            ["projekt-x"],
            "derived keywords dropped"
        );
        let block = String::from_utf8(header.category_headers).expect("utf-8");
        assert!(block.starts_with("From: Rust Users"), "{block}");
        assert!(block.contains("Precedence: bulk\r\n"));
        assert!(!block.contains("X-Vicemail-Seed") && !block.contains("Date:"));
    }

    #[test]
    fn broken_values_never_panic_and_bidi_controls_go() {
        let email = json!({
            "subject": "Invoice\u{202E}fdp.exe", "from": [{"name": "\u{202E}x", "email": " a@b "}, {}],
            "sentAt": "yesterday", "size": -1, "keywords": {"$seen": false, "$Flagged": true},
            "messageId": ["", "<id@x>"], "headers": "nope"
        });
        let header = header(1, &email, 1);
        assert_eq!(header.subject, "Invoicefdp.exe");
        assert_eq!(header.from.len(), 1);
        assert_eq!(header.from[0].email, "a@b");
        assert_eq!(header.from[0].name.as_deref(), Some("x"));
        assert_eq!(header.date, None);
        assert_eq!(header.internal_date, 0);
        assert!(!header.flags.seen && header.flags.flagged);
        assert_eq!(header.message_id.as_deref(), Some("id@x"));
        assert!(header.category_headers.is_empty());
        let header = super::header(2, &json!(null), 1);
        assert_eq!(header.uid, 2);
    }

    #[test]
    fn category_fields_are_the_classifier_s() {
        let ours: Vec<&str> = CATEGORY_FIELDS
            .iter()
            .copied()
            .filter(|f| *f != "List-Unsubscribe-Post")
            .collect();
        assert_eq!(ours, mail_content::CLASSIFY_HEADER_FIELDS);
    }

    #[test]
    fn flags_and_keywords_round_trip() {
        let flags = [
            Flag::Seen,
            Flag::Flagged,
            Flag::Keyword("$Forwarded".into()),
            Flag::Label("x".into()),
        ];
        assert_eq!(
            keywords_object(&flags),
            json!({"$seen": true, "$flagged": true, "$forwarded": true})
        );
        assert_eq!(
            keyword_patch(&["$seen".into(), "a/b".into()], false),
            json!({"keywords/$seen": null, "keywords/a~1b": null})
        );
        assert_eq!(
            move_patch("a", "b"),
            json!({"mailboxIds/a": null, "mailboxIds/b": true})
        );
        let back = flags_of(Some(
            &json!({"$seen": true, "$forwarded": true, "$answered": true}),
        ));
        assert!(back.seen && back.answered && back.has_keyword("$Forwarded"));
        assert_eq!(utc_midnight(2026, 1, 2), "2026-01-02T00:00:00Z");
    }

    #[test]
    fn structure_gets_imap_part_numbers() {
        let body = json!({
            "type": "multipart/mixed", "subParts": [
                {"type": "multipart/alternative", "subParts": [
                    {"partId": "1", "blobId": "b1", "size": 10, "type": "text/plain", "charset": "utf-8",
                     "headers": [{"name": "Content-Type", "value": " text/plain; charset=utf-8"},
                                 {"name": "Content-Transfer-Encoding", "value": " quoted-printable"}]},
                    {"partId": "2", "blobId": "b2", "size": 20, "type": "text/html"}
                ]},
                {"partId": "3", "blobId": "b3", "size": 3_000_000, "type": "application/pdf",
                 "disposition": "attachment", "name": "a.pdf"},
                {"partId": "4", "blobId": "b4", "size": 5, "type": "image/png", "cid": "<logo@x>",
                 "disposition": "inline"}
            ]
        });
        let (structure, leaves) = structure(3_100_000, &body);
        let paths: Vec<String> = structure
            .parts
            .iter()
            .map(|p| p.path.as_ref().map(ToString::to_string).unwrap_or_default())
            .collect();
        assert_eq!(paths, ["", "1", "1.1", "1.2", "2", "3"]);
        assert_eq!(
            structure.parts[4].disposition,
            Some(Disposition::Attachment)
        );
        assert_eq!(structure.parts[5].content_id.as_deref(), Some("logo@x"));
        assert_eq!(leaves.len(), 4);
        let text = String::from_utf8(leaves[0].mime_header.clone()).expect("utf-8");
        assert_eq!(
            text,
            "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: binary\r\n\r\n"
        );
        let html = String::from_utf8(leaves[1].mime_header.clone()).expect("utf-8");
        assert!(html.starts_with("Content-Type: text/html\r\n"), "{html}");
        match structure.fetch_plan(2 * 1024 * 1024) {
            mail_model::BodyFetchPlan::Sections(sections) => {
                assert!(sections.len() >= 5, "{sections:?}");
            }
            other @ mail_model::BodyFetchPlan::Whole => panic!("{other:?}"),
        }

        let (single, leaves) =
            super::structure(10, &json!({"type": "text/plain", "blobId": "x", "size": 4}));
        assert_eq!(
            single.parts[0]
                .path
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("1")
        );
        assert_eq!(leaves.len(), 1);
    }
}
