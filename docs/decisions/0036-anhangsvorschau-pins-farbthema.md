# 0036 — Anhänge (Vorschau, alle speichern, anheften), Farbthema im Kern, ausgeblendete Mails zählen

Stand 23.11.2026. Letzte offene Teile der Stufe 16 (0032, 0033): Aufgabe „Anhänge“, die Kernseite
der „Farbthemen“ (bisher nur lokal in der Oberfläche) und die Zahl der ausgeblendeten Mails
blockierter Absender für den Hinweis in der Liste.

## Kontext

- Anhänge sind nicht vertrauenswürdig (Sicherheitsgrenzen). Bis Stufe 15 konnte man sie nur
  speichern (Systemdialog, Kern schreibt) oder mit der Standard-App öffnen (Kopie, Programme erst
  nach Warnung). Die Stufe verlangt eine Vorschau für Bilder, PDF und Text „in einem gesicherten
  Fenster ohne Netz“, „Alle speichern“ in einen Ordner und angeheftete Anhänge mit eigener Ansicht.
- Die CSP der App erlaubt `img-src 'self' data:`, `object-src 'none'`, `frame-src 'self' http:
  https:` (für die Link-Navigation der Mail-Frames) und `connect-src` nur zur IPC.
- Die Farbthemen der Oberfläche lagen im Web-Storage (markierter Ersatz in `accent.ts`).

## Entscheidungen

### 1. Vorschau: der Kern dekodiert, die WebView zeigt nur, was der Kern selbst erzeugt hat

`previewAttachment(messageId, index, page)` (und `previewEmlAttachment` für geöffnete
`.eml`-Dateien) liefert `AttachmentPreview`:

- **Bilder** (`kind: "image"`): erkannt an den Bytes (PNG, JPEG, GIF, WebP; nie SVG), in Rust mit
  `image` dekodiert (höchstens 64 Megapixel, Seitenlänge höchstens 32.768, Speichergrenze im
  Dekoder), nach EXIF aufgerichtet, auf höchstens 2048 px je Seite verkleinert und **neu kodiert**:
  PNG, wenn ein Pixel Transparenz nutzt, sonst JPEG (Qualität 85). Die WebView bekommt einen
  `data:`-URI dieser neuen Bytes; die Bild-Dekoder des Systems (ImageIO, WIC) sehen die Bytes des
  Anhangs nie, Metadaten (EXIF, ICC, Kommentare) fallen weg. GIF: erstes Bild.
- **PDF** (`kind: "pdf"`): jede Seite wird im Kern mit hayro (reines Rust, `forbid(unsafe_code)`)
  zu PNG gerastert, je Aufruf eine Seite (`page`, 0-basiert), höchstens 200 Seiten, Maßstab
  höchstens 2 und längste Seite höchstens 2048 px. hayro hat keine Script-Engine, keine Formulare
  und keinen Netzzugriff; Links, Aktionen und eingebettete Dateien der PDF haben in einem Rasterbild
  keine Wirkung. Verschlüsselte oder geschützte PDFs: `preview_damaged`.
- **Text** (`kind: "text"`): `text/*` außer HTML/XML, `application/json`, bei
  `application/octet-stream` nach Endung (`.txt`, `.log`, `.csv`, `.tsv`, `.md`, `.json`).
  UTF-8 (BOM entfernt), UTF-16 mit BOM, sonst Latin-1; Steuerzeichen außer Tab und Zeilenumbruch
  entfernt, CRLF und CR werden LF; nach 512 KiB auf Zeichengrenze gekürzt (`truncated`). Die
  Oberfläche zeigt ihn als Text (`<pre>`), nie als Markup.
- **Nie**: HTML, XHTML, XML, SVG, auch wenn nur der Typ oder nur der Name es sagt (`trick.svg` mit
  `image/png`, `page.txt` mit `text/html`). Ebenso alle anderen Formate (Office, Archive,
  Programme). `AttachmentInfo.preview` (`"image" | "pdf" | "text" | null`) sagt der Oberfläche
  vorher, ob es eine Vorschau gibt; die Bytes entscheiden beim Erstellen noch einmal (eine als PNG
  deklarierte HTML-Datei ist `preview_unsupported`).
- **Grenzen und Fehler:** höchstens 32 MiB Eingabe (vor dem Holen nach der angegebenen Größe und
  danach an den Bytes geprüft). Drei neue Fehlercodes mit englischem und deutschem Text:
  `preview_unsupported` (kein Vorschauformat, oder die Bytes passen nicht zum Typ),
  `preview_too_large` (Bytes oder Pixel über der Grenze), `preview_damaged` (nicht dekodierbar,
  verschlüsselt, Seite außerhalb). Eine Panik eines Dekoders auf feindlicher Eingabe wird
  `preview_damaged` (`catch_unwind`), kein Absturz. Gerendert wird im Blocking-Pool von Tokio.
- **Anzeige im Lesebereich** als Dialog über der App (Klick oder Leertaste auf dem Anhang), nicht
  als eigenes Fenster. Die Sicherheit liegt in den Daten, nicht im Fenster: Ein eigenes Fenster
  wäre eine weitere WebView mit derselben CSP und denselben Rechten (die Vorschau braucht die IPC),
  also keine zusätzliche Grenze. Netz ist ausgeschlossen, weil der Inhalt nur aus `data:`-Bildern
  und Text besteht und die CSP der App keine fremden Quellen erlaubt; es gibt keine neue
  Capability und keine Änderung an CSP, Navigations-Hook oder Fenstern.

Verworfen:

- **PDF im PDF-Betrachter der WebView** (eigenes Vorschaufenster, `<embed>`/`<iframe>` mit
  `data:application/pdf`): verlangt `object-src` bzw. `frame-src data:` in der CSP; der Betrachter
  von WebView2 ist PDFium im Chromium-Viewer mit eigener JavaScript-Ausführung für PDF-Skripte, die
  sich aus der App nicht abschalten lässt, und unter WKWebView ist das Verhalten des PDF-Plugins
  (Skripte, Formular-Aktionen, Links) nicht dokumentiert. „Ohne Skripte und ohne Netz“ wäre nicht
  nachweisbar.
- **Bilder unverändert als `data:`** (wie Inline-Bilder in Mails): dann dekodiert der Bild-Stack des
  Systems fremde Bytes; über Bild- und PDF-Dekoder des Systems liefen in den letzten Jahren
  Zero-Click-Angriffe. Neu kodieren in speichersicherem Rust kostet wenig und entfernt Metadaten.
- **Quick Look bzw. Vorschau-Handler des Systems:** nicht auf beiden Plattformen gleich, außerhalb
  der App und ihrer Grenzen.

Neue Abhängigkeiten (Stack-Tabelle): `image` 0.25.10 (nur `png`, `jpeg`, `gif`, `webp`; MIT OR
Apache-2.0, Release März 2026) und `hayro` 0.9.0 (Apache-2.0 OR MIT, Release November 2026, mit
eingebetteten Standardschriften und CMaps). hayro bringt rund 25 Crates mit (u. a. `vello_cpu`,
`skrifa`, `pic-scale`, `zune-jpeg`, `hayro-jbig2`, `hayro-jpeg2000`), alle reines Rust.

### 2. „Alle speichern“

`saveAllAttachments(messageId)` (und `saveAllEmlAttachments(fileId)`): Die Shell zeigt den
Ordnerdialog des Systems (`rfd`, als Sheet, Start im Download-Ordner), nur der Pfad geht an den
Kern. Der Kern schreibt jeden Anhang unter seinem bereinigten Namen (dieselbe Bereinigung wie beim
einzelnen Speichern). **Eine vorhandene Datei wird nie ersetzt:** Der Name wird mit `create_new`
belegt; ist er vergeben (auch durch einen gleichnamigen Anhang derselben Mail), folgt
`Name (2).ext`, `Name (3).ext` … bis 999. Unter Windows bekommt jede Datei die Herkunftsmarke
(Zone 3) wie beim einzelnen Speichern. Ergebnis: `{ folder, saved, renamed }` bzw. `cancelled`.
Ein Fehler bricht ab; was geschrieben ist, bleibt. Anders als das einzelne Speichern ist eine
Datei nicht atomar geschrieben (ein Umbenennen über das Ziel würde ersetzen): ein Absturz kann
eine halbe Datei unter dem neuen Namen hinterlassen, nie eine vorhandene beschädigen.

### 3. Angeheftete Anhänge

- **Lokale Markierung** wie Lese-Liste und Pins, Tabelle `attachment_pin` (Migration 0015): Konto-
  UUID, Message-ID, Index des Anhangs in der Leseansicht, dazu Name, Typ und Größe vom Zeitpunkt des
  Anheftens, damit „Angeheftete Anhänge“ sie ohne Body zeigt. Verweist mit `ON UPDATE CASCADE` auf
  `account(uuid)`. Nichts geht an den Server.
- **Commands:** `setAttachmentPinned(messageId, index, pinned)` (Anheften braucht die
  Anhangsliste des gespeicherten Bodys, die der Lesebereich zeigt; Lösen nicht),
  `getAttachmentPins(messageId)` (angeheftete Indizes für den Lesebereich, ohne die Mail neu zu
  rendern), `listPinnedAttachments(scope: MarkScope)` (neueste zuerst, höchstens 1.000, je Eintrag
  die Mail als Listenzeile im Posteingang, wo sie dort liegt). Pins von Mails, die auf diesem Gerät
  nicht liegen, fehlen in der Liste (wie bei den anderen Markierungen). Höchstens 1.000 Pins
  (`invalid_argument` darüber), Mails ohne Message-ID lassen sich nicht anheften. Ereignis
  `inboxChanged`.
- **Sync:** neue Datenart `attachmentPin` am Ende des Registers, ID-Form `MailPart`
  `<Konto-UUID>/<Message-ID>/<Index>` (der Index ist das letzte Segment, die Message-ID darf `/`
  enthalten; Index eine Zahl ohne führende Null), Inhalt `{pinnedAtMs, name, mimeType, size}`
  (unbekannte Felder abgelehnt, Name nicht leer, keine Steuerzeichen), höchstens 2 KiB; je Anhang
  gewinnt die neuere Fassung, Löschen wird übernommen. Ein umgeschlüsseltes Konto nimmt seine Pins
  mit.
- **Annahme zum Index:** Er ist die Position in der Anhangsliste der Leseansicht. Für dieselbe Mail
  zählen vollständiger und teilweise geladener Body gleich (beide lassen im HTML gezeigte
  Inline-Bilder weg); weicht ein Gerät ab, zeigt die Liste weiter den gemerkten Namen, Vorschau und
  Speichern nehmen den Anhang an dieser Position.

### 4. Farbthema im Kern

`UiPreferences.accentTheme` (`AccentTheme`: `teal`, `blue`, `violet`, `rose`, `orange`, `green`;
Standard `orange`, das Orange des Logos nach Entscheidung 0038), Schlüssel `ui.accent_theme`, in `SYNCED_SETTINGS` (je Schlüssel gewinnt die
neuere Version, wie die Schriftgröße). Geprüft wird an der Brücke: Das Enum nimmt nur die sechs
Werte an. Ein gespeicherter Wert, den diese Version nicht kennt (ein neueres Gerät), gilt als
Standard (`orange`) und wird protokolliert. Wer schon ein Thema gewählt hat, behält es: Der Kern
ändert gespeicherte Werte nie. Die Oberfläche übernimmt einen gültigen Wert aus dem alten
Web-Storage einmal in den Kern und löscht ihn danach; der Ersatzspeicher entfällt.

### 5. Ausgeblendete Mails blockierter Absender zählen

Eigener Command `countBlockedHidden(scope: InboxScope) -> u32` statt eines Feldes an den drei
Seitentypen (`InboxPage`, `MessagePage`, `UnreadFirstPage`): Die Zahl braucht nur der Hinweis über
der Liste, sie hängt nicht vom Blättern ab, und die Seitentypen bleiben, wie sie sind. Gezählt
werden Orte in den Posteingängen des Bereichs, deren Mail in `blocked_hidden` steht; je Posteingang
eine Abfrage (Primärschlüssel von `blocked_hidden` je Konto, dann `message_msgid` und ein Zugriff
auf `message_location`), im Plan-Test der Stufe 16 geprüft.

## Folgen

- Neue Commands: `preview_attachment`, `preview_eml_attachment`, `save_all_attachments`,
  `save_all_eml_attachments`, `set_attachment_pinned`, `get_attachment_pins`,
  `list_pinned_attachments`, `count_blocked_hidden`. Neue Konstanten in den Bindings:
  `MAX_PREVIEW_INPUT_BYTES`, `MAX_PREVIEW_TEXT_BYTES`, `MAX_PREVIEW_PDF_PAGES`,
  `MAX_PINNED_ATTACHMENTS`.
- Neue Typen: `AttachmentPreview`, `PreviewKind`, `SaveAllAttachmentsResult`, `PinnedAttachment`,
  `AccentTheme`; neue Felder `AttachmentInfo.preview`, `UiPreferences.accentTheme`.
- Neue Fehlercodes `preview_unsupported`, `preview_too_large`, `preview_damaged`.
- Migration `0015_attachment_pins.sql`; Datenart `attachmentPin`; abgeglichener Schlüssel
  `ui.accent_theme`.
- `docs/ARCHITECTURE.md` (Stack, Datenmodell, Sicherheitsgrenzen, Einstellungs-Sync),
  `docs/SYNC.md`.

## Ungeprüft

- Die Vorschau unter Windows (WebView2) und im Desktop-Test; geprüft sind Kern und Oberfläche in
  Unit-Tests (`mail-content`, `mail-engine`, Vitest). Der e2e-Test ist geschrieben, aber nicht
  gelaufen.
- Echte PDFs aus dem Alltag: Im Test läuft eine kleine selbst gebaute PDF (zwei Seiten, eine
  gefüllte Fläche, eine `/OpenAction` mit JavaScript ohne Wirkung). hayro rendert nach eigener
  Angabe die meisten PDFs, aber nicht alles (Mischmodi, Knockout-Gruppen, verschlüsselte Dateien).
- Laufzeit: Eine bösartige PDF kann einen Blocking-Thread lange beschäftigen; die Vorschau rendert
  nur eine Seite je Aufruf, hat aber kein Zeitlimit (ein Abbruch bräuchte einen eigenen Prozess).
- Die Annahme zum Anhangsindex (3) zwischen einem Gerät mit vollständigem und einem mit teilweise
  geladenem Body ist nicht mit einer echten Mail beider Formen geprüft.

## Rückweg

Die Vorschau ist ein Command; fällt hayro weg, liefert PDF `preview_unsupported` und öffnet weiter
in der System-App. Die Datenart `attachmentPin` lässt sich aus dem Register nehmen (Datensätze
bleiben im Sync-Zustand). `ui.accent_theme` ist ein gewöhnlicher Schlüssel.
