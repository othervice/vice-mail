//! Stage 14, block D (decision 0041): the JMAP adapter's UID map ([`mail_model::UidMapStore`]).
//!
//! The adapter numbers the messages of each JMAP mailbox itself and writes every change of that
//! numbering here (tables `jmap_state`, `jmap_mailbox`, `jmap_uid`, migration 0017), so the
//! numbers survive a restart. One call is one transaction. Class "Cache": deleting the account
//! deletes its map; a lost map only costs a rebuild of each mailbox.

use std::collections::HashMap;

use mail_model::{
    AccountId, BackendError, UidMapChanges, UidMapEntry, UidMapFuture, UidMapMailbox,
    UidMapSnapshot, UidMapStore,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Store, StoreError, accounts};

fn to_backend(err: StoreError) -> BackendError {
    BackendError::InvalidState(format!("local JMAP map: {err}"))
}

fn u32_at(value: i64, what: &str) -> Result<u32, StoreError> {
    u32::try_from(value).map_err(|_| StoreError::Config(format!("{what} {value} out of range")))
}

fn u64_at(value: i64, what: &str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Config(format!("{what} {value} out of range")))
}

fn i64_of(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub(crate) fn load(
    conn: &Connection,
    account: AccountId,
) -> Result<Option<UidMapSnapshot>, StoreError> {
    let Some(row) = conn
        .prepare_cached(accounts::ACCOUNT_ROW)?
        .query_row([account.to_string()], |r| r.get::<_, i64>(0))
        .optional()?
    else {
        return Ok(None);
    };
    let Some((remote, state, modseq)) = conn
        .prepare_cached("SELECT remote, email_state, modseq FROM jmap_state WHERE account_id = ?1")?
        .query_row([row], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .optional()?
    else {
        return Ok(None);
    };
    let mut mailboxes: Vec<(UidMapMailbox, Vec<UidMapEntry>)> = Vec::new();
    let mut index = HashMap::new();
    let rows = conn
        .prepare_cached(
            "SELECT mailbox, uid_validity, uid_next, complete FROM jmap_mailbox
             WHERE account_id = ?1 ORDER BY mailbox",
        )?
        .query_map([row], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (mailbox, uid_validity, uid_next, complete) in rows {
        index.insert(mailbox.clone(), mailboxes.len());
        mailboxes.push((
            UidMapMailbox {
                mailbox,
                uid_validity: u32_at(uid_validity, "UIDVALIDITY")?,
                uid_next: u32_at(uid_next, "UIDNEXT")?,
                complete,
            },
            Vec::new(),
        ));
    }
    let entries = conn
        .prepare_cached(
            "SELECT mailbox, uid, email, modseq FROM jmap_uid
             WHERE account_id = ?1 ORDER BY mailbox, uid",
        )?
        .query_map([row], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (mailbox, uid, email, entry_modseq) in entries {
        if let Some((_, list)) = index.get(&mailbox).and_then(|i| mailboxes.get_mut(*i)) {
            list.push(UidMapEntry {
                uid: u32_at(uid, "UID")?,
                remote_id: email,
                modseq: u64_at(entry_modseq, "modseq")?,
            });
        }
    }
    Ok(Some(UidMapSnapshot {
        remote_account: remote,
        state,
        modseq: u64_at(modseq, "modseq")?,
        mailboxes,
    }))
}

pub(crate) fn save(
    conn: &Connection,
    account: AccountId,
    changes: &UidMapChanges,
) -> Result<(), StoreError> {
    let row = accounts::row_id(conn, account)?;
    if changes.reset {
        // Entries go with their mailboxes (foreign key).
        conn.prepare_cached("DELETE FROM jmap_mailbox WHERE account_id = ?1")?
            .execute([row])?;
    }
    conn.prepare_cached(
        "INSERT INTO jmap_state (account_id, remote, email_state, modseq) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (account_id) DO UPDATE SET remote = excluded.remote,
           email_state = excluded.email_state, modseq = excluded.modseq",
    )?
    .execute(params![
        row,
        changes.remote_account,
        changes.state,
        i64_of(changes.modseq)
    ])?;
    for mailbox in &changes.removed_mailboxes {
        conn.prepare_cached("DELETE FROM jmap_mailbox WHERE account_id = ?1 AND mailbox = ?2")?
            .execute(params![row, mailbox])?;
    }
    for mailbox in &changes.mailboxes {
        conn.prepare_cached(
            "INSERT INTO jmap_mailbox (account_id, mailbox, uid_validity, uid_next, complete)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (account_id, mailbox) DO UPDATE SET
               uid_validity = excluded.uid_validity, uid_next = excluded.uid_next,
               complete = excluded.complete",
        )?
        .execute(params![
            row,
            mailbox.mailbox,
            mailbox.uid_validity,
            mailbox.uid_next,
            mailbox.complete
        ])?;
    }
    for (mailbox, uid) in &changes.removed {
        conn.prepare_cached(
            "DELETE FROM jmap_uid WHERE account_id = ?1 AND mailbox = ?2 AND uid = ?3",
        )?
        .execute(params![row, mailbox, uid])?;
    }
    for (mailbox, entry) in &changes.upserts {
        conn.prepare_cached(
            "INSERT INTO jmap_uid (account_id, mailbox, uid, email, modseq)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (account_id, mailbox, uid) DO UPDATE SET
               email = excluded.email, modseq = excluded.modseq",
        )?
        .execute(params![
            row,
            mailbox,
            entry.uid,
            entry.remote_id,
            i64_of(entry.modseq)
        ])?;
    }
    Ok(())
}

impl UidMapStore for Store {
    fn load_uid_map(&self, account: AccountId) -> UidMapFuture<'_, Option<UidMapSnapshot>> {
        Box::pin(async move {
            self.read(move |conn| load(conn, account))
                .await
                .map_err(to_backend)
        })
    }

    fn save_uid_map(&self, account: AccountId, changes: UidMapChanges) -> UidMapFuture<'_, ()> {
        Box::pin(async move {
            self.write(move |tx| save(tx, account, &changes))
                .await
                .map_err(to_backend)
        })
    }
}

#[cfg(test)]
mod tests {
    use mail_model::Protocol;

    use super::*;
    use crate::StoreOptions;
    use crate::test_support::{db_path, new_account};

    fn entry(uid: u32, id: &str, modseq: u64) -> UidMapEntry {
        UidMapEntry {
            uid,
            remote_id: id.into(),
            modseq,
        }
    }

    fn mailbox(id: &str, uid_next: u32, complete: bool) -> UidMapMailbox {
        UidMapMailbox {
            mailbox: id.into(),
            uid_validity: 7,
            uid_next,
            complete,
        }
    }

    #[tokio::test]
    async fn the_map_round_trips_changes_and_goes_with_the_account() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
        let imap = store
            .insert_account(new_account(1, "bob@example.org"))
            .await
            .expect("insert");
        assert_eq!(imap.protocol, Protocol::Imap, "the default");
        let new = new_account(2, "alice@example.org");
        let account = new.id;
        let stored = store
            .insert_account_with_protocol(new, Protocol::Jmap)
            .await
            .expect("insert");
        assert_eq!(stored.protocol, Protocol::Jmap);
        let read = store
            .account(account)
            .await
            .expect("read")
            .expect("account");
        assert_eq!(read.protocol, Protocol::Jmap);
        let shown = read.into_account(mail_model::AccountStatus::connecting());
        assert_eq!(shown.protocol, Protocol::Jmap);

        assert_eq!(store.load_uid_map(account).await.expect("load"), None);
        store
            .save_uid_map(
                account,
                UidMapChanges {
                    remote_account: "u1".into(),
                    state: Some("s1".into()),
                    modseq: 2,
                    mailboxes: vec![mailbox("inbox", 3, true), mailbox("sent", 1, false)],
                    upserts: vec![
                        ("inbox".into(), entry(1, "e1", 1)),
                        ("inbox".into(), entry(2, "e2", 2)),
                    ],
                    ..UidMapChanges::default()
                },
            )
            .await
            .expect("save");
        store
            .save_uid_map(
                account,
                UidMapChanges {
                    remote_account: "u1".into(),
                    state: Some("s2".into()),
                    modseq: 3,
                    mailboxes: vec![mailbox("inbox", 4, true)],
                    removed: vec![("inbox".into(), 1)],
                    upserts: vec![
                        ("inbox".into(), entry(2, "e2", 3)),
                        ("inbox".into(), entry(3, "e3", 3)),
                    ],
                    removed_mailboxes: vec!["sent".into()],
                    ..UidMapChanges::default()
                },
            )
            .await
            .expect("save");
        let snapshot = store
            .load_uid_map(account)
            .await
            .expect("load")
            .expect("map");
        assert_eq!(snapshot.remote_account, "u1");
        assert_eq!(snapshot.state.as_deref(), Some("s2"));
        assert_eq!(snapshot.modseq, 3);
        assert_eq!(
            snapshot.mailboxes,
            vec![(
                mailbox("inbox", 4, true),
                vec![entry(2, "e2", 3), entry(3, "e3", 3)]
            )]
        );

        // A reset drops every mailbox and entry first.
        store
            .save_uid_map(
                account,
                UidMapChanges {
                    reset: true,
                    remote_account: "u2".into(),
                    state: None,
                    modseq: 4,
                    ..UidMapChanges::default()
                },
            )
            .await
            .expect("reset");
        let snapshot = store
            .load_uid_map(account)
            .await
            .expect("load")
            .expect("map");
        assert_eq!(snapshot.remote_account, "u2");
        assert!(snapshot.mailboxes.is_empty());

        assert!(store.delete_account(account).await.expect("delete"));
        assert_eq!(store.load_uid_map(account).await.expect("load"), None);
        let err = store
            .save_uid_map(account, UidMapChanges::default())
            .await
            .expect_err("no account");
        assert_eq!(err.code(), mail_model::ErrorCode::Internal);
    }
}
