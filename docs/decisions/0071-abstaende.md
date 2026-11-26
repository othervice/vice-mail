# 0071 — Abstände: Raster per Test, gleiche Leisten, ein Abstand zwischen Symbol und Text

- **Datum:** 25.11.2026
- **Anlass:** Rückmeldung 4 zu 0.1.1 (Stufe 21), zweiter Teil: „das spacing allgemein müsste man
  mal anschauen, buttons überschriften usw, stimmen da die abstände überall damit das programm
  polished aussieht?“ (Der erste Teil, der leere Platz links neben den Initialen, ist 0065.)
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, Punkt 4).
- **Code:** `app/tests/spacing-scale.test.ts` (Wächter), `app/src/styles/tokens.css`
  (`--ui-filter-compact-below`, `--ui-attachment-row-height`, `--ui-meter-width`,
  `--ui-code-max-height`, `--ui-field-min-width`, `--ui-attachment-width`),
  `app/src/features/views/QuickFilterBar.tsx` (kompakte Chips), `app/src/components/ChoiceMenu.tsx`,
  `app/src/features/reading/ReadingPane.tsx` (Symbolleiste), `app/src/features/ai/AiPanel.tsx`
  (Kopf), `app/src/features/messages/MessageRow.tsx` (Pille, Punkt), `MessageList.tsx` und
  `organize/unread.tsx` (Abschnitte), `layout/SectionRow.tsx`, `components/ContextMenu.tsx`,
  `actions/OverflowMenu.tsx`, `palette/CommandPalette.tsx`, `settings/SettingsView.tsx`,
  `reading/Attachments.tsx`, `reading/MessageHeader.tsx`, `actions/ActionButtons.tsx`,
  `activity/ActivityView.tsx`, `settings/accounts/AccountDetail.tsx` und `QuotaBlock.tsx`,
  `keyboard/KeyboardSettings.tsx`, `updates/UpdateNotices.tsx`, `mbox/MboxJobs.tsx`,
  `app/src/i18n/de.ts` (geschützte Leerzeichen vor „…“) und die 24 Stellen unter Punkt 1
- **Tests:** Vitest `spacing-scale.test.ts`, `list-views.test.tsx` (Schnellfilter breit und
  schmal), `conversation-tree.test.tsx` (Pille); Desktop `spacing.e2e.mjs` (alle Ansichten hell und
  dunkel bei 1280 × 800 und 900 × 600, gemessen)

## Entscheidung

**1. Nur Stufen, die es gibt, und ein Test dafür.** Das Theme (`app.css`) kennt nur das
4-px-Raster, die Schriftgrößen und die vier Rundungen; eine Klasse mit einer anderen Stufe erzeugt
kein CSS und fällt still weg. Der Gestaltungsdurchgang vom 23.11. hatte solche Klassen schon einmal
ersetzt („Korrigiert“), seitdem waren 24 neue dazugekommen: Menüeinträge mit Hinweis ohne
Innenabstand, die dritte Einzugsebene der Menüs ohne Einzug, der Ungelesen-Punkt zweizeiliger Zeilen
oben statt mittig zur ersten Zeile, Auswahl und Datumsfeld der Erinnerung niedriger als der Knopf
daneben, die Update-Frage am Fensterrand, die „+“-Symbole der Tastenkürzel 24 statt 12 px, die
Zeilen der Anhangsansicht ohne Höhe und anderes. Jetzt prüft ein Vitest mit Tailwind selbst und
`app.css`, welche Abstands-, Größen-, Schrift- und Rundungsklassen in `src` CSS erzeugen, und nennt
Datei und Zeile jeder anderen. Zwischenwerte kommen aus Tokens (`calc()` mit `--ui-space-*` oder
neue Tokens mit Kommentar), nie aus festen Pixeln.

**2. Leisten.** Oben am Fenster 56 px (`--ui-header-height`), jetzt auch der Kopf des KI-Panels
(vorher 48 px). Darunter sind Schnellfilter und Symbolleiste des Lesebereichs gleich gebaut (4 px,
32-px-Bedienelemente, 4 px, Haarlinie `border-subtle`) und enden auf einer Linie; vorher war die
Symbolleiste 48 px hoch und der Schnellfilter neben dem Lesebereich drei Zeilen (113 px). In einer
Leiste sind alle Bedienelemente gleich hoch („Änderungen erlauben …“ 32 statt 16 px).

**3. Der Schnellfilter schmal.** Unter 800 px Breite (`--ui-filter-compact-below`; beschriftete
Chips und Feld brauchen in Englisch und Deutsch knapp so viel) zeigen die Chips nur ihr Symbol
(32 × 32 px, rund); ihr Name bleibt ihr zugänglicher Name (`sr-only`) und erscheint als Tooltip.
Das Feld ist dann mindestens 160 px breit. So ist die Leiste bei der Standardbreite der Liste
(384 px) eine Zeile. Gemessen wird wie bei der zweizeiligen Liste (ResizeObserver); ohne Layout
(Tests) gilt die Leiste als breit.

**4. Symbol und Text 8 px, in Chips 4 px.** Knöpfe, Menüeinträge, Zeilen der Seitenspalte und der
Einstellungen, Befehlspalette, Zeitwahlen und Anhangkarten: 8 px (vorher 12 px in Kontextmenü,
„…“-Menüs, Einstellungsliste, Palette und Zeitwahlen, 4 px im Sortierknopf). Runde Chips, die Pille
„n Antworten“ und kleine Knöpfe mit 12-px-Text: 4 px.

**5. Kanten.** In der Liste beginnen Titel der Ansicht, Abschnitte („Heute“) und Avatare auf
einer Linie (Abschnitte vorher 4 px weiter links); in der Seitenspalte die Etiketten und die
Symbole der Zeilen (vorher 4 px versetzt); in der Befehlspalette die Gruppen und die Symbole
(vorher 4 px versetzt); die Aktivität im Einzug des Kopfes (vorher 8 px weiter rechts). Leise
Knöpfe allein unter Text („← Alle Konten“, „Erneut prüfen“ beim Speicher) stehen mit Pfeil bzw.
Text auf der Kante der Seite statt 12 px eingerückt.

**6. Nichts angeschnitten.** Die Pille „n Antworten“ gibt ihr Wort wie in 0049 zuerst her, zeigt
aber nie weniger als 2,5 em davon: Die Spalte des Worts ist so breit wie eine unsichtbare Kopie
(`::before`), das sichtbare Wort liegt darüber in einem Container und verschwindet unter 2,5 em
(Container-Abfrage). Vorher blieb ein Rest der Auslassungspunkte („› 1 .“). Anhangkarten im
Lesebereich sind 320 px breit (vorher 256 px; der Name hatte neben vier Knöpfen gut 60 px). Das
Datum im Kopf einer Mail bricht in einer schmalen Karte um, statt über ihren Rand zu laufen. Der
Platzhalter des Filterfelds endet mit „…“. Deutsche Texte haben vor „…“ ein geschütztes
Leerzeichen (92 Stellen, im Englischen 4), damit die Punkte in schmalen Menüs nicht allein
umbrechen.

**7. Ecken und Kleinigkeiten.** Karten in einer Fensterecke stehen 24 px von beiden Rändern
(Update-Frage, mbox-Aufträge; wie die Sendehinweise). Die Statusmeldung der Tastenkürzel steht
neben „Alle zurücksetzen“ statt in einer leer gehaltenen Zeile darunter (gut 40 px Lücke). Die
Unterzeile der Kontakte hat 12 px wie in allen Ansichten.

## Verworfen

- **Ein Lint-Verbot statt eines Tests mit Tailwind:** müsste das Theme nachbauen; der Test fragt
  den Compiler mit derselben `app.css` und kann nicht auseinanderlaufen.
- **Schnellfilter weiter umbrechen lassen:** drei Zeilen über einer 384 px schmalen Liste, und
  Filter und Symbolleiste kommen nie auf eine Linie.
- **Kleinere Chips mit Text (12 px, enger):** passt in Deutsch nicht in 352 px.
- **Symbolleiste 56 px wie der Kopf:** eine zweite Kopfhöhe ohne Inhalt; die Leiste gehört zur
  Ebene unter dem Kopf.
- **Pille ganz oder gar nicht (wie der Kontoname, 0049):** dann bliebe in der Pille ein leerer
  Rest, solange die Spalte schmaler als das Wort ist.
- **12 px als einheitlicher Abstand:** in 32-px-Knöpfen zu weit; 8 px war schon die Mehrheit
  (Knöpfe, Seitenspalte, Auswahlmenüs).

## Folgen

- Neue Tokens: `--ui-filter-compact-below`, `--ui-attachment-row-height`, `--ui-meter-width`
  (Fortschrittsbalken und Speicheranzeige, beide 320 px), `--ui-code-max-height`,
  `--ui-field-min-width`; `--ui-attachment-width` 20 rem.
- `docs/DESIGN.md`: Abschnitt „Abstände“, Schnellfilter, Token-Tabelle (Abstand).
- Unter Windows nicht geprüft: Segoe UI hat andere Breiten als SF Pro; die Schwelle von 800 px
  hat Luft, die Desktop-Tests laufen dort nicht.

## Rückweg

Den Wächter-Test zu entfernen bringt die stillen Lücken zurück; die übrigen Punkte sind einzelne
Klassen und Tokens und lassen sich einzeln zurücknehmen.

## Nachtrag 26.11.2026

Punkt 5 gilt seit Stufe 22 (Entscheidung 0072) für die Markenspalte statt der Avatare: Titel der
Ansicht, Abschnitte und die Markenspalte der Zeilen (auch der Tabelle) beginnen auf einer Linie;
`spacing.e2e.mjs` misst `rowMark` statt `rowAvatar` und meldet Avatare in der Liste als Fehler.
