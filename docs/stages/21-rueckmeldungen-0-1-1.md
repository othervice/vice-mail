# Stufe 21 — Rückmeldungen zu 0.1.1

**Ziel:** Alles, was der Mensch beim Test von 0.1.1 findet, gesammelt umsetzen und in einem Zug
bauen.

**Anlass:** Test von 0.1.1 am 25.11.2026. Der Mensch hat entschieden: erst alle Rückmeldungen
sammeln, dann umsetzen und neu bauen.

**Stand:** Liste vollständig („ok das ist es erstmal, build“, 25.11.2026, 11:18). Ergibt 0.1.2.
Aufteilung: 2+3 (Spalte), 4a+5+6 (Liste), 8+9 (Farben der Mails), 7+10 (System), danach 4
(Abstände) auf dem zusammengeführten Stand.

## Rückmeldungen

### 1 — Windows-Installer startet nicht („NSIS Error: Error launching installer“)

- Gemeldet: der neueste Installer (0.1.1) startet unter Windows nicht.
- Geprüft: Die Datei im Gitea-Release ist vollständig und unverändert (SHA-256 wie in
  `SHA256SUMS`, 19.074.607 Bytes; die NSIS-eigene Prüfsumme ab Byte 512 stimmt, ebenso bei 0.1.0).
- GitHub-CI auf dem Code von 0.1.1 (Lauf 45583550063 auf e9874ca): `check.sh` grün auf macOS und
  auf echtem Windows.
- NSIS zeigt genau diese Meldung nur, wenn der Installer seine eigene Datei nicht öffnen kann
  (`_LANG_CANTOPENSELF` in `Source/exehead/fileform.c`): Start aus einem Netzlaufwerk, einem
  Ordner, den OneDrive nur online hält, einem ZIP, direkt aus dem Download-Fenster, oder ein
  Virenschutz hält die Datei gesperrt.
- Wartet auf: wo die Datei beim Start lag, ob der Start vom Desktop aus mit richtiger Prüfsumme
  geht, ob 0.1.0 auf diesem PC lief. Das Bildschirmfoto vom 25.11. zeigt 0.1.1 (Schnellfilter)
  unter Windows: vermutlich lief der Installer später doch.

### 2 — „Ordner als gelesen markieren“ im Menü eines Ordners

- Gewünscht: im „…“-Menü (und per Rechtsklick) eines Ordners „Ordner als gelesen markieren“.
- Stand im Code: Es gibt keine Aktion für einen ganzen Ordner. Nötig ist eine neue Outbox-Aktion
  (IMAP `UID STORE 1:* +FLAGS (\Seen)`, JMAP `Email/set` über die ungelesenen des Ordners), mit
  sichtbarem Zustand und Rückgängig. Bei nur lesenden Konten gesperrt mit Begründung. Vermutlich
  auch für die Ansichten (vereinter Posteingang, Ungelesen) sinnvoll.

### 3 — Menü eines Ordners ragt links aus dem Bild (Konto bitaxe.de, Posteingang)

- Gemeldet: Klick auf „…“ beim Posteingang des Kontos bitaxe.de zeigt etwas, das links aus dem
  Fenster ragt und nicht lesbar ist.
- Vermutete Ursache: Das Menü einer Zeile der linken Spalte hängt am rechten Rand des „…“-Knopfs
  und wächst mit seinem längsten Eintrag nach links (`OverflowMenu`, `narrow` setzt nur eine
  Mindestbreite von 12rem). Mit den neuen Einträgen aus Stufe 18 und 19 („Neuer Unterordner…“,
  „Als mbox exportieren …“, „mbox importieren …“) und den längeren deutschen Texten wird es
  breiter als der Platz bis zum linken Fensterrand.
- Bildschirmfoto vom 25.11. (Windows, dunkel): danach ist die ganze linke Spalte waagerecht
  verschoben, ihre Texte sind links abgeschnitten („…wort“, „…hänge“, „…sichten“). Das zu breite
  Menü lässt die Spalte, die nur senkrecht scrollen soll, auch waagerecht scrollen, und sie
  bleibt so stehen.
- Lösung (geplant): Menüs der Spalte mit fester Breite und umbrechenden Texten, die Spalte scrollt
  nie waagerecht (`overflow-x: clip`), dazu eine Begrenzung auf den sichtbaren Bereich für alle
  Menüs; per Desktop-Test in Deutsch und Englisch, bei schmalster Spalte, prüfen.

### 4 — Abstände: Platz links neben den Initialen, allgemeiner Feinschliff

- Gemeldet: in der Liste viel Platz links neben den Initialen; die Abstände allgemein (Knöpfe,
  Überschriften, Leisten) sollen stimmen, damit das Programm „polished“ aussieht.
- Ursache des Platzes: eine eigene Spalte für den Ungelesen-Punkt, die beim Überfahren zum
  Auswahlkästchen wird. Ohne ungelesene Mail bleibt sie leer.
- Plan: Punkt und Auswahlkästchen ohne eigene breite Spalte (Kästchen erscheint beim Überfahren
  und bei Mehrfachauswahl an der Stelle des Bildes, wie bei Gmail; der Punkt schmal davor oder am
  Bild). Danach ein Durchgang über alle Ansichten in Hell und Dunkel, Mac und Windows: 4-px-Raster,
  gleiche Höhen von Knöpfen und Leisten, Abstände von Überschriften, Ausrichtung an Kanten;
  geprüft an Bildschirmfotos, Abweichungen per Test festgehalten.

### 5 — Rechtsklick-Menü für Mails

- Gewünscht: Rechtsklick auf eine Mail öffnet ein ausführliches Menü im Stil von Thunderbird
  (Bild vom 25.11.: oben eine Zeile mit Symbolen, darunter Öffnen ›, Verschieben in ›, Kopieren
  in ›, Schlagwörter ›, Markieren ›, Speichern unter…, Drucken…). Die Leiste beim Überfahren
  bleibt wie sie ist.
- Plan: oben Symbole für Antworten, Archivieren, Spam, Löschen; darunter Öffnen (Lesebereich,
  eigenes Fenster) ›, Antworten/Allen/Weiterleiten/Als Anhang/Umleiten ›, Als gelesen bzw.
  ungelesen, Markieren ›, Anpinnen, Später ›, Aufgabe, Zum Lesen, Erinnern ohne Antwort ›,
  Verschieben in ›, Kopieren in ›, Schlagwörter ›, Als neu bearbeiten, Quelltext, Als .eml
  speichern, Drucken. Wirkt auf die Auswahl, wenn die angeklickte Mail dazugehört. Auch mit der
  Kontextmenü-Taste und Umschalt+F10; Untermenüs per Tastatur. In jeder Liste: Ordner,
  vereinter Posteingang, Ansichten, Suchergebnisse.

### 6 — Leiste beim Überfahren fehlt im vereinten Posteingang

- Gemeldet: in „Alle Posteingänge“ erscheinen die Schnellaktionen beim Überfahren nicht, nur im
  Posteingang eines Kontos. Fehler; dort auch das Rechtsklick-Menü aus 5.

### 7 — Taskleisten-Symbol und Logo scharf, Logo in den neuen Farben

- Gemeldet: die Zahl neuer Mails am Taskleisten-Symbol (Windows) ist unscharf; das ganze Logo
  soll in besserer Auflösung „crisp“ sein; Logo und Farben sollen zum neuen Farbkonzept
  (Blau, GitHub-Dunkel) passen.
- Plan: das Zahlen-Overlay in der Größe der Bildschirmskalierung zeichnen (16/20/24/32 px bei
  100–200 %), Ziffern pixelgenau; das App-Symbol mit allen Größen der ICO (16, 20, 24, 32, 40,
  48, 64, 256) und eigens gezeichneten kleinen Größen; macOS-Symbol entsprechend. Logo in den
  neuen Farben: Entwurf zur Freigabe durch den Menschen, dann `docs/brand/`, Symbole, Wortmarke;
  Entscheidung festhalten (löst das Orange aus `docs/brand/README.md` ab).
- Entwurf am 25.11.2026 gezeigt (gleiche Form, Farben Blau und GitHub-Dunkel); vom Menschen
  freigegeben: „Blau wie im Entwurf“.

### 8 — Mails mit hellem und mit dunklem Hintergrund

- Gemeldet: manche Mails haben dunklen, manche weißen Hintergrund. Wie wird das einheitlich?
- Ursache: Mails ohne eigene Farben (reiner Text, schlichtes HTML) folgen dem dunklen Modus; HTML
  mit eigenen Farben (Newsletter, Signaturen) bekommt seinen weißen Grund, damit dunkle Schrift
  lesbar bleibt (so seit Stufe 3, siehe `docs/DESIGN.md`).
- Plan: Einstellung „Mails im dunklen Modus“: angepasst (Standard) oder immer hell. Angepasst
  heißt: der Kern rechnet die Farben der Mail beim Aufbereiten um (Helligkeit umgekehrt, Farbton
  bleibt, Bilder unverändert), im bestehenden Sicherheitsweg, ohne Skript; Mails, die selbst
  `color-scheme` angeben, werden respektiert. Pro Mail „Original ansehen“.

### 9 — Keine dunkle-Modus-Farben in gesendeten Mails

- Gemeldet: verhindern, dass helle Schrift aus dem dunklen Modus beim Empfänger hell auf weiß
  ankommt.
- Plan: der Composer schreibt keine Text- oder Hintergrundfarbe in die Mail, außer man wählt
  ausdrücklich eine; eingefügter Text verliert Farben, die nur aus dem dunklen Modus stammen;
  Zitate behalten die Originalfarben (nicht die umgerechneten aus 8); ausgehende Mails sagen
  `color-scheme: light dark`, damit Programme der Empfänger selbst anpassen dürfen. Test: das
  gesendete HTML enthält ohne ausdrückliche Wahl keine Farbe.

### 10 — Symbol im Infobereich, Weiterlaufen ohne Fenster, Autostart

- Vom Menschen am 25.11. aus den Vorschlägen gewählt (nicht gewählt: Windows-11-Optik,
  Einstellungen durchsuchen, Ziehen in Explorer/Finder).
- Plan: Symbol im Infobereich (Windows) bzw. in der Menüleiste (macOS) mit der Zahl ungelesener
  Mails und einem Menü (Öffnen, Neue Mail, Benachrichtigungen pausieren, Beenden). Fenster
  schließen beendet die App nicht mehr, sondern lässt sie im Hintergrund weiterlaufen (Abgleich,
  Popup, Benachrichtigungen); beim ersten Schließen ein einmaliger Hinweis, wie man ganz beendet.
  Einstellung „Mit dem System starten“ (Standard aus), dann ohne Fenster im Hintergrund.
  Speicherziel im Hintergrund (0048) prüfen: ohne Fenster kein WebView, wenn möglich.

## Ergebnis

(offen, sammeln)
