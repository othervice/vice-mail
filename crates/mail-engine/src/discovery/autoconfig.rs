//! The domain's own autoconfig file (Thunderbird's `config-v1.1.xml` format).
//!
//! Fetched over HTTPS only, from two places of the address's domain, in this order:
//! 1. `https://autoconfig.<domain>/mail/config-v1.1.xml?emailaddress=<address>`
//! 2. `https://<domain>/.well-known/autoconfig/mail/config-v1.1.xml`
//!
//! Never from a third-party directory such as Mozilla's ISPDB.
//!
//! The HTTP client ([`HttpsFetcher`]): rustls with the app's TLS setup (`mail_imap::tls`: the
//! system's certificate check, decision 0002), HTTP/1.1, no cookies (no cookie store exists), no
//! `Referer`, no proxy, no compression, a neutral `User-Agent` without product name or
//! version, 4 s to connect and 8 s per request, at most [`MAX_BODY`] bytes.
//!
//! Redirects are followed by hand, at most [`MAX_REDIRECTS`], and only to `https` URLs on the
//! default port whose host is the domain itself or one of its subdomains (for example
//! `example.org` to `www.example.org`). The domain's owner controls those hosts, so following
//! them contacts nobody the user did not already choose by typing the address; a redirect to
//! any other host is refused (it would hand the configuration to a third party and widen the
//! list of allowed connections).
//!
//! The XML parser (roxmltree) rejects any DTD, so there are no entity expansions or external
//! entities, and it stops after [`MAX_XML_NODES`] nodes.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use mail_model::{Security, ServerSettings};
use reqwest::{StatusCode, Url};

use super::{EmailAddress, LookupError, Servers, error_chain, is_host_name};

/// Largest accepted file. Real files are a few kilobytes.
pub const MAX_BODY: usize = 128 * 1024;
pub const MAX_REDIRECTS: usize = 3;
pub const MAX_XML_NODES: u32 = 10_000;
/// Sent instead of a product name: says nothing about the app, its version or the system.
pub const NEUTRAL_USER_AGENT: &str = "Mozilla/5.0";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// The two URLs for `domain` (ASCII), in the order they are tried.
pub fn candidate_urls(domain: &str, address: &EmailAddress) -> Result<[Url; 2], LookupError> {
    let parse = |text: String| Url::parse(&text).map_err(|err| LookupError(format!("{err}")));
    let mut first = parse(format!("https://autoconfig.{domain}/mail/config-v1.1.xml"))?;
    first
        .query_pairs_mut()
        .append_pair("emailaddress", address.address());
    let second = parse(format!(
        "https://{domain}/.well-known/autoconfig/mail/config-v1.1.xml"
    ))?;
    Ok([first, second])
}

/// Fetches an autoconfig file. Injectable so tests need no network.
pub trait AutoconfigFetch: Send + Sync {
    /// The body of `url`, `None` if the server has no such file (any status but 200).
    /// Redirects stay within `domain` (see the module docs).
    fn fetch(
        &self,
        url: &Url,
        domain: &str,
    ) -> impl Future<Output = Result<Option<Vec<u8>>, LookupError>> + Send;

    /// Stage 14 (decision 0041): a JMAP server at `https://<domain>/.well-known/jmap`, asked
    /// without credentials. The default (test fakes) finds none.
    fn probe_jmap(
        &self,
        domain: &str,
        address: &EmailAddress,
    ) -> impl Future<Output = Option<ServerSettings>> + Send {
        let _ = (domain, address);
        std::future::ready(None)
    }
}

/// Tries both URLs and parses the first file that names at least one usable server.
/// Failures are logged and count as "no file".
pub async fn discover<F: AutoconfigFetch>(
    fetcher: &F,
    domain: &str,
    address: &EmailAddress,
) -> Option<Servers> {
    let urls = match candidate_urls(domain, address) {
        Ok(urls) => urls,
        Err(err) => {
            tracing::debug!(domain, error = %err, "no autoconfig URL");
            return None;
        }
    };
    for (index, url) in urls.iter().enumerate() {
        let place = if index == 0 {
            "autoconfig host"
        } else {
            "well-known"
        };
        match fetcher.fetch(url, domain).await {
            Ok(Some(body)) => match parse(&body, address) {
                Ok(servers) if servers.imap.is_some() || servers.smtp.is_some() => {
                    return Some(servers);
                }
                Ok(_) => tracing::debug!(domain, place, "autoconfig file has no usable server"),
                Err(err) => {
                    tracing::debug!(domain, place, error = %err, "autoconfig file rejected")
                }
            },
            Ok(None) => tracing::debug!(domain, place, "no autoconfig file"),
            Err(err) => tracing::debug!(domain, place, error = %err, "autoconfig fetch failed"),
        }
    }
    None
}

/// The product's fetcher. Construction never fails: if the TLS setup cannot be built, every
/// fetch returns that error.
#[derive(Debug, Clone)]
pub struct HttpsFetcher {
    client: Result<reqwest::Client, LookupError>,
}

impl HttpsFetcher {
    /// Certificates are checked against the system's trust store.
    pub fn new() -> Self {
        Self {
            client: build_client(mail_imap::tls::client_config(None), &[], REQUEST_TIMEOUT),
        }
    }

    /// For tests: trusts `extra_root` in addition to the system store, sends requests for the
    /// given host names to the given local addresses and gives up after `timeout`.
    #[cfg(test)]
    pub(crate) fn for_tests(
        extra_root: rustls::pki_types::CertificateDer<'static>,
        overrides: &[(&str, std::net::SocketAddr)],
        timeout: Duration,
    ) -> Self {
        Self {
            client: build_client(
                mail_imap::tls::client_config_with_roots(vec![extra_root]),
                overrides,
                timeout,
            ),
        }
    }
}

impl Default for HttpsFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// An HTTPS client over the app's TLS setup (`mail_imap::tls`: system trust store, the one
/// crypto provider).
fn build_client(
    tls: Result<Arc<rustls::ClientConfig>, mail_model::BackendError>,
    overrides: &[(&str, std::net::SocketAddr)],
    timeout: Duration,
) -> Result<reqwest::Client, LookupError> {
    let tls = tls.map_err(|err| LookupError(err.to_string()))?;
    let mut builder = reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
        .https_only(true)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .no_proxy()
        .retry(reqwest::retry::never())
        .user_agent(NEUTRAL_USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT.min(timeout))
        .timeout(timeout);
    for (host, addr) in overrides {
        builder = builder.resolve(host, *addr);
    }
    builder
        .build()
        .map_err(|err| LookupError(format!("HTTP client: {}", error_chain(&err))))
}

/// Whether a request to `url` is allowed for `domain` (the rules in the module docs).
pub fn allowed_url(url: &Url, domain: &str) -> bool {
    let host_ok = url.domain().is_some_and(|host| {
        host == domain
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    });
    url.scheme() == "https"
        && host_ok
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

impl AutoconfigFetch for HttpsFetcher {
    async fn fetch(&self, url: &Url, domain: &str) -> Result<Option<Vec<u8>>, LookupError> {
        let client = self.client.as_ref().map_err(Clone::clone)?;
        let mut url = url.clone();
        let mut redirects = 0;
        loop {
            if !allowed_url(&url, domain) {
                return Err(LookupError(format!(
                    "refused to fetch {} (only https on {domain} and its subdomains)",
                    url.host_str().unwrap_or("?")
                )));
            }
            let response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|err| LookupError(error_chain(&err.without_url())))?;
            let status = response.status();
            if matches!(
                status,
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            ) {
                redirects += 1;
                if redirects > MAX_REDIRECTS {
                    return Err(LookupError("too many redirects".into()));
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| LookupError(format!("redirect ({status}) without location")))?;
                url = url
                    .join(location)
                    .map_err(|err| LookupError(format!("bad redirect location: {err}")))?;
                continue;
            }
            if status != StatusCode::OK {
                return Ok(None);
            }
            return read_limited(response).await.map(Some);
        }
    }

    async fn probe_jmap(&self, domain: &str, address: &EmailAddress) -> Option<ServerSettings> {
        let client = self.client.as_ref().ok()?;
        match mail_jmap::probe(client, domain, 443, false).await {
            Ok(Some(found))
                if found.host == domain || found.host.ends_with(&format!(".{domain}")) =>
            {
                Some(ServerSettings {
                    host: found.host,
                    port: found.port,
                    security: Security::Tls,
                    username: address.address().to_owned(),
                })
            }
            Ok(_) => None,
            Err(err) => {
                tracing::debug!(domain, error = %err, "no JMAP session found");
                None
            }
        }
    }
}

async fn read_limited(mut response: reqwest::Response) -> Result<Vec<u8>, LookupError> {
    let too_large = || LookupError(format!("file larger than {MAX_BODY} bytes"));
    if response
        .content_length()
        .is_some_and(|len| len > MAX_BODY as u64)
    {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| LookupError(error_chain(&err.without_url())))?
    {
        if body.len() + chunk.len() > MAX_BODY {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Reads the IMAP and SMTP server of an autoconfig file. Per protocol the first listed server
/// that the app can use wins: `SSL` or `STARTTLS` (never `plain`), password sign-in (or no
/// method given; OAuth and the like are skipped), a plausible host name. Placeholders
/// `%EMAILADDRESS%`, `%EMAILLOCALPART%` and `%EMAILDOMAIN%` are filled in.
pub fn parse(body: &[u8], address: &EmailAddress) -> Result<Servers, LookupError> {
    if body.len() > MAX_BODY {
        return Err(LookupError(format!("file larger than {MAX_BODY} bytes")));
    }
    let text = std::str::from_utf8(body)
        .map_err(|_| LookupError("file is not UTF-8".into()))?
        .trim_start_matches('\u{feff}');
    let options = roxmltree::ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_XML_NODES,
        entity_resolver: None,
    };
    let document = roxmltree::Document::parse_with_options(text, options)
        .map_err(|err| LookupError(format!("not well-formed XML: {err}")))?;
    let root = document.root_element();
    if root.tag_name().name() != "clientConfig" {
        return Err(LookupError(format!(
            "root element is <{}>, not <clientConfig>",
            root.tag_name().name()
        )));
    }
    let provider = root
        .children()
        .find(|node| node.has_tag_name("emailProvider"))
        .ok_or_else(|| LookupError("no <emailProvider>".into()))?;

    let pick = |element: &str, kind: &str, ports: [u16; 2]| {
        provider
            .children()
            .filter(|node| node.has_tag_name(element) && node.attribute("type") == Some(kind))
            .find_map(|node| server(node, address, ports))
    };
    Ok(Servers {
        imap: pick("incomingServer", "imap", [993, 143]),
        smtp: pick("outgoingServer", "smtp", [465, 587]),
        // Stage 14 (decision 0041): HTTPS only; without a port 443.
        jmap: provider
            .children()
            .filter(|node| {
                node.has_tag_name("incomingServer") && node.attribute("type") == Some("jmap")
            })
            .find_map(|node| jmap_server(node, address)),
    })
}

/// A `<incomingServer type="jmap">`: host (and port) over TLS, password sign-in.
fn jmap_server(node: roxmltree::Node<'_, '_>, address: &EmailAddress) -> Option<ServerSettings> {
    let has_port = node.children().any(|c| c.has_tag_name("port"));
    let has_socket = node.children().any(|c| c.has_tag_name("socketType"));
    if !has_port && !has_socket {
        // `server` needs one of them; JMAP is HTTPS on 443 by default.
        let mut settings = server(node, address, [443, 443]).or_else(|| {
            let host = node
                .children()
                .find(|c| c.has_tag_name("hostname"))
                .and_then(|c| c.text())
                .map(str::trim)?;
            let host = address
                .expand(host, true)?
                .trim_end_matches('.')
                .to_ascii_lowercase();
            is_host_name(&host).then(|| ServerSettings {
                host,
                port: 443,
                security: Security::Tls,
                username: address.address().to_owned(),
            })
        })?;
        settings.security = Security::Tls;
        return Some(settings);
    }
    server(node, address, [443, 443]).filter(|s| s.security == Security::Tls)
}

/// One `<incomingServer>`/`<outgoingServer>`; `None` if the app cannot use it.
/// `default_ports` are for TLS and STARTTLS when the file gives no port.
fn server(
    node: roxmltree::Node<'_, '_>,
    address: &EmailAddress,
    default_ports: [u16; 2],
) -> Option<ServerSettings> {
    let child = |name: &str| {
        node.children()
            .find(|c| c.has_tag_name(name))
            .and_then(|c| c.text())
            .map(str::trim)
    };
    let port = match child("port") {
        Some(text) => Some(text.parse::<u16>().ok().filter(|p| *p != 0)?),
        None => None,
    };
    let security = match child("socketType") {
        Some("SSL") => Security::Tls,
        Some("STARTTLS") => Security::Starttls,
        Some(_) => return None, // `plain` or unknown: never without encryption
        // Not given: only the well-known ports say what is meant.
        None => match port? {
            993 | 465 => Security::Tls,
            143 | 587 => Security::Starttls,
            _ => return None,
        },
    };
    let port = port.unwrap_or(match security {
        Security::Tls => default_ports[0],
        Security::Starttls => default_ports[1],
    });

    let methods: Vec<&str> = node
        .children()
        .filter(|c| c.has_tag_name("authentication"))
        .filter_map(|c| c.text().map(str::trim))
        .collect();
    let password_login = methods.is_empty()
        || methods
            .iter()
            .any(|m| matches!(*m, "password-cleartext" | "password-encrypted"));
    if !password_login {
        return None;
    }

    let host = address
        .expand(child("hostname")?, true)?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !is_host_name(&host) {
        return None;
    }
    let username = address.expand(child("username").unwrap_or("%EMAILADDRESS%"), false)?;
    Some(ServerSettings {
        host,
        port,
        security,
        username,
    })
}

#[cfg(test)]
mod tests;
