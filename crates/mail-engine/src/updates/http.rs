//! The HTTP side of the update check (decision 0037), for the shell's update source: the
//! reqwest settings the updater plugin gets through `configure_client`, and the hosts an update
//! may come from.
//!
//! Same rules as the app's other HTTP clients: the app's rustls setup (system trust store,
//! `aws-lc-rs`), HTTP/1.1, no proxy, no cookies, no referrer, no retries, a neutral user agent.
//! Redirects only over HTTPS and only to the update hosts: the release page on `github.com`
//! sends its downloads on to GitHub's file hosts, nothing else is followed.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use mail_model::UpdateError;
use url::Url;

/// GitHub answers `/releases/latest/download/<file>` with a redirect to its file hosts.
const GITHUB_FILE_HOSTS: &[&str] = &[
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// At most this many redirects per request (GitHub uses two).
const MAX_REDIRECTS: usize = 5;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Between two reads; a download of several megabytes has no overall limit.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The hosts an update check and its download may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateHosts {
    hosts: Vec<String>,
    /// On `github.com`: the endpoint's release path (`/<owner>/<repo>/releases/`). Every
    /// address there must lie under it, so a changed manifest cannot point to another
    /// repository's files (stage 15 review). GitHub's file hosts are reached only by redirect.
    github_releases: Option<String>,
    /// Plain HTTP to a loopback address: only when the endpoint itself is one (the local update
    /// server of tests and desktop tests). The shipped endpoint is HTTPS.
    insecure_loopback: bool,
}

impl UpdateHosts {
    /// The endpoint's host, plus GitHub's file hosts for an endpoint on `github.com`.
    pub fn for_endpoint(endpoint: &Url) -> Result<Self, UpdateError> {
        let host = endpoint
            .host_str()
            .ok_or_else(|| UpdateError::CheckFailed("the update address has no host".into()))?
            .to_ascii_lowercase();
        let insecure_loopback = match endpoint.scheme() {
            "https" => false,
            "http" if is_loopback(endpoint) => true,
            other => {
                return Err(UpdateError::CheckFailed(format!(
                    "the update address must use https, not {other}"
                )));
            }
        };
        let mut hosts = vec![host.clone()];
        let mut github_releases = None;
        if host == "github.com" {
            hosts.extend(GITHUB_FILE_HOSTS.iter().map(|h| (*h).to_owned()));
            let path = endpoint.path();
            github_releases = path
                .find("/releases/")
                .map(|end| path[..end + "/releases/".len()].to_owned())
                .filter(|prefix| prefix.split('/').filter(|s| !s.is_empty()).count() == 3);
        }
        Ok(Self {
            hosts,
            github_releases,
            insecure_loopback,
        })
    }

    /// Whether `url` may be requested: HTTPS (or HTTP to loopback for a loopback endpoint) and
    /// one of the hosts.
    pub fn permits(&self, url: &Url) -> bool {
        let scheme_ok = match url.scheme() {
            "https" => true,
            "http" => self.insecure_loopback && is_loopback(url),
            _ => false,
        };
        let Some(host) = url.host_str() else {
            return false;
        };
        let path_ok = match &self.github_releases {
            Some(prefix) if host.eq_ignore_ascii_case("github.com") => url
                .path()
                .get(..prefix.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(prefix)),
            _ => true,
        };
        scheme_ok && path_ok && self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
    }

    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        _ => false,
    }
}

/// A `configure_client` hook: the app's TLS configuration and the redirect rule for `hosts`.
pub type ClientConfigurator =
    Arc<dyn Fn(reqwest::ClientBuilder) -> reqwest::ClientBuilder + Send + Sync + 'static>;

/// Builds the hook. Fails only if the system's certificate verifier cannot be set up.
pub fn client_configurator(hosts: UpdateHosts) -> Result<ClientConfigurator, UpdateError> {
    let tls = mail_imap::tls::client_config(None)
        .map_err(|err| UpdateError::CheckFailed(format!("TLS setup: {err}")))?;
    Ok(Arc::new(move |builder: reqwest::ClientBuilder| {
        let hosts = hosts.clone();
        builder
            .tls_backend_preconfigured((*tls).clone())
            .http1_only()
            .no_proxy()
            .referer(false)
            .retry(reqwest::retry::never())
            .user_agent(crate::discovery::autoconfig::NEUTRAL_USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .read_timeout(READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if hosts.permits(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("redirect outside the update hosts")
                }
            }))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).expect("url")
    }

    #[test]
    fn github_endpoint_allows_its_file_hosts_over_https_only() {
        let hosts = UpdateHosts::for_endpoint(&url(
            "https://github.com/othervice/vice-mail/releases/latest/download/latest.json",
        ))
        .expect("hosts");
        assert!(hosts.permits(&url(
            "https://github.com/othervice/vice-mail/releases/download/v0.2.0/x.tar.gz"
        )));
        assert!(hosts.permits(&url("https://release-assets.githubusercontent.com/a?b=c")));
        assert!(hosts.permits(&url("https://objects.githubusercontent.com/a")));
        assert!(!hosts.permits(&url("http://github.com/a")), "no plain HTTP");
        assert!(
            !hosts.permits(&url("https://example.org/a")),
            "no other host"
        );
        assert!(!hosts.permits(&url("https://github.com.example.org/a")));
        assert!(!hosts.permits(&url("ftp://github.com/a")));
        assert!(
            !hosts.permits(&url("http://127.0.0.1/a")),
            "no loopback for a real endpoint"
        );
        // Stage 15 review: on github.com only this repository's releases.
        assert!(hosts.permits(&url(
            "https://github.com/othervice/vice-mail/releases/latest/download/latest.json"
        )));
        assert!(hosts.permits(&url(
            "https://github.com/OtherVice/Vice-Mail/releases/download/v0.2.0/x.tar.gz"
        )));
        for other in [
            "https://github.com/someone/else/releases/download/v9/x.tar.gz",
            "https://github.com/othervice/vice-mail-fork/releases/download/v9/x.tar.gz",
            "https://github.com/othervice/vice-mail/archive/x.tar.gz",
            "https://github.com/a",
        ] {
            assert!(!hosts.permits(&url(other)), "{other}");
        }
    }

    #[test]
    fn other_endpoints_allow_only_their_own_host() {
        let hosts = UpdateHosts::for_endpoint(&url("https://updates.example.org/latest.json"))
            .expect("hosts");
        assert_eq!(hosts.hosts(), ["updates.example.org"]);
        assert!(!hosts.permits(&url("https://objects.githubusercontent.com/a")));
    }

    #[test]
    fn plain_http_only_for_a_loopback_test_endpoint() {
        let hosts =
            UpdateHosts::for_endpoint(&url("http://127.0.0.1:4567/latest.json")).expect("hosts");
        assert!(hosts.permits(&url("http://127.0.0.1:4567/update.bin")));
        assert!(!hosts.permits(&url("http://10.0.0.1/update.bin")));
        assert!(
            UpdateHosts::for_endpoint(&url("http://updates.example.org/latest.json")).is_err(),
            "plain HTTP to a real host is refused"
        );
        assert!(UpdateHosts::for_endpoint(&url("file:///tmp/latest.json")).is_err());
    }

    #[test]
    fn the_configurator_builds_a_client() {
        let hosts = UpdateHosts::for_endpoint(&url("https://github.com/x")).expect("hosts");
        let configure = client_configurator(hosts).expect("configurator");
        configure(reqwest::Client::builder())
            .build()
            .expect("client");
    }
}
