# Stufe 03 — Nachricht lesen

**Ziel:** Mails lassen sich öffnen und sicher lesen, mit Anhängen und auch ohne Netz. HTML aus Mails
kann weder Code ausführen noch unbemerkt nach außen funken.

**Voraussetzung:** Stufe 02 fertig. Abschnitt „Sicherheitsgrenzen“ in `docs/ARCHITECTURE.md` und die
Entscheidung aus Spike D gelesen.

## Aufgaben

- [x] **Body holen und ablegen.** Beim Öffnen die Mail mit `BODY.PEEK[]` holen (verändert den Gelesen-Status nicht). Bei sehr großen Mails zuerst die Textteile, Anhänge erst auf Wunsch. Ablage komprimiert als Datei im Datenverzeichnis, atomar geschrieben, Verweis in der Datenbank. Kurz gegen Ablage in der Datenbank messen und die Wahl festhalten.
- [x] **Vorausladen.** Im Hintergrund die Bodies der letzten 30 Tage für Posteingang, Gesendet, Entwürfe und Markiertes laden. Zeitraum einstellbar (7, 30, 90, 365 Tage, alles), mit Größenobergrenze.
- [x] **Parsen.** Mit `mail-parser` in eine eigene Struktur: Text, HTML, Anhänge, Inline-Teile, Kopfzeilen. Zeichensätze, kodierte Kopfzeilen, kaputte Mails ohne Absturz. Vorschautext erzeugen und in `message` speichern; die Liste zeigt ihn ab jetzt.
- [x] **HTML-Pipeline.** Wie in der Architektur: bereinigen, externe Adressen ersetzen, `cid:`-Bilder lokal auflösen, Anzeige im abgeschotteten `iframe`. Umsetzung nach Spike D (0005 und Architektur, Sicherheitsgrenzen 3, 5, 6): `sandbox="allow-same-origin"` ohne `allow-scripts`, Pflicht-Meta-CSP in jedem `srcdoc`, CSP der App wie dort beschrieben, Link-Abfang nur über den Navigations-Hook in Rust, der Bereiniger schreibt jedes `href` auf einen Platzhalter um und entfernt `<link>`, `target`, `ping`, `download`. Höhe des Frames automatisch anpassen (ob `ResizeObserver` im Frame feuert, ist ungeprüft).
- [x] **Externe Inhalte.** Standard blockiert, mit Hinweisleiste: einmal laden oder für diesen Absender immer laden. Freigegebene Bilder lädt der Rust-Kern ohne Cookies und Referrer, mit Größen- und Typgrenzen.
- [x] **Links.** Ein Klick zeigt das echte Ziel. Warnung, wenn Linktext und Ziel verschiedene Domains nennen oder die Domain Verwechslungszeichen enthält. Öffnen im System-Browser, nur `http`, `https`, `mailto`.
- [x] **Anhänge.** Liste mit Name, Größe, Typ. Speichern über den Systemdialog, Dateiname bereinigt. Öffnen nur nach Klick; bei ausführbaren Typen eine Warnung. Nichts öffnet sich von selbst.
- [x] **Lesebereich.** Nach `docs/DESIGN.md`: Kopf mit Absender, Empfängern, Zeit, Kontofarbe; HTML auf eigenem Hintergrund, reiner Text im Thema der App; Umschalter HTML/Text.
- [x] **Testkorpus.** Feindliche Mails unter `testmail/corpus/hostile/`: Script-Tags, Ereignis-Attribute, `javascript:`-Links, SVG mit Script, CSS mit `url()` und `@import`, `meta refresh`, Formulare, `base`, eingebettete Objekte und Frames, Zählpixel. Dazu Anhänge mit gefährlichen Namen (`../../x`, `C:\…`, `CON`, Null-Bytes, Richtungsumkehr-Zeichen). Dazu aus Spike D: `<link rel=preconnect>`, `target=_blank`, `mailto:`-Links, eigene Schemata und `<iframe src="/index.html">`.

## Abnahme

- [x] eine lokal vorhandene Mail öffnet sich in unter 100 ms (gemessen)
- [x] Unit-Tests: Die bereinigte Ausgabe des gesamten feindlichen Korpus enthält keines der verbotenen Elemente
- [x] Desktop-Test mit WebdriverIO: Beim Anzeigen des Korpus läuft kein Script und der lokale Zähl-Server registriert null Zugriffe; nach Freigabe eines Absenders werden dessen Bilder geladen (Muster aus `spikes/tauri-app/e2e/`: Zähl-Server für TCP-Verbindungen, Script-Detektoren, echte Mausklicks, Positivkontrollen; nach der Bereinigung ist die Liste bekannter Lecks leer)
- [x] Tests: Jeder gefährliche Dateiname wird zu einem harmlosen; ein gespeicherter Anhang hat dieselbe Prüfsumme wie das Original
- [x] vorausgeladene Mails lassen sich bei gestopptem Testserver öffnen
- [x] kaputte Mails aus dem Seed führen zu einer lesbaren Anzeige oder einer klaren Meldung, nie zu einem Absturz
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Als gelesen markieren und alle anderen Aktionen (Stufe 4)
- Thread-Ansicht (Stufe 7), Kalendereinladungen und Abmelden (Stufe 9)
- Drucken und Export (Stufe 9)

## Der Mensch prüft danach

- Auf dem echten Konto verschiedene Mails lesen: Newsletter, Rechnungen mit PDF, Mails mit Umlauten und Bildern. Was sieht falsch aus?

## Ergebnis

Ausgeführt am 22./23.11.2026; `scripts/check.sh` (499 Rust-, 446 Vitest-Tests) und
`pnpm --dir app test:e2e` (4 Spec-Dateien) am 23.11. von der Hauptsitzung in einem sauberen Worktree
(Commit 9ffdfb8) grün ausgeführt. (Hauptsitzung mit Subagenten: Inhalt/Bereinigung, Grundstein Body-Ablage,
Lese-Oberfläche, Engine/Shell, Desktop-Abnahme). Entscheidungen: 0009 (Body-Ablage), 0011
(WebView2-Frame-Navigation, Option B).

**Nachgewiesen**
- Öffnen unter 100 ms: im Kern Median 0,6 ms, p95 1,5 ms über 2.000 Aufrufe (Newsletter 1,2–1,6 ms);
  in der echten App vom Klick bis Kopf und Inhalt im Dokument Median 5–6 ms, p95 11–14 ms, Maximum
  27 ms über 40 lokal vorhandene Mails (Fenster verborgen, also ohne tatsächliches Zeichnen gemessen).
- Feindlicher Korpus (`testmail/corpus/hostile/`, 19 Mails inklusive aller 53 Vektoren aus Spike D):
  Unit-Test mit Neu-Parsen der Ausgabe, 0 verbotene Elemente in 48 bereinigten Dokumenten; Fuzzing über
  Millionen mutierter Eingaben ohne Panic. In der App (`reading.e2e.mjs`): je Mail 0 TCP-Verbindungen zum
  Zähl-Server, keine Script-Marker, keine Navigation, kein CSP-Verstoß; echte Mausklicks auf Links zeigen
  das echte Ziel mit Warnungen und übergeben nach Bestätigung genau dieses Ziel an den (im Test
  aufzeichnenden) Opener. Freigabe eines Absenders lädt nur dessen Bilder (ohne Referrer und Cookies),
  andere Absender bleiben bei 0. Positivkontrollen für Zähler, Marker, CSP-Recorder.
- Dateinamen: 27 gefährliche Fälle werden harmlos; gespeicherter Anhang (1,5 MB PDF und 5 MB ZIP aus
  einer teilweise geholten Mail) hat dieselbe SHA-256 wie das Original.
- Vorausgeladene Mails öffnen sich bei gestopptem Server (Kern: 813 von 813; App: 6 von 6 Stichproben),
  nicht geladene zeigen eine verständliche Meldung.
- Alle 26 kaputten Seed-Mails: lesbar oder mit Hinweis, kein Absturz (Kern und App).
- Body-Ablage (0009): zstd-Dateien statt BLOBs, gleich schnelles Lesen, Datenbank bleibt klein.

**Abweichungen und Annahmen**
- Link-Abfang nur im Navigations-Hook; der Platzhalter trägt einen Render-Token. Unter Windows über die
  eigene Crate `webview2-guard` (einzige Stelle mit `unsafe`).
- Externe Bilder nie aus privaten, Loopback- oder Link-Local-Netzen (Test-Schalter nur im e2e-Build).
- „Für diesen Absender immer laden“ vertraut der `From`-Adresse, die fälschbar ist; ein Fälscher kann
  so höchstens Tracking-Bilder nachladen lassen.
- Dateidialog über `rfd`, Öffnen über `open` (nur in Rust), nicht über Tauri-Plugins.

**Ungeprüft / offen**
- Windows zur Laufzeit (Link-Abfang über WebView2, Mark-of-the-Web, Dateidialog).
- Nach einem Server-Neustart scheitert das erste Öffnen einer nicht lokalen Mail einmal mit „Verbindung
  verloren“, „Erneut versuchen“ hilft. Ein automatischer Wiederholversuch mit neuer Verbindung folgt.
- „Einmal laden“ bleibt bis zu 30 s sichtbar, wenn die Mail erneut geöffnet wird (Zwischenspeicher der
  Oberfläche); der Kern merkt sich die Freigabe nicht.

**Wartet auf**
- Mensch: auf dem echten Konto verschiedene Mails lesen (Newsletter, Rechnungen mit PDF, Umlaute,
  Bilder). Was sieht falsch aus?
