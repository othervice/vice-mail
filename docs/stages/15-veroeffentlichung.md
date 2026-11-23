# Stufe 15 — Veröffentlichung

**Ziel:** Das Projekt ist öffentlich, andere können es bauen und installieren, Updates kommen
sicher an, und eine Sicherheitsprüfung ist gelaufen.

**Voraussetzung:** Mindestens Stufen 00 bis 11 fertig, dazu 14 und 16 (0032). Entschieden (0035):
Open Source unter GPL-3.0-or-later, öffentliches Repo unter `github.com/othervice`; das Repo legt der
Mensch an oder gibt dafür einen Zugang frei (das `gh`-Konto `othervice` ist angemeldet). Der Name
bleibt „Vice Mail“ mit eigenem Logo. Die ersten Releases erscheinen als Privatperson **ohne
Signatur**; Signieren lässt sich später nachrüsten (0035).

## Aufgaben

- [ ] **Repo aufräumen.** Geschichte auf Geheimnisse und fremde Dateien prüfen (insbesondere `design-reference/` darf nie eingecheckt worden sein). Plan-Dokumente ins Englische übertragen oder als internes Archiv kennzeichnen.
- [ ] **Dokumente.** `README.md` mit Bildschirmfotos, `docs/INSTALL.md`, `CONTRIBUTING.md`, `SECURITY.md` (wie man Lücken meldet), `CHANGELOG.md`, `docs/PROVIDERS.md`, `docs/SYNC.md`.
- [ ] **Lizenzen.** Liste aller Bibliotheken mit Lizenz erzeugen und der App beilegen (Ansicht „Über“). `cargo deny` und eine Prüfung der npm-Pakete laufen im Prüfbefehl.
- [ ] **Datenschutz.** Kurze, ehrliche Erklärung: welche Verbindungen die App aufbaut (Liste aus der Architektur), dass es keine Telemetrie gibt, was lokal unverschlüsselt liegt.
- [ ] **CI.** Gitea Actions mit Runner auf dem Mac mini, und nach dem Umzug GitHub Actions mit gehosteten macOS- und Windows-Runnern. Beide rufen `scripts/check.sh` auf und bauen die Installer. Damit laufen Tests erstmals regelmäßig auf echtem Windows.
- [ ] **Signierung.** Erste Releases ohne Signatur (0035): die Warnungen von Gatekeeper und SmartScreen in `docs/INSTALL.md` und im Release-Text ehrlich beschreiben. Später mit Zertifikaten: macOS signieren und notarisieren, Windows signieren.
  - *macOS vorbereitet am 23.11.2026 (0035, Nachtrag; noch nicht abgehakt):* Apple-Developer-Mitgliedschaft als Einzelperson beantragt, Aktivierung ausstehend. `scripts/apple-signing-setup.sh` (Schlüssel und CSR, Zertifikat importieren, notarytool-Profil `vicemail-notary`, `--check`); `scripts/build-macos.sh` signiert mit einer gefundenen Identität „Developer ID Application“ (Hardened Runtime, Zeitstempel, `mail-mcp` zuerst, leere `app/src-tauri/Entitlements.plist` mit Begründung), notarisiert App und Disk-Image, stapelt, prüft mit `codesign`, `spctl` und `stapler`, baut das Update-Archiv aus der gestapelten App und bricht ab, statt ad hoc weiterzumachen; ohne Identität bleibt der Ad-hoc-Build unverändert. Ablauf nach der Aktivierung, Erneuerung und Sicherung in `docs/RELEASING.md` („macOS signing and notarisation“), `docs/INSTALL.md` beschreibt signierte und unsignierte Builds. Getestet mit einem selbst signierten Testzertifikat (siehe Ergebnis).
  - *Offen:* Aktivierung, die Portal-Schritte des Menschen, der erste notarisierte Build und sein Test auf einem zweiten Mac. Windows bleibt unsigniert (0035).
- [ ] **Updates.** Tauri-Updater mit Signaturprüfung. Der private Schlüssel liegt im Schlüsselbund des Build-Macs (0037; mit CI später als CI-Geheimnis, nie im Repo). Update-Beschreibung als statische Datei bei den Releases. Prüfung nur mit Zustimmung (0037: Standard aus). Rückweg: die vorherige Version bleibt herunterladbar, und die Datenbank-Sicherungen aus Stufe 1 erlauben das Zurückgehen.
  - *Vorbereitet am 23.11.2026 (0037, noch nicht abgehakt):* `tauri-plugin-updater` =2.14.1 nur aus Rust; Schalter „Automatisch nach Updates suchen“ (aus), einmalige Frage nach dem ersten Konto, Tagesdrossel, „Jetzt suchen“, Download und „Neu starten und installieren“ nur nach Klick, Windows `passive`; Endpunkt `https://github.com/othervice/vice-mail/releases/latest/download/latest.json` in `tauri.conf.json`; Schlüssel `org.vicemail.updater` im Schlüsselbund erzeugt, öffentlicher Schlüssel in `tauri.conf.json`; `VICEMAIL_UPDATER=1` für `scripts/build-macos.sh`, `build-windows.sh`, `release-local.sh` (signierte Artefakte, Signaturprüfung, `latest.json` über `scripts/updater-manifest.mjs`); Ablauf, Sicherung und Wechsel des Schlüssels in `docs/RELEASING.md`. Getestet gegen einen lokalen Update-Server (Rust) und in Vitest; der Desktop-Test `app/tests/e2e/updates.e2e.mjs` lief im Desktop-Lauf auf a7dd759 mit 4 von 4 grün (siehe Ergebnis).
  - *Offen bis zur Veröffentlichung:* erstes Release mit `latest.json` auf GitHub, dann Update von einer Version auf die nächste auf echtem macOS und Windows (Abnahmepunkt 2). **Der Mensch sichert den privaten Schlüssel und sein Passwort** (`docs/RELEASING.md`, „Back it up now“).
- [ ] **Sicherheitsprüfung.** `/security-review` über den gesamten Code. Bedrohungsmodell als `docs/SECURITY_MODEL.md`: was geschützt ist, was nicht, welche Grenzen es gibt. Zeitlich begrenztes Fuzzing von MIME-Parser-Anbindung und HTML-Pipeline. Befunde beheben oder begründet offen lassen.
- [ ] **Erster Start.** Geführte Einrichtung für neue Nutzer: Konto, Thema, optional Sync und KI.
- [x] **Diagnose.** „Diagnose exportieren“ packt Logs und Systemangaben in eine Datei; ein Test stellt sicher, dass keine Geheimnisse und keine Mail-Inhalte enthalten sind.
  - *Umgesetzt am 23.11.2026:* Einstellungen → Über → „Diagnose exportieren …“: Speichern-Dialog des Systems (Downloads, `<produkt>-diagnostics-JJJJ-MM-TT.txt`), dann sammelt, schwärzt und schreibt der Kern (`crates/mail-engine/src/diagnostics/`) eine reine Textdatei. Inhalt, Ausschlüsse und Schwärzung in `docs/PRIVACY.md` („Diagnostics“); Prüfung siehe Ergebnis.

## Abnahme

- [ ] Installation aus den veröffentlichten Dateien auf einem frischen Mac und einem frischen Windows
- [ ] Update von einer Version auf die nächste gelingt; eine manipulierte Update-Datei wird abgelehnt
- [ ] Bau aus dem öffentlichen Repo nach `README.md` auf einem Rechner, der das Projekt nie gesehen hat
- [ ] CI ist grün auf macOS und Windows
- [ ] Lizenzliste vollständig; keine unverträgliche Lizenz
- [ ] alle Befunde der Sicherheitsprüfung behoben oder mit Begründung dokumentiert
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- App Stores
- Bezahlfunktionen, Telemetrie

## Der Mensch prüft danach

- Eine unabhängige Sicherheitsprüfung durch Dritte ist vor breiter Verbreitung sinnvoll; diese Stufe ersetzt sie nicht.

## Ergebnis

**Vorbereitet (Stand 23.11.2026, 13:30; die Stufe ist nicht begonnen, nichts ist abgehakt).**
- Entscheidungen: 0035 (GPL-3.0-or-later, öffentliches Repo unter `github.com/othervice`, erste
  Releases als Privatperson ohne Signatur, Name „Vice Mail“ mit eigenem Logo), 0037 (Updates, siehe
  Aufgabe „Updates“), 0038 (Orange aus dem Logo als Standardakzent).
- Marke: `docs/brand/` (Symbol, Zeichen, Wortmarke als Umrisse, also ohne Schriftlizenz) und die
  App-Symbole für macOS und Windows, reproduzierbar über `app/src-tauri/icons/generate.sh`
  (205c0b8); eigene Zeichnungen für die acht kleinen Windows-Größen (90934bb); `docs/INSTALL.md`
  erklärt, wie Windows den Symbol-Cache nach einem Update erneuert.
- Vorhanden: `README.md`, `LICENSE`, `docs/INSTALL.md` (mit Gatekeeper und SmartScreen),
  `docs/PROVIDERS.md` (Stufe 14), `docs/SYNC.md`, `docs/RELEASING.md` (Bau, Signatur der Updates,
  Veröffentlichung, Sicherung und Wechsel des Schlüssels). Es fehlen `CONTRIBUTING.md`,
  `SECURITY.md`, `CHANGELOG.md` und `docs/SECURITY_MODEL.md`.
- „Über“ zeigt Version, Commit, Build-Datum, Plattform, Lizenz (`LICENSE_SPDX`) und die Adresse
  des Quellcodes; die Liste der Bibliotheken mit Lizenz fehlt. `cargo deny` (Lizenzen,
  Sicherheitsmeldungen) und `pnpm audit` (Sicherheitsmeldungen der npm-Pakete, keine
  Lizenzprüfung) laufen in `scripts/check.sh`, grün auf a7dd759.
- Updates: Der Desktop-Test `updates.e2e.mjs` lief im Desktop-Lauf auf a7dd759 mit 4 von 4 grün
  (keine Verbindung bei ausgeschaltetem Schalter; „Jetzt suchen“ zeigt das Update mit Hinweisen als
  Text; eine manipulierte Datei wird abgelehnt; die echte Datei lädt und wird geprüft, ein
  Entwicklungs-Build installiert sich nicht selbst). Der Lauf als Ganzes war rot (6 von 20
  Spec-Dateien rot, siehe Stufe 8). `scripts/build-macos.sh` und `build-windows.sh` bauten am 23.11.
  mit `VICEMAIL_UPDATER=1` signierte Update-Dateien (`.app.tar.gz` und `-setup.exe` mit `.sig`)
  und prüften die Signatur gegen den öffentlichen Schlüssel („signature verified for 0.1.0“), auf
  dem Arbeitsstand vor dem Commit a7dd759; nichts davon ist veröffentlicht.
- Windows-Prototyp `prototype-2026-11-23-3.1` (76d4da2): behebt die Sicherung vor einer Migration
  unter Windows (bbd0fe8; „flush backup: access denied (os error 5)“, vom Menschen beim Update von
  Prototyp 2 auf 3 gefunden). Test `tests::partial_backups_left_behind_are_removed_and_never_listed`
  (in `check.sh`); der Fehler selbst tritt nur unter Windows auf und ist dort noch nicht nachgeprüft.

- macOS-Signatur (23.11., 15:15, auf dem Arbeitsstand nach e0ad102, nicht committet): vorbereitet,
  siehe Aufgabe „Signierung“ und `docs/RELEASING.md`. **Ausgeführt und geprüft:**
  `scripts/build-macos.sh 0.1.0-signtest.1` mit `VICEMAIL_UPDATER=1` und einem selbst signierten
  Code-Signing-Zertifikat in einem Wegwerf-Schlüsselbund (`VICEMAIL_SIGNING_KEYCHAIN`,
  `VICEMAIL_SIGNING_TEST=1`; Schlüsselbund danach gelöscht): `mail-mcp` und App mit Hardened
  Runtime, sicherem Zeitstempel von Apple und leeren Entitlements signiert, `codesign --verify
  --deep --strict` grün, Disk-Image (HFS+, UDZO, Volume-Symbol, Applications-Link wie bei Tauri)
  aus der signierten App gebaut und signiert, dieselbe CDHash in App, Disk-Image und
  Update-Archiv, Update-Signatur für die Version geprüft, Start aus dem Disk-Image grün; von Hand:
  der WebContent-Prozess lädt die Oberfläche wie beim Ad-hoc-Build (gleicher Speicherbedarf),
  `spctl` lehnt den Testbuild erwartungsgemäß ab. Derselbe Lauf ohne Identität baut ad hoc wie
  bisher (`--bundles app,dmg`, Update-Archiv von Tauri, Start grün). Abbrüche geprüft: unbekannte
  Identität, Nicht-Apple-Identität ohne `VICEMAIL_SIGNING_TEST`, ein selbst ausgestelltes
  Zertifikat mit dem Namen „Developer ID Application: …“ gilt nicht als Apple-Identität.
  `scripts/apple-signing-setup.sh` gegen einen Wegwerf-Schlüsselbund: `--check` meldet fehlende
  Identität und fehlendes Profil, `csr` legt Schlüssel und CSR an (CSR gültig, Schlüssel im
  Schlüsselbund, keine Schlüsseldatei), ein zweiter Lauf legt keinen zweiten Schlüssel an, `cert`
  lehnt ein fremdes Zertifikat und eines zu einem anderen Schlüssel ab und holt bei unbekannter
  Kette Apples Zwischenzertifikat; `notary` prüft Key ID und Issuer ID, ein erfundener Schlüssel
  wird von Apple abgelehnt und nichts gespeichert. **Ungeprüft bis zur Aktivierung:** Import eines
  echten Developer-ID-Zertifikats, Notarisierung (beide Runden), Stapeln, `spctl` „accepted,
  source=Notarized Developer ID“, die Option Apple-Account im Setup, das Update-Archiv mit
  gestapelter App im echten Updater, die Schlüsselbund-Abfrage beim Wechsel, Gatekeeper auf einem
  zweiten Mac.

- Diagnose (23.11., b13d553, 177ef1b und der UI-Commit danach): **Format** reine Textdatei
  (UTF-8) statt JSON, damit man sie vor dem Weitergeben lesen kann; die App verschickt sie nie. **Inhalt:** Version, Commit,
  Build-Datum, Plattform; System, Version (macOS aus `SystemVersion.plist`, Windows über
  `cmd /D /C ver` ohne Konsolenfenster), Architektur, Sprache der WebView, App-Sprache,
  WebView-Version; Größe des Datenordners (Pfad mit `~`) und seiner Teile; Schema-Version,
  Datenbankgröße, Zeilen je Tabelle (ohne FTS-Tabellen), Outbox-Vorgänge je Zustand und
  Fehlercodes, Suchindex; je Konto UUID, Anbieterart (`gmail`, `icloud`, … aus der eingebauten
  Anbieterliste, `local`, sonst `other`), Anmeldung „Passwort“, TLS-Art und Port, Schreiben an/aus,
  Sync-Zustand mit Fehlercode und Zeiten, Zahl der Ordner, Nachrichten und gespeicherten Texte;
  Zustand von Einstellungs-Sync, Updates, KI und MCP; nicht persönliche Einstellungen; das Ende der
  neuesten drei Logdateien, je höchstens 256 KiB, Zeilen höchstens 2000 Zeichen. **Nie:**
  Passwörter, Schlüssel, Tokens (der Schlüsselbund wird nicht gelesen), Adressen, Namen,
  Benutzernamen, Hostnamen eigener Server, Betreffe, Texte, Anhangsnamen, Kontakte, Ordnernamen.
  **Entscheidung Hostnamen:** Server der eingebauten Anbieterliste und der Update-Quelle dürfen in
  Logzeilen stehen bleiben (sie verraten nur die Anbieterart), alle anderen werden `<host>`. Die
  Logzeilen werden ein zweites Mal geschwärzt: bekannte Kontodaten, Identitäten und eigene
  Ordnernamen, Adressen, Hostnamen, IPv4 außer Loopback, Pfade (Feld und freier Text; unter dem
  Benutzerordner als `~/…`, sonst `<path>`, ohne Dateinamen), Geheimnis- und Personenfelder,
  `Bearer`/`Basic`, Token-Formate, lange Zufallsfolgen; lieber zu viel (`ui.theme` wird `<host>`). Betreffe oder Kontaktnamen in freiem Text kann sie nicht
  erkennen; sie werden nicht protokolliert. Sammeln scheitert nie (kaputte Datenbank, fehlende Logs
  stehen als „unavailable“ in der Datei); nur das Schreiben kann scheitern: neuer Fehlercode
  `diagnostics_write_failed` (atomar, ohne Reste). Keine neue Abhängigkeit. Aus der
  Sicherheitsprüfung mit erledigt: **V-07** (177ef1b) – die IPC-Brücke protokolliert fehlgeschlagene
  Befehle nur mit Code und fester Kurzbeschreibung, Details nur bei `internal`/`database` und
  geschwärzt (`logging::command_failed`); `docs/SECURITY_MODEL.md` 7.2 → 7.1. **I-1** – der Satz
  zur Autoconfig-Suche in `docs/PRIVACY.md` entspricht jetzt dem Code (die volle Adresse geht als
  `?emailaddress=` an `autoconfig.<domain>`; Code unverändert).
  **Ausgeführt und geprüft:** `cargo test` für `mail-model`, `mail-store`, `mail-engine` und
  `mail-app` grün, darin `diagnostics::tests::the_file_holds_facts_and_counts_but_no_secret_and_no_mail_content`
  (Konto mit Passwort im Test-Schlüsselbund, Nachricht mit Betreff, Text, Adressen und Anhang,
  eigener Ordner, Logzeilen mit Adresse, OAuth-Token, API-Schlüssel, Passwort, Benutzername,
  Server, Ordner, Anhangspfad sowie Fehlertexten mit Pfad im Benutzerordner und mit Adresse: keine
  dieser Zeichenfolgen und nicht der Benutzerordner steht in der Datei),
  `tests::failed_commands_are_logged_without_personal_error_texts` (V-07),
  `only_the_newest_log_files_and_only_their_ends_are_included` (fünf Logdateien zu je über 1 MiB:
  höchstens 3 × 256 KiB + 64 KiB, nur die neuesten drei, nur ihr Ende),
  `a_broken_database_or_missing_logs_still_give_a_file`,
  `a_file_that_cannot_be_written_is_a_typed_error_and_leaves_nothing`,
  die Schwärzungs- und System-Tests und der Zähltest im Store; Bindings mit `--features e2e`
  erzeugt, Frische-Test grün; Vitest `tests/diagnostics.test.tsx` (Erklärung, Export nur auf Klick,
  Abbruch, Fehler mit erneutem Versuch, Deutsch, axe), `ipc.test.ts`, `i18n.test.ts`;
  `pnpm --dir app typecheck` und `lint`; Clippy auch für das Windows-Ziel (cargo-xwin).
  `scripts/check.sh`: der erste Lauf war rot in „Rust tests“ mit zwei zeitabhängigen
  Compose-Tests (`compose::stage10_tests::offline_at_the_scheduled_time_it_goes_out_once_online`,
  `compose::tests::with_core::sending_at_once_while_offline_waits_and_goes_out_once_online`:
  Zustand `sending` statt `queued` bei einer Systemlast um 20; einzeln und in drei Wiederholungen
  der ganzen Engine-Tests grün, von dieser Änderung nicht berührt); der zweite Lauf und der letzte
  nach V-07 (Arbeitsstand mit der Oberfläche) vollständig grün.
  **Ungeprüft:** der Klick in der echten App (Speichern-Dialog, Datei auf der Platte; kein
  Desktop-Test), die Windows-Version über `ver` und die Pfade unter Windows (nur übersetzt, nicht
  ausgeführt).
- CI (23.11., b408449, de88964, b8e41a4): **Gitea** `.gitea/workflows/ci.yml`, Runner
  `gitea-runner` 6.0.1 (Homebrew, `brew services`) im Host-Modus auf dem Mac mini, nur für dieses
  Repo registriert, Bau-Cache unter `~/.cache/vice-mail-ci`; Einrichtung und Grenzen in
  `docs/RELEASING.md` („CI“). **Ausgeführt:** Lauf 464 (manuell, Zweig `ci-test`) rot in Vitest:
  der neue axe-Test der Lizenzliste brauchte auf dem ausgelasteten Mac mehr als 5 s (Zeitlimit
  jetzt 20 s, de88964); Lauf 465 (manuell, de88964) grün: `check.sh` in 393 s, beide Installer
  gebaut und als Artefakt hochgeladen (`.dmg` ad hoc signiert, `-setup.exe`, `SHA256SUMS`); Lauf
  466 (Push auf `main`, 788dcd4) grün. **GitHub** `.github/workflows/ci.yml` (b8e41a4): dieselben
  Schritte auf gehosteten Runnern `macos-latest` und `windows-latest`, unter Windows `check.sh`
  und der Installer nativ. **Ungeprüft:** der GitHub-Workflow ist noch nie gelaufen (erst nach dem
  ersten Push auf GitHub), damit auch noch kein Testlauf auf echtem Windows.

**Wartet auf**
- Mensch: den privaten Update-Schlüssel und sein Passwort sichern (`docs/RELEASING.md`, „Back it up
  now“).
- Mensch (macOS-Signatur, nach der Aktivierung der Apple-Mitgliedschaft): die Schritte 1 bis 6 in
  `docs/RELEASING.md`, „macOS signing and notarisation“ (Schlüssel und CSR mit
  `scripts/apple-signing-setup.sh`, Developer-ID-Zertifikat im Portal, Import, Team-API-Schlüssel
  mit Rolle „Developer“, notarytool-Profil, `--check`); `.p8` und das exportierte `.p12` im
  Passwort-Manager sichern. Danach den ersten notarisierten Build auf einem zweiten Mac öffnen.
- Mensch (Windows): `prototype-2026-11-23-3.1` über eine installierte ältere Version installieren
  (Update mit Migration, `docs/testing/windows-checklist.md`, Abschnitt 19). Das Symbol in der
  Taskleiste bei 125 und 150 Prozent Skalierung ansehen, sobald ein Prototyp mit 90934bb gebaut ist
  (3.1 hat noch die alten Symbole). Der Update-Weg in der App (Installer im Modus `passive`) lässt
  sich erst mit dem ersten öffentlichen Release prüfen.
