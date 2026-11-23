//! The session resource (RFC 8620 section 2): where the API is, which account holds the mail,
//! what the server allows. Fetched at `https://<host>[:<port>]/.well-known/jmap`, following at
//! most three redirects to the same host or one of its subdomains.

use mail_model::{BackendError, Capabilities, ConnectConfig};
use serde_json::Value;
use url::Url;

use crate::capability;
use crate::client::{self, Client};
use crate::error;

/// Redirects followed on the way to the session resource.
const MAX_REDIRECTS: usize = 3;

/// Largest session resource read.
const MAX_SESSION: usize = 1024 * 1024;

/// What the app uses of a session resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Session {
    /// Where the session was found (after redirects).
    pub url: Url,
    pub api_url: Url,
    /// RFC 6570 level 1 templates.
    pub download_url: String,
    pub upload_url: String,
    pub event_source_url: Option<String>,
    /// The primary account for mail (`primaryAccounts["urn:ietf:params:jmap:mail"]`).
    pub account_id: String,
    /// The account may submit mail (`urn:ietf:params:jmap:submission`).
    pub submission: bool,
    /// `SIZE` of the submission extensions, when the server names a limit.
    pub max_message_size: Option<u64>,
    pub max_objects_in_get: usize,
    pub max_objects_in_set: usize,
    pub max_size_upload: Option<u64>,
    /// Capability URIs of the server, sorted.
    pub capabilities: Vec<String>,
}

impl Session {
    /// Parses the resource found at `url`.
    pub(crate) fn parse(url: Url, value: &Value) -> Result<Self, BackendError> {
        let unavailable = |what: &str| BackendError::JmapUnavailable(what.to_owned());
        let caps = value
            .get("capabilities")
            .and_then(Value::as_object)
            .ok_or_else(|| unavailable("the session resource has no capabilities"))?;
        if !caps.contains_key(capability::CORE) {
            return Err(unavailable("the server does not announce JMAP core"));
        }
        if !caps.contains_key(capability::MAIL) {
            return Err(unavailable("the server does not offer JMAP mail"));
        }
        let core = caps.get(capability::CORE);
        let limit = |name: &str, default: u64| {
            core.and_then(|c| c.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(default)
        };
        let account_id = value
            .get("primaryAccounts")
            .and_then(|p| p.get(capability::MAIL))
            .and_then(Value::as_str)
            .ok_or_else(|| unavailable("the user has no mail account on this server"))?
            .to_owned();
        let account = value.get("accounts").and_then(|a| a.get(&account_id));
        let account_caps = account.and_then(|a| a.get("accountCapabilities"));
        let submission_caps = account_caps.and_then(|c| c.get(capability::SUBMISSION));
        let max_message_size = submission_caps
            .and_then(|s| s.get("submissionExtensions"))
            .and_then(|e| e.get("SIZE"))
            .and_then(Value::as_array)
            .and_then(|args| args.first())
            .and_then(|n| {
                n.as_u64()
                    .or_else(|| n.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .filter(|n| *n > 0);
        let text = |name: &str| value.get(name).and_then(Value::as_str);
        let api = text("apiUrl").ok_or_else(|| unavailable("the session names no apiUrl"))?;
        let api_url = url
            .join(api)
            .map_err(|err| BackendError::Protocol(format!("apiUrl: {err}")))?;
        let resolve = |template: &str| -> Result<String, BackendError> {
            // Relative templates resolve against the session; braces survive `join` encoded,
            // so absolute ones are kept as they are.
            if Url::parse(template).is_ok() || template.starts_with("http") {
                Ok(template.to_owned())
            } else {
                let base = url
                    .join("/")
                    .map_err(|err| BackendError::Protocol(format!("session URL: {err}")))?;
                Ok(format!(
                    "{}{}",
                    base.as_str().trim_end_matches('/'),
                    if template.starts_with('/') {
                        template.to_owned()
                    } else {
                        format!("/{template}")
                    }
                ))
            }
        };
        let download_url = resolve(
            text("downloadUrl").ok_or_else(|| unavailable("the session names no downloadUrl"))?,
        )?;
        let upload_url = resolve(
            text("uploadUrl").ok_or_else(|| unavailable("the session names no uploadUrl"))?,
        )?;
        let event_source_url = text("eventSourceUrl").map(resolve).transpose()?;
        let mut capabilities: Vec<String> = caps.keys().cloned().collect();
        capabilities.sort();
        Ok(Self {
            url,
            api_url,
            download_url,
            upload_url,
            event_source_url,
            submission: submission_caps.is_some(),
            max_message_size,
            account_id,
            max_objects_in_get: usize::try_from(limit("maxObjectsInGet", 500).clamp(1, 500))
                .unwrap_or(500),
            max_objects_in_set: usize::try_from(limit("maxObjectsInSet", 500).clamp(1, 500))
                .unwrap_or(500),
            max_size_upload: core
                .and_then(|c| c.get("maxSizeUpload"))
                .and_then(Value::as_u64),
            capabilities,
        })
    }

    /// The capabilities the engine sees: what the adapter offers on top of JMAP.
    pub(crate) fn engine_capabilities(&self) -> Capabilities {
        Capabilities {
            imap4rev1: false,
            idle: self.event_source_url.is_some(),
            condstore: true,
            qresync: false,
            enable: false,
            move_: true,
            uidplus: true,
            special_use: true,
            list_status: true,
            literal_plus: false,
            esearch: false,
            gmail: false,
            jmap: true,
            names: self
                .capabilities
                .iter()
                .map(|c| c.to_ascii_uppercase())
                .collect(),
        }
    }

    /// Expands a URL template (RFC 6570 level 1: `{name}` with percent-encoded values).
    pub(crate) fn expand(template: &str, values: &[(&str, &str)]) -> Result<Url, BackendError> {
        let mut out = template.to_owned();
        for (name, value) in values {
            out = out.replace(&format!("{{{name}}}"), &encode(value));
        }
        Url::parse(&out).map_err(|err| BackendError::Protocol(format!("URL template: {err}")))
    }

    pub(crate) fn download(
        &self,
        blob_id: &str,
        name: &str,
        kind: &str,
    ) -> Result<Url, BackendError> {
        Self::expand(
            &self.download_url,
            &[
                ("accountId", &self.account_id),
                ("blobId", blob_id),
                ("name", name),
                ("type", kind),
            ],
        )
    }

    pub(crate) fn upload(&self) -> Result<Url, BackendError> {
        Self::expand(&self.upload_url, &[("accountId", &self.account_id)])
    }

    /// The event source for Email and Mailbox changes, without automatic close.
    pub(crate) fn events(&self, ping_secs: u64) -> Option<Result<Url, BackendError>> {
        let ping = ping_secs.to_string();
        self.event_source_url.as_deref().map(|template| {
            Self::expand(
                template,
                &[
                    ("types", "Email,Mailbox"),
                    ("closeafter", "no"),
                    ("ping", &ping),
                ],
            )
        })
    }
}

/// Percent-encoding of everything except unreserved characters (RFC 3986), as level 1
/// templates need. `,` stays, because `types` is a list.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b',') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The well-known address of a JMAP server.
pub(crate) fn well_known(
    host: &str,
    port: u16,
    insecure_loopback: bool,
) -> Result<Url, BackendError> {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let scheme = if insecure_loopback { "http" } else { "https" };
    let default_port = if insecure_loopback { 80 } else { 443 };
    let text = if port == default_port {
        format!("{scheme}://{host}/.well-known/jmap")
    } else {
        format!("{scheme}://{host}:{port}/.well-known/jmap")
    };
    Url::parse(&text).map_err(|err| BackendError::InvalidState(format!("JMAP address: {err}")))
}

/// Whether a redirect from `from` to `to` stays with the server: the same host or one of its
/// subdomains (`fastmail.com` → `api.fastmail.com`).
fn same_server(from: &Url, to: &Url) -> bool {
    let (Some(a), Some(b)) = (from.host_str(), to.host_str()) else {
        return false;
    };
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    b == a || b.ends_with(&format!(".{a}"))
}

/// Fetches and parses the session resource of `config`'s server.
pub(crate) async fn fetch(
    client: &Client,
    config: &ConnectConfig,
) -> Result<Session, BackendError> {
    let start = well_known(&config.host, config.port, client.insecure_loopback())?;
    let mut url = start.clone();
    for _ in 0..=MAX_REDIRECTS {
        let response = client.get(&url).await?;
        let status = response.status();
        if status.is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    BackendError::Protocol(format!("redirect ({status}) without location"))
                })?;
            let next = url
                .join(location)
                .map_err(|err| BackendError::Protocol(format!("bad redirect location: {err}")))?;
            client.check_url(&next)?;
            if !same_server(&start, &next) {
                return Err(BackendError::JmapUnavailable(format!(
                    "the server sends the session to another host ({})",
                    next.host_str().unwrap_or_default()
                )));
            }
            url = next;
            continue;
        }
        let body = client::read_ok(response, MAX_SESSION).await?;
        let value: Value = serde_json::from_slice(&body).map_err(|_| {
            BackendError::JmapUnavailable("the address answers, but not with a JMAP session".into())
        })?;
        return Session::parse(url, &value);
    }
    Err(BackendError::JmapUnavailable(format!(
        "more than {MAX_REDIRECTS} redirects"
    )))
}

/// What [`probe`] found without credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionProbe {
    /// Host and port that answered as a JMAP session resource (after redirects).
    pub host: String,
    pub port: u16,
}

/// Account setup (discovery): whether `https://<domain>/.well-known/jmap` leads to a JMAP
/// session resource, without credentials. A JMAP server answers 401 there (or the session for
/// public access); redirects are followed to the same host or its subdomains, at most three.
/// `http` (`insecure_loopback`) only in tests.
pub async fn probe(
    http: &reqwest::Client,
    domain: &str,
    port: u16,
    insecure_loopback: bool,
) -> Result<Option<SessionProbe>, BackendError> {
    let start = well_known(domain, port, insecure_loopback)?;
    let mut url = start.clone();
    for _ in 0..=MAX_REDIRECTS {
        let ok_scheme = url.scheme() == "https"
            || (insecure_loopback && url.scheme() == "http" && client::is_loopback(&url));
        if !ok_scheme {
            return Ok(None);
        }
        let response = http
            .get(url.clone())
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|err| error::from_http(&err))?;
        let status = response.status();
        if status.is_redirection() {
            let Some(next) = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .and_then(|location| url.join(location).ok())
            else {
                return Ok(None);
            };
            if !same_server(&start, &next) {
                return Ok(None);
            }
            url = next;
            continue;
        }
        let found = if status == reqwest::StatusCode::UNAUTHORIZED {
            // A JMAP server asks for credentials here; plain web servers answer 404.
            true
        } else if status.is_success() {
            let body = client::read_limited(response, MAX_SESSION)
                .await
                .unwrap_or_default();
            serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| v.get("capabilities").cloned())
                .is_some_and(|caps| caps.get(capability::CORE).is_some())
        } else {
            false
        };
        if !found {
            return Ok(None);
        }
        let host = url
            .host_str()
            .unwrap_or_default()
            .trim_matches(['[', ']'])
            .to_owned();
        let port = url.port_or_known_default().unwrap_or(443);
        return Ok(Some(SessionProbe { host, port }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Stalwart 1.1.1's session for alice (abridged; recorded from the test server).
    pub(crate) fn stalwart() -> Value {
        json!({
            "capabilities": {
                "urn:ietf:params:jmap:core": {
                    "maxSizeUpload": 50000000, "maxConcurrentUpload": 4,
                    "maxSizeRequest": 10000000, "maxConcurrentRequests": 4,
                    "maxCallsInRequest": 16, "maxObjectsInGet": 500, "maxObjectsInSet": 500,
                    "collationAlgorithms": ["i;ascii-numeric", "i;ascii-casemap", "i;unicode-casemap"]
                },
                "urn:ietf:params:jmap:mail": {},
                "urn:ietf:params:jmap:submission": {},
                "urn:ietf:params:jmap:vacationresponse": {}
            },
            "accounts": {"b": {"name": "alice@vicemail.test", "isPersonal": true, "isReadOnly": false,
                "accountCapabilities": {
                    "urn:ietf:params:jmap:mail": {"maxMailboxesPerEmail": 100},
                    "urn:ietf:params:jmap:submission": {"maxDelayedSend": 2592000,
                        "submissionExtensions": {"FUTURERELEASE": [], "SIZE": [], "DSN": []}}
                }}},
            "primaryAccounts": {"urn:ietf:params:jmap:mail": "b", "urn:ietf:params:jmap:submission": "b"},
            "username": "alice@vicemail.test",
            "apiUrl": "https://localhost:18443/jmap/",
            "downloadUrl": "https://localhost:18443/jmap/download/{accountId}/{blobId}/{name}?accept={type}",
            "uploadUrl": "https://localhost:18443/jmap/upload/{accountId}/",
            "eventSourceUrl": "https://localhost:18443/jmap/eventsource/?types={types}&closeafter={closeafter}&ping={ping}",
            "state": "a7383cb8"
        })
    }

    fn url(text: &str) -> Url {
        Url::parse(text).expect("url")
    }

    #[test]
    fn stalwart_s_session_parses() {
        let session = Session::parse(url("https://localhost:18443/jmap/session"), &stalwart())
            .expect("parse");
        assert_eq!(session.account_id, "b");
        assert_eq!(session.api_url.as_str(), "https://localhost:18443/jmap/");
        assert!(session.submission);
        assert_eq!(session.max_message_size, None, "SIZE without a number");
        assert_eq!(session.max_objects_in_get, 500);
        assert_eq!(
            session
                .download("B 1", "message.eml", "message/rfc822")
                .expect("url")
                .as_str(),
            "https://localhost:18443/jmap/download/b/B%201/message.eml?accept=message%2Frfc822"
        );
        assert_eq!(
            session.upload().expect("url").as_str(),
            "https://localhost:18443/jmap/upload/b/"
        );
        assert_eq!(
            session
                .events(15)
                .expect("event source")
                .expect("url")
                .as_str(),
            "https://localhost:18443/jmap/eventsource/?types=Email,Mailbox&closeafter=no&ping=15"
        );
        let caps = session.engine_capabilities();
        assert!(caps.jmap && caps.idle && caps.condstore && caps.move_ && caps.uidplus);
        assert!(!caps.imap4rev1 && !caps.gmail);
        assert!(caps.has("urn:ietf:params:jmap:mail"));
    }

    #[test]
    fn sessions_without_mail_or_without_event_source() {
        let mut no_mail = stalwart();
        no_mail["capabilities"]
            .as_object_mut()
            .expect("object")
            .remove("urn:ietf:params:jmap:mail");
        let err = Session::parse(url("https://h/s"), &no_mail).expect_err("no mail");
        assert!(matches!(err, BackendError::JmapUnavailable(_)), "{err:?}");

        let mut no_primary = stalwart();
        no_primary["primaryAccounts"] = json!({});
        assert!(Session::parse(url("https://h/s"), &no_primary).is_err());

        let mut quiet = stalwart();
        quiet
            .as_object_mut()
            .expect("object")
            .remove("eventSourceUrl");
        quiet["accounts"]["b"]["accountCapabilities"]["urn:ietf:params:jmap:submission"]["submissionExtensions"]
            ["SIZE"] = json!(["35882577"]);
        let session = Session::parse(url("https://h/s"), &quiet).expect("parse");
        assert!(!session.engine_capabilities().idle, "polling without push");
        assert!(session.events(15).is_none());
        assert_eq!(session.max_message_size, Some(35_882_577));

        assert!(Session::parse(url("https://h/s"), &json!([])).is_err());
    }

    #[test]
    fn well_known_addresses_and_redirect_rule() {
        assert_eq!(
            well_known("jmap.example.org", 443, false)
                .expect("url")
                .as_str(),
            "https://jmap.example.org/.well-known/jmap"
        );
        assert_eq!(
            well_known("::1", 8443, false).expect("url").as_str(),
            "https://[::1]:8443/.well-known/jmap"
        );
        assert!(same_server(
            &url("https://fastmail.com/.well-known/jmap"),
            &url("https://api.fastmail.com/jmap/session")
        ));
        assert!(same_server(
            &url("https://h.example/x"),
            &url("https://H.example:8443/jmap/session")
        ));
        assert!(!same_server(
            &url("https://example.org/x"),
            &url("https://evil-example.org/x")
        ));
        assert!(!same_server(
            &url("https://api.example.org/x"),
            &url("https://example.org/x")
        ));
    }
}
