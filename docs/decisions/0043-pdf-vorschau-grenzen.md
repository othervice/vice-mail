# 0043 — PDF-Vorschau: Grenzen für Speicher und Zeit (offen)

- **Datum:** 23.11.2026
- **Anlass:** Sicherheitsprüfung Stufe 15, Befund C-2 (`docs/SECURITY_MODEL.md`, 7.2). hayro
  entpackt Streams einer PDF ohne Obergrenze, und die Vorschau hat keine Zeitgrenze (sie läuft auf
  dem Blocking-Pool und lässt sich nicht abbrechen). Eine kleine PDF kann beim Öffnen ihrer
  Vorschau den Speicher erschöpfen; ein Abbruch wegen Speichermangels beendet die App und lässt
  sich nicht abfangen. Ein Klick ist nötig.
- **Status:** entschieden von der ausführenden Sitzung: **Code bleibt, wie er ist**; C-2 bleibt
  offen, mit der Empfehlung unten.
- **Code:** keiner geändert. Betroffen: `crates/mail-content/src/attachment_preview.rs`
  (`pdf_preview`), `crates/mail-engine/src/reading/` (Vorschau auf dem Blocking-Pool).

## Messung

Proben (nicht im Repo, Erzeugung unten): eine Seite, deren Inhalts-Stream aus Nullbytes hinter
zwei `FlateDecode`-Filtern besteht. Abgespielt mit `replay_preview_file`
(`crates/mail-content/tests/fuzz.rs`), Release-Build, Apple M4, gemessen mit `/usr/bin/time -l`:

| Probe | Größe | entpackt | Spitze Speicher (RSS) | Zeit |
|---|---|---|---|---|
| `flate-bomb.pdf` | 1.016 Byte | 256 MiB | 310 MB | 0,09–0,36 s |
| `flate-bomb-1g.pdf` | 2.253 Byte | 1 GiB | 1.115 MB | 0,37 s |

Der Speicher wächst linear mit der entpackten Größe (der Ausgabepuffer verdoppelt sich, dazu die
Daten jeder Filterstufe). Jedes weitere GiB kostet gut ein GB; eine Probe von wenigen KB, die
8 oder 16 GiB entpackt, beendet die App auf den meisten Rechnern. Die Probe der Prüfung (1.121
Byte, 578 MB) passt in diese Reihe. Erzeugt mit Python (`zlib`):

```python
inner = zlib.compress(b"\0" * (256 * 1024 * 1024), 9)  # bzw. als Strom für 1 GiB
outer = zlib.compress(inner, 9)
# Inhalts-Stream: << /Length … /Filter [/FlateDecode /FlateDecode] >> stream outer endstream
```

## Stand bei hayro (23.11.2026)

- hayro 0.9.0 (2.11.2026) ist die neueste Version auf crates.io. `hayro-syntax` entpackt Flate
  und LZW ohne Grenze (`filter/lzw_flate.rs`); `InterpreterSettings` und `RenderSettings` haben
  keine Grenzen.
- Auf `main` (noch nicht veröffentlicht) wurde der Flate-Decoder neu geschrieben (miniz_oxide
  bzw. zlib-rs, 6.11.2026); der Ausgabepuffer wächst weiter ohne Grenze. Neu sind
  Speichergrenzen und Abbrechen für JBIG2, CCITT und JPEG 2000 (7.–12.11.2026). Grenzen sind
  dort also willkommen, für Streams gibt es sie noch nicht.
- **Berichtigung:** `hayro` selbst hat `#![forbid(unsafe_code)]`, `hayro-syntax` 0.9.0 aber
  eine `unsafe`-Stelle (`page.rs`: `core::mem::transmute` für die selbstreferenzielle
  Seitenliste). `docs/SECURITY_MODEL.md` (Abschnitt 8) und die Stack-Tabelle sagten „reines,
  sicheres Rust“; beides ist korrigiert.

## Varianten

**(a) Vorschau in einem Kindprozess.** Ein Hilfsprogramm im Vorschau-Modus (`mail-mcp`, das
schon mit der App ausgeliefert wird, oder die App selbst mit einem eigenen Argument) bekommt die
Bytes über stdin und gibt das Ergebnis über stdout zurück. Der Kern startet es je Seite, beendet
es nach einer festen Zeit (etwa 10 s) und überwacht den Speicher von außen.
- Deckt alles ab: jeden Decoder (Flate, LZW, Bilder, Schriften, Muster), Rechenzeit, Abstürze
  und Abbrüche wegen Speichermangels. Die App selbst bleibt stehen.
- Speichergrenze von außen: macOS setzt `RLIMIT_AS`/`RLIMIT_DATA` für `mmap`-Speicher nicht
  durch, Windows bräuchte ein Job-Objekt (Win32-Aufrufe, also `unsafe` oder eine weitere
  Abhängigkeit). Bleibt eine Überwachung durch Abfragen des Speichers alle 10–20 ms, plattform-
  übergreifend nur mit einer neuen Abhängigkeit (etwa `sysinfo`, MIT, aktiv gepflegt; Eintrag in
  der Stack-Tabelle nötig). Bei 2–3 GB/s Wachstum überschreitet der Prozess die Grenze vor dem
  Beenden um einige zehn MB.
- Kosten: ein Prozessstart je Seite (geschätzt einige zehn ms, nicht gemessen), Code für Start,
  Rohr, Zeitgrenze und Überwachung, Weitergabe des Hilfsprogramm-Pfads von der Shell an den
  Kern. Die Windows-Seite ist hier nicht testbar.

**(b) Gepatchtes `hayro-syntax` über `[patch.crates-io]`.** Grenze in den Decodern (Flate, LZW,
RunLength: abbrechen, sobald die Ausgabe z. B. 128 MiB übersteigt) und eine Pixelgrenze am Anfang
von `Stream::decoded_image` (Breite × Höhe, etwa 64 Megapixel wie bei Bildvorschauen). Der Patch
wäre rund 40 Zeilen.
- Widerspricht den Projektregeln: Eine Kopie von `hayro-syntax` im Repo brächte dessen
  `unsafe`-Block mit (Regel: `unsafe` nur in `webview2-guard`); ein Fork als Git-Abhängigkeit
  ist durch „nur crates.io“ ausgeschlossen. Den `transmute` zu entfernen hieße, die Seitenliste
  umzubauen.
- Deckt nur entpackte Größe und Bildmaße ab, nicht die Rechenzeit und nicht Speicher, den
  `hayro-interpret` selbst anlegt (z. B. füllt es Bilder mit zu wenig Daten auf, Zeilenlänge aus
  der angegebenen Breite).
- Muss bei jeder hayro-Version neu angewandt werden.

**(c) Vorprüfung in eigenem Code vor hayro.** Alle Streams mit begrenzten Decodern durchgehen
(z. B. `miniz_oxide::inflate::decompress_to_vec_zlib_with_limit`) und zu große ablehnen. Braucht
einen eigenen PDF-Leser für Objekt- und Querverweis-Streams und Inline-Bilder in Inhalts-Streams,
also einen zweiten PDF-Parser als neue Angriffsfläche. Verworfen.

**(d) Grenzen in hayro selbst.** Eine Option für die größte entpackte Stream-Größe und die
größte Bildfläche, wie sie hayro für JBIG2 schon eingeführt hat. Die beste Lösung auf Dauer;
heute nicht verfügbar.

## Entscheidung und Empfehlung

In dieser Sitzung wird nichts davon umgesetzt: (b) verstößt gegen die `unsafe`-Regel und ließe
Rechenzeit und `hayro-interpret` offen, (c) ist verworfen, (d) gibt es nicht, und (a) ist mit
Prozessstart, Überwachung, neuer Abhängigkeit und ungetesteter Windows-Seite mehr, als sich hier
sauber machen lässt. C-2 bleibt offen (mittel, ein Klick nötig).

Empfohlen, in dieser Reihenfolge:

1. **(a) als nächster Schritt vor einer breiten Verteilung:** Vorschau-Modus in `mail-mcp`,
   Zeitgrenze und Speicherüberwachung im Kern, die bestehende Vorschau als Rückfall nur in Tests.
   Mit derselben Isolation ließen sich später auch die Bilddecoder (C-9) und, falls gewünscht,
   das Neukodieren der Inline-Bilder (C-4) absichern.
2. **(d) parallel anstoßen:** eine Grenze für entpackte Streams und Bildflächen bei hayro
   vorschlagen; sobald eine Version sie hat, die Grenzen in `pdf_preview` setzen.

## Nachtrag 23.11.2026: Variante (a) umgesetzt

Auf Wunsch des Projekts noch am selben Tag umgesetzt, ohne neue Abhängigkeit und ohne `unsafe`.

- **Kind:** `mail-mcp --render-preview <image|pdf|text> <Seite>` (`crates/mail-mcp/src/preview.rs`,
  `run_child`). Liest die Datei von stdin (höchstens `MAX_PREVIEW_INPUT_BYTES`), rendert mit
  `mail_content::render_preview` und schreibt eine JSON-Zeile nach stdout: die Vorschau (PNG oder
  JPEG als Data-URL) oder den Grund ohne Vorschau. `mail-mcp` liegt in beiden Installern schon
  neben der App.
- **Eltern:** `render_isolated` startet das Kind je Vorschau, schreibt die Bytes, liest die
  Antwort (höchstens 96 MiB) und schaut alle 50 ms nach: nach 20 s wird das Kind beendet, ebenso
  über 512 MiB residentem Speicher. Den Speicher liest es mit `/bin/ps -o rss= -p <pid>` (macOS,
  Linux) bzw. `tasklist /FI "PID eq <pid>" /FO CSV /NH` (Windows). Zwischen zwei Blicken kann ein
  schnell wachsendes Kind das Budget um das überschreiten, was es in der Zeit anlegt.
- **Fehler:** Zeit oder Speicher überschritten: `preview_too_large` mit dem Grund im Detail. Kind
  abgestürzt, ohne Speicher beendet oder unlesbare Antwort: `preview_damaged`. Helfer fehlt oder
  startet nicht: `preview_unsupported`. Die App läuft weiter; keine neuen Fehlercodes.
- **Im Kern:** `ReadingServices::preview_helper`. Bild- und PDF-Vorschauen (auch aus `.eml`-Dateien)
  laufen im Helfer, Text weiter im Kern. Die Shell setzt den Helfer neben der ausführbaren Datei;
  ein Release-Build ohne ihn lehnt diese Vorschauen ab, statt sie im Prozess zu rendern. Ein
  Entwicklungs-Build ohne `mail-mcp` (`cargo build -p mail-mcp`) rendert sie wie bisher in der App;
  Tests im Kern ebenso.
- **Gemessen** (`crates/mail-mcp/tests/preview_isolation.rs`, M4): die Probe von 2.253 Byte
  (1 GiB entpackt) endet im Release-Build nach 0,35 s als `preview_too_large`, im Debug-Build nach
  1,9 s; der Speicher des Elternprozesses bleibt gleich (2,9 bzw. 3,7 MB vorher wie nachher). Eine
  einseitige PDF braucht über den Helfer 92–102 ms (Release) bzw. 0,45 s (Debug), darin Start des
  Prozesses und Laden von hayro.
- **Nur unter Windows und dort ungetestet:** `tasklist` als Speicherquelle (Ausgabe je nach
  Sprache mit `.` oder `,` als Tausendertrenner, beides wird gelesen) und `mail-mcp.exe` als Kind.
  Der Parser für die `tasklist`-Ausgabe ist getestet, der Aufruf nicht.
- Bleibt: die Rechenzeit eines Kinds bis zur Zeitgrenze (20 s, ein Kern), und das Kind läuft mit
  den Rechten des Nutzers (keine Sandbox). (b), (c) und (d) entfallen; (d) bleibt als Wunsch an
  hayro sinnvoll.
