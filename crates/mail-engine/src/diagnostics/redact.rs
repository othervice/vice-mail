//! Defensive redaction of log lines for the diagnostics export.
//!
//! The logs are meant to be free of secrets and personal data already ([`mail_model::Secret`]
//! prints `[redacted]`, events carry ids and codes). This is the second line: before a line
//! leaves the computer in a diagnostics file, everything that looks like a secret or like
//! personal data is replaced with a placeholder. It errs on the side of removing too much.
//!
//! In this order:
//! 1. control characters are dropped, the home directory becomes `~`;
//! 2. strings the app knows to be personal (the accounts' addresses, names, user names and
//!    server names, the identities, folder names) become placeholders, whole words, ASCII case
//!    ignored;
//! 3. a scanner replaces: values of secret-like fields (`password=…`, `token: …`, `"secret":"…"`)
//!    and of personal or path fields (`subject=…`, `path=…`); the word after `Bearer`/`Basic`;
//!    file paths in free text (`~/…`, `/Users/…`, `C:\…`, to the next field or the line's end),
//!    as `~/…` under the home directory and `<path>` elsewhere, without the file name;
//!    email addresses; known token formats (JWT, `sk-…`, `ghp_…`, `AGE-SECRET-KEY-…`, …); IPv4
//!    addresses other than loopback; host names other than a short list of known provider and
//!    app hosts; long opaque strings of letters and digits (24 characters or more);
//! 4. the system's user name becomes `<user>` (whole words);
//! 5. a line is cut after [`MAX_LINE_CHARS`] characters.
//!
//! Over-redaction is accepted: a setting key such as `ui.theme` looks like a host name and
//! becomes `<host>`.

/// Longest line kept, in characters (after redaction).
pub(crate) const MAX_LINE_CHARS: usize = 2_000;

/// Shortest known personal string that is replaced; shorter ones would hit ordinary words.
const MIN_KNOWN_CHARS: usize = 3;

/// Shortest run of letters and digits taken for an opaque token.
const MIN_TOKEN_CHARS: usize = 24;

pub(crate) const REDACTED: &str = "[redacted]";
pub(crate) const EMAIL: &str = "<email>";
pub(crate) const HOST: &str = "<host>";
pub(crate) const IP: &str = "<ip>";
pub(crate) const PATH: &str = "<path>";
/// A path under the home directory: only that it was there stays.
pub(crate) const HOME_PATH: &str = "~/…";
pub(crate) const REMOVED: &str = "<removed>";
pub(crate) const USER: &str = "<user>";

/// Field names whose value is a secret (lowercase; a name containing one of these counts).
const SECRET_FIELD_PARTS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "authorization",
    "cookie",
    "private_key",
    "privatekey",
    "bearer",
];

/// Field names whose value is a secret (lowercase, exact).
const SECRET_FIELDS: &[&str] = &["pwd", "pass", "auth", "sig", "signature"];

/// Field names whose value is personal (lowercase, exact). Numeric values (ids, versions) stay.
const PERSONAL_FIELDS: &[&str] = &[
    "subject",
    "from",
    "to",
    "cc",
    "bcc",
    "sender",
    "recipient",
    "recipients",
    "email",
    "address",
    "addr",
    "name",
    "display_name",
    "user",
    "username",
    "user_name",
    "login",
    "folder",
    "mailbox_name",
    "filename",
    "file_name",
    "attachment",
    "body",
    "text",
    "preview",
    "snippet",
];

/// Field names whose value is a file system path (lowercase, exact; also `*_path`, `*_dir`).
const PATH_FIELDS: &[&str] = &["path", "dir", "directory", "file", "cwd", "home"];

/// Prefixes of well-known secret formats, with the shortest whole token they need.
const SECRET_PREFIXES: &[(&str, usize)] = &[
    ("AGE-SECRET-KEY-", 20),
    ("github_pat_", 20),
    ("pk_live_", 16),
    ("sk_live_", 16),
    ("rk_live_", 16),
    ("ghp_", 12),
    ("gho_", 12),
    ("ghs_", 12),
    ("ghu_", 12),
    ("ghr_", 12),
    ("glpat-", 12),
    ("xoxb-", 12),
    ("xoxp-", 12),
    ("xoxa-", 12),
    ("xoxs-", 12),
    ("ya29.", 12),
    ("AKIA", 16),
    ("AIza", 24),
    ("sk-", 8),
    ("sk_", 8),
];

/// Last labels that look like a top-level domain but name files in our own logs.
const FILE_SUFFIXES: &[&str] = &["log", "rs", "toml", "json", "lock", "tmp", "part", "txt"];

/// What [`Redactor::line`] replaces, set up once per export.
#[derive(Debug, Default)]
pub(crate) struct Redactor {
    /// Known personal strings and their placeholders, longest first.
    known: Vec<(String, &'static str)>,
    /// Hosts that are not personal (lowercase): provider servers, the update host, the app id.
    allowed_hosts: Vec<String>,
    /// The user's home directory as it appears in paths.
    home: Vec<String>,
    /// The system's user name.
    user: Option<String>,
}

impl Redactor {
    pub(crate) fn new(home: Option<&str>, user: Option<&str>) -> Self {
        let mut home_forms = Vec::new();
        if let Some(home) = home.map(str::trim).filter(|h| h.len() > 1) {
            let home = home.trim_end_matches(['/', '\\']).to_owned();
            if home.contains('\\') {
                // Debug output doubles backslashes.
                home_forms.push(home.replace('\\', "\\\\"));
            }
            home_forms.push(home);
        }
        Self {
            known: Vec::new(),
            allowed_hosts: Vec::new(),
            home: home_forms,
            user: user
                .map(str::trim)
                .filter(|u| u.chars().count() >= MIN_KNOWN_CHARS)
                .map(str::to_owned),
        }
    }

    /// A host that may stay in the file (lowercase compare).
    pub(crate) fn allow_host(&mut self, host: &str) {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if !host.is_empty() && !self.allowed_hosts.contains(&host) {
            self.allowed_hosts.push(host);
        }
    }

    pub(crate) fn is_allowed_host(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.');
        host.eq_ignore_ascii_case("localhost")
            || host.to_ascii_lowercase().ends_with(".localhost")
            || self
                .allowed_hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(host))
    }

    /// A string the app knows to be personal; replaced by `placeholder` wherever it appears as
    /// a whole word. Too short strings and allowed hosts are ignored.
    pub(crate) fn add_known(&mut self, value: &str, placeholder: &'static str) {
        let value = value.trim();
        if value.chars().count() < MIN_KNOWN_CHARS || self.is_allowed_host(value) {
            return;
        }
        if self
            .known
            .iter()
            .any(|(known, _)| known.eq_ignore_ascii_case(value))
        {
            return;
        }
        self.known.push((value.to_owned(), placeholder));
        self.known
            .sort_by_key(|(known, _)| std::cmp::Reverse(known.len()));
    }

    /// One of the app's own directories for the report's header: the home directory becomes
    /// `~` and known personal strings their placeholders, the rest (folder names the app chose)
    /// stays readable.
    pub(crate) fn app_path(&self, path: &str) -> String {
        let mut text: String = path.chars().filter(|c| !c.is_control()).collect();
        for home in &self.home {
            text = replace_ascii_ci(&text, home, "~", false);
        }
        for (known, placeholder) in &self.known {
            text = replace_ascii_ci(&text, known, placeholder, true);
        }
        if let Some(user) = &self.user {
            text = replace_ascii_ci(&text, user, USER, true);
        }
        text
    }

    /// The redacted line, at most [`MAX_LINE_CHARS`] characters (plus a cut mark).
    pub(crate) fn line(&self, line: &str) -> String {
        let mut text: String = line
            .chars()
            .filter(|c| *c == '\t' || !c.is_control())
            .collect();
        for home in &self.home {
            text = replace_ascii_ci(&text, home, "~", false);
        }
        for (known, placeholder) in &self.known {
            text = replace_ascii_ci(&text, known, placeholder, true);
        }
        let mut text = self.scan(&text);
        // After the scanner, so an address that contains the user name is still recognised.
        if let Some(user) = &self.user {
            text = replace_ascii_ci(&text, user, USER, true);
        }
        if text.chars().count() > MAX_LINE_CHARS {
            let mut cut: String = text.chars().take(MAX_LINE_CHARS).collect();
            cut.push_str(" …[cut]");
            cut
        } else {
            text
        }
    }

    fn scan(&self, line: &str) -> String {
        let mut out = String::with_capacity(line.len());
        let mut i = 0;
        while i < line.len() {
            let prev = line.get(..i).and_then(|head| head.chars().next_back());
            if let Some((len, replacement)) = self.match_at(line, i, prev) {
                out.push_str(&replacement);
                i += len.max(1);
                continue;
            }
            let Some(c) = line.get(i..).and_then(|rest| rest.chars().next()) else {
                break;
            };
            out.push(c);
            i += c.len_utf8();
        }
        out
    }

    /// The rule that matches at byte `i`: how many bytes it covers and what replaces them.
    fn match_at(&self, line: &str, i: usize, prev: Option<char>) -> Option<(usize, String)> {
        let rest = line.get(i..)?;
        field_value(rest, prev)
            .or_else(|| auth_scheme(rest, prev))
            .or_else(|| free_path(rest, prev))
            .or_else(|| email(rest, prev))
            .or_else(|| prefixed_secret(rest, prev))
            .or_else(|| ipv4(rest, prev))
            .or_else(|| self.host(rest, prev))
            .or_else(|| opaque_token(rest, prev))
    }

    fn host(&self, rest: &str, prev: Option<char>) -> Option<(usize, String)> {
        if prev.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '@')) {
            return None;
        }
        let run = run_len(rest, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')
        });
        let candidate = rest.get(..run)?.trim_end_matches(['.', '-']);
        if !is_host(candidate) || self.is_allowed_host(candidate) {
            return None;
        }
        Some((candidate.len(), HOST.to_owned()))
    }
}

/// Byte length of the run of characters from the start of `s` that satisfy `class`.
fn run_len(s: &str, class: impl Fn(char) -> bool) -> usize {
    s.char_indices()
        .find(|(_, c)| !class(*c))
        .map_or(s.len(), |(at, _)| at)
}

/// `key=value`, `key: value`, `"key":"value"` with a secret, personal or path key.
fn field_value(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.')) {
        return None;
    }
    let key_len = run_len(rest, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-')
    });
    if key_len == 0 {
        return None;
    }
    let key = rest.get(..key_len)?;
    let kind = field_kind(&key.to_ascii_lowercase())?;
    let mut at = key_len;
    let after = rest.get(at..)?;
    let quoted_key = prev == Some('"') && after.starts_with('"');
    if quoted_key {
        at += 1;
    }
    let sep = rest.get(at..)?.chars().next()?;
    if sep != '=' && sep != ':' {
        return None;
    }
    at += 1;
    if sep == ':' {
        at += run_len(rest.get(at..)?, |c| c == ' ');
    }
    let value_start = at;
    let value = rest.get(value_start..)?;
    let value_len = if value.starts_with('"') {
        quoted_len(value)
    } else if kind == FieldKind::Path {
        // Paths may hold spaces: up to the next ` field=` of the log line, or the end.
        next_field(value).unwrap_or(value.len())
    } else if let Some((len, _)) = auth_scheme(value, None).filter(|_| kind == FieldKind::Secret) {
        // `Authorization: Bearer xyz`: the scheme and the credential.
        len
    } else {
        run_len(value, |c| {
            !c.is_whitespace() && !matches!(c, ',' | ';' | '}' | ')' | ']' | '&' | '"')
        })
    };
    if value_len == 0 {
        return None;
    }
    let raw = value.get(..value_len)?;
    let bare = raw.trim_matches('"');
    if kind != FieldKind::Secret && (is_number(bare) || is_uuid(bare) || bare == "None") {
        return None;
    }
    if bare == REDACTED {
        return None;
    }
    let placeholder = match kind {
        FieldKind::Secret => REDACTED,
        FieldKind::Personal => REMOVED,
        FieldKind::Path => path_placeholder(bare),
    };
    let head = rest.get(..value_start)?;
    Some((value_start + value_len, format!("{head}{placeholder}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Secret,
    Personal,
    Path,
}

fn field_kind(key: &str) -> Option<FieldKind> {
    if SECRET_FIELDS.contains(&key) || SECRET_FIELD_PARTS.iter().any(|part| key.contains(part)) {
        Some(FieldKind::Secret)
    } else if PERSONAL_FIELDS.contains(&key) {
        Some(FieldKind::Personal)
    } else if PATH_FIELDS.contains(&key) || key.ends_with("_path") || key.ends_with("_dir") {
        Some(FieldKind::Path)
    } else {
        None
    }
}

/// Length of a double-quoted value with backslash escapes, quotes included (to the end of the
/// line if it never closes).
fn quoted_len(value: &str) -> usize {
    let mut escaped = false;
    for (at, c) in value.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return at + 1,
            _ => {}
        }
    }
    value.len()
}

/// Where the next ` name=` field of a log line starts in `value` (at its space).
fn next_field(value: &str) -> Option<usize> {
    value.char_indices().find_map(|(at, c)| {
        if c != ' ' {
            return None;
        }
        let tail = value.get(at + 1..)?;
        let name = run_len(tail, |c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
        (name > 0 && tail.get(name..)?.starts_with('=')).then_some(at)
    })
}

/// A file path in free text (an error text, a message): `~/…`, `/Users/…` (an absolute path
/// with two parts or more), `C:\…`, `\\server\…`. Paths may hold spaces, so it reaches to the
/// next ` field=` of the log line, a closing quote, or the end.
fn free_path(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(|c| {
        !c.is_whitespace() && !matches!(c, '=' | '(' | '[' | '"' | '\'' | ':' | '<' | ',')
    }) {
        return None;
    }
    let bytes = rest.as_bytes();
    let starts = match bytes {
        [b'~', b'/' | b'\\', ..] => true,
        [b'\\', b'\\', c, ..] => c.is_ascii_alphanumeric(),
        [drive, b':', b'\\' | b'/', ..] => drive.is_ascii_alphabetic(),
        [b'/', first, ..] if first.is_ascii_alphanumeric() => {
            let segment = run_len(rest.get(1..)?, |c| {
                c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
            });
            rest.get(1 + segment..)?.starts_with('/')
        }
        _ => false,
    };
    if !starts {
        return None;
    }
    let mut end = next_field(rest).unwrap_or(rest.len());
    if matches!(prev, Some('"' | '\'')) {
        let quote = prev.unwrap_or('"');
        if let Some(close) = rest.find(quote).filter(|at| *at < end) {
            end = close;
        }
    }
    Some((end, path_placeholder(rest).to_owned()))
}

/// `~/…` for a path under the home directory (already shortened to `~`), `<path>` for any
/// other. Not even the file name stays: it can be an attachment's name.
fn path_placeholder(path: &str) -> &'static str {
    if path.trim_start_matches(['"', '\'']).starts_with('~') {
        HOME_PATH
    } else {
        PATH
    }
}

/// `Bearer xyz`, `Basic xyz`: the credential after the scheme.
fn auth_scheme(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(char::is_alphanumeric) {
        return None;
    }
    let word = run_len(rest, |c| c.is_ascii_alphabetic());
    let scheme = rest.get(..word)?;
    if !scheme.eq_ignore_ascii_case("bearer") && !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let spaces = run_len(rest.get(word..)?, |c| c == ' ');
    if spaces == 0 {
        return None;
    }
    let credential = run_len(rest.get(word + spaces..)?, |c| {
        !c.is_whitespace() && !matches!(c, ',' | ';' | '"')
    });
    if credential == 0 {
        return None;
    }
    Some((word + spaces + credential, format!("{scheme} {REDACTED}")))
}

fn is_local_part_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-' | '\'')
}

/// An email address: local part, `@`, a domain with a dot and an alphabetic last label.
fn email(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(is_local_part_char) {
        return None;
    }
    let local = run_len(rest, is_local_part_char);
    if local == 0 || !rest.get(local..)?.starts_with('@') {
        return None;
    }
    let domain_start = local + 1;
    let domain_run = run_len(rest.get(domain_start..)?, |c| {
        c.is_alphanumeric() || matches!(c, '.' | '-')
    });
    let domain = rest
        .get(domain_start..domain_start + domain_run)?
        .trim_end_matches(['.', '-']);
    let (_, last) = domain.rsplit_once('.')?;
    if last.chars().count() < 2 || !last.chars().all(char::is_alphabetic) {
        return None;
    }
    Some((domain_start + domain.len(), EMAIL.to_owned()))
}

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '.' | '~' | '-')
}

/// Well-known token formats: a JWT, or a known prefix with enough characters after it.
fn prefixed_secret(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')) {
        return None;
    }
    let run = run_len(rest, is_token_char);
    let token = rest.get(..run)?.trim_end_matches('.');
    let jwt = token.starts_with("eyJ") && token.matches('.').count() >= 2 && token.len() >= 20;
    let prefixed = SECRET_PREFIXES
        .iter()
        .any(|(prefix, min)| token.starts_with(prefix) && token.len() >= *min);
    (jwt || prefixed).then(|| (token.len(), REDACTED.to_owned()))
}

/// An IPv4 address other than loopback or 0.0.0.0.
fn ipv4(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(|c| c.is_ascii_alphanumeric() || c == '.') {
        return None;
    }
    let run = run_len(rest, |c| c.is_ascii_digit() || c == '.');
    let candidate = rest.get(..run)?.trim_end_matches('.');
    let after = rest.get(candidate.len()..)?.chars().next();
    if after.is_some_and(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let parts: Vec<&str> = candidate.split('.').collect();
    if parts.len() != 4
        || !parts
            .iter()
            .all(|p| (1..=3).contains(&p.len()) && p.parse::<u16>().is_ok_and(|n| n <= 255))
    {
        return None;
    }
    if candidate.starts_with("127.") || candidate == "0.0.0.0" {
        return None;
    }
    Some((candidate.len(), IP.to_owned()))
}

/// A DNS name: two or more labels of letters, digits and hyphens, with an alphabetic top-level
/// label (or an IDN one) that is not one of the file suffixes of our own logs.
fn is_host(candidate: &str) -> bool {
    let labels: Vec<&str> = candidate.split('.').collect();
    if labels.len() < 2 || candidate.len() > 253 {
        return false;
    }
    let label_ok = |label: &&str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if !labels.iter().all(label_ok) {
        return false;
    }
    let Some(tld) = labels.last() else {
        return false;
    };
    let alphabetic = (2..=24).contains(&tld.len()) && tld.bytes().all(|b| b.is_ascii_alphabetic());
    let idn = tld.to_ascii_lowercase().starts_with("xn--");
    (alphabetic || idn) && !FILE_SUFFIXES.contains(&tld.to_ascii_lowercase().as_str())
}

fn is_opaque_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '-' | '~')
}

/// A long run of letters and digits (both), not a UUID: an API key, a hash, a session id.
/// Base64 padding (`=`) at its end goes with it.
fn opaque_token(rest: &str, prev: Option<char>) -> Option<(usize, String)> {
    if prev.is_some_and(is_opaque_char) {
        return None;
    }
    let run = run_len(rest, is_opaque_char);
    let core = rest.get(..run)?;
    if core.len() < MIN_TOKEN_CHARS
        || is_uuid(core)
        || !core.bytes().any(|b| b.is_ascii_digit())
        || !core.bytes().any(|b| b.is_ascii_alphabetic())
    {
        return None;
    }
    let padding = run_len(rest.get(run..)?, |c| c == '=').min(2);
    Some((run + padding, REDACTED.to_owned()))
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// The canonical UUID form (8-4-4-4-12 hex digits): account ids stay readable.
pub(crate) fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, len)| g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Replaces every occurrence of `needle` (ASCII case ignored) in `text`; with `whole_word`
/// only where it is not part of a longer word.
fn replace_ascii_ci(text: &str, needle: &str, with: &str, whole_word: bool) -> String {
    let (hay, pin) = (text.as_bytes(), needle.as_bytes());
    if pin.is_empty() || pin.len() > hay.len() {
        return text.to_owned();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::with_capacity(text.len());
    let (mut last, mut i) = (0, 0);
    while i + pin.len() <= hay.len() {
        let end = i + pin.len();
        let hit = text.is_char_boundary(i)
            && text.is_char_boundary(end)
            && hay.get(i..end).is_some_and(|w| w.eq_ignore_ascii_case(pin))
            && (!whole_word
                || (!word(text.get(..i).and_then(|h| h.chars().next_back()))
                    && !word(text.get(end..).and_then(|t| t.chars().next()))));
        if hit {
            out.push_str(text.get(last..i).unwrap_or_default());
            out.push_str(with);
            last = end;
            i = end;
        } else {
            i += 1;
        }
    }
    out.push_str(text.get(last..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        let mut r = Redactor::new(Some("/Users/ada"), Some("ada"));
        r.allow_host("imap.gmail.com");
        r.allow_host("github.com");
        r.allow_host("org.vicemail.desktop");
        r.add_known("Ada Lovelace", REMOVED);
        r.add_known("ada.l", REMOVED);
        r.add_known("mail.lovelace.example", HOST);
        r.add_known("Steuer 2026", "<folder>");
        r
    }

    #[test]
    fn addresses_hosts_and_ips_become_placeholders() {
        let r = redactor();
        assert_eq!(
            r.line("an account for ada@lovelace.example exists already"),
            "an account for <email> exists already"
        );
        assert_eq!(
            r.line("to: \"Ada Lovelace\" <ada.lovelace+news@mail.example.co.uk>."),
            "to: <removed> <<email>>."
        );
        assert_eq!(
            r.line("link opened in the system scheme=https host=tracker.newsletter.example"),
            "link opened in the system scheme=https host=<host>"
        );
        assert_eq!(
            r.line("connected to imap.gmail.com:993 and smtp.lovelace.example"),
            "connected to imap.gmail.com:993 and <host>"
        );
        assert_eq!(
            r.line("dns failed for mail.lovelace.example (192.168.10.20), 127.0.0.1 ok"),
            "dns failed for <host> (<ip>), 127.0.0.1 ok"
        );
        assert_eq!(
            r.line("user ada.l logged in as ada"),
            "user <removed> logged in as <user>"
        );
        assert_eq!(
            r.line("mailbox not found: Steuer 2026"),
            "mailbox not found: <folder>"
        );
    }

    #[test]
    fn secrets_in_fields_headers_and_token_formats_are_removed() {
        let r = redactor();
        for (input, leaked) in [
            ("login password=hunter2-correct-horse ok", "hunter2"),
            ("Authorization: Bearer abc.def.ghi-jkl", "abc.def"),
            (
                "header \"authorization\":\"Basic YWxhZGRpbjpvcGVuc2VzYW1l\"",
                "YWxh",
            ),
            ("{\"api_key\":\"sk-test-api-key-0815\"}", "0815"),
            (
                "got token sk-proj-1234567890abcdef from env",
                "1234567890abcdef",
            ),
            (
                "jwt eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl here",
                "eyJzdWIi",
            ),
            ("key AGE-SECRET-KEY-1QQQQQQQQQQQQQQQQQQQQQQQQQQ", "1QQQQ"),
            ("refresh_token=1//0gAbCdEf-GhIjKlMnOpQr&x=1", "0gAbCdEf"),
            (
                "opaque 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08 end",
                "9f86d0",
            ),
            ("cookie: session=abc123", "abc123"),
        ] {
            let out = r.line(input);
            assert!(!out.contains(leaked), "{input:?} → {out:?}");
            assert!(out.contains(REDACTED), "{input:?} → {out:?}");
        }
    }

    #[test]
    fn paths_and_personal_fields_are_removed_but_ids_and_numbers_stay() {
        let r = redactor();
        assert_eq!(
            r.line("WARN copy could not be removed path=/Users/ada/Library/Caches/org.vicemail.desktop/attachments/Rechnung März.pdf err=denied"),
            "WARN copy could not be removed path=~/… err=denied"
        );
        assert_eq!(
            r.line("database ready schema_version=15 migrated_from=14 backup=None"),
            "database ready schema_version=15 migrated_from=14 backup=None"
        );
        assert_eq!(
            r.line("database migrated from=14 to=15"),
            "database migrated from=14 to=15"
        );
        assert_eq!(
            r.line("subject=\"Rechnung März\" account=00000000-0000-4000-8000-000000000001"),
            "subject=<removed> account=00000000-0000-4000-8000-000000000001"
        );
        assert_eq!(
            r.line("error=file not found: /Users/ada/Documents/Rechnung März.pdf code=NotFound"),
            "error=file not found: ~/… code=NotFound"
        );
        assert_eq!(
            r.line("drop refused: \"C:\\Users\\x\\Steuer 2025.pdf\" (too large)"),
            "drop refused: \"<path>\" (too large)"
        );
        assert_eq!(r.line("opened ~/x and more"), "opened ~/…");
        assert_eq!(
            r.line("copy failed path=/private/var/folders/T/attachments/Rechnung.pdf kind=io"),
            "copy failed path=<path> kind=io"
        );
        assert_eq!(
            r.app_path("/Users/ada/Library/Application Support/org.vicemail.desktop"),
            "~/Library/Application Support/org.vicemail.desktop"
        );
        for kept in [
            "Mozilla/5.0 text/plain",
            "see https://github.com/othervice/x",
            "a / b",
        ] {
            assert_eq!(r.line(kept), kept);
        }
    }

    #[test]
    fn ordinary_log_lines_stay_readable() {
        let r = redactor();
        for line in [
            "2026-11-23T10:15:30.123456Z  INFO main mail_engine::core: database ready schema_version=15",
            "2026-11-23T10:15:31.000001Z  WARN tokio-runtime-worker mail_app::commands: command failed command=open_link code=Internal error=internal error: the frame is gone",
            "sync pass done account=00000000-0000-4000-8000-0000000000aa mailboxes=12 took_ms=840",
            "starting version=0.1.0 platform=macos",
            "update check failed host=github.com status=404",
            "log file app.2026-11-23.log rotated, see core.rs:120",
        ] {
            assert_eq!(r.line(line), line);
        }
    }

    #[test]
    fn long_lines_are_cut_and_control_characters_dropped() {
        let r = Redactor::new(None, None);
        let out = r.line(&"ab ".repeat(2_000));
        assert!(out.ends_with(" …[cut]"));
        assert_eq!(
            out.chars().count(),
            MAX_LINE_CHARS + " …[cut]".chars().count()
        );
        assert_eq!(r.line("a\u{1b}[31mred\u{7}"), "a[31mred");
    }

    #[test]
    fn case_insensitive_whole_word_replacement() {
        assert_eq!(
            replace_ascii_ci("Ada ada ADA adam", "ada", "X", true),
            "X X X adam"
        );
        assert_eq!(
            replace_ascii_ci("C:\\Users\\Ada\\x", "c:\\users\\ada", "~", false),
            "~\\x"
        );
        assert_eq!(replace_ascii_ci("Grüße ada", "ada", "X", true), "Grüße X");
    }
}
