//! Replying and forwarding, as pure logic: who gets the answer, from which identity, the
//! subject, the References chain, and the quote of the original as editor content (HTML and
//! plain text). Forwarding takes the original's attachments along.

use mail_model::{Address, Identity, IdentityId};
use mail_parser::MessageParser;

use super::build::Attachment;
use super::html::{
    Block, Inline, Marks, fragment, html_blocks_with, quote_line, text_blocks, trim_paragraphs,
};
use crate::parse::{MessageHeaders, ParsedMessage, attachment_bytes, parse};

/// Most Message-IDs kept in `References`: the first one and the latest ones (RFC 5322 sets no
/// limit; very long chains are cut in the middle, as RFC 5537 suggests, so the header stays
/// short and the thread root stays known).
pub const MAX_REFERENCES: usize = 20;

/// Headers that name the address a message was delivered to, most precise first.
const DELIVERY_HEADERS: &[&str] = &[
    "X-Original-To",
    "Delivered-To",
    "Envelope-To",
    "X-Envelope-To",
];

/// Reply prefixes of common languages and programs (lower case): `Re`, German `AW`/`Antw`,
/// Nordic `SV`/`VS`, Polish `Odp`, Turkish `YNT`, Chinese.
const REPLY_PREFIXES: &[&str] = &[
    "re", "aw", "antw", "sv", "vs", "odp", "ynt", "atb", "ref", "rif", "res", "回复", "答复",
    "回覆",
];
/// Forward prefixes: `Fwd`/`Fw`, German `WG`, French `TR`, Spanish `RV`, Portuguese `ENC`,
/// Dutch `Doorst`, Finnish `VL`.
const FORWARD_PREFIXES: &[&str] = &["fwd", "fw", "wg", "tr", "rv", "enc", "doorst", "vl", "转发"];

/// A received message to answer or forward.
#[derive(Debug, Clone)]
pub struct Original<'a> {
    raw: &'a [u8],
    pub message: ParsedMessage,
    /// Addresses from `X-Original-To`, `Delivered-To` and `Envelope-To`, in that order.
    pub delivered_to: Vec<String>,
}

impl<'a> Original<'a> {
    /// Parses the raw message (never fails, like [`crate::parse`]).
    pub fn parse(raw: &'a [u8]) -> Self {
        let message = parse(raw);
        let mut delivered_to = Vec::new();
        if let Some(headers) = MessageParser::new().parse_headers(raw) {
            for wanted in DELIVERY_HEADERS {
                for header in headers.headers().iter() {
                    if !header.raw_name().eq_ignore_ascii_case(wanted) {
                        continue;
                    }
                    let value = String::from_utf8_lossy(header.raw_value());
                    let address = value
                        .trim()
                        .trim_start_matches('<')
                        .split(['>', ',', ' ', '\t', '\r', '\n'])
                        .next()
                        .unwrap_or_default()
                        .to_owned();
                    if address.contains('@') && delivered_to.len() < 20 {
                        delivered_to.push(address);
                    }
                }
            }
        }
        Self {
            raw,
            message,
            delivered_to,
        }
    }
}

/// Whom a reply goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyKind {
    /// The sender (or its Reply-To).
    Sender,
    /// The sender and every other recipient, except the user's own identities.
    All,
}

/// Language of the attribution line ("On … wrote:") and the forward header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteLanguage {
    English,
    German,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteOptions {
    pub language: QuoteLanguage,
    /// Offset of the user's time zone from UTC in minutes (`60` for CET), for dates in the
    /// attribution line and the forward header.
    pub utc_offset_minutes: i32,
}

/// The start of a reply or forward, for the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedDraft {
    /// The identity the original was sent to (see [`choose_identity`]); `None` without identities.
    pub identity: Option<IdentityId>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub subject: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Editor content for the HTML composer: attribution line and quote (reply) or forward
    /// header and the original (forward). The composer puts the cursor and the signature above.
    pub html: String,
    /// The same for the plain-text composer (`> ` quote).
    pub text: String,
    /// Forwarding: the original's attachments (decoded). Empty for replies.
    pub attachments: Vec<Attachment>,
}

/// A reply to `original` (see [`ReplyKind`]); `identities` are those of the account the
/// original is in.
pub fn prepare_reply(
    original: &Original<'_>,
    kind: ReplyKind,
    identities: &[Identity],
    options: QuoteOptions,
) -> PreparedDraft {
    let headers = &original.message.headers;
    let identity = choose_identity(identities, headers, &original.delivered_to);
    let (to, cc) = reply_recipients(headers, kind, identities);
    let (in_reply_to, references) = reply_references(headers);

    let attribution = attribution(headers, options);
    let mut quoted = original_blocks(&original.message);
    trim_paragraphs(&mut quoted);
    let mut blocks = vec![Block::Paragraph(vec![plain(&attribution)])];
    if !quoted.is_empty() {
        blocks.push(Block::Quote(quoted));
    }

    let mut text = attribution;
    for line in original_text(&original.message).lines() {
        text.push('\n');
        text.push_str(&quote_line(line));
    }
    PreparedDraft {
        identity: identity.map(|i| i.id),
        to,
        cc,
        subject: reply_subject(headers.subject.as_deref().unwrap_or_default()),
        in_reply_to,
        references,
        html: fragment(&blocks),
        text,
        attachments: Vec::new(),
    }
}

/// A forward of `original` with its attachments; no recipients yet.
pub fn prepare_forward(
    original: &Original<'_>,
    identities: &[Identity],
    options: QuoteOptions,
) -> PreparedDraft {
    let headers = &original.message.headers;
    let identity = choose_identity(identities, headers, &original.delivered_to);
    // The forward is linked to the original's thread, but does not answer it.
    let (_, references) = reply_references(headers);

    let fields = forward_fields(headers, options);
    let title = match options.language {
        QuoteLanguage::English => "-------- Forwarded Message --------",
        QuoteLanguage::German => "-------- Weitergeleitete Nachricht --------",
    };
    let mut header = vec![plain(title)];
    let mut text = title.to_owned();
    for (label, value) in &fields {
        header.push(Inline::Break);
        header.push(Inline::Text(
            format!("{label}:"),
            Marks {
                bold: true,
                ..Marks::default()
            },
        ));
        header.push(plain(&format!(" {value}")));
        text.push_str(&format!("\n{label}: {value}"));
    }
    let mut blocks = vec![Block::Paragraph(header)];
    let mut body = original_blocks(&original.message);
    trim_paragraphs(&mut body);
    blocks.extend(body);
    let body_text = original_text(&original.message);
    if !body_text.is_empty() {
        text.push_str("\n\n");
        text.push_str(&body_text);
    }

    let attachments = original
        .message
        .attachments
        .iter()
        .filter_map(|part| {
            attachment_bytes(original.raw, part.part_id).map(|bytes| Attachment {
                file_name: part.file_name.clone(),
                mime: part.mime.clone(),
                bytes,
            })
        })
        .collect();
    PreparedDraft {
        identity: identity.map(|i| i.id),
        to: Vec::new(),
        cc: Vec::new(),
        subject: forward_subject(headers.subject.as_deref().unwrap_or_default()),
        in_reply_to: None,
        references,
        html: fragment(&blocks),
        text,
        attachments,
    }
}

fn plain(text: &str) -> Inline {
    Inline::Text(text.to_owned(), Marks::default())
}

/// The original's content as blocks: its HTML if it has any visible content, else its text.
fn original_blocks(message: &ParsedMessage) -> Vec<Block> {
    if let Some(html) = &message.html {
        let blocks = html_blocks_with(html, false);
        if !blocks.is_empty() {
            return blocks;
        }
    }
    text_blocks(&message.display_text())
}

/// The original's text (text body, else the text of its HTML), without trailing blank lines.
fn original_text(message: &ParsedMessage) -> String {
    let text = message.display_text();
    text.trim_end().to_owned()
}

/// Removes leading reply prefixes (`Re:`, `AW:`, `Antw:`, `SV:`, `Re[2]:`, ...) and adds one
/// `Re: `. Forward prefixes stay: `Fwd: X` becomes `Re: Fwd: X`.
pub fn reply_subject(subject: &str) -> String {
    let base = strip_prefixes(subject, &[REPLY_PREFIXES]);
    format!("Re: {base}").trim_end().to_owned()
}

/// Removes leading forward prefixes (`Fwd:`, `Fw:`, `WG:`, `TR:`, ...) and adds one `Fwd: `.
pub fn forward_subject(subject: &str) -> String {
    let base = strip_prefixes(subject, &[FORWARD_PREFIXES]);
    format!("Fwd: {base}").trim_end().to_owned()
}

/// The subject without any leading reply or forward prefixes, for comparing subjects (thread
/// fallback).
pub fn strip_subject_prefixes(subject: &str) -> &str {
    strip_prefixes(subject, &[REPLY_PREFIXES, FORWARD_PREFIXES])
}

fn strip_prefixes<'s>(subject: &'s str, lists: &[&[&str]]) -> &'s str {
    let mut rest = subject.trim();
    while let Some(after) = strip_one_prefix(rest, lists) {
        rest = after.trim_start();
    }
    rest
}

/// `rest` after one prefix: a known word, an optional counter (`[2]`, `(2)`), a colon (also
/// the full-width one, and with a space before it as French programs write it).
fn strip_one_prefix<'s>(subject: &'s str, lists: &[&[&str]]) -> Option<&'s str> {
    let end = subject
        .find(|c: char| !c.is_alphabetic())
        .unwrap_or(subject.len());
    let (word, rest) = subject.split_at(end);
    let word = word.to_lowercase();
    if word.is_empty() || !lists.iter().any(|list| list.contains(&word.as_str())) {
        return None;
    }
    let mut rest = rest;
    for (open, close) in [('[', ']'), ('(', ')')] {
        if let Some(inner) = rest.strip_prefix(open) {
            let (count, after) = inner.split_once(close)?;
            if count.is_empty() || !count.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            rest = after;
            break;
        }
    }
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))
}

fn same_address(a: &str, b: &str) -> bool {
    let (a, b) = (a.trim(), b.trim());
    !a.is_empty() && a.eq_ignore_ascii_case(b)
}

/// The address with a `+tag` removed from the local part (`alice+lists@x` → `alice@x`).
fn without_tag(email: &str) -> Option<String> {
    let (local, domain) = email.trim().rsplit_once('@')?;
    let (base, _) = local.split_once('+')?;
    (!base.is_empty()).then(|| format!("{base}@{domain}"))
}

/// The identity a reply is sent from: the one that sent the original (a message from the user),
/// else the first identity in To, Cc or the delivery headers (`X-Original-To`, `Delivered-To`),
/// also when the original went to `name+tag@`; else the default identity, else the first one.
pub fn choose_identity<'i>(
    identities: &'i [Identity],
    headers: &MessageHeaders,
    delivered_to: &[String],
) -> Option<&'i Identity> {
    let find = |email: &str| identities.iter().find(|i| same_address(&i.email, email));
    if let Some(own) = headers.from.iter().find_map(|a| find(&a.email)) {
        return Some(own);
    }
    let candidates: Vec<&str> = headers
        .to
        .iter()
        .chain(&headers.cc)
        .map(|a| a.email.as_str())
        .chain(delivered_to.iter().map(String::as_str))
        .collect();
    if let Some(found) = candidates.iter().find_map(|email| find(email)) {
        return Some(found);
    }
    if let Some(found) = candidates
        .iter()
        .filter_map(|email| without_tag(email))
        .find_map(|email| find(&email))
    {
        return Some(found);
    }
    identities
        .iter()
        .find(|i| i.is_default)
        .or_else(|| identities.first())
}

/// To and Cc of a reply (see [`ReplyKind`]). Reply-To wins over From; a reply to the user's own
/// message goes to that message's recipients. The user's identities (also as `name+tag@`) and
/// duplicates are removed (addresses compared without case).
pub fn reply_recipients(
    headers: &MessageHeaders,
    kind: ReplyKind,
    identities: &[Identity],
) -> (Vec<Address>, Vec<Address>) {
    let own = |a: &Address| {
        let untagged = without_tag(&a.email);
        identities.iter().any(|i| {
            same_address(&i.email, &a.email)
                || untagged
                    .as_deref()
                    .is_some_and(|u| same_address(&i.email, u))
        })
    };
    let from_me = headers.from.iter().any(own);
    let primary: Vec<Address> = if from_me {
        headers.to.clone()
    } else if !headers.reply_to.is_empty() {
        headers.reply_to.clone()
    } else if !headers.from.is_empty() {
        headers.from.clone()
    } else {
        headers.sender.clone()
    };
    let mut to = primary.clone();
    let mut cc = Vec::new();
    if kind == ReplyKind::All {
        if !from_me {
            to.extend(headers.to.iter().cloned());
        }
        cc.extend(headers.cc.iter().cloned());
    }
    let mut seen: Vec<String> = Vec::new();
    let mut keep = |a: &Address| {
        let email = a.email.trim().to_ascii_lowercase();
        if email.is_empty() || own(a) || seen.contains(&email) {
            return false;
        }
        seen.push(email);
        true
    };
    to.retain(&mut keep);
    cc.retain(&mut keep);
    if to.is_empty() && cc.is_empty() {
        // A message the user sent to themselves: answer to themselves.
        to = primary
            .into_iter()
            .filter(|a| !a.email.trim().is_empty())
            .take(1)
            .collect();
    }
    if to.is_empty() {
        std::mem::swap(&mut to, &mut cc);
    }
    (to, cc)
}

/// `In-Reply-To` and `References` of an answer to a message with these headers (RFC 5322,
/// section 3.6.4): the parent's References (or its single In-Reply-To) followed by its
/// Message-ID, duplicates removed, cut to [`MAX_REFERENCES`] (the first and the latest kept).
pub fn reply_references(parent: &MessageHeaders) -> (Option<String>, Vec<String>) {
    let parent_id = parent
        .message_id
        .as_deref()
        .map(str::trim)
        .filter(|id| super::address::is_valid_message_id(id))
        .map(str::to_owned);
    let base: &[String] = if !parent.references.is_empty() {
        &parent.references
    } else if parent.in_reply_to.len() == 1 {
        &parent.in_reply_to
    } else {
        &[]
    };
    let mut references: Vec<String> = Vec::new();
    for id in base {
        let id = id.trim();
        if super::address::is_valid_message_id(id)
            && parent_id.as_deref() != Some(id)
            && !references.iter().any(|known| known == id)
        {
            references.push(id.to_owned());
        }
    }
    references.extend(parent_id.clone());
    if references.len() > MAX_REFERENCES {
        // Keeps the first and the latest MAX_REFERENCES - 1.
        let excess = references.len() - MAX_REFERENCES;
        references.drain(1..=excess);
    }
    (parent_id, references)
}

fn person(address: Option<&Address>, language: QuoteLanguage) -> String {
    match address {
        Some(Address {
            name: Some(name),
            email,
        }) if !email.is_empty() => format!("{name} <{email}>"),
        Some(Address {
            name: Some(name), ..
        }) => name.clone(),
        Some(Address { email, .. }) if !email.is_empty() => email.clone(),
        _ => match language {
            QuoteLanguage::English => "unknown sender".to_owned(),
            QuoteLanguage::German => "unbekannter Absender".to_owned(),
        },
    }
}

fn people(list: &[Address], language: QuoteLanguage) -> String {
    list.iter()
        .map(|a| person(Some(a), language))
        .collect::<Vec<_>>()
        .join(", ")
}

/// "On Mon, 23 Nov 2026 at 14:05, Alice <alice@example.org> wrote:" or
/// "Am 23.11.2026 um 14:05 schrieb Alice <alice@example.org>:".
fn attribution(headers: &MessageHeaders, options: QuoteOptions) -> String {
    let who = person(
        headers.from.first().or(headers.sender.first()),
        options.language,
    );
    let when = headers
        .date
        .map(|date| LocalTime::new(date, options.utc_offset_minutes));
    match (options.language, when) {
        (QuoteLanguage::English, Some(t)) => format!(
            "On {}, {} {} {} at {:02}:{:02}, {who} wrote:",
            t.weekday_en(),
            t.day,
            t.month_en(),
            t.year,
            t.hour,
            t.minute
        ),
        (QuoteLanguage::German, Some(t)) => format!(
            "Am {:02}.{:02}.{} um {:02}:{:02} schrieb {who}:",
            t.day, t.month, t.year, t.hour, t.minute
        ),
        (QuoteLanguage::English, None) => format!("{who} wrote:"),
        (QuoteLanguage::German, None) => format!("{who} schrieb:"),
    }
}

/// Label and value lines of the forward header.
fn forward_fields(headers: &MessageHeaders, options: QuoteOptions) -> Vec<(&'static str, String)> {
    let language = options.language;
    let labels = match language {
        QuoteLanguage::English => ["Subject", "Date", "From", "To", "Cc"],
        QuoteLanguage::German => ["Betreff", "Datum", "Von", "An", "Kopie"],
    };
    let [subject, date, from, to, cc] = labels;
    let mut fields = Vec::new();
    if let Some(s) = headers.subject.as_deref().filter(|s| !s.is_empty()) {
        fields.push((subject, s.to_owned()));
    }
    if let Some(ts) = headers.date {
        let t = LocalTime::new(ts, options.utc_offset_minutes);
        let value = match language {
            QuoteLanguage::English => format!(
                "{}, {} {} {} {:02}:{:02} {}",
                t.weekday_en(),
                t.day,
                t.month_en(),
                t.year,
                t.hour,
                t.minute,
                t.offset()
            ),
            QuoteLanguage::German => format!(
                "{:02}.{:02}.{} {:02}:{:02} {}",
                t.day,
                t.month,
                t.year,
                t.hour,
                t.minute,
                t.offset()
            ),
        };
        fields.push((date, value));
    }
    let sender = headers.from.first().or(headers.sender.first());
    fields.push((from, person(sender, language)));
    if !headers.to.is_empty() {
        fields.push((to, people(&headers.to, language)));
    }
    if !headers.cc.is_empty() {
        fields.push((cc, people(&headers.cc, language)));
    }
    fields
}

/// A Unix time in a fixed offset from UTC, as calendar fields.
struct LocalTime {
    year: i64,
    month: u32,
    day: u32,
    /// 0 = Sunday.
    weekday: u32,
    hour: u32,
    minute: u32,
    offset_minutes: i32,
}

impl LocalTime {
    fn new(unix: i64, offset_minutes: i32) -> Self {
        let local = unix.saturating_add(i64::from(offset_minutes) * 60);
        let days = local.div_euclid(86_400);
        let seconds = local.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            // 1970-01-01 was a Thursday.
            weekday: u32::try_from((days + 4).rem_euclid(7)).unwrap_or(0),
            hour: u32::try_from(seconds / 3600).unwrap_or(0),
            minute: u32::try_from(seconds % 3600 / 60).unwrap_or(0),
            offset_minutes,
        }
    }

    fn weekday_en(&self) -> &'static str {
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
            .get(self.weekday as usize)
            .copied()
            .unwrap_or("")
    }

    fn month_en(&self) -> &'static str {
        [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ]
        .get(self.month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("")
    }

    /// `+0100`, `-0530`.
    fn offset(&self) -> String {
        let sign = if self.offset_minutes < 0 { '-' } else { '+' };
        let abs = self.offset_minutes.unsigned_abs();
        format!("{sign}{:02}{:02}", abs / 60, abs % 60)
    }
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian (Howard Hinnant's
/// `civil_from_days`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}
