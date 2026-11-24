# Stufe 17 — Gespräche wie in Thunderbird, Kontakte mit Bildern, eigene Ansichten

**Ziel:** Gespräche lassen sich in der Liste aufklappen, rechts steht immer genau eine Mail.
Kontakte sind eine eigene, schön gestaltete Ansicht mit eigenen Bildern, guter Suche und
„Als Spam markieren“. Die Ansichten in der linken Spalte stellt jeder selbst zusammen.

**Anlass:** Wunsch des Menschen vom 24.11.2026 (mit Bildschirmfoto von Thunderbird), vor dem ersten
öffentlichen Release umzusetzen.

**Voraussetzung:** Stufen 07 (Gespräche), 08 (Spalte, Tastatur), 10 (Kontakte), 16 und 0039.

## Aufgaben

### A — Gespräche in der Liste aufklappen

- [ ] Eine Zeile je Gespräch wie bisher (0039), mit dem Absender einmal und der Zahl der Antworten
  („3 Antworten“) samt Pfeil zum Aufklappen. Aufgeklappt stehen alle Mails des Gesprächs
  eingerückt darunter, mit einer feinen Linie links, älteste zuerst, eigene gesendete Mails
  eingeschlossen, auch aus anderen Ordnern desselben Kontos.
- [ ] Rechts im Lesebereich steht **nur die gewählte Mail**, nicht mehr das ganze Gespräch
  untereinander. Ein Klick auf die Gesprächszeile zeigt die neueste (bzw. die älteste ungelesene)
  Mail des Gesprächs. Die Ansicht als ganzes Gespräch rechts entfällt; Entscheidung festhalten.
- [ ] Tastatur: Pfeil rechts klappt auf, Pfeil links klappt zu bzw. springt zur Gesprächszeile,
  Pfeil hoch/runter läuft durch sichtbare Zeilen; Bildschirmleser hören Ebene, Position und
  „aufgeklappt/zugeklappt“. Aktionen auf der Gesprächszeile wirken wie bisher auf das Gespräch,
  auf einer Mailzeile nur auf diese Mail.
- [ ] Virtualisierte Liste bleibt flüssig (Leistungsziele aus `docs/SPEC.md`), aufgeklappte
  Gespräche bleiben beim Nachladen und Sortieren stabil.

### B — Kontakte

- [ ] Jede Adresse aus empfangenen und gesendeten Mails (Von, An, Cc, Antwort an) wird Kontakt,
  auch rückwirkend für vorhandene Mails; Listen- und Systemadressen (no-reply, Mailer-Daemon)
  bleiben erkennbar und lassen sich ausblenden.
- [ ] Eigene Bilder: Bild aus einer Datei wählen (Ziehen oder Dateiauswahl), quadratisch
  zuschneiden, verkleinert lokal speichern; Entfernen. Kein Laden von Bildern aus dem Netz
  (kein Gravatar, kein BIMI). Das Bild erscheint überall, wo heute Initialen stehen (Liste,
  Lesebereich, Composer, Kontakte); ohne Bild bleiben die Initialen.
- [ ] Die Bilder werden wie Bilder aus Mails im abgeschotteten Hilfsprozess dekodiert und neu
  kodiert (0046), mit Größengrenze.
- [ ] Kontaktseite als eigene Ansicht: Liste mit Bild, Name, Adresse, letzter Mail; schnelle
  Suche über Name, Adresse und Domain; Detailbereich mit allen Adressen, Bild, Notiz, letzten Mails
  mit diesem Kontakt, „Mail schreiben“, „Alle Mails anzeigen“, „Als Spam markieren“.
- [ ] „Als Spam markieren“ sperrt die Adresse (vorhandene Sperrliste aus Stufe 16) und bietet an,
  vorhandene Mails in den Spam-Ordner zu verschieben (über die Outbox, nur bei Konten mit
  Schreibzugriff). Rückgängig machbar.
- [ ] Kontakte, Bilder (verkleinert, mit Größengrenze) und Spam-Markierungen laufen über den
  Einstellungs-Sync, wenn er an ist.

### C — Ansichten in der Spalte bearbeiten

- [ ] Neue Ansicht „Kontakte“ in der Spalte und in der Befehlspalette.
- [ ] „Ansichten bearbeiten …“: Ansichten ein- und ausblenden und sortieren (Später, Aufgaben,
  Leseliste, Markiert, Angeheftet, Angeheftete Anhänge, Gespeicherte Suchen, Kontakte, …); per
  Maus und Tastatur; Zurücksetzen auf die Standardauswahl. Gespeichert als UI-Einstellung im Kern,
  mit dem Einstellungs-Sync.

## Abnahme

- [ ] Gespräche lassen sich in der Liste mit Maus und Tastatur auf- und zuklappen; rechts steht
  immer genau eine Mail; Desktop-Test und axe ohne schwere Befunde.
- [ ] Ein eigenes Bild erscheint in Liste, Lesebereich, Composer und Kontakten; nach einem
  Neustart noch da; ein manipuliertes Bild wird abgelehnt, ohne die App zu stören.
- [ ] Kontaktsuche findet nach Name, Adresse und Domain bei 10.000 Kontakten unter 100 ms.
- [ ] „Als Spam markieren“ sperrt die Adresse, neue Mails von ihr landen nicht im Posteingang;
  Rückgängig hebt das auf.
- [ ] Ansichten lassen sich ein-, ausblenden und sortieren; die Auswahl übersteht einen Neustart.
- [ ] Bildschirmfotos in `docs/images/` neu, wo sich etwas sichtbar geändert hat.
- [ ] `scripts/check.sh` ist grün, Desktop-Tests grün.

## Nicht in dieser Stufe

- Kontakte aus dem Netz (CardDAV, Google, Microsoft): nach Version 1 (0032).
- Bilder aus dem Netz (Gravatar, BIMI, Favicons).

## Ergebnis

*(noch offen)*
