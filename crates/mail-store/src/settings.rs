//! The `setting` table: JSON values per (scope, key).

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;

use crate::{Store, StoreError};

/// Scope of app-wide settings.
pub const GLOBAL_SCOPE: &str = "global";

/// One stored setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    /// JSON text.
    pub value: String,
    /// Starts at 1, grows by one with every change of the value.
    pub version: i64,
    /// Unix milliseconds of the last change.
    pub updated_at: i64,
}

impl Store {
    /// All settings of `scope`, ordered by key.
    pub async fn settings(&self, scope: &str) -> Result<Vec<Setting>, StoreError> {
        let scope = scope.to_owned();
        self.read(move |conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT key, value, version, updated_at FROM setting WHERE scope = ?1 ORDER BY key",
            )?;
            let rows = stmt.query_map([&scope], |r| {
                Ok(Setting {
                    key: r.get(0)?,
                    value: r.get(1)?,
                    version: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
    }

    /// Writes `entries` (key, JSON value) into `scope` in one transaction. Rows whose value does
    /// not change keep their version.
    pub async fn put_settings(
        &self,
        scope: &str,
        entries: Vec<(String, String)>,
    ) -> Result<(), StoreError> {
        let scope = scope.to_owned();
        let now = now_millis();
        self.write(move |tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO setting (scope, key, value, version, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4)
                 ON CONFLICT (scope, key) DO UPDATE
                   SET value = excluded.value,
                       version = setting.version + 1,
                       updated_at = excluded.updated_at
                   WHERE setting.value IS NOT excluded.value",
            )?;
            for (key, value) in &entries {
                stmt.execute(params![scope, key, value, now])
                    .map_err(StoreError::sqlite("write setting"))?;
            }
            Ok(())
        })
        .await
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
