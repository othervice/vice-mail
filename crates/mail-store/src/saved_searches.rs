//! Saved searches (class sync, kind `savedView`) and this device's recent searches (stage 7,
//! decision 0026). The engine cleans names and queries; the store checks the limit and the
//! uniqueness of names in the writing transaction, so two quick saves cannot both pass.

use mail_model::{MAX_SAVED_SEARCHES, SavedSearchId};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::settings::now_millis;
use crate::{Store, StoreError};

/// A saved search as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSavedSearch {
    pub id: SavedSearchId,
    pub name: String,
    pub query: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// What a create or rename did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedSearchWrite {
    Stored(StoredSavedSearch),
    /// Another saved search has this name (without case); nothing changed.
    NameTaken,
    /// [`MAX_SAVED_SEARCHES`] exist already; nothing changed.
    Limit,
    /// No saved search has this id (rename).
    Missing,
}

/// A search of this device's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecentSearch {
    pub query: String,
    pub used_at: i64,
}

const COLUMNS: &str = "SELECT uuid, name, query, position, created_at, updated_at FROM saved_view";

fn from_row(r: &Row<'_>) -> rusqlite::Result<StoredSavedSearch> {
    let text: String = r.get(0)?;
    let id = SavedSearchId::parse(&text).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("not a UUID: {text:?}").into(),
        )
    })?;
    Ok(StoredSavedSearch {
        id,
        name: r.get(1)?,
        query: r.get(2)?,
        position: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

pub(crate) fn saved_searches(conn: &Connection) -> Result<Vec<StoredSavedSearch>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("{COLUMNS} ORDER BY position, id"))?
        .query_map([], from_row)?
        .collect::<Result<Vec<_>, _>>()?)
}

pub(crate) fn saved_search(
    conn: &Connection,
    id: &SavedSearchId,
) -> Result<Option<StoredSavedSearch>, StoreError> {
    Ok(conn
        .prepare_cached(&format!("{COLUMNS} WHERE uuid = ?1"))?
        .query_row([id.as_str()], from_row)
        .optional()?)
}

/// Whether a saved search other than `except` has this name, compared without case (Unicode
/// lower case, which SQLite's `lower()` does not do beyond ASCII).
fn name_taken(
    conn: &Connection,
    name: &str,
    except: Option<&SavedSearchId>,
) -> Result<bool, StoreError> {
    let wanted = name.to_lowercase();
    Ok(saved_searches(conn)?
        .iter()
        .any(|s| Some(&s.id) != except && s.name.to_lowercase() == wanted))
}

fn create(
    conn: &Connection,
    id: &SavedSearchId,
    name: &str,
    query: &str,
    now: i64,
) -> Result<SavedSearchWrite, StoreError> {
    let count: i64 = conn
        .prepare_cached("SELECT count(*) FROM saved_view")?
        .query_row([], |r| r.get(0))?;
    if usize::try_from(count).unwrap_or(usize::MAX) >= MAX_SAVED_SEARCHES {
        return Ok(SavedSearchWrite::Limit);
    }
    if name_taken(conn, name, None)? {
        return Ok(SavedSearchWrite::NameTaken);
    }
    conn.prepare_cached(
        "INSERT INTO saved_view (uuid, name, query, position, created_at, updated_at)
         VALUES (?1, ?2, ?3, (SELECT coalesce(max(position), -1) + 1 FROM saved_view), ?4, ?4)",
    )?
    .execute(params![id.as_str(), name, query, now])
    .map_err(StoreError::sqlite("store saved search"))?;
    saved_search(conn, id)?
        .map(SavedSearchWrite::Stored)
        .ok_or_else(|| StoreError::NotFound(format!("saved search {id}")))
}

fn rename(
    conn: &Connection,
    id: &SavedSearchId,
    name: &str,
    now: i64,
) -> Result<SavedSearchWrite, StoreError> {
    let Some(existing) = saved_search(conn, id)? else {
        return Ok(SavedSearchWrite::Missing);
    };
    if existing.name == name {
        return Ok(SavedSearchWrite::Stored(existing));
    }
    if name_taken(conn, name, Some(id))? {
        return Ok(SavedSearchWrite::NameTaken);
    }
    conn.prepare_cached("UPDATE saved_view SET name = ?2, updated_at = ?3 WHERE uuid = ?1")?
        .execute(params![id.as_str(), name, now])?;
    saved_search(conn, id)?
        .map(SavedSearchWrite::Stored)
        .ok_or_else(|| StoreError::NotFound(format!("saved search {id}")))
}

fn recent_searches(conn: &Connection, limit: usize) -> Result<Vec<StoredRecentSearch>, StoreError> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    Ok(conn
        .prepare_cached(
            "SELECT query, used_at FROM recent_search ORDER BY used_at DESC, query LIMIT ?1",
        )?
        .query_map([limit], |r| {
            Ok(StoredRecentSearch {
                query: r.get(0)?,
                used_at: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn add_recent(
    conn: &Connection,
    query: &str,
    keep: usize,
    now: i64,
) -> Result<Vec<StoredRecentSearch>, StoreError> {
    // Never older than the newest entry, so a clock that went back still puts it on top.
    conn.prepare_cached(
        "INSERT INTO recent_search (query, used_at)
         VALUES (?1, max(?2, coalesce((SELECT max(used_at) + 1 FROM recent_search), ?2)))
         ON CONFLICT (query) DO UPDATE SET used_at = excluded.used_at",
    )?
    .execute(params![query, now])?;
    let kept = recent_searches(conn, keep)?;
    if let Some(oldest) = kept.last() {
        conn.prepare_cached(
            "DELETE FROM recent_search
             WHERE used_at < ?1 OR (used_at = ?1 AND query > ?2)",
        )?
        .execute(params![oldest.used_at, oldest.query])?;
    }
    Ok(kept)
}

impl Store {
    /// Every saved search, in the sidebar's order.
    pub async fn saved_searches(&self) -> Result<Vec<StoredSavedSearch>, StoreError> {
        self.read(saved_searches).await
    }

    pub async fn saved_search(
        &self,
        id: SavedSearchId,
    ) -> Result<Option<StoredSavedSearch>, StoreError> {
        self.read(move |conn| saved_search(conn, &id)).await
    }

    /// Adds a saved search at the end (name and query cleaned by the engine).
    pub async fn create_saved_search(
        &self,
        id: SavedSearchId,
        name: String,
        query: String,
    ) -> Result<SavedSearchWrite, StoreError> {
        self.write(move |tx| create(tx, &id, &name, &query, now_millis()))
            .await
    }

    pub async fn rename_saved_search(
        &self,
        id: SavedSearchId,
        name: String,
    ) -> Result<SavedSearchWrite, StoreError> {
        self.write(move |tx| rename(tx, &id, &name, now_millis()))
            .await
    }

    /// `false` if there was none.
    pub async fn delete_saved_search(&self, id: SavedSearchId) -> Result<bool, StoreError> {
        self.write(move |tx| {
            Ok(tx
                .prepare_cached("DELETE FROM saved_view WHERE uuid = ?1")?
                .execute([id.as_str()])?
                > 0)
        })
        .await
    }

    /// The newest `limit` recent searches, newest first.
    pub async fn recent_searches(
        &self,
        limit: usize,
    ) -> Result<Vec<StoredRecentSearch>, StoreError> {
        self.read(move |conn| recent_searches(conn, limit)).await
    }

    /// Puts `query` on top of the recent searches and keeps the newest `keep`; returns them.
    pub async fn add_recent_search(
        &self,
        query: String,
        keep: usize,
    ) -> Result<Vec<StoredRecentSearch>, StoreError> {
        self.write(move |tx| add_recent(tx, &query, keep, now_millis()))
            .await
    }

    pub async fn clear_recent_searches(&self) -> Result<(), StoreError> {
        self.write(|tx| {
            tx.prepare_cached("DELETE FROM recent_search")?
                .execute([])?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use serde_json::{Value, json};

    use super::*;
    use crate::test_support::db_path;
    use crate::{
        CaptureOutcome, CapturedChange, StoreOptions, SyncApply, SyncCapture, SyncRecordRow,
        TableWrite,
    };

    fn id(n: u128) -> SavedSearchId {
        SavedSearchId::from_random(n)
    }

    async fn open_temp() -> (tempfile::TempDir, Store) {
        let dir = tempfile::TempDir::new().expect("dir");
        let store = Store::open(&db_path(&dir), StoreOptions::default()).expect("open");
        (dir, store)
    }

    /// Versions every change with a counter; a gone row becomes a tombstone.
    #[derive(Default)]
    struct Recorder(Mutex<u64>);

    impl SyncCapture for Recorder {
        fn capture(&self, changes: Vec<CapturedChange>) -> CaptureOutcome {
            let mut counter = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            let mut rows = Vec::new();
            for change in changes {
                if change.current.is_none() && change.stored.is_none() {
                    continue;
                }
                *counter += 1;
                rows.push(SyncRecordRow {
                    deleted_version: change
                        .current
                        .is_none()
                        .then(|| change.stored.as_ref().map(|s| s.version.clone()))
                        .flatten(),
                    kind: change.kind,
                    id: change.id,
                    version: format!("v{:04}", *counter),
                    synced_version: None,
                    content: change.current,
                });
            }
            CaptureOutcome {
                rows,
                hlc_last: None,
            }
        }
    }

    #[tokio::test]
    async fn saved_searches_are_captured_and_applied_recent_ones_are_not() {
        let (_dir, store) = open_temp().await;
        store.set_sync_capture(Arc::new(Recorder::default()) as Arc<dyn SyncCapture>);
        store
            .create_saved_search(id(1), "Bob".into(), "from:bob".into())
            .await
            .expect("create");
        let record = store
            .sync_record("savedView", id(1).as_str())
            .await
            .expect("read")
            .expect("captured");
        assert_eq!(
            record.content,
            Some(json!({ "name": "Bob", "query": "from:bob", "position": 0 }))
        );
        store
            .rename_saved_search(id(1), "Bob B.".into())
            .await
            .expect("rename");
        let renamed = store
            .sync_record("savedView", id(1).as_str())
            .await
            .expect("read")
            .expect("row");
        assert_eq!(
            renamed.content.as_ref().map(|c| c["name"].clone()),
            Some(json!("Bob B."))
        );
        assert_ne!(renamed.version, record.version);
        // Recent searches stay on the device: no record at all.
        store
            .add_recent_search("from:alice".into(), 10)
            .await
            .expect("recent");
        assert!(
            store
                .sync_records()
                .await
                .expect("rows")
                .iter()
                .all(|r| r.kind == "savedView"),
            "only saved searches are records"
        );
        assert!(store.delete_saved_search(id(1)).await.expect("delete"));
        let tombstone = store
            .sync_record("savedView", id(1).as_str())
            .await
            .expect("read")
            .expect("tombstone");
        assert!(tombstone.content.is_none() && tombstone.deleted_version.is_some());

        // From another device: written into the table (without a uniqueness check), then gone.
        let other = id(2);
        let row = |value: Option<Value>| SyncRecordRow {
            kind: "savedView".into(),
            id: other.as_str().into(),
            version: "1764000000000-00000000-x".into(),
            synced_version: Some("1764000000000-00000000-x".into()),
            content: value,
            deleted_version: None,
        };
        let value =
            json!({ "name": "Rechnungen", "query": "rechnung has:attachment", "position": 4 });
        let outcome = store
            .apply_sync(
                vec![SyncApply {
                    kind: "savedView".into(),
                    id: other.as_str().into(),
                    expected: None,
                    row: Some(row(Some(value.clone()))),
                    table: TableWrite::Put(value),
                    rekey_from: None,
                }],
                None,
            )
            .await
            .expect("apply");
        assert_eq!(outcome.applied.len(), 1, "{outcome:?}");
        assert!(outcome.kinds.contains("savedView"));
        let applied = store
            .saved_search(other.clone())
            .await
            .expect("read")
            .expect("applied");
        assert_eq!(
            (
                applied.name.as_str(),
                applied.query.as_str(),
                applied.position
            ),
            ("Rechnungen", "rechnung has:attachment", 4)
        );
        assert_eq!(
            store
                .sync_record("savedView", other.as_str())
                .await
                .expect("read")
                .expect("row")
                .version,
            "1764000000000-00000000-x",
            "applying is not a local change"
        );
        let expected = Some("1764000000000-00000000-x".to_owned());
        store
            .apply_sync(
                vec![SyncApply {
                    kind: "savedView".into(),
                    id: other.as_str().into(),
                    expected,
                    row: None,
                    table: TableWrite::Delete,
                    rekey_from: None,
                }],
                None,
            )
            .await
            .expect("apply delete");
        assert!(store.saved_search(other).await.expect("read").is_none());
    }

    #[tokio::test]
    async fn saved_searches_keep_their_order_names_and_limit() {
        let (_dir, store) = open_temp().await;
        let first = store
            .create_saved_search(id(1), "Ungelesen".into(), "is:unread".into())
            .await
            .expect("create");
        let SavedSearchWrite::Stored(first) = first else {
            panic!("not stored: {first:?}");
        };
        assert_eq!(first.position, 0);
        let second = store
            .create_saved_search(id(2), "Rechnungen".into(), "has:attachment rechnung".into())
            .await
            .expect("create");
        assert!(matches!(second, SavedSearchWrite::Stored(ref s) if s.position == 1));
        // Names are unique without case, also beyond ASCII.
        assert_eq!(
            store
                .create_saved_search(id(3), "UNGELESEN".into(), "x".into())
                .await
                .expect("create"),
            SavedSearchWrite::NameTaken
        );
        store
            .create_saved_search(id(4), "Äpfel".into(), "äpfel".into())
            .await
            .expect("create");
        assert_eq!(
            store
                .rename_saved_search(id(2), "äPFEL".into())
                .await
                .expect("rename"),
            SavedSearchWrite::NameTaken
        );
        // Renaming to its own name (another case) is fine; the query and position stay.
        let renamed = store
            .rename_saved_search(id(1), "UNGELESEN".into())
            .await
            .expect("rename");
        let SavedSearchWrite::Stored(renamed) = renamed else {
            panic!("not renamed: {renamed:?}");
        };
        assert_eq!(renamed.name, "UNGELESEN");
        assert_eq!(renamed.query, "is:unread");
        assert_eq!(renamed.position, 0);
        assert_eq!(
            store
                .rename_saved_search(id(9), "x".into())
                .await
                .expect("rename"),
            SavedSearchWrite::Missing
        );
        let names: Vec<String> = store
            .saved_searches()
            .await
            .expect("list")
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["UNGELESEN", "Rechnungen", "Äpfel"]);
        assert!(store.delete_saved_search(id(2)).await.expect("delete"));
        assert!(!store.delete_saved_search(id(2)).await.expect("again"));

        for n in 0..MAX_SAVED_SEARCHES - 2 {
            let n32 = u128::try_from(n).expect("n");
            assert!(matches!(
                store
                    .create_saved_search(id(100 + n32), format!("View {n}"), "x".into())
                    .await
                    .expect("create"),
                SavedSearchWrite::Stored(_)
            ));
        }
        assert_eq!(
            store
                .create_saved_search(id(99), "One more".into(), "x".into())
                .await
                .expect("create"),
            SavedSearchWrite::Limit
        );
        assert_eq!(
            store.saved_searches().await.expect("list").len(),
            MAX_SAVED_SEARCHES
        );
    }

    #[tokio::test]
    async fn recent_searches_move_up_and_keep_the_newest() {
        let (_dir, store) = open_temp().await;
        for query in ["a", "b", "c"] {
            store.add_recent_search(query.into(), 3).await.expect("add");
        }
        // The same query again moves to the top, no duplicate.
        let kept = store.add_recent_search("a".into(), 3).await.expect("add");
        let queries: Vec<&str> = kept.iter().map(|r| r.query.as_str()).collect();
        assert_eq!(queries, ["a", "c", "b"]);
        // A fourth drops the oldest, also from the table.
        store.add_recent_search("d".into(), 3).await.expect("add");
        let all = store.recent_searches(100).await.expect("all");
        let queries: Vec<&str> = all.iter().map(|r| r.query.as_str()).collect();
        assert_eq!(queries, ["d", "a", "c"]);
        assert!(all.windows(2).all(|w| w[0].used_at > w[1].used_at));
        store.clear_recent_searches().await.expect("clear");
        assert!(store.recent_searches(100).await.expect("empty").is_empty());
    }
}
