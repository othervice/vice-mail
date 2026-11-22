//! Schema migrations, forward only. The schema version is SQLite's `user_version`, managed by
//! `rusqlite_migration`. Append new migrations at the end; never edit or reorder old ones.

use rusqlite_migration::{M, Migrations};

const MIGRATIONS: &[M<'static>] = &[M::up(include_str!("../migrations/0001_setting.sql"))];

/// All migrations this build knows.
pub(crate) fn migrations() -> Migrations<'static> {
    Migrations::from_slice(MIGRATIONS)
}

/// Newest schema version this build knows (`user_version` after all migrations).
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations()
            .validate()
            .expect("migrations apply to an empty database");
    }
}
