# 0049 — Gespräche in der Liste aufklappen, rechts genau eine Mail

- **Datum:** 24.11.2026
- **Anlass:** Wunsch des Menschen vom 24.11.2026 mit einem Bildschirmfoto von Thunderbird: Ein
  Gespräch steht einmal in der Liste, mit dem Absender einmal und „3 Antworten“ samt Pfeil.
  Aufgeklappt stehen alle Mails des Gesprächs eingerückt darunter, mit einer feinen gepunkteten
  Linie links. Rechts steht nur die gewählte Mail, nicht das ganze Gespräch untereinander.
- **Status:** entschieden von der ausführenden Sitzung (Stufe 17, Block A)
- **Code:** `app/src/features/threads/tree.ts` (Identität, Kindzeilen, Ebenen und Positionen),
  `app/src/features/messages/MessageList.tsx` (Baum, Tastatur, Auswahl),
  `app/src/features/messages/MessageRow.tsx` (`RepliesToggle`, `ChildRowContent`, `TreeLine`),
  `app/src/features/threads/ThreadPane.tsx` (eine Mail für die Gesprächszeile),
  `app/src/features/threads/queries.ts` (`conversationQuery`),
  `app/src/features/actions/useActionController.ts` (`Exactness`, `hostFor`),
  `app/src/features/mail/MailView.tsx`, `app/src/features/unified/MixedReading.tsx`
- **Tests:** `app/tests/conversation-tree.test.tsx`, angepasst `conversations.test.tsx`,
  `threads.test.tsx`, `ai-panel.test.tsx`; Store `conversation_tests::an_expanded_row_*`, Engine
  `comfort::tests::an_expanded_conversation_row_lists_its_messages_from_every_folder`;
  Desktop `app/tests/e2e/conversations.e2e.mjs`

Ändert 0039 (Abschnitte „Öffnen“ und „Beim Öffnen als gelesen markieren“), ergänzt 0017 und die
Tastaturregeln aus Stufe 8 (`docs/DESIGN.md`, „Tastatur: Tab-Stopps, Bäume und Leiste“).

## Entscheidung

**1. Die Liste der Gespräche ist ein Baum.** In der Konversationsansicht (0039: Posteingang,
vereinter Posteingang, Ordner, „Nur ungelesene“, „Ungelesene oben“, „Alle anzeigen“ der Priorität,
Suchtreffer) ist die Liste ein mehrfach auswählbarer `tree` statt einer `listbox`, flach im DOM wie
die Bäume der Seitenspalte: Zeilen sind `treeitem`s mit `aria-level`, `aria-posinset`,
`aria-setsize` und, bei Gesprächen mit Antworten, `aria-expanded`. Der Fokus bleibt auf der Liste,
`aria-activedescendant` zeigt auf die Zeile (wie seit Stufe 4). Ein aufgeklapptes Bündel ist
ebenfalls ein Knoten (`aria-expanded`, seine Mitglieder Ebene 2, deren Mails Ebene 3); es nennt
„zugeklappt“ dann nicht mehr in Worten. Listen einzelner Mails (Konversationen aus, Später,
Angepinnt, Aufgaben, Lese-Liste, Markiert) bleiben eine `listbox`: Dort gibt es nichts aufzuklappen.

**2. Kindzeilen sind alle Mails des Gesprächs.** Aufgeklappt folgen der Gesprächszeile alle Mails
ihres Threads in allen Ordnern ihres Kontos, älteste zuerst, eigene gesendete eingeschlossen; jede
an ihrem Ort im gezeigten Ordner, wo sie dort liegt, sonst mit dem Namen ihres Ordners
(„Gesendet“, „Archiv“, auch „Papierkorb“). Das ist genau die Menge, die die Zeile zählt: „n
Antworten“ ist die Anzahl der Mails des Gesprächs (0039) minus eins. Geladen wird beim Aufklappen,
mit `getThread(Mail der Zeile, Ordner der Zeile)` (Stufe 7; höchstens die neuesten 1.000, dann eine
Hinweiszeile „Zeigt die neuesten 1.000 von n Mails.“). Solange sie lädt, steht eine Ladezeile unter
dem Gespräch (`aria-busy` an der Zeile); scheitert sie, eine Zeile „… konnte nicht geladen werden.
Erneut versuchen“ (ein Baumeintrag, Eingabe versucht es erneut). Die Liste bleibt dabei bedienbar.

**3. Die Identität einer Gesprächszeile ist ihr Thread.** Aufgeklappt-Zustand, Auswahl, Cursor,
Abfrage der Kinder und Lesebereich hängen an `t<Thread>` (ohne Thread an der Mail), nicht an der
Mail der Zeile. Kommt eine Antwort, wird sie die Mail der Zeile, die Zeile wandert nach oben, bleibt
aber aufgeklappt, gewählt und rechts geöffnet; ihre Kinder laden im Hintergrund neu. Kindzeilen
heißen `<Thread>/<Mail>`.

**4. Rechts steht immer genau eine Mail.** Die Ansicht des ganzen Gesprächs als Kartenstapel
(Stufe 7) entfällt. Eine Gesprächszeile öffnet die älteste ungelesene Mail ihres Gesprächs, sonst
die neueste (dieselbe Regel wie bisher, `initialEntry`, auch eine eigene Antwort in „Gesendet“).
Welche, wird einmal entschieden, sobald das Gespräch geladen ist, und bleibt, solange die Zeile
gewählt ist: Gelesen markieren oder eine neue Antwort wechseln die Mail nicht. Eine Kindzeile
öffnet ihre Mail. Aus der gestapelten Ansicht bleibt: der Betreff als Titel, die KI-Chips unter der
Mail (das Panel arbeitet weiter mit dem Gespräch der Zeile), die Schnellantwort, die Antwortzeile,
der Hinweis bei einem Gespräch, das nicht lädt (die Mail der Zeile wird trotzdem gezeigt), der
Hinweis auf gekürzte Gespräche (jetzt als Zeile im Baum) und das Zeigen einer KI-Quelle: Gehört sie
zum offenen Gespräch, klappt es auf und ihre Kindzeile wird gewählt.

**5. Beim Öffnen als gelesen markieren: die gezeigte Mail.** 0039 markierte beim Öffnen das ganze
Gespräch, weil rechts das ganze Gespräch stand. Jetzt sieht man eine Mail; nur sie wird gelesen
markiert. Die Zeile bleibt halbfett, solange eine andere Mail des Gesprächs ungelesen ist, und
zeigt damit, dass es dort noch etwas gibt (wie Thunderbird). „Gelesen“ an der Zeile wirkt weiter auf
das ganze Gespräch.

**6. Aktionen.** An der Gesprächszeile wie bisher auf das Gespräch (0039, `conversationTargets`).
An einer Kindzeile (Schnellaktionen, Tastenkürzel, Befehlspalette, Auswahlleiste, Ziehen auf einen
Ordner, Symbolleiste im Lesebereich) nur auf diese Mail, mit den Aktionen ihres eigenen Ordners. Eine
Auswahl darf beides mischen: Jede Zeile wirkt, wie sie gewählt wurde, in einem Aufruf
(`Exactness`: eine Menge von Mail-IDs, die für sich selbst stehen). Kindzeilen verlassen die Liste
nach einer Aktion nicht sofort: Eine verschobene oder gelöschte Mail bleibt Teil des Gesprächs (in
ihrem neuen Ordner); die Liste lädt neu.

**7. Tastatur** (WAI-ARIA Tree View wie die Seitenspalte): → klappt ein zugeklapptes Gespräch oder
Bündel auf, auf einem aufgeklappten springt es zur ersten Zeile darunter; ← klappt ein aufgeklapptes
zu, sonst springt es zur Gesprächs- bzw. Bündelzeile darüber. ↑/↓ (und j/k) laufen durch alle
sichtbaren Zeilen, Lade- und Hinweiszeilen ausgenommen; Umschalt erweitert die Auswahl über
Gesprächs- und Kindzeilen; Pos1, Ende, Bild ↑/↓, Eingabe, Leertaste, X und Escape wie bisher.
Klappt ein Gespräch zu, während der Cursor auf einer seiner Mails steht, geht der Cursor auf die
Gesprächszeile. Der Pfeil mit „n Antworten“ ist ein Ziel für die Maus wie das Auswahlkästchen
(`data-row-toggle`); ein Klick darauf wählt nichts.

## Verworfen

- **`listbox` mit `aria-expanded` an `option`:** In ARIA 1.2 an `option` nicht erlaubt (axe
  `aria-allowed-attr`), Ebenen gibt es dort nicht.
- **`treegrid`:** Zellen wären fokussierbar und jede Aktion ein Tab-Stopp je Zeile; die Gründe aus
  Stufe 4 gegen ein `grid` gelten weiter.
- **Immer ein Baum, auch für Listen einzelner Mails:** Ein Baum ohne Ebenen sagt Bildschirmlesern
  mehr, als er ist.
- **Ein eigener Command für die Kinder:** `getThread` liefert schon alle Mails eines Threads im
  Konto, älteste zuerst, jede an ihrem Ort im gewünschten Ordner, mit Markierungen; ein Thread mit
  7.620 Mails öffnet in 3,7–5,6 ms (`docs/testing/performance.md`). Store- und Engine-Tests halten
  jetzt fest, dass Kinder und Zahl der Zeile übereinstimmen.
- **Kinder ohne Papierkorb und Spam:** Dann müsste die Zeile anders zählen als heute (je Mail die
  Rolle ihrer Orte prüfen, in jeder Zeile jeder Liste); Zahl und Kinder dürfen nicht auseinanderlaufen.
  Der Ordnername an der Kindzeile sagt, wo eine gelöschte Mail jetzt liegt.
- **Beim Öffnen das ganze Gespräch gelesen markieren:** Rechts steht nur noch eine Mail.
- **Die Gesprächszeile öffnet immer die Mail der Zeile:** Die Regel „älteste ungelesene, sonst
  neueste“ gilt seit Stufe 7 und ist die, mit der man ein Gespräch weiterliest.

## Folgen

- `docs/decisions/0039-konversationen-im-sortierten-posteingang.md`: „Öffnen“ und „Beim Öffnen als
  gelesen markieren“ gelten in der Form dieser Entscheidung (Nachtrag dort).
- `docs/DESIGN.md`: Abschnitt „Gespräche aufklappen (Stufe 17, Entscheidung 0049)“; „Lesebereich“
  und „Konversationsansicht“ angepasst.
- Die erzeugten Bindings ändern sich nicht; kein neuer Command, keine Migration.
- Desktop-Tests, die das ganze Gespräch rechts oder `option`-Zeilen in Gesprächslisten erwarteten,
  sind angepasst.

## Rückweg

- Wieder das ganze Gespräch rechts: `ThreadPane` als Stapel aus dem Stand vor Stufe 17
  (Commit `cffffec`) zurückholen; der Baum in der Liste kann bleiben.
- Gelesen markieren des ganzen Gesprächs beim Öffnen: in `MailView` und `MixedReading` die Zeile
  statt der gezeigten Mail an `useMarkReadOnOpen` geben, ohne `exact`.

## Nachtrag 24.11.2026: Betreff des Gesprächs, Feinschliff nach den Bildern

Ein Durchgang über die README-Bilder (`docs/images/inbox-light.png`) fand fünf Stellen.

- **Betreff.** Die Gesprächszeile zeigte den Betreff ihrer neuesten Mail („Re: Website relaunch“).
  Jetzt wie Thunderbird den des Gesprächs: den Betreff seiner ältesten Mail ohne Antwort- und
  Weiterleitungs-Präfixe. Der Kern liefert ihn (`ThreadListItem.subject`,
  `ConversationSummary.subject`, `SearchHit.conversationSubject`; eine Abfrage über
  `message_thread` je Zeile), die Präfixe entfernt `mail_model::threading::display_subject` mit der
  Liste, die auch das Threading nutzt. Nur Präfixe am Anfang fallen weg, der Rest bleibt wie er ist,
  sodass der gezeigte Betreff das Ende des Betreffs ist und Fundstellen der Suche passen. Eine Mail
  allein behält ihren Betreff samt Präfix. Die Suche findet weiter jeden
  Betreff; ein Treffer zeigt den Betreff seines Gesprächs und markiert nur, wo das der eigene ist.
  Verworfen: die Präfixe nur in der Oberfläche vom Betreff der neuesten Mail zu entfernen (ein
  geänderter Betreff wie „(was: venue shortlist)“ stünde dann für das ganze Gespräch, und die
  Präfixliste gäbe es zweimal).
- **Pille, Kindzeilen, Linie, Konto.** Die Pille gibt dem Betreff zuerst nach, bis Pfeil und Zahl;
  Absender der Kindzeilen so breit wie ihr Name; die Linie beginnt auch in der schmalen Liste am
  Avatar; im vereinten Posteingang weicht das Konto-Etikett den Namen bis auf seinen Farbpunkt.
  Einzelheiten in `docs/DESIGN.md`, gemessen in `conversations.e2e.mjs`.
- **Tests:** `threading::subject::tests::display_subjects_lose_their_prefixes_only`,
  `conversation_tests::a_conversation_row_shows_its_oldest_subject_without_prefixes` (Posteingang,
  Ordner, Gesendet, Suche, Abfrageplan), Vitest `conversations.test.tsx` („the subject of a
  conversation row“) und `conversation-tree.test.tsx` („the look“).
