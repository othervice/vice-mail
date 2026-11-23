//! Built-in table of well-known providers: server settings without any network access.
//!
//! Only providers whose servers are the same for every address of a domain belong here; custom
//! domains are found online. SMTP uses implicit TLS (465) where the provider offers it
//! (RFC 8314), otherwise STARTTLS.
//!
//! Checked on 22.11.2026: Gmail, iCloud, Posteo, Fastmail and Infomaniak against their own SRV
//! records, mailbox.org and Proton against their own autoconfig files (the ignored test
//! `discovery::tests::real_network_smoke` prints the comparison). Outlook.com, Yahoo, GMX,
//! WEB.DE and Telekom publish neither; for them host, port and TLS mode were confirmed by a TLS
//! handshake, except `secureimap.t-online.de:993`, which did not answer from the test network.
//! Login-name rules and hints follow the providers' help pages and are not machine-checked.

use mail_model::{ProviderHint, Security, ServerSettings};

use super::EmailAddress;

/// How the login name is formed from the address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Login {
    /// The full address (`alice@example.org`).
    Address,
    /// The part before the `@` (`alice`).
    LocalPart,
}

/// One server of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Server {
    pub host: &'static str,
    pub port: u16,
    pub security: Security,
    pub login: Login,
}

impl Server {
    /// The settings for `address` (no password; that is entered by the user).
    pub fn settings(&self, address: &EmailAddress) -> ServerSettings {
        ServerSettings {
            host: self.host.to_owned(),
            port: self.port,
            security: self.security,
            username: match self.login {
                Login::Address => address.address().to_owned(),
                Login::LocalPart => address.local_part().to_owned(),
            },
        }
    }
}

/// A provider and the address domains it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provider {
    pub name: &'static str,
    /// Lowercase ASCII domains, matched exactly.
    pub domains: &'static [&'static str],
    pub imap: Server,
    pub smtp: Server,
    pub hint: Option<ProviderHint>,
}

/// The provider serving `domain` (lowercase ASCII), if it is in the table.
pub fn find(domain: &str) -> Option<&'static Provider> {
    PROVIDERS
        .iter()
        .find(|provider| provider.domains.contains(&domain))
}

/// All providers of the table.
pub fn all() -> &'static [Provider] {
    PROVIDERS
}

const fn tls(host: &'static str, port: u16) -> Server {
    Server {
        host,
        port,
        security: Security::Tls,
        login: Login::Address,
    }
}

const fn starttls(host: &'static str, port: u16) -> Server {
    Server {
        host,
        port,
        security: Security::Starttls,
        login: Login::Address,
    }
}

const fn local_part(server: Server) -> Server {
    Server {
        login: Login::LocalPart,
        ..server
    }
}

static PROVIDERS: &[Provider] = &[
    Provider {
        name: "Gmail",
        domains: &["gmail.com", "googlemail.com"],
        imap: tls("imap.gmail.com", 993),
        smtp: tls("smtp.gmail.com", 465),
        hint: Some(ProviderHint::Gmail),
    },
    Provider {
        // Apple: IMAP login is the name before the @, SMTP login the full address.
        name: "iCloud",
        domains: &["icloud.com", "me.com", "mac.com"],
        imap: local_part(tls("imap.mail.me.com", 993)),
        smtp: starttls("smtp.mail.me.com", 587),
        hint: Some(ProviderHint::Icloud),
    },
    Provider {
        // Settings as Microsoft publishes them; sign-in needs OAuth (stage 14), see the hint.
        name: "Outlook.com",
        domains: &[
            "outlook.com",
            "outlook.de",
            "hotmail.com",
            "hotmail.de",
            "hotmail.co.uk",
            "hotmail.fr",
            "live.com",
            "live.de",
            "msn.com",
        ],
        imap: tls("outlook.office365.com", 993),
        smtp: starttls("smtp-mail.outlook.com", 587),
        hint: Some(ProviderHint::Outlook),
    },
    Provider {
        name: "Yahoo",
        domains: &[
            "yahoo.com",
            "yahoo.de",
            "yahoo.co.uk",
            "yahoo.fr",
            "ymail.com",
            "rocketmail.com",
        ],
        imap: tls("imap.mail.yahoo.com", 993),
        smtp: tls("smtp.mail.yahoo.com", 465),
        hint: Some(ProviderHint::Yahoo),
    },
    Provider {
        name: "GMX",
        domains: &["gmx.de", "gmx.net", "gmx.at", "gmx.ch"],
        imap: tls("imap.gmx.net", 993),
        smtp: tls("mail.gmx.net", 465),
        hint: Some(ProviderHint::Gmx),
    },
    Provider {
        name: "WEB.DE",
        domains: &["web.de"],
        imap: tls("imap.web.de", 993),
        smtp: starttls("smtp.web.de", 587),
        hint: Some(ProviderHint::Webde),
    },
    Provider {
        name: "Posteo",
        domains: &["posteo.de", "posteo.net", "posteo.at", "posteo.ch"],
        imap: tls("posteo.de", 993),
        smtp: tls("posteo.de", 465),
        hint: None,
    },
    Provider {
        name: "mailbox.org",
        domains: &["mailbox.org"],
        imap: tls("imap.mailbox.org", 993),
        smtp: tls("smtp.mailbox.org", 465),
        hint: None,
    },
    Provider {
        name: "Fastmail",
        domains: &["fastmail.com", "fastmail.fm"],
        imap: tls("imap.fastmail.com", 993),
        smtp: tls("smtp.fastmail.com", 465),
        hint: Some(ProviderHint::Fastmail),
    },
    Provider {
        // Only through Proton Mail Bridge on this computer (its default ports, STARTTLS).
        name: "Proton Mail",
        domains: &["proton.me", "protonmail.com", "protonmail.ch", "pm.me"],
        imap: starttls("127.0.0.1", 1143),
        smtp: starttls("127.0.0.1", 1025),
        hint: Some(ProviderHint::Proton),
    },
    Provider {
        name: "Telekom",
        domains: &["t-online.de", "magenta.de"],
        imap: tls("secureimap.t-online.de", 993),
        smtp: tls("securesmtp.t-online.de", 465),
        hint: Some(ProviderHint::Tonline),
    },
    Provider {
        name: "Infomaniak",
        domains: &["ik.me", "ikmail.com", "etik.com"],
        imap: tls("mail.infomaniak.com", 993),
        smtp: tls("mail.infomaniak.com", 465),
        hint: None,
    },
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use mail_model::AccountSettings;

    use super::*;
    use crate::discovery::is_host_name;

    #[test]
    fn table_is_well_formed() {
        let mut seen = HashSet::new();
        for provider in all() {
            assert!(!provider.domains.is_empty(), "{}", provider.name);
            for domain in provider.domains {
                assert!(seen.insert(*domain), "{domain} listed twice");
                assert_eq!(*domain, domain.to_ascii_lowercase());
                assert!(is_host_name(domain) && domain.contains('.'), "{domain}");
            }
            for server in [provider.imap, provider.smtp] {
                assert!(is_host_name(server.host), "{}", server.host);
                assert_ne!(server.port, 0);
                // The usual pairs; anything else would be a typo.
                let expected = match server.security {
                    Security::Tls => [993, 465],
                    Security::Starttls => [143, 587],
                };
                assert!(
                    expected.contains(&server.port) || provider.name == "Proton Mail",
                    "{}: {}:{} {:?}",
                    provider.name,
                    server.host,
                    server.port,
                    server.security
                );
            }
        }
    }

    /// Every entry yields settings the setup command accepts.
    #[test]
    fn every_entry_passes_account_validation() {
        for provider in all() {
            for domain in provider.domains {
                let address =
                    EmailAddress::parse(&format!("Alice.Example@{domain}")).expect("address");
                let settings = AccountSettings {
                    protocol: Default::default(),
                    email: address.address().to_owned(),
                    display_name: String::new(),
                    imap: provider.imap.settings(&address),
                    smtp: provider.smtp.settings(&address),
                };
                settings.validated().expect("valid settings");
            }
        }
    }

    #[test]
    fn finds_providers_by_exact_domain() {
        assert_eq!(find("gmail.com").map(|p| p.name), Some("Gmail"));
        assert_eq!(find("googlemail.com").map(|p| p.name), Some("Gmail"));
        assert_eq!(find("me.com").map(|p| p.name), Some("iCloud"));
        assert_eq!(find("gmx.ch").map(|p| p.name), Some("GMX"));
        assert!(find("mail.gmail.com").is_none(), "no subdomain matching");
        assert!(find("example.org").is_none());
        assert!(
            find("GMAIL.COM").is_none(),
            "callers pass the lowercase domain"
        );
    }

    #[test]
    fn login_names_follow_the_provider() {
        let address = EmailAddress::parse("emily.parker@icloud.com").expect("address");
        let icloud = find("icloud.com").expect("icloud");
        assert_eq!(icloud.imap.settings(&address).username, "emily.parker");
        assert_eq!(
            icloud.smtp.settings(&address).username,
            "emily.parker@icloud.com"
        );

        let address = EmailAddress::parse("Max@GMX.de").expect("address");
        let gmx = find("gmx.de").expect("gmx");
        assert_eq!(gmx.imap.settings(&address).username, "Max@GMX.de");
    }

    #[test]
    fn hints_mark_special_sign_in() {
        let hint = |domain| find(domain).and_then(|p| p.hint);
        assert_eq!(hint("gmail.com"), Some(ProviderHint::Gmail));
        assert_eq!(hint("mac.com"), Some(ProviderHint::Icloud));
        assert_eq!(hint("ymail.com"), Some(ProviderHint::Yahoo));
        assert_eq!(hint("hotmail.de"), Some(ProviderHint::Outlook));
        assert_eq!(hint("pm.me"), Some(ProviderHint::Proton));
        assert_eq!(hint("web.de"), Some(ProviderHint::Webde));
        assert_eq!(hint("magenta.de"), Some(ProviderHint::Tonline));
        assert_eq!(hint("posteo.de"), None);
        assert_eq!(hint("mailbox.org"), None);
    }
}
