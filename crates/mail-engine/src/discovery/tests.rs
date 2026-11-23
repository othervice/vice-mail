use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use mail_model::{ErrorCode, Security, ServerWarning};
use reqwest::Url;
use serde_json::json;

use super::srv::SrvRecord;
use super::*;

fn settings(host: &str, port: u16, security: Security, username: &str) -> ServerSettings {
    ServerSettings {
        host: host.into(),
        port,
        security,
        username: username.into(),
    }
}

#[test]
fn addresses_are_split_and_normalised() {
    let a = EmailAddress::parse("  Alice.Doe@Example.ORG. ").expect("address");
    assert_eq!(a.address(), "Alice.Doe@Example.ORG.");
    assert_eq!(a.local_part(), "Alice.Doe");
    assert_eq!(a.dns_domain(), Some("example.org"));

    let idn = EmailAddress::parse("max@Müller.de").expect("address");
    assert_eq!(idn.dns_domain(), Some("xn--mller-kva.de"));
    assert_eq!(
        idn.expand("%EMAILLOCALPART%@%EMAILDOMAIN%", false)
            .as_deref(),
        Some("max@müller.de")
    );
    assert_eq!(
        idn.expand("imap.%EMAILDOMAIN%", true).as_deref(),
        Some("imap.xn--mller-kva.de")
    );
    assert_eq!(idn.expand("%REALNAME%", false), None);

    // Quoted local parts may contain an @; the domain is after the last one.
    let quoted = EmailAddress::parse("\"a@b\"@example.org").expect("address");
    assert_eq!(quoted.dns_domain(), Some("example.org"));

    for literal in ["alice@[192.0.2.1]", "alice@192.0.2.1"] {
        assert_eq!(
            EmailAddress::parse(literal).expect(literal).dns_domain(),
            None,
            "{literal}"
        );
    }

    for bad in [
        "",
        "alice",
        "@example.org",
        "alice@",
        "alice@exa mple.org",
        "al ice@example.org",
        "alice@example.org/path",
        "alice@example.org:993",
        "alice@example.org?x",
        "alice@exa_mple.org",
        "alice@-example.org",
        "alice@example..org",
        "alice@\u{0}example.org",
    ] {
        let err = EmailAddress::parse(bad).expect_err(bad);
        assert_eq!(err.code(), ErrorCode::InvalidArgument, "{bad}");
    }
}

#[test]
fn known_provider_fills_both_servers_offline() {
    let found = discover_known("someone@GMAIL.com").expect("discover");
    assert_eq!(found.source, DiscoverySource::KnownProvider);
    assert_eq!(
        found.imap,
        Some(settings(
            "imap.gmail.com",
            993,
            Security::Tls,
            "someone@GMAIL.com"
        ))
    );
    assert_eq!(
        found.smtp,
        Some(settings(
            "smtp.gmail.com",
            465,
            Security::Tls,
            "someone@GMAIL.com"
        ))
    );
    assert_eq!(found.provider_hint, Some(ProviderHint::Gmail));

    let found = discover_known("ich@posteo.de").expect("discover");
    assert_eq!(found.source, DiscoverySource::KnownProvider);
    assert_eq!(found.provider_hint, None);
}

#[test]
fn unknown_domains_and_literals_find_nothing() {
    for email in ["alice@example.org", "alice@[192.0.2.1]", "alice@localhost"] {
        assert_eq!(
            discover_known(email).expect(email),
            nothing(None),
            "{email}"
        );
    }
    assert_eq!(
        discover_known("alice").expect_err("invalid").code(),
        ErrorCode::InvalidArgument
    );
}

/// The result crosses the bridge with exactly these fields: there is no place for a password.
#[test]
fn result_wire_format_has_no_secret_field() {
    let found = discover_known("emily@icloud.com").expect("discover");
    assert_eq!(
        serde_json::to_value(&found).expect("serialize"),
        json!({
            "source": "knownProvider",
            "imap": { "host": "imap.mail.me.com", "port": 993, "security": "tls", "username": "emily" },
            "smtp": { "host": "smtp.mail.me.com", "port": 587, "security": "starttls",
                      "username": "emily@icloud.com" },
            "providerHint": "icloud",
        })
    );
    let hints = [
        (ProviderHint::Gmail, "gmail"),
        (ProviderHint::Icloud, "icloud"),
        (ProviderHint::Yahoo, "yahoo"),
        (ProviderHint::Fastmail, "fastmail"),
        (ProviderHint::Outlook, "outlook"),
        (ProviderHint::Proton, "proton"),
        (ProviderHint::Gmx, "gmx"),
        (ProviderHint::Webde, "webde"),
        (ProviderHint::Tonline, "tonline"),
    ];
    for (hint, wire) in hints {
        assert_eq!(serde_json::to_value(hint).expect("serialize"), wire);
    }
    let none = serde_json::to_value(nothing(None)).expect("serialize");
    assert_eq!(
        none,
        json!({ "source": "none", "imap": null, "smtp": null, "providerHint": null })
    );
}

// ---------------------------------------------------------------------------------------------
// Online discovery with fakes

#[derive(Default)]
struct FakeDns {
    records: HashMap<String, Vec<SrvRecord>>,
    asked: Mutex<Vec<String>>,
}

impl FakeDns {
    fn with(mut self, name: &str, port: u16, target: &str) -> Self {
        self.records
            .entry(name.into())
            .or_default()
            .push(SrvRecord {
                priority: 0,
                weight: 0,
                port,
                target: target.into(),
            });
        self
    }

    fn asked(&self) -> usize {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

impl SrvResolver for FakeDns {
    async fn lookup_srv(&self, name: &str) -> Result<Vec<SrvRecord>, LookupError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.to_owned());
        Ok(self.records.get(name).cloned().unwrap_or_default())
    }
}

#[derive(Default)]
struct FakeWeb {
    files: HashMap<String, String>,
    asked: Mutex<Vec<String>>,
}

impl FakeWeb {
    fn with(mut self, url: &str, body: &str) -> Self {
        self.files.insert(url.into(), body.into());
        self
    }

    fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl AutoconfigFetch for FakeWeb {
    async fn fetch(&self, url: &Url, _domain: &str) -> Result<Option<Vec<u8>>, LookupError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(url.to_string());
        Ok(self
            .files
            .get(url.as_str())
            .map(|body| body.clone().into_bytes()))
    }
}

const AUTOCONFIG_URL: &str =
    "https://autoconfig.example.org/mail/config-v1.1.xml?emailaddress=alice%40example.org";
const WELL_KNOWN_URL: &str = "https://example.org/.well-known/autoconfig/mail/config-v1.1.xml";
const FILE: &str = r#"<clientConfig version="1.1"><emailProvider id="example.org">
  <incomingServer type="imap"><hostname>imap.hoster.net</hostname><port>993</port>
    <socketType>SSL</socketType><username>%EMAILADDRESS%</username></incomingServer>
  <outgoingServer type="smtp"><hostname>smtp.hoster.net</hostname><port>465</port>
    <socketType>SSL</socketType><username>%EMAILADDRESS%</username></outgoingServer>
</emailProvider></clientConfig>"#;

#[tokio::test]
async fn complete_srv_answer_skips_autoconfig() {
    let dns = FakeDns::default()
        .with("_imaps._tcp.example.org.", 993, "imap.example.org")
        .with("_submissions._tcp.example.org.", 465, "smtp.example.org");
    let discovery = OnlineDiscovery::new(dns, FakeWeb::default().with(AUTOCONFIG_URL, FILE));
    let found = discovery
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(found.source, DiscoverySource::Srv);
    assert_eq!(
        found.imap,
        Some(settings(
            "imap.example.org",
            993,
            Security::Tls,
            "alice@example.org"
        ))
    );
    assert_eq!(
        found.smtp,
        Some(settings(
            "smtp.example.org",
            465,
            Security::Tls,
            "alice@example.org"
        ))
    );
    assert!(
        discovery.fetcher.asked().is_empty(),
        "autoconfig fetched although SRV was complete"
    );
}

#[tokio::test]
async fn autoconfig_fills_what_srv_left_open() {
    let dns = FakeDns::default().with("_imap._tcp.example.org.", 143, "imap.example.org");
    let discovery = OnlineDiscovery::new(dns, FakeWeb::default().with(WELL_KNOWN_URL, FILE));
    let found = discovery
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(
        found.source,
        DiscoverySource::Srv,
        "the IMAP settings came from SRV"
    );
    assert_eq!(
        found.imap,
        Some(settings(
            "imap.example.org",
            143,
            Security::Starttls,
            "alice@example.org"
        ))
    );
    assert_eq!(
        found.smtp,
        Some(settings(
            "smtp.hoster.net",
            465,
            Security::Tls,
            "alice@example.org"
        ))
    );
    assert_eq!(discovery.fetcher.asked(), [AUTOCONFIG_URL, WELL_KNOWN_URL]);
}

#[tokio::test]
async fn autoconfig_alone() {
    let discovery = OnlineDiscovery::new(
        FakeDns::default(),
        FakeWeb::default().with(AUTOCONFIG_URL, FILE),
    );
    let found = discovery
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(found.source, DiscoverySource::Autoconfig);
    assert_eq!(
        found.imap.map(|s| s.host).as_deref(),
        Some("imap.hoster.net")
    );
    assert_eq!(
        found.smtp.map(|s| s.host).as_deref(),
        Some("smtp.hoster.net")
    );
    assert_eq!(
        discovery.fetcher.asked(),
        [AUTOCONFIG_URL],
        "second URL only when the first fails"
    );
    assert_eq!(discovery.resolver.asked(), 4);
}

#[tokio::test]
async fn nothing_found_is_source_none_with_the_known_hint() {
    let discovery = OnlineDiscovery::new(FakeDns::default(), FakeWeb::default());
    assert_eq!(
        discovery
            .discover("alice@example.org")
            .await
            .expect("discover"),
        nothing(None)
    );

    // A known domain keeps its hint even when it is searched online.
    let found = discovery.discover("max@gmx.de").await.expect("discover");
    assert_eq!(found, nothing(Some(ProviderHint::Gmx)));
}

#[tokio::test]
async fn no_network_for_literals_single_labels_and_invalid_addresses() {
    let discovery = OnlineDiscovery::new(FakeDns::default(), FakeWeb::default());
    for email in ["alice@[192.0.2.1]", "alice@192.0.2.1", "alice@localhost"] {
        assert_eq!(
            discovery.discover(email).await.expect(email),
            nothing(None),
            "{email}"
        );
    }
    let err = discovery
        .discover("not an address")
        .await
        .expect_err("invalid");
    assert_eq!(err.code(), ErrorCode::InvalidArgument);
    assert_eq!(discovery.resolver.asked(), 0);
    assert!(discovery.fetcher.asked().is_empty());
}

/// Real network, by hand only: `VICEMAIL_REAL_NETWORK=1 cargo test -p mail-engine
/// discovery::tests::real_network -- --ignored --nocapture` (a no-op without the variable, so
/// `--include-ignored` for the test server never goes out). Uses the system's DNS and contacts
/// the providers' own autoconfig hosts. Prints, for every provider of the built-in table, what
/// the provider publishes itself next to the table's values (to maintain the table).
#[tokio::test]
#[ignore = "contacts real DNS and web servers"]
async fn real_network_smoke() {
    if std::env::var("VICEMAIL_REAL_NETWORK").as_deref() != Ok("1") {
        println!("skipped: set VICEMAIL_REAL_NETWORK=1 to contact real DNS and web servers");
        return;
    }
    let short = |s: &Option<ServerSettings>| {
        s.as_ref().map_or("-".to_owned(), |s| {
            format!("{}:{} {:?} {}", s.host, s.port, s.security, s.username)
        })
    };
    for provider in known::all() {
        for domain in provider.domains {
            let email = format!("someone@{domain}");
            let table = discover_known(&email).expect("known");
            let online = discover_online(&email).await.expect("discover");
            println!(
                "{email}\n  table  imap {} | smtp {}\n  {:<6} imap {} | smtp {}",
                short(&table.imap),
                short(&table.smtp),
                format!("{:?}", online.source),
                short(&online.imap),
                short(&online.smtp),
            );
        }
    }
    let found = discover_online("someone@fastmail.com")
        .await
        .expect("discover");
    assert_eq!(found.source, DiscoverySource::Srv, "{found:?}");
}

/// Stage 14 (decision 0034): a Google Workspace domain publishes Gmail's servers; the setup
/// shows the Gmail hint (app password, IMAP allowed by the administrator).
#[tokio::test]
async fn workspace_domains_get_the_gmail_hint_from_their_servers() {
    let dns = FakeDns::default()
        .with("_imaps._tcp.example.org.", 993, "imap.gmail.com")
        .with("_submissions._tcp.example.org.", 465, "smtp.gmail.com");
    let discovery = OnlineDiscovery::new(dns, FakeWeb::default());
    let found = discovery
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(found.provider_hint, Some(ProviderHint::Gmail));
    // Other servers get none.
    let dns = FakeDns::default()
        .with("_imaps._tcp.example.org.", 993, "imap.example.org")
        .with("_submissions._tcp.example.org.", 465, "smtp.example.org");
    let found = OnlineDiscovery::new(dns, FakeWeb::default())
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(found.provider_hint, None);
}

// ---------------------------------------------------------------------------------------------
// Warnings for found servers (security review N-4)

#[tokio::test]
async fn srv_targets_outside_the_domain_or_as_ip_addresses_are_flagged() {
    let dns = FakeDns::default()
        .with(
            "_imaps._tcp.example.org.",
            993,
            "imap.example.org.evil.test",
        )
        .with("_submissions._tcp.example.org.", 465, "192.0.2.25");
    let found = OnlineDiscovery::new(dns, FakeWeb::default())
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(found.source, DiscoverySource::Srv);
    assert_eq!(
        found.warnings,
        ServerWarnings {
            imap: Some(ServerWarning::OutsideDomain),
            smtp: Some(ServerWarning::IpAddress),
            jmap: None,
        }
    );

    // Subdomains of the address's domain (any case in the address) and the built-in table's
    // servers (a Google Workspace domain) are not.
    for (email, imap, smtp) in [
        (
            "alice@Example.ORG",
            "imap.example.org",
            "smtp.mail.example.org",
        ),
        ("alice@example.org", "imap.gmail.com", "smtp.gmail.com"),
    ] {
        let dns = FakeDns::default()
            .with("_imaps._tcp.example.org.", 993, imap)
            .with("_submissions._tcp.example.org.", 465, smtp);
        let found = OnlineDiscovery::new(dns, FakeWeb::default())
            .discover(email)
            .await
            .expect("discover");
        assert_eq!(found.warnings, ServerWarnings::NONE, "{email} {imap}");
    }
}

#[tokio::test]
async fn autoconfig_servers_are_flagged_per_server() {
    // FILE names servers of another domain, as many hosted domains do.
    let discovery = OnlineDiscovery::new(
        FakeDns::default(),
        FakeWeb::default().with(AUTOCONFIG_URL, FILE),
    );
    let found = discovery
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(found.source, DiscoverySource::Autoconfig);
    assert_eq!(
        found.warnings,
        ServerWarnings {
            imap: Some(ServerWarning::OutsideDomain),
            smtp: Some(ServerWarning::OutsideDomain),
            jmap: None,
        }
    );

    // A file with an IP address, a subdomain and a JMAP server elsewhere; SRV gave nothing.
    let file = r#"<clientConfig version="1.1"><emailProvider id="example.org">
  <incomingServer type="imap"><hostname>192.0.2.10</hostname><port>993</port>
    <socketType>SSL</socketType></incomingServer>
  <outgoingServer type="smtp"><hostname>SMTP.%EMAILDOMAIN%.</hostname><port>465</port>
    <socketType>SSL</socketType></outgoingServer>
  <incomingServer type="jmap"><hostname>jmap.evil.test</hostname></incomingServer>
</emailProvider></clientConfig>"#;
    let found = OnlineDiscovery::new(
        FakeDns::default(),
        FakeWeb::default().with(WELL_KNOWN_URL, file),
    )
    .discover("alice@example.org")
    .await
    .expect("discover");
    assert_eq!(
        found.imap.as_ref().map(|s| s.host.as_str()),
        Some("192.0.2.10")
    );
    assert_eq!(
        found.jmap.as_ref().map(|s| s.host.as_str()),
        Some("jmap.evil.test")
    );
    assert_eq!(
        found.warnings,
        ServerWarnings {
            imap: Some(ServerWarning::IpAddress),
            smtp: None,
            jmap: Some(ServerWarning::OutsideDomain),
        }
    );

    // Mixed sources: the SRV server inside the domain, the autoconfig one outside.
    let dns = FakeDns::default().with("_imap._tcp.example.org.", 143, "imap.example.org");
    let found = OnlineDiscovery::new(dns, FakeWeb::default().with(WELL_KNOWN_URL, FILE))
        .discover("alice@example.org")
        .await
        .expect("discover");
    assert_eq!(
        found.warnings,
        ServerWarnings {
            imap: None,
            smtp: Some(ServerWarning::OutsideDomain),
            jmap: None,
        }
    );
}

#[tokio::test]
async fn idn_domains_compare_in_their_ascii_form() {
    let dns = FakeDns::default()
        .with(
            "_imaps._tcp.xn--mller-kva.de.",
            993,
            "imap.xn--mller-kva.de",
        )
        .with(
            "_submissions._tcp.xn--mller-kva.de.",
            465,
            "smtp.mueller.de",
        );
    let found = OnlineDiscovery::new(dns, FakeWeb::default())
        .discover("max@Müller.de")
        .await
        .expect("discover");
    assert_eq!(
        found.warnings,
        ServerWarnings {
            imap: None,
            smtp: Some(ServerWarning::OutsideDomain),
            jmap: None,
        }
    );
}

/// The built-in table is trusted: no warning, also for Proton's Bridge on `127.0.0.1`.
#[test]
fn known_providers_carry_no_warning() {
    for provider in known::all() {
        let email = format!("someone@{}", provider.domains[0]);
        let found = discover_known(&email).expect("known");
        assert_eq!(found.warnings, ServerWarnings::NONE, "{email}");
    }
    assert_eq!(
        discover_known("someone@proton.me")
            .expect("known")
            .imap
            .map(|s| s.host)
            .as_deref(),
        Some("127.0.0.1")
    );
}

#[test]
fn warnings_on_the_wire() {
    let mut found = nothing(None);
    found.warnings = ServerWarnings {
        imap: Some(ServerWarning::OutsideDomain),
        smtp: None,
        jmap: Some(ServerWarning::IpAddress),
    };
    assert_eq!(
        serde_json::to_value(&found).expect("serialize"),
        json!({ "source": "none", "imap": null, "smtp": null, "providerHint": null,
                "warnings": { "imap": "outsideDomain", "jmap": "ipAddress" } })
    );
}
