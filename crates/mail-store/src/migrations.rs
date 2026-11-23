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
    // Stage 12: AI access over MCP: switch, shared accounts, clients, access log (decision 0021).
    M::up(include_str!("../migrations/0008_mcp.sql")),
    // Stage 11: settings sync records, change capture, device state (decisions 0014, 0022).
    M::up(include_str!("../migrations/0009_settings_sync.sql")),
    // Stage 10: several HTML signatures, templates, default signatures per identity, merged
    // contacts and dismissed duplicate pairs (decision 0023).
    M::up(include_str!("../migrations/0010_composer.sql")),
    // Stage 13: AI request log and dismissed open-reply hints (decision 0024).
    M::up(include_str!("../migrations/0011_ai.sql")),
    // Stage 7: saved searches (synced) and recent searches (device) (decision 0026).
    M::up(include_str!("../migrations/0012_saved_searches.sql")),
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
