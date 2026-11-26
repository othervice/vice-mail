# 0081 — Mails aus jeder Liste auf jeden Ordner ziehen; Ablegen verschiebt

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 12. Der Mensch: „Ich muss Mails per Drag and Drop in Ordner
  verschieben können. Z. B. aus Alle Posteingänge eine Mail nehmen und in einen Ordner eines anderen
  Postfachs ziehen.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22, Punkt 12). Ändert
  0033 für das Ziehen: Dort fragte ein Ablegen in einem anderen Konto „Verschieben oder Kopieren“.
- **Code:** `crates/mail-store/src/drops.rs` (`plan_drop`), `crates/mail-engine/src/parity.rs`
  (`drop_messages`), Command `dropMessages`, Typ `DropResult`; Oberfläche `stores/drag.ts`
  (Sitzung, `dropVerdict`, `dropModeOf`), `features/mailboxes/useDropTarget.ts`, `DropHint.tsx`,
  `useDragAutoScroll.ts`, `features/organize/useDropRows.ts`, `actions/undo.ts` (`undoEach`),
  `actions/Notices.tsx` (Hinweis `dropped`), eingebunden in `mail/MailView.tsx`,
  `unified/UnifiedInbox.tsx`, `search/SearchResults.tsx`, `mailboxes/MailboxTree.tsx`,
  `organize/folders.tsx` (Favoriten), `app/IconRail.tsx`, `app/SidebarColumn.tsx`; Texte `drop.*`
- **Tests:** Store `drop_tests.rs`, Engine-Servertest `crates/mail-engine/tests/drop.rs` (aus „Alle
  Posteingänge“ in den Ordner eines anderen Kontos, gegen Stalwart), Vitest
  `drop-anywhere.test.tsx`, angepasst `actions.test.tsx` und `organize.test.tsx`, Desktop
  `stage22-accounts.e2e.mjs`

## Entscheidung

**1. Ziehen aus jeder Liste.** Die Liste eines Ordners, die Ansichten (Später, Angeheftet, Aufgaben
…), „Alle Posteingänge“ und Kontogruppen, Suchergebnisse und gespeicherte Suchen: Jede Zeile lässt
sich ziehen, auch eine Auswahl mit Zeilen mehrerer Ordner und Konten. Die Sitzung (`DragSession`)
hält die Zeilen selbst, nicht mehr ein Konto und einen Quellordner; das Ziehbild nennt immer die
Zahl („3 Mails“).

**2. Ablegen auf jeden Ordner links.** Die Ordner aller Konten in der Spalte, die Favoriten und in
der schmalen Leiste Posteingang, Archiv und Papierkorb. Die Leiste meint die **Rolle**: Jede Zeile
geht in den Ordner mit dieser Rolle in ihrem eigenen Konto. Sonst landete eine Mail aus „Alle
Posteingänge“ beim Ziehen auf „Papierkorb“ im Papierkorb des gerade gewählten Kontos, also in einem
fremden Konto. Hat ein Konto der Zeilen den Ordner nicht, nimmt die Leiste die Zeilen nicht und sagt
es.

**3. Ablegen verschiebt, ohne zu fragen; Wahltaste (macOS) bzw. Strg (Windows) kopiert.** Das Ziehen
ist die Antwort auf die Frage aus 0033; „Verschieben nach …“ im Menü fragt weiter, bevor eine Mail
das Konto wechselt. `effectAllowed` ist `copyMove`, `dropEffect` folgt der Taste, und der Zeiger
zeigt Kopieren oder Verschieben. Der Hinweis neben dem Ordner sagt es in Worten („3 Mails nach
Projekte verschieben“, „… kopieren“). Strg statt Wahltaste unter Windows, weil das der Explorer so
hält; Cmd ist unter macOS „Verknüpfen“.

**4. Der Kern sortiert, ein Befehl je Zielordner.** `dropMessages(refs, ziel, modus, request)`
(höchstens 500 Mails, die Oberfläche teilt größere Mengen):
- Zeilen im Zielordner bleiben unberührt (`skipped`).
- Verschieben: Zeilen des Zielkontos werden gewöhnlich verschoben (`applyAction` mit `move`,
  sofort, mit Rückgängig); Zeilen anderer Konten gehen den Weg aus 0033 (Kopie per `append` im
  Zielkonto, die Quelle verschwindet erst nach der bestätigten Kopie).
- Kopieren: alles als Kopie über die Outbox (wie „Kopieren nach …“, 0064).
- Vorher, ohne etwas aufzuzeichnen, geprüft: Ziel vorhanden und wählbar, Zielkonto mit
  Schreibzugriff, beim Verschieben jedes Quellkonto mit Schreibzugriff (`read_only` nennt das
  Konto), jede Mail gefunden. Die Übertragung wird zuerst aufgezeichnet: Das Holen der Bytes kann
  scheitern, dann ist nichts geschehen. Zeilen von Gesprächen stehen für ihr Gespräch in diesem
  Ordner (0039), Kindzeilen für sich (0049); das löst die Oberfläche wie bei jeder Aktion auf.

**5. Nur lesende Konten sind gesperrt, sichtbar und angesagt.** Als Ziel immer, als Quelle beim
Verschieben (Kopieren ändert die Quelle nicht und geht). Schon beim Überfahren: Der Ordner nimmt die
Zeilen nicht (`dropEffect` `none`, gestrichelte Umrandung in der Fehlerfarbe), daneben steht der
Grund („Nicht hierher: Für Club sind Änderungen ausgeschaltet.“; bei der Quelle mit dem Tipp, die
Wahltaste bzw. Strg zu halten), und eine immer vorhandene Statusregion sagt denselben Text. Ein
Ordner, in dem schon alle Zeilen liegen, ist kein Ziel und bleibt still.

**6. Während des Ziehens.** Ein zugeklappter Ordner (auch ein Platzhalter oder ein zugeklapptes
Konto) klappt auf, wenn die Zeilen 700 ms auf ihm ruhen (`dragover` kommt weiter). Die Spalte rollt
mit, solange der Zeiger höchstens 40 px vor ihrem oberen oder unteren Rand ist, schneller je näher
am Rand.

**7. Ein Hinweis, ein Rückgängig.** Für den ganzen Ablauf ein Hinweis: „3 Mails nach Projekte
verschoben“, mit Mails aus anderen Konten „… werden nach Projekte in Arbeit verschoben“ und dem Satz,
dass sie ihren Ordner verlassen, sobald die Kopie angekommen ist (Fortschritt unter Aktivität), bei
Kopien „… werden … kopiert“; dazu, wie viele schon dort lagen. Zeilen, die sofort gehen (Verschieben
im Konto), verschwinden aus der Liste und der Cursor springt weiter wie bei jeder Aktion; Zeilen
anderer Konten bleiben stehen, bis ihre Kopie bestätigt ist, wie bisher.

**Was Rückgängig über Konten hinweg kann (geprüft im Kern, `outbox::undo`):** Das Verschieben
innerhalb eines Kontos wird wie jedes Verschieben zurückgenommen. Eine Übertragung nur, solange
ihr `append` noch wartet (etwa offline): Dann wird sie gestrichen, und es ist nichts passiert. Ist
die Kopie auf dem Server, lehnt der Kern ab (`not_undoable`, „a copy on the server is not changed
back“); ein Zurückübertragen gibt es nicht. Rückgängig nimmt deshalb die Teile eines Ablaufs
einzeln, das Neueste zuerst, und sagt danach „Rückgängig gemacht, bis auf Mails, die schon in ein
anderes Konto kopiert waren“. Reine Kopien bieten kein Rückgängig an, sondern „Aktivität zeigen“.

## Ungeprüft

- Die Zieh-Geste des Betriebssystems selbst im Desktop-Test: WebDriver und die nachgebildeten
  Mausereignisse starten keine echte Drag-Sitzung von AppKit; der Desktop-Test löst die
  Drag-Ereignisse im WebView aus (Kern, Testserver und Oberfläche echt). Zeiger, Wahltaste und das
  Mitrollen am Rand prüft der Mensch.
- Windows (Strg zum Kopieren in WebView2).
- Zwischen zwei echten Konten (wie 0033: der Mensch prüft danach).

## Rückweg

`dropMessages` und die Sitzung bleiben; `useDropRows.run` kann für andere Konten wieder den Dialog
aus 0033 öffnen. Die Leiste kann statt der Rolle wieder den Ordner des gewählten Kontos nehmen.
