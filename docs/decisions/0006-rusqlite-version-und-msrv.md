# 0006 — rusqlite 0.41 statt 0.43, Mindestversion von Rust 1.90

Stand 22.11.2026, Stufe 1.

## Kontext

Die Architektur nennt rusqlite 0.43 (bundled) und `rusqlite_migration` 2.7 sowie Rust „mindestens
1.88“. Beim Aufbau des Workspaces in Stufe 1 zeigte sich:

- `rusqlite_migration` 2.7.0 (neueste Version) verlangt `rusqlite ^0.41`. Zwei rusqlite-Versionen
  lassen sich nicht nebeneinander verwenden, weil beide `sqlite3` linken (`links = "sqlite3"`).
- tauri 2.12.7 und tauri-build 2.7.6 verlangen Rust 1.90, rusqlite und `rusqlite_migration` 1.89.

## Optionen

1. rusqlite 0.41.1 mit `rusqlite_migration` 2.7.0.
2. rusqlite 0.43 und ein eigener, kleiner Migrations-Runner über `user_version` statt `rusqlite_migration`.
3. Warten, bis `rusqlite_migration` rusqlite 0.43 unterstützt.

## Entscheidung

Option 1. rusqlite 0.41.1 bündelt dasselbe SQLite (3.54.1) wie 0.43; FTS5, `contentless_delete`
und `remove_diacritics` sind vorhanden. Der Test `bundled_sqlite_has_required_features` in
`mail-store` sichert das ab. Die Messwerte aus Spike B (0003) hängen am SQLite, nicht an der
rusqlite-Version, und gelten weiter.

`rust-version` im Workspace ist 1.90. Geprüft wird mit Rust 1.100.

## Folgen

- Die Stack-Tabelle in `docs/ARCHITECTURE.md` nennt rusqlite 0.41 und Rust mindestens 1.90.
- Jede Aktualisierung von rusqlite muss mit `rusqlite_migration` zusammenpassen; `cargo deny`
  verbietet doppelte `sqlite3`-Verknüpfungen ohnehin, weil der Build sonst scheitert.

## Rückweg

Option 2: Der Migrations-Code steckt allein in `mail-store/src/migrations.rs`. Sicherungskopie
und Ablehnung zu neuer Datenbanken sind schon eigener Code; ein eigener Runner über `user_version`
wären wenige Dutzend Zeilen. Anlass zum Wechsel: eine Sicherheitsmeldung zu rusqlite 0.41 oder
eine SQLite-Funktion, die erst eine neuere rusqlite-Version freischaltet.

## Nachtrag (Stufe 2)

`mail-parser` 1.0.1 (Zeichensätze, RFC 2047, Datumsangaben im IMAP-Adapter) verlangt Rust 1.98. Die
Mindestversion des Workspace ist deshalb 1.98. Geprüft wird weiter mit Rust 1.100.
