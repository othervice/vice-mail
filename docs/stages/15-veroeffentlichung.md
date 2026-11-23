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
  - *Vorbereitet am 23.11.2026 (0037, noch nicht abgehakt):* `tauri-plugin-updater` =2.14.1 nur aus Rust; Schalter „Automatisch nach Updates suchen“ (aus), einmalige Frage nach dem ersten Konto, Tagesdrossel, „Jetzt suchen“, Download und „Neu starten und installieren“ nur nach Klick, Windows `passive`; Endpunkt `https://github.com/othervice/vice-mail/releases/latest/download/latest.json` in `tauri.conf.json`; Schlüssel `org.vicemail.updater` im Schlüsselbund erzeugt, öffentlicher Schlüssel in `tauri.conf.json`; `VICEMAIL_UPDATER=1` für `scripts/build-macos.sh`, `build-windows.sh`, `release-local.sh` (signierte Artefakte, Signaturprüfung, `latest.json` über `scripts/updater-manifest.mjs`); Ablauf, Sicherung und Wechsel des Schlüssels in `docs/RELEASING.md`. Getestet gegen einen lokalen Update-Server (Rust) und in Vitest; der Desktop-Test `app/tests/e2e/updates.e2e.mjs` ist geschrieben, aber nicht gelaufen.
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

_Von der ausführenden Sitzung auszufüllen._
