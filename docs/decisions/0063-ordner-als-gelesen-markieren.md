# 0063 — Ordner als gelesen markieren, Menüs der Spalte

- **Datum:** 25.11.2026
- **Anlass:** Stufe 21, Rückmeldungen 2 und 3 zu 0.1.1: „Ordner als gelesen markieren“ im
  „…“-Menü und per Rechtsklick eines Ordners; das Menü eines Ordners ragte links aus dem Fenster
  (Konto bitaxe.de, Posteingang, Windows, dunkel) und ließ die ganze linke Spalte waagerecht
  verschoben stehen.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, Arbeitsbaum
  `wt-s21-sidebar`)
- **Code:** `crates/mail-model/src/outbox.rs` (`OperationPayload::MarkMailbox`,
  `ActionKind::MarkMailboxRead`, `OperationInfo::message_count`), `crates/mail-model/src/uid_set.rs`
  (`parse`, `split`), `crates/mail-store/src/mark_read.rs`, `crates/mail-store/src/outbox.rs`
  (Wiederanwenden, Zurücknehmen, Rückgängig, Wiederholen), Migration `0027_mark_mailbox.sql`,
  `crates/mail-engine/src/outbox/exec.rs` (`mark_mailbox`), `crates/mail-engine/src/folders.rs`
  (`AppCore::mark_mailboxes_read`), Befehl `markMailboxesRead`;
  `app/src/features/mailboxes/markRead.ts`, `organize/folders.tsx`, `mailboxes/MailboxTree.tsx`,
  `unified/UnifiedTree.tsx`, `actions/OverflowMenu.tsx`, `actions/useMenuPlacement.ts`,
  `app/SidebarColumn.tsx`
- **Tests:** `mail_store::mark_read_tests` (7), `mail_engine::outbox::tests::mark_read` (4, IMAP
  gegen den Fake), `mail_jmap::tests::a_folder_mark_sets_seen_in_batches_and_its_undo_clears_the_same`,
  Servertest `outbox::server_tests::a_folder_marked_read_is_seen_on_the_server_and_undone_on_the_same_messages`
  (Stalwart), `uid_set::tests` (Lesen, Teilen), Vitest `mark-folder-read.test.tsx` (mit axe),
  Desktop `mark-folder-read.e2e.mjs`

## Entscheidung

**1. Eine Operation je Ordner, nicht je Mail.** Der Kern kannte nur Flags je Mail
(`set_flag`, eine Zeile in `operation` je Mail, höchstens 10.000 je Aktion, auf CONDSTORE-Servern
ein `STORE` je bekanntem MODSEQ). Für „alles in diesem Ordner“ wäre das bei Tausenden ungelesener
Mails Tausende Zeilen und Befehle. Neu ist die Outbox-Operation `mark_mailbox` (Aktion
`mark_mailbox_read`): Ordner, UIDVALIDITY und die Menge der UIDs, die beim Klick ungelesen waren,
als Sequenzmenge im Payload (`"1:40,52"`), dazu `seen` (gelesen setzen oder, beim Rückgängig,
wieder wegnehmen). Mehrere Ordner eines Kontos (das Konto: Posteingang und eigene Ordner) sind ein
Stapel, also ein Rückgängig.

**2. Die gespeicherten UIDs, nicht `1:*`.** `UID STORE 1:* +FLAGS (\Seen)` würde auch Mail
gelesen machen, die nach dem letzten Abgleich ankam und die der Mensch nie gesehen hat. Die
gespeicherten UIDs treffen genau, was die App als ungelesen zeigte; eine UID, die inzwischen
gelöscht oder verschoben wurde, übergeht der Server (UIDs werden in einer UIDVALIDITY nie neu
vergeben). Ohne `UNCHANGEDSINCE`: die Absicht „alles hier gelesen“ gilt auch für eine Mail, die
anderswo markiert oder verschlagwortet wurde; als ungelesen markiert hat sie in diesem Fall
höchstens ein anderes Programm nach unserem letzten Abgleich, und der Mensch hat den Ordner
ausdrücklich als gelesen markiert. Gesendet wird in Teilen von höchstens 250 Bereichen und 5.000
UIDs je Befehl (Zeile unter den 8.192 Bytes aus RFC 7162, Antwort begrenzt); JMAP geht über
denselben `store_flags` des Adapters: `Email/set` mit `keywords/$seen` in Paketen von
`maxObjectsInSet` (über die UID-Zuordnung des Adapters, ohne eigenes `Email/query`, damit IMAP und
JMAP dieselbe Menge treffen).

**3. Sofort sichtbar, absturzsicher.** Beim Aufzeichnen werden genau diese Orte gelesen (eine
Transaktion mit der Operation), die Zähler fallen sofort. Ein Abgleich, der noch den alten Stand
des Servers schreibt, wendet ausstehende Markierungen wieder an, in der Reihenfolge der Aufzeichnung
zusammen mit den Flag-Operationen je Mail (eine danach als ungelesen markierte Mail bleibt
ungelesen). Nach einem Absturz oder Verbindungsabbruch wird eine Operation `in_flight` einfach noch
einmal gesendet: ein gesetztes Flag noch einmal setzen ändert nichts. Weist der Server einen Teil
ab, ist die Operation `failed` und die Orte werden wieder ungelesen (bereits gesendete Teile zeigt
der nächste Abgleich); neu aufgebaut (andere UIDVALIDITY) oder verschwunden ist `conflict`, der
Server gewinnt.

**4. Rückgängig.** Noch wartend: gestrichen, die Orte wieder ungelesen. Gesendet: eine
Gegen-Operation `\Seen` weg auf derselben Menge (auch von einer Mail, die zwischendurch geöffnet
wurde; neue Mail bleibt unberührt). Hat der Ordner inzwischen eine andere UIDVALIDITY, ist es
`not_undoable`. Wiederholen nimmt dieselbe Menge, solange der Ordner in derselben Generation ist.

**5. Oberfläche.** „Ordner als gelesen markieren“ ist der erste Eintrag im „…“ jedes Ordners (Baum
des Kontos und Favoriten); Rechtsklick, Umschalt+F10 und Kontextmenütaste öffnen dasselbe Menü.
„Alle als gelesen markieren“ steht im „…“ der Kontozeile und meint, was ihr Zähler zählt:
Posteingang und eigene Ordner (nicht Gesendet, Entwürfe, Spam, Papierkorb, Gmails „Alle
Nachrichten“). Die Zeilen „Alle Posteingänge“ und die Kontogruppen bekommen ein „…“ mit „Alle als
gelesen markieren“ für die Posteingänge ihrer sichtbaren Konten: ein Aufruf je Konto, ein Hinweis
mit „Rückgängig“ für alle; nur lesende Konten übergeht es. Die Ansichten der Spalte (Später,
Aufgaben, Markiert …) bekommen den Eintrag nicht: sie liegen quer über alle Ordner, „alles gelesen“
wäre dort jeder Ordner jedes Kontos. Ohne ungelesene Mail fehlt der Eintrag (das Menü bleibt kurz;
der Zähler zeigt schon, dass nichts zu tun ist). Bei einem nur lesenden Konto bleibt er im Menü,
als nicht verfügbar markiert, mit dem Grund als zweite Zeile („Änderungen sind für dieses Konto
ausgeschaltet.“, auch als Beschreibung für Screenreader); ein Klick zeigt wie bei „Umbenennen“ den
Hinweis mit „Änderungen erlauben …“. Ein Tastenkürzel gibt es nicht (Thunderbirds Umschalt+C ist
hier frei, aber ein Kürzel für eine Spaltenzeile ohne Fokusbezug wäre missverständlich; die
Befehlspalette bleibt für einen späteren Schritt). Die Aktivität zeigt den Vorgang mit der Zahl
der Mails (`messageCount`).

**6. Menüs der Spalte (Rückmeldung 3).** Ursache: das Menü hängt am Ende des „…“-Knopfs und wuchs
mit seinem längsten Eintrag (12 rem waren nur die Mindestbreite) nach links über den Rand der
Spalte; die Spalte scrollte zwar „nur“ mit `overflow-x: hidden`, das verhindert aber nur
Bildlaufleisten, nicht das Verschieben durch den Fokus (`focus()` scrollt das Element in den
sichtbaren Bereich) oder durch das Rad. Lösung:
- Menüs der Spalte (`OverflowMenu narrow`) sind so breit wie die Zeilen der Spalte (Breite der
  Spalte minus ihr Rand, höchstens 16 rem) und nie breiter; Einträge brechen um statt abgeschnitten
  zu werden, die Zeile wächst mit (mindestens 32 px).
- Im Bildlaufbereich der Spalte liegt ein Block mit `overflow-x: clip` (auf dem Bildlaufbereich
  selbst würde `clip` neben `overflow-y: auto` zu `hidden`): nichts darin trägt mehr zur
  waagerechten Breite bei; falls doch einmal verschoben, setzt `onScroll` `scrollLeft` auf 0. Der
  Fokus geht mit `preventScroll` in Menüs.
- Jedes `OverflowMenu` bleibt im sichtbaren Bereich (`useMenuPlacement`): Fenster, geschnitten
  durch jeden Vorfahren, der in einer Achse abschneidet. Ohne Platz darunter und mit mehr darüber
  öffnet es sich nach oben (`data-side="above"`), an den Seiten wird es zurückgeschoben (CSSOM
  `translate`, kein Style-Attribut wegen CSP), passt es nirgends ganz, bekommt es eine kleinere
  Höhe und rollt. Gemessen einmal beim Öffnen, vor dem Zeichnen, ohne React-Zustand.

## Verworfen

- **Eine `set_flag`-Operation je Mail** (bestehende Aktion `markRead` auf alle Ungelesenen): fertig
  vorhanden, aber Tausende Zeilen, die Grenze von 10.000 Mails je Aktion, ein `STORE` je MODSEQ und
  eine Aktivität mit Tausenden Einträgen.
- **`UID STORE 1:*`**: siehe 2. Auch `1:<höchste bekannte UID>` verworfen: es träfe Mails, die ein
  anderes Programm nach unserem Abgleich als ungelesen markiert hat, und das Rückgängig wäre nicht
  mehr genau.
- **JMAP mit eigenem `Email/query` nach ungelesenen Mails des Ordners**: fände auch Mail, die die
  App noch nicht kennt (wie `1:*`), und bräuchte einen zweiten Weg neben der UID-Zuordnung.
- **Menü als `position: fixed` an den Fensterrand gehängt**: bliebe beim Rollen der Spalte nicht
  am Knopf; das Umklappen und Zurückschieben im sichtbaren Bereich genügt.
