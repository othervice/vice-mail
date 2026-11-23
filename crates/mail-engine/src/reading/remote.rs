//! External images of a mail, after the user allowed them (architecture, security boundary 4).
//!
//! The core loads them itself, so the mail frame never talks to the network:
//! - reqwest over rustls with the system's certificate check (as for autoconfig), HTTP/1.1,
//!   no cookie store, no `Referer`, no proxy, no retries, a neutral `User-Agent` without
//!   product name or version;
//! - only `http` and `https`; redirects are followed by hand, at most [`MAX_REDIRECTS`], and
//!   only to `http`/`https` again;
//! - **never to private, loopback or link-local addresses** (or other non-public ranges): a
//!   mail must not make the app talk to the user's router, NAS or local services. IP literals
//!   are checked before the request, host names after resolution (the connection only uses the
//!   checked addresses, so a DNS answer cannot switch to a private address afterwards).
//!   [`RemoteConfig::allow_loopback`] lifts this for loopback only, for tests with a local
//!   counter server; the product never sets it;
//! - time limits per connection, per request and for the whole mail; size limits per image
//!   and in total (the renderer's limits); at most [`MAX_IMAGES`] different URLs.
//!
//! What is a picture is decided by `mail-content` from the bytes (no SVG); here only sizes and
//! the transport are checked.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use mail_content::ImageBytes;
use reqwest::{StatusCode, Url};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::discovery::autoconfig::NEUTRAL_USER_AGENT;

pub const MAX_REDIRECTS: usize = 3;
/// Different image URLs fetched per mail; the rest stays blocked.
pub const MAX_IMAGES: usize = 200;
/// Images fetched at the same time.
const PARALLEL: usize = 6;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// All images of one mail, then whatever did not arrive stays blocked.
pub const MAIL_DEADLINE: Duration = Duration::from_secs(30);

/// How external images are fetched. The default is the product's.
#[derive(Debug, Clone, Default)]
pub struct RemoteConfig {
    /// **Tests only:** images on loopback addresses (`127.0.0.0/8`, `::1`) may load, for the
    /// local counter server of the tests. Private and link-local addresses stay refused. The
    /// product never sets it; the desktop-test build sets it from an environment variable that
    /// only exists with the `e2e` feature.
    pub allow_loopback: bool,
}

/// The image fetcher. Cheap to clone.
#[derive(Clone)]
pub(crate) struct RemoteImages {
    client: Result<reqwest::Client, String>,
    allow_loopback: bool,
}

impl RemoteImages {
    pub fn new(config: &RemoteConfig) -> Self {
        Self {
            client: build_client(config.allow_loopback),
            allow_loopback: config.allow_loopback,
        }
    }

    /// Fetches `urls` (absolute `http`/`https`, as `mail-content` lists them). Returns the
    /// bytes of those that arrived within the limits; failures are logged with the host only.
    pub async fn fetch_all(
        &self,
        urls: Vec<String>,
        max_image: usize,
        max_total: usize,
    ) -> HashMap<String, ImageBytes> {
        let Ok(client) = self.client.clone() else {
            if let Err(err) = &self.client {
                tracing::error!(error = %err, "no HTTP client for external images");
            }
            return HashMap::new();
        };
        let budget = Arc::new(AtomicUsize::new(max_total));
        let permits = Arc::new(Semaphore::new(PARALLEL));
        let mut tasks = JoinSet::new();
        for url in urls.into_iter().take(MAX_IMAGES) {
            let client = client.clone();
            let budget = Arc::clone(&budget);
            let permits = Arc::clone(&permits);
            let allow_loopback = self.allow_loopback;
            tasks.spawn(async move {
                let _permit = permits.acquire_owned().await.ok()?;
                match fetch_one(&client, &url, allow_loopback, max_image, &budget).await {
                    Ok(bytes) => Some((url, bytes)),
                    Err(reason) => {
                        let host = Url::parse(&url)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_owned));
                        tracing::info!(host, %reason, "external image not loaded");
                        None
                    }
                }
            });
        }
        let mut images = HashMap::new();
        let collect = async {
            while let Some(joined) = tasks.join_next().await {
                if let Ok(Some((url, bytes))) = joined {
                    images.insert(url, bytes);
                }
            }
        };
        if tokio::time::timeout(MAIL_DEADLINE, collect).await.is_err() {
            tracing::info!("external images: time limit for the mail reached");
        }
        images
    }
}

fn build_client(allow_loopback: bool) -> Result<reqwest::Client, String> {
    // The app's TLS setup: system trust store, the one crypto provider.
    let tls = mail_imap::tls::client_config(None).map_err(|err| err.to_string())?;
    reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .retry(reqwest::retry::never())
        .user_agent(NEUTRAL_USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .dns_resolver(PublicResolver { allow_loopback })
        .build()
        .map_err(|err| format!("HTTP client: {err}"))
}

async fn fetch_one(
    client: &reqwest::Client,
    url: &str,
    allow_loopback: bool,
    max_image: usize,
    budget: &AtomicUsize,
) -> Result<ImageBytes, String> {
    let mut url = Url::parse(url).map_err(|err| format!("bad URL: {err}"))?;
    let mut redirects = 0;
    loop {
        check_url(&url, allow_loopback)?;
        let response = client
            .get(url.clone())
            .header(reqwest::header::ACCEPT, "image/*")
            .send()
            .await
            .map_err(|err| err.without_url().to_string())?;
        let status = response.status();
        if status.is_redirection() {
            redirects += 1;
            if redirects > MAX_REDIRECTS {
                return Err("too many redirects".into());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| format!("redirect ({status}) without location"))?;
            url = url
                .join(location)
                .map_err(|err| format!("bad redirect location: {err}"))?;
            continue;
        }
        if status != StatusCode::OK {
            return Err(format!("status {status}"));
        }
        return read_limited(response, max_image, budget).await;
    }
}

/// Scheme and, for IP literals, the address. Host names are checked by [`PublicResolver`].
/// Also used for one-click unsubscribing (stage 9).
pub(crate) fn check_url(url: &Url, allow_loopback: bool) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("scheme {} refused", url.scheme()));
    }
    let ip = match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip),
        Some(url::Host::Domain(_)) => return Ok(()),
        None => return Err("no host".into()),
    };
    if target_allowed(ip, allow_loopback) {
        Ok(())
    } else {
        Err("refused: private, loopback or link-local address".into())
    }
}

async fn read_limited(
    mut response: reqwest::Response,
    max_image: usize,
    budget: &AtomicUsize,
) -> Result<ImageBytes, String> {
    let too_large = || format!("larger than {max_image} bytes");
    if response
        .content_length()
        .is_some_and(|len| len > max_image as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| err.without_url().to_string())?
    {
        if body.len() + chunk.len() > max_image {
            return Err(too_large());
        }
        // Takes the bytes from the mail's total budget; refuses once it is used up.
        if !take_budget(budget, chunk.len()) {
            return Err("the mail's image budget is used up".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Arc::from(body))
}

/// Takes `amount` bytes from the shared budget; `false` (and nothing taken) if not enough is
/// left.
fn take_budget(budget: &AtomicUsize, amount: usize) -> bool {
    let mut left = budget.load(Ordering::SeqCst);
    loop {
        let Some(rest) = left.checked_sub(amount) else {
            return false;
        };
        match budget.compare_exchange(left, rest, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(now) => left = now,
        }
    }
}

/// Resolves host names with the system resolver and keeps only addresses that may be
/// contacted ([`target_allowed`]); none left is an error. Also used for one-click
/// unsubscribing (stage 9).
pub(crate) struct PublicResolver {
    allow_loopback: bool,
}

impl PublicResolver {
    pub(crate) const fn new(allow_loopback: bool) -> Self {
        Self { allow_loopback }
    }
}

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        let allow_loopback = self.allow_loopback;
        Box::pin(async move {
            let resolved = tokio::net::lookup_host((host.as_str(), 0)).await?;
            let allowed: Vec<SocketAddr> = resolved
                .filter(|addr| target_allowed(addr.ip(), allow_loopback))
                .collect();
            if allowed.is_empty() {
                return Err(
                    "refused: the host has no public address (private, loopback or link-local)"
                        .into(),
                );
            }
            Ok(Box::new(allowed.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Whether the app may connect to `ip` for an external image: only public unicast addresses,
/// plus loopback when the test switch is on.
pub(crate) fn target_allowed(ip: IpAddr, allow_loopback: bool) -> bool {
    is_public(ip) || (allow_loopback && is_loopback(ip))
}

fn is_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || a == 0 // "this network"
        || (a == 100 && (64..128).contains(&b)) // shared address space (carrier-grade NAT)
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (18..20).contains(&b)) // benchmarking
        || a >= 240) // reserved
}

fn public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = ip.segments();
    let embedded = |hi: u16, lo: u16| Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo));
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    match s {
        // IPv4-compatible (deprecated) and other addresses in ::/96.
        [0, 0, 0, 0, 0, 0, _, _] => false,
        // NAT64: the IPv4 address decides.
        [0x64, 0xff9b, 0, 0, 0, 0, hi, lo] => public_v4(embedded(hi, lo)),
        // 6to4: the embedded IPv4 address decides.
        [0x2002, hi, lo, ..] => public_v4(embedded(hi, lo)),
        // Documentation.
        [0x2001, 0x0db8, ..] => false,
        // Unique local (fc00::/7), link-local (fe80::/10), site-local (fec0::/10, deprecated).
        [first, ..]
            if first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80 || first & 0xffc0 == 0xfec0 =>
        {
            false
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().expect("ip")
    }

    #[test]
    fn only_public_addresses_are_contacted() {
        for public in [
            "93.184.216.34",
            "1.1.1.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
            "2002:0808:0808::1",
        ] {
            assert!(target_allowed(ip(public), false), "{public}");
        }
        for private in [
            "127.0.0.1",
            "127.8.9.10",
            "10.0.0.1",
            "172.16.5.4",
            "192.168.10.251",
            "169.254.169.254",
            "0.0.0.0",
            "255.255.255.255",
            "224.0.0.1",
            "100.64.0.1",
            "198.18.0.1",
            "192.0.2.1",
            "240.0.0.1",
            "::1",
            "::",
            "fe80::1",
            "fd00::1",
            "fc00::1",
            "fec0::1",
            "ff02::1",
            "::ffff:192.168.1.1",
            "::ffff:127.0.0.1",
            "::127.0.0.1",
            "64:ff9b::10.0.0.1",
            "2002:c0a8:0101::1",
            "2001:db8::1",
        ] {
            assert!(!target_allowed(ip(private), false), "{private}");
        }
    }

    #[test]
    fn the_test_switch_opens_loopback_only() {
        assert!(target_allowed(ip("127.0.0.1"), true));
        assert!(target_allowed(ip("::1"), true));
        assert!(target_allowed(ip("::ffff:127.0.0.1"), true));
        for still_refused in [
            "192.168.1.1",
            "10.1.2.3",
            "169.254.1.1",
            "fe80::1",
            "0.0.0.0",
        ] {
            assert!(!target_allowed(ip(still_refused), true), "{still_refused}");
        }
    }

    #[test]
    fn urls_are_checked_before_the_request() {
        let check = |u: &str| check_url(&Url::parse(u).expect("url"), false);
        assert!(check("https://images.example.org/a.png").is_ok());
        assert!(check("http://93.184.216.34/a.png").is_ok());
        assert!(check("http://127.0.0.1:18999/a.png").is_err());
        assert!(check("http://[::1]/a.png").is_err());
        assert!(check("http://192.168.10.1/a.png").is_err());
        assert!(check("ftp://example.org/a.png").is_err());
        assert!(check("file:///etc/passwd").is_err());
    }

    #[tokio::test]
    async fn host_names_resolving_to_loopback_are_refused() {
        // `localhost` resolves to loopback addresses only.
        let images = RemoteImages::new(&RemoteConfig::default());
        let fetched = images
            .fetch_all(vec!["http://localhost:9/x.png".into()], 1024, 4096)
            .await;
        assert!(fetched.is_empty());
        let resolver = PublicResolver {
            allow_loopback: false,
        };
        let name: reqwest::dns::Name = "localhost".parse().expect("name");
        let resolved = reqwest::dns::Resolve::resolve(&resolver, name).await;
        assert!(resolved.is_err());
    }
}
