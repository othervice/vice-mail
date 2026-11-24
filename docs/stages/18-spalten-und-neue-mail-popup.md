# Stufe 18 — Verschiebbare Spalten, Aufklappen trotz Schnellaktionen, Popup für neue Mails, Ordner anlegen, neue Farben, oberste Mail gleich offen

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

### D — Ordner anlegen, auch auf dem Server

Wunsch vom 24.11.2026 (später am Abend): „für jedes Konto Ordner erstellen können, die auch auf dem
Server angelegt werden, auch vorhandene Ordner müssen geladen werden“.

- [ ] „Neuer Ordner…“ am Konto und „Neuer Unterordner…“ an jedem Ordner, die ein Unterordner
  sein darf (Menü der Zeile, Befehlspalette). Name prüfen (leer, doppelt, Trennzeichen des
  Servers); angelegt wird über die Outbox (`CreateMailbox`, IMAP und JMAP) mit sichtbarem Zustand,
  der Ordner steht danach sofort im Baum und nimmt Mails auf (Ziehen, Verschieben).
- [ ] Nur lesende Konten: Aktion gesperrt mit Begründung, wie bei Umbenennen und Löschen.
- [ ] Vorhandene Ordner: Alle wählbaren Ordner des Servers stehen im Baum (heute `LIST "" "*"`
  beim vollen Abgleich). Ordner, die ein anderes Programm anlegt, erscheinen ohne Neustart;
  dazu „Ordner vom Server neu laden“ am Konto.

### E — Farbwelt: Hell mit Blau, Dunkel in GitHub-Farben

Wunsch vom 24.11.2026: „die helle Version sollte in einem hellen mit Blau Design sein, die dunkle
mehr die GitHub-Farben haben statt dieses Braun.“ Hell, Dunkel und „wie das System“ gibt es seit
Stufe 1 (Einstellungen → Darstellung); es geht um die Farben.

- [ ] Hell: kühle, klare Flächen (Weiß und helles Blaugrau statt Beige), Blau als Akzent.
- [ ] Dunkel: neutrale, leicht bläuliche Grautöne wie bei GitHub (sehr dunkles Blaugrau als
  Grund, abgestufte Flächen, kühle Linien) statt der warmen Brauntöne; Blau als Akzent.
- [ ] Standard-Akzent wird Blau (löst den Standard aus 0038 ab); die anderen Farbthemen bleiben
  wählbar, eine gespeicherte Wahl bleibt. Entscheidung festhalten.
- [ ] Kontrast nach WCAG AA in beiden Modi für jedes Thema (`app/tests/contrast.test.ts`),
  `docs/DESIGN.md` und die README-Bilder angepasst.
- [ ] Hell/Dunkel schneller umschalten: Befehl in der Befehlspalette.

### F — Oberste Mail beim Öffnen eines Ordners gleich zeigen

Wunsch vom 24.11.2026: „Wenn man z. B. in einen Ordner wie Posteingang geht, soll da nicht eine
breite Liste aller Mails sein, sondern die oberste Mail bereits geöffnet. Dann entstehen nicht
diese komischen langen Zeilen.“

- [ ] Steht der Lesebereich rechts oder unten, zeigt das Öffnen eines Ordners oder einer Ansicht
  (Posteingang, vereinter Posteingang, Ungelesen, …) gleich die oberste Mail im Lesebereich; bei
  einem Gespräch die Mail, die ein Klick auf die Gesprächszeile zeigen würde (Stufe 17). In der
  Stellung „statt der Liste“ bleibt es bei der Liste.
- [ ] Die Liste behält immer ihre Lesebreite (Maß aus A), auch wenn nichts gewählt ist (leerer
  Ordner, Auswahl aufgehoben): der Lesebereich zeigt dann einen ruhigen leeren Zustand statt
  einer Liste über die ganze Breite.
- [ ] Automatisch geöffnet heißt nicht gelesen: eine so gezeigte Mail wird nur nach der
  Einstellung „Als gelesen markieren“ behandelt, wenn du sie selbst auswählst oder im Lesebereich
  mit ihr arbeitest. Entscheidung festhalten (0054).
- [ ] Während des Tippens einer Suche springt die Auswahl nicht mit; Tastatur und Fokus bleiben
  wie bisher (der Fokus springt nicht von selbst in den Lesebereich).

## Nicht in dieser Stufe

- Benachrichtigungen für fehlgeschlagene Sendungen und Snoozes bleiben Mitteilungen des Systems.
- Weitere Knöpfe im Popup (Antworten, Archivieren).

## Abnahme

- [ ] `scripts/check.sh` grün, Server- und Desktop-Tests grün.
- [ ] Griff zwischen Liste und Lesebereich (rechts und unten) per Desktop-Test gezogen, Maß nach
  Neustart des Fensterlayouts wieder da.
- [ ] Aufklappen per echtem Klick, während die Schnellaktionen sichtbar sind, per Desktop-Test.
- [ ] Ordner anlegen per Desktop-Test: Ordner und Unterordner entstehen auf dem Testserver,
  nur lesendes Konto sperrt die Aktion, ein auf dem Server angelegter Ordner erscheint ohne Neustart.
- [ ] Popup per Desktop-Test: neue Mail auf dem Testserver → Popup mit Absender und Ausschnitt,
  „Lesen“ zeigt die Mail im Hauptfenster, „Löschen“ legt sie in den Papierkorb, nur lesendes Konto
  sperrt „Löschen“.

## Ergebnis

(offen)
