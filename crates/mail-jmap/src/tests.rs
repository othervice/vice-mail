//! The adapter against a scripted JMAP server in this process (plain HTTP on loopback, which
//! only these tests may use): sessions, the first and later syncs, the UID map across
//! connections, writes, push, sending, read-only protection and error mapping. The parsing of
//! recorded Stalwart answers is tested next to the parsers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mail_model::{
    Access, AccountId, BackendError, ConnectConfig, Envelope, Flag, IdleOutcome, MailBackend,
    MailConnector, MailSender, MailWriter, Secret, Security, SendOutcome, ServerSettings,
    StoreMode, UidMapChanges, UidMapFuture, UidMapSnapshot, UidMapStore, UidSet, WriteConnector,
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

use crate::{JmapConnector, JmapSender, TlsConfigFn};

const ACCOUNT: &str = "u1";
const USER: &str = "alice@example.test";
const PASSWORD: &str = "alice-pw";

#[derive(Debug, Clone)]
struct FakeEmail {
    mailboxes: BTreeSet<String>,
    keywords: BTreeSet<String>,
    received: String,
    raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    Created,
    Updated,
    Destroyed,
}

#[derive(Default)]
struct State {
    state: u64,
    log: Vec<(u64, String, Change)>,
    mailboxes: Vec<(String, String, Option<String>, Option<String>)>,
    emails: BTreeMap<String, FakeEmail>,
    next_id: u64,
    blobs: HashMap<String, Vec<u8>>,
    /// Method names and HTTP requests, in order.
    requests: Vec<String>,
    cannot_calculate: bool,
    reject_submission: Option<Value>,
    submissions: Vec<Value>,
    push: bool,
}

/// A small JMAP server: enough of RFC 8620/8621 for the adapter.
#[derive(Clone)]
struct Fake {
    state: Arc<Mutex<State>>,
    events: broadcast::Sender<String>,
    port: u16,
}

fn header_block(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or_default();
    let mut out: Vec<(String, String)> = Vec::new();
    for line in head.split("\r\n") {
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.1.push_str("\r\n");
                last.1.push_str(line);
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.to_owned(), value.to_owned()));
        }
    }
    out
}

fn header_value(raw: &[u8], name: &str) -> Option<String> {
    header_block(raw)
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.trim().to_owned())
}

impl Fake {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let (events, _) = broadcast::channel(64);
        let fake = Self {
            state: Arc::new(Mutex::new(State {
                push: true,
                ..State::default()
            })),
            events,
            port,
        };
        {
            let mut s = fake.lock();
            for (id, name, role) in [
                ("mi", "Inbox", Some("inbox")),
                ("ma", "Archive", Some("archive")),
                ("ms", "Sent", Some("sent")),
                ("md", "Drafts", Some("drafts")),
                ("mt", "Trash", Some("trash")),
            ] {
                s.mailboxes
                    .push((id.into(), name.into(), None, role.map(str::to_owned)));
            }
        }
        let server = fake.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let server = server.clone();
                tokio::spawn(async move { server.serve(stream).await });
            }
        });
        fake
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn methods(&self) -> Vec<String> {
        self.lock().requests.clone()
    }

    fn bump(s: &mut State, id: &str, change: Change) {
        s.state += 1;
        let state = s.state;
        s.log.push((state, id.to_owned(), change));
    }

    fn notify(&self) {
        let state = format!("s{}", self.lock().state);
        let _ = self.events.send(state);
    }

    /// Adds a message as another client would; returns its id.
    fn add(&self, mailbox: &str, n: u32, keywords: &[&str]) -> String {
        let id = {
            let mut s = self.lock();
            s.next_id += 1;
            let id = format!("e{}", s.next_id);
            let raw = format!(
                "From: Sender {n} <s{n}@example.test>\r\nTo: {USER}\r\nSubject: Message {n}\r\n\
                 Date: Mon, 02 Nov 2026 10:{:02}:00 +0000\r\nMessage-ID: <m{n}@example.test>\r\n\
                 List-Id: <list.example.test>\r\n\r\nBody {n}.\r\n",
                n % 60
            )
            .into_bytes();
            let email = FakeEmail {
                mailboxes: BTreeSet::from([mailbox.to_owned()]),
                keywords: keywords.iter().map(|k| (*k).to_owned()).collect(),
                received: format!("2026-11-02T10:{:02}:{:02}Z", n / 60 % 60, n % 60),
                raw,
            };
            s.emails.insert(id.clone(), email);
            Self::bump(&mut s, &id, Change::Created);
            id
        };
        self.notify();
        id
    }

    fn set_keyword(&self, id: &str, keyword: &str, on: bool) {
        {
            let mut s = self.lock();
            if let Some(email) = s.emails.get_mut(id) {
                if on {
                    email.keywords.insert(keyword.to_owned());
                } else {
                    email.keywords.remove(keyword);
                }
            }
            Self::bump(&mut s, id, Change::Updated);
        }
        self.notify();
    }

    fn move_to(&self, id: &str, mailbox: &str) {
        {
            let mut s = self.lock();
            if let Some(email) = s.emails.get_mut(id) {
                email.mailboxes = BTreeSet::from([mailbox.to_owned()]);
            }
            Self::bump(&mut s, id, Change::Updated);
        }
        self.notify();
    }

    fn email(&self, id: &str) -> Option<FakeEmail> {
        self.lock().emails.get(id).cloned()
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn session(&self) -> Value {
        let base = self.base();
        let mut session = json!({
            "capabilities": {
                "urn:ietf:params:jmap:core": {"maxObjectsInGet": 2, "maxObjectsInSet": 2},
                "urn:ietf:params:jmap:mail": {},
                "urn:ietf:params:jmap:submission": {}
            },
            "accounts": {ACCOUNT: {"name": USER, "accountCapabilities": {
                "urn:ietf:params:jmap:mail": {}, "urn:ietf:params:jmap:submission": {}}}},
            "primaryAccounts": {"urn:ietf:params:jmap:mail": ACCOUNT},
            "username": USER,
            "apiUrl": format!("{base}/api/"),
            "downloadUrl": format!("{base}/download/{{accountId}}/{{blobId}}/{{name}}?type={{type}}"),
            "uploadUrl": format!("{base}/upload/{{accountId}}/"),
            "eventSourceUrl": format!("{base}/events?types={{types}}&closeafter={{closeafter}}&ping={{ping}}"),
            "state": "x"
        });
        if !self.lock().push
            && let Some(map) = session.as_object_mut()
        {
            map.remove("eventSourceUrl");
        }
        session
    }

    async fn serve(&self, mut stream: TcpStream) {
        let mut buffer = Vec::new();
        loop {
            let head_end = loop {
                if let Some(i) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                    break i;
                }
                let mut chunk = [0u8; 4096];
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                }
            };
            let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or_default().to_owned();
            let mut length = 0usize;
            let mut authorized = false;
            let expected = format!(
                "basic {}",
                base64_encode(format!("{USER}:{PASSWORD}").as_bytes())
            );
            for line in lines {
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    if name.eq_ignore_ascii_case("authorization")
                        && value.trim().eq_ignore_ascii_case(&expected)
                    {
                        authorized = true;
                    }
                }
            }
            while buffer.len() < head_end + 4 + length {
                let mut chunk = [0u8; 4096];
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buffer.extend_from_slice(&chunk[..n]),
                }
            }
            let body: Vec<u8> = buffer[head_end + 4..head_end + 4 + length].to_vec();
            buffer.drain(..head_end + 4 + length);
            let mut parts = request_line.split(' ');
            let method = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            self.lock().requests.push(format!(
                "{method} {}",
                path.split('?').next().unwrap_or_default()
            ));
            if !authorized {
                write(&mut stream, 401, "text/plain", b"no").await;
                continue;
            }
            if path == "/.well-known/jmap" {
                let location = "/jmap/session";
                let response = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                );
                let _ = stream.write_all(response.as_bytes()).await;
            } else if path == "/jmap/session" {
                let body = serde_json::to_vec(&self.session()).expect("json");
                write(&mut stream, 200, "application/json", &body).await;
            } else if path == "/api/" {
                let request: Value = serde_json::from_slice(&body).expect("request json");
                let answer = self.api(&request);
                let body = serde_json::to_vec(&answer).expect("json");
                write(&mut stream, 200, "application/json", &body).await;
            } else if let Some(rest) = path.strip_prefix("/download/") {
                let blob = rest.split('/').nth(1).unwrap_or_default().to_owned();
                let data = self.blob(&blob);
                match data {
                    Some(data) => write(&mut stream, 200, "application/octet-stream", &data).await,
                    None => write(&mut stream, 404, "text/plain", b"gone").await,
                }
            } else if path.starts_with("/upload/") {
                let id = {
                    let mut s = self.lock();
                    s.next_id += 1;
                    let id = format!("up{}", s.next_id);
                    s.blobs.insert(id.clone(), body);
                    id
                };
                let answer = json!({"accountId": ACCOUNT, "blobId": id, "type": "message/rfc822"});
                write(
                    &mut stream,
                    200,
                    "application/json",
                    answer.to_string().as_bytes(),
                )
                .await;
            } else if path.starts_with("/events") {
                let mut events = self.events.subscribe();
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
                if stream.write_all(head.as_bytes()).await.is_err() {
                    return;
                }
                let _ = stream.write_all(b": hello\n\n").await;
                while let Ok(state) = events.recv().await {
                    let event = format!(
                        "event: state\ndata: {{\"@type\":\"StateChange\",\"changed\":{{\"{ACCOUNT}\":{{\"Email\":\"{state}\"}}}}}}\n\n"
                    );
                    if stream.write_all(event.as_bytes()).await.is_err() {
                        return;
                    }
                }
                return;
            } else {
                write(&mut stream, 404, "text/plain", b"not here").await;
            }
        }
    }

    fn blob(&self, blob: &str) -> Option<Vec<u8>> {
        let s = self.lock();
        if let Some(data) = s.blobs.get(blob) {
            return Some(data.clone());
        }
        let id = blob.strip_prefix("blob-")?;
        let (id, part) = match id.split_once('.') {
            Some((id, part)) => (id, Some(part)),
            None => (id, None),
        };
        let raw = &s.emails.get(id)?.raw;
        match part {
            None => Some(raw.clone()),
            Some(_) => {
                let text = String::from_utf8_lossy(raw);
                Some(
                    text.split("\r\n\r\n")
                        .nth(1)
                        .unwrap_or_default()
                        .as_bytes()
                        .to_vec(),
                )
            }
        }
    }

    fn api(&self, request: &Value) -> Value {
        let mut responses = Vec::new();
        let mut created_ids: HashMap<String, String> = HashMap::new();
        for call in request["methodCalls"].as_array().expect("calls") {
            let name = call[0].as_str().expect("name").to_owned();
            let args = &call[1];
            let id = call[2].as_str().expect("id").to_owned();
            self.lock().requests.push(name.clone());
            let mut extra = Vec::new();
            let answer = self.method(&name, args, &mut created_ids, &mut extra);
            responses.push(match answer {
                Ok(args) => json!([name, args, id]),
                Err(error) => json!(["error", error, id]),
            });
            for (name, args) in extra {
                responses.push(json!([name, args, id]));
            }
        }
        json!({"methodResponses": responses, "sessionState": "x"})
    }

    #[allow(clippy::too_many_lines)]
    fn method(
        &self,
        name: &str,
        args: &Value,
        created_ids: &mut HashMap<String, String>,
        extra: &mut Vec<(String, Value)>,
    ) -> Result<Value, Value> {
        let state_now = |s: &State| format!("s{}", s.state);
        match name {
            "Core/echo" => Ok(args.clone()),
            "Mailbox/get" => {
                let s = self.lock();
                let list: Vec<Value> = s
                    .mailboxes
                    .iter()
                    .map(|(id, name, parent, role)| {
                        let total = s
                            .emails
                            .values()
                            .filter(|e| e.mailboxes.contains(id))
                            .count();
                        json!({"id": id, "name": name, "parentId": parent, "role": role,
                               "totalEmails": total, "unreadEmails": 0})
                    })
                    .collect();
                Ok(
                    json!({"accountId": ACCOUNT, "state": state_now(&s), "list": list, "notFound": []}),
                )
            }
            "Mailbox/set" => {
                let mut s = self.lock();
                let mut created = serde_json::Map::new();
                for (key, mailbox) in args["create"].as_object().into_iter().flatten() {
                    s.next_id += 1;
                    let id = format!("mx{}", s.next_id);
                    s.mailboxes.push((
                        id.clone(),
                        mailbox["name"].as_str().unwrap_or_default().to_owned(),
                        mailbox["parentId"].as_str().map(str::to_owned),
                        mailbox["role"].as_str().map(str::to_owned),
                    ));
                    created.insert(key.clone(), json!({"id": id}));
                }
                Ok(json!({"accountId": ACCOUNT, "created": created}))
            }
            "Email/get" => {
                let s = self.lock();
                let ids: Vec<String> = args["ids"]
                    .as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if ids.len() > 2 {
                    return Err(json!({"type": "requestTooLarge"}));
                }
                let mut list = Vec::new();
                let mut not_found = Vec::new();
                for id in ids {
                    let Some(email) = s.emails.get(&id) else {
                        not_found.push(id);
                        continue;
                    };
                    let headers: Vec<Value> = header_block(&email.raw)
                        .into_iter()
                        .map(|(n, v)| json!({"name": n, "value": v}))
                        .collect();
                    let from = header_value(&email.raw, "From").unwrap_or_default();
                    let (from_name, from_email) = from
                        .split_once(" <")
                        .map(|(n, e)| (n.to_owned(), e.trim_end_matches('>').to_owned()))
                        .unwrap_or((String::new(), from.clone()));
                    let keywords: serde_json::Map<String, Value> = email
                        .keywords
                        .iter()
                        .map(|k| (k.clone(), json!(true)))
                        .collect();
                    let mailbox_ids: serde_json::Map<String, Value> = email
                        .mailboxes
                        .iter()
                        .map(|m| (m.clone(), json!(true)))
                        .collect();
                    list.push(json!({
                        "id": id,
                        "blobId": format!("blob-{id}"),
                        "size": email.raw.len(),
                        "receivedAt": email.received,
                        "sentAt": "2026-11-02T10:00:00+00:00",
                        "messageId": [header_value(&email.raw, "Message-ID").unwrap_or_default()
                            .trim_matches(['<', '>']).to_owned()],
                        "subject": header_value(&email.raw, "Subject"),
                        "from": [{"name": from_name, "email": from_email}],
                        "keywords": keywords,
                        "mailboxIds": mailbox_ids,
                        "hasAttachment": false,
                        "headers": headers,
                        "bodyStructure": {"partId": "1", "blobId": format!("blob-{id}.1"),
                            "type": "text/plain", "size": 8,
                            "headers": [{"name": "Content-Type", "value": " text/plain"}]},
                    }));
                }
                Ok(
                    json!({"accountId": ACCOUNT, "state": state_now(&s), "list": list, "notFound": not_found}),
                )
            }
            "Email/query" => {
                let s = self.lock();
                let filter = &args["filter"];
                let conditions: Vec<&Value> = match filter["conditions"].as_array() {
                    Some(list) => list.iter().collect(),
                    None => vec![filter],
                };
                let mut hits: Vec<(&String, &FakeEmail)> = s
                    .emails
                    .iter()
                    .filter(|(_, e)| {
                        conditions.iter().all(|c| {
                            if let Some(m) = c["inMailbox"].as_str() {
                                return e.mailboxes.contains(m);
                            }
                            if let Some(h) = c["header"].as_array() {
                                let name = h[0].as_str().unwrap_or_default();
                                let want = h[1].as_str().unwrap_or_default();
                                return header_value(&e.raw, name)
                                    .is_some_and(|v| v.contains(want));
                            }
                            if let Some(text) = c["subject"].as_str() {
                                return header_value(&e.raw, "Subject")
                                    .is_some_and(|v| v.contains(text));
                            }
                            true
                        })
                    })
                    .collect();
                let descending = args["sort"][0]["isAscending"] == json!(false);
                hits.sort_by(|a, b| a.1.received.cmp(&b.1.received).then(a.0.cmp(b.0)));
                if descending {
                    hits.reverse();
                }
                let total = hits.len();
                let position = usize::try_from(args["position"].as_u64().unwrap_or(0)).unwrap_or(0);
                // Pages of three, to exercise paging.
                let ids: Vec<&String> = hits
                    .iter()
                    .skip(position)
                    .take(3)
                    .map(|(id, _)| *id)
                    .collect();
                Ok(
                    json!({"accountId": ACCOUNT, "ids": ids, "position": position, "total": total,
                          "queryState": state_now(&s)}),
                )
            }
            "Email/changes" => {
                let s = self.lock();
                if s.cannot_calculate {
                    return Err(json!({"type": "cannotCalculateChanges"}));
                }
                let since: u64 = args["sinceState"]
                    .as_str()
                    .and_then(|t| t.strip_prefix('s'))
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                let mut per_id: BTreeMap<&String, (Change, Change)> = BTreeMap::new();
                for (state, id, change) in &s.log {
                    if *state > since {
                        per_id
                            .entry(id)
                            .and_modify(|e| e.1 = *change)
                            .or_insert((*change, *change));
                    }
                }
                let (mut created, mut updated, mut destroyed) = (vec![], vec![], vec![]);
                for (id, (first, last)) in per_id {
                    match (first, last) {
                        (Change::Created, Change::Destroyed) => {}
                        (_, Change::Destroyed) => destroyed.push(id.clone()),
                        (Change::Created, _) => created.push(id.clone()),
                        _ => updated.push(id.clone()),
                    }
                }
                Ok(
                    json!({"accountId": ACCOUNT, "oldState": args["sinceState"], "newState": state_now(&s),
                          "hasMoreChanges": false, "created": created, "updated": updated,
                          "destroyed": destroyed}),
                )
            }
            "Email/set" => {
                let mut s = self.lock();
                let old = state_now(&s);
                let mut updated = serde_json::Map::new();
                let mut not_updated = serde_json::Map::new();
                for (id, patch) in args["update"].as_object().into_iter().flatten() {
                    let Some(email) = s.emails.get_mut(id) else {
                        not_updated.insert(id.clone(), json!({"type": "notFound"}));
                        continue;
                    };
                    for (path, value) in patch.as_object().into_iter().flatten() {
                        if let Some(k) = path.strip_prefix("keywords/") {
                            if value.is_null() {
                                email.keywords.remove(k);
                            } else {
                                email.keywords.insert(k.to_owned());
                            }
                        } else if let Some(m) = path.strip_prefix("mailboxIds/") {
                            if value.is_null() {
                                email.mailboxes.remove(m);
                            } else {
                                email.mailboxes.insert(m.to_owned());
                            }
                        }
                    }
                    updated.insert(id.clone(), Value::Null);
                    Self::bump(&mut s, id, Change::Updated);
                }
                let mut destroyed = Vec::new();
                for id in args["destroy"].as_array().into_iter().flatten() {
                    let id = id.as_str().unwrap_or_default().to_owned();
                    let id = created_ids
                        .get(id.trim_start_matches('#'))
                        .cloned()
                        .unwrap_or(id);
                    if s.emails.remove(&id).is_some() {
                        Self::bump(&mut s, &id, Change::Destroyed);
                        destroyed.push(id);
                    }
                }
                Ok(
                    json!({"accountId": ACCOUNT, "oldState": old, "newState": state_now(&s),
                          "updated": updated, "notUpdated": not_updated, "destroyed": destroyed}),
                )
            }
            "Email/import" => {
                let mut s = self.lock();
                let mut created = serde_json::Map::new();
                for (key, item) in args["emails"].as_object().into_iter().flatten() {
                    let blob = item["blobId"].as_str().unwrap_or_default();
                    let Some(raw) = s.blobs.get(blob).cloned() else {
                        return Err(json!({"type": "invalidArguments"}));
                    };
                    s.next_id += 1;
                    let id = format!("e{}", s.next_id);
                    let email = FakeEmail {
                        mailboxes: item["mailboxIds"]
                            .as_object()
                            .map(|m| m.keys().cloned().collect())
                            .unwrap_or_default(),
                        keywords: item["keywords"]
                            .as_object()
                            .map(|m| m.keys().cloned().collect())
                            .unwrap_or_default(),
                        received: "2026-11-03T00:00:00Z".into(),
                        raw,
                    };
                    s.emails.insert(id.clone(), email);
                    Self::bump(&mut s, &id, Change::Created);
                    created_ids.insert(key.clone(), id.clone());
                    created.insert(key.clone(), json!({"id": id, "blobId": blob}));
                }
                Ok(json!({"accountId": ACCOUNT, "created": created}))
            }
            "Identity/get" => Ok(json!({"accountId": ACCOUNT, "list": [
                {"id": "i1", "email": USER, "name": "Alice"}]})),
            "EmailSubmission/set" => {
                let reject = self.lock().reject_submission.clone();
                let create = &args["create"];
                let (key, submission) = create
                    .as_object()
                    .and_then(|m| m.iter().next())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .expect("submission");
                if let Some(error) = reject {
                    return Ok(json!({"accountId": ACCOUNT, "notCreated": {key: error}}));
                }
                self.lock().submissions.push(submission.clone());
                if args["onSuccessDestroyEmail"]
                    .as_array()
                    .is_some_and(|l| !l.is_empty())
                {
                    let email = submission["emailId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    let mut s = self.lock();
                    if s.emails.remove(&email).is_some() {
                        Self::bump(&mut s, &email, Change::Destroyed);
                    }
                    extra.push(("Email/set".into(), json!({"destroyed": [email]})));
                }
                Ok(json!({"accountId": ACCOUNT, "created": {key: {"id": "sub1"}}}))
            }
            other => Err(json!({"type": "unknownMethod", "description": other})),
        }
    }
}

async fn write(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Every write-through, kept in memory: the store a restart reads.
#[derive(Default)]
struct MemoryStore {
    maps: Mutex<HashMap<AccountId, UidMapSnapshot>>,
    saves: Mutex<u32>,
}

impl UidMapStore for MemoryStore {
    fn load_uid_map(&self, account: AccountId) -> UidMapFuture<'_, Option<UidMapSnapshot>> {
        let found = self
            .maps
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&account)
            .cloned();
        Box::pin(async move { Ok(found) })
    }

    fn save_uid_map(&self, account: AccountId, changes: UidMapChanges) -> UidMapFuture<'_, ()> {
        *self.saves.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        let mut maps = self.maps.lock().unwrap_or_else(PoisonError::into_inner);
        let map = maps.entry(account).or_default();
        if changes.reset {
            map.mailboxes.clear();
        }
        map.remote_account = changes.remote_account;
        map.state = changes.state;
        map.modseq = changes.modseq;
        map.mailboxes
            .retain(|(m, _)| !changes.removed_mailboxes.contains(&m.mailbox));
        for mailbox in changes.mailboxes {
            match map
                .mailboxes
                .iter_mut()
                .find(|(m, _)| m.mailbox == mailbox.mailbox)
            {
                Some((m, _)) => *m = mailbox,
                None => map.mailboxes.push((mailbox, Vec::new())),
            }
        }
        for (mailbox, uid) in changes.removed {
            if let Some((_, entries)) = map.mailboxes.iter_mut().find(|(m, _)| m.mailbox == mailbox)
            {
                entries.retain(|e| e.uid != uid);
            }
        }
        for (mailbox, entry) in changes.upserts {
            if let Some((_, entries)) = map.mailboxes.iter_mut().find(|(m, _)| m.mailbox == mailbox)
            {
                entries.retain(|e| e.uid != entry.uid);
                entries.push(entry);
            }
        }
        Box::pin(async { Ok(()) })
    }
}

fn tls() -> TlsConfigFn {
    Arc::new(|_| {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|err| BackendError::Tls(err.to_string()))?
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        Ok(Arc::new(config))
    })
}

fn account() -> AccountId {
    "00000000-0000-4000-8000-000000000001".parse().expect("id")
}

fn config(fake: &Fake, access: Access) -> ConnectConfig {
    let server = ServerSettings {
        host: "127.0.0.1".into(),
        port: fake.port,
        security: Security::Tls,
        username: USER.into(),
    };
    let mut config = ConnectConfig::new(&server, Secret::new(PASSWORD.into()), access);
    config.timeouts.read = Duration::from_secs(5);
    config
}

fn connector(store: &Arc<MemoryStore>) -> JmapConnector {
    let connector = JmapConnector::new(tls()).insecure_loopback_for_tests();
    connector.attach_uid_map_store(Arc::clone(store) as Arc<dyn UidMapStore>);
    connector
}

fn subjects(headers: &[mail_model::MessageHeader]) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = headers.iter().map(|h| (h.uid, h.subject.clone())).collect();
    out.sort();
    out
}

#[tokio::test]
async fn first_sync_numbers_by_arrival_and_reads_headers_and_flags() {
    let fake = Fake::start().await;
    for n in 1..=5 {
        fake.add("mi", n, if n % 2 == 0 { &["$seen"] } else { &[] });
    }
    fake.add("ma", 6, &["$flagged", "projekt-x"]);
    let store = Arc::new(MemoryStore::default());
    let mut backend = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    let caps = backend.capabilities().clone();
    assert!(caps.jmap && caps.idle && caps.condstore && !caps.imap4rev1);

    let mailboxes = backend.list_mailboxes().await.expect("list");
    let names: Vec<&str> = mailboxes.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["Archive", "Drafts", "INBOX", "Sent", "Trash"]);
    assert!(mailboxes[0].has_attribute("\\Archive"));

    let inbox = backend.open_mailbox("INBOX").await.expect("open");
    assert_eq!((inbox.uid_next, inbox.exists), (6, 5));
    assert!(inbox.read_only && inbox.highest_modseq.is_some());
    let uids = backend.search_uids(&inbox).await.expect("search");
    assert_eq!(uids, UidSet::range(1, 5));
    // Paging of three per query, two per get: the map and the headers are still complete.
    let headers = backend.fetch_headers(&inbox, &uids).await.expect("headers");
    assert_eq!(
        subjects(&headers),
        (1..=5)
            .map(|n| (n, format!("Message {n}")))
            .collect::<Vec<_>>()
    );
    let first = headers.iter().find(|h| h.uid == 1).expect("uid 1");
    assert_eq!(first.message_id.as_deref(), Some("m1@example.test"));
    assert_eq!(first.list_id.as_deref(), Some("<list.example.test>"));
    assert_eq!(first.from[0].email, "s1@example.test");
    let flags = backend
        .fetch_flags(&inbox, &uids, None)
        .await
        .expect("flags");
    let seen: Vec<u32> = flags
        .iter()
        .filter(|f| f.flags.seen)
        .map(|f| f.uid)
        .collect();
    assert_eq!(seen, [2, 4]);

    let archive = backend.open_mailbox("Archive").await.expect("archive");
    let all = backend.search_uids(&archive).await.expect("search");
    let flags = backend
        .fetch_flags(&archive, &all, None)
        .await
        .expect("flags");
    assert!(flags[0].flags.flagged && flags[0].flags.has_keyword("projekt-x"));
    // A call for a mailbox that is not the open one is refused.
    assert!(matches!(
        backend.search_uids(&inbox).await,
        Err(BackendError::InvalidState(_))
    ));
    let raws = backend
        .fetch_messages(&archive, &all)
        .await
        .expect("bodies");
    assert!(String::from_utf8_lossy(&raws[0].data).contains("Body 6."));
    assert!(matches!(
        backend.open_mailbox("Nowhere").await,
        Err(BackendError::MailboxNotFound(_))
    ));
    backend.noop().await.expect("noop");
}

#[tokio::test]
async fn later_syncs_follow_changes_and_survive_a_restart() {
    let fake = Fake::start().await;
    let ids: Vec<String> = (1..=3).map(|n| fake.add("mi", n, &[])).collect();
    let store = Arc::new(MemoryStore::default());
    let mut backend = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    backend.list_mailboxes().await.expect("list");
    let inbox = backend.open_mailbox("INBOX").await.expect("open");
    let archive = backend.open_mailbox("Archive").await.expect("archive");
    assert_eq!(archive.exists, 0);
    let old = inbox.highest_modseq.expect("modseq");

    // Another client: a new message, a flag, a move to the archive, a deletion.
    fake.add("mi", 4, &[]);
    fake.set_keyword(&ids[0], "$flagged", true);
    fake.move_to(&ids[1], "ma");
    {
        let mut s = fake.lock();
        s.emails.remove(&ids[2]);
        Fake::bump(&mut s, &ids[2], Change::Destroyed);
    }

    let inbox = backend.open_mailbox("INBOX").await.expect("open");
    assert_eq!(inbox.uid_next, 5, "UIDs are never reused");
    assert!(inbox.highest_modseq > Some(old));
    assert_eq!(
        backend.search_uids(&inbox).await.expect("search"),
        UidSet::from_uids([1, 4])
    );
    let changed = backend
        .fetch_flags(&inbox, &UidSet::range(1, 10), Some(old))
        .await
        .expect("changed flags");
    let changed: Vec<(u32, bool)> = changed.iter().map(|f| (f.uid, f.flags.flagged)).collect();
    assert_eq!(changed, [(1, true), (4, false)]);
    let archive_now = backend.open_mailbox("Archive").await.expect("archive");
    assert_eq!(archive_now.uid_validity, archive.uid_validity);
    assert_eq!(
        backend.search_uids(&archive_now).await.expect("search"),
        UidSet::from_uids([1])
    );

    // A new connector over the same store: same numbers, nothing listed again.
    let before = fake
        .methods()
        .iter()
        .filter(|m| *m == "Email/query")
        .count();
    let mut again = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    again.list_mailboxes().await.expect("list");
    let reopened = again.open_mailbox("INBOX").await.expect("open");
    assert_eq!(reopened.uid_validity, inbox.uid_validity);
    assert_eq!(reopened.uid_next, 5);
    assert_eq!(
        again.search_uids(&reopened).await.expect("search"),
        UidSet::from_uids([1, 4])
    );
    let after = fake
        .methods()
        .iter()
        .filter(|m| *m == "Email/query")
        .count();
    assert_eq!(before, after, "the stored map needs no new listing");
}

#[tokio::test]
async fn cannot_calculate_changes_lists_again_and_keeps_numbers() {
    let fake = Fake::start().await;
    let first = fake.add("mi", 1, &[]);
    fake.add("mi", 2, &[]);
    let store = Arc::new(MemoryStore::default());
    let mut backend = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    backend.list_mailboxes().await.expect("list");
    let inbox = backend.open_mailbox("INBOX").await.expect("open");
    fake.lock().cannot_calculate = true;
    {
        let mut s = fake.lock();
        s.emails.remove(&first);
    }
    fake.add("mi", 3, &["$seen"]);
    let again = backend.open_mailbox("INBOX").await.expect("open");
    assert_eq!(again.uid_validity, inbox.uid_validity);
    assert_eq!(
        backend.search_uids(&again).await.expect("search"),
        UidSet::from_uids([2, 3])
    );
    let flags = backend
        .fetch_flags(&again, &UidSet::range(1, 10), inbox.highest_modseq)
        .await
        .expect("flags");
    assert_eq!(flags.len(), 2, "every message counts as changed");
}

#[tokio::test]
async fn read_only_refuses_every_write_before_sending() {
    let fake = Fake::start().await;
    fake.add("mi", 1, &[]);
    let store = Arc::new(MemoryStore::default());
    let connector = connector(&store);
    let err = connector
        .connect_writer(&config(&fake, Access::ReadOnly))
        .await
        .err()
        .expect("refused");
    assert!(matches!(err, BackendError::ReadOnly(_)), "{err:?}");
    assert!(
        fake.methods().is_empty(),
        "nothing went out: {:?}",
        fake.methods()
    );

    let sender = JmapSender::new(tls()).insecure_loopback_for_tests();
    let envelope = Envelope {
        from: USER.into(),
        recipients: vec!["bob@example.test".into()],
    };
    let err = sender
        .send(
            &config(&fake, Access::ReadOnly),
            &envelope,
            b"Subject: x\r\n\r\nx",
        )
        .await
        .expect_err("refused");
    assert!(matches!(err, BackendError::ReadOnly(_)), "{err:?}");
    assert!(fake.methods().is_empty());

    // The client of a read-only connection refuses writing methods itself.
    let backend = connector
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    let before = fake.methods().len();
    for method in [
        "Email/set",
        "Email/import",
        "Mailbox/set",
        "EmailSubmission/set",
    ] {
        let err = backend
            .call(vec![(method, json!({"accountId": ACCOUNT}), "x")])
            .await
            .expect_err("refused");
        assert_eq!(err, BackendError::ReadOnly(method.into()));
    }
    let upload = backend.session.upload().expect("url");
    let err = backend
        .client
        .upload(&upload, "message/rfc822", b"x")
        .await
        .expect_err("refused");
    assert!(matches!(err, BackendError::ReadOnly(_)));
    assert_eq!(
        fake.methods().len(),
        before,
        "no request for refused writes"
    );
    // A read-only connection still reads, and the server data is unchanged.
    assert!(
        fake.lock()
            .log
            .iter()
            .all(|(_, _, c)| *c == Change::Created)
    );
}

#[tokio::test]
async fn writes_go_through_email_set_and_report_uids() {
    let fake = Fake::start().await;
    let ids: Vec<String> = (1..=3).map(|n| fake.add("mi", n, &[])).collect();
    let store = Arc::new(MemoryStore::default());
    let connector = connector(&store);
    let mut writer = connector
        .connect_writer_for(&config(&fake, Access::ReadWrite), Some(account()))
        .await
        .expect("writer");
    writer.list_mailboxes().await.expect("list");
    let inbox = writer.open_mailbox("INBOX").await.expect("open");
    assert!(!inbox.read_only);

    // Flags.
    let outcome = writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([1, 2]),
            StoreMode::Add,
            &[Flag::Seen],
            None,
        )
        .await
        .expect("store");
    assert_eq!(outcome.updates.len(), 2);
    assert!(outcome.updates.iter().all(|u| u.flags.seen));
    assert!(fake.email(&ids[0]).expect("e1").keywords.contains("$seen"));

    // The own change is no conflict for the next change with the modseq it reported.
    let mine = outcome
        .updates
        .iter()
        .find(|u| u.uid == 1)
        .and_then(|u| u.modseq);
    let next = writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([1]),
            StoreMode::Add,
            &[Flag::Answered],
            mine,
        )
        .await
        .expect("store");
    assert!(
        next.modified.is_empty() && next.updates.len() == 1,
        "{next:?}"
    );

    // Conflict: changed by someone else after the modseq the caller knows.
    let known = inbox.highest_modseq.expect("modseq");
    fake.set_keyword(&ids[2], "$flagged", true);
    let outcome = writer
        .store_flags(
            &inbox,
            &UidSet::from_uids([3]),
            StoreMode::Remove,
            &[Flag::Flagged],
            Some(known),
        )
        .await
        .expect("store");
    assert_eq!(outcome.modified, UidSet::from_uids([3]));
    assert!(
        fake.email(&ids[2])
            .expect("e3")
            .keywords
            .contains("$flagged"),
        "left alone"
    );

    // Move with COPYUID.
    let status = writer.mailbox_status("Archive").await.expect("status");
    let moved = writer
        .move_messages(&inbox, &UidSet::from_uids([1]), "Archive")
        .await
        .expect("move");
    let copy = moved.copy_uid.expect("copyuid");
    assert_eq!(copy.uid_validity, status.uid_validity.expect("validity"));
    assert_eq!(copy.pairs, [(1, status.uid_next.expect("next"))]);
    assert_eq!(
        fake.email(&ids[0]).expect("e1").mailboxes,
        BTreeSet::from(["ma".to_owned()])
    );

    // Expunge.
    let inbox = writer.open_mailbox("INBOX").await.expect("open");
    writer
        .expunge_messages(&inbox, &UidSet::from_uids([2]))
        .await
        .expect("expunge");
    assert!(fake.email(&ids[1]).is_none());
    assert_eq!(
        writer.search_uids(&inbox).await.expect("search"),
        UidSet::from_uids([3])
    );

    // Append with APPENDUID, found again by Message-ID.
    let appended = writer
        .append(
            "Drafts",
            &[Flag::Seen, Flag::Draft],
            b"From: a@example.test\r\nSubject: Draft\r\nMessage-ID: <draft-1@example.test>\r\n\r\nx\r\n",
        )
        .await
        .expect("append")
        .expect("appenduid");
    let drafts = writer.open_mailbox("Drafts").await.expect("drafts");
    assert_eq!(drafts.uid_validity, appended.uid_validity);
    assert_eq!(
        writer
            .find_message_id(&drafts, "draft-1@example.test")
            .await
            .expect("find"),
        UidSet::from_uids([appended.uid])
    );

    // Create a folder below another, twice.
    assert_eq!(
        writer
            .create_mailbox("Projects/2026", None)
            .await
            .expect("create"),
        "Projects/2026"
    );
    writer
        .create_mailbox("Projects/2026", None)
        .await
        .expect("again");
    let names: Vec<String> = writer
        .list_mailboxes()
        .await
        .expect("list")
        .into_iter()
        .map(|m| m.name)
        .collect();
    assert!(names.contains(&"Projects/2026".to_owned()), "{names:?}");
    assert_eq!(
        names.iter().filter(|n| n.starts_with("Projects")).count(),
        2
    );
}

#[tokio::test]
async fn push_wakes_idle_and_stop_and_time_end_it() {
    let fake = Fake::start().await;
    fake.add("mi", 1, &[]);
    let store = Arc::new(MemoryStore::default());
    let mut backend = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    let inbox = backend.open_mailbox("INBOX").await.expect("open");

    // Quiet: the time runs out; a stop ends it at once.
    let outcome = backend
        .idle(&inbox, Duration::from_millis(300), std::future::pending())
        .await
        .expect("idle");
    assert_eq!(outcome, IdleOutcome::TimedOut);
    let outcome = backend
        .idle(&inbox, Duration::from_secs(30), async {})
        .await
        .expect("idle");
    assert_eq!(outcome, IdleOutcome::Stopped);

    // A message arrives while idling.
    let pusher = fake.clone();
    let push = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        pusher.add("mi", 2, &[]);
    });
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        backend.idle(&inbox, Duration::from_secs(30), std::future::pending()),
    )
    .await
    .expect("in time")
    .expect("idle");
    assert_eq!(outcome, IdleOutcome::Changed);
    push.await.expect("pusher");

    // A change while not idling is reported at the next idle at once.
    fake.add("mi", 3, &[]);
    let outcome = backend
        .idle(&inbox, Duration::from_secs(30), std::future::pending())
        .await
        .expect("idle");
    assert_eq!(outcome, IdleOutcome::Changed);

    // Without an event source: no IDLE capability, idle is unsupported.
    fake.lock().push = false;
    let mut quiet = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    assert!(!quiet.capabilities().idle);
    let inbox = quiet.open_mailbox("INBOX").await.expect("open");
    assert!(matches!(
        quiet.idle(&inbox, Duration::from_secs(1), async {}).await,
        Err(BackendError::Unsupported(_))
    ));
}

#[tokio::test]
async fn sending_imports_submits_and_cleans_up() {
    let fake = Fake::start().await;
    let sender = JmapSender::new(tls()).insecure_loopback_for_tests();
    let config = config(&fake, Access::ReadWrite);
    sender.verify(&config).await.expect("verify");
    let envelope = Envelope {
        from: USER.into(),
        recipients: vec!["bob@example.test".into(), "carol@example.test".into()],
    };
    let message = b"From: alice@example.test\r\nTo: bob@example.test\r\nSubject: Hi\r\n\r\nHi\r\n";
    let outcome = sender
        .send(&config, &envelope, message)
        .await
        .expect("send");
    assert!(matches!(outcome, SendOutcome::Accepted(_)), "{outcome:?}");
    let submission = fake.lock().submissions[0].clone();
    assert_eq!(submission["identityId"], "i1");
    assert_eq!(
        submission["envelope"]["rcptTo"],
        json!([{"email": "bob@example.test"}, {"email": "carol@example.test"}])
    );
    assert!(fake.lock().emails.is_empty(), "the imported copy is gone");

    fake.lock().reject_submission = Some(json!({"type": "invalidRecipients",
        "invalidRecipients": ["carol@example.test"]}));
    let outcome = sender
        .send(&config, &envelope, message)
        .await
        .expect("send");
    let SendOutcome::Rejected(rejection) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(rejection.recipients[0].address, "carol@example.test");
    assert!(fake.lock().emails.is_empty(), "the refused copy is removed");

    let wrong = Envelope {
        from: "mallory@example.test".into(),
        recipients: vec!["bob@example.test".into()],
    };
    let err = sender
        .send(&config, &wrong, message)
        .await
        .expect_err("no identity");
    assert!(matches!(err, BackendError::Rejected(_)), "{err:?}");
}

#[tokio::test]
async fn bodies_in_sections_and_server_search() {
    let fake = Fake::start().await;
    fake.add("mi", 1, &[]);
    fake.add("mi", 2, &[]);
    let store = Arc::new(MemoryStore::default());
    let mut backend = connector(&store)
        .connect_for(&config(&fake, Access::ReadOnly), Some(account()))
        .await
        .expect("connect");
    let inbox = backend.open_mailbox("INBOX").await.expect("open");
    let structure = backend
        .fetch_structure(&inbox, 2)
        .await
        .expect("structure")
        .expect("found");
    assert_eq!(structure.parts.len(), 1);
    let path: mail_model::PartPath = "1".parse().expect("path");
    let sections = backend
        .fetch_sections(
            &inbox,
            2,
            &[
                mail_model::BodySection::Header,
                mail_model::BodySection::Mime(path.clone()),
                mail_model::BodySection::Part(path),
            ],
        )
        .await
        .expect("sections")
        .expect("found");
    let texts: Vec<String> = sections
        .iter()
        .map(|s| String::from_utf8_lossy(&s.data).into_owned())
        .collect();
    assert!(texts[0].contains("Subject: Message 2\r\n") && texts[0].ends_with("\r\n\r\n"));
    assert!(texts[1].contains("Content-Transfer-Encoding: binary"));
    assert_eq!(texts[2], "Body 2.\r\n");
    let found = backend
        .search_text(
            &inbox,
            &[mail_model::search::ServerCriterion::Subject(
                "Message 2".into(),
            )],
        )
        .await
        .expect("search");
    assert_eq!(found, UidSet::from_uids([2]));
    assert!(
        backend
            .fetch_structure(&inbox, 99)
            .await
            .expect("structure")
            .is_none()
    );
}

#[tokio::test]
async fn wrong_password_and_missing_service_are_typed() {
    let fake = Fake::start().await;
    let store = Arc::new(MemoryStore::default());
    let mut bad = config(&fake, Access::ReadOnly);
    bad.password = Secret::new("wrong".into());
    let err = connector(&store)
        .connect(&bad)
        .await
        .err()
        .expect("refused");
    assert!(matches!(err, BackendError::AuthFailed(_)), "{err:?}");
    assert!(
        !format!("{err:?}").contains("wrong"),
        "the password never shows: {err:?}"
    );

    // A web server without JMAP.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf).await;
            write(&mut stream, 404, "text/html", b"<html>").await;
        }
    });
    let mut other = config(&fake, Access::ReadOnly);
    other.port = port;
    let err = connector(&store)
        .connect(&other)
        .await
        .err()
        .expect("no JMAP");
    assert!(matches!(err, BackendError::JmapUnavailable(_)), "{err:?}");
    assert_eq!(err.code(), mail_model::ErrorCode::JmapUnavailable);

    // Nothing listening.
    drop(fake);
    let closed = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = closed.local_addr().expect("addr").port();
    drop(closed);
    let mut nowhere = config(&Fake::start().await, Access::ReadOnly);
    nowhere.port = port;
    let err = connector(&store)
        .connect(&nowhere)
        .await
        .err()
        .expect("refused");
    assert!(matches!(err, BackendError::ConnectionRefused), "{err:?}");
}

#[tokio::test]
async fn the_probe_finds_a_session_without_credentials() {
    let fake = Fake::start().await;
    let config = tls()(None).expect("tls");
    let http = reqwest::Client::builder()
        .tls_backend_preconfigured((*config).clone())
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client");
    let found = crate::probe(&http, "127.0.0.1", fake.port, true)
        .await
        .expect("probe")
        .expect("found");
    assert_eq!(found.port, fake.port);
    assert_eq!(found.host, "127.0.0.1");
}
