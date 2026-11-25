# Stufe 19 — Sortieren, Schnellfilter, Quelltext, Umleiten, Kontaktgruppen und Export

**Ziel:** Die Funktionen aus Thunderbird, die der Mensch am 24.11.2026 ausgewählt hat: die Liste
sortieren und schnell filtern, Quelltext ansehen, Mails umleiten oder als neu bearbeiten,
Kontaktgruppen sowie Import und Export von Kontakten und Ordnern.

**Anlass:** Vergleich mit Thunderbird vom 24.11.2026. Gewählt: „Sortieren + Schnellfilter“,
„Quelltext, Umleiten, Als neu“, „Kontaktgruppen + Export“. Nicht gewählt und deshalb nicht in
dieser Stufe: stärkere Filterregeln, lernender Spamfilter. Signaturen je Absenderadresse gibt es
seit Stufe 10 (Einstellungen → Schreiben → Signaturen, Standard je Adresse für neue Mails und für
Antworten).

**Voraussetzung:** Stufe 18. Teil A (Liste) baut auf Stufe 18 A/B/F auf und beginnt nach ihnen; B und C laufen parallel zu Stufe 18.

**Version:** Der Mensch hat am 24.11.2026 entschieden: Stufen 18, 19 und 20 zusammen ergeben 0.1.1 (Gitea-Release, GitHub bleibt bis zu seinem Okay privat).

## Aufgaben

### A — Sortieren und Schnellfilter

- [x] Liste sortieren nach Datum (Standard), Absender, Empfänger (in Gesendet und Entwürfen),
  Betreff, Größe, auf- und absteigend; je Ordner bzw. Ansicht gespeichert. Gespräche sortieren
  nach ihrer neuesten Mail. Leistungsziele aus `docs/SPEC.md` gelten weiter (500.000 Mails).
- [x] Tabellenansicht als weitere Darstellung der breiten Liste: Spalten mit Kopf zum Sortieren,
  Breiten ziehbar, Spalten wählbar. Tastatur und Bildschirmleser wie in der bisherigen Liste.
- [x] Schnellfilter-Leiste über der Liste: Ungelesen, Markiert, Mit Anhang, Von Kontakten,
  Schlagwort, Textfeld (Absender, Betreff, Text); kombinierbar, mit „Zurücksetzen“, per Tastatur
  erreichbar; gilt für die aktuelle Ansicht und zeigt die Trefferzahl.

### B — Quelltext, Umleiten, Als neu bearbeiten

- [x] „Quelltext anzeigen“ (Cmd/Strg+U) in eigenem Fenster: die Mail roh als reiner Text in
  Festbreitenschrift, nie interpretiert; sehr große Mails gekürzt mit „Als .eml sichern“. Dazu im
  Kopf des Lesebereichs „Alle Kopfzeilen“.
- [x] „Umleiten“: die Mail unverändert an neue Empfänger, mit `Resent-*`-Kopfzeilen
  (RFC 5322, Abschnitt 3.6.6); über die Outbox mit Senden-rückgängig; nur mit Schreibzugriff.
- [x] „Als neu bearbeiten“: neuer Entwurf mit Empfängern, Betreff, Text und Anhängen der Mail,
  kein Antwortbezug; auch aus Gesendet.

### C — Kontaktgruppen, Import und Export

- [x] Kontaktgruppen (Verteilerlisten) auf der Kontaktseite: anlegen, umbenennen, Mitglieder
  hinzufügen und entfernen. Im Composer schlägt die Autovervollständigung Gruppen vor; eine Gruppe
  wird zu ihren Mitgliedern, bei vielen Empfängern mit Hinweis auf Bcc.
- [x] vCard-Import und -Export (`.vcf`, 3.0 und 4.0): einzelne Kontakte, Gruppen, alle; der
  Import führt Dubletten zusammen, Bilder laufen durch den abgeschotteten Hilfsprozess (0046).
- [x] Ordner als mbox exportieren; mbox in einen Ordner importieren (über die Outbox per APPEND,
  nur mit Schreibzugriff, mit Fortschritt und Abbrechen).
- [x] Übernahme aus Thunderbird: Profil wählen, Konten (Server, Identitäten, Signaturen) und lokale
  Ordner anzeigen und ausgewählt übernehmen. Passwörter werden nie übernommen; neue Konten
  starten nur lesend.

## Nicht in dieser Stufe

- Stärkere Filterregeln, lernender Spamfilter, Offline-Einstellungen je Ordner.
- OpenPGP, S/MIME, CalDAV, CardDAV, POP3 (nach Version 1).

## Abnahme

- [x] `scripts/check.sh` grün, Server- und Desktop-Tests grün.
- [x] Sortieren, Tabellenansicht und Schnellfilter per Desktop-Test, Leistung mit 100.000 Mails
  gemessen.
- [x] Quelltext, Umleiten (Empfänger erhält die unveränderte Mail mit `Resent-*` auf dem
  Testserver) und Als neu per Test.
- [x] Gruppen im Composer, vCard- und mbox-Rundweg, Übernahme aus einem Test-Thunderbird-Profil
  per Test.

## Ergebnis

Erledigt am 25.11.2026 (A in einem Zweig, B und C in einem zweiten). Entscheidungen 0055
(Quelltext, Umleiten, Als neu), 0056 (Kontaktgruppen, vCard), 0057 (mbox, Übernahme aus
Thunderbird), 0061 (Sortieren, Tabellenansicht), 0062 (Schnellfilter). Migrationen 0022 und 0026.
Keine neuen Abhängigkeiten (vCard-, mbox- und `prefs.js`-Leser in `mail-content`).

- Leistung mit 100.000 Mails: sortierte Seite 0,3 ms (vereint 1,1 ms), als Gespräche 2,6 ms;
  in der App Klick bis neue Reihenfolge 23–55 ms, Filter-Chips 14–78 ms. Tippen im Textfilter
  213–312 ms ab der letzten Taste einschließlich der bewussten Pause von 150 ms: knapp über dem
  Suchziel von 200 ms; Vorschlag in `docs/testing/performance.md`.
- Server-Tests: eine umgeleitete Mail kommt unverändert mit `Resent-*` an; ein mbox-Import landet
  über die Outbox im Ordner.

Gesamtprüfung auf `afeedc3` (Version 0.1.1, 25.11.2026, 09:36–09:57): `scripts/check.sh` grün
(1.520 Rust-Tests, 4.144 Vitest, Clippy auch für das Windows-Ziel, Lizenzen, `cargo deny`,
`pnpm audit`), Desktop-Suite 33/33 Spec-Dateien (173 Tests). Server-Tests auf `c31ab8d`: 1.613
bestanden; die 9 Updater-Fehler dort kamen von einer falsch geänderten Testkonstante (die
Test-App meldet immer 0.1.0) und sind auf `afeedc3` behoben (`updater` 11/11).

**Ungeprüft:** Sortieren und Filtern mit 500.000 Mails (nur die Migration gemessen: 7,9 s);
Zeichengeschwindigkeit der Tabellenansicht; echte Thunderbird-Profile (nur die Testvorlage in
`testmail/thunderbird/`) und vCards anderer Programme (nur Unit-Tests); Windows (Profilpfad
`%APPDATA%\Thunderbird`, Quelltext-Fenster).
