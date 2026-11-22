//! Link checks before a link leaves the app (architecture, security boundary 5): only `http`,
//! `https` and `mailto` open; warnings when the visible text names another domain than the
//! target, or the target domain is international, mixes scripts or imitates Latin letters.

use icu_properties::CodePointMapData;
use icu_properties::props::Script;
use url::{Host, Url};

/// What the core knows about a link before it may be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkCheck {
    /// The normalised URL to hand to the system browser or mail composer. `None` when the link
    /// must not be opened (scheme not allowed, or not a valid URL).
    pub open_url: Option<String>,
    /// Lower-case scheme (`https`, `mailto`, `javascript`, …); empty for an invalid URL.
    pub scheme: String,
    /// The host of an `http(s)` link in Unicode form (punycode decoded), `None` otherwise.
    pub host: Option<String>,
    /// The real target as shown to the user before opening (with the host in its ASCII form,
    /// as the network sees it).
    pub display: String,
    /// Everything suspicious; empty for an ordinary link.
    pub warnings: Vec<LinkWarning>,
}

impl LinkCheck {
    /// Whether the link can be opened at all.
    pub fn is_openable(&self) -> bool {
        self.open_url.is_some()
    }
}

/// One reason to warn before opening a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkWarning {
    /// Not `http`, `https` or `mailto` (for example `javascript:`, `data:`, `file:`, custom
    /// schemes). Never opened.
    Blocked { scheme: String },
    /// Not a valid absolute URL (relative links have no meaning outside the mail). Never opened.
    Invalid,
    /// The visible text names another domain than the target.
    TextNamesOtherDomain {
        text_domain: String,
        target_domain: String,
    },
    /// The target host contains international characters (punycode `xn--`).
    InternationalDomain { ascii: String, unicode: String },
    /// A label of the host mixes writing systems (for example Latin and Cyrillic).
    MixedScripts { domain: String },
    /// The host imitates Latin letters with look-alike characters (`аррӏе.com`).
    Confusable { domain: String },
    /// The URL carries user info before an `@` (`https://bank.example@evil.example/`); the real
    /// host is the part after it.
    UserInfo { host: String },
    /// The host is a numeric IP address.
    IpAddress { host: String },
}

/// Checks a link: `text` is what the mail shows, `href` the real target.
pub fn check_link(text: &str, href: &str) -> LinkCheck {
    let href = href.trim();
    let display: String = href.chars().take(2000).collect();
    let invalid = |display: String| LinkCheck {
        open_url: None,
        scheme: String::new(),
        host: None,
        display,
        warnings: vec![LinkWarning::Invalid],
    };
    let Ok(url) = Url::parse(href) else {
        return invalid(display);
    };
    let mut host = None;
    let mut warnings = Vec::new();
    let target_domains: Vec<String> = match url.scheme() {
        "http" | "https" => match url.host() {
            Some(Host::Domain(domain)) => {
                host = Some(unicode_domain(domain));
                domain_warnings(domain, &mut warnings);
                if !url.username().is_empty() || url.password().is_some() {
                    warnings.push(LinkWarning::UserInfo {
                        host: domain.to_owned(),
                    });
                }
                vec![domain.to_owned()]
            }
            Some(Host::Ipv4(ip)) => {
                host = Some(ip.to_string());
                warnings.push(LinkWarning::IpAddress {
                    host: ip.to_string(),
                });
                vec![ip.to_string()]
            }
            Some(Host::Ipv6(ip)) => {
                host = Some(format!("[{ip}]"));
                warnings.push(LinkWarning::IpAddress {
                    host: format!("[{ip}]"),
                });
                vec![format!("[{ip}]")]
            }
            None => return invalid(display),
        },
        "mailto" => {
            let mut domains = Vec::new();
            for address in percent_decoded(url.path()).split(',') {
                if let Some((_, domain)) = address.trim().rsplit_once('@') {
                    let domain = ascii_domain(domain).unwrap_or_else(|| domain.to_lowercase());
                    domain_warnings(&domain, &mut warnings);
                    domains.push(domain);
                }
            }
            domains
        }
        other => {
            return LinkCheck {
                open_url: None,
                scheme: other.to_owned(),
                host: None,
                display,
                warnings: vec![LinkWarning::Blocked {
                    scheme: other.to_owned(),
                }],
            };
        }
    };

    if let Some(text_domain) = domain_in_text(text) {
        let matches = target_domains
            .iter()
            .any(|target| same_site(&text_domain, target));
        if !matches && let Some(target) = target_domains.first() {
            warnings.push(LinkWarning::TextNamesOtherDomain {
                text_domain,
                target_domain: target.clone(),
            });
        }
    }

    LinkCheck {
        open_url: Some(url.as_str().to_owned()),
        scheme: url.scheme().to_owned(),
        host,
        display: url.as_str().chars().take(2000).collect(),
        warnings,
    }
}

/// A host with its punycode labels decoded.
fn unicode_domain(ascii: &str) -> String {
    ascii
        .split('.')
        .map(|label| match label.strip_prefix("xn--") {
            Some(encoded) => decode_punycode(encoded).unwrap_or_else(|| label.to_owned()),
            None => label.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// Same domain, or one is a subdomain of the other, ignoring a leading `www.`.
fn same_site(a: &str, b: &str) -> bool {
    let a = a.strip_prefix("www.").unwrap_or(a);
    let b = b.strip_prefix("www.").unwrap_or(b);
    a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
}

/// The host of `domain` in ASCII (punycode) form, as the url crate normalises it (UTS 46:
/// lower case, full-width letters mapped).
fn ascii_domain(domain: &str) -> Option<String> {
    let url = Url::parse(&format!("http://{domain}/")).ok()?;
    match url.host()? {
        Host::Domain(d) => Some(d.to_owned()),
        Host::Ipv4(ip) => Some(ip.to_string()),
        Host::Ipv6(ip) => Some(format!("[{ip}]")),
    }
}

/// File extensions that look like top-level domains in link texts ("Download report.pdf").
const NOT_DOMAINS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "csv", "jpg", "jpeg", "png", "gif",
    "zip", "htm", "html", "exe", "mp3", "mp4", "mov", "ics", "eml", "odt", "ods", "rtf", "xml",
];

/// The first domain the link text names: in a URL, after `www.`, in an e-mail address, or the
/// whole text when it is a bare domain.
fn domain_in_text(text: &str) -> Option<String> {
    let trim = |s: &str| {
        s.trim_matches(|c: char| {
            matches!(
                c,
                '<' | '>' | '(' | ')' | '[' | ']' | '"' | '\'' | ',' | ';' | '.' | ':' | '!' | '?'
            )
        })
        .to_owned()
    };
    let tokens: Vec<String> = text.split_whitespace().map(trim).collect();
    for token in &tokens {
        let lower = token.to_lowercase();
        if let Some(pos) = lower.find("://") {
            let scheme = &lower[..pos];
            if scheme == "http" || scheme == "https" {
                if let Ok(url) = Url::parse(token)
                    && let Some(host) = url.host_str()
                {
                    return Some(host.to_owned());
                }
                continue;
            }
        }
        if lower.starts_with("www.") && lower.len() > 4 {
            let host = token.split(['/', '?', '#']).next().unwrap_or(token);
            if let Some(d) = ascii_domain(host) {
                return Some(d);
            }
        }
        if let Some((local, domain)) = token.rsplit_once('@')
            && !local.is_empty()
            && looks_like_domain(domain)
        {
            return ascii_domain(domain);
        }
    }
    if let [only] = tokens.as_slice() {
        let host = only.split(['/', '?', '#']).next().unwrap_or(only);
        if looks_like_domain(host) {
            return ascii_domain(host);
        }
    }
    None
}

fn looks_like_domain(text: &str) -> bool {
    let labels: Vec<&str> = text.split('.').collect();
    let Some(tld) = labels.last() else {
        return false;
    };
    labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && tld.chars().count() >= 2
        && tld.chars().all(char::is_alphabetic)
        && !NOT_DOMAINS.contains(&tld.to_lowercase().as_str())
}

/// Warnings about the host itself: punycode, mixed scripts, look-alikes.
fn domain_warnings(ascii: &str, warnings: &mut Vec<LinkWarning>) {
    let labels: Vec<String> = ascii
        .split('.')
        .map(|label| match label.strip_prefix("xn--") {
            Some(encoded) => decode_punycode(encoded).unwrap_or_else(|| label.to_owned()),
            None => label.to_owned(),
        })
        .collect();
    let unicode = labels.join(".");
    if unicode != ascii {
        warnings.push(LinkWarning::InternationalDomain {
            ascii: ascii.to_owned(),
            unicode: unicode.clone(),
        });
    }
    if labels.iter().any(|label| mixes_scripts(label)) {
        warnings.push(LinkWarning::MixedScripts {
            domain: unicode.clone(),
        });
    }
    if labels.iter().any(|label| imitates_latin(label)) {
        warnings.push(LinkWarning::Confusable { domain: unicode });
    }
}

/// More than one script in a label, except the combinations UTS 39 ("highly restrictive")
/// allows for Chinese, Japanese and Korean together with Latin.
fn mixes_scripts(label: &str) -> bool {
    let scripts = CodePointMapData::<Script>::new();
    let mut found: Vec<Script> = Vec::new();
    for ch in label.chars() {
        let script = scripts.get(ch);
        if script == Script::Common || script == Script::Inherited {
            continue;
        }
        if !found.contains(&script) {
            found.push(script);
        }
    }
    if found.len() <= 1 {
        return false;
    }
    let allowed: [&[Script]; 3] = [
        &[
            Script::Latin,
            Script::Han,
            Script::Hiragana,
            Script::Katakana,
        ],
        &[Script::Latin, Script::Han, Script::Bopomofo],
        &[Script::Latin, Script::Han, Script::Hangul],
    ];
    !allowed
        .iter()
        .any(|set| found.iter().all(|script| set.contains(script)))
}

/// Characters that look like Latin letters in common fonts (lower case only: hosts are
/// lower-cased before this check).
const LOOKALIKES: &[char] = &[
    // Cyrillic
    'а', 'в', 'е', 'к', 'м', 'н', 'о', 'р', 'с', 'т', 'у', 'х', 'ѕ', 'і', 'ї', 'ј', 'һ', 'ԁ', 'ԛ',
    'ԝ', 'ӏ', 'ү', 'ɡ', 'ь', 'ꙇ', // Greek
    'α', 'ο', 'ν', 'ρ', 'ι', 'κ', 'υ', 'χ', 'ϲ', 'ε', 'ω', 'τ', 'η', // Armenian
    'օ', 'ս', 'զ', 'հ', 'ո', 'ց', 'ա', // Latin look-alikes outside ASCII
    'ɑ', 'ı', 'ȷ', 'ɩ', 'ʟ', 'ɪ', 'ᴏ', 'ⅰ', 'ⅼ', 'ꞁ', 'ſ',
];

/// A label made only of ASCII letters/digits/hyphens and look-alikes, with at least one
/// look-alike: it reads as a Latin word but is not one.
fn imitates_latin(label: &str) -> bool {
    let mut has_lookalike = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            continue;
        }
        if LOOKALIKES.contains(&ch) {
            has_lookalike = true;
        } else {
            return false;
        }
    }
    has_lookalike
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        if byte == b'%'
            && let Some(value) = text
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            i += 3;
            continue;
        }
        out.push(byte);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// RFC 3492 punycode decoding of one label (without the `xn--` prefix).
pub(crate) fn decode_punycode(input: &str) -> Option<String> {
    const BASE: u32 = 36;
    const T_MIN: u32 = 1;
    const T_MAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;

    fn adapt(mut delta: u32, num_points: u32, first: bool) -> u32 {
        delta /= if first { DAMP } else { 2 };
        delta += delta / num_points;
        let mut k = 0;
        while delta > ((BASE - T_MIN) * T_MAX) / 2 {
            delta /= BASE - T_MIN;
            k += BASE;
        }
        k + (BASE - T_MIN + 1) * delta / (delta + SKEW)
    }

    if !input.is_ascii() || input.len() > 256 {
        return None;
    }
    let (basic, encoded) = match input.rfind('-') {
        Some(pos) => (&input[..pos], &input[pos + 1..]),
        None => ("", input),
    };
    let mut output: Vec<char> = basic.chars().collect();
    let mut n: u32 = 128;
    let mut i: u32 = 0;
    let mut bias: u32 = 72;
    let mut digits = encoded.bytes().peekable();
    while digits.peek().is_some() {
        let old_i = i;
        let mut weight: u32 = 1;
        let mut k = BASE;
        loop {
            let byte = digits.next()?;
            let digit = match byte {
                b'a'..=b'z' => u32::from(byte - b'a'),
                b'A'..=b'Z' => u32::from(byte - b'A'),
                b'0'..=b'9' => u32::from(byte - b'0') + 26,
                _ => return None,
            };
            i = i.checked_add(digit.checked_mul(weight)?)?;
            let t = if k <= bias {
                T_MIN
            } else if k >= bias + T_MAX {
                T_MAX
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
            weight = weight.checked_mul(BASE - t)?;
            k += BASE;
        }
        let len = u32::try_from(output.len()).ok()? + 1;
        bias = adapt(i - old_i, len, old_i == 0);
        n = n.checked_add(i / len)?;
        i %= len;
        let ch = char::from_u32(n)?;
        output.insert(usize::try_from(i).ok()?, ch);
        i += 1;
    }
    Some(output.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punycode_decodes_rfc_samples() {
        assert_eq!(decode_punycode("bnk-qla").as_deref(), Some("bänk"));
        assert_eq!(decode_punycode("bnk-sna").as_deref(), Some("bönk"));
        assert_eq!(decode_punycode("mnchen-3ya").as_deref(), Some("münchen"));
        assert_eq!(
            decode_punycode("ihqwcrb4cv8a8dqg056pqjye").as_deref(),
            Some("他们为什么不说中文")
        );
        assert_eq!(decode_punycode("80ak6aa92e").as_deref(), Some("аррӏе"));
        assert_eq!(decode_punycode("!!!"), None);
        assert_eq!(decode_punycode("99999999999999"), None);
    }

    #[test]
    fn only_web_and_mail_links_open() {
        let ok = check_link("Example", "https://example.com/a?b=1");
        assert_eq!(ok.open_url.as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(
            (ok.scheme.as_str(), ok.host.as_deref()),
            ("https", Some("example.com"))
        );
        assert!(ok.warnings.is_empty());
        assert!(check_link("x", "mailto:someone@example.test?subject=hi").is_openable());
        for (href, scheme) in [
            ("javascript:alert(1)", "javascript"),
            ("JavaScript:alert(1)", "javascript"),
            ("data:text/html,<script>alert(1)</script>", "data"),
            ("file:///etc/passwd", "file"),
            ("vmlink:42", "vmlink"),
            ("ftp://example.com/", "ftp"),
        ] {
            let check = check_link("x", href);
            assert_eq!(check.open_url, None, "{href}");
            assert_eq!(
                check.warnings,
                [LinkWarning::Blocked {
                    scheme: scheme.to_owned()
                }]
            );
        }
        assert_eq!(
            check_link("x", "/relative").warnings,
            [LinkWarning::Invalid]
        );
        assert_eq!(check_link("x", "#top").warnings, [LinkWarning::Invalid]);
    }

    #[test]
    fn text_naming_another_domain_warns() {
        let check = check_link("https://bank.example/login", "https://evil.example/login");
        assert!(check.warnings.contains(&LinkWarning::TextNamesOtherDomain {
            text_domain: "bank.example".into(),
            target_domain: "evil.example".into(),
        }));
        // Same site, subdomains and www. are fine.
        for (text, href) in [
            ("www.example.com", "https://example.com/x"),
            ("example.com", "https://news.example.com/x"),
            ("Visit https://shop.example.com", "https://example.com/"),
            ("Download report.pdf", "https://files.example/report.pdf"),
            ("Click here", "https://anything.example/"),
            ("support@bank.example", "mailto:support@bank.example"),
        ] {
            let check = check_link(text, href);
            assert!(
                !check
                    .warnings
                    .iter()
                    .any(|w| matches!(w, LinkWarning::TextNamesOtherDomain { .. })),
                "{text} -> {href}: {:?}",
                check.warnings
            );
        }
        let mail = check_link("support@bank.example", "mailto:evil@attacker.example");
        assert!(
            mail.warnings
                .iter()
                .any(|w| matches!(w, LinkWarning::TextNamesOtherDomain { .. }))
        );
    }

    #[test]
    fn suspicious_hosts_warn() {
        let idn = check_link("https://bänk.example/", "https://xn--bnk-qla.example/");
        assert_eq!(
            idn.warnings,
            [LinkWarning::InternationalDomain {
                ascii: "xn--bnk-qla.example".into(),
                unicode: "bänk.example".into(),
            }]
        );
        // The seed's phishing link: the text says bänk, the punycode target is bönk.
        let seed = check_link("https://bänk.example/", "https://xn--bnk-sna.example/");
        assert!(seed.warnings.contains(&LinkWarning::TextNamesOtherDomain {
            text_domain: "xn--bnk-qla.example".into(),
            target_domain: "xn--bnk-sna.example".into(),
        }));
        assert_eq!(idn.host.as_deref(), Some("bänk.example"));
        let apple = check_link("Apple", "https://xn--80ak6aa92e.com/");
        assert!(apple.warnings.contains(&LinkWarning::Confusable {
            domain: "аррӏе.com".into()
        }));
        // "pаypal" with a Cyrillic а.
        let mixed = check_link("PayPal", "https://p\u{0430}ypal.com/");
        assert!(mixed.warnings.contains(&LinkWarning::MixedScripts {
            domain: "p\u{0430}ypal.com".into()
        }));
        let userinfo = check_link(
            "https://bank.example/account",
            "https://bank.example@evil.example/",
        );
        assert!(userinfo.warnings.contains(&LinkWarning::UserInfo {
            host: "evil.example".into()
        }));
        assert!(
            userinfo
                .warnings
                .iter()
                .any(|w| matches!(w, LinkWarning::TextNamesOtherDomain { .. }))
        );
        let ip = check_link("Login", "http://192.168.1.1/login");
        assert!(ip.warnings.contains(&LinkWarning::IpAddress {
            host: "192.168.1.1".into()
        }));
        // Japanese with Latin is allowed; a plain German IDN only gets the international note.
        assert!(!mixes_scripts("東京tokyo"));
        let muenchen = check_link("x", "https://münchen.de/");
        assert_eq!(muenchen.warnings.len(), 1);
    }
}
