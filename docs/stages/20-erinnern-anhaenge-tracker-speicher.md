# Stufe 20 — Anhang-Erinnerung, Erinnern ohne Antwort, Anhänge-Ansicht, Tracker-Schutz, Speicher

**Ziel:** Fünf Funktionen aus Gmail, Outlook, Apple Mail, Spark und eM Client, die der Mensch
am 24.11.2026 ausgewählt hat.

**Anlass:** Vergleich mit Mailbird und anderen Mail-Programmen vom 24.11.2026 (nach 0032).
Ausgewählt: Anhang-Erinnerung, Erinnern ohne Antwort, Anhänge-Ansicht, Tracker-Schutz sichtbar,
Postfach-Speicher. Nicht gewählt: Auswahl zitieren, automatisch Bcc/Cc, Gespräch stummschalten,
Newsletter-Übersicht, neue Absender prüfen, App-Sperre, Abwesenheitsnotiz.

**Voraussetzung:** Stufe 19 abgeschlossen.

## Aufgaben

### A — Anhang-Erinnerung

- [ ] Beim Senden: Erwähnt der eigene Text einen Anhang („Anhang“, „angehängt“, „attached“,
  „attachment“ …, Englisch und Deutsch, ohne den zitierten Teil und die Signatur), ist aber keiner
  dabei, fragt die App „Trotzdem senden?“ mit „Anhang hinzufügen“. Abschaltbar unter Schreiben;
  keine Rückfrage beim Senden aus dem Postausgang.

### B — Erinnern, wenn keine Antwort kommt

- [ ] Beim Senden „Erinnern, wenn bis … keine Antwort kommt“ (morgen, in 3 Tagen, nächste Woche,
  eigenes Datum). Kommt bis dahin keine Antwort im Gespräch, erscheint die gesendete Mail wieder
  oben im Posteingang mit dem Hinweis „Keine Antwort seit …“; eine Antwort hebt die Erinnerung
  auf. Ohne KI, lokal wie Snooze, abgeglichen über den Einstellungs-Sync wie Snooze.
- [ ] Ansicht bzw. Filter „Warten auf Antwort“, Erinnerung ändern und entfernen.

### C — Anhänge-Ansicht

- [ ] Eigene Ansicht „Anhänge“ (unter den Ansichten, abschaltbar wie die anderen): alle Anhänge
  aller Konten mit Dateiname, Typ, Größe, Absender, Datum; filtern nach Typ (Bilder, PDF,
  Dokumente, Tabellen, Archive, Sonstige), Absender und Konto; Suche nach Dateinamen.
- [ ] Öffnen, Sichern, Vorschau wie im Lesebereich (Bilder nur über den abgeschotteten
  Hilfsprozess, 0046); „Zur Mail“. Nicht geladene Anhänge werden erst beim Öffnen geladen.

### D — Tracker-Schutz sichtbar

- [ ] Pro Mail zeigt der Lesebereich, wie viele Tracker blockiert wurden (Zählpixel, bekannte
  Tracking-Domains, Bilder mit Tracking-Parametern), mit Erklärung, was das heißt.
- [ ] Links: Tracking-Parameter (`utm_*`, `fbclid`, `gclid`, `mc_eid`, `mc_cid` u. a., gepflegte
  Liste im Code) werden beim Öffnen entfernt; die Linkvorschau zeigt das saubere Ziel.
  Abschaltbar. Weitergeleitete Links über Tracking-Umleitungen werden nicht aufgelöst (das wäre
  eine Netzverbindung).

### E — Postfach-Speicher

- [ ] Belegter und verfügbarer Speicher je Konto vom Server (IMAP `QUOTA`, RFC 9208; JMAP Quota,
  RFC 9425), in den Kontoeinstellungen und als Hinweis am Konto ab 90 %; ohne Server-Unterstützung
  steht dort „Der Server nennt keinen Speicherplatz“.
- [ ] Bei vollem Postfach (Fehler beim Speichern von Entwürfen oder Gesendet) ein verständlicher
  Hinweis mit dem Weg zum Aufräumen (größte Mails zuerst über die Suche).

## Nicht in dieser Stufe

- Die nicht gewählten Funktionen (siehe Anlass).

## Abnahme

- [ ] `scripts/check.sh` grün, Server- und Desktop-Tests grün.
- [ ] Jede Aufgabe per Test auf dem Testserver nachgewiesen (Erinnerung mit vorgestellter Uhr,
  Quota des Testservers, Tracker-Zählung an Beispielmails).

## Ergebnis

(offen)
