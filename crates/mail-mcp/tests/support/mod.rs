//! Shared test setup: a fake [`MailAccess`] with three accounts, a recording access log, a host
//! on a socket in a temporary directory, and MCP clients (rmcp in both protocol eras, or raw
//! JSON-RPC lines) talking to an in-process helper.
//!
//! The fake is deliberately leaky: it ignores the account restrictions it is given and returns
//! messages of every account, so the tests show that the host's own checks hide what is not
//! shared, even if `MailAccess` gets it wrong. It records which accounts it was asked for.

#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mail_mcp::helper::{AppBackend, HelperConfig};
use mail_mcp::mcp::McpServer;
use mail_mcp::{
    AccessEvent, AccountAccess, BodyParts, ClientRecord, ClientToken, DraftCreated, Endpoint,
    HostStatus, ListFilter, ListQuery, MailAccess, McpHost, McpSettings, MessageContent, NewDraft,
    SearchQuery, SearchResults, ThreadContent,
};
use mail_model::{
    Account, AccountColor, AccountId, AccountState, AccountStatus, Address, CoreError, ListCursor,
    Mailbox, MailboxId, MailboxRole, MessageId, MessageListItem, RoleSource, Security,
    ServerSettings,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Notify, oneshot, watch};

pub const A: u128 = 0xA;
pub const B: u128 = 0xB;
pub const C: u128 = 0xC;

pub fn account_id(n: u128) -> AccountId {
    AccountId::from_uuid(uuid::Uuid::from_u128(n))
}

/// Text only in messages of the hidden account B.
pub const SECRET: &str = "TOPSECRET";

fn account(n: u128, email: &str, name: &str) -> Account {
    let server = ServerSettings {
        host: "mail.example".into(),
        port: 993,
        security: Security::Tls,
        username: email.into(),
    };
    Account {
        id: account_id(n),
        email: email.into(),
        display_name: name.into(),
        color: AccountColor::new(0).unwrap(),
        imap: server.clone(),
        smtp: server,
        // Stage 14 (decision 0042): a password account.
        oauth: None,
        writes_enabled: false,
        status: AccountStatus {
            state: AccountState::Idle,
            progress: None,
            error: None,
            retry_at: None,
            last_sync_at: None,
        },
    }
}

fn mailbox(id: i64, account: u128, path: &str, role: MailboxRole) -> Mailbox {
    Mailbox {
        id: MailboxId(id),
        account_id: account_id(account),
        path: path.into(),
        name: path.rsplit('/').next().unwrap().into(),
        delimiter: Some("/".into()),
        parent_id: None,
        role,
        role_source: RoleSource::Server,
        selectable: true,
        unread: 1,
        total: 3,
        favorite_position: None,
        color: None,
    }
}

pub fn address(name: &str, email: &str) -> Address {
    Address {
        name: Some(name.into()),
        email: email.into(),
    }
}

/// A plain text message.
pub fn text_message(
    id: i64,
    account: u128,
    mailbox: i64,
    subject: &str,
    text: &str,
) -> MessageContent {
    MessageContent {
        id: MessageId(id),
        account_id: account_id(account),
        mailbox_ids: vec![MailboxId(mailbox)],
        date: 1_795_000_000_000 + id * 60_000,
        subject: subject.into(),
        from: vec![address("Bob Sender", "bob@example.com")],
        to: vec![address("Alice", "alice@example.com")],
        cc: Vec::new(),
        reply_to: Vec::new(),
        unread: id % 2 == 0,
        flagged: id % 3 == 0,
        answered: false,
        body: Some(BodyParts {
            text: Some(text.into()),
            html: None,
            attachments: Vec::new(),
        }),
    }
}

/// A message from raw RFC 5322 bytes, parsed like the app does.
pub fn parsed_message(id: i64, account: u128, mailbox: i64, raw: &[u8]) -> MessageContent {
    let parsed = mail_content::parse(raw);
    let headers = &parsed.headers;
    MessageContent {
        id: MessageId(id),
        account_id: account_id(account),
        mailbox_ids: vec![MailboxId(mailbox)],
        date: headers.date.unwrap_or(1_795_000_000) * 1000,
        subject: headers.subject.clone().unwrap_or_default(),
        from: headers.from.clone(),
        to: headers.to.clone(),
        cc: headers.cc.clone(),
        reply_to: headers.reply_to.clone(),
        unread: true,
        flagged: false,
        answered: false,
        body: Some(BodyParts::from_parsed(&parsed)),
    }
}

/// One call the host made, with the accounts it named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Accounts,
    Mailboxes(AccountId),
    Search(SearchQuery),
    List(ListQuery),
    Message(MessageId, Vec<AccountId>),
    Thread(MessageId, u32, Vec<AccountId>),
    CreateDraft(NewDraft),
}

pub struct FakeAccess {
    pub accounts: Vec<Account>,
    pub mailboxes: Vec<Mailbox>,
    pub messages: Mutex<Vec<MessageContent>>,
    /// Thread key of a message id; messages with the same key form a thread.
    pub threads: BTreeMap<i64, u32>,
    pub calls: Mutex<Vec<Call>>,
    /// When set, `message` waits for it (to revoke a client during a call).
    pub gate: Mutex<Option<Arc<Notify>>>,
    /// Ignore the accounts the host names and return data of every account, to show that the
    /// host's own checks hide what is not shared.
    pub leaky: bool,
}

impl FakeAccess {
    /// Accounts A (work), B (private, holds the secret) and C (drafts allowed in the default
    /// settings), each with an inbox and more.
    pub fn standard() -> Self {
        let messages = vec![
            text_message(
                101,
                A,
                10,
                "Quarterly report",
                "The report is attached. Regards, Bob",
            ),
            text_message(102, A, 10, "Re: Quarterly report", "Thanks, looks good."),
            text_message(103, A, 11, "Old invoice", "Invoice 2025-17 is paid."),
            text_message(104, A, 12, "Deleted newsletter", "unread in the trash"),
            text_message(
                201,
                B,
                20,
                "Private note",
                &format!("{SECRET} the safe code is 1234"),
            ),
            text_message(
                202,
                B,
                20,
                &format!("{SECRET} plans"),
                "report about the surprise",
            ),
            text_message(301, C, 30, "Team lunch", "Friday at noon? report back"),
        ];
        let mut messages = messages;
        messages[1].answered = true;
        Self {
            accounts: vec![
                account(A, "alice@work.example", "Work"),
                account(B, "alice@private.example", "Private"),
                account(C, "alice@club.example", "Club"),
            ],
            mailboxes: vec![
                mailbox(10, A, "INBOX", MailboxRole::Inbox),
                mailbox(11, A, "Archive", MailboxRole::Archive),
                mailbox(12, A, "Trash", MailboxRole::Trash),
                mailbox(20, B, "INBOX", MailboxRole::Inbox),
                mailbox(30, C, "INBOX", MailboxRole::Inbox),
                mailbox(31, C, "Drafts", MailboxRole::Drafts),
            ],
            messages: Mutex::new(messages),
            threads: BTreeMap::from([(101, 1), (102, 1), (201, 2), (202, 2)]),
            calls: Mutex::new(Vec::new()),
            gate: Mutex::new(None),
            leaky: false,
        }
    }

    /// [`FakeAccess::standard`], but ignoring every account restriction.
    pub fn leaky() -> Self {
        Self {
            leaky: true,
            ..Self::standard()
        }
    }

    fn allowed(&self, accounts: &[AccountId], message: &MessageContent) -> bool {
        self.leaky || accounts.contains(&message.account_id)
    }

    pub fn add(&self, message: MessageContent) {
        self.messages.lock().unwrap().push(message);
    }

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn drafts(&self) -> Vec<NewDraft> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                Call::CreateDraft(draft) => Some(draft),
                _ => None,
            })
            .collect()
    }

    /// Every account the host named in any call.
    pub fn accounts_asked(&self) -> Vec<AccountId> {
        let mut asked = Vec::new();
        for call in self.calls() {
            match call {
                Call::Accounts => {}
                Call::Mailboxes(account) => asked.push(account),
                Call::Search(q) => asked.extend(q.accounts),
                Call::List(q) => asked.extend(q.accounts),
                Call::Message(_, accounts) | Call::Thread(_, _, accounts) => asked.extend(accounts),
                Call::CreateDraft(draft) => asked.push(draft.account_id),
            }
        }
        asked
    }

    fn record(&self, call: Call) {
        self.calls.lock().unwrap().push(call);
    }

    fn item(message: &MessageContent) -> MessageListItem {
        let text = message
            .body
            .as_ref()
            .and_then(|b| b.text.clone().or_else(|| b.html.clone()))
            .unwrap_or_default();
        MessageListItem {
            id: message.id,
            account_id: message.account_id,
            mailbox_id: message.mailbox_ids[0],
            from: message.from.first().cloned(),
            to: message.to.clone(),
            subject: message.subject.clone(),
            date: message.date,
            unread: message.unread,
            flagged: message.flagged,
            answered: message.answered,
            draft: false,
            has_attachments: message
                .body
                .as_ref()
                .is_some_and(|b| !b.attachments.is_empty()),
            keywords: Vec::new(),
            preview: Some(text.chars().take(120).collect()),
            activity: None,
            category: None,
            important: false,
            pinned: false,
            snooze: None,
            task: None,
            reading_list: false,
        }
    }

    fn page(
        mut hits: Vec<MessageContent>,
        limit: u32,
        cursor: Option<ListCursor>,
    ) -> SearchResults {
        hits.sort_by_key(|m| std::cmp::Reverse(m.id));
        let hits: Vec<MessageContent> = hits
            .into_iter()
            .filter(|m| cursor.is_none_or(|c| m.id.0 < c.0))
            .collect();
        let limit = usize::try_from(limit).unwrap();
        let next_cursor = (hits.len() > limit).then(|| ListCursor(hits[limit - 1].id.0));
        SearchResults {
            hits: hits.iter().take(limit).map(Self::item).collect(),
            next_cursor,
            problems: Vec::new(),
        }
    }

    fn in_trash_or_junk(&self, message: &MessageContent) -> bool {
        self.mailboxes.iter().any(|mb| {
            message.mailbox_ids.contains(&mb.id)
                && matches!(mb.role, MailboxRole::Trash | MailboxRole::Junk)
        })
    }
}

impl MailAccess for FakeAccess {
    async fn accounts(&self) -> Result<Vec<Account>, CoreError> {
        self.record(Call::Accounts);
        Ok(self.accounts.clone())
    }

    async fn mailboxes(&self, account: AccountId) -> Result<Vec<Mailbox>, CoreError> {
        self.record(Call::Mailboxes(account));
        Ok(self
            .mailboxes
            .iter()
            .filter(|m| m.account_id == account)
            .cloned()
            .collect())
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResults, CoreError> {
        self.record(Call::Search(query.clone()));
        let words: Vec<String> = query
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let hits = self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|m| self.allowed(&query.accounts, m))
            .filter(|m| query.mailbox.is_none_or(|mb| m.mailbox_ids.contains(&mb)))
            .filter(|m| {
                let body = m.body.clone().unwrap_or_default();
                let haystack = format!(
                    "{} {} {} {:?}",
                    m.subject,
                    body.text.unwrap_or_default(),
                    body.html.unwrap_or_default(),
                    m.from
                )
                .to_lowercase();
                words.iter().all(|w| haystack.contains(w.as_str()))
            })
            .cloned()
            .collect();
        Ok(Self::page(hits, query.limit, query.cursor))
    }

    async fn list_messages(&self, query: ListQuery) -> Result<SearchResults, CoreError> {
        self.record(Call::List(query.clone()));
        let hits = self
            .messages
            .lock()
            .unwrap()
            .iter()
            .filter(|m| self.allowed(&query.accounts, m))
            .filter(|m| match query.mailbox {
                Some(mb) => m.mailbox_ids.contains(&mb),
                None => !self.in_trash_or_junk(m),
            })
            .filter(|m| match query.filter {
                ListFilter::Unread => m.unread,
                ListFilter::Flagged => m.flagged,
            })
            .cloned()
            .collect();
        Ok(Self::page(hits, query.limit, query.cursor))
    }

    async fn message(
        &self,
        id: MessageId,
        accounts: Vec<AccountId>,
    ) -> Result<Option<MessageContent>, CoreError> {
        self.record(Call::Message(id, accounts.clone()));
        let gate = self.gate.lock().unwrap().clone();
        if let Some(gate) = gate {
            gate.notified().await;
        }
        Ok(self
            .messages
            .lock()
            .unwrap()
            .iter()
            .find(|m| m.id == id && self.allowed(&accounts, m))
            .cloned())
    }

    async fn thread(
        &self,
        id: MessageId,
        newest: u32,
        accounts: Vec<AccountId>,
    ) -> Result<Option<ThreadContent>, CoreError> {
        self.record(Call::Thread(id, newest, accounts.clone()));
        let messages = self.messages.lock().unwrap().clone();
        let Some(start) = messages
            .iter()
            .find(|m| m.id == id && self.allowed(&accounts, m))
        else {
            return Ok(None);
        };
        let mut thread: Vec<MessageContent> = match self.threads.get(&id.0) {
            Some(key) => messages
                .iter()
                .filter(|m| self.threads.get(&m.id.0) == Some(key))
                .cloned()
                .collect(),
            None => vec![start.clone()],
        };
        thread.sort_by_key(|m| m.date);
        let total = u32::try_from(thread.len()).unwrap();
        let skip = thread
            .len()
            .saturating_sub(usize::try_from(newest).unwrap());
        Ok(Some(ThreadContent {
            messages: thread.split_off(skip),
            total,
        }))
    }

    async fn create_draft(&self, draft: NewDraft) -> Result<DraftCreated, CoreError> {
        self.record(Call::CreateDraft(draft));
        Ok(DraftCreated::default())
    }
}

#[derive(Default)]
pub struct RecordingLog {
    pub events: Mutex<Vec<AccessEvent>>,
}

impl RecordingLog {
    pub fn events(&self) -> Vec<AccessEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl mail_mcp::AccessLog for RecordingLog {
    fn record(&self, event: AccessEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// MCP on; A readable, B hidden, C readable with drafts; one client.
pub fn standard_settings(token: &ClientToken) -> McpSettings {
    McpSettings {
        enabled: true,
        accounts: BTreeMap::from([
            (account_id(A), AccountAccess::Read),
            (account_id(B), AccountAccess::Hidden),
            (account_id(C), AccountAccess::ReadAndDrafts),
        ]),
        clients: vec![ClientRecord {
            id: token.id,
            name: "Test client".into(),
            key: token.key.clone(),
        }],
    }
}

/// A host on a socket in a temporary directory.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub endpoint: Endpoint,
    pub settings: watch::Sender<McpSettings>,
    pub access: Arc<FakeAccess>,
    pub log: Arc<RecordingLog>,
    pub token: ClientToken,
    pub host: McpHost<FakeAccess>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Fixture {
    pub async fn start() -> Self {
        Self::start_with(FakeAccess::standard(), standard_settings).await
    }

    pub async fn start_with(access: FakeAccess, settings: fn(&ClientToken) -> McpSettings) -> Self {
        let dir = tempfile::Builder::new().prefix("mcp").tempdir().unwrap();
        let endpoint = endpoint_in(dir.path());
        let token = ClientToken::generate().unwrap();
        let (settings_tx, settings_rx) = watch::channel(settings(&token));
        let access = Arc::new(access);
        let log = Arc::new(RecordingLog::default());
        let host = McpHost::new(Arc::clone(&access), settings_rx, log.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn({
            let host = host.clone();
            let endpoint = endpoint.clone();
            async move {
                host.run(endpoint, async {
                    let _ = shutdown_rx.await;
                })
                .await;
            }
        });
        let fixture = Self {
            dir,
            endpoint,
            settings: settings_tx,
            access,
            log,
            token,
            host,
            shutdown: Some(shutdown_tx),
            task: Some(task),
        };
        if fixture.settings.borrow().enabled {
            fixture.wait_status(HostStatus::Listening).await;
        }
        fixture
    }

    pub async fn wait_status(&self, wanted: HostStatus) {
        let mut status = self.host.status();
        tokio::time::timeout(Duration::from_secs(5), status.wait_for(|s| *s == wanted))
            .await
            .unwrap_or_else(|_| panic!("host status never became {wanted:?}"))
            .unwrap();
    }

    pub fn config(&self) -> HelperConfig {
        HelperConfig {
            endpoint: self.endpoint.clone(),
            token: self.token.clone(),
        }
    }

    pub fn update(&self, change: impl FnOnce(&mut McpSettings)) {
        self.settings.send_modify(change);
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            tokio::time::timeout(Duration::from_secs(5), task)
                .await
                .expect("host stops")
                .unwrap();
        }
    }
}

pub fn endpoint_in(dir: &Path) -> Endpoint {
    #[cfg(unix)]
    {
        Endpoint::for_data_dir(dir).unwrap()
    }
    #[cfg(windows)]
    {
        let _ = dir;
        Endpoint::random_pipe().unwrap()
    }
}

/// An in-process helper for `config`, reachable over raw JSON-RPC lines.
pub struct RawClient {
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    reader: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    next: u64,
}

pub fn spawn_helper(config: Result<HelperConfig, String>) -> tokio::io::DuplexStream {
    let (client, server) = tokio::io::duplex(4 << 20);
    let (read, write) = tokio::io::split(server);
    tokio::spawn(async move {
        let _ = McpServer::new(AppBackend::new(config))
            .serve(read, write)
            .await;
    });
    client
}

impl RawClient {
    pub fn new(config: Result<HelperConfig, String>) -> Self {
        let (read, writer) = tokio::io::split(spawn_helper(config));
        Self {
            writer,
            reader: BufReader::new(read),
            next: 0,
        }
    }

    /// Sends a modern (2026-07-28) request and returns the whole response.
    pub async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let mut params = params;
        params["_meta"] = json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
            "io.modelcontextprotocol/clientInfo": { "name": "raw-test", "version": "1" }
        });
        let message =
            json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params });
        let mut line = serde_json::to_vec(&message).unwrap();
        line.push(b'\n');
        self.writer.write_all(&line).await.unwrap();
        let mut answer = String::new();
        tokio::time::timeout(Duration::from_secs(20), self.reader.read_line(&mut answer))
            .await
            .expect("helper answers")
            .unwrap();
        let answer: Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(answer["id"], self.next, "{answer}");
        answer
    }

    pub async fn tool_names(&mut self) -> Vec<String> {
        let list = self.request("tools/list", json!({})).await;
        list["result"]["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("no tools: {list}"))
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_owned())
            .collect()
    }

    /// `tools/call`; returns (text, isError). Panics on a JSON-RPC error.
    pub async fn call(&mut self, tool: &str, arguments: Value) -> (String, bool) {
        let answer = self
            .request(
                "tools/call",
                json!({ "name": tool, "arguments": arguments }),
            )
            .await;
        let result = &answer["result"];
        assert!(result.is_object(), "{answer}");
        (
            result["content"][0]["text"].as_str().unwrap().to_owned(),
            result["isError"].as_bool().unwrap(),
        )
    }
}

/// The JSON inside the single data block of a tool result. Panics unless the result has exactly
/// one block, opened and closed with the same boundary, and nothing after it but a note.
pub fn data_block(text: &str) -> Value {
    let (inside, _) = split_block(text);
    serde_json::from_str(inside).unwrap_or_else(|err| panic!("block is not JSON ({err}): {inside}"))
}

/// (inside of the block, text outside it).
pub fn split_block(text: &str) -> (&str, String) {
    assert!(
        text.starts_with(mail_mcp::output::UNTRUSTED_NOTICE),
        "{text}"
    );
    let open = "<mail_data boundary=\"";
    assert_eq!(
        text.matches("<mail_data").count(),
        1,
        "one opening tag: {text}"
    );
    assert_eq!(
        text.matches("</mail_data").count(),
        1,
        "one closing tag: {text}"
    );
    let start = text.find(open).unwrap() + open.len();
    let boundary = &text[start..start + 24];
    let head_end = start + 24 + "\">\n".len();
    assert_eq!(&text[start + 24..head_end], "\">\n");
    let close = format!("\n</mail_data boundary=\"{boundary}\">");
    let end = text
        .rfind(&close)
        .expect("closing tag with the same boundary");
    let outside = format!("{}{}", &text[..head_end], &text[end..]);
    (&text[head_end..end], outside)
}
