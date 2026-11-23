//! Schema migrations, forward only. The schema version is SQLite's `user_version`, managed by
//! `rusqlite_migration`. Append new migrations at the end; never edit or reorder old ones.

use rusqlite_migration::{M, Migrations};

pub(crate) const MIGRATIONS: &[M<'static>] = &[
    // Stage 1: key-value settings.
    M::up(include_str!("../migrations/0001_setting.sql")),
    // Stage 2: accounts, identities, mailboxes, message headers and locations.
    M::up(include_str!("../migrations/0002_mail.sql")),
    // Stage 3: body references, prefetch index, remote-content allowlist (decision 0009).
    M::up(include_str!("../migrations/0003_body.sql")),
    // Stage 4: the outbox (decision 0012).
    M::up(include_str!("../migrations/0004_operation.sql")),
    // Stage 7: full-text index, threads, account groups, hidden accounts (decision 0013).
    M::up(include_str!("../migrations/0005_search_threads.sql")),
    // Stage 5: drafts, sends in the outbox, contacts, signatures (decision 0018).
    M::up(include_str!("../migrations/0006_compose.sql")),
    // Stage 9: categories, sender rules, snoozes, pins, tasks, local rules (decision 0020).
    M::up(include_str!("../migrations/0007_inbox.sql")),
];

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
