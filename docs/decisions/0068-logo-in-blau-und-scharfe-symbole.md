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
  `app/src/styles/tokens.css` (`--ui-brand-ink`)
- **Tests:** `app/tests/brand.test.tsx` (Farben von `Brand.tsx` = Farben von `docs/brand`)

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

## Rückweg

Die Farben in `make_svgs.py` und `Brand.tsx` zurücktauschen (die Paare stehen in der Geschichte
dieses Commits) und `scripts/icons.sh` ausführen.
