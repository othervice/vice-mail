//! Addresses, header text and Message-IDs of outgoing messages.

use std::net::IpAddr;

use url::Host;

use super::build::ComposeError;

/// Longest local part (RFC 5321, section 4.5.3.1.1).
const MAX_LOCAL_PART: usize = 64;
/// Longest domain (RFC 5321, section 4.5.3.1.2).
const MAX_DOMAIN: usize = 255;

/// Checks an address typed or chosen in the composer and returns the form that goes into the
/// message and the envelope: surrounding space and angle brackets removed, the domain in lower
/// case and in its ASCII form (`müller.de` → `xn--mller-kva.de`). The local part may contain
/// non-ASCII letters (RFC 6531; sending it then needs a server with SMTPUTF8).
pub fn normalize_email(email: &str) -> Result<String, ComposeError> {
    let invalid = || ComposeError::InvalidAddress(email.to_owned());
    let trimmed = email.trim();
    let trimmed = trimmed
        .strip_prefix('<')
        .and_then(|e| e.strip_suffix('>'))
        .unwrap_or(trimmed);
    let (local, domain) = trimmed.rsplit_once('@').ok_or_else(invalid)?;
    if !valid_local_part(local) {
        return Err(invalid());
    }
    let domain = ascii_domain(domain).ok_or_else(invalid)?;
    Ok(format!("{local}@{domain}"))
}

/// A dot-atom local part (RFC 5322) with UTF-8 letters allowed; no quoted strings.
fn valid_local_part(local: &str) -> bool {
    !local.is_empty()
        && local.len() <= MAX_LOCAL_PART
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || "!#$%&'*+-/=?^_`{|}~.".contains(c)
                || (!c.is_ascii() && !c.is_whitespace() && !c.is_control())
        })
}

/// The domain in ASCII lower case, or an address literal (`[192.0.2.1]`, `[IPv6:…]`).
fn ascii_domain(domain: &str) -> Option<String> {
    if domain.is_empty() || domain.len() > MAX_DOMAIN || domain.ends_with('.') {
        return None;
    }
    // The URL host parser would decode `%41` or drop characters; none of them belongs in a
    // domain anyway.
    if domain.contains(['%', '/', '\\', '?', '#', '@', ':']) || domain.contains(char::is_whitespace)
    {
        return None;
    }
    if let Some(literal) = domain.strip_prefix('[').and_then(|d| d.strip_suffix(']')) {
        let ip = literal.strip_prefix("IPv6:").unwrap_or(literal);
        return ip
            .parse::<IpAddr>()
            .is_ok()
            .then(|| domain.to_ascii_lowercase());
    }
    match Host::parse(domain).ok()? {
        Host::Domain(ascii) => {
            let labels_ok = ascii.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
            labels_ok.then(|| ascii.to_ascii_lowercase())
        }
        // A bare IP address is not a valid domain in an email address.
        Host::Ipv4(_) | Host::Ipv6(_) => None,
    }
}

/// Header text without control characters (line breaks, tabs: spaces), trimmed. Nothing in a
/// subject or name can start a header of its own.
pub(crate) fn clean_header_text(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    mapped.trim().to_owned()
}

/// A Message-ID without angle brackets that can be written as `<id>`: printable ASCII without
/// angle brackets, with an `@` that has something on both sides. Lenient on purpose: ids of
/// other programs in References are kept as long as they cannot break the header.
pub(crate) fn is_valid_message_id(id: &str) -> bool {
    let Some((left, right)) = id.rsplit_once('@') else {
        return false;
    };
    !left.is_empty()
        && !right.is_empty()
        && id.len() <= 250
        && id
            .bytes()
            .all(|b| b.is_ascii_graphic() && b != b'<' && b != b'>')
}

/// A new Message-ID (without angle brackets) for a draft: 128 random bits from the caller
/// (`random`, for example a UUID v4's bytes; this crate owns no randomness) as hex, and the
/// domain of the sender's address. Nothing names the program or the computer. A sender without
/// a usable domain gets `localhost`.
pub fn new_message_id(sender_email: &str, random: [u8; 16]) -> String {
    let domain = normalize_email(sender_email)
        .ok()
        .and_then(|email| email.rsplit_once('@').map(|(_, d)| d.to_owned()))
        .filter(|d| !d.starts_with('['))
        .unwrap_or_else(|| "localhost".to_owned());
    let mut id = String::with_capacity(32 + 1 + domain.len());
    for byte in random {
        id.push_str(&format!("{byte:02x}"));
    }
    id.push('@');
    id.push_str(&domain);
    id
}
