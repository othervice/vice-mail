//! Thunderbird profiles (stage 19, takeover from Thunderbird): the list of profiles in
//! `profiles.ini`, the preferences in a profile's `prefs.js`, and the accounts, identities,
//! signatures and SMTP servers they describe. Pure: the engine finds and reads the files
//! (macOS `~/Library/Thunderbird/`, Windows `%APPDATA%\Thunderbird\`) and passes their text in;
//! nothing here opens a file, and paths come back as written.
//!
//! **Passwords are never involved.** Thunderbird keeps saved passwords in `logins.json` and
//! `key4.db`; nothing in this module reads, parses or names them, and `prefs.js` holds none.
//!
//! - [`parse_profiles_ini`]: `[ProfileN]` sections; the default is the one an `[Install…]`
//!   section names (what Thunderbird starts with since version 68), else `Default=1`.
//! - [`parse_prefs_js`]: `user_pref("key", value);` statements (also `pref(`) with string,
//!   integer and boolean values. Everything else is ignored; the work is linear in the input.
//! - [`thunderbird_accounts`]: accounts in the order of `mail.accountmanager.accounts`, each with
//!   its incoming server ([`TbServer`]), identities ([`TbIdentity`]) with signature and SMTP
//!   server.
//!
//! Limits keep a hostile profile cheap: `profiles.ini` is cut at 256 KiB and gives at most 64
//! profiles; `prefs.js` is cut at 16 MiB, gives at most 100,000 preferences, and a value over
//! 1 MiB is dropped; at most 64 accounts with 32 identities each.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

/// `profiles.ini` is read up to this many bytes.
const MAX_PROFILES_INI_BYTES: usize = 256 * 1024;
/// At most this many profiles come out of `profiles.ini`.
const MAX_PROFILES: usize = 64;
/// `prefs.js` is read up to this many bytes.
const MAX_PREFS_BYTES: usize = 16 * 1024 * 1024;
/// At most this many preferences come out of `prefs.js`.
const MAX_PREFS: usize = 100_000;
/// A preference value (or key) longer than this is dropped.
const MAX_PREF_VALUE_BYTES: usize = 1024 * 1024;
/// Accounts and identities per account.
const MAX_ACCOUNTS: usize = 64;
const MAX_IDENTITIES: usize = 32;

/// One profile of `profiles.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThunderbirdProfileEntry {
    /// `Name=`, else the last part of the path.
    pub name: String,
    /// `Path=` as written (`/` stays `/`, a Windows path keeps its `\`).
    pub path: String,
    /// `IsRelative=1`: the path is relative to the directory of `profiles.ini`.
    pub is_relative: bool,
    /// The profile Thunderbird starts with.
    pub is_default: bool,
}

/// The text up to `max` bytes, cut at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// `[ProfileN]` sections (`Name`, `Path`, `IsRelative`, `Default=1`); an `[Install…]` section's
/// `Default=<path>` marks that profile as the default and wins over `Default=1`. Sections without
/// a path are skipped. Paths keep their separators as written. At most 64 profiles; the input is
/// cut at 256 KiB.
pub fn parse_profiles_ini(text: &str) -> Vec<ThunderbirdProfileEntry> {
    #[derive(Default)]
    struct Section {
        name: Option<String>,
        path: Option<String>,
        relative: Option<bool>,
        default: bool,
    }
    let text = cut(text, MAX_PROFILES_INI_BYTES);
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut profiles: Vec<Section> = Vec::new();
    let mut install_default: Option<String> = None;
    // Which section the lines belong to: a profile (index), an install section, or neither.
    enum In {
        Profile(usize),
        Install,
        Other,
    }
    let mut current = In::Other;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let header = header.trim();
            let lower = header.to_ascii_lowercase();
            current = if lower.starts_with("profile") && profiles.len() < MAX_PROFILES {
                profiles.push(Section::default());
                In::Profile(profiles.len() - 1)
            } else if lower.starts_with("install") {
                In::Install
            } else {
                In::Other
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
        match current {
            In::Profile(index) => {
                let Some(section) = profiles.get_mut(index) else {
                    continue;
                };
                match key.as_str() {
                    "name" => section.name = Some(value.to_owned()),
                    "path" => section.path = Some(value.to_owned()),
                    "isrelative" => section.relative = Some(value == "1"),
                    "default" => section.default = value == "1",
                    _ => {}
                }
            }
            In::Install => {
                if key == "default" && !value.is_empty() && install_default.is_none() {
                    install_default = Some(value.to_owned());
                }
            }
            In::Other => {}
        }
    }
    let same_path = |a: &str, b: &str| a.replace('\\', "/") == b.replace('\\', "/");
    let installed = install_default.filter(|default| {
        profiles.iter().any(|p| {
            p.path
                .as_deref()
                .is_some_and(|path| same_path(path, default))
        })
    });
    profiles
        .into_iter()
        .filter_map(|section| {
            let path = section.path.filter(|p| !p.is_empty())?;
            let is_default = match &installed {
                Some(default) => same_path(&path, default),
                None => section.default,
            };
            let is_relative = section.relative.unwrap_or_else(|| !looks_absolute(&path));
            let name = section
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| last_part(&path).to_owned());
            Some(ThunderbirdProfileEntry {
                name,
                path,
                is_relative,
                is_default,
            })
        })
        .collect()
}

/// `/x`, `\x`, `C:\x`, `C:/x`.
fn looks_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    matches!(bytes.first(), Some(b'/' | b'\\'))
        || (bytes.len() >= 3
            && bytes.first().is_some_and(u8::is_ascii_alphabetic)
            && bytes.get(1) == Some(&b':')
            && matches!(bytes.get(2), Some(b'/' | b'\\')))
}

fn last_part(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
}

/// A value of `prefs.js`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

/// The statements of `prefs.js`: `user_pref("key", value);` and `pref("key", value);` with a
/// JavaScript string (double or single quotes, escapes `\"`, `\'`, `\\`, `\n`, `\r`, `\t`,
/// `\uXXXX` with surrogate pairs, `\xXX`), an integer or `true`/`false`. Comments (`//`,
/// `/* */`, `#`) and anything else are skipped; a later statement for the same key wins, as in
/// Thunderbird. At most 100,000 preferences; the input is cut at 16 MiB, and a key or value over
/// 1 MiB is dropped.
pub fn parse_prefs_js(text: &str) -> BTreeMap<String, PrefValue> {
    let text = cut(text, MAX_PREFS_BYTES);
    let mut scanner = Scanner { text, pos: 0 };
    let mut prefs = BTreeMap::new();
    while scanner.skip_trivia() {
        let start = scanner.pos;
        match scanner.statement() {
            Some((key, value)) => {
                if prefs.len() < MAX_PREFS || prefs.contains_key(&key) {
                    prefs.insert(key, value);
                }
            }
            None => {
                // Not a statement we read: go on with the line after the one it started on (a
                // string that never closed must not swallow the statements after it).
                scanner.pos = start;
                scanner.skip_line();
            }
        }
    }
    prefs
}

/// A small scanner over `prefs.js` (positions are byte offsets at character boundaries).
struct Scanner<'a> {
    text: &'a str,
    pos: usize,
}

impl Scanner<'_> {
    fn rest(&self) -> &str {
        self.text.get(self.pos..).unwrap_or_default()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.rest().chars().nth(offset)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.bump() {
            if c == '\n' {
                break;
            }
        }
    }

    /// Skips whitespace and comments; `false` at the end.
    fn skip_trivia(&mut self) -> bool {
        loop {
            match (self.peek(), self.peek_at(1)) {
                (None, _) => return false,
                (Some(c), _) if c.is_whitespace() => self.pos += c.len_utf8(),
                (Some('#'), _) | (Some('/'), Some('/')) => self.skip_line(),
                (Some('/'), Some('*')) => {
                    self.pos += 2;
                    loop {
                        match (self.peek(), self.peek_at(1)) {
                            (None, _) => return false,
                            (Some('*'), Some('/')) => {
                                self.pos += 2;
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => return true,
            }
        }
    }

    /// Whitespace and comments inside a statement.
    fn space(&mut self) {
        let _ = self.skip_trivia();
    }

    fn expect(&mut self, wanted: char) -> Option<()> {
        self.space();
        (self.bump()? == wanted).then_some(())
    }

    fn identifier(&mut self) -> String {
        let mut word = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
                self.pos += 1;
                if word.len() > 32 {
                    break;
                }
            } else {
                break;
            }
        }
        word
    }

    /// `user_pref(<string>, <value>);` or `pref(…);`.
    fn statement(&mut self) -> Option<(String, PrefValue)> {
        let word = self.identifier();
        if word != "user_pref" && word != "pref" {
            return None;
        }
        self.expect('(')?;
        self.space();
        let key = self.string()?;
        self.expect(',')?;
        self.space();
        let value = self.value()?;
        self.expect(')')?;
        // The semicolon is optional at the very end of a file.
        self.space();
        if self.peek() == Some(';') {
            self.pos += 1;
        }
        let key = key?;
        let value = value?;
        Some((key, value))
    }

    /// A value; the inner `None` when it was read but is too large (the statement is skipped).
    fn value(&mut self) -> Option<Option<PrefValue>> {
        match self.peek()? {
            '"' | '\'' => Some(self.string()?.map(PrefValue::Str)),
            '-' | '+' | '0'..='9' => {
                let negative = self.peek() == Some('-');
                if matches!(self.peek(), Some('-' | '+')) {
                    self.pos += 1;
                }
                let mut digits = String::new();
                while let Some(c) = self.peek().filter(char::is_ascii_digit) {
                    digits.push(c);
                    self.pos += 1;
                }
                if digits.is_empty() {
                    return None;
                }
                let magnitude = digits.parse::<i64>().ok();
                Some(magnitude.map(|m| PrefValue::Int(if negative { -m } else { m })))
            }
            _ => match self.identifier().as_str() {
                "true" => Some(Some(PrefValue::Bool(true))),
                "false" => Some(Some(PrefValue::Bool(false))),
                _ => None,
            },
        }
    }

    /// A string literal; the inner `None` when it is longer than the limit.
    fn string(&mut self) -> Option<Option<String>> {
        let quote = self.bump()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let mut out = String::new();
        let mut too_long = false;
        let mut push = |out: &mut String, c: char| {
            if out.len() + c.len_utf8() > MAX_PREF_VALUE_BYTES {
                too_long = true;
            } else {
                out.push(c);
            }
        };
        loop {
            let c = self.bump()?;
            match c {
                c if c == quote => break,
                '\\' => {
                    let escaped = self.bump()?;
                    let decoded = match escaped {
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        'v' => '\u{b}',
                        '0' => '\0',
                        'x' => self.hex(2).and_then(char::from_u32).unwrap_or('\u{fffd}'),
                        'u' => self.unicode_escape(),
                        // `\"`, `\'`, `\\`, `\/` and anything else: the character itself.
                        other => other,
                    };
                    push(&mut out, decoded);
                }
                other => push(&mut out, other),
            }
        }
        Some((!too_long).then_some(out))
    }

    /// `n` hex digits as a number; nothing is consumed when they are not there.
    fn hex(&mut self, n: usize) -> Option<u32> {
        let mut value = 0u32;
        for offset in 0..n {
            let digit = self.peek_at(offset)?.to_digit(16)?;
            value = value * 16 + digit;
        }
        self.pos += n;
        Some(value)
    }

    /// After `\u`: four hex digits, a surrogate pair (`\uD83D\uDE00`) as one character.
    fn unicode_escape(&mut self) -> char {
        let Some(first) = self.hex(4) else {
            return '\u{fffd}';
        };
        if (0xD800..0xDC00).contains(&first) {
            if self.peek() == Some('\\') && self.peek_at(1) == Some('u') {
                let save = self.pos;
                self.pos += 2;
                if let Some(second) = self.hex(4)
                    && (0xDC00..0xE000).contains(&second)
                {
                    let code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                    return char::from_u32(code).unwrap_or('\u{fffd}');
                }
                self.pos = save;
            }
            return '\u{fffd}';
        }
        char::from_u32(first).unwrap_or('\u{fffd}')
    }
}

/// The kind of an incoming server (`mail.server.<key>.type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TbServerKind {
    /// `imap`.
    Imap,
    /// `pop3`.
    Pop3,
    /// `none`: Thunderbird's "Local Folders".
    Local,
    /// RSS, news, movemail and anything else.
    Other,
}

/// Transport security of a server: `socketType` (0 or 1 none, 2 STARTTLS, 3 TLS) for incoming
/// servers, `try_ssl` (0 none, 1 and 2 STARTTLS, 3 TLS) for SMTP servers. Missing or unknown
/// values are `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TbSecurity {
    None,
    StartTls,
    Tls,
}

/// An incoming server (`mail.server.<key>.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TbServer {
    pub key: String,
    pub kind: TbServerKind,
    pub host: String,
    /// `None` when not set (Thunderbird then uses the protocol's default for the security).
    pub port: Option<u16>,
    pub security: TbSecurity,
    pub user_name: String,
    /// The account's name in Thunderbird (`name`).
    pub name: String,
    /// `directory-rel` as written (`[ProfD]Mail/Local Folders`), else `directory`.
    pub directory: Option<String>,
}

/// An SMTP server (`mail.smtpserver.<key>.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TbSmtp {
    pub key: String,
    pub host: String,
    pub port: Option<u16>,
    pub security: TbSecurity,
    pub user_name: String,
}

/// The signature of an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TbSignature {
    None,
    /// `htmlSigText` with `htmlSigFormat` false.
    Text(String),
    /// `htmlSigText` with `htmlSigFormat` true.
    Html(String),
    /// `attach_signature` true: the file `sig_file-rel` (`[ProfD]…`), else `sig_file`, as
    /// written; `html` when its name ends in `.htm` or `.html`.
    File {
        path: String,
        html: bool,
    },
}

/// An identity (`mail.identity.<key>.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TbIdentity {
    pub key: String,
    pub email: String,
    pub full_name: String,
    pub reply_to: Option<String>,
    pub signature: TbSignature,
    /// The identity's `smtpServer`, else `mail.smtp.defaultserver`, else the first of
    /// `mail.smtpservers`; `None` when none of them exists.
    pub smtp: Option<TbSmtp>,
}

/// An account: its incoming server and identities (Local Folders has none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TbAccount {
    pub key: String,
    pub server: TbServer,
    pub identities: Vec<TbIdentity>,
}

type Prefs = BTreeMap<String, PrefValue>;

fn text_pref<'a>(prefs: &'a Prefs, key: &str) -> Option<&'a str> {
    match prefs.get(key)? {
        PrefValue::Str(s) => Some(s.as_str()),
        PrefValue::Int(_) | PrefValue::Bool(_) => None,
    }
}

/// A string preference, trimmed; empty when missing.
fn string_pref(prefs: &Prefs, key: &str) -> String {
    text_pref(prefs, key).unwrap_or_default().trim().to_owned()
}

fn int_pref(prefs: &Prefs, key: &str) -> Option<i64> {
    match prefs.get(key)? {
        PrefValue::Int(n) => Some(*n),
        PrefValue::Str(s) => s.trim().parse().ok(),
        PrefValue::Bool(_) => None,
    }
}

fn bool_pref(prefs: &Prefs, key: &str) -> bool {
    match prefs.get(key) {
        Some(PrefValue::Bool(b)) => *b,
        Some(PrefValue::Int(n)) => *n != 0,
        Some(PrefValue::Str(s)) => s.trim() == "true",
        None => false,
    }
}

fn port_pref(prefs: &Prefs, key: &str) -> Option<u16> {
    int_pref(prefs, key)
        .and_then(|n| u16::try_from(n).ok())
        .filter(|&p| p != 0)
}

/// A comma-separated list of keys, trimmed, without empty entries and repeats.
fn key_list(prefs: &Prefs, key: &str) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for part in text_pref(prefs, key).unwrap_or_default().split(',') {
        let part = part.trim();
        if !part.is_empty() && !keys.iter().any(|k| k == part) {
            keys.push(part.to_owned());
        }
    }
    keys
}

/// The accounts of a profile in the order of `mail.accountmanager.accounts`; see the module
/// documentation. Accounts whose server is missing (no `type`) and identities without an email
/// address are skipped.
pub fn thunderbird_accounts(prefs: &Prefs) -> Vec<TbAccount> {
    let mut accounts = Vec::new();
    for key in key_list(prefs, "mail.accountmanager.accounts") {
        if accounts.len() >= MAX_ACCOUNTS {
            break;
        }
        let Some(server_key) = text_pref(prefs, &format!("mail.account.{key}.server"))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let Some(server) = server(prefs, server_key) else {
            continue;
        };
        let identities = key_list(prefs, &format!("mail.account.{key}.identities"))
            .into_iter()
            .filter_map(|id| identity(prefs, &id))
            .take(MAX_IDENTITIES)
            .collect();
        accounts.push(TbAccount {
            key,
            server,
            identities,
        });
    }
    accounts
}

fn server(prefs: &Prefs, key: &str) -> Option<TbServer> {
    let pref = |name: &str| format!("mail.server.{key}.{name}");
    let kind = match text_pref(prefs, &pref("type"))?.trim() {
        "imap" => TbServerKind::Imap,
        "pop3" => TbServerKind::Pop3,
        "none" => TbServerKind::Local,
        _ => TbServerKind::Other,
    };
    let security = match int_pref(prefs, &pref("socketType")) {
        Some(2) => TbSecurity::StartTls,
        Some(3) => TbSecurity::Tls,
        _ => TbSecurity::None,
    };
    let directory = [pref("directory-rel"), pref("directory")]
        .iter()
        .map(|k| string_pref(prefs, k))
        .find(|d| !d.is_empty());
    Some(TbServer {
        key: key.to_owned(),
        kind,
        host: string_pref(prefs, &pref("hostname")),
        port: port_pref(prefs, &pref("port")),
        security,
        user_name: string_pref(prefs, &pref("userName")),
        name: string_pref(prefs, &pref("name")),
        directory,
    })
}

fn smtp(prefs: &Prefs, key: &str) -> Option<TbSmtp> {
    let pref = |name: &str| format!("mail.smtpserver.{key}.{name}");
    let host = string_pref(prefs, &pref("hostname"));
    if host.is_empty() {
        return None;
    }
    let security = match int_pref(prefs, &pref("try_ssl")) {
        Some(1 | 2) => TbSecurity::StartTls,
        Some(3) => TbSecurity::Tls,
        _ => TbSecurity::None,
    };
    Some(TbSmtp {
        key: key.to_owned(),
        host,
        port: port_pref(prefs, &pref("port")),
        security,
        user_name: string_pref(prefs, &pref("username")),
    })
}

fn identity(prefs: &Prefs, key: &str) -> Option<TbIdentity> {
    let pref = |name: &str| format!("mail.identity.{key}.{name}");
    let email = string_pref(prefs, &pref("useremail"));
    if email.is_empty() {
        return None;
    }
    let reply_to = Some(string_pref(prefs, &pref("reply_to"))).filter(|r| !r.is_empty());
    let file = bool_pref(prefs, &pref("attach_signature"))
        .then(|| {
            [pref("sig_file-rel"), pref("sig_file")]
                .iter()
                .map(|k| string_pref(prefs, k))
                .find(|p| !p.is_empty())
        })
        .flatten();
    let signature = match file {
        Some(path) => {
            let lower = path.to_ascii_lowercase();
            let html = lower.ends_with(".htm") || lower.ends_with(".html");
            TbSignature::File { path, html }
        }
        None => {
            let text = text_pref(prefs, &pref("htmlSigText")).unwrap_or_default();
            if text.trim().is_empty() {
                TbSignature::None
            } else if bool_pref(prefs, &pref("htmlSigFormat")) {
                TbSignature::Html(text.to_owned())
            } else {
                TbSignature::Text(text.to_owned())
            }
        }
    };
    let own_smtp = Some(string_pref(prefs, &pref("smtpServer"))).filter(|s| !s.is_empty());
    let default_smtp =
        Some(string_pref(prefs, "mail.smtp.defaultserver")).filter(|s| !s.is_empty());
    let first_smtp = key_list(prefs, "mail.smtpservers").into_iter().next();
    let smtp = [own_smtp, default_smtp, first_smtp]
        .into_iter()
        .flatten()
        .find_map(|k| smtp(prefs, &k));
    Some(TbIdentity {
        key: key.to_owned(),
        email,
        full_name: string_pref(prefs, &pref("fullName")),
        reply_to,
        signature,
        smtp,
    })
}
