# 0031 — Feinschliff der Oberfläche

Stand 23.11.2026.

## Kontext

Die Oberfläche war vollständig, wirkte aber flach: eine Fläche für Leiste, Liste und Lesebereich,
harte Linien, gleich laute Texte in jeder Zeile, Avatare als graue Kreise mit Kontoring, Bündel mit
umrandeten Chips. Der Mensch hat eine zweite Vorlage gegeben (`design-reference/polish-reference.png`,
Screenshot eines fremden Produkts, nicht eingecheckt). Gewünscht ist dieselbe Qualität, nicht
dasselbe Aussehen. Die Regel aus `docs/DESIGN.md` gilt weiter: Übernommen wird die Struktur, nicht
das Aussehen.

Dazu kamen zwei Befunde von axe in der echten WebView (`app/tests/e2e/a11y.e2e.mjs`):

- `aria-required-children` (kritisch): Die Mailliste ist eine `listbox`, enthielt aber echte Buttons
  (Bündelzeilen, „Alle anzeigen“, „Zurück zum Posteingang“).
- `label-content-name-mismatch` (ernst, WCAG 2.5.3): Der Name einer Zeile (`aria-label`) enthielt
  nicht ihren sichtbaren Text (Vorschau, Uhrzeit wie angezeigt).

## Entscheidung

### Übernommen aus der Vorlage (als Qualität)

- Großzügige, gleichmäßige Abstände auf dem 4-px-Raster, dazu die Stufen 20 und 40 px.
- Klare Schrifthierarchie in der Zeile: Absender halbfett (ungelesen) bzw. mittel, Betreff mittel
  (ungelesen) bzw. normal, Vorschau gedämpft, Zeit klein und rechtsbündig.
- In schmaler Liste drei Zeilen pro Mail: Absender und Zeit, Betreff mit Plaketten, Vorschau.
  Kompakt bleiben es zwei Zeilen ohne Vorschau.
- Runde Avatare: Initialen auf einer weichen Tönung je Absender, in hell und dunkel gleich zugeordnet.
- Weiche Flächen statt harter Linien: Leiste am dunkelsten, Listenspalte leicht getönt, Lesebereich
  am hellsten; Karten heben sich mit weichem Schatten ab.
- Der Block „Priorität“ als erhabene, warm getönte Karte mit „Alle anzeigen (n)“ als Abschluss.
- Bündelzeilen mit Symbolkachel, Zähler und Absender-Chips mit Mini-Avatar und Anzahl, „+n“ für den Rest.
  In schmaler Liste stehen die Chips unter dem Namen und umbrechen.
- Kleine Plaketten als Pillen (Zahl der Mails im Thread), Anhang als Symbol.
- Ruhiger Kopf: Titel 16 px halbfett mit kleiner Unterzeile, rechts nur Symbole.
- Schnellaktionen erscheinen beim Überfahren (bestand schon).

### Bewusst nicht übernommen

- Team-Funktionen der Vorlage: Zuweisen („Assign“), „Shared“, Kommentare, Präsenz-Avatare. Sie
  gehören nicht zum Umfang (`docs/SPEC.md`).
- Fremde Farben, Icons, Logos, Illustrationen und die genaue Pixel-Anordnung. Vice Mail behält den
  blaugrünen Akzent, warme Grautöne, lucide-Icons und die Systemschrift (SF Pro, Segoe UI Variable).
- Eine dunkle Leiste im hellen Modus: Sie ist das Markenzeichen der Vorlage. Hier ist die Leiste
  eine dunklere Stufe derselben warmen Grautöne.
- Absenderbilder aus dem Netz: weiterhin nie, nur Initialen.

### Tokens (`app/src/styles/tokens.css`)

| Gruppe | Neu oder geändert | Hell | Dunkel |
|---|---|---|---|
| Flächen | `canvas` | `#f4f2ee` | `#141312` |
| | `rail` (dunkelste Stufe) | `#e9e6df` | `#0e0d0c` |
| | `rail-hover` | `#dedad2` | `#1f1d1b` |
| | `surface-list` (Listenspalte) | `#f8f7f4` | `#1a1917` |
| | `surface` (Lesebereich, unverändert) | `#fdfcfa` | `#1d1c1a` |
| | `badge` (Pillen) | `#ebe8e2` | `#2e2c28` |
| | `priority-tint` | `#fbf1e1` | `#2c2519` |
| Linien | `border-subtle` | `#ebe8e2` | `#2a2825` |
| | `priority-border` | `#f0dfc2` | `#3d3222` |
| Avatar-Tönungen | `avatar-1` … `avatar-8`, je `-bg` und `-text` | helle Pastellfläche, dunkle Schrift | tiefe Fläche, helle Schrift |
| Schatten | `shadow-row` | `0 1px 2px` 7 % | `0 1px 2px` 40 % |
| | `shadow-card` | `0 1px 2px` 6 % + `0 2px 8px` 5 % | 45 % + 25 % |
| Rundung | `radius-row` | 8 px (Zeilen, Kacheln) | |
| Abstand | `space-5`, `space-10` | 20 px, 40 px | |
| Schrift | `font-size-9` | 9 px, nur für das Initial im 16-px-Mini-Avatar (dekorativ) | |
| Maße | `avatar` 28 → 32 px, `avatar-mini` 16 px, `row-height-two-line` 60 → 76 px | | |

Die acht Tönungen (Blau, Violett, Rosa, Koralle, Bernstein, Grün, Petrol, Sand) werden über einen
stabilen Hash (FNV-1a) der Adresse in Kleinbuchstaben gewählt (`features/accounts/senderTint.ts`):
dieselbe Person hat überall, in beiden Modi und auf beiden Plattformen dieselbe Farbe. Die Farbe
trägt keine Bedeutung; der Name steht immer daneben.

Der Kontoring um Avatare erscheint nur noch, wo er etwas unterscheidet: in Listen, die Konten
mischen (vereinter Posteingang, Suche), in der Kontoauswahl der Leiste und in den Einstellungen.
In der Liste eines einzelnen Kontos hätte jede Zeile denselben Ring.

### Kontrastziele

WCAG AA wie bisher (`docs/SPEC.md`): Text 4,5:1, Bedienelemente und bedeutungstragende Grafik 3:1,
in beiden Modi. `app/tests/contrast.test.ts` prüft zusätzlich: Text auf `surface-list`, `badge` und
`rail-hover`; Akzent, Kontofarben und Fokusring auf `surface-list`; Schrift jeder Avatar-Tönung auf
ihrer Fläche (4,5:1); Warnfarbe (Prioritätssymbol) auf `priority-tint`.

### Bedienung und Barrierefreiheit

- Bündelzeilen, „Alle anzeigen“ und „Zurück zum Posteingang“ sind jetzt Einträge (`option`) der
  Liste statt Buttons. Pfeiltasten und j/k erreichen sie, Enter oder Leertaste führen sie aus, ein
  Klick ebenso. Auf ihnen ändert sich die Auswahl nicht. Auf- oder zugeklappt steht im Namen.
  `aria-posinset`/`aria-setsize` zählen alle Einträge.
- Der Name einer Mailzeile kommt aus ihrem Inhalt: ein visuell verborgener Text in Lesereihenfolge
  mit allem Sichtbaren (Absender, Betreff, Vorschau, Zeit wie angezeigt) und den Zuständen, die die
  Zeile nur als Symbol zeigt (ungelesen, Konto, Thread, Anhang, markiert, ausstehend, angepinnt …).
  Die gezeichnete Zeile ist für Screenreader verborgen. Das volle Datum beschreibt die Zeile
  (`aria-describedby`). Kein `aria-label` mehr auf Zeilen.
- Bewegung bleibt bei `transition-colors` mit dem kurzen Token (0030).

## Begründung

- Tonale Stufen und Schatten ordnen die drei Bereiche ohne zusätzliche Linien; das wirkt ruhiger und
  hochwertiger als gleich helle Flächen mit Rändern.
- Je Absender getönte Avatare machen Listen schneller lesbar, ohne Bilder aus dem Netz zu laden.
- Die Prioritätskarte wird aus den Zeilen der virtualisierten Liste zusammengesetzt (jede Zeile trägt
  Fläche, Seitenränder und Schatten, `clip-path` schneidet den Schatten an den Nahtstellen ab). Ein
  eigenes Element über mehrere Zeilen bräuchte Inline-Styles, die die CSP verbietet.
- Ein Name aus dem Inhalt erfüllt WCAG 2.5.3 von selbst und bleibt in jeder Zeilenform gleich.

## Rückweg

Alle Werte sind Tokens; eine andere Gewichtung (z. B. weniger Schatten) ist eine Änderung in
`tokens.css`. Die Dreizeiligkeit hängt nur an `--ui-row-height-two-line` und der Dichte.

## Nachtrag 24.11.2026

Die Farbwerte der Tabelle oben gelten seit Entscheidung 0053 nicht mehr: kühle Grautöne statt
warmer, die Prioritätskarte kühl statt warm getönt, die achte Avatar-Tönung Schiefer statt Sand.
Stufen, Schatten, Avatare und Kontrastziele bleiben wie hier beschrieben.

## Nachtrag 26.11.2026

Die Avatare mit Initialen, ihre acht Tönungen und der Kontoring sind seit Stufe 22 entfernt
(Entscheidung 0072). Zeilen beginnen mit einer schmalen Markenspalte; das Konto zeigt in
gemischten Listen sein Etikett mit Punkt und Name.
