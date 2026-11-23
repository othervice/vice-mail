//! Addresses as contact keys: the local part in NFC and lower case, the domain in its ASCII
//! form (IDNA, UTS 46) in lower case; for display the domain in Unicode.

use icu_normalizer::ComposingNormalizerBorrowed;

use crate::compose::normalize_email;
use crate::links::decode_punycode;

/// An address in its key form and the parts needed for display and duplicate checks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NormalizedAddress {
    /// `local@ascii-domain`, all lower case: the identity of a contact address.
    pub key: String,
    /// The local part as written (only for sending; servers may in theory tell case apart).
    pub local_as_written: String,
    /// The local part in lower case.
    pub local: String,
    /// The domain in ASCII lower case (`xn--mller-kva.example`).
    pub domain: String,
}

impl NormalizedAddress {
    /// The address to write into a message: local part as written, ASCII domain.
    pub fn email(&self) -> String {
        format!("{}@{}", self.local_as_written, self.domain)
    }

    /// The address to show: lower case, the domain in Unicode (`müller.example`).
    pub fn display(&self) -> String {
        format!("{}@{}", self.local, unicode_domain(&self.domain))
    }

    /// The same address without a `+tag` (`alice+shop@x` → `alice@x`), if it has one.
    pub fn without_tag(&self) -> Option<String> {
        let (base, tag) = self.local.split_once('+')?;
        (!base.is_empty() && !tag.is_empty()).then(|| format!("{base}@{}", self.domain))
    }

    /// The mailbox a provider delivers to, where it ignores parts of the address: Gmail ignores
    /// dots and treats `googlemail.com` as `gmail.com`. Only for duplicate suggestions.
    pub fn provider_mailbox(&self) -> Option<String> {
        if self.domain != "gmail.com" && self.domain != "googlemail.com" {
            return None;
        }
        let base = self.local.split('+').next().unwrap_or_default();
        let base: String = base.chars().filter(|c| *c != '.').collect();
        (!base.is_empty()).then(|| format!("{base}@gmail.com"))
    }
}

/// Normalises an address for use as a key (see module docs). `None` for anything that is not
/// one valid address (as checked by [`crate::normalize_email`]).
pub fn normalize_address(email: &str) -> Option<NormalizedAddress> {
    let normalized = normalize_email(email).ok()?;
    let (local, domain) = normalized.rsplit_once('@')?;
    let lower = ComposingNormalizerBorrowed::new_nfc()
        .normalize(local)
        .to_lowercase();
    Some(NormalizedAddress {
        key: format!("{lower}@{domain}"),
        local_as_written: local.to_owned(),
        local: lower,
        domain: domain.to_owned(),
    })
}

/// A domain in ASCII lower case (IDNA), for domain rules. `None` if it is not a valid domain.
pub fn normalize_domain(domain: &str) -> Option<String> {
    let domain = domain.trim().trim_start_matches('@').trim_matches('.');
    normalize_address(&format!("x@{domain}")).map(|a| a.domain)
}

/// The Unicode form of an ASCII domain (punycode labels decoded; undecodable ones stay).
pub fn unicode_domain(domain: &str) -> String {
    domain
        .split('.')
        .map(|label| match label.strip_prefix("xn--") {
            Some(encoded) => decode_punycode(encoded).unwrap_or_else(|| label.to_owned()),
            None => label.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fold_case_and_idna_but_keep_tags() {
        let a = normalize_address(" <Hans.Mueller@Firma.EXAMPLE> ").expect("valid");
        assert_eq!(a.key, "hans.mueller@firma.example");
        assert_eq!(a.email(), "Hans.Mueller@firma.example");
        let b = normalize_address("jürgen@müller.example").expect("valid");
        assert_eq!(b.key, "jürgen@xn--mller-kva.example");
        assert_eq!(b.display(), "jürgen@müller.example");
        assert_eq!(
            normalize_address("JÜRGEN@xn--mller-kva.example").map(|a| a.key),
            Some(b.key.clone())
        );
        assert_eq!(
            normalize_address("ju\u{308}rgen@müller.example").map(|a| a.key),
            Some(b.key.clone()),
            "decomposed umlaut in the local part"
        );
        let tagged = normalize_address("alice+shop@example.org").expect("valid");
        assert_eq!(tagged.key, "alice+shop@example.org", "tags are kept");
        assert_eq!(tagged.without_tag().as_deref(), Some("alice@example.org"));
        assert_eq!(
            normalize_address("alice@example.org").and_then(|a| a.without_tag()),
            None
        );
        for bad in [
            "",
            "alice",
            "a@",
            "@b",
            "a b@c.example",
            "a@b@c",
            "a@-x.example",
        ] {
            assert_eq!(normalize_address(bad), None, "{bad}");
        }
    }

    #[test]
    fn provider_mailboxes() {
        let mailbox = |e: &str| normalize_address(e).and_then(|a| a.provider_mailbox());
        assert_eq!(
            mailbox("Hans.Mueller+x@googlemail.com").as_deref(),
            Some("hansmueller@gmail.com")
        );
        assert_eq!(
            mailbox("hansmueller@gmail.com").as_deref(),
            Some("hansmueller@gmail.com")
        );
        assert_eq!(mailbox("hans.mueller@firma.example"), None);
    }

    #[test]
    fn domains() {
        assert_eq!(
            normalize_domain("@Müller.Example.").as_deref(),
            Some("xn--mller-kva.example")
        );
        assert_eq!(normalize_domain("not a domain"), None);
        assert_eq!(unicode_domain("xn--mller-kva.example"), "müller.example");
        assert_eq!(unicode_domain("xn--!!!.example"), "xn--!!!.example");
    }
}
