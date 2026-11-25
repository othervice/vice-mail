# 0068 — Logo in Blau, scharfe Symbole und Zähler in der Größe des Bildschirms

- **Datum:** 25.11.2026
- **Anlass:** Stufe 21, Rückmeldung 7 nach dem Test von 0.1.1: „Die Zahl am Taskleisten-Symbol bei
  neuer Mail muss schärfer sein; das ganze Logo kann eine bessere Auflösung haben, es muss crisp
  sein.“ Dazu sollen Logo und Farben zum neuen Farbkonzept passen (Blau, GitHub-Dunkel, 0053).
- **Status:** Logo in Blau vom Menschen am 25.11.2026 freigegeben (Entwurf der Auftraggeberin:
  dieselben Formen, jede Farbe auf ihr GitHub-Gegenstück abgebildet, 52 Farbpaare). Umsetzung,
  Symbolkette und Zähler entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, 7+10).
- **Code:** `docs/brand/make_svgs.py`, `scripts/icons.sh` (bisher
  `app/src-tauri/icons/generate.sh`), `app/src-tauri/icons/`, `app/src/components/Brand.tsx`,
  `app/src/styles/tokens.css` (`--ui-brand-ink`); Zähler und Fenstersymbol:
  `app/src-tauri/src/badge.rs`, `notifications.rs`, `windows.rs` (`set_window_icon`)
- **Tests:** `app/tests/brand.test.tsx` (Farben von `Brand.tsx` = Farben von `docs/brand`),
  `badge::tests` (Größen je Skalierung, pixelgenaue Ziffern, Abstand der Ziffern, Mitte, „99+“,
  Goldbilder bei 16 px, Abzeichen des Symbols im Infobereich)

## Entscheidung

### Logo in Blau (löst das Orange aus 0038 und `docs/brand/README.md` ab)

- Formen, Verläufe, Leuchten und die kleinen, auf das Pixelraster gezeichneten Größen bleiben,
  wie sie am 23.11. gezeichnet wurden. Getauscht sind nur die Farben: die Orange-Familie gegen die
  Blau-Skala von GitHub (`#cae8ff` bis `#1158c7`; das V oben `#a5d6ff`, Mitte `#4493f8`, unten
  `#1f6feb`), die warmen Dunkeltöne gegen die kühlen von GitHub-Dunkel (Grund `#0d1117`, Umschlag
  `#161b22`, Ink `#1f2328`). Die Namen der Palette in `make_svgs.py` heißen jetzt `SKY`, `BLUE`,
  `DEEP_BLUE`; `NIGHT`, `INK`, `CHARCOAL`, `STEEL` bleiben.
- „Vice“ der Wortmarke nimmt hell `#1f2328` (`--ui-brand-ink`), dunkel Weiß wie bisher.
- Orange bleibt als wählbares Akzent-Thema (0053), ist aber keine Markenfarbe mehr.

### Ein Befehl für alle Symbole

- `scripts/icons.sh` ersetzt `app/src-tauri/icons/generate.sh`: `make_svgs.py` schreibt die
  SVG-Quellen, `tauri icon` (resvg) rastert jede Quelle nur in den Größen, für die sie gezeichnet
  ist, `iconutil` baut `icon.icns`, ein kleines Python-Stück `icon.ico`. Neue Farben oder eine neue
  Zeichnung heißen: `make_svgs.py` ändern, `scripts/icons.sh` ausführen.
- `icon.ico` enthält 16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 128 und 256 px; jede
  Größe bis 48 px aus ihrer eigenen Quelle auf ihrem Pixelraster (nicht verkleinert), ab 60 px aus
  dem Master im Windows-Rahmen. Der erste Eintrag ist jetzt 32 px (wie bei `tauri icon`); Windows
  wählt nach Größe, nicht nach Lage.
- `icon.icns` vollständig (16 bis 512 @1x/@2x; 16 und 32 px aus den eigenen kleinen Quellen),
  dazu die PNG-Dateien, die `tauri.conf.json` nennt. Das Volume-Symbol des Disk-Images kommt aus
  `icon.icns`, das Symbol des NSIS-Installers ist `icon.ico`.
- Neu: `icons/tray/macos-template.png`, die einfarbige Glyphe (`glyph-mono.svg`) als
  Vorlagenbild für die Menüleiste (36 px für 18 pt, siehe 0069).
- `Brand.tsx` (Inline-SVG der App) wird nicht vom Skript geschrieben; der Test vergleicht seine
  Farben mit `docs/brand/icon.svg` und der Wortmarke, so bleibt die App beim nächsten Farbwechsel
  nicht zurück.

### Der Zähler in der Größe, in der Windows ihn zeigt

- **Ursache der Unschärfe:** Das Overlay der Stufe 9 war immer 32 × 32 px mit Ziffern aus einer
  3×5-Schrift in 3- oder 4-facher Vergrößerung. Windows zeigt Overlays in der Größe kleiner
  Symbole, 16 logische Pixel; bei 100 % wurde das Bild auf die Hälfte verkleinert, aus 3 px
  breiten Strichen wurden 1,5 px, also graue Kanten.
- **Jetzt** (`badge.rs`, ohne Bilddateien und ohne Schriftbibliothek): das Bild hat genau die
  Größe für die Skalierung des Fensters, `round(16 × Skalierung)` (16/20/24/28/32/40 px bei
  100–250 %), und wird bei `ScaleFactorChanged` neu gezeichnet. Ziffern aus zwei Pixelschriften
  (3×5, 5×7) in ganzzahliger Vergrößerung (1×, 2×, 3×), immer volle Pixel: jedes Ziffernpixel und
  seine acht Nachbarn liegen ganz im roten Kreis (geprüft je Größe und Zahl), nur der Rand des
  Kreises ist geglättet (4×4 Abtastungen). Es gilt die größte Schrift, die so passt und höchstens
  64 % der Höhe einnimmt. Über 99 steht „99+“; passt das im Kreis nur in der kleinen Schrift, wird
  es ein Rechteck mit runden Ecken so breit wie das Bild (ab 20 px).
- **macOS:** das Abzeichen des Systems (`NSDockTile.badgeLabel`, Systemschrift, schon immer
  scharf). Neu wird es auf der App gesetzt statt über ein Fenster, so bleibt es aktuell, während
  kein Fenster offen ist (Entscheidung 0069).
- **Fenstersymbol unter Windows:** Tauri 2.12 lädt das Fenstersymbol aus der Ressource der Exe in
  der Größe `SM_CXICON` (32 px bei 100 %) und setzt es nur als kleines Symbol; ohne großes Symbol
  nimmt die Taskleiste dieses und verkleinert es auf ihre 24 px. Die Shell setzt jedem Fenster
  stattdessen den eigenen Eintrag der ICO für die Taskleiste bei seiner Skalierung
  (`round(24 × Skalierung)`: 24/30/36/48/60 px, `Image::from_app_icon_resource`) und tauscht ihn
  bei `ScaleFactorChanged`. Die Titelleiste verkleinert dann 24 auf 16 px (vorher 32 auf 16);
  die Taskleiste ist das, was der Mensch unscharf fand.
- **Symbol im Infobereich (Windows, 0069):** der eigene Eintrag der Kleinsymbol-Größe
  (16/20/24/32 px) mit der Zahl in einem roten Abzeichen unten rechts, gleiche Schriften.

## Ungeprüft

- Auf echtem Windows ist nichts davon gesehen: Overlay, Fenstersymbol und Symbol im Infobereich
  sind mit `cargo xwin clippy` übersetzt und als Pixel in Tests geprüft. Ob die Taskleiste von
  Windows 10/11 wirklich das kleine Fenstersymbol nimmt, wenn kein großes gesetzt ist, und ob sie
  es bei 24 px ohne weiteres Skalieren zeigt, steht in Microsofts Dokumentation nicht eindeutig;
  ein Test auf Windows bei 100, 125, 150 und 200 % klärt es (`docs/testing/windows-checklist.md`).

## Rückweg

Die Farben in `make_svgs.py` und `Brand.tsx` zurücktauschen (die Paare stehen in der Geschichte
dieses Commits) und `scripts/icons.sh` ausführen.
Für den Zähler: `badge::overlay` durch ein festes 32-px-Bild ersetzen (Stand Stufe 9); das
Fenstersymbol fällt ohne `set_window_icon` auf Tauris Wahl zurück.
