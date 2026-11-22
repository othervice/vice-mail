# Stufe 01 — Projektgerüst und Design-Grundlage

**Ziel:** Die App startet auf dem Mac mit der leeren Oberfläche im Zieldesign. Workspace, Datenbank
mit Migrationen, typisierte Brücke zwischen Oberfläche und Kern, Logging und der Prüfbefehl stehen.

**Voraussetzung:** Stufe 00 fertig. Lizenz und Bundle-ID sind entschieden; falls nicht, mit den
Empfehlungen aus `docs/SPEC.md` arbeiten und es unter „Wartet auf“ vermerken.

## Aufgaben

- [ ] **Workspace.** `Cargo.toml` mit den Crates `mail-model`, `mail-store`, `mail-engine` (Gerüste) und `app/src-tauri`. Gemeinsame Lints: `unsafe_code = "forbid"`, clippy mit `-D warnings`.
- [ ] **Frontend.** Vite, React, TypeScript strict, Tailwind 4, Radix, ESLint, Vitest. Ordner nach `docs/ARCHITECTURE.md`. Produktname nur in `tauri.conf.json` und `app/src/config/product.ts`.
- [ ] **Tokens und Themen.** `app/src/styles/tokens.css` nach `docs/DESIGN.md`, hell und dunkel, Umschalter System/Hell/Dunkel, gespeichert.
- [ ] **App-Rahmen.** Icon-Leiste, Listenbereich mit Kopf und leerem Zustand („Noch kein Konto“ mit Schaltfläche), Einstellungen als eigene Ansicht (Allgemein, Konten, Darstellung, Über). Fenster mit nativer Titelleiste auf macOS und Windows sauber eingebunden.
- [ ] **Sprachen.** i18next mit Englisch und Deutsch. Eine Lint-Regel verbietet Textliterale in Komponenten.
- [ ] **Datenbank.** `mail-store` öffnet die Datei im Datenverzeichnis der App (WAL, Fremdschlüssel an). Migrationen mit `rusqlite_migration`; erste Migration legt `setting` an. Vor jeder Migration Sicherungskopie, die letzten drei bleiben. Eine neuere Datenbank als bekannt wird mit klarer Meldung abgelehnt. Ein Schreib-Thread, Lese-Pool.
- [ ] **Brücke.** Commands in `app/src-tauri/src/commands/`, die nur an `mail-engine::AppCore` durchreichen. Typisierter Fehler (`code`, `message`, `detail`). TypeScript-Typen werden mit dem in Stufe 0 gewählten Werkzeug erzeugt. `app/src/services/ipc.ts` ist die einzige Stelle mit `invoke`. Typisierter Ereignisstrom vom Kern zur Oberfläche.
- [ ] **Logging.** `tracing` in eine rotierende Datei, im Entwicklungsmodus zusätzlich auf die Konsole. Typ `Secret<T>`, dessen Ausgabe geschwärzt ist.
- [ ] **Sicherheits-Grundlage.** Capabilities minimal, strenge Content-Security-Policy, `withGlobalTauri` aus.
- [ ] **Prüfbefehl.** `scripts/check.sh`: `cargo fmt --check`, clippy, `cargo test`, Typecheck, ESLint, Vitest, Vite-Build, `cargo deny check` (Lizenzen und Sicherheitsmeldungen), `pnpm audit --prod`.
- [ ] **Dateien für das Repo.** `README.md` (Englisch: was es ist, Stand „pre-alpha“, wie man baut), `LICENSE`.

## Abnahme

- [ ] `pnpm --dir app tauri dev` startet; der Rahmen ist in hell und dunkel zu sehen; der Sprachwechsel wirkt sofort
- [ ] Tests: Migration von leer, zweiter Start ohne Änderung, Ablehnung einer zu neuen Datenbank, Sicherungskopie entsteht
- [ ] Test: ein `Secret` erscheint weder im Log noch in einer Fehlermeldung
- [ ] ein Command liefert eine typisierte Antwort, ein Ereignis kommt in der Oberfläche an (Vitest und ein Rust-Test)
- [ ] `scripts/check.sh` ist grün
- [ ] der Windows-Cross-Build aus Stufe 0 gelingt auch mit dem neuen Gerüst

## Nicht in dieser Stufe

- jede Mail-Funktion, Konten, Synchronisierung
- Beispiel-Mails oder Platzhalterdaten in der Oberfläche
- Command Palette, Tastaturkürzel, Layout-Varianten (Stufe 8)

## Der Mensch prüft danach

- Rahmen auf dem Mac ansehen: Stimmen Richtung, Akzentfarbe und Dichte? Änderungswünsche jetzt sind billig.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
