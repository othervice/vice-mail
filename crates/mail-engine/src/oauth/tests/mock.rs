//! A mock authorization server (authorize, token, revoke over plain HTTP on 127.0.0.1) and a
//! scripted browser for the sign-in tests.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use mail_model::{CoreError, Secret};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use url::Url;

use crate::oauth::pkce::base64url;
use crate::reading::SystemOpener;

/// What the mock saw and how it behaves.
#[derive(Debug, Default)]
pub(crate) struct MockState {
    /// Query parameters of every authorize request.
    pub authorize: Vec<HashMap<String, String>>,
    /// Form fields of every token request.
    pub token: Vec<HashMap<String, String>>,
    /// Tokens sent to the revocation endpoint.
    pub revoked: Vec<String>,
    /// Code exchanges whose verifier matched the challenge.
    pub verified_pkce: usize,
    /// Refuse the authorization (the user clicked "No").
    pub deny: bool,
    /// Refuse refreshes with this error.
    pub refresh_error: Option<&'static str>,
    /// Hand out a new refresh token with every refresh.
    pub rotate: bool,
    /// Lifetime of access tokens in seconds.
    pub expires_in: u64,
    /// Hand out this access token every time (the fake IMAP server's "password").
    pub fixed_access: Option<String>,
    codes: HashMap<String, (String, String)>,
    refresh_tokens: HashSet<String>,
    issued: u64,
}

impl MockState {
    fn next(&mut self, kind: &str) -> String {
        self.issued += 1;
        format!("{kind}-{}", self.issued)
    }
}

pub(crate) struct MockAuth {
    addr: SocketAddr,
    state: Arc<Mutex<MockState>>,
    task: JoinHandle<()>,
}

impl Drop for MockAuth {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockAuth {
    pub(crate) async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(Mutex::new(MockState {
            expires_in: 3600,
            ..MockState::default()
        }));
        let shared = Arc::clone(&state);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = Arc::clone(&shared);
                tokio::spawn(async move { serve(stream, state).await });
            }
        });
        Self { addr, state, task }
    }

    pub(crate) fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://{}{path}", self.addr)).expect("url")
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, MockState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn state_mut(&self, f: impl FnOnce(&mut MockState)) {
        f(&mut self.state());
    }

    /// Number of refresh requests so far.
    pub(crate) fn refreshes(&self) -> usize {
        self.state()
            .token
            .iter()
            .filter(|f| f.get("grant_type").map(String::as_str) == Some("refresh_token"))
            .count()
    }

    /// A refresh token the mock accepts, as if from an earlier sign-in.
    pub(crate) fn issue_refresh_token(&self) -> Secret<String> {
        let mut state = self.state();
        let token = state.next("refresh");
        state.refresh_tokens.insert(token.clone());
        Secret::new(token)
    }

    /// Stops answering (the endpoint is unreachable from now on).
    pub(crate) fn stop(&self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, state: Arc<Mutex<MockState>>) {
    let Some((head, body)) = read_request(&mut stream).await else {
        return;
    };
    let line = head.lines().next().unwrap_or_default().to_owned();
    let mut parts = line.split(' ');
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let params = |text: &str| -> HashMap<String, String> {
        url::form_urlencoded::parse(text.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    };
    let response = {
        let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
        match (method, path) {
            ("GET", "/authorize") => authorize(&mut state, params(query)),
            ("POST", "/token") => token(&mut state, params(&body)),
            ("POST", "/revoke") => {
                if let Some(token) = params(&body).get("token") {
                    state.revoked.push(token.clone());
                    state.refresh_tokens.remove(token);
                }
                reply(200, "application/json", "{}")
            }
            _ => reply(404, "text/plain", "not found"),
        }
    };
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn authorize(state: &mut MockState, params: HashMap<String, String>) -> String {
    state.authorize.push(params.clone());
    let redirect = params.get("redirect_uri").cloned().unwrap_or_default();
    let mut target = match Url::parse(&redirect) {
        Ok(url) => url,
        Err(_) => return reply(400, "text/plain", "bad redirect_uri"),
    };
    {
        let mut query = target.query_pairs_mut();
        if state.deny {
            query
                .append_pair("error", "access_denied")
                .append_pair("error_description", "The user declined.");
        } else {
            let code = state.next("code");
            state.codes.insert(
                code.clone(),
                (
                    params.get("code_challenge").cloned().unwrap_or_default(),
                    redirect.clone(),
                ),
            );
            query.append_pair("code", &code);
        }
        if let Some(st) = params.get("state") {
            query.append_pair("state", st);
        }
    }
    format!(
        "HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )
}

fn token(state: &mut MockState, form: HashMap<String, String>) -> String {
    state.token.push(form.clone());
    let error = |code: &str| {
        reply(
            400,
            "application/json",
            &format!(r#"{{"error":"{code}","error_description":"mock refused"}}"#),
        )
    };
    let get = |key: &str| form.get(key).cloned().unwrap_or_default();
    let (refresh, new_refresh) = match get("grant_type").as_str() {
        "authorization_code" => {
            let Some((challenge, redirect)) = state.codes.remove(&get("code")) else {
                return error("invalid_grant");
            };
            if redirect != get("redirect_uri") {
                return error("invalid_grant");
            }
            if base64url(&Sha256::digest(get("code_verifier").as_bytes())) != challenge {
                return error("invalid_grant");
            }
            state.verified_pkce += 1;
            (None, true)
        }
        "refresh_token" => {
            if let Some(code) = state.refresh_error {
                return error(code);
            }
            if !state.refresh_tokens.contains(&get("refresh_token")) {
                return error("invalid_grant");
            }
            (Some(get("refresh_token")), state.rotate)
        }
        _ => return error("unsupported_grant_type"),
    };
    let access = match &state.fixed_access {
        Some(fixed) => fixed.clone(),
        None => state.next("access"),
    };
    let refresh_field = if new_refresh {
        let token = state.next("refresh");
        state.refresh_tokens.insert(token.clone());
        if let Some(old) = refresh {
            state.refresh_tokens.remove(&old);
        }
        format!(r#","refresh_token":"{token}""#)
    } else {
        String::new()
    };
    reply(
        200,
        "application/json",
        &format!(
            r#"{{"access_token":"{access}","token_type":"Bearer","expires_in":{}{refresh_field}}}"#,
            state.expires_in
        ),
    )
}

fn reply(status: u16, kind: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|l| {
            let (name, value) = l.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    while buffer.len() < head_end + length {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buffer[head_end..]).into_owned();
    Some((head, body))
}

/// What the scripted browser does with the provider's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Script {
    /// Follows the redirect back to the app.
    Normal,
    /// The user refuses (the mock redirects with `access_denied`).
    Deny,
    /// Another program sends a forged answer (other `state`, other code) first.
    ForgedStateFirst,
    /// Only a forged answer arrives.
    OnlyForgedState,
    /// The right answer on another path.
    OnlyWrongPath,
    /// The right answer to another port.
    WrongPortOnly,
    /// The user closes the browser.
    Never,
}

/// A "browser" in place of the system's: drives the provider's page over HTTP in the
/// background. Records the address it was asked to open and the app pages it got back.
pub(crate) struct Browser {
    script: Script,
    opened: Mutex<Vec<String>>,
    pages: Arc<Mutex<Vec<(u16, String)>>>,
}

impl Browser {
    pub(crate) fn new(script: Script) -> Arc<Self> {
        Arc::new(Self {
            script,
            opened: Mutex::default(),
            pages: Arc::default(),
        })
    }

    pub(crate) fn opened(&self) -> Vec<String> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Status and body of every page the app's loopback listener answered with.
    pub(crate) fn pages(&self) -> Vec<(u16, String)> {
        self.pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SystemOpener for Browser {
    fn open_url(&self, url: &str) -> Result<(), CoreError> {
        self.opened
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(url.to_owned());
        let script = self.script;
        let pages = Arc::clone(&self.pages);
        let url = url.to_owned();
        tokio::runtime::Handle::current().spawn(async move { drive(script, url, pages).await });
        Ok(())
    }

    fn open_path(&self, _path: &std::path::Path) -> Result<(), CoreError> {
        Err(CoreError::Internal("not a file browser".into()))
    }
}

async fn drive(script: Script, url: String, pages: Arc<Mutex<Vec<(u16, String)>>>) {
    if script == Script::Never {
        return;
    }
    let tls = mail_imap::tls::client_config(None).expect("tls");
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(Arc::unwrap_or_clone(tls))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .expect("client");
    let Ok(response) = client.get(&url).send().await else {
        return;
    };
    let Some(location) = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Url::parse(v).ok())
    else {
        return;
    };
    let forged = {
        let mut forged = location.clone();
        forged
            .query_pairs_mut()
            .clear()
            .append_pair("code", "forged-code")
            .append_pair("state", "forged-state");
        forged
    };
    let mut visit = Vec::new();
    match script {
        Script::Normal | Script::Deny => visit.push(location),
        Script::ForgedStateFirst => {
            visit.push(forged);
            visit.push(location);
        }
        Script::OnlyForgedState => visit.push(forged),
        Script::OnlyWrongPath => {
            let mut other = location.clone();
            other.set_path("/callback");
            visit.push(other);
        }
        Script::WrongPortOnly => {
            let mut other = location.clone();
            let port = location.port().unwrap_or(80);
            let _ = other.set_port(Some(if port == 65535 { 1 } else { port + 1 }));
            visit.push(other);
        }
        Script::Never => {}
    }
    for target in visit {
        if let Ok(response) = client.get(target).send().await {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            pages
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((status, body));
        }
    }
}
