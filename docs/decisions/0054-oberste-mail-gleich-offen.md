# 0054 — Oberste Mail gleich offen, aber noch nicht gelesen

- **Datum:** 24.11.2026
- **Anlass:** Wunsch des Menschen vom 24.11.2026 (Stufe 18, Aufgabe F): „Wenn man z. B. in einen
  Ordner wie Posteingang geht, soll da nicht eine breite Liste aller Mails sein, sondern die
  oberste Mail bereits geöffnet. Dann entstehen nicht diese komischen langen Zeilen.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 18, Aufgabe F)
- **Code:** `app/src/features/messages/MessageList.tsx` (`autoSelectFirst`, `ListSelection.auto`),
  `app/src/features/reading/usePreviewing.ts`, `app/src/features/reading/NoMessageSelected.tsx`,
  `app/src/features/layout/useReadingLayout.ts` (`placeholderShown`, `autoSelectFirst`),
  `app/src/features/layout/ListResizeHandle.tsx`, `app/src/features/mail/MailView.tsx`,
  `app/src/features/unified/UnifiedInbox.tsx`, `app/src/features/unified/MixedReading.tsx`
  (`markRead`), `app/src/features/search/SearchResults.tsx`
- **Tests:** Vitest `top-message.test.tsx`, `search.test.tsx` („opens no hit by itself while the
  query is typed“), angepasst `reading.test.tsx`, `list-resize.test.tsx`, `threads.test.tsx`,
  `conversation-tree.test.tsx`, `unified-inbox.test.tsx`, `comfort.test.tsx`, `composer.test.tsx`,
  `ai-off-and-hints.test.tsx`, `attachments-stage16.test.tsx`, `motion.test.tsx`; Desktop
  `app/tests/e2e/top-message.e2e.mjs`

Ändert Stufe 8 („Lesebereich“: die Liste wird erst schmaler, wenn eine Mail offen ist) und
ergänzt „Beim Öffnen als gelesen markieren“ (Stufe 4, 0039, 0049).

## Entscheidung

**1. Die Liste behält ihre Lesebreite.** Steht der Lesebereich rechts oder unten, hat die Liste
immer ihr Maß aus Aufgabe A (Breite bzw. Höhe, ziehbar), auch wenn nichts gewählt ist: in einem
leeren Ordner, nach „Schließen“ oder Escape im Lesebereich. An der Stelle des Lesebereichs steht
dann ein ruhiger Hinweis „Keine Mail ausgewählt – Wähle in der Liste eine Mail, um sie hier zu
lesen.“ (gedämpft, ohne Bedienelement, kein Tab-Stopp). Die Zeilen bleiben so in der schmalen,
mehrzeiligen Form; die breite einzeilige Liste gibt es nur noch, wenn der Nutzer die Liste über
720 px zieht. In der Stellung „statt der Liste“ bleibt alles wie bisher.

**2. Beim Öffnen eines Ordners oder einer Ansicht steht die oberste Mail im Lesebereich.** Das gilt
für jeden Ordner und jede Ansicht der Mailansicht (Posteingang, eigene Ordner, Ungelesen, Später,
Angepinnt, Aufgaben, …) und für den vereinten Posteingang, rechts oder unten. Oberste Mail ist die
erste Zeile, wie die Liste sie zeigt (auch im Block „Priorität“); bei einer Gesprächszeile zeigt
der Lesebereich, was ein Klick zeigen würde: die älteste ungelesene Mail, sonst die neueste
(0049). Die Liste wählt die Zeile selbst, einmal je geöffnetem Ordner, sobald ihre ersten Zeilen
da sind. Nicht wieder, wenn der Nutzer die Auswahl aufgehoben hat; nicht, solange eine Mail per
Kennung auf dem Weg in diese Liste ist (eine Benachrichtigung, eine KI-Quelle, 0028): die geht vor.

**3. Automatisch gezeigt heißt nicht gelesen.** Eine so gezeigte Mail wird von „Beim Öffnen als
gelesen markieren“ (sofort, nach 2 Sekunden, nie) erst behandelt, wenn der Nutzer sie selbst
wählt (Klick auf die Zeile, Pfeiltasten auf eine Zeile und zurück, Leertaste/X, Eingabe oder
Doppelklick) oder im Lesebereich mit ihr arbeitet (ein Klick, eine Taste oder der Fokus darin).
Ab dann gilt die Einstellung wie bei jeder geöffneten Mail, mit ihrer Frist ab diesem Moment.
Der Tab in die Liste allein ändert nichts: der Cursor steht schon auf der gezeigten Mail.

**4. Fokus und Suche.** Die Liste wählt ohne den Fokus: Der Fokus bleibt, wo er war, und springt
nie von selbst in den Lesebereich. Suchergebnisse wählen nichts von selbst, damit die Auswahl
beim Tippen nicht mitspringt; ihr Lesebereich zeigt bis zur Wahl den ruhigen Hinweis.

## Verworfen

- **Die automatisch gezeigte Mail sofort nach der Einstellung als gelesen markieren:** Jeder Blick
  in einen Ordner würde dessen neueste Mail lesen, ohne dass der Nutzer sie gewählt hat; „nach
  2 Sekunden“ würde zur Falle, sobald man einen Ordner kurz offen lässt.
- **Die Liste ohne Auswahl wieder breit zeigen:** Genau diese langen Zeilen wollte der Mensch
  nicht mehr; der Wechsel der Zeilenform beim Wählen springt zudem.
- **Auch Suchergebnisse wählen ihr erstes Ergebnis:** Jede Taste würde den Lesebereich wechseln
  und eine Mail laden; der Wunsch nennt Ordner und Ansichten.
- **Die oberste Mail nur im Posteingang:** Der Wunsch nennt den Posteingang als Beispiel; die
  langen Zeilen entstehen in jedem Ordner.

## Folgen

- Das KI-Panel zeigt beim Öffnen eines Ordners das Gespräch der obersten Mail; die Liste der
  offenen Antworten („Vielleicht noch offen“) steht dort, wenn keine Mail gewählt ist (nach
  „Schließen“ oder in einem leeren Ordner).
- Befehle, die eine offene Mail brauchen (etwa „Alle Anhänge speichern“), sind nach dem Öffnen
  eines Ordners gleich verfügbar.
- Beim Öffnen eines Ordners lädt der Lesebereich die oberste Mail (aus der lokalen Datenbank oder,
  wenn ihr Text noch fehlt, vom Server).
- Tests, die „nichts offen“ voraussetzten, schließen die gezeigte Mail zuerst
  (`closeOpenedMessage` in `app/tests/support/render.tsx`) oder vergleichen ab dem eigenen Klick.

## Rückweg

`autoSelectFirst` der Leselayouts auf `false` setzen (`useReadingLayout`); für die alte Breite ohne
Auswahl in `listColumn` wieder `!open || replace` zur vollen Breite führen und den Hinweis
weglassen.
