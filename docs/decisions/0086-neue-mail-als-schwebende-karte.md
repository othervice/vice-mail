# 0086 — Neue Mail als schwebende Karte: Betreff als Titel, Chips, kompakte Formatleiste, „Senden ▾“ und „…“

- **Datum:** 26.11.2026
- **Anlass:** Stufe 23, Punkt 2, Schritt 4 („Neue Mail: schwebende Karte über dem Lesebereich
  (eigenes Fenster bleibt möglich), Betreff als Titel, Chips für An und Von, kompakte
  Formatleiste, ‚Senden ▾‘ (später senden), Symbolreihe, alles andere im ‚…‘-Menü“). Vorlage ist
  der am 26.11.2026 freigegebene Entwurf (Entscheidung 0083), dort der Teil `role="dialog"`. Vorher
  stand alles auf einmal da: Umschalter Formatiert/Reiner Text, die volle Formatleiste,
  Signatur, Platz des Zitats, Ursprungsmail archivieren, Lesebestätigung, Erinnern, der Hinweis
  zum Ziehen von Dateien und ein eigener Knopf „Später senden …“ (`docs/images/composer.png`).
- **Status:** umgesetzt von der ausführenden Sitzung. Ergänzt 0083 (Schritt 4). Ändert die
  **Darstellung** aus 0023 (Formatleiste, Vorlagen, Später senden), 0025 (Composer über dem
  Lesebereich; das eigene Fenster bleibt in der Funktion gleich), 0033 (Senden und archivieren,
  Lesebestätigung), 0059 (Erinnern, wenn keine Antwort kommt) und 0082 (Hinweis beim Ziehen). Am
  Verhalten des Kerns ändert sich nichts: dieselben Commands, dieselben Regeln für nur lesende
  Konten und Identitäten, jede schreibende Aktion weiter über die Outbox.
- **Code:** `app/src/features/composer/Composer.tsx` (`ComposerFrame`, `useCardPlacement`,
  `placeCard`, Kopfzeile, Felder, Fuß mit Sendezeile, `TemplateMenu` als Feld über dem Symbol),
  neu `ComposerMenu.tsx` (Menüknopf mit Befehlen, Auswahlen und Optionen), `ComposerFormatBar.tsx`
  (kompakte Leiste), `formatting.ts` (Absatzformat, Ausrichtung, Formatierung entfernen, Links;
  aus `RichEditor.tsx` herausgelöst), `RichEditor.tsx` (`formatBarSlot`, `onPlainText`),
  `FollowUpOption.tsx` (Symbol mit Menü und Feld für eine eigene Zeit), `ComposerAttachments.tsx`
  (kompakte Chips), `RecipientField.tsx` (`look="card"`), `features/palette/KeyboardLayer.tsx`
  (`data-app-shortcuts`), `app/src/styles/tokens.css` (`--ui-composer-*`), Texte in
  `app/src/i18n/en.ts` und `de.ts`.
- **Tests:** Vitest neu `composer-card.test.tsx` (Dialog und Name, Platz über dem Lesebereich bzw.
  am Ende des Fensters, Tab bleibt drin, Escape, Verkleinern und Zurück mit Fokus, Ziehen holt die
  Karte zurück, „Von“ als Chip mit Menü und Kontofarbe, Sendezeile, jede frühere Option an ihrem
  Platz) und `support/composerMenus.ts`; angepasst `composer.test.tsx`,
  `composer-complete.test.tsx` (unter anderem Einzug und „Formatierung entfernen“),
  `send-archive-templates.test.tsx`, `stage20.test.tsx`, `windows.test.tsx`, `a11y.test.tsx`
  (axe mit jedem Menü offen und verkleinert), `focus-ring.test.tsx`,
  `stage16-writing-a11y.test.tsx`. Desktop angepasst `compose.e2e.mjs`,
  `composer-complete.e2e.mjs`, `stage20.e2e.mjs`, `dark-mails.e2e.mjs`, `stage23-drag.e2e.mjs`
  (Helfer `composerMenuChoice`, `setComposerFormat` in `support/app.mjs`).

## Entscheidung

**1. Eine Karte über dem Lesebereich, die Liste bleibt.** Im Hauptfenster liegt der Composer als
schwebende Karte über dem Lesebereich: Rundung 14 (`rounded-panel`), Haarlinie `border-subtle`,
Schatten `shadow-panel` (nur Schwebendes hat Schatten, 0083), Fläche `surface-raised`; 20 px Abstand
oben und unten, 24 px an den Seiten. Liste, Seitenleiste und die Werkzeugleiste des Lesebereichs
bleiben sichtbar und bedienbar. Wo genau, misst die Karte im DOM (`placeCard`): Steht der
Lesebereich rechts der Liste, beginnt sie dort, wo die Liste endet (der Griff zwischen Liste und
Lesebereich nennt die Liste per `aria-controls`), und unter einer Werkzeugleiste, die oben im
Lesebereich steht; sie folgt dem Ziehen der Grenze, dem Wechsel des Layouts und der Ansicht
(`ResizeObserver`, `MutationObserver`, je Bild einmal). Die Werte gehen als CSS-Variablen an die
Karte (CSSOM, kein `style`-Attribut: CSP), das Stylesheet hält sie im Fenster: nie schmaler als der
Lesebereich mindestens ist (22,5 rem), ab 60 rem mittig. Ohne Lesebereich neben der Liste (unten, an
ihrer Stelle, andere Ansichten wie Einstellungen) steht sie am Ende des Fensters, so breit wie der
Composer vorher (46 rem). Mehrere Entwürfe wie bisher: einer im Hauptfenster (ein neuer sichert den
alten und gibt ihn an die Kopie auf dem Server), weitere in eigenen Fenstern.

**2. Ein Dialog, aber kein modaler.** `role="dialog"`, benannt nach dem Betreff (sonst „Neue
Nachricht“, „Antworten“ …), ohne `aria-modal`: Die Liste bleibt mit der Maus erreichbar. Tab und
Umschalt+Tab gehen an den Enden der Karte im Kreis, Escape schließt sie (der Entwurf bleibt, wie
bisher), danach geht der Fokus dorthin zurück, wo er vorher war. Die Einzeltasten-Kürzel der Liste
wirken nicht durch die Karte hindurch (wie bisher); die Kürzel der App (Befehlspalette, ⌘K) wirken
weiter, damit die Palette Vorlagen einfügt: Die Tastaturebene überspringt Dialoge, außer solchen mit
`data-app-shortcuts`.

**3. Verkleinern.** Die App hatte keinen Begriff dafür; jetzt hat die Karte „Verkleinern“: Sie
wird zur schmalen Leiste (20 rem) unten rechts, mit dem Titel als Knopf „Vergrößern: …“ und
Schließen. Das Formular bleibt dabei bestehen, nur unsichtbar: Text, Cursor, Rückgängig-Verlauf
des Editors und der Speicherstand bleiben. Beim Verkleinern geht der Fokus auf „Vergrößern“, beim
Vergrößern dorthin zurück, wo zuletzt gearbeitet wurde (nicht auf die Knöpfe der Kopfzeile). Werden
Dateien über das Fenster gezogen, kommt die Karte von selbst zurück, damit man sieht, wohin sie
gehen; eine Vorlage aus der Befehlspalette holt sie ebenfalls zurück. Im eigenen Fenster gibt es
kein Verkleinern (das Fenster hat sein eigenes).

**4. Kopfzeile.** Der Betreff ist der Titel: ein Feld ohne Rahmen in 20 px fett (der Entwurf hat
19 px; die Skala der App hat 20, 0083), Platzhalter „Betreff“, Name „Betreff“ für Screenreader.
Daneben klein (11 px, `text-tertiary`) der Speicherstand wie bisher; zu lange Stände enden mit „…“,
der ganze Text ist ihr Tooltip, `aria-live` sagt ihn an. Dann „In eigenem Fenster öffnen“ (nur
im Hauptfenster), „Verkleinern“, „Schließen“.

**5. Empfänger und Absender.** An, Cc und Bcc als Zeilen mit Chips (`badge`, rund, ohne Haarlinie)
und kleinen grauen Etiketten (12 px, `text-tertiary`), 24 px Einzug wie die Karte. „Cc“ und „Bcc“
sind kleine Textlinks am Ende der An-Zeile und blenden ihre Zeilen ein. „Von“ steht wie im Entwurf
unter „An“: ein Chip mit einem Quadrat in der Kontofarbe und „Name · Adresse“; bei mehreren
Identitäten ein Menü mit ihnen (Name „Von: …“), bei einer der Chip allein. Die Regeln für
Identitäten bleiben, wie sie sind.

**6. Text, Signatur, Zitat.** Die Schreibfläche bleibt in der Systemschrift (`font-mail`, 0083),
14 px mit 1,65, 24 px Einzug. Signatur wie bisher direkt unter dem Text. Das Zitat ist eingeklappt
hinter einem kleinen Chip „··· Zitat anzeigen“ (bei Weiterleitungen „Weitergeleitete Nachricht
anzeigen“, `aria-expanded`); gesendet wird es trotzdem, ob es eingeblendet ist oder nicht. Ob es
überhaupt mitgeht und wo der eigene Text steht, steht im „…“-Menü.

**7. Kompakte Formatleiste.** Unten über der Sendezeile, eine runde Gruppe auf dem Ton der Chips:
Absatzformat (Menü, zeigt das aktuelle), Fett, Kursiv, Unterstrichen, Liste, Link, Zitat und
„Weitere Formatierung“ (⋮). Darin alles Übrige der bisherigen Leiste: Durchgestrichen, Code,
Nummerierte Liste, Ausrichtung (vier, als Gruppe), Bild einfügen, Rückgängig, Wiederholen, und
neu Einzug vergrößern bzw. verkleinern (für Listenpunkte; außerhalb einer Liste sagen die Einträge,
dass sie nichts tun) und „Formatierung entfernen“ (Auszeichnungen weg, Überschriften, Listen,
Zitate und Codeblöcke werden Absätze, Ausrichtung links; Bilder bleiben). Am Ende der Wechsel zu
„Reiner Text“. Ist ein Bild gewählt, erscheint seine Größe als eigenes Menü in der Gruppe. Eine Wahl
in diesen Menüs gibt den Fokus sofort an den Editor zurück, mit seiner Auswahl. Die Kürzel des
Editors (⌘B, ⌘I, ⌘U, ⌘Z) bleiben. Der Editor zeichnet die Leiste per Portal in den Fuß der Karte;
die Editoren für Signaturen und Vorlagen behalten ihre volle Leiste. In reinem Text gibt es keine
Leiste (es gibt nichts zu formatieren); zurück zu „Formatiert“ geht es über „…“.

**8. Sendezeile.** „Senden“ als geteilter Knopf in `accent-fill`: links Senden (Name und Tooltip
„Senden (⌘↩)“, `aria-keyshortcuts`; der eigene Chip mit ⌘↩ entfällt), rechts der Pfeil „Weitere
Arten zu senden“ mit „Später senden“ (die drei Vorschläge mit Zeit, „Datum und Uhrzeit wählen …“
öffnet das bisherige Feld mit Vorschlägen und eigener Zeit über der Zeile) und, wo es ein Original
gibt, „Senden und archivieren“ (⌘⇧↩). Unter den Einträgen steht die Regel „Wird nur gesendet, wenn
die App zu diesem Zeitpunkt läuft und online ist“ (beschreibt das Menü). Lehnt der Kern einen
Zeitpunkt aus dem Menü ab, steht der Grund über der Zeile. Danach Symbole: Dateien anhängen
(Dialog des Kerns), Bild einfügen (nur formatiert), Vorlage einfügen (Feld mit den Vorlagen über dem
Symbol, Escape oder ein Klick daneben schließt es), Erinnern, wenn keine Antwort kommt (Menü mit
„Nicht erinnern“, den drei Vorschlägen und „Datum und Uhrzeit wählen …“ mit einem kleinen Feld;
ist eine Erinnerung gesetzt, steht das Symbol im Akzent mit einem Punkt, und sein Name sagt den
Zeitpunkt), „…“ (Weitere Optionen). Ganz rechts Entwurf verwerfen. Anhänge stehen als kompakte
Chips über der Formatleiste; der Hinweis zum Ziehen erscheint nur, solange Dateien über dem Fenster
sind (gestrichelt, mit dem Zusatz für Bilder im formatierten Text), dazu der Rahmen um die Karte.
Empfänger der Dateien bleibt das Fenster (0082).

**9. „…“ — Weitere Optionen.** In Gruppen: Signatur (Standard der Identität, jede Signatur,
„Keine Signatur“; ohne Signaturen-Pool „Signatur anfügen“), „Ursprüngliche Nachricht zitieren“
bzw. „Weitergeleitete Nachricht einfügen“, „Mein Text steht“ über bzw. unter dem Zitat, „Beim
Senden“ (Ursprungsmail nach dem Senden archivieren, Lesebestätigung anfordern mit ihrer Erklärung
als Beschreibung), Format (Formatiert, Reiner Text). Optionen zum Ein- und Ausschalten lassen das
Menü offen, Auswahlen schließen es.

**10. Wo jede bisherige Bedienung jetzt ist.**

| Vorher | Jetzt |
|---|---|
| Kopf: Titel nach Art der Mail, Verwerfen, Schließen, Eigenes Fenster | Betreff als Titel; Verwerfen rechts in der Sendezeile; Schließen, Eigenes Fenster, neu Verkleinern im Kopf |
| Feld „Betreff“ | Titel der Karte |
| „Von“ als Auswahlliste oder Text | Chip mit Kontofarbe, Menü bei mehreren Identitäten |
| Umschalter Formatiert / Reiner Text | „…“ → Format; im formatierten Text auch „Weitere Formatierung“ |
| Leiste: Rückgängig, Wiederholen | „Weitere Formatierung“ (und ⌘Z, ⌘⇧Z) |
| Leiste: Absatzformat | Menü am Anfang der kompakten Leiste |
| Leiste: Fett, Kursiv, Unterstrichen, Aufzählung, Link, Zitat | kompakte Leiste |
| Leiste: Durchgestrichen, Code, Nummerierte Liste, Ausrichtung | „Weitere Formatierung“ |
| Leiste: Bild einfügen | Symbol der Sendezeile und „Weitere Formatierung“ |
| Bildgröße (bei gewähltem Bild) | Menü in der kompakten Leiste, solange ein Bild gewählt ist |
| „Vorlage“ | Symbol der Sendezeile, Feld darüber |
| Signatur-Auswahl bzw. „Signatur anfügen“ | „…“ → Signatur |
| „Ursprüngliche Nachricht zitieren“ / „Weitergeleitete Nachricht einfügen“ | „…“ |
| „Mein Text steht über/unter dem Zitat“ | „…“ → Mein Text steht |
| „Ursprungsmail nach dem Senden archivieren“ | „…“ → Beim Senden |
| „Lesebestätigung anfordern“ | „…“ → Beim Senden |
| „Erinnern, wenn keine Antwort kommt“ (Auswahlliste) | Symbol der Sendezeile mit Menü |
| „Dateien anhängen“ und „oder Dateien auf das Fenster ziehen“ | Büroklammer der Sendezeile; Hinweis nur beim Ziehen |
| Senden, ⌘↩-Chip | „Senden“, ⌘↩ im Tooltip und Namen |
| „…“ neben Senden: Senden und archivieren | Pfeil am Senden-Knopf |
| „Später senden …“ | Pfeil am Senden-Knopf: Vorschläge, „Datum und Uhrzeit wählen …“ |

**11. Tastatur und Screenreader.** Jedes Menü ist ein Menüknopf nach WAI-ARIA (`ComposerMenu`):
Eingabe, Leertaste oder ↓ öffnen es auf der gewählten Auswahl (sonst dem ersten Eintrag), ↑ auf dem
letzten; Pfeile, Pos1 und Ende bewegen, Escape schließt es und gibt den Fokus an den Knopf (nie
schließt Escape dabei den Composer, auch nicht, solange der Fokus noch auf dem Knopf steht), Tab
schließt es. Einträge sind `menuitem`, `menuitemradio` (in benannten Gruppen) oder
`menuitemcheckbox`; lange Namen brechen um statt abgeschnitten zu werden. Menüs am Fuß der Karte
öffnen nach oben (`useMenuPlacement`). Alle Kürzel bleiben: ⌘↩ senden, ⌘⇧↩ senden und archivieren,
Escape schließen, die Kürzel des Editors, die Befehlspalette.

**12. Das eigene Fenster** zeigt dieselbe Karte, randlos über das ganze Fenster, ohne Verkleinern
und ohne „In eigenem Fenster öffnen“.

## Verworfen

- **Ein modaler Dialog** (Rest des Fensters `inert`): Die Liste soll sichtbar und benutzbar
  bleiben, wie im Entwurf; Mails nachschlagen beim Schreiben ist der häufigste Grund dafür.
- **Die Karte im Lesebereich rendern** (Portal in den Lesebereich): Das hätte den Code des
  Lesebereichs geändert, den Schritt 3 gleichzeitig umbaut; die Messung von außen kommt mit beiden
  Fassungen aus.
- **Signatur, Zitat, Archivieren und Lesebestätigung weiter sichtbar als Kästchen unter dem Text:**
  genau das wirkte überladen; im Entwurf sind sie im „…“-Menü.
- **Ein Pfeil, der nur das Feld „Später senden“ öffnet:** Die drei Vorschläge direkt im Menü sparen
  einen Schritt; die Regel steht im Menü.
- **Popover-Bibliothek für die Menüs:** Die CSP erlaubt kein eingefügtes `<style>` (wie bei
  `OverflowMenu` und `ChoiceMenu`).
- **Die Leiste mit Rovingfokus (ein Tab-Stopp):** wäre ein eigener Umbau aller Leisten; die Knöpfe
  bleiben einzeln erreichbar wie bisher.

## Nachweise

Siehe den Bericht der Sitzung (Tests mit Zahlen, Bildschirmfotos deutsch 1280×800 und 1440×900,
hell und dunkel: neue Mail, Antwort mit eingeklapptem Zitat, „…“-Menü offen, „Später senden“
offen, „Weitere Formatierung“ offen, verkleinert, eigenes Fenster).
