# 0039 — Konversationen im sortierten Posteingang, als Standard

- **Datum:** 23.11.2026
- **Anlass:** Test des Menschen: Mehrere Mails derselben Unterhaltung stehen im Posteingang einzeln.
  Stufe 7 und 9 hatten die Lücke festgehalten: Der sortierte Posteingang (Stufe 9) hatte keine
  Thread-Variante, die Konversationsansicht galt nur für normale Ordner, war aus und wurde nicht
  gemerkt.
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-store/src/conversations.rs` (Listen, Zusammenfassung, Ziele von Aktionen),
  `crates/mail-store/src/conversation_tests.rs`, `crates/mail-engine/src/comfort/mod.rs`
  (`list_inbox_conversations`, `list_inbox_conversations_filtered`, `conversation_targets`),
  `crates/mail-engine/src/parity.rs` (`list_unread_first_conversations`),
  `crates/mail-model/src/views.rs` (`ThreadListItem`, `ConversationSummary`, `ConversationIntent`),
  `app/src-tauri/src/commands/threads.rs`, `crates/mail-engine/src/ui_prefs.rs` (`ui.conversations`)
- **Messung:** `docs/testing/performance.md` (Konversationen, 23.11.2026)

Ergänzt 0017 (Threads, Thread-Liste eines Postfachs) und 0020 (sortierter Posteingang). Keine
Migration: alles läuft über die vorhandenen Indizes (`message_thread`, Schlüssel von
`message_location`, `loc_category`, `loc_important`, `loc_unseen`, eindeutige Indizes von `pin`,
`snooze`, `blocked_hidden`).

## Was eine Konversationszeile ist

**Die Konversationsliste ist die Nachrichtenliste derselben Art, nach Thread gruppiert, eine Zeile je
Thread bei seinem neuesten Mitglied.** Ein Ort ist die Zeile seines Threads, wenn keine neuere Mail
desselben Threads im selben Postfach liegt, *die in derselben Liste stünde* (gleiche Kategorie,
ungelesen, nicht zurückgestellt, nicht ausgeblendet …). Folgen:

- Jede Mail der Nachrichtenliste gehört zu genau einer Konversationszeile; nichts fehlt, nichts
  doppelt. Der Vertreter eines Threads hängt nicht vom Cursor ab, deshalb hat das Keyset über die
  Mail-ID der Zeile weder Dubletten noch Lücken (Test mit Seitengrößen 1 bis 500).
- Wie in 0017 sortiert die Zeile nach der neuesten Mail *in diesem Postfach*; eine Antwort nur in
  „Gesendet“ hebt die Konversation nicht nach oben, `lastActivity` zeigt sie.
- **Zusammenfassung** je Zeile über die Mails des Threads in allen Postfächern des Kontos: Anzahl,
  Ungelesene (in mindestens einem Postfach ungelesen, wie 0017) und `lastActivity` über alle Mails
  (Schlüssel von `message_thread` und eine Abfrage von `loc_message` je Mail); Teilnehmer
  (verschiedene Absender, älteste zuerst, ohne Groß/Klein, höchstens 4), „hat Anhang“ und
  „markiert“ über die **neuesten 200 Mails** (`SUMMARY_MESSAGES`). Ohne diese Grenze kostete die
  Zeile des langen Listen-Threads (7.620 Mails) 18 ms, weil jede Mail ihre Zeile in `message`
  lesen musste; echte Unterhaltungen sind fast immer kürzer, dort ist alles exakt. Gelesen beim
  Lesen, nicht gepflegt (wie 0017); die Thread-Liste eines Ordners nutzt dieselbe Zusammenfassung. Die Markierungen der Zeile (Kategorie, wichtig, angepinnt,
  Snooze, Aufgabe, Lese-Liste) sind die ihrer Mail.
- **Nie über Konten hinweg:** jeder `UNION ALL`-Zweig ist ein Postfach eines Kontos, `thread` gehört
  zu einem Konto. Dieselben Message-IDs in zwei Konten sind zwei Konversationen (Test).

## Posteingang-Semantik

- **Angepinnt und zurückgekehrt stehen oben, je Konversation einmal.** Eine Konversation ist
  angepinnt (zurückgekehrt), wenn irgendeine ihrer Mails angepinnt ist (einen zurückgekehrten Snooze
  hat). Die Zeilen lassen solche Threads ganz weg (eine Abfrage je Thread-Mitglied über den
  eindeutigen Index von `pin` bzw. `snooze`); oben steht der Thread einmal, als seine neueste Mail im
  Posteingang, auch wenn die angepinnte Mail selbst schon im Archiv liegt. Grund: Sonst stünde
  dieselbe Konversation oben und noch einmal in den Zeilen, sobald eine Antwort kommt.
- **Wartende Snoozes und ausgeblendete Mails blockierter Absender wirken je Mail**, wie bisher.
  „Konversation zurückstellen“ stellt alle ihre Mails im Posteingang zurück; kommt eine Antwort, ist
  die Konversation mit der Antwort als Zeile wieder da (wie Gmail). Die zurückgestellten Mails bleiben
  in „Später“, bis sie zurückkehren; dann steht die Konversation oben.
- **Priorität:** höchstens drei Konversationen wichtiger Absender (gruppierte Liste „wichtig“). Sie
  stehen wie bisher auch in den Zeilen, dort aber eventuell mit einer anderen (neueren, nicht
  wichtigen) Mail als Vertreter; die Oberfläche zeigt eine Konversation nur einmal (über die
  Thread-ID). „Alle anzeigen (n)“ zählt weiter **Mails** (ein Indexbereich); Konversationen zu
  zählen hieße `count(DISTINCT …)` mit temporärem B-Baum. Die Liste dahinter zeigt Konversationen.
- **Nur ungelesene:** Konversationen mit einer ungelesenen Mail im Posteingang, bei ihrer neuesten
  ungelesenen Mail. **Ungelesene oben:** diese, dann die gruppierte Liste der gelesenen Mails ohne die
  Threads, die eine ungelesene Mail im Postfach haben (jede Konversation einmal, Test).
- **Kategorien aus:** keine Bündel, kein Prioritätsblock, Zeilen rein zeitlich, wie bisher.

## Bündel: je Konversation

Bündel (Benachrichtigungen, Newsletter je Datumsabschnitt) enthalten **Konversationen**, nicht
Mails. Die Kategorie, „wichtig“ und „angepinnt“ der Zeile kommen von ihrem Vertreter; ungelesen ist
ein Mitglied, wenn eine Mail der Konversation ungelesen ist. Das Limit einer Seite zählt
Konversationen.

Begründung: Die Zeile der Liste ist die Konversation; ein Bündel fasst Zeilen zusammen. Eine
GitHub- oder Jira-Benachrichtigung zu einem Vorgang trägt References und bildet einen Thread; mit
Bündeln je Mail stünde derselbe Vorgang zehnmal im aufgeklappten Bündel, und die Zahl am Bündel
(„12 Benachrichtigungen“) wiche von dem ab, was beim Aufklappen erscheint. Newsletter bilden fast nie
Threads (keine References, Betreff-Rückfall für Listen gesperrt, 0013), für sie ändert sich nichts.

## Schnittstellen

- `listInboxConversations(scope, cursor, limit)` → `InboxPage` wie `listInbox`; jede Mail in
  Zeilen und `top` steht für ihre Konversation, `conversations` hält eine `ConversationSummary` je
  solcher Mail. Die Oberfläche behält ihre Zeilen-, Block- und Bündel-Logik und hängt Zähler,
  Teilnehmer und Markierungen an. `listInbox` bleibt (Konversationen aus), `conversations` ist dort leer.
- `listInboxConversationsFiltered(scope, filter, cursor, limit)` → `ThreadPage` (Kategorie,
  Priorität, nur ungelesene), `listUnreadFirstConversations(scope, cursor, limit)` →
  `UnreadFirstThreadPage`.
- `ThreadListItem` (auch `listThreads`) hat zusätzlich `participants`, `hasAttachments`, `flagged`.
- `SearchHit` hat `threadId` und `threadMessageCount`: Die Suche bleibt eine Trefferliste nach
  `rowid` (FTS); die Oberfläche fasst die geladenen Treffer je Thread zusammen (der neueste Treffer
  ist die Zeile). Eine gruppierte Suche im Store müsste den FTS-Ausdruck je Kandidat erneut prüfen.

## Aktionen auf Konversationszeilen

`conversationTargets(rows, intent)` sagt, welche Mails eine Aktion auf Konversationszeilen trifft
(`rows`: Mail und Postfach je Zeile); die Oberfläche führt danach ihre gewohnte Aktion aus — Mail-
Aktionen über die Outbox (`applyAction`, mit Rückgängig und sichtbarem Zustand), Markierungen über
ihre Commands. Die Regel steht im Kern (`ConversationIntent::reach`, getestet):

| Absicht | trifft |
|---|---|
| Archivieren, Löschen, endgültig löschen, Verschieben, Spam / kein Spam, Schlagwörter, Labels, Zurückstellen | alle Mails der Konversation im angezeigten Postfach |
| Als gelesen markieren, Markierung entfernen, Lösen (Pin), Snooze beenden, Aufgabe entfernen, von der Lese-Liste nehmen | alle Mails der Konversation in allen Postfächern (die Zeile fasst sie zusammen) |
| Als ungelesen markieren, markieren, anpinnen, Aufgabe, auf die Lese-Liste, Absender blockieren | die Mail der Zeile |

Mail-Aktionen bekommen jeden Ort einzeln (Flags gehören zum Ort), Markierungen jede Mail einmal.
Eine Zeile, deren Mail fehlt, ist `not_found` (Liste veraltet). Die Aktionen im Lesebereich wirken
weiter auf die eine geöffnete Mail. „Beim Öffnen als gelesen markieren“ ist eine Aktion auf der
Zeile und markiert die ganze Konversation als gelesen (wie Gmail); die zuerst geöffnete Mail bleibt
offen.

Warum zwei Schritte statt eines Commands je Aktion: Rückgängig, Rückfragen (fehlender Ordner,
endgültig löschen), Hinweise und das sofortige Ausblenden der Zeilen gibt es schon einmal in der
Oberfläche; ein zweiter Weg im Kern müsste all das verdoppeln.

## Einstellung

`UiPreferences.conversations` (Schlüssel `ui.conversations`, Standard **an**), synchronisiert wie
die Schriftgröße (`SYNCED_SETTINGS`). Ein unlesbarer Wert gilt als „an“. Umschalten im Kopf der
Liste, im Menü „Listenoptionen“, unter Einstellungen → Lesen und in der Befehlspalette (Gruppe
Layout); gespeichert optimistisch, ein abgelehnter Wert wird zurückgesetzt und unter Lesen erklärt
(`app/src/features/threads/preference.ts`). Gilt für Posteingang,
vereinten Posteingang, Ordner und Suchergebnisse; „Später“, „Angepinnt“, „Aufgaben“ und die
Lese-Liste bleiben Listen von Mails (dort sind Mails markiert, nicht Konversationen).

## Kosten

- Je Kandidat: die Prüfungen der Nachrichtenliste, dazu „keine neuere Mail des Threads in derselben
  Liste“ (Bereich in `message_thread`, Schlüssel des Orts) und in den Zeilen die Pin- und
  Rückkehr-Prüfung über die Mitglieder des Threads. Je Zeile die Zusammenfassung (Zählung über alle
  Mails, Absender und Markierungen über höchstens 200). Beides wächst mit der Thread-Größe (ein
  Thread mit 7.620 Mails kostet wie in 0017 einige Millisekunden).
- Gemessen bei 100.000 Mails: siehe `docs/testing/performance.md`. Plan-Tests:
  `conversation_tests::every_conversation_query_uses_its_index` (frisch, nach `ANALYZE`, neu
  geöffnet; Leser und Schreiber). Die Teilabfragen der Zusammenfassung nennen `loc_message` per
  `INDEXED BY`: ohne markierte Mail hält `PRAGMA optimize` den leeren Teilindex `loc_flagged` sonst
  für billiger (wie 0029).

## Geprüft

- Store (`conversation_tests`): Antworten mehrerer Personen und eine Antwort in „Gesendet“ ergeben
  eine Zeile mit Anzahl, Ungelesenen, Teilnehmern und Anhang; ein Thread mit 205 Mails zählt alle,
  nennt Absender, Anhang und Markierung der neuesten 200; Seiten ohne Dubletten und Lücken
  (40 Threads über Posteingang und Archiv, Seitengrößen 1–500, auch „nur ungelesene“); angepinnte
  und zurückgekehrte Konversationen oben und einmal, wartende Snoozes je Mail, eine Antwort holt die
  zurückgestellte Konversation zurück; „Ungelesene oben“ mit jeder Konversation einmal; Threads im
  vereinten Posteingang je Konto; Ziele je Reichweite; Trefferzählung der Suche; Abfragepläne.
- Engine (`comfort::tests::the_inbox_as_conversations_bundles_conversations_and_keeps_the_top`):
  über den Fake-Server Bündel aus zwei Konversationen statt vier Mails, Zusammenfassungen, Pin einer
  älteren Mail stellt die Konversation nach oben, Ziele von Archivieren, Ungelesen und Lösen, nur
  ungelesene und ungelesene oben. Der Fake-Server liest dafür `In-Reply-To`/`References` aus der
  Rohmail.
- Modell: Reichweite jeder Absicht, Drahtformat; Einstellung (Standard an, gespeichert, angekündigt,
  unlesbarer Wert = an, synchronisierter Schlüssel: `ui_prefs::tests::the_conversation_view_is_on_by_default_stored_and_synced`).
- Shell (`mail-app`, Tauri-Mock): die vier Commands mit leeren Seiten und typisierten Fehlern
  (`not_found`, `invalid_argument`), Einstellung über `get/set_ui_preferences`.
- Oberfläche (Vitest `app/tests/conversations.test.tsx` und angepasste Tests): Standard an, die vier
  Schalter, Ereignis eines anderen Geräts, abgelehnte Speicherung; Zeilen mit Zähler, Absendern,
  ungelesen und Name aus dem sichtbaren Text; Priorität ohne Doppelung; Bündel aus Konversationen;
  nur ungelesene und ungelesene oben; Suche gruppiert; Konversation aus Posteingang und vereintem
  Posteingang (axe); Löschen trifft die Konversation im Posteingang, nicht die Kopie in Gesendet,
  während „Markieren“ im Lesebereich eine Mail trifft; Anpinnen und Zurückstellen.
- Desktop-Tests (nicht ausgeführt, laufen in der Hauptsitzung): `account-sync.e2e.mjs` schaltet
  Konversationen vor dem Zählen der Zeilen gegen den Server aus (`setConversations(false)` in
  `support/app.mjs`); die folgenden Specs zählen damit weiter Mails. `search.e2e.mjs` erwartet den
  Schalter jetzt auch im Posteingang und prüft dort eine Konversationszeile.

**Ungeprüft:** echte Postfächer mit großen Threads; die Oberfläche in der echten App (Desktop-Tests
laufen in der Hauptsitzung). Offen: Die Auswahlleiste nennt bei „nicht geladen“ weiter die Zahl der
Mails des Ordners, nicht der Konversationen.

## Rückweg

- Bündel je Mail: in `sorted_inbox` die Mitglieder einer Bündelzeile über `conversationTargets`
  (Reichweite „im Postfach“) aufklappen; die Liste selbst bleibt.
- Sortierung nach Aktivität überall statt „neueste Mail im Postfach“: Tabelle
  `mailbox_thread(mailbox_id, thread_id, last_message_id)` (Rückweg aus 0017).
- Konversationen aus als Standard: eine Zeile, der Standard von `UiPreferences.conversations`.

## Nachtrag 24.11.2026: Folgen von Stufe 17 (Entscheidung 0049)

- **Öffnen:** Die Zeile öffnet nicht mehr das ganze Gespräch als Kartenstapel, sondern genau eine
  Mail: die älteste ungelesene, sonst die neueste (dieselbe Regel). Die anderen Mails klappen in der
  Liste unter der Zeile auf (alle Mails des Threads im Konto, älteste zuerst, über `getThread`).
- **Beim Öffnen als gelesen markieren** gilt der gezeigten Mail, nicht mehr dem ganzen Gespräch.
  „Gelesen“ an der Zeile wirkt weiter auf das ganze Gespräch (Tabelle oben unverändert).
- **Aktionen:** Die Tabelle gilt für Gesprächszeilen. Eine aufgeklappte Mail (Kindzeile) handelt
  für sich allein, auch in einer gemischten Auswahl.
- **Zeile:** Statt der Zahl der Mails nennt die Pille „n Antworten“ (Zahl der Mails minus eins) und
  klappt auf. Die Liste der Gespräche ist ein `tree`; die Identität einer Zeile ist ihr Thread.
- **Geprüft** zusätzlich: `conversation_tests::an_expanded_row_*` und
  `comfort::tests::an_expanded_conversation_row_lists_its_messages_from_every_folder` (Kinder und
  Zahl der Zeile stimmen überein), Vitest `conversation-tree.test.tsx`, Desktop
  `conversations.e2e.mjs`.
