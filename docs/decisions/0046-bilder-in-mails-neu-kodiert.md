# 0046 — Bilder in Mails nur neu kodiert, im Helfer

- **Datum:** 24.11.2026
- **Anlass:** Sicherheitsprüfung Stufe 15, Befund C-4 (`docs/SECURITY_MODEL.md`). Inline-Bilder
  (`cid:`), freigegebene externe Bilder und `data:`-URLs im HTML einer Mail gingen als erkannte
  Bytes unverändert an die WebView, deren Bilddecoder des Systems (ImageIO/WebKit unter macOS,
  WebView2 unter Windows) sie lasen. Eine Lücke in diesen Decodern war ohne Klick erreichbar, weil
  Inline-Bilder sofort angezeigt werden. Der Teil-Fix (BMP, AVIF, ICO abgewiesen) ließ PNG, JPEG,
  GIF und WebP durch; das Neukodieren war gemessen (68–69 ms je Newsletter, einkernig) und offen.
- **Status:** entschieden vom Projektkoordinator, umgesetzt von der ausführenden Sitzung.
- **Code:** `crates/mail-content/src/reencode.rs` (Neukodieren, Stapel, `ReencodedImages`),
  `crates/mail-content/src/html.rs` (Bilder nur über `RenderOptions::images`, Liste
  `RenderedHtml::pending_images`), `crates/mail-content/src/attachment_preview.rs` (gemeinsame
  Dekodier- und Kodierschritte), `crates/mail-mcp/src/preview.rs` (`--reencode-images`,
  `reencode_isolated`), `crates/mail-engine/src/reading/images.rs` (Stapel, Cache) und
  `reading/mod.rs` (`render_body`, `editor_images`), `crates/mail-engine/src/compose/inline.rs`
  und `compose/snippets.rs` (Bilder im Editor).

## Entscheidung

Kein Byte eines Bildes aus einer Mail erreicht die Bilddecoder des Systems. Jedes Bild, das eine
Darstellung zeigt, wird in Rust dekodiert und neu kodiert, und das im Helfer `mail-mcp`, nicht im
App-Prozess.

- **Wo:** im selben isolierten Helfer wie die Anhangsvorschau (0043), als eigener Modus
  `mail-mcp --reencode-images`, unter denselben Grenzen und derselben Überwachung (20 s, 512 MiB
  residenter Speicher, alle 50 ms geprüft). Ein Aufruf je Darstellung (ein Stapel), nicht je Bild,
  damit der Prozessstart einmal zählt. Der Helfer verteilt den Stapel auf höchstens acht Kerne
  (`std::thread`, keine neue Abhängigkeit); was gleichzeitig dekodiert wird, darf zusammen
  höchstens 384 MiB brauchen (am Kopf geschätzt), das größte Bild beginnt zuerst.
- **Ausgabe:** Standbilder als PNG (wo ein Pixel transparent ist) oder JPEG (Qualität 85), wie die
  Anhangsvorschau, höchstens 2048 px je Seite. Animierte GIFs bleiben animiert: Einzelbilder
  dekodiert und mit `image` als GIF neu geschrieben, Schleifenzahl übernommen (ein GIF ohne
  Schleifen-Erweiterung läuft wie im Browser einmal). Animiertes WebP wird ein animiertes GIF
  (Quantisierer von `gif`, Geschwindigkeit 10). Grenzen: höchstens 300 Einzelbilder, 12 Mi Pixel
  über alle Einzelbilder, 16 MiB GIF, Leinwand höchstens 2048 px; darüber, bei nur einem
  Einzelbild oder einem Abbruch nach dem ersten, zeigt die Mail das erste Einzelbild als
  Standbild. Ein defektes Einzelbild nach den ersten beiden beendet die Animation dort.
- **Abgelehnt:** Was der Helfer ablehnt (zu groß, defekt, kein PNG/JPEG/GIF/WebP) oder woran der
  Stapel scheitert (Abbruch nach Zeit oder Speicher, Absturz, unlesbare Antwort), wird gezeigt
  wie jedes Bild, das sich nicht zeigen lässt: das Element bleibt ohne Quelle. Nie die
  ursprünglichen Bytes.
- **Zweiphasig:** `render_html` zeigt ein Bild nur, wenn `RenderOptions::images` (Schlüssel: die
  ursprünglichen Bytes) eine Neukodierung dafür hat; sonst bleibt es weg und steht in
  `RenderedHtml::pending_images`. Der Kern rendert einmal mit dem, was im Cache liegt, schickt die
  fehlenden Bilder (und die eben geladenen externen) in einem Stapel an den Helfer und rendert
  ein zweites Mal. Größen- und Formatprüfung kommen vor dem Nachschlagen, also verlangt die erste
  Darstellung genau, was die zweite braucht. Eine Antwort des Helfers kommt nur in ein Dokument,
  wenn sie genau die Form `data:image/(png|jpeg|gif);base64,…` hat (`is_reencoded_data_url`).
- **Cache:** im Kern, im Speicher, nach einem Schlüssel aus zwei SipHash-Werten der Bytes mit
  zufälligen Schlüsseln je Prozess und der Länge (von außen nicht zu Kollisionen zu bringen);
  höchstens 64 MiB Data-URLs und 4096 Einträge, die am längsten unbenutzten gehen zuerst.
  Ablehnungen werden mitgemerkt (ein feindliches Bild läuft nicht bei jeder Anzeige wieder 20 s),
  ein Helfer, der nicht starten konnte, nicht. Eine Mail zahlt beim ersten Anzeigen.
- **Ohne Helfer:** Tests und Entwicklungs-Builds ohne die Datei (`ReadingServices::preview_helper`
  ist `None`) kodieren mit demselben Code im Prozess neu, auf dem Blocking-Pool. Ein Release-Build
  übergibt den Pfad immer; fehlt die Datei, startet der Stapel nicht und die Bilder fehlen.

## Editor

Antworten und Weiterleitungen tragen keine empfangenen Bilder in den Editor: das Zitat nimmt aus
fremdem HTML alle Bilder heraus (`html_blocks_with(…, false)`), Inline-Teile einer Weiterleitung
werden Anhänge (Vorschau über den Helfer). Der Editor zeigt aber Inline-Bilder eines Entwurfs
(eigene Dateien; behält ein Entwurf eines anderen Programms aus dem Entwürfe-Ordner seine Bilder,
woran am 24.11.2026 gearbeitet wird, auch Bytes vom Server) und die Bilder von Signaturen (eigene
Dateien, verschlüsselt über den Einstellungs-Sync). Damit die Regel ohne Ausnahme gilt,
laufen auch sie durch denselben Weg (`Reading::editor_images`, ein Stapel, derselbe Cache):
`inlineImageData` und `signatureImageData` liefern die Neukodierung; Entwurf und Signatur behalten
und versenden ihre eigenen Bytes. Ein Bild, das sich nicht neu kodieren lässt, fehlt im Editor
(`inlineImageData` antwortet mit einem Fehler, `signatureImageData` lässt es aus).

## Messung

Mac mini M4 (10 Kerne), andere Sitzungen bauten gleichzeitig (Load 6–19). Der Newsletter aus
`crates/mail-content/tests/newsletter/mod.rs`: 32 Bilder, 7,1 Megapixel, 1.215 KiB. Bester Wert
mehrerer Durchläufe, Spannen über mehrere Läufe.

| Weg | Release | Debug |
|---|---|---|
| bisher (erkennen, Base64) | 1,2 ms | 20–30 ms |
| neu kodiert, nacheinander im Prozess | 66–72 ms | 1.632–1.648 ms |
| neu kodiert als Stapel im Prozess (`reencode_images`) | 13–18 ms | 312–351 ms |
| Stapel durch den Helfer, Start eingeschlossen (`reencode_isolated`) | 18–21 ms (Median 19–23 ms, unter höherer Last bis 33 ms) | 364–405 ms (Median 386–713 ms) |

- Der erste Start des Helfers in einem Prozess kostet einmalig 77–100 ms (Release; Debug 417 ms),
  jeder weitere 2–4 ms. Es ist der erste Start dieses Programms, nicht Prozessstarts allgemein
  (`/bin/echo` als erstes Kind: 1–3 ms; aus der Shell antwortet der Helfer in unter 10 ms). In
  der App fällt das einmal je Sitzung an, beim ersten Bild oder der ersten Vorschau; auch dann
  bleibt der Newsletter mit rund 100–120 ms unter 150 ms.
- Animationen (Release, im Prozess): ein Banner 600×200 mit 24 Einzelbildern (≤ 256 Farben)
  56–59 ms, rund 20 ms je Megapixel; ein animiertes WebP 400×300 mit 12 fotoähnlichen
  Einzelbildern 153–162 ms, rund 100 ms je Megapixel, weil der Quantisierer läuft
  (Geschwindigkeit 30 statt 10 spart nur 20 %). Im Debug-Build 1,2 s und 1,3 s. Die Grenze von 12 Mi Pixeln hält eine Animation im Release-Build unter etwa 0,25 s
  (GIF) bzw. 1,3 s (WebP); im Debug-Build ist sie rund 20-mal langsamer.
- Messen: `cargo test -p mail-content --test inline_reencode_cost --release -- --ignored
  --nocapture` und `cargo test -p mail-mcp --test reencode_isolation --release -- --ignored
  --nocapture` (ohne `--release` für den Debug-Build). Einzelheiten in
  `docs/testing/performance.md`.

## Erwogen, nicht gewählt

- **Im App-Prozess dekodieren** (auch nur „kleine Symbole“): verworfen; ein Decoder ohne eigene
  Grenzen gehört in den Helfer (C-2, C-9).
- **Ein Helfer je Bild:** 32 Prozessstarts je Newsletter; der Stapel kostet einen.
- **Ein dauerhaft laufender Helfer:** spart nur die 2 ms des Starts (nach dem ersten), aber ein
  Absturz oder Abbruch träfe alle wartenden Darstellungen, und die Überwachung je Stapel wäre
  schwerer. Nicht nötig.
- **Neukodierte Bilder mit dem Mailtext speichern:** würde auch das erste Anzeigen nach einem
  Neustart sparen, braucht aber Speicherplatz, eine Migration und Aufräumen. Bei 20 ms je
  Newsletter nicht nötig; möglich, falls langsamere Rechner es verlangen.
- **Den Helfer beim Start einmal vorwärmen** (z. B. `mail-mcp --version` im Hintergrund), um die
  einmaligen 77–100 ms des ersten Helferstarts vor die erste Mail zu ziehen: nicht umgesetzt,
  weil das Budget auch so hält; eine kleine Verbesserung für später.

## Offen und ungeprüft

- Windows: Helfer, Threads und `tasklist` sind für Windows geschrieben, dort aber nicht gelaufen;
  ebenso WebView2 mit den neu kodierten Bildern.
- Die Desktop-Tests (`app/tests/e2e`, GUI) liefen hier nicht. Sie zählen `data:`-Bilder im Rahmen
  (`reading.e2e.mjs`, gleich der Zahl in `X-Corpus-Check`: unverändert 3 für
  `related-cid-data.eml`, im Korpus-Test geprüft) und prüfen `data:image/(png|jpeg)` nur für
  Anhangsvorschauen; ein Bild wird jetzt eventuell als JPEG statt PNG gezeigt.
- APNG zeigt nur das Standbild (das `image`-Standardbild), wie bisher in der Anhangsvorschau.
