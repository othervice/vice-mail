# 0029 — Start ohne breites Lesen: Teilindex fest, Nachholarbeit nach dem Start

Stand 23.11.2026. Ergebnis der Nachmessung für Stufe 8 und 9 (`docs/testing/performance.md`,
Abschnitt „Stufe 8/9 – Nachmessung 23.11.2026“): Seit Stufe 7 war der Start des Kerns von 2 auf
13 ms gewachsen und der residente Speicher im Leerlauf von 21 auf 98 MB (100.000 Mails) bzw.
171 MB (500.000 Mails). Baut auf 0003 (Store, Pläne), 0017 (Hintergrundaufträge der Suche) und
0020 (Kategorien) auf.

## Befund

- `category_work_pending` (`SELECT EXISTS (… FROM message WHERE category IS NULL)`) läuft bei jedem
  Start. Der Teilindex `message_category_todo` ist nach dem Nachholen der Kategorien leer, aber
  `PRAGMA optimize` beim Schließen hatte seine Größe aufgenommen, solange alle Mails noch ohne
  Kategorie waren (eine Datenbank von vor Stufe 9; im Benchmark speichert der Generator ohne
  Einordnung, wie es Stufe 7 tat). Die Zahl der Mails
  ändert sich danach nicht, also frischt `PRAGMA optimize` die Statistik nicht auf. Mit dieser
  Statistik wählt der Planer `SCAN message`: Der Start liest die ganze Tabelle, um keine Zeile zu
  finden. Über `mmap` bleiben die gelesenen Seiten resident (100.000 Mails: +80 MB, kalt 279 ms).
- Findet der Start Mails ohne Kategorie, schreibt der Kategorie-Auftrag sofort. Sein erster Schritt
  (500 Mails) hält den Schreib-Thread, und die Schreibvorgänge des Starts selbst (verpasste
  geplante Sendungen, Outbox) warten dahinter: Start 120–134 ms statt 5 ms.
- Der Trigger `loc_category_new` (Migration 0007) kopiert Kategorie und Wichtig-Markierung mit
  einem `UPDATE` auf jeden neuen Ort, auch wenn das Einfügen sie schon kennen könnte: 1,8 ms je
  Fenster von 500 Kopfdaten.

## Entscheidung

1. **Teilindex fest.** Die beiden Abfragen auf Mails ohne Kategorie (`categories::UNCLASSIFIED`,
   `categories::ANY_UNCLASSIFIED`) nennen den Index mit `INDEXED BY message_category_todo`, wie
   schon `CONTACTS_TODO` den Index `message_contacts_todo`. Ergebnis und Reihenfolge bleiben gleich.
2. **Nachholarbeit nach dem Start.** `comfort::start` sieht wie bisher nach, ob Kategorien zu tun
   sind, und startet den Auftrag, der aber erst auf den Anstoß wartet. `AppCore::start` stößt ihn
   als Letztes an (`kick_categories`; `Notify` hebt einen Anstoß vor dem Warten auf). Ebenso der
   Auftrag der Suche aus Stufe 7 (Index füllen oder neu aufbauen, Threads nachtragen):
   `SearchState::start` merkt sich die gefundene Arbeit, `SearchState::started` am Ende des Starts
   beginnt sie. Beides kommt also wenige Millisekunden später, gleich nach dem Start. Neue Mails
   ordnet und indiziert der Abgleich weiter beim Speichern. Alles andere an den Aufträgen
   (Schritte, Ereignisse, schwacher Verweis auf Kern bzw. Store) bleibt.
3. **Kategorie gleich beim Einfügen.** Der Ort einer neuen Nachricht bekommt im `INSERT` die
   Kategorie und Wichtig-Markierung, die `store_headers` gerade für sie berechnet hat
   (`INSERT_LOCATION`, Parameter `?11`/`?12`). Der Trigger bleibt im Schema und greift nur noch,
   wo diese Werte nicht vorliegen: bei einem zusammengeführten Ort (die Nachricht lag schon in
   einem anderen Ordner) und bei Mails ohne Kategorie (dann kopiert er nichts Neues). Eine
   Migration ist nicht nötig. Spätere Änderungen erreichen die Orte wie bisher über
   `message_category_change`.

Nicht geändert, weil nötig oder schon klein (Messwerte in `docs/testing/performance.md`): das
Öffnen der vier Verbindungen (jede liest das gewachsene Schema, 161 Objekte), die Zeitzone des
Systems für die Datumsabschnitte, der Abmelde-Client, Outbox-, Entwurfs-, MCP- und Sync-Start.
Der Kontakt-Auftrag wartet schon 2 s nach dem Start.

## Folgen

- `docs/ARCHITECTURE.md`: Abschnitt Store (Teilindizes mit `INDEXED BY`, Kategorie am Ort beim
  Einfügen, Nachholarbeit erst nach dem Start).
- Für neue Hintergrundaufträge gilt dasselbe: Was der Start an Arbeit findet, beginnt erst nach
  dem Start; Abfragen auf einen Teilindex, der nach einem Nachholen leer wird, nennen ihn fest.

## Geprüft

- `mail-store`: `stage9_plan_tests::stale_statistics_of_the_unclassified_index_do_not_cause_a_scan`
  (Statistik bei 3.000 Mails ohne Kategorie, danach alle eingeordnet, Datenbank neu geöffnet;
  scheitert ohne `INDEXED BY` mit `SCAN message`), `every_stage_9_query_uses_its_index` mit
  `ANY_UNCLASSIFIED`; `stage9_tests::new_locations_take_the_category_without_the_trigger`
  (Trigger gelöscht, die Orte neuer Mails tragen Kategorie und Wichtig; scheitert mit dem alten
  `INSERT`), `merged_locations_take_category_and_important_mark`,
  `unclassified_locations_follow_the_later_classification`.
- `mail-engine`: `comfort::tests::the_core_classifies_older_messages_after_its_start`,
  `search::tests::an_index_job_found_at_start_runs_after_the_start`;
  `search::bench::child_measure` scheitert, wenn der Start allein mehr als 40 MB resident macht
  (vorher 80 MB, jetzt 8 MB) oder Start, erste Seite oder Leerlauf die Ziele verfehlen.

## Rückweg

- `INDEXED BY` entfernen, wenn die Statistik nach jedem Nachholen erneuert wird (`ANALYZE` des
  Teilindex am Ende des Auftrags); beides zusammen schadet nicht.
- Den Anstoß am Ende von `AppCore::start` wieder an den Anfang legen (`pending` an den Auftrag
  geben bzw. `kick` in `SearchState::start`), wenn die Arbeit vor dem Ende des Starts gebraucht
  würde.
