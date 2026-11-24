# Stufe 18 — Verschiebbare Spalten, Aufklappen trotz Schnellaktionen, Popup für neue Mails

**Ziel:** Die Grenze zwischen Liste und Lesebereich lässt sich ziehen wie die zur linken Spalte.
Die Schnellaktionen beim Überfahren einer Zeile verdecken nie mehr das Aufklappen eines Gesprächs.
Neue Mails melden sich unten rechts am Bildschirmrand mit Absender, kurzem Textausschnitt und den
Knöpfen „Lesen“ und „Löschen“.

**Anlass:** Wunsch des Menschen vom 24.11.2026 nach dem ersten Test von 0.1.0.

**Voraussetzung:** Stufen 08 (Spalte, Lesebereich), 09 (Benachrichtigungen), 16 (Töne), 17
(Gespräche aufklappen, Kontaktbilder).

## Aufgaben

### A — Grenze zwischen Liste und Lesebereich verschieben

- [ ] Steht der Lesebereich rechts, lässt sich die Breite der Liste an der Grenze zum Lesebereich
  ziehen, genauso wie die linke Spalte (gleicher Griff, gleiche Rückmeldung beim Überfahren,
  Doppelklick setzt auf den Standard zurück). Steht er unten, gilt dasselbe für die Höhe der
  Liste. In der Stellung „statt der Liste“ gibt es keine Grenze.
- [ ] Grenzen: Liste und Lesebereich bleiben beide benutzbar (Mindestmaße), auch wenn das Fenster
  danach schmaler wird; die Zeilen der Liste wechseln wie bisher je nach Breite zwischen ein- und
  mehrzeilig.
- [ ] Tastatur und Bildschirmleser wie beim Griff der linken Spalte (Trennlinie mit Wert,
  Pfeiltasten, Pos1/Ende).
- [ ] Die Maße werden mit dem Fensterlayout gespeichert (`WindowLayout`, wie `sidebar_width`) und
  sind nach dem Neustart wieder da.

### B — Aufklappen trotz Schnellaktionen

- [ ] Beim Überfahren einer Zeile bleiben die Schnellaktionen (Löschen, Archivieren, …) erreichbar,
  verdecken aber nie den Knopf zum Aufklappen („3 Antworten“) und nie andere Klickziele der Zeile.
  Gilt für schmale mehrzeilige und breite einzeilige Listen, für Gesprächszeilen und
  aufgeklappte Mailzeilen, mit und ohne Kontospalte.
- [ ] Ein Klick auf den Aufklapp-Knopf klappt auf bzw. zu, während die Maus über der Zeile ist;
  ein Test prüft das mit echtem Zeiger an der Stelle, an der der Knopf zu sehen ist.

### C — Popup für neue Mails unten rechts

- [ ] Kommt neue Mail an (dieselben Regeln wie bisher: nur nach dem ersten Abgleich, stumme Konten,
  Ruhezeiten, Kategorien, Regeln), erscheint unten rechts im Arbeitsbereich des Bildschirms
  (über Dock bzw. Taskleiste) ein Popup von Vice Mail: Bild bzw. Initialen und Name des
  Absenders, Betreff, zwei Zeilen Textausschnitt als reiner Text, Zeit, bei mehreren Konten das
  Konto; Knöpfe „Lesen“ und „Löschen“, dazu Schließen.
- [ ] „Lesen“ holt das Hauptfenster nach vorn und zeigt die Mail im Posteingang. „Löschen“ legt
  sie über die Outbox in den Papierkorb, mit „Rückgängig“ für ein paar Sekunden; bei nur lesenden
  Konten ist „Löschen“ gesperrt und sagt warum.
- [ ] Mehrere neue Mails: mehrere Karten übereinander bis zu einer Grenze, darüber eine Zeile
  „N weitere“. Das Popup verschwindet nach einer Weile von selbst, nicht solange die Maus darauf
  ist. Es nimmt der App im Vordergrund nicht den Fokus.
- [ ] Einstellung unter Benachrichtigungen: Popup von Vice Mail (Standard) oder Mitteilung des
  Systems. Verborgene Inhalte zeigen im Popup weder Absender noch Text. Der Ton der Stufe 16
  klingt auch beim Popup.
- [ ] Sicherheit: eigenes Fenster mit eigener, minimaler Berechtigung, gleiche Schutzmaßnahmen wie
  alle Fenster (Navigation, WebView2-Schutz); das Fenster entsteht erst bei Bedarf und wird danach
  wieder geschlossen. Entscheidung festhalten.

## Nicht in dieser Stufe

- Benachrichtigungen für fehlgeschlagene Sendungen und Snoozes bleiben Mitteilungen des Systems.
- Weitere Knöpfe im Popup (Antworten, Archivieren).

## Abnahme

- [ ] `scripts/check.sh` grün, Server- und Desktop-Tests grün.
- [ ] Griff zwischen Liste und Lesebereich (rechts und unten) per Desktop-Test gezogen, Maß nach
  Neustart des Fensterlayouts wieder da.
- [ ] Aufklappen per echtem Klick, während die Schnellaktionen sichtbar sind, per Desktop-Test.
- [ ] Popup per Desktop-Test: neue Mail auf dem Testserver → Popup mit Absender und Ausschnitt,
  „Lesen“ zeigt die Mail im Hauptfenster, „Löschen“ legt sie in den Papierkorb, nur lesendes Konto
  sperrt „Löschen“.

## Ergebnis

(offen)
