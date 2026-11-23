//! Warnings for servers the online search found (security review N-4, RFC 6186 section 6).
//!
//! SRV records and autoconfig files are not verified (no DNSSEC), so a network attacker during
//! setup can propose his own server. The setup form shows every found server anyway; these
//! warnings say specifically when one deserves a second look:
//!
//! - [`ServerWarning::IpAddress`]: the host is an IPv4 or IPv6 address (also the forms the
//!   system resolver reads as one, such as `0x7f.1`), not a name.
//! - [`ServerWarning::OutsideDomain`]: the host is neither the address's domain nor one of its
//!   subdomains. Names are compared in their ASCII (IDNA) form, case-insensitively and without a
//!   trailing dot, the way [`EmailAddress`](super::EmailAddress) reads the domain. Hosts of the
//!   built-in provider table ([`known`]) are exempt: custom domains at Google Workspace,
//!   Microsoft 365, Fastmail and the like point there, and a password sent there reaches that
//!   provider, not an attacker. A host that cannot be read as a name counts as outside.
//!
//! The built-in table itself is trusted and never gets a warning (its Proton entry is
//! `127.0.0.1`, the Bridge on this computer). Discovery asks no MX records and no provider's
//! autoconfig by MX, so every online result belongs to the address's own domain or is flagged.

use std::net::{IpAddr, Ipv6Addr};

use mail_model::{DiscoveryResult, ServerSettings, ServerWarning, ServerWarnings};
use url::{Host, Url};

use super::{is_host_name, known};

/// The warnings for the servers of `result`, found online for an address at `domain` (the
/// ASCII form of [`EmailAddress::dns_domain`](super::EmailAddress::dns_domain)).
pub fn check(result: &DiscoveryResult, domain: &str) -> ServerWarnings {
    let warn = |server: &Option<ServerSettings>| {
        server
            .as_ref()
            .and_then(|server| server_warning(&server.host, domain))
    };
    ServerWarnings {
        imap: warn(&result.imap),
        smtp: warn(&result.smtp),
        jmap: warn(&result.jmap),
    }
}

/// The warning for a server `host` found online for an address at `domain`, if any (rules in
/// the module docs). At most one: an IP address is not also reported as outside the domain.
pub fn server_warning(host: &str, domain: &str) -> Option<ServerWarning> {
    let host = match name(host) {
        Some(Name::Ip) => return Some(ServerWarning::IpAddress),
        Some(Name::Dns(host)) => host,
        None => return Some(ServerWarning::OutsideDomain),
    };
    let inside = match name(domain) {
        Some(Name::Dns(domain)) => {
            host == domain
                || host
                    .strip_suffix(&domain)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        }
        _ => false,
    };
    (!inside && !is_known_server(&host)).then_some(ServerWarning::OutsideDomain)
}

/// A host of the built-in provider table.
fn is_known_server(host: &str) -> bool {
    known::all()
        .iter()
        .any(|provider| provider.imap.host == host || provider.smtp.host == host)
}

#[derive(Debug, PartialEq, Eq)]
enum Name {
    Ip,
    /// Lowercase ASCII (IDNA), without a trailing dot.
    Dns(String),
}

/// `text` as a host: an IP address, a DNS name in its ASCII form, or `None` if it is neither.
fn name(text: &str) -> Option<Name> {
    let text = text.trim();
    let text = text.strip_suffix('.').unwrap_or(text);
    let bracketed = text.strip_prefix('[').and_then(|t| t.strip_suffix(']'));
    if text.parse::<IpAddr>().is_ok() || bracketed.is_some_and(|t| t.parse::<Ipv6Addr>().is_ok()) {
        return Some(Name::Ip);
    }
    // Anything that would make the URL below read another host (user info, port, path).
    if text.is_empty() || text.contains(['/', '\\', '?', '#', ':', '@', '[', ']', '%']) {
        return None;
    }
    // The URL parser maps Unicode to IDNA and lowercases, as for the address's domain, and
    // reads the IPv4 forms (`0x7f.1`, `2130706433`) that the system resolver accepts too.
    let url = Url::parse(&format!("https://{text}/")).ok()?;
    match url.host()? {
        Host::Ipv4(_) | Host::Ipv6(_) => Some(Name::Ip),
        Host::Domain(ascii) => {
            let ascii = ascii.strip_suffix('.').unwrap_or(ascii);
            is_host_name(ascii).then(|| Name::Dns(ascii.to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use mail_model::{DiscoverySource, Security};

    use super::*;

    const OUTSIDE: Option<ServerWarning> = Some(ServerWarning::OutsideDomain);
    const IP: Option<ServerWarning> = Some(ServerWarning::IpAddress);

    fn at_example(host: &str) -> Option<ServerWarning> {
        server_warning(host, "example.org")
    }

    #[test]
    fn the_domain_and_its_subdomains_are_inside() {
        assert_eq!(at_example("example.org"), None);
        assert_eq!(at_example("imap.example.org"), None);
        assert_eq!(at_example("a.b.mail.example.org"), None);
    }

    #[test]
    fn other_domains_and_look_alikes_are_outside() {
        for host in [
            "imap.hoster.net",
            "example.net",
            "example.org.evil.test",
            "imap.example.org.evil.test",
            "notexample.org",
            "evil-example.org",
            "org",
            "example",
        ] {
            assert_eq!(at_example(host), OUTSIDE, "{host}");
        }
    }

    #[test]
    fn case_and_a_trailing_dot_do_not_matter() {
        assert_eq!(at_example("IMAP.Example.ORG"), None);
        assert_eq!(at_example("imap.example.org."), None);
        assert_eq!(server_warning("imap.example.org", "EXAMPLE.org."), None);
        assert_eq!(at_example("Example.Org.Evil.Test."), OUTSIDE);
    }

    #[test]
    fn ip_addresses_are_flagged_as_such() {
        for host in [
            "192.0.2.1",
            "192.0.2.1.",
            "127.0.0.1",
            "2001:db8::1",
            "::1",
            "[2001:db8::1]",
            "::ffff:192.0.2.1",
            // Read as IPv4 by the system resolver as well.
            "0x7f.1",
            "2130706433",
        ] {
            assert_eq!(at_example(host), IP, "{host}");
        }
        // Also for an address literal or IP address after the @ (discovery never gets there).
        assert_eq!(server_warning("192.0.2.1", "192.0.2.1"), IP);
    }

    #[test]
    fn idn_names_compare_in_their_ascii_form() {
        let domain = "xn--mller-kva.de"; // müller.de, as discovery holds it
        assert_eq!(server_warning("imap.xn--mller-kva.de", domain), None);
        assert_eq!(server_warning("imap.müller.de", domain), None);
        assert_eq!(server_warning("IMAP.MÜLLER.DE.", domain), None);
        assert_eq!(server_warning("imap.xn--mller-kva.de", "Müller.de"), None);
        // A look-alike (Cyrillic "е") is another name.
        assert_eq!(server_warning("imap.m\u{fc}ll\u{435}r.de", domain), OUTSIDE);
        assert_eq!(server_warning("imap.muller.de", domain), OUTSIDE);
        assert_eq!(
            server_warning("imap.xn--mller-kva.de.evil.test", domain),
            OUTSIDE
        );
    }

    #[test]
    fn hosts_that_are_no_names_count_as_outside() {
        for host in [
            "",
            ".",
            "evil.test@example.org",
            "example.org:993",
            "evil.test/example.org",
            "bad host.example.org",
            "a..example.org",
            ".example.org",
            "example.123",
        ] {
            assert_eq!(at_example(host), OUTSIDE, "{host:?}");
        }
    }

    #[test]
    fn servers_of_the_built_in_table_are_not_outside() {
        // A Google Workspace or Microsoft 365 domain points to its provider's servers.
        assert_eq!(at_example("imap.gmail.com"), None);
        assert_eq!(at_example("SMTP.gmail.com."), None);
        assert_eq!(at_example("outlook.office365.com"), None);
        assert_eq!(at_example("imap.fastmail.com"), None);
        // Only the exact hosts, and an IP address of the table stays an IP address.
        assert_eq!(at_example("evil.gmail.com"), OUTSIDE);
        assert_eq!(at_example("imap.gmail.com.evil.test"), OUTSIDE);
        assert_eq!(at_example("127.0.0.1"), IP);
    }

    #[test]
    fn check_gives_one_warning_per_server() {
        let server = |host: &str| ServerSettings {
            host: host.into(),
            port: 993,
            security: Security::Tls,
            username: "alice@example.org".into(),
        };
        let mut result = DiscoveryResult {
            source: DiscoverySource::Srv,
            imap: Some(server("imap.example.org")),
            smtp: Some(server("smtp.hoster.net")),
            provider_hint: None,
            jmap: Some(server("192.0.2.7")),
            warnings: ServerWarnings::NONE,
        };
        assert_eq!(
            check(&result, "example.org"),
            ServerWarnings {
                imap: None,
                smtp: OUTSIDE,
                jmap: IP,
            }
        );
        result.smtp = None;
        result.jmap = None;
        assert!(check(&result, "example.org").is_empty());
    }
}
