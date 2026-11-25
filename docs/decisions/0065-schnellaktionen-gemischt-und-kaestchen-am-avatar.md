# 0065 — Schnellaktionen in gemischten Listen, Auswahlkästchen am Avatar

- **Datum:** 25.11.2026
- **Anlass:** Rückmeldungen 6 und 4 (erster Teil) zu 0.1.1 (Stufe 21): „In ‚Alle Posteingänge‘
  erscheinen die Schnellaktionen beim Überfahren nicht, nur im Posteingang eines Kontos. Fehler.“
  und „viel Platz links neben den Initialen“: eine eigene Spalte für den Ungelesen-Punkt, die beim
  Überfahren zum Auswahlkästchen wird und ohne ungelesene Mail leer bleibt. Plan: das Kästchen
  beim Überfahren und bei Mehrfachauswahl an der Stelle des Bildes, wie bei Gmail; der Punkt schmal
  davor oder am Bild.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, Punkte 6 und 4a).
  Löst Punkt 5 von 0052 ab („Listen mit Konto-Etikett haben keine Schnellaktionen“).
- **Code:** `app/src/features/unified/MixedActions.tsx` (`useMixedActions`, `ScopeController`),
  `app/src/features/unified/UnifiedInbox.tsx`, `app/src/features/search/SearchResults.tsx`,
  `app/src/features/actions/useActionController.ts` (`listKey`),
  `app/src/features/messages/MessageRow.tsx` (`AvatarMark`, `SelectionMark` nur noch in der
  Tabelle), `app/src/styles/tokens.css` (`--ui-row-lead`, `--ui-tree-line-start`)
- **Tests:** Vitest `context-menu.test.tsx` („lists that mix accounts“), `conversation-tree.test.tsx`
  („lists that mix accounts have them too“, Linie unter dem Avatar), `actions.test.tsx`
  (Kästchen an der Stelle des Avatars), Desktop `context-menu.e2e.mjs` (Überfahren in „Alle
  Posteingänge“ mit einem nur lesenden zweiten Konto, Abstand des Avatars, Punkt davor)

## Entscheidung

**1. Ein Controller je Ordner einer gemischten Liste.** Der Aktions-Controller arbeitet je Konto und
Ordner (Outbox, Rückgängig, Rückfrage nach einem fehlenden Ordner, Auswahlfenster); darum hatten der
vereinte Posteingang und die Suchergebnisse keine Schnellaktionen. Jetzt bekommt jeder Ordner, aus
dem Zeilen der Liste stammen, den Controller seiner eigenen Ansicht (mit dessen Fragen und
Auswahlfenstern), und Schnellaktionen und Kontextmenü (0064) einer Zeile laufen durch den ihres
Ordners: dieselben Aktionen, dieselben Gründe bei nur lesenden Konten (gesperrt, Tooltip, beim
Klick die Meldung mit „Änderungen erlauben …“), dieselbe Outbox. Die Controller bleiben, solange
die Liste lebt, auch wenn die letzte Zeile ihres Ordners gegangen ist (ihre offenen Fragen bleiben
so erhalten). Zeilen, die die Liste verlassen, bleiben ausgeblendet, bis auch die gemischte Liste
neu geladen ist (`listKey`), sonst kämen sie kurz zurück. Eine Auswahl über mehrere Ordner teilt
das Kontextmenü nach Konto und Ordner; Verschieben, Kopieren und Schlagwörter nur bei einem Konto.

**2. Konto und Zeit weichen der Leiste.** In gemischten Listen gilt 0052 wie überall: Beim Überfahren
und auf der Cursorzeile nimmt die Leiste den Platz von Markierungen, Konto-Etikett und Zeit ein.
Das Konto steht weiter im Namen der Zeile und am Ring des Avatars; die Regeln des Etiketts (0049,
Nachtrag) gelten unverändert, solange die Leiste nicht zeigt.

**3. Das Kästchen an der Stelle des Avatars.** Die Zeile hat keine eigene Spalte mehr für Punkt und
Kästchen. Beim Überfahren und bei Mehrfachauswahl (dann in jeder Zeile) zeigt der Platz des Avatars
das Auswahlkästchen (16 px, Haarlinie `border-secondary`, gewählt `accent-fill` mit Häkchen), der
Avatar ist so lange unsichtbar (Platz bleibt). Ein Klick darauf nimmt die Zeile in die Auswahl
oder heraus wie bisher (`data-row-check`), Leertaste und X von der Tastatur; für Bildschirmleser
bleibt es `aria-selected` an der Option. Kindzeilen machen es mit dem Mini-Avatar ebenso.

**4. Der Punkt davor.** Der Ungelesen-Punkt (8 px, `accent`) sitzt im Vorlauf der Zeile
(`--ui-row-lead`, 16 px) 4 px vor dem Avatar, mittig zu ihm (in der schmalen Liste zur ersten Zeile).
Er bleibt beim Überfahren und in der Auswahl sichtbar; ungelesen ist außerdem halbfett, nie nur
eine Farbe. Der Avatar beginnt damit 16 px statt 40 px nach dem Rand der Zeile.

**5. Die Tabelle behält ihre Spalte.** Sie hat keinen Avatar, an dessen Stelle das Kästchen treten
könnte; ihre schmale Auswahlspalte ist eine Spalte der Tabelle mit Platz im Kopf (0061).

## Verworfen

- **Leiste nur im Posteingang eines Kontos lassen und gemischte Listen mit einem Menü „Mehr“:**
  ein Bedienelement in einer Option (axe `nested-interactive`, 0052), und der Mensch will dieselbe
  Leiste überall.
- **Ein gemeinsamer Controller über Konten:** hätte Outbox, Rückgängig, die Fragen nach fehlenden
  Ordnern und die Auswahlfenster ein zweites Mal je Konto gebraucht; der Controller der
  Ordneransicht kann das schon und ist getestet.
- **Punkt auf dem Avatar (wie ein Abzeichen):** verdeckt Initialen oder Bild und ist auf den
  farbigen Tönungen schlecht zu sehen.
- **Balken am Rand der Zeile (Outlook):** nur Farbe, und er fällt mit dem Ring der Kontofarbe und der
  Auswahlfläche zusammen.

## Folgen

- Je Ordner einer Liste ein Controller: im vereinten Posteingang meist einer je Konto, bei
  Suchergebnissen einer je Ordner unter den Treffern.
- Die gepunktete Linie der Gespräche liegt weiter mittig unter dem Avatar (`--ui-tree-line-start`
  aus dem Vorlauf und dem halben Avatar). Ladezeilen und Skelett beginnen am Vorlauf.
- Der allgemeine Durchgang über die Abstände (Rückmeldung 4, zweiter Teil) folgt auf dem
  zusammengeführten Stand; der Vorlauf ist dafür ein Token.

## Rückweg

`AvatarMark` durch die frühere Spalte (`SelectionMark` vor dem Avatar) ersetzen und
`--ui-row-lead` auf `--ui-space-3` mit dem alten `--ui-tree-line-start`; gemischte Listen ohne
`renderQuickActions` und `onRowMenu` rendern.
