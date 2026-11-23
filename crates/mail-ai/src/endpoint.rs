//! The provider's address: validation and the "local" label.
//!
//! [`locality`] is a pure function of the URL. It is a promise the transport keeps: a profile
//! labelled local only ever connects to addresses of that kind. IP literals are classified as
//! they are; for host names the resolved addresses are checked before connecting
//! ([`address_allowed`], used by the transport's resolver), so a name that only looks local
//! (`nas`, `box.local`) but resolves to a public address is refused instead of silently sending
//! mail to the internet under the "local" label.
//!
//! Further rules for every profile: only `http` and `https`; `http` only for local endpoints
//! (no mail and no key in clear text over the internet); no user name or password in the URL;
//! no query or fragment (the paths are fixed by the provider kind).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

use crate::AiError;

/// Where an endpoint is, as far as the user is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locality {
    /// This computer (loopback, `localhost`).
    ThisDevice,
    /// The user's own network (private and link-local ranges, `.local`, `.home.arpa`,
    /// `.internal`, `.lan`, single-label names).
    LocalNetwork,
    /// Anything else: a cloud provider.
    Internet,
}

impl Locality {
    /// Shown as "local": nothing leaves this computer or the own network.
    pub fn is_local(self) -> bool {
        !matches!(self, Self::Internet)
    }
}

/// The locality of a base URL as the user typed it. Errors for URLs that are not usable at all
/// (building a provider validates the rest: scheme, credentials in the URL, `http` only locally).
pub fn locality_of(base_url: &str) -> Result<Locality, AiError> {
    let url = parse_url(base_url)?;
    Ok(locality(&url))
}

/// Classifies the host of `url`. Pure: no DNS.
pub fn locality(url: &Url) -> Locality {
    match url.host() {
        Some(Host::Ipv4(ip)) => ip_locality(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => ip_locality(IpAddr::V6(ip)),
        Some(Host::Domain(name)) => name_locality(name),
        None => Locality::Internet,
    }
}

/// Locality of an address.
pub fn ip_locality(ip: IpAddr) -> Locality {
    match ip {
        IpAddr::V4(v4) => v4_locality(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => v4_locality(v4),
            None => v6_locality(v6),
        },
    }
}

fn v4_locality(ip: Ipv4Addr) -> Locality {
    if ip.is_loopback() || ip.is_unspecified() {
        Locality::ThisDevice
    } else if ip.is_private() || ip.is_link_local() {
        Locality::LocalNetwork
    } else {
        // Including carrier-grade NAT (100.64.0.0/10): that is the provider's network, not the
        // user's. Overlay networks there (Tailscale) count as internet; the label errs on the
        // side of "cloud".
        Locality::Internet
    }
}

fn v6_locality(ip: Ipv6Addr) -> Locality {
    if ip.is_loopback() || ip.is_unspecified() {
        return Locality::ThisDevice;
    }
    let first = ip.segments()[0];
    // Unique local (fc00::/7) and link-local (fe80::/10).
    if first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80 {
        Locality::LocalNetwork
    } else {
        Locality::Internet
    }
}

/// Suffixes of names that only exist in the own network.
const LOCAL_SUFFIXES: &[&str] = &[".local", ".home.arpa", ".internal", ".lan"];

fn name_locality(name: &str) -> Locality {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    if name == "localhost" || name.ends_with(".localhost") {
        Locality::ThisDevice
    } else if !name.contains('.') || LOCAL_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        Locality::LocalNetwork
    } else {
        Locality::Internet
    }
}

/// Whether a resolved address keeps the promise of `locality`: loopback for
/// [`Locality::ThisDevice`], loopback or the own network for [`Locality::LocalNetwork`],
/// anything for [`Locality::Internet`].
pub(crate) fn address_allowed(locality: Locality, ip: IpAddr) -> bool {
    match (locality, ip_locality(ip)) {
        (Locality::Internet, _) => true,
        (Locality::ThisDevice, found) => found == Locality::ThisDevice,
        (Locality::LocalNetwork, found) => found.is_local(),
    }
}

/// A validated base URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// Without trailing slash, so paths can be appended.
    base: String,
    locality: Locality,
    /// Host (and port, if not the default) for display and logs.
    host: String,
    /// The host is a name (not an IP literal): the transport checks its addresses.
    is_name: bool,
}

impl Endpoint {
    pub fn parse(base_url: &str) -> Result<Self, AiError> {
        let url = parse_url(base_url)?;
        let locality = locality(&url);
        if url.scheme() == "http" && !locality.is_local() {
            return Err(AiError::InvalidConfig(
                "plain http is only allowed for local endpoints; use https".into(),
            ));
        }
        let host = match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            (None, _) => return Err(AiError::InvalidConfig("the URL has no host".into())),
        };
        let is_name = matches!(url.host(), Some(Host::Domain(_)));
        let base = url.as_str().trim_end_matches('/').to_owned();
        Ok(Self {
            base,
            locality,
            host,
            is_name,
        })
    }

    pub fn locality(&self) -> Locality {
        self.locality
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn is_name(&self) -> bool {
        self.is_name
    }

    /// The base path ends with this segment (`/v1`).
    pub fn ends_with(&self, segment: &str) -> bool {
        self.base.ends_with(segment)
    }

    /// `base` + `path` (`path` starts with `/`, fixed by the provider; never from model output).
    pub fn url(&self, path: &str) -> Result<Url, AiError> {
        Url::parse(&format!("{}{path}", self.base))
            .map_err(|err| AiError::InvalidConfig(format!("URL: {err}")))
    }
}

fn parse_url(base_url: &str) -> Result<Url, AiError> {
    let trimmed = base_url.trim();
    let url =
        Url::parse(trimmed).map_err(|err| AiError::InvalidConfig(format!("not a URL ({err})")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(AiError::InvalidConfig(format!(
            "scheme {} is not supported; use https (or http for a local server)",
            url.scheme()
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AiError::InvalidConfig(
            "the URL must not contain a user name or password; enter the key separately".into(),
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(AiError::InvalidConfig(
            "the URL must not contain a query or fragment".into(),
        ));
    }
    if url.host().is_none() {
        return Err(AiError::InvalidConfig("the URL has no host".into()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(url: &str) -> Locality {
        locality_of(url).expect(url)
    }

    #[test]
    fn this_device() {
        for url in [
            "http://localhost:11434/v1",
            "http://LOCALHOST.:1234/v1",
            "http://ollama.localhost/v1",
            "http://127.0.0.1:8080/v1",
            "http://127.9.8.7/v1",
            "http://2130706433/v1", // 127.0.0.1 as a number
            "http://0.0.0.0:1234/v1",
            "http://[::1]:11434/v1",
            "http://[::ffff:127.0.0.1]/v1",
            "http://[::]/v1",
        ] {
            assert_eq!(of(url), Locality::ThisDevice, "{url}");
        }
    }

    #[test]
    fn own_network() {
        for url in [
            "http://192.168.1.20:11434/v1",
            "http://10.0.0.5/v1",
            "http://172.16.0.1/v1",
            "http://172.31.255.254/v1",
            "http://169.254.1.2/v1",
            "http://[fe80::1]/v1",
            "http://[fd12:3456::1]/v1",
            "http://[::ffff:192.168.1.10]/v1",
            "http://workstation.local:1234/v1",
            "http://llm.home.arpa/v1",
            "http://gpu.internal/v1",
            "http://router.lan/v1",
            "http://nas:11434/v1",
        ] {
            assert_eq!(of(url), Locality::LocalNetwork, "{url}");
        }
    }

    #[test]
    fn internet() {
        for url in [
            "https://api.openai.com/v1",
            "https://openrouter.ai/api/v1",
            "https://api.anthropic.com",
            "https://8.8.8.8/v1",
            "https://100.64.0.1/v1",
            "https://172.32.0.1/v1",
            "https://[2001:db8::1]/v1",
            "https://[2606:4700::1111]/v1",
            "https://[::ffff:8.8.8.8]/v1",
            "https://localhost.example.com/v1",
            "https://local.example.org/v1",
            "https://example.localhost.evil.com/v1",
        ] {
            assert_eq!(of(url), Locality::Internet, "{url}");
        }
    }

    #[test]
    fn unusable_urls_are_rejected() {
        for url in [
            "",
            "localhost:11434",
            "ftp://example.org/v1",
            "file:///etc/passwd",
            "https://user:pw@api.example.org/v1",
            "https://key@api.example.org/v1",
            "https://api.example.org/v1?key=abc",
            "https://api.example.org/v1#x",
        ] {
            assert!(Endpoint::parse(url).is_err(), "{url}");
        }
    }

    #[test]
    fn plain_http_only_for_local_endpoints() {
        assert!(Endpoint::parse("http://localhost:11434/v1").is_ok());
        assert!(Endpoint::parse("http://192.168.1.5:1234/v1").is_ok());
        let err = Endpoint::parse("http://api.openai.com/v1").expect_err("http to the internet");
        assert!(matches!(err, AiError::InvalidConfig(_)));
        assert!(Endpoint::parse("https://api.openai.com/v1").is_ok());
    }

    #[test]
    fn endpoint_paths_and_display() {
        let endpoint = Endpoint::parse(" http://localhost:11434/v1/ ").expect("endpoint");
        assert_eq!(endpoint.host(), "localhost:11434");
        assert!(endpoint.is_name());
        assert!(endpoint.ends_with("/v1"));
        assert_eq!(
            endpoint.url("/chat/completions").expect("url").as_str(),
            "http://localhost:11434/v1/chat/completions"
        );
        let anthropic = Endpoint::parse("https://api.anthropic.com").expect("endpoint");
        assert_eq!(anthropic.host(), "api.anthropic.com");
        assert_eq!(
            anthropic.url("/v1/messages").expect("url").as_str(),
            "https://api.anthropic.com/v1/messages"
        );
        assert!(
            !Endpoint::parse("http://127.0.0.1:1/")
                .expect("ip")
                .is_name()
        );
    }

    #[test]
    fn resolved_addresses_must_keep_the_label() {
        let ip = |s: &str| s.parse::<IpAddr>().expect("ip");
        assert!(address_allowed(Locality::ThisDevice, ip("127.0.0.1")));
        assert!(address_allowed(Locality::ThisDevice, ip("::1")));
        assert!(!address_allowed(Locality::ThisDevice, ip("192.168.1.2")));
        assert!(!address_allowed(Locality::ThisDevice, ip("93.184.216.34")));
        assert!(address_allowed(Locality::LocalNetwork, ip("192.168.1.2")));
        assert!(address_allowed(Locality::LocalNetwork, ip("fd00::1")));
        assert!(address_allowed(Locality::LocalNetwork, ip("127.0.0.1")));
        assert!(!address_allowed(
            Locality::LocalNetwork,
            ip("93.184.216.34")
        ));
        assert!(!address_allowed(
            Locality::LocalNetwork,
            ip("2606:4700::1111")
        ));
        assert!(address_allowed(Locality::Internet, ip("93.184.216.34")));
        assert!(address_allowed(Locality::Internet, ip("10.0.0.1")));
    }
}
