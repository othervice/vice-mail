# 0045 — MCP: Papierkorb und Spam auch beim Lesen per ID nur mit Postfachangabe

- **Datum:** 23.11.2026
- **Anlass:** Sicherheitsprüfung Stufe 15, Befund F4 (`docs/SECURITY_MODEL.md`, 7.1). Die Suche
  und `list_messages` lassen Papierkorb und Spam weg, solange der Aufruf sie nicht nennt (0021).
  `read_message` und `read_thread` lieferten aber jede Mail eines freigegebenen Kontos, deren ID
  bekannt war, und IDs sind fortlaufend. Ein Client (oder eine eingeschleuste Anweisung, die ihn
  steuert) konnte so Spam lesen, ohne das Postfach zu nennen, und damit die Anweisungen, die gerade
  dort stehen.
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-mcp/src/access.rs` (`readable_in`, `MailAccess::message` und
  `MailAccess::thread` mit `named`), `crates/mail-mcp/src/exec.rs`, `crates/mail-mcp/src/tools.rs`
  (Argument `mailbox_id`, Beschreibungen), `crates/mail-engine/src/mcp/access.rs`
- **Tests:** `crates/mail-engine/src/mcp/tests.rs`
  (`reading_by_id_leaves_out_mail_only_in_trash_or_junk`, echter Speicher),
  `crates/mail-mcp/tests/contract.rs`
  (`mail_only_in_trash_or_junk_is_read_only_when_its_mailbox_is_named`, über rmcp, Hilfsprogramm
  und Host), `crates/mail-mcp/src/access.rs` (`mail_only_in_trash_or_junk_needs_its_mailbox_named`)

## Regel

Eine Mail ist für `read_message` und `read_thread` lesbar, wenn sie **irgendwo außerhalb von
Papierkorb und Spam** liegt oder in dem Postfach, das der Aufruf mit `mailbox_id` nennt.
Papierkorb und Spam sind die Postfächer des Kontos mit der Rolle `trash` bzw. `junk`.

- **Neues, optionales Argument `mailbox_id`** für beide Werkzeuge, benannt wie bei
  `search_messages` und `list_messages`. Es erweitert nur: Liegt die Mail ohnehin außerhalb von
  Papierkorb und Spam, spielt es keine Rolle. Eine ID, die zu keinem Postfach der Mail gehört
  (auch eines anderen Kontos), erweitert nichts; der Host prüft sie deshalb nicht gegen die Konten
  und verrät nichts über fremde Postfächer. Das ergänzt die Werkzeugtabelle aus 0015.
- **Ablehnung wie „gibt es nicht“.** Ohne passendes Postfach gilt die Mail als nicht vorhanden:
  derselbe Text wie für eine unbekannte ID oder eine Mail eines nicht freigegebenen Kontos. Der
  Text nennt die Regel für jeden Fall gleich („A message that lies only in Trash or Junk is read
  only with the mailbox_id of that mailbox“), damit ein Client weiß, wie er Papierkorb oder Spam
  liest, ohne zu erfahren, ob es die ID gibt.
- **Thread.** Ist die Ausgangsmail nicht lesbar, wird der Aufruf abgelehnt wie bei
  `read_message`. Sonst fehlen Mitglieder, die nur in Papierkorb oder Spam liegen (und nicht im
  genannten Postfach). Sie fallen weg, **bevor** die neuesten 25 gewählt werden, und zählen nicht
  zu `total`; `older_not_shown` verrät sie also nicht. Bei Threads mit mehr als 1.000 Mails kann
  `total` noch weggelassene ältere Mitglieder enthalten (der Speicher liefert nur die neuesten
  1.000).
- **Antwort-Entwurf.** `create_draft` mit `in_reply_to` auf eine Mail nur in Papierkorb oder Spam
  wird abgelehnt („in_reply_to is not a message of this account“). Das Zitat der Antwort läge
  sonst im Entwürfe-Ordner und wäre dort ohne Postfachangabe lesbar. `create_draft` bekommt kein
  `mailbox_id`: Antworten auf Spam sind kein Anwendungsfall, der das rechtfertigt.
- **Mehrere Orte.** Eine Mail in Posteingang und Papierkorb bleibt ohne Angabe lesbar (sie ist
  nicht nur gelöscht). Die Suche ist hier strenger und lässt jede Mail weg, die auch im Papierkorb
  liegt; das bleibt so, weil sie ohne `in:` nur „sauberen“ Bestand zeigen soll.

## Wo die Regel gilt

Im Kern (`CoreAccess`), wie das Weglassen bei Suche und Listen: Nur dort lässt sich der Thread
vor der Auswahl der neuesten Mitglieder filtern und `total` richtig zählen. Der Host reicht
`mailbox_id` durch. `mail_mcp::readable_in` ist die eine Fassung der Regel; der Kern und die
Testattrappe in `crates/mail-mcp/tests/support` nutzen sie.

## Verworfen

- **Regel nur im Host** (nach dem Aufruf mit den Rollen aus `mailboxes`): Der Host bekommt nur die
  neuesten 25 Thread-Mitglieder und eine Gesamtzahl; weggefilterte Mitglieder hätten das Fenster
  verkleinert und in `total` weiter gezählt.
- **Eigene Fehlermeldung „liegt im Papierkorb“:** würde verraten, dass es die ID gibt.
- **Jede Mail ablehnen, die auch in Papierkorb oder Spam liegt** (wie die Suche): Eine Mail, die
  zugleich im Posteingang liegt, ist für den Nutzer nicht gelöscht; der Befund betrifft nur Mail,
  die ausschließlich dort liegt.
