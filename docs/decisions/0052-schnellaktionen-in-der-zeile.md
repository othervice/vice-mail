# 0052 — Schnellaktionen in der Zeile statt darüber

- **Datum:** 24.11.2026
- **Anlass:** Wunsch des Menschen vom 24.11.2026 nach dem ersten Test von 0.1.0 (Stufe 18,
  Aufgabe B): „Wenn ich über eine Mail im Posteingang fahre, erscheinen die Symbole für Löschen,
  Archivieren usw.; sie verhindern, dass ich die Mail aufklappe, um die anderen Nachrichten zu
  sehen.“ Die schwebende Leiste lag über dem Ende der Zeile und in der schmalen Liste über fast der
  ganzen Breite: Die Pille „n Antworten“ (0049) lag darunter.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 18, Aufgabe B)
- **Code:** `app/src/features/messages/MessageRow.tsx` (`RowActions`, `RowMeta`,
  `MessageRowContent`, `ChildRowContent`), `app/src/features/actions/ActionButtons.tsx`
  (`QuickActions`: Ziele 24 px)
- **Tests:** Vitest `conversation-tree.test.tsx` („quick actions beside the replies toggle“,
  breite und schmale Liste, Kindzeilen, Konto-Etikett), Desktop `app/tests/e2e/columns.e2e.mjs`
  (echter Zeiger: Zeile überfahren, Klick in die Mitte der Pille klappt auf und zu; breit, schmal,
  komfortabel, kompakt)

Ändert die Schnellaktionen aus Stufe 4 und 0031 („kleine schwebende Leiste am Zeilenende, über der
Zeitspalte und in schmalen Zeilen über den Zeilenenden“). `docs/DESIGN.md`, Tabelle „Zeile“ und
„Pille „n Antworten““, ist angepasst.

## Entscheidung

**1. Die Leiste nimmt den Platz der Angaben am Zeilenende ein, in der Zeile selbst.** Solange die
Maus über der Zeile steht oder der Tastatur-Cursor auf ihr (wie bisher), verschwinden Markierungen,
Konto-Etikett und Zeit, und die Leiste steht an ihrer Stelle als Teil der Zeile (ein Flex-Element
derselben Zeile), nicht als Ebene darüber. Was davor steht, weicht nach den Regeln, die schon
gelten: erst die Vorschau, dann das Wort der Pille („3 Antworten“ → „3“), dann der Betreff; Pfeil
und Zahl der Pille bleiben immer. So liegt nichts, was ein Klick erreichen soll (Auswahlkästchen am
Anfang, Pille nach dem Betreff), je unter der Leiste, in jeder Breite und mit jedem Inhalt, ohne
dass die Leiste ihren Platz vorher kennen muss.

**2. In der schmalen Liste in der ersten Zeile.** Dort stehen Absender und Zeit; die Leiste nimmt
den Platz der Zeit ein, der Absender weicht. Die Zeile mit Betreff und Pille bleibt, wie sie ist.
Die Leiste ist 24 px hoch, die erste Zeile 20 px: Sie ragt 4 px nach oben (in den Innenabstand der
Zeile), nie nach unten in die Zeile mit der Pille.

**3. Ziele 24 px statt 32 px.** Die Leiste passt so in jede Zeile, auch in kompakte Kindzeilen
(28 px), und in die erste Zeile der schmalen Liste. 24 × 24 px ist das Mindestmaß aus WCAG 2.5.8;
die Aktionen bleiben über Tastenkürzel, Symbolleiste und Befehlspalette erreichbar (Stufe 8).

**4. Kindzeilen genauso.** Büroklammer, Markierung, ausstehende Änderung und Zeit weichen der
Leiste; der Text der Mail gibt den Platz. Die Gruppe aus Absender, Ordner und Text schneidet ab,
was in einer sehr schmalen Liste neben der Leiste nicht passt (der Ordner war sonst breiter als
der Rest).

**5. Listen mit Konto-Etikett** (vereinter Posteingang, Suche über Konten) haben keine
Schnellaktionen; Konto und Zeit stehen dort wie bisher, ihre Regeln (0049, Nachtrag) gelten
unverändert.

## Verworfen

- **Leiste nur über der Zeitspalte, der Betreff endet davor:** Die Leiste ist mit sechs bis acht
  Aktionen 150 bis 200 px breit, die Zeitspalte 40 bis 80 px. Damit der Betreff davor endet, müsste
  die Zeile beim Überfahren ohnehin Platz in der Breite der Leiste freihalten; das tut die Leiste
  in der Zeile von selbst und genau, ohne eine feste Breite, die bei acht statt sechs Aktionen
  oder anderer Schriftgröße nicht mehr stimmt.
- **Pfeil zum Aufklappen am Zeilenanfang (wie Thunderbird):** Der Mensch hat die Pille nach dem
  Betreff ausdrücklich so gewählt (0049); ein zweites Ziel am Anfang hätte eine Spalte in jeder
  Zeile gekostet und die Pille trotzdem unter der Leiste gelassen.
- **Weniger Aktionen in der schmalen Liste oder ein „Mehr“-Menü:** Ein Menü wäre ein Bedienelement
  in einer `option`/einem `treeitem` (axe `nested-interactive`, Stufe 8); weniger Aktionen nehmen
  die Schnellaktionen dort weg, wo die Liste neben dem Lesebereich steht, also fast immer.
- **Leiste unten in der Zeile (über der Vorschau):** Kompakte schmale Zeilen haben keine Vorschau,
  die Leiste läge über der Pille.

## Folgen

- Beim Überfahren ist die Zeit der Zeile nicht zu sehen (vorher lag die Leiste darüber, auch dann
  war sie nicht zu sehen). Der Tooltip der Zeit und der Name der Zeile für Bildschirmleser bleiben.
- Der Text vor der Leiste wird beim Überfahren neu umbrochen (die Vorschau wird kürzer); die Zeile
  ändert weder Höhe noch Position.
- Desktop-Tests brauchen das Überfahren mit echtem Zeiger: `e2e_native_mouse_event` (nur
  e2e-Build) schickt Bewegung, Drücken, Ziehen und Loslassen an die WKWebView. Ist das Fenster
  nicht das aktive (gesperrter Bildschirm), prüft WebKit eine reine Mausbewegung nur für die
  Bildlaufleisten und lässt `:hover`, wie es ist; der Test schickt die Bewegung dann mit der Angabe
  der linken Taste (ohne vorheriges Drücken: kein Ziehen), die WebKit voll prüft.

## Rückweg

Die Leiste wieder absolut über das Zeilenende legen (`RowActions` mit `absolute inset-y-0 end-2`,
`RowMeta` immer sichtbar); dann liegt die Pille wieder unter ihr, sobald der Betreff lang ist.

## Nachtrag 25.11.2026: auch in gemischten Listen

Punkt 5 gilt nicht mehr: Seit Stufe 21 (Rückmeldung 6, Entscheidung 0065) haben der vereinte
Posteingang und die Suchergebnisse dieselbe Leiste, mit dem Konto und Ordner jeder Zeile; Konto
und Zeit weichen ihr wie die übrigen Angaben. Das Auswahlkästchen liegt seitdem an der Stelle des
Avatars statt in einer eigenen Spalte; die Leiste liegt weiter nie darüber.
