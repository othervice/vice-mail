//! Server settings for an email address, proposed for the setup form.
//!
//! - [`discover_known`]: the built-in table of well-known providers ([`known`]). No network.
//!   The UI may call it while the user types.
//! - [`discover_online`]: only after the user clicks "search": DNS SRV records of the address's
//!   domain (RFC 6186, [`srv`]), then the domain's own autoconfig file (Thunderbird format,
//!   [`autoconfig`]). No third-party directory (no Mozilla ISPDB, no MX guessing).
//!
//! Network use of [`discover_online`], and nothing else:
//! - DNS queries for `_imaps._tcp.<domain>`, `_imap._tcp.<domain>`, `_submissions._tcp.<domain>`
//!   and `_submission._tcp.<domain>` (type SRV) to the DNS servers configured in the system;
//! - if SRV did not yield both servers: HTTPS GET (port 443) to
//!   `autoconfig.<domain>/mail/config-v1.1.xml?emailaddress=<address>`, then, if that gave
//!   nothing, `<domain>/.well-known/autoconfig/mail/config-v1.1.xml`. Host names are resolved by
//!   the system resolver. Redirects are followed only to `https` URLs on port 443 of `<domain>`
//!   or its subdomains (at most 3). The operating system may check the certificate's
//!   revocation status (OCSP/CRL) with the issuing authority.
//!
//! - Stage 14 (decision 0041): HTTPS GET (port 443) to `<domain>/.well-known/jmap` without
//!   credentials, to learn whether the domain has a JMAP server (it answers 401 or with a
//!   session resource). Redirects only to `https` on `<domain>` or its subdomains (at most 3).
//!   An autoconfig file may also list one (`<incomingServer type="jmap">`).
//!
//! Results never contain a password, and every failure on the way (no record, timeout,
//! unreachable, bad file) ends in `source: none`, so the user fills in the form by hand. Only
//! an address that cannot be valid is an error (`invalid_argument`).

pub mod autoconfig;
pub mod known;
pub mod srv;

use mail_model::{CoreError, DiscoveryResult, DiscoverySource, ProviderHint, ServerSettings};

use self::autoconfig::{AutoconfigFetch, HttpsFetcher};
use self::srv::{SrvResolver, SystemSrvResolver};

/// Server settings from the built-in provider table. Never touches the network.
pub fn discover_known(email: &str) -> Result<DiscoveryResult, CoreError> {
    let address = EmailAddress::parse(email)?;
    let Some(domain) = address.dns_domain() else {
        return Ok(nothing(None));
    };
    let Some(provider) = known::find(domain) else {
        return Ok(nothing(None));
    };
    Ok(DiscoveryResult {
        source: DiscoverySource::KnownProvider,
        imap: Some(provider.imap.settings(&address)),
        smtp: Some(provider.smtp.settings(&address)),
        provider_hint: provider.hint,
        jmap: None,
    })
}

/// DNS SRV, then the domain's autoconfig file, with the system's DNS and TLS configuration.
/// Call only after the user asked for it (it contacts the network, see the module docs).
pub async fn discover_online(email: &str) -> Result<DiscoveryResult, CoreError> {
    OnlineDiscovery::new(SystemSrvResolver::new(), HttpsFetcher::new())
        .discover(email)
        .await
}

/// [`discover_online`] with an injectable resolver and fetcher (tests use local fakes).
#[derive(Debug)]
pub struct OnlineDiscovery<R, F> {
    resolver: R,
    fetcher: F,
}

impl<R: SrvResolver, F: AutoconfigFetch> OnlineDiscovery<R, F> {
    pub const fn new(resolver: R, fetcher: F) -> Self {
        Self { resolver, fetcher }
    }

    /// Settings for `email`. Precedence per server: SRV, then autoconfig. `source` names where
    /// the IMAP settings came from (or the SMTP settings, if there are no IMAP settings).
    /// A provider hint from the built-in table is attached for known domains.
    pub async fn discover(&self, email: &str) -> Result<DiscoveryResult, CoreError> {
        let address = EmailAddress::parse(email)?;
        let Some(domain) = address.dns_domain() else {
            return Ok(nothing(None));
        };
        let hint = known::find(domain).and_then(|provider| provider.hint);
        if !domain.contains('.') {
            // `localhost` and other single labels: nothing the user's provider could publish.
            return Ok(nothing(hint));
        }

        let from_srv = srv::discover(&self.resolver, domain, &address).await;
        let mut found = Found::default();
        found.add(DiscoverySource::Srv, from_srv);
        if !found.complete()
            && let Some(from_file) = autoconfig::discover(&self.fetcher, domain, &address).await
        {
            found.add(DiscoverySource::Autoconfig, from_file);
        }
        // Stage 14 (decision 0041): JMAP is offered beside IMAP, never instead of it.
        if found.jmap.is_none() {
            found.jmap = self.fetcher.probe_jmap(domain, &address).await;
        }
        let result = found.into_result(hint);
        tracing::info!(domain, source = ?result.source, imap = result.imap.is_some(),
            smtp = result.smtp.is_some(), jmap = result.jmap.is_some(),
            "online server discovery finished");
        Ok(result)
    }
}

/// IMAP and SMTP settings found by one discovery method; either may be missing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Servers {
    pub imap: Option<ServerSettings>,
    pub smtp: Option<ServerSettings>,
    /// Stage 14: a JMAP server (autoconfig `type="jmap"`).
    pub jmap: Option<ServerSettings>,
}

#[derive(Default)]
struct Found {
    imap: Option<(DiscoverySource, ServerSettings)>,
    smtp: Option<(DiscoverySource, ServerSettings)>,
    jmap: Option<ServerSettings>,
}

impl Found {
    /// Fills the servers still missing; earlier sources win.
    fn add(&mut self, source: DiscoverySource, servers: Servers) {
        if self.imap.is_none() {
            self.imap = servers.imap.map(|s| (source, s));
        }
        if self.smtp.is_none() {
            self.smtp = servers.smtp.map(|s| (source, s));
        }
        if self.jmap.is_none() {
            self.jmap = servers.jmap;
        }
    }

    const fn complete(&self) -> bool {
        self.imap.is_some() && self.smtp.is_some()
    }

    fn into_result(self, hint: Option<ProviderHint>) -> DiscoveryResult {
        let source = self
            .imap
            .as_ref()
            .or(self.smtp.as_ref())
            .map_or(DiscoverySource::None, |(source, _)| *source);
        // Stage 14 (decision 0034): a Google Workspace domain is Gmail too; its servers say so.
        let hint = hint.or_else(|| {
            self.imap
                .as_ref()
                .filter(|(_, s)| mail_model::is_gmail_host(&s.host))
                .map(|_| ProviderHint::Gmail)
        });
        DiscoveryResult {
            source,
            imap: self.imap.map(|(_, s)| s),
            smtp: self.smtp.map(|(_, s)| s),
            provider_hint: hint,
            jmap: self.jmap,
        }
    }
}

const fn nothing(hint: Option<ProviderHint>) -> DiscoveryResult {
    DiscoveryResult {
        source: DiscoverySource::None,
        imap: None,
        smtp: None,
        provider_hint: hint,
        jmap: None,
    }
}

/// An email address split for discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailAddress {
    /// The address as typed, trimmed. `%EMAILADDRESS%` in user names.
    address: String,
    local: String,
    /// The domain as typed, lowercased (may be Unicode). `%EMAILDOMAIN%` in user names.
    domain: String,
    /// The domain in ASCII (IDNA) form for DNS and URLs, `None` for an address literal or an
    /// IP address.
    dns_domain: Option<String>,
}

impl EmailAddress {
    /// Accepts `local@domain` with a plausible domain; at least as strict as
    /// `AccountSettings::validated`.
    pub fn parse(email: &str) -> Result<Self, CoreError> {
        let invalid =
            || CoreError::InvalidArgument(format!("email address {email:?} is not valid"));
        let address = email.trim();
        if address.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(invalid());
        }
        let (local, domain) = address.rsplit_once('@').ok_or_else(invalid)?;
        let domain = domain.strip_suffix('.').unwrap_or(domain).to_lowercase();
        if local.is_empty() || domain.is_empty() || domain.len() > 253 {
            return Err(invalid());
        }
        let dns_domain = if domain.starts_with('[') && domain.ends_with(']') {
            None // address literal, e.g. user@[192.0.2.1]
        } else if domain.contains(['/', '\\', '?', '#', ':', '@', '[', ']', '%']) {
            return Err(invalid());
        } else {
            let url = reqwest::Url::parse(&format!("https://{domain}/")).map_err(|_| invalid())?;
            if url.path() != "/" || url.query().is_some() {
                return Err(invalid());
            }
            match url.domain() {
                Some(ascii) if is_host_name(ascii) => Some(ascii.to_owned()),
                Some(_) => return Err(invalid()),
                None => None, // an IP address
            }
        };
        Ok(Self {
            address: address.to_owned(),
            local: local.to_owned(),
            domain,
            dns_domain,
        })
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn local_part(&self) -> &str {
        &self.local
    }

    /// The domain in ASCII form, `None` if the address has no DNS domain.
    pub fn dns_domain(&self) -> Option<&str> {
        self.dns_domain.as_deref()
    }

    /// Replaces `%EMAILADDRESS%`, `%EMAILLOCALPART%` and `%EMAILDOMAIN%` (Thunderbird's
    /// placeholders). In host names the domain is the ASCII form. `None` if the template
    /// contains another placeholder or nothing is left.
    pub fn expand(&self, template: &str, for_host: bool) -> Option<String> {
        let domain = if for_host {
            self.dns_domain.as_deref()?
        } else {
            &self.domain
        };
        let expanded = template
            .trim()
            .replace("%EMAILADDRESS%", &self.address)
            .replace("%EMAILLOCALPART%", &self.local)
            .replace("%EMAILDOMAIN%", domain);
        (!expanded.is_empty() && !expanded.contains('%')).then_some(expanded)
    }
}

/// A DNS host name (or dotted IPv4 address) as servers are configured: ASCII letters, digits
/// and hyphens in labels of 1 to 63 characters, at most 253 characters, no trailing dot.
pub fn is_host_name(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Error of one discovery step. Only logged: the result of a failed step is "nothing found".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupError(pub String);

impl std::fmt::Display for LookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LookupError {}

/// `err` and its sources, for log lines (library errors often hide the cause in the chain).
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests;
