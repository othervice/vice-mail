# Stufe 21 — Rückmeldungen zu 0.1.1

**Ziel:** Alles, was der Mensch beim Test von 0.1.1 findet, gesammelt umsetzen und in einem Zug
bauen.

**Anlass:** Test von 0.1.1 am 25.11.2026. Der Mensch hat entschieden: erst alle Rückmeldungen
sammeln, dann umsetzen und neu bauen.

**Stand:** Sammeln. Nichts wird gebaut, bis der Mensch sagt, dass die Liste vollständig ist.

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
  geht, ob 0.1.0 auf diesem PC lief.

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
- Lösung (geplant): Menüs der Spalte mit fester Breite und umbrechenden Texten, dazu eine
  Begrenzung auf den sichtbaren Bereich für alle Menüs; per Desktop-Test in Deutsch und Englisch,
  bei schmalster Spalte, prüfen.

## Ergebnis

(offen, sammeln)
