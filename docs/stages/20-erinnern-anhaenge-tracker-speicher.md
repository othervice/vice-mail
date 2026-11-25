# Stufe 20 — Anhang-Erinnerung, Erinnern ohne Antwort, Anhänge-Ansicht, Tracker-Schutz, Speicher

**Ziel:** Fünf Funktionen aus Gmail, Outlook, Apple Mail, Spark und eM Client, die der Mensch
am 24.11.2026 ausgewählt hat.

**Anlass:** Vergleich mit Mailbird und anderen Mail-Programmen vom 24.11.2026 (nach 0032).
Ausgewählt: Anhang-Erinnerung, Erinnern ohne Antwort, Anhänge-Ansicht, Tracker-Schutz sichtbar,
Postfach-Speicher. Nicht gewählt: Auswahl zitieren, automatisch Bcc/Cc, Gespräch stummschalten,
Newsletter-Übersicht, neue Absender prüfen, App-Sperre, Abwesenheitsnotiz.

**Voraussetzung:** Stufen 16 und 17; läuft parallel zu Stufe 18 und 19.

**Version:** Stufen 18, 19 und 20 zusammen ergeben 0.1.1 (Entscheidung des Menschen vom 24.11.2026).

## Aufgaben

### A — Anhang-Erinnerung

- [x] Beim Senden: Erwähnt der eigene Text einen Anhang („Anhang“, „angehängt“, „attached“,
  „attachment“ …, Englisch und Deutsch, ohne den zitierten Teil und die Signatur), ist aber keiner
  dabei, fragt die App „Trotzdem senden?“ mit „Anhang hinzufügen“. Abschaltbar unter Schreiben;
  keine Rückfrage beim Senden aus dem Postausgang.

### B — Erinnern, wenn keine Antwort kommt

- [x] Beim Senden „Erinnern, wenn bis … keine Antwort kommt“ (morgen, in 3 Tagen, nächste Woche,
  eigenes Datum). Kommt bis dahin keine Antwort im Gespräch, erscheint die gesendete Mail wieder
  oben im Posteingang mit dem Hinweis „Keine Antwort seit …“; eine Antwort hebt die Erinnerung
  auf. Ohne KI, lokal wie Snooze, abgeglichen über den Einstellungs-Sync wie Snooze.
- [x] Ansicht bzw. Filter „Warten auf Antwort“, Erinnerung ändern und entfernen.

### C — Anhänge-Ansicht

- [x] Eigene Ansicht „Anhänge“ (unter den Ansichten, abschaltbar wie die anderen): alle Anhänge
  aller Konten mit Dateiname, Typ, Größe, Absender, Datum; filtern nach Typ (Bilder, PDF,
  Dokumente, Tabellen, Archive, Sonstige), Absender und Konto; Suche nach Dateinamen.
- [x] Öffnen, Sichern, Vorschau wie im Lesebereich (Bilder nur über den abgeschotteten
  Hilfsprozess, 0046); „Zur Mail“. Nicht geladene Anhänge werden erst beim Öffnen geladen.

### D — Tracker-Schutz sichtbar

- [x] Pro Mail zeigt der Lesebereich, wie viele Tracker blockiert wurden (Zählpixel, bekannte
  Tracking-Domains, Bilder mit Tracking-Parametern), mit Erklärung, was das heißt.
- [x] Links: Tracking-Parameter (`utm_*`, `fbclid`, `gclid`, `mc_eid`, `mc_cid` u. a., gepflegte
  Liste im Code) werden beim Öffnen entfernt; die Linkvorschau zeigt das saubere Ziel.
  Abschaltbar. Weitergeleitete Links über Tracking-Umleitungen werden nicht aufgelöst (das wäre
  eine Netzverbindung).

### E — Postfach-Speicher

- [x] Belegter und verfügbarer Speicher je Konto vom Server (IMAP `QUOTA`, RFC 9208; JMAP Quota,
  RFC 9425), in den Kontoeinstellungen und als Hinweis am Konto ab 90 %; ohne Server-Unterstützung
  steht dort „Der Server nennt keinen Speicherplatz“.
- [x] Bei vollem Postfach (Fehler beim Speichern von Entwürfen oder Gesendet) ein verständlicher
  Hinweis mit dem Weg zum Aufräumen (größte Mails zuerst über die Suche).

## Nicht in dieser Stufe

- Die nicht gewählten Funktionen (siehe Anlass).

## Abnahme

- [x] `scripts/check.sh` grün, Server- und Desktop-Tests grün.
- [x] Jede Aufgabe per Test auf dem Testserver nachgewiesen (Erinnerung mit vorgestellter Uhr,
  Quota des Testservers, Tracker-Zählung an Beispielmails).

## Ergebnis

Erledigt am 25.11.2026. Entscheidungen 0058 (Anhang-Erinnerung, Tracker-Schutz), 0059 (Erinnern,
wenn keine Antwort kommt), 0060 (Anhänge-Ansicht, Postfach-Speicher). Migrationen 0024 und 0025.
Keine neuen Abhängigkeiten. Dazu der Suchfilter `larger:`/`größer:`.

- Server-Tests: Quota des Testservers (IMAP `QUOTA`), Erinnerung mit vorgestellter Uhr (eine
  Antwort von außen beendet sie, eine eigene nicht).
- Beim Zusammenführen gefunden und behoben: `RecordKind::ALL` des Einstellungs-Syncs enthielt die
  Kontaktgruppen aus Stufe 19 nicht; jetzt 23 Arten.

Gesamtprüfung auf `afeedc3` (Version 0.1.1, 25.11.2026, 09:36–09:57): `scripts/check.sh` grün
(1.520 Rust-Tests, 4.144 Vitest, Clippy auch für das Windows-Ziel, Lizenzen, `cargo deny`,
`pnpm audit`), Desktop-Suite 33/33 Spec-Dateien (173 Tests). Server-Tests auf `c31ab8d`: 1.613
bestanden; die 9 Updater-Fehler dort kamen von einer falsch geänderten Testkonstante (die
Test-App meldet immer 0.1.0) und sind auf `afeedc3` behoben (`updater` 11/11).

**Ungeprüft:** JMAP-Quota nur gegen eine Test-Attrappe; der Weg „Postfach voll“ nur auf
Adapterebene; Windows. Die Anhänge-Ansicht zeigt nur Mails, deren Inhalt auf dem Rechner liegt
(standardmäßig 30 Tage vorab geladen); `larger:` sortiert nach Datum, nicht nach Größe.
