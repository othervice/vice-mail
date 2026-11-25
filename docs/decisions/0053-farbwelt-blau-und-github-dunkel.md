# 0053 — Farbwelt: Hell mit Blau, Dunkel in kühlen Grautönen, Blau als Standard-Akzent

- **Datum:** 24.11.2026
- **Anlass:** Wunsch des Menschen vom 24.11.2026 nach dem ersten Test von 0.1.0 (Stufe 18,
  Aufgabe E): „die helle Version sollte in einem hellen mit Blau Design sein, die dunkle mehr die
  GitHub-Farben haben statt dieses Braun.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 18, Aufgabe E). Löst
  den Standard-Akzent aus 0038 ab; die Farbwerte aus 0031 gelten nicht mehr, ihre Struktur schon.
- **Code:** `app/src/styles/tokens.css` (Wurzel, dunkler Block, Farbthemen),
  `app/src/features/extras/print.css` (Druck mit den hellen Werten),
  `app/src/features/preferences/accent.ts` (`DEFAULT_ACCENT`),
  `crates/mail-model/src/ui.rs` (`AccentTheme`, Standard `Blue`),
  `app/src/features/preferences/palette.ts` (Befehle Hell, Dunkel, System),
  `app/tests/contrast.test.ts`.

## Kontext

Seit Stufe 1 waren die Grautöne leicht warm (Beige im hellen, Braun-Schwarz im dunklen Modus), seit
0031 mit drei tonalen Stufen, seit 0038 mit dem Orange des Logos als Standard-Akzent. Im Test von
0.1.0 wirkte das dem Menschen zu braun. Gewünscht: hell klar und blau, dunkel neutral-bläulich wie
GitHub. Hell, Dunkel und „System“ gab es schon (Einstellungen → Darstellung).

## Entscheidung

### Hell: klar, kühl, Blau

Weiß und helles Blaugrau statt Beige. Die Stufen aus 0031 bleiben: Leiste am dunkelsten,
Listenspalte leicht getönt, Lesebereich am hellsten, Karten erhaben.

| Token | Hell | Dunkel |
|---|---|---|
| `canvas` | `#eef2f6` | `#0d1117` |
| `rail` (dunkelste Stufe) | `#e5eaf0` | `#090c11` |
| `rail-hover` | `#dbe1e9` | `#1a2029` |
| `surface-list` | `#f6f8fb` | `#12171e` |
| `surface` (Lesebereich) | `#fdfeff` | `#161c24` |
| `surface-raised` | `#ffffff` | `#212830` |
| `hover` | `#edf1f6` | `#1d232c` |
| `badge`, `track` | `#e5eaf0` | `#262d37`, `#0b0f14` |
| `priority-tint` / `priority-border` | `#fafcff` / `#d9e2ee` | `#1a2230` / `#2e3b50` |
| `border` / `border-subtle` / `border-strong` | `#d8dfe7` / `#e3e8ee` / `#737f8d` | `#30363d` / `#21262d` / `#6e7681` |
| `text-primary` / `-secondary` / `-tertiary` | `#19212c` / `#444e5c` / `#566171` | `#e6edf3` / `#b7c0ca` / `#9198a1` |
| `accent` / `accent-fill` | `#0b5cc2` / `#1665d8` | `#4493f8` / `#1f6feb` |
| `accent-soft` / `selected` | `#e0ebf9` / `#dde8f8` | `#1c2a3d` / `#182638` |
| `focus-ring` | `#2a72dc` | `#58a6ff` |
| `danger` / `success` / `warning` | `#b42318` / `#1a6d34` / `#875000` | `#fb6a60` / `#3fb950` / `#d29922` |

### Dunkel: neutrale, leicht bläuliche Grautöne

Angelehnt an das dunkle Thema von GitHub, als Idee einer Palette, nicht als Kopie seiner Marke:
sehr dunkles Blaugrau als Grund, abgestufte Flächen, kühle Linien, Blau als Akzent. Die Braun-Töne
sind überall ersetzt: Prioritätskarte, Pillen, Spur, Suchtreffer, Schatten, Avatar-Flächen. Die
Flächen der Avatare und die weichen Flächen jedes Farbthemas sind ihr Farbton, zu etwa 12 bis 22 %
in die kühle Fläche gemischt, damit kein Ton schlammig wirkt.

### Prioritätskarte

In beiden Modi eine erhabene, kühle Karte (hell fast weiß, dunkel eine Stufe über der Liste) mit
Haarlinie und Schatten. Die Farbe trägt das Symbol in der Warnfarbe; eine warm getönte Fläche wie
in 0031 passt nicht mehr zur kühlen Farbwelt.

### Avatare

Die achte Tönung ist Schiefer (kühles Blaugrau) statt Sand, in beiden Modi; die übrigen sieben
Farbtöne und ihre Zuordnung per Hash bleiben. Die Kontofarbe 8 („Braun“) bleibt, sie ist eine
benannte Wahl des Nutzers.

### Standard-Akzent Blau

- Standard ist **Blau** (`AccentTheme::Blue` im Kern, auch als Rückfall für unlesbare Werte;
  `DEFAULT_ACCENT` in der Oberfläche). Die Wurzel von `tokens.css` trägt Blaus Werte, damit schon
  das erste Bild vor der Antwort des Kerns stimmt; der Block `[data-accent="blue"]` wiederholt sie
  (geprüft in `contrast.test.ts`).
- Blau hat wie Orange zwei Stufen: `accent` für Text, Symbole und Schalter (≥ 4,5:1 auf jeder
  Fläche), `accent-fill` als etwas helleres Blau für Hauptknopf und kleine Marken, weiße Schrift
  darauf (hell 5,3:1, dunkel 4,6:1).
- Die anderen Themen (Blaugrün, Violett, Rosé, Orange, Grün) bleiben wählbar und bestehen die
  Kontrastprüfung auf den neuen Flächen. Eine gespeicherte Wahl bleibt, auch Orange: Es gibt keine
  Migration. Wer nie gewählt hat, sieht ab jetzt Blau.
- Das Logo bleibt unverändert Orange auf Fast-Schwarz (`docs/brand/README.md`); die Wortmarke nimmt
  weiter `brand-ink`.

### Schneller umschalten

Die Befehlspalette hat in der Gruppe „Layout“ drei Befehle: „Zum hellen Modus wechseln“, „Zum
dunklen Modus wechseln“, „Hell oder dunkel wie dein System“ (englisch „Switch to light mode“ …).
Sie finden sich auch über „Darstellung“ und „Erscheinungsbild“, der aktuelle trägt „Aktuell“.
Gespeichert wird über den Kern wie in den Einstellungen, sofort angewandt und bei Ablehnung
zurückgenommen; dann erscheint ein Hinweis („Die Darstellung konnte nicht geändert werden.“).

## Begründung

- Kühle Neutraltöne lassen Blau als einzigen Farbakzent ruhig wirken; Beige und Blau zusammen
  wirkten unentschieden.
- Das GitHub-Schema ist vielen Entwicklern und Nutzern vertraut und für lange Lesezeiten erprobt;
  übernommen sind nur Helligkeitsstufen und Farbtemperatur.
- Orange als Standard-Akzent zusammen mit der kühlen Farbwelt wäre ein harter Bruch; Blau trägt die
  Wünsche des Menschen direkt. Das Orange bleibt als Marke im Logo und als wählbares Thema.

## Prüfung

- `app/tests/contrast.test.ts` prüft jedes Paar (Text ≥ 4,5:1, Bedienelemente ≥ 3:1) in beiden
  Modi für alle sechs Themen, die Reihenfolge der Flächen (0031) und neu, dass alle Neutraltöne
  kühl bleiben (Blauanteil nie unter Rotanteil).
- Sichtprüfung mit einer statischen Vorschau aus der echten `tokens.css` (Scratchpad, nicht im
  Repo). Die README-Bilder sind noch nicht neu aufgenommen (`app/tests/e2e/readme-shots.e2e.mjs`).

## Rückweg

Alle Werte sind Tokens in `tokens.css`; der Standard-Akzent ist eine Zeile im Kern
(`#[default]` an `AccentTheme`) und eine in `accent.ts`, dazu die Werte der Wurzel.

## Nachtrag 25.11.2026

Das Logo folgt der Farbwelt: blau in den Tönen von GitHub, dieselben Formen (Entscheidung 0068,
vom Menschen am 25.11.2026 freigegeben). Der Satz oben „Das Logo bleibt unverändert Orange“ gilt
damit nicht mehr.
