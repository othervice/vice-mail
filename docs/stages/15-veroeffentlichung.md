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
- [ ] **Updates.** Tauri-Updater mit Signaturprüfung. Der private Schlüssel liegt im Schlüsselbund des Build-Macs (0037; mit CI später als CI-Geheimnis, nie im Repo). Update-Beschreibung als statische Datei bei den Releases. Prüfung nur mit Zustimmung (0037: Standard aus). Rückweg: die vorherige Version bleibt herunterladbar, und die Datenbank-Sicherungen aus Stufe 1 erlauben das Zurückgehen.
  - *Vorbereitet am 23.11.2026 (0037, noch nicht abgehakt):* `tauri-plugin-updater` =2.14.1 nur aus Rust; Schalter „Automatisch nach Updates suchen“ (aus), einmalige Frage nach dem ersten Konto, Tagesdrossel, „Jetzt suchen“, Download und „Neu starten und installieren“ nur nach Klick, Windows `passive`; Endpunkt `https://github.com/othervice/vice-mail/releases/latest/download/latest.json` in `tauri.conf.json`; Schlüssel `org.vicemail.updater` im Schlüsselbund erzeugt, öffentlicher Schlüssel in `tauri.conf.json`; `VICEMAIL_UPDATER=1` für `scripts/build-macos.sh`, `build-windows.sh`, `release-local.sh` (signierte Artefakte, Signaturprüfung, `latest.json` über `scripts/updater-manifest.mjs`); Ablauf, Sicherung und Wechsel des Schlüssels in `docs/RELEASING.md`. Getestet gegen einen lokalen Update-Server (Rust) und in Vitest; der Desktop-Test `app/tests/e2e/updates.e2e.mjs` lief im Desktop-Lauf auf a7dd759 mit 4 von 4 grün (siehe Ergebnis).
  - *Offen bis zur Veröffentlichung:* erstes Release mit `latest.json` auf GitHub, dann Update von einer Version auf die nächste auf echtem macOS und Windows (Abnahmepunkt 2). **Der Mensch sichert den privaten Schlüssel und sein Passwort** (`docs/RELEASING.md`, „Back it up now“).
- [ ] **Sicherheitsprüfung.** `/security-review` über den gesamten Code. Bedrohungsmodell als `docs/SECURITY_MODEL.md`: was geschützt ist, was nicht, welche Grenzen es gibt. Zeitlich begrenztes Fuzzing von MIME-Parser-Anbindung und HTML-Pipeline. Befunde beheben oder begründet offen lassen.
- [ ] **Erster Start.** Geführte Einrichtung für neue Nutzer: Konto, Thema, optional Sync und KI.
- [ ] **Diagnose.** „Diagnose exportieren“ packt Logs und Systemangaben in eine Datei; ein Test stellt sicher, dass keine Geheimnisse und keine Mail-Inhalte enthalten sind.

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

**Wartet auf**
- Mensch: den privaten Update-Schlüssel und sein Passwort sichern (`docs/RELEASING.md`, „Back it up
  now“).
- Mensch (Windows): `prototype-2026-11-23-3.1` über eine installierte ältere Version installieren
  (Update mit Migration, `docs/testing/windows-checklist.md`, Abschnitt 19). Das Symbol in der
  Taskleiste bei 125 und 150 Prozent Skalierung ansehen, sobald ein Prototyp mit 90934bb gebaut ist
  (3.1 hat noch die alten Symbole). Der Update-Weg in der App (Installer im Modus `passive`) lässt
  sich erst mit dem ersten öffentlichen Release prüfen.
