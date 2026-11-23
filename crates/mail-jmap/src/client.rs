//! One authenticated HTTP client: method calls (RFC 8620 section 3), downloads, uploads and the
//! event source, with the read-only allowlist checked before anything is sent.

use std::time::Duration;

use mail_model::{Access, BackendError, ConnectConfig, Secret};
use serde_json::{Value, json};
use url::Url;

use crate::connector::TlsConfigFn;
use crate::error::{self, MethodError};

/// Neutral client name: the server is the user's own, and the app says nothing about itself.
const USER_AGENT: &str = "Mozilla/5.0";

/// Largest JSON answer read into memory.
const MAX_JSON: usize = 64 * 1024 * 1024;

/// Largest blob (a whole message) read into memory.
pub(crate) const MAX_BLOB: usize = 256 * 1024 * 1024;

/// Methods a read-only connection may call. An allowlist, not a denylist: a method added later
/// is refused until someone classifies it.
const READ_ONLY_METHODS: &[&str] = &[
    "Core/echo",
    "Mailbox/get",
    "Mailbox/changes",
    "Mailbox/query",
    "Mailbox/queryChanges",
    "Email/get",
    "Email/changes",
    "Email/query",
    "Email/queryChanges",
    "Thread/get",
    "Thread/changes",
    "SearchSnippet/get",
    "Identity/get",
];

/// Whether `access` permits calling `method`.
pub(crate) fn permitted(access: Access, method: &str) -> bool {
    access == Access::ReadWrite || READ_ONLY_METHODS.contains(&method)
}

/// Test support: the methods and HTTP requests of a connector's connections, in order (the
/// JMAP counterpart of the IMAP adapter's wire tap). Only set through the `test-support`
/// feature; the product never records.
#[derive(Debug, Clone, Default)]
pub struct MethodLog(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl MethodLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every entry so far: method names (`Email/get`), `upload`, `download`, `eventsource`.
    pub fn entries(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn push(&self, entry: &str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(entry.to_owned());
    }
}

/// One method call: name, arguments, call id.
pub(crate) type Call = (&'static str, Value, &'static str);

/// The answers of one request, by call id.
#[derive(Debug, Default)]
pub(crate) struct Responses {
    items: Vec<(String, Value, String)>,
}

impl Responses {
    pub(crate) fn parse(body: &[u8]) -> Result<Self, BackendError> {
        let value: Value = serde_json::from_slice(body)
            .map_err(|err| BackendError::Protocol(format!("JMAP answer: {err}")))?;
        let list = value
            .get("methodResponses")
            .and_then(Value::as_array)
            .ok_or_else(|| BackendError::Protocol("JMAP answer without methodResponses".into()))?;
        let mut items = Vec::with_capacity(list.len());
        for item in list {
            match item.as_array().map(Vec::as_slice) {
                Some([Value::String(name), args, Value::String(id)]) => {
                    items.push((name.clone(), args.clone(), id.clone()));
                }
                _ => return Err(BackendError::Protocol("malformed method response".into())),
            }
        }
        Ok(Self { items })
    }

    /// The first answer for call `id`: its arguments, or the method error.
    pub(crate) fn raw(&self, id: &str) -> Result<Result<&Value, MethodError>, BackendError> {
        let (name, args, _) = self
            .items
            .iter()
            .find(|(_, _, call)| call == id)
            .ok_or_else(|| BackendError::Protocol(format!("no answer for call {id}")))?;
        if name == "error" {
            return Ok(Err(MethodError::from_args(args)));
        }
        Ok(Ok(args))
    }

    /// The answer for call `id` of `method`; a method error becomes a [`BackendError`].
    pub(crate) fn get(&self, id: &str, method: &str) -> Result<&Value, BackendError> {
        self.raw(id)?.map_err(|err| err.into_backend(method))
    }
}

/// How requests prove who they are. HTTP Basic with the keychain password for now; an OAuth
/// bearer token (decision 0042) is one more variant here.
#[derive(Clone)]
enum Auth {
    Basic {
        username: String,
        password: Secret<String>,
    },
}

/// An HTTP client for one connection.
#[derive(Clone)]
pub(crate) struct Client {
    http: reqwest::Client,
    /// For the event source: no read limit (the server may stay silent for minutes).
    stream: reqwest::Client,
    auth: Auth,
    access: Access,
    /// Plain HTTP to loopback addresses: only in this crate's tests (a scripted server).
    insecure_loopback: bool,
    log: Option<MethodLog>,
}

impl Client {
    pub(crate) fn new(
        config: &ConnectConfig,
        tls: &TlsConfigFn,
        insecure_loopback: bool,
        log: Option<MethodLog>,
    ) -> Result<Self, BackendError> {
        let tls = tls(config.extra_root_ca.as_ref())?;
        let builder = |read: Option<Duration>| {
            let mut builder = reqwest::Client::builder()
                .tls_backend_preconfigured((*tls).clone())
                .http1_only()
                .no_proxy()
                .referer(false)
                .retry(reqwest::retry::never())
                .user_agent(USER_AGENT)
                .connect_timeout(config.timeouts.connect)
                .redirect(reqwest::redirect::Policy::none());
            if let Some(read) = read {
                builder = builder.read_timeout(read);
            }
            builder
                .build()
                .map_err(|err| BackendError::Tls(error::short(&format!("HTTP client: {err}"))))
        };
        let auth = Auth::Basic {
            username: config.username.clone(),
            password: config.password.clone(),
        };
        Ok(Self {
            http: builder(Some(config.timeouts.read))?,
            stream: builder(None)?,
            auth,
            access: config.access,
            insecure_loopback,
            log,
        })
    }

    pub(crate) fn access(&self) -> Access {
        self.access
    }

    pub(crate) fn insecure_loopback(&self) -> bool {
        self.insecure_loopback
    }

    /// Only HTTPS (and plain HTTP to loopback in this crate's tests).
    pub(crate) fn check_url(&self, url: &Url) -> Result<(), BackendError> {
        match url.scheme() {
            "https" => Ok(()),
            "http" if self.insecure_loopback && is_loopback(url) => Ok(()),
            other => Err(BackendError::JmapUnavailable(format!(
                "the server named an address without encryption ({other}://{})",
                url.host_str().unwrap_or_default()
            ))),
        }
    }

    fn record(&self, entry: &str) {
        if let Some(log) = &self.log {
            log.push(entry);
        }
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            Auth::Basic { username, password } => {
                request.basic_auth(username, Some(password.expose_secret()))
            }
        }
    }

    /// GET with credentials; the answer as it is (redirects are not followed).
    pub(crate) async fn get(&self, url: &Url) -> Result<reqwest::Response, BackendError> {
        self.check_url(url)?;
        self.authorize(self.http.get(url.clone()))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|err| error::from_http(&err))
    }

    /// Calls the methods in one request. On a read-only connection every method must be on the
    /// allowlist, or nothing is sent.
    pub(crate) async fn call(
        &self,
        api: &Url,
        using: &[&str],
        calls: Vec<Call>,
    ) -> Result<Responses, BackendError> {
        if let Some((name, _, _)) = calls
            .iter()
            .find(|(name, _, _)| !permitted(self.access, name))
        {
            return Err(BackendError::ReadOnly((*name).to_owned()));
        }
        let names: Vec<&str> = calls.iter().map(|(name, _, _)| *name).collect();
        for name in &names {
            self.record(name);
        }
        let body = json!({
            "using": using,
            "methodCalls": calls
                .into_iter()
                .map(|(name, args, id)| json!([name, args, id]))
                .collect::<Vec<_>>(),
        });
        let body = serde_json::to_vec(&body)
            .map_err(|err| BackendError::InvalidState(format!("JMAP request: {err}")))?;
        self.check_url(api)?;
        tracing::trace!(methods = ?names, bytes = body.len(), "JMAP request");
        let response = self
            .authorize(self.http.post(api.clone()))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|err| error::from_http(&err))?;
        let body = read_ok(response, MAX_JSON).await?;
        Responses::parse(&body)
    }

    /// Downloads a blob (at most [`MAX_BLOB`] bytes). 404 means the blob is gone (`None`).
    pub(crate) async fn download(&self, url: &Url) -> Result<Option<Vec<u8>>, BackendError> {
        self.check_url(url)?;
        self.record("download");
        let response = self
            .authorize(self.http.get(url.clone()))
            .send()
            .await
            .map_err(|err| error::from_http(&err))?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        read_ok(response, MAX_BLOB).await.map(Some)
    }

    /// Uploads `data` (read-write connections only). Returns the blob id.
    pub(crate) async fn upload(
        &self,
        url: &Url,
        content_type: &str,
        data: &[u8],
    ) -> Result<String, BackendError> {
        if self.access != Access::ReadWrite {
            return Err(BackendError::ReadOnly("upload".into()));
        }
        self.check_url(url)?;
        self.record("upload");
        let response = self
            .authorize(self.http.post(url.clone()))
            .header("Content-Type", content_type)
            .header("Accept", "application/json")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|err| error::from_http(&err))?;
        let body = read_ok(response, MAX_JSON).await?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|err| BackendError::Protocol(format!("upload answer: {err}")))?;
        value
            .get("blobId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| BackendError::Protocol("upload answer without blobId".into()))
    }

    /// Opens the event source (`text/event-stream`); the caller reads it chunk by chunk.
    pub(crate) async fn event_stream(&self, url: &Url) -> Result<reqwest::Response, BackendError> {
        self.check_url(url)?;
        self.record("eventsource");
        let response = self
            .authorize(self.stream.get(url.clone()))
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .send()
            .await
            .map_err(|err| error::from_http(&err))?;
        let status = response.status();
        if !status.is_success() {
            let body = read_limited(response, 64 * 1024).await.unwrap_or_default();
            return Err(error::from_status(status.as_u16(), &body));
        }
        Ok(response)
    }
}

/// The body of a successful answer, or the error its status means.
pub(crate) async fn read_ok(
    response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, BackendError> {
    let status = response.status();
    if !status.is_success() {
        let body = read_limited(response, 64 * 1024).await.unwrap_or_default();
        return Err(error::from_status(status.as_u16(), &body));
    }
    read_limited(response, limit).await
}

/// Reads at most `limit` bytes; more is a protocol error.
pub(crate) async fn read_limited(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, BackendError> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| error::from_http(&err))?
    {
        if body.len() + chunk.len() > limit {
            return Err(BackendError::Protocol(format!(
                "answer larger than {limit} bytes"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(crate) fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain(name)) => name.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_only_reads() {
        for method in ["Email/get", "Email/changes", "Mailbox/get", "Identity/get"] {
            assert!(permitted(Access::ReadOnly, method), "{method}");
        }
        for method in [
            "Email/set",
            "Email/import",
            "Email/copy",
            "Mailbox/set",
            "EmailSubmission/set",
            "Identity/set",
            "Blob/upload",
            "VacationResponse/set",
            "Email/Get",
        ] {
            assert!(!permitted(Access::ReadOnly, method), "{method}");
            assert!(permitted(Access::ReadWrite, method), "{method}");
        }
    }

    #[test]
    fn responses_by_call_id() {
        let body = br#"{"methodResponses":[["Email/get",{"list":[]},"a"],["error",{"type":"cannotCalculateChanges"},"b"],["Email/set",{"destroyed":["x"]},"a"]],"sessionState":"s"}"#;
        let responses = Responses::parse(body).expect("parse");
        assert!(responses.get("a", "Email/get").is_ok());
        let err = responses.raw("b").expect("answer").expect_err("error");
        assert_eq!(err.kind, "cannotCalculateChanges");
        assert!(responses.get("c", "x").is_err());
        assert!(Responses::parse(b"{}").is_err());
        assert!(Responses::parse(b"{\"methodResponses\":[[1,2]]}").is_err());
    }
}
