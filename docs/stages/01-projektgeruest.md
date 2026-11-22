# Stufe 01 — Projektgerüst und Design-Grundlage

**Ziel:** Die App startet auf dem Mac mit der leeren Oberfläche im Zieldesign. Workspace, Datenbank
mit Migrationen, typisierte Brücke zwischen Oberfläche und Kern, Logging und der Prüfbefehl stehen.

**Voraussetzung:** Stufe 00 fertig. Lizenz und Bundle-ID sind entschieden; falls nicht, mit den
Empfehlungen aus `docs/SPEC.md` arbeiten und es unter „Wartet auf“ vermerken.

## Aufgaben

- [x] **Workspace.** `Cargo.toml` mit den Crates `mail-model`, `mail-store`, `mail-engine` (Gerüste) und `app/src-tauri`. Gemeinsame Lints: `unsafe_code = "forbid"`, clippy mit `-D warnings`.
- [x] **Frontend.** Vite, React, TypeScript strict, Tailwind 4, Radix, ESLint, Vitest. Ordner nach `docs/ARCHITECTURE.md`. Produktname nur in `tauri.conf.json` und `app/src/config/product.ts`.
- [x] **Tokens und Themen.** `app/src/styles/tokens.css` nach `docs/DESIGN.md`, hell und dunkel, Umschalter System/Hell/Dunkel, gespeichert.
- [x] **App-Rahmen.** Icon-Leiste, Listenbereich mit Kopf und leerem Zustand („Noch kein Konto“ mit Schaltfläche), Einstellungen als eigene Ansicht (Allgemein, Konten, Darstellung, Über). Fenster mit nativer Titelleiste auf macOS und Windows sauber eingebunden.
- [x] **Sprachen.** i18next mit Englisch und Deutsch. Eine Lint-Regel verbietet Textliterale in Komponenten.
- [x] **Datenbank.** `mail-store` öffnet die Datei im Datenverzeichnis der App (WAL, Fremdschlüssel an). Migrationen mit `rusqlite_migration`; erste Migration legt `setting` an. Vor jeder Migration Sicherungskopie, die letzten drei bleiben. Eine neuere Datenbank als bekannt wird mit klarer Meldung abgelehnt. Ein Schreib-Thread, Lese-Pool.
- [x] **Brücke.** Commands in `app/src-tauri/src/commands/`, die nur an `mail-engine::AppCore` durchreichen. Typisierter Fehler (`code`, `message`, `detail`). TypeScript-Typen werden mit dem in Stufe 0 gewählten Werkzeug erzeugt. `app/src/services/ipc.ts` ist die einzige Stelle mit `invoke`. Typisierter Ereignisstrom vom Kern zur Oberfläche.
- [x] **Logging.** `tracing` in eine rotierende Datei, im Entwicklungsmodus zusätzlich auf die Konsole. Typ `Secret<T>`, dessen Ausgabe geschwärzt ist.
- [x] **Sicherheits-Grundlage.** Capabilities minimal, strenge Content-Security-Policy, `withGlobalTauri` aus.
- [x] **Prüfbefehl.** `scripts/check.sh`: `cargo fmt --check`, clippy, `cargo test`, Typecheck, ESLint, Vitest, Vite-Build, `cargo deny check` (Lizenzen und Sicherheitsmeldungen), `pnpm audit --prod`.
- [x] **Desktop-Test-Grundlage** (nach Spike D, `docs/decisions/0005-html-sandbox-und-desktop-tests.md`). Cargo-Feature `e2e` mit eingebettetem WebDriver (`tauri-plugin-wdio-webdriver`), `wdio.conf`, `pnpm --dir app test:e2e` mit einem Rauchtest (App startet, Rahmen sichtbar, Bildschirmfoto hell und dunkel). Prüfung, dass ein Release-Build den WebDriver nicht enthält. Läuft nicht in `scripts/check.sh`, weil er eine GUI-Sitzung braucht.
- [x] **Dateien für das Repo.** `README.md` (Englisch: was es ist, Stand „pre-alpha“, wie man baut), `LICENSE`.

## Abnahme

- [x] `pnpm --dir app tauri dev` startet; der Rahmen ist in hell und dunkel zu sehen; der Sprachwechsel wirkt sofort
- [x] Tests: Migration von leer, zweiter Start ohne Änderung, Ablehnung einer zu neuen Datenbank, Sicherungskopie entsteht
- [x] Test: ein `Secret` erscheint weder im Log noch in einer Fehlermeldung
- [x] ein Command liefert eine typisierte Antwort, ein Ereignis kommt in der Oberfläche an (Vitest und ein Rust-Test)
- [x] `scripts/check.sh` ist grün
- [x] der Windows-Cross-Build aus Stufe 0 gelingt auch mit dem neuen Gerüst

## Nicht in dieser Stufe

- jede Mail-Funktion, Konten, Synchronisierung
- Beispiel-Mails oder Platzhalterdaten in der Oberfläche
- Command Palette, Tastaturkürzel, Layout-Varianten (Stufe 8)

## Der Mensch prüft danach

- Rahmen auf dem Mac ansehen: Stimmen Richtung, Akzentfarbe und Dichte? Änderungswünsche jetzt sind billig.

## Ergebnis

Ausgeführt am 22.11.2026 (Hauptsitzung mit Subagenten für Frontend, Rust-Kern und Desktop-Tests).

**Erledigt und geprüft (in der Hauptsitzung ausgeführt)**
- `scripts/check.sh` grün: fmt, Clippy (mit und ohne Feature `e2e`), 77 Rust-Tests, Typecheck,
  ESLint, 207 Vitest-Tests, Vite-Build, `cargo deny`, `pnpm audit --prod`.
- `pnpm --dir app test:e2e`: 15 Prüfungen grün. Nachgewiesen: Fenster öffnet ohne CSP-Verstöße
  (mit Positivkontrolle), Icon-Leiste und Listenkopf, leerer Zustand ohne erfundene Daten,
  Einstellungen mit vier Bereichen, Hell und Dunkel sofort (Hintergrundfarbe entspricht dem Token),
  Sprachwechsel sofort ohne Neuladen, Über-Seite mit Version und Plattform, gespeicherte Wahl
  übersteht einen Neustart. Bildschirmfotos hell und dunkel angesehen: sauber, nichts abgeschnitten.
- `pnpm --dir app test:release-clean`: Das Release-Binary enthält weder WebDriver noch Test-Hilfen,
  ignoriert die Test-Umgebungsvariable und öffnet keinen WebDriver-Port.
- Tests der Abnahme: `migrates_an_empty_database`, `second_start_changes_nothing`,
  `rejects_a_newer_database_and_leaves_it_untouched`, `backs_up_before_each_migration_and_keeps_three`,
  `secret_never_appears_in_the_log`, `secret_never_appears_in_error_messages`,
  `command_returns_a_typed_response` (echter Handler über `tauri::test`),
  `set_preferences_emits_core_event_to_the_ui` (Rust) und die Vitest-Tests zu `ipc` und `core-events`.
- Windows-Cross-Build mit dem neuen Gerüst: 74 s, Installer
  `target/x86_64-pc-windows-msvc/release/bundle/nsis/Vice Mail_0.1.0_x64-setup.exe` (2,84 MiB).
- Vom Subagenten geprüft, nicht von der Hauptsitzung wiederholt: `pnpm --dir app tauri dev` startet
  ohne Fehler (unter einem temporären HOME); die Release-App (`.app` 7,3 MB) startet, Log ohne Fehler.

**Entscheidungen und Abweichungen**
- rusqlite 0.41.1 statt 0.43 und Rust mindestens 1.90: `docs/decisions/0006-rusqlite-version-und-msrv.md`.
- Desktop-Test-Grundlage nach Spike D in diese Stufe vorgezogen (Aufgabe ergänzt).
- Typecheck mit TypeScript 7, ESLint braucht daneben TypeScript 6 (siehe Stack-Tabelle).
- Die deutschen Texte duzen (Annahme; bei Wunsch auf „Sie“ umstellbar, alle Texte liegen in `app/src/i18n/de.ts`).
- Die Datenbank-Datei heißt `mail.sqlite3`, Sicherungen liegen unter `backups/` daneben, Logs
  rotieren täglich (14 Dateien) im Log-Verzeichnis der App.

**Ungeprüft**
- Alles unter Windows (WebView2, CSP mit `http://ipc.localhost`, Titelleiste).
- Lässt sich die Datenbank nicht öffnen, startet der Kern trotzdem und meldet den Fehler bei jedem
  Speicherzugriff; ein blockierender Hinweis in der Oberfläche ist noch nicht gestaltet.
- Ein Fehler im `setup` der App endet auf macOS in einem Panic statt einer Meldung (Tauri führt
  `setup` dort in der Ereignisschleife aus); wird mit der Fehleranzeige beim Start in Stufe 2 behandelt.

**Wartet auf**
- Mensch: den Rahmen ansehen (`pnpm --dir app tauri dev` oder die `.app`). Stimmen Richtung,
  Akzentfarbe (Blaugrün) und Dichte? Die Ampel-Knöpfe sitzen derzeit 12 px höher als der Titel
  und ragen über die 56 px breite Leiste; Stufe 2 richtet sie am Listenkopf aus.
- Mensch: Duzen oder Siezen in der deutschen Oberfläche.
