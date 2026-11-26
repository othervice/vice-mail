//! The tokens of a message for the spam filter (decision 0077).
//!
//! Every token is a short text with a prefix that says where it came from (`subject:`, `from:`,
//! `url:`, `h:` for a header field's name …); words of the text have no prefix. The texts are
//! hashed with 64-bit FNV-1a ([`spam_token_hash`]): stable across versions and platforms, so a
//! learned model stays valid, and the database holds numbers instead of words from mails.
//!
//! Words are split at white space, trimmed of punctuation and lower-cased; words of 3 to 12
//! characters count as they are, longer ones only by their first character and length
//! (`skip:x 20`, SpamBayes), shorter ones not at all. Text is read up to
//! [`MAX_SPAM_TEXT_BYTES`]; an HTML body by what a reader sees, with the hosts of its links and
//! whether it hides text. At most [`MAX_SPAM_TOKENS`] tokens per message.

use std::collections::HashSet;

use mail_model::Address;

use crate::{HeaderField, ParsedMessage, header_fields, html_to_visible_text, normalize_address};

/// Most tokens of one message (time and storage bound).
pub const MAX_SPAM_TOKENS: usize = 2_000;
/// The text body and the visible text of an HTML body are each read up to this many bytes.
pub const MAX_SPAM_TEXT_BYTES: usize = 64 * 1024;
/// HTML read for the visible text, links and images.
const MAX_HTML_BYTES: usize = 256 * 1024;
const MAX_HEADER_FIELDS: usize = 200;
const MAX_HEADER_VALUE_CHARS: usize = 1_000;
/// `Received` fields read, the newest (top) first.
const MAX_RECEIVED: usize = 8;
const MIN_WORD_CHARS: usize = 3;
const MAX_WORD_CHARS: usize = 12;
/// Words of a name or the path of a link.
const MAX_NAME_WORDS: usize = 6;
const MAX_URLS: usize = 64;

/// The tokens of one message: 64-bit hashes, sorted, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpamTokens {
    hashes: Vec<u64>,
}

impl SpamTokens {
    /// Tokens from hashes in any order (duplicates are dropped).
    pub fn from_hashes(mut hashes: Vec<u64>) -> Self {
        hashes.sort_unstable();
        hashes.dedup();
        Self { hashes }
    }

    pub fn hashes(&self) -> &[u64] {
        &self.hashes
    }

    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Whether the message has the token `text` (for tests and explanations).
    pub fn contains(&self, text: &str) -> bool {
        self.hashes.binary_search(&spam_token_hash(text)).is_ok()
    }

    /// The hashes as bytes, 8 per token, little endian (how the database keeps a learned
    /// message's tokens for unlearning it).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hashes.iter().flat_map(|h| h.to_le_bytes()).collect()
    }

    /// The reverse of [`SpamTokens::to_bytes`]; a trailing partial hash is ignored.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let hashes = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| u64::from_le_bytes(*chunk))
            .collect();
        Self::from_hashes(hashes)
    }
}

/// 64-bit FNV-1a of the token's UTF-8 bytes.
pub fn spam_token_hash(text: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    text.bytes().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// The stored header data of a message whose body is not stored: what
/// [`spam_tokens_of_fields`] reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpamFields<'a> {
    pub from: Option<&'a Address>,
    pub reply_to: &'a [Address],
    /// Addresses in To and Cc together.
    pub recipients: usize,
    pub subject: &'a str,
    /// The `List-Id` identifier.
    pub list_id: Option<&'a str>,
    /// The start of the text, as the list shows it.
    pub preview: Option<&'a str>,
}

/// The tokens of a message: `header` is its raw top-level header block (or the whole message),
/// `parsed` the message parsed by [`crate::parse`] (or, for a large message fetched in parts,
/// its header and text parts).
pub fn spam_tokens(header: &[u8], parsed: &ParsedMessage) -> SpamTokens {
    let mut tokens = Tokens::default();
    let fields = header_fields(header, MAX_HEADER_FIELDS, MAX_HEADER_VALUE_CHARS);
    header_tokens(&mut tokens, &fields, parsed);
    body_tokens(&mut tokens, parsed);
    tokens.finish()
}

/// [`spam_tokens`] of a whole raw message.
pub fn spam_tokens_of_raw(raw: &[u8]) -> SpamTokens {
    spam_tokens(raw, &crate::parse(raw))
}

/// The tokens the stored header data gives: sender, Reply-To, the number of recipients,
/// subject, list and the words of the preview. A subset of what [`spam_tokens`] gives for the
/// same message.
pub fn spam_tokens_of_fields(fields: &SpamFields<'_>) -> SpamTokens {
    let mut tokens = Tokens::default();
    sender_tokens(&mut tokens, fields.from, fields.reply_to);
    recipient_tokens(&mut tokens, fields.recipients);
    subject_tokens(&mut tokens, Some(fields.subject));
    if let Some(id) = fields.list_id {
        list_tokens(&mut tokens, id);
    }
    if let Some(preview) = fields.preview {
        text_tokens(&mut tokens, preview);
    }
    tokens.finish()
}

// ---------------------------------------------------------------------------------------------
// Collecting

#[cfg(test)]
thread_local! {
    /// The texts of the hashes seen in this test thread (explanations in the corpus test).
    pub(crate) static NAMES: std::cell::RefCell<std::collections::HashMap<u64, String>> =
        std::cell::RefCell::default();
}

#[derive(Default)]
struct Tokens {
    seen: HashSet<u64>,
}

impl Tokens {
    fn add(&mut self, text: &str) {
        if self.seen.len() < MAX_SPAM_TOKENS {
            let hash = spam_token_hash(text);
            #[cfg(test)]
            NAMES.with(|names| {
                names
                    .borrow_mut()
                    .entry(hash)
                    .or_insert_with(|| text.to_owned());
            });
            self.seen.insert(hash);
        }
    }

    fn finish(self) -> SpamTokens {
        SpamTokens::from_hashes(self.seen.into_iter().collect())
    }
}

// ---------------------------------------------------------------------------------------------
// Header

fn header_tokens(tokens: &mut Tokens, fields: &[HeaderField], parsed: &ParsedMessage) {
    let headers = &parsed.headers;
    let mut received = 0usize;
    for field in fields {
        let name = field.name.trim().to_ascii_lowercase();
        if name.is_empty() || name.len() > 40 || !name.bytes().all(|b| b.is_ascii_graphic()) {
            continue;
        }
        tokens.add(&format!("h:{name}"));
        let value = field.value.trim();
        match name.as_str() {
            "received" => {
                if received < MAX_RECEIVED {
                    received_tokens(tokens, value);
                }
                received += 1;
            }
            "authentication-results" | "arc-authentication-results" => {
                auth_tokens(tokens, value);
            }
            "received-spf" => {
                if let Some(word) = first_word(value) {
                    tokens.add(&format!("received-spf:{word}"));
                }
            }
            "dkim-signature" => dkim_tokens(tokens, value, headers.from.first()),
            "x-mailer" | "user-agent" => {
                for word in words_of(value).take(3) {
                    tokens.add(&format!("mailer:{word}"));
                }
            }
            "content-type" => content_type_tokens(tokens, value),
            "message-id" => match value.rsplit_once('@') {
                Some((_, domain)) => {
                    let domain = domain.trim_end_matches('>').trim().to_ascii_lowercase();
                    if domain.is_empty() || !domain.contains('.') {
                        tokens.add("msgid:no-domain");
                    } else {
                        tokens.add(&format!("msgid:domain:{domain}"));
                    }
                }
                None => tokens.add("msgid:no-domain"),
            },
            "return-path" => return_path_tokens(tokens, value, headers.from.first()),
            "precedence" => {
                if let Some(word) = first_word(value) {
                    tokens.add(&format!("precedence:{word}"));
                }
            }
            "x-priority" => {
                if let Some(c) = value.chars().next() {
                    tokens.add(&format!("x-priority:{c}"));
                }
            }
            "to" => {
                if value.to_lowercase().contains("undisclosed") {
                    tokens.add("to:undisclosed");
                }
            }
            "x-spam-level" => {
                let stars = value.chars().filter(|c| *c == '*').count();
                tokens.add(&format!("x-spam-level:{}", bucket(stars)));
            }
            "x-ms-exchange-organization-scl" => {
                if let Some(word) = first_word(value) {
                    tokens.add(&format!("scl:{word}"));
                }
            }
            "x-forefront-antispam-report" => {
                for part in value.split(';') {
                    if let Some((key, val)) = part.split_once(':') {
                        let key = key.trim().to_ascii_lowercase();
                        if matches!(key.as_str(), "sfv" | "cat" | "scl") {
                            tokens.add(&format!("ms:{key}:{}", val.trim().to_ascii_lowercase()));
                        }
                    }
                }
            }
            _ => {
                // A server's own spam filter: its verdict word.
                if (name.starts_with("x-spam") || name.starts_with("x-rspamd"))
                    && let Some(word) = first_word(value)
                {
                    tokens.add(&format!("{name}:{word}"));
                }
            }
        }
    }
    tokens.add(&format!("h:count:{}", bucket(fields.len())));
    tokens.add(&format!("received:count:{}", bucket(received)));
    if !fields
        .iter()
        .any(|f| f.name.eq_ignore_ascii_case("message-id"))
    {
        tokens.add("msgid:none");
    }
    sender_tokens(tokens, headers.from.first(), &headers.reply_to);
    if let Some(address) = headers
        .sender
        .first()
        .and_then(|sender| normalize_address(&sender.email))
    {
        domain_tokens(tokens, "sender:domain", &address.domain);
    }
    recipient_tokens(tokens, headers.to.len() + headers.cc.len());
    subject_tokens(tokens, headers.subject.as_deref());
    if let Some(id) = &headers.list.id {
        list_tokens(tokens, id);
    }
}

/// From and Reply-To; returns the sender's domain.
fn sender_tokens(
    tokens: &mut Tokens,
    from: Option<&Address>,
    reply_to: &[Address],
) -> Option<String> {
    let Some(from) = from else {
        tokens.add("from:none");
        return None;
    };
    let domain = address_tokens(tokens, "from", from);
    for address in reply_to.iter().take(2) {
        let reply_domain = address_tokens(tokens, "reply-to", address);
        if reply_domain.is_some() && reply_domain != domain {
            tokens.add("reply-to:other-domain");
        }
    }
    domain
}

/// Tokens of one address under `prefix`; returns its ASCII domain.
fn address_tokens(tokens: &mut Tokens, prefix: &str, address: &Address) -> Option<String> {
    let name = address.name.as_deref().map(str::trim).unwrap_or_default();
    if name.is_empty() {
        tokens.add(&format!("{prefix}:name:none"));
    } else {
        for word in words_of(name).take(MAX_NAME_WORDS) {
            tokens.add(&format!("{prefix}:name:{word}"));
        }
        if name.contains('@') {
            tokens.add(&format!("{prefix}:name:has-address"));
        }
    }
    let Some(normalized) = normalize_address(&address.email) else {
        tokens.add(&format!("{prefix}:invalid"));
        return None;
    };
    tokens.add(&format!("{prefix}:addr:{}", normalized.key));
    if normalized.local.chars().count() <= 24 {
        tokens.add(&format!("{prefix}:local:{}", normalized.local));
    }
    if normalized
        .local
        .chars()
        .filter(char::is_ascii_digit)
        .count()
        >= 4
    {
        tokens.add(&format!("{prefix}:local:digits"));
    }
    domain_tokens(tokens, &format!("{prefix}:domain"), &normalized.domain);
    Some(normalized.domain)
}

/// The domain and each parent with at least two labels (the last four of them), and the
/// top-level label.
fn domain_tokens(tokens: &mut Tokens, prefix: &str, domain: &str) {
    let domain = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty() {
        return;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    let last = labels.len().saturating_sub(1);
    for start in last.saturating_sub(4)..last {
        let suffix = labels.get(start..).unwrap_or_default().join(".");
        tokens.add(&format!("{prefix}:{suffix}"));
    }
    if let Some(tld) = labels.last() {
        tokens.add(&format!("{prefix}:tld:{tld}"));
    }
}

fn recipient_tokens(tokens: &mut Tokens, count: usize) {
    tokens.add(&format!("to:count:{}", bucket(count)));
}

fn list_tokens(tokens: &mut Tokens, id: &str) {
    let id = id.trim().trim_matches(['<', '>']).trim().to_lowercase();
    if !id.is_empty() && id.chars().count() <= 120 {
        tokens.add(&format!("list-id:{id}"));
    }
}

fn subject_tokens(tokens: &mut Tokens, subject: Option<&str>) {
    let subject = subject.map(str::trim).unwrap_or_default();
    if subject.is_empty() {
        tokens.add("subject:none");
        return;
    }
    for chunk in subject.split_whitespace() {
        if let Some(word) = word_token(chunk) {
            tokens.add(&format!("subject:{word}"));
        }
    }
    let letters: Vec<char> = subject.chars().filter(|c| c.is_alphabetic()).collect();
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    if letters.len() >= 8 && upper * 10 >= letters.len() * 6 {
        tokens.add("subject:caps");
    }
    let mut run = String::new();
    for c in subject.chars().chain(std::iter::once(' ')) {
        if c.is_alphanumeric() || c.is_whitespace() {
            if !run.is_empty() && run.chars().count() <= 4 {
                tokens.add(&format!("subject:punct:{run}"));
            }
            run.clear();
        } else {
            run.push(c);
        }
    }
    let lower = subject.to_lowercase();
    if ["re:", "aw:", "fwd:", "fw:", "wg:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        tokens.add("subject:reply");
    }
    tokens.add(&format!(
        "subject:len:{}",
        bucket(subject.chars().count() / 10)
    ));
}

/// The hosts and addresses of a `Received` field: the `from` part's names, the IPv4 address's
/// first two and three numbers, the protocol after `with`.
fn received_tokens(tokens: &mut Tokens, value: &str) {
    let lower = value.to_ascii_lowercase();
    let from_part = lower
        .strip_prefix("from ")
        .map(|rest| rest.split(" by ").next().unwrap_or(rest))
        .unwrap_or_default();
    for chunk in from_part.split(|c: char| c.is_whitespace() || "()[]<>;,".contains(c)) {
        let chunk = chunk.trim_matches(|c: char| c == '.' || c == '=' || c == '"');
        if chunk.is_empty() {
            continue;
        }
        if let Some(octets) = ipv4(chunk) {
            tokens.add(&format!("received:ip:{}.{}", octets[0], octets[1]));
            tokens.add(&format!(
                "received:ip:{}.{}.{}",
                octets[0], octets[1], octets[2]
            ));
        } else if chunk.contains('.')
            && chunk.chars().any(|c| c.is_ascii_alphabetic())
            && chunk
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            domain_tokens(tokens, "received", chunk);
        }
    }
    if let Some((_, after)) = lower.split_once(" with ")
        && let Some(protocol) = first_word(after)
    {
        tokens.add(&format!("received:with:{protocol}"));
    }
}

fn ipv4(text: &str) -> Option<[u8; 4]> {
    let mut octets = [0u8; 4];
    let mut parts = text.split('.');
    for octet in &mut octets {
        *octet = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(octets)
}

/// `method=result` pairs of `Authentication-Results` for the methods that say something.
fn auth_tokens(tokens: &mut Tokens, value: &str) {
    for part in value.split([';', ' ', '\t']) {
        let Some((method, result)) = part.split_once('=') else {
            continue;
        };
        let method = method.trim().to_ascii_lowercase();
        if !matches!(
            method.as_str(),
            "spf" | "dkim" | "dmarc" | "arc" | "iprev" | "auth" | "bimi" | "compauth"
        ) {
            continue;
        }
        let result: String = result
            .trim()
            .chars()
            .take_while(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_lowercase();
        if !result.is_empty() && result.len() <= 16 {
            tokens.add(&format!("auth:{method}={result}"));
        }
    }
}

/// The signing domain of a `DKIM-Signature`, and whether it is the sender's.
fn dkim_tokens(tokens: &mut Tokens, value: &str, from: Option<&Address>) {
    let Some(domain) = value.split(';').find_map(|tag| {
        let (key, val) = tag.split_once('=')?;
        (key.trim() == "d").then(|| val.trim().to_ascii_lowercase())
    }) else {
        return;
    };
    if domain.is_empty() {
        return;
    }
    tokens.add(&format!("dkim:d:{domain}"));
    let from_domain = from
        .and_then(|f| normalize_address(&f.email))
        .map(|a| a.domain);
    if let Some(from_domain) = from_domain {
        let aligned = from_domain == domain || from_domain.ends_with(&format!(".{domain}"));
        tokens.add(if aligned {
            "dkim:aligned"
        } else {
            "dkim:other-domain"
        });
    }
}

fn return_path_tokens(tokens: &mut Tokens, value: &str, from: Option<&Address>) {
    let inner = value.trim().trim_matches(['<', '>']).trim();
    if inner.is_empty() {
        tokens.add("return-path:empty");
        return;
    }
    let Some(address) = normalize_address(inner) else {
        tokens.add("return-path:invalid");
        return;
    };
    domain_tokens(tokens, "return-path:domain", &address.domain);
    let from_domain = from
        .and_then(|f| normalize_address(&f.email))
        .map(|a| a.domain);
    if from_domain.is_some_and(|d| d != address.domain) {
        tokens.add("return-path:other-domain");
    }
}

fn content_type_tokens(tokens: &mut Tokens, value: &str) {
    let mut parts = value.split(';');
    if let Some(kind) = parts.next() {
        let kind = kind.trim().to_ascii_lowercase();
        if !kind.is_empty() && kind.len() <= 60 {
            tokens.add(&format!("ct:{kind}"));
        }
    }
    for param in parts {
        if let Some((key, val)) = param.split_once('=')
            && key.trim().eq_ignore_ascii_case("charset")
        {
            let charset = val.trim().trim_matches('"').to_ascii_lowercase();
            if !charset.is_empty() && charset.len() <= 30 {
                tokens.add(&format!("charset:{charset}"));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Body

fn body_tokens(tokens: &mut Tokens, parsed: &ParsedMessage) {
    let mut words = 0usize;
    if let Some(text) = parsed.text.as_deref() {
        words += text_tokens(tokens, cut(text, MAX_SPAM_TEXT_BYTES));
    }
    if let Some(html) = parsed.html.as_deref().filter(|h| !h.trim().is_empty()) {
        let html = cut(html, MAX_HTML_BYTES);
        if parsed.text.is_none() {
            tokens.add("html:only");
        }
        let visible = html_to_visible_text(html);
        if visible.hidden_chars > 0 {
            tokens.add("html:hidden-text");
            tokens.add(&format!(
                "html:hidden:{}",
                bucket(visible.hidden_chars / 10)
            ));
        }
        words += text_tokens(tokens, cut(&visible.text, MAX_SPAM_TEXT_BYTES));
        let lower = html.to_ascii_lowercase();
        tokens.add(&format!(
            "html:img:{}",
            bucket(lower.matches("<img").count())
        ));
        tokens.add(&format!(
            "html:links:{}",
            bucket(lower.matches("href").count())
        ));
        let mut urls = 0usize;
        for scheme in ["http://", "https://"] {
            for (at, _) in lower.match_indices(scheme) {
                if urls >= MAX_URLS {
                    break;
                }
                if let Some(rest) = html.get(at..) {
                    url_tokens(tokens, rest);
                    urls += 1;
                }
            }
        }
    }
    if words == 0 {
        tokens.add("body:empty");
    }
    tokens.add(&format!("body:words:{}", bucket(words / 10)));
    for attachment in &parsed.attachments {
        let kind = attachment.mime.to_ascii_lowercase();
        if kind.len() <= 80 {
            tokens.add(&format!("attach:type:{kind}"));
        }
        if let Some((_, ext)) = attachment.file_name.rsplit_once('.') {
            let ext = ext.to_lowercase();
            if !ext.is_empty() && ext.chars().count() <= 8 {
                tokens.add(&format!("attach:ext:{ext}"));
            }
        }
    }
    tokens.add(&format!(
        "attach:count:{}",
        bucket(parsed.attachments.len())
    ));
    if !parsed.inline.is_empty() {
        tokens.add(&format!("inline:count:{}", bucket(parsed.inline.len())));
    }
}

/// Words, links and addresses of a text; returns the number of words read.
fn text_tokens(tokens: &mut Tokens, text: &str) -> usize {
    let mut words = 0usize;
    for chunk in text.split_whitespace() {
        words += 1;
        let lower = chunk.to_ascii_lowercase();
        if let Some(at) = lower.find("http://").or_else(|| lower.find("https://")) {
            if let Some(rest) = chunk.get(at..) {
                url_tokens(tokens, rest);
            }
            continue;
        }
        if lower.starts_with("www.") {
            url_tokens(tokens, &format!("http://{chunk}"));
            continue;
        }
        if chunk.contains('@') {
            let candidate = chunk.trim_matches(|c: char| !c.is_alphanumeric());
            if let Some(address) = normalize_address(candidate) {
                tokens.add(&format!("email:addr:{}", address.key));
                domain_tokens(tokens, "email:domain", &address.domain);
                continue;
            }
        }
        if let Some(word) = word_token(chunk) {
            tokens.add(&word);
        }
    }
    words
}

/// A word as a token: trimmed of punctuation (a currency sign in front and `%` behind stay),
/// lower case; 3 to 12 characters as they are, longer ones as `skip:<first> <tens>`.
fn word_token(chunk: &str) -> Option<String> {
    let word = chunk
        .trim_start_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '$' | '€' | '£')))
        .trim_end_matches(|c: char| !(c.is_alphanumeric() || c == '%'));
    let count = word.chars().count();
    if count < MIN_WORD_CHARS {
        return None;
    }
    let word = word.to_lowercase();
    if count <= MAX_WORD_CHARS {
        return Some(word);
    }
    let first = word.chars().next()?;
    Some(format!("skip:{first} {}", count / 10 * 10))
}

/// Words for names and program names: alphanumeric runs, lower case, 2 to 20 characters.
fn words_of(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| (2..=20).contains(&w.chars().count()))
        .map(str::to_lowercase)
}

fn first_word(text: &str) -> Option<String> {
    let word: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!word.is_empty() && word.len() <= 24).then(|| word.to_ascii_lowercase())
}

/// The host of the link at the start of `text` (`http://…`): its domain and parents (or
/// `url:ip`), the scheme, and a few words of the path.
fn url_tokens(tokens: &mut Tokens, text: &str) {
    let Some((scheme, rest)) = text.split_once("://") else {
        return;
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme == "http" || scheme == "https" {
        tokens.add(&format!("url:scheme:{scheme}"));
    }
    let end = rest
        .find(|c: char| "/?#\"'<> \t\r\n)]}\\".contains(c))
        .unwrap_or(rest.len());
    let (authority, after) = rest.split_at(end);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if authority.contains('@') {
        tokens.add("url:userinfo");
    }
    if ipv4(&host).is_some() {
        tokens.add("url:ip");
    } else if host.contains('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        domain_tokens(tokens, "url", &host);
    } else if !host.is_empty() {
        tokens.add("url:odd-host");
    }
    let path_end = after
        .find(|c: char| "\"'<> \t\r\n".contains(c))
        .unwrap_or(after.len());
    let path = after.get(..path_end).unwrap_or_default();
    for word in path
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| (MIN_WORD_CHARS..=MAX_WORD_CHARS).contains(&w.chars().count()))
        .take(MAX_NAME_WORDS)
    {
        tokens.add(&format!("url:path:{}", word.to_lowercase()));
    }
}

/// `text` cut to at most `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    text.get(..text.floor_char_boundary(max))
        .unwrap_or_default()
}

/// A rough size class: 0, 1, 2, 3–4, 5–8, 9–16, … (powers of two).
fn bucket(n: usize) -> usize {
    if n <= 2 {
        n
    } else {
        usize::BITS as usize - (n - 1).leading_zeros() as usize + 1
    }
}
