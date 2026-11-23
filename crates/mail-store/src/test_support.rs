//! Fixtures shared by the mail tests, the plan tests and the benchmark.

use std::path::PathBuf;

use mail_model::{
    AccountId, Address, Flags, MailboxId, MailboxRole, MessageHeader, RemoteMailbox, Security,
    ServerSettings, UidValidity,
};
use tempfile::TempDir;

use crate::{MailboxBaseline, NewAccount, Store, StoreOptions};

/// UIDVALIDITY the fixtures sync with.
pub(crate) const UV: UidValidity = UidValidity(1_000);

pub(crate) fn db_path(dir: &TempDir) -> PathBuf {
    dir.path().join("data").join("mail.sqlite3")
}

pub(crate) fn account_id(n: u32) -> AccountId {
    format!("00000000-0000-4000-8000-{n:012}")
        .parse()
        .expect("valid uuid")
}

pub(crate) fn new_account(n: u32, email: &str) -> NewAccount {
    let server = |host: &str, port| ServerSettings {
        host: host.into(),
        port,
        security: Security::Tls,
        username: email.into(),
    };
    NewAccount {
        id: account_id(n),
        email: email.into(),
        display_name: format!("User {n}"),
        imap: server("imap.example.org", 993),
        smtp: server("smtp.example.org", 465),
        credential_ref: format!("vice-mail:{}", account_id(n)),
    }
}

pub(crate) fn remote(name: &str, attributes: &[&str]) -> RemoteMailbox {
    RemoteMailbox {
        name: name.into(),
        path: name.into(),
        delimiter: Some('/'),
        attributes: attributes.iter().map(|a| (*a).to_owned()).collect(),
        status: None,
    }
}

/// A header with `Date` = INTERNALDATE = `date`, unread, from `sender@example.org`.
pub(crate) fn header(uid: u32, date: i64, message_id: Option<&str>) -> MessageHeader {
    MessageHeader {
        uid,
        modseq: None,
        flags: Flags::default(),
        size: 1_000 + uid,
        internal_date: date,
        date: Some(date),
        subject: format!("Subject {uid}"),
        from: vec![Address {
            name: Some("Sender".into()),
            email: "sender@example.org".into(),
        }],
        to: vec![Address {
            name: None,
            email: "me@example.org".into(),
        }],
        cc: Vec::new(),
        message_id: message_id.map(str::to_owned),
        in_reply_to: None,
        references: Vec::new(),
        list_id: None,
        list_unsubscribe: None,
        auto_submitted: None,
        precedence: None,
        has_attachments: false,
        category_headers: Vec::new(),
    }
}

pub(crate) fn seen(mut h: MessageHeader) -> MessageHeader {
    h.flags.seen = true;
    h
}

/// A store with one account (INBOX, Archive, Sent) whose mailboxes are reset to [`UV`].
pub(crate) struct Fixture {
    /// Keeps the database directory alive for the test.
    pub _dir: TempDir,
    pub store: Store,
    pub account: AccountId,
    pub inbox: MailboxId,
    pub archive: MailboxId,
    pub sent: MailboxId,
}

pub(crate) async fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
    let account = store
        .insert_account(new_account(1, "alice@example.org"))
        .await
        .expect("insert account")
        .id;
    store
        .sync_mailboxes(
            account,
            vec![
                remote("INBOX", &[]),
                remote("Archive", &["\\Archive"]),
                remote("Sent", &["\\Sent"]),
            ],
        )
        .await
        .expect("sync mailboxes");
    let boxes = store.mailboxes(account).await.expect("mailboxes");
    let by_role = |role| {
        boxes
            .iter()
            .find(|m| m.role == role)
            .map(|m| m.id)
            .expect("mailbox with role")
    };
    let (inbox, archive, sent) = (
        by_role(MailboxRole::Inbox),
        by_role(MailboxRole::Archive),
        by_role(MailboxRole::Sent),
    );
    for mailbox in [inbox, archive, sent] {
        store
            .reset_mailbox(
                mailbox,
                MailboxBaseline {
                    uid_validity: UV,
                    uid_next: 1,
                    highest_modseq: None,
                },
            )
            .await
            .expect("reset");
    }
    Fixture {
        _dir: dir,
        store,
        account,
        inbox,
        archive,
        sent,
    }
}

/// All message ids of a mailbox, newest first, by paging with `limit`.
pub(crate) async fn all_ids(store: &Store, mailbox: MailboxId, limit: u32) -> Vec<i64> {
    let mut ids = Vec::new();
    let mut cursor = None;
    loop {
        let page = store
            .list_messages(mailbox, cursor, limit)
            .await
            .expect("list");
        ids.extend(page.items.iter().map(|i| i.id.0));
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    ids
}

/// Runs one query on a reader and returns its single integer.
pub(crate) async fn count(store: &Store, sql: &'static str) -> i64 {
    store
        .read(move |conn| Ok(conn.query_row(sql, [], |r| r.get(0))?))
        .await
        .expect("count")
}

/// `user_version` and the names of all tables and indexes, through a read-only connection.
pub(crate) fn inspect(path: &std::path::Path) -> (usize, Vec<String>) {
    use rusqlite::{Connection, OpenFlags};
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open read-only");
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("user_version");
    let names = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type IN ('table', 'index') ORDER BY name")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<Vec<String>, _>>()
        .expect("names");
    (usize::try_from(version).expect("non-negative"), names)
}

/// Stores new headers with the SQL of stage 2 (no threads, no full-text index), for tests that
/// build a database of an older schema version. Ids are the sort dates (no collisions).
pub(crate) fn stage2_store_headers(
    conn: &rusqlite::Connection,
    mailbox: MailboxId,
    headers: &[MessageHeader],
) -> Result<(), crate::StoreError> {
    let account: i64 = conn.query_row(
        "SELECT account_id FROM mailbox WHERE id = ?1",
        [mailbox.0],
        |r| r.get(0),
    )?;
    for h in headers {
        let from = h.from.first();
        conn.execute(
            "INSERT INTO message (id, account_id, message_id_hdr, date, subject, from_name,
                                  from_addr, size, has_attachments)
             VALUES (?1, ?2, ?3, ?1, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                h.sort_date(),
                account,
                h.message_id,
                h.subject,
                from.and_then(|a| a.name.as_deref()),
                from.map(|a| a.email.as_str()),
                h.size,
                h.has_attachments
            ],
        )?;
        conn.execute(
            "INSERT INTO message_location (mailbox_id, message_id, uid, seen) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![mailbox.0, h.sort_date(), h.uid, h.flags.seen],
        )?;
    }
    Ok(())
}
