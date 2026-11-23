//! Saved and recent searches (stage 7, decision 0026) as methods of [`AppCore`].
//!
//! A saved search is a name and a query; it searches all accounts and appears as a view in the
//! sidebar. It is synchronised (kind `savedView`; the store's triggers capture every change).
//! Recent searches are this device's own: the newest [`MAX_RECENT_SEARCHES`], the same query
//! again only moves up.

use mail_model::{
    CoreError, CoreEvent, MAX_RECENT_SEARCHES, MAX_SAVED_SEARCH_NAME_CHARS, RecentSearch,
    SavedSearch, SavedSearchError, SavedSearchId, clean_saved_search_name, clean_search_query,
};
use mail_store::{SavedSearchWrite, StoredRecentSearch, StoredSavedSearch};

use crate::core::{AppCore, Connector};

fn info(stored: StoredSavedSearch) -> SavedSearch {
    SavedSearch {
        id: stored.id,
        name: stored.name,
        query: stored.query,
        position: stored.position,
        updated_at: stored.updated_at,
    }
}

fn recent(stored: Vec<StoredRecentSearch>) -> Vec<RecentSearch> {
    stored
        .into_iter()
        .map(|r| RecentSearch {
            query: r.query,
            used_at: r.used_at,
        })
        .collect()
}

fn name_of(name: &str) -> Result<String, CoreError> {
    clean_saved_search_name(name).ok_or_else(|| {
        CoreError::InvalidArgument(format!(
            "a saved search needs a name of 1 to {MAX_SAVED_SEARCH_NAME_CHARS} characters \
             without control characters"
        ))
    })
}

fn query_of(query: &str) -> Result<String, CoreError> {
    clean_search_query(query).ok_or_else(|| {
        CoreError::InvalidArgument(format!(
            "a search of 1 to {} bytes without control characters",
            mail_model::search::MAX_QUERY_BYTES
        ))
    })
}

fn written(
    write: SavedSearchWrite,
    name: &str,
    id: Option<&SavedSearchId>,
) -> Result<SavedSearch, CoreError> {
    match write {
        SavedSearchWrite::Stored(stored) => Ok(info(stored)),
        SavedSearchWrite::NameTaken => Err(SavedSearchError::Exists(name.to_owned()).into()),
        SavedSearchWrite::Limit => Err(SavedSearchError::Limit.into()),
        SavedSearchWrite::Missing => Err(CoreError::NotFound(format!(
            "saved search {}",
            id.map(SavedSearchId::as_str).unwrap_or_default()
        ))),
    }
}

impl<C: Connector> AppCore<C> {
    /// Every saved search, in the sidebar's order.
    pub async fn list_saved_searches(&self) -> Result<Vec<SavedSearch>, CoreError> {
        Ok(self
            .store_ref()?
            .saved_searches()
            .await?
            .into_iter()
            .map(info)
            .collect())
    }

    /// Saves `query` under `name`, after the others. Refused before anything changes:
    /// `invalid_argument` (name or query empty, too long, control characters),
    /// `saved_search_exists` (another one has this name, without case), `saved_search_limit`.
    /// Emits `savedSearchesChanged`.
    pub async fn save_search(&self, name: String, query: String) -> Result<SavedSearch, CoreError> {
        let name = name_of(&name)?;
        let query = query_of(&query)?;
        let id = SavedSearchId::from_random(uuid::Uuid::new_v4().as_u128());
        let write = self
            .store_ref()?
            .create_saved_search(id, name.clone(), query)
            .await?;
        let saved = written(write, &name, None)?;
        tracing::info!("search saved");
        self.emit_event(CoreEvent::SavedSearchesChanged);
        Ok(saved)
    }

    /// Renames a saved search; its query and place stay. `not_found`, `invalid_argument`,
    /// `saved_search_exists`. Emits `savedSearchesChanged`.
    pub async fn rename_saved_search(
        &self,
        id: SavedSearchId,
        name: String,
    ) -> Result<SavedSearch, CoreError> {
        let name = name_of(&name)?;
        let write = self
            .store_ref()?
            .rename_saved_search(id.clone(), name.clone())
            .await?;
        let saved = written(write, &name, Some(&id))?;
        self.emit_event(CoreEvent::SavedSearchesChanged);
        Ok(saved)
    }

    /// Deletes a saved search (`not_found` if there is none); emits `savedSearchesChanged`.
    pub async fn delete_saved_search(&self, id: SavedSearchId) -> Result<(), CoreError> {
        if !self.store_ref()?.delete_saved_search(id.clone()).await? {
            return Err(CoreError::NotFound(format!("saved search {id}")));
        }
        self.emit_event(CoreEvent::SavedSearchesChanged);
        Ok(())
    }

    /// This device's recent searches, newest first (at most [`MAX_RECENT_SEARCHES`]).
    pub async fn list_recent_searches(&self) -> Result<Vec<RecentSearch>, CoreError> {
        Ok(recent(
            self.store_ref()?
                .recent_searches(MAX_RECENT_SEARCHES)
                .await?,
        ))
    }

    /// Puts a search on top of this device's history (trimmed; the same one again moves up)
    /// and returns the history. `invalid_argument` for an empty or too long query.
    pub async fn add_recent_search(&self, query: String) -> Result<Vec<RecentSearch>, CoreError> {
        let query = query_of(&query)?;
        Ok(recent(
            self.store_ref()?
                .add_recent_search(query, MAX_RECENT_SEARCHES)
                .await?,
        ))
    }

    /// Forgets this device's recent searches.
    pub async fn clear_recent_searches(&self) -> Result<(), CoreError> {
        Ok(self.store_ref()?.clear_recent_searches().await?)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use mail_model::{
        CoreEvent, ErrorCode, MAX_RECENT_SEARCHES, MAX_SAVED_SEARCHES, SavedSearchId,
    };
    use tempfile::TempDir;
    use tokio::sync::broadcast;

    use crate::credentials::{Credentials, MemorySecretStore};
    use crate::{AppCore, BuildInfo, CoreConfig, CoreServices};

    async fn core() -> (TempDir, AppCore) {
        let dir = TempDir::new().expect("temp dir");
        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0.0.0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            CoreServices::new(Credentials::new(MemorySecretStore::new())),
        )
        .await;
        (dir, core)
    }

    async fn changed(events: &mut broadcast::Receiver<CoreEvent>) -> bool {
        loop {
            match tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
                Ok(Ok(CoreEvent::SavedSearchesChanged)) => return true,
                Ok(Ok(_)) => {}
                _ => return false,
            }
        }
    }

    #[tokio::test]
    async fn searches_are_saved_renamed_and_deleted_with_typed_errors() {
        let (_dir, core) = core().await;
        let mut events = core.subscribe();
        assert!(core.list_saved_searches().await.expect("list").is_empty());

        let unread = core
            .save_search("  Ungelesen \n".into(), "  is:unread  ".into())
            .await
            .expect("save");
        assert_eq!(unread.name, "Ungelesen");
        assert_eq!(unread.query, "is:unread");
        assert!(changed(&mut events).await, "savedSearchesChanged");
        let bills = core
            .save_search("Rechnungen 🧾".into(), "rechnung has:attachment".into())
            .await
            .expect("save");
        assert!(bills.position > unread.position);
        assert!(changed(&mut events).await);

        // Refused before anything changes, each with its own code.
        for (name, query, code) in [
            ("", "x", ErrorCode::InvalidArgument),
            ("x", "   ", ErrorCode::InvalidArgument),
            ("a\u{0}b", "x", ErrorCode::InvalidArgument),
            ("x", &"y".repeat(2049), ErrorCode::InvalidArgument),
            (&"n".repeat(101), "x", ErrorCode::InvalidArgument),
            ("UNGELESEN", "is:unread", ErrorCode::SavedSearchExists),
        ] {
            let err = core
                .save_search(name.to_owned(), query.to_owned())
                .await
                .expect_err("refused");
            assert_eq!(err.code(), code, "{name:?} {query:?}");
        }
        assert!(!changed(&mut events).await, "no event for a refused save");
        assert_eq!(core.list_saved_searches().await.expect("list").len(), 2);

        let renamed = core
            .rename_saved_search(bills.id.clone(), "Belege".into())
            .await
            .expect("rename");
        assert_eq!(renamed.query, "rechnung has:attachment");
        assert!(changed(&mut events).await);
        let err = core
            .rename_saved_search(bills.id.clone(), "ungelesen".into())
            .await
            .expect_err("taken");
        assert_eq!(err.code(), ErrorCode::SavedSearchExists);
        let unknown = SavedSearchId::from_random(42);
        assert_eq!(
            core.rename_saved_search(unknown.clone(), "x".into())
                .await
                .expect_err("unknown")
                .code(),
            ErrorCode::NotFound
        );
        assert_eq!(
            core.delete_saved_search(unknown)
                .await
                .expect_err("unknown")
                .code(),
            ErrorCode::NotFound
        );

        let names: Vec<String> = core
            .list_saved_searches()
            .await
            .expect("list")
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(names, ["Ungelesen", "Belege"]);
        core.delete_saved_search(unread.id).await.expect("delete");
        assert!(changed(&mut events).await);
        assert_eq!(core.list_saved_searches().await.expect("list").len(), 1);

        // The limit.
        for n in 1..MAX_SAVED_SEARCHES {
            core.save_search(format!("View {n}"), format!("from:x{n}@example.org"))
                .await
                .expect("save");
        }
        let err = core
            .save_search("One more".into(), "x".into())
            .await
            .expect_err("limit");
        assert_eq!(err.code(), ErrorCode::SavedSearchLimit);
        core.shutdown().await;
    }

    #[tokio::test]
    async fn recent_searches_stay_few_newest_first_and_survive_a_restart() {
        let (dir, core) = core().await;
        assert!(core.list_recent_searches().await.expect("list").is_empty());
        for n in 0..MAX_RECENT_SEARCHES + 3 {
            core.add_recent_search(format!("query {n}"))
                .await
                .expect("add");
        }
        let again = core
            .add_recent_search("  query 5 ".into())
            .await
            .expect("again");
        assert_eq!(again.len(), MAX_RECENT_SEARCHES);
        assert_eq!(again[0].query, "query 5");
        assert_eq!(again[1].query, format!("query {}", MAX_RECENT_SEARCHES + 2));
        assert_eq!(
            again.iter().filter(|r| r.query == "query 5").count(),
            1,
            "no duplicates"
        );
        assert!(
            !again.iter().any(|r| r.query == "query 0"),
            "the oldest went"
        );
        assert_eq!(
            core.add_recent_search(" ".into())
                .await
                .expect_err("empty")
                .code(),
            ErrorCode::InvalidArgument
        );
        core.shutdown().await;
        drop(core);

        let core = AppCore::start(
            CoreConfig {
                data_dir: dir.path().join("data"),
                build: BuildInfo {
                    version: "0.0.0".into(),
                    commit: None,
                    build_date: None,
                },
            },
            CoreServices::new(Credentials::new(MemorySecretStore::new())),
        )
        .await;
        let kept = core.list_recent_searches().await.expect("list");
        assert_eq!(kept, again, "kept on the device");
        core.clear_recent_searches().await.expect("clear");
        assert!(core.list_recent_searches().await.expect("list").is_empty());
        core.shutdown().await;
    }
}
