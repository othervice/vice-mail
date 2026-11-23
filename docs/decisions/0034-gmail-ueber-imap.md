# 0034 — Gmail über IMAP: Labels, eine Mail über `X-GM-MSGID`, Archivieren als Label

**Status:** entschieden (Stufe 14, Block A) · **Datum:** 23.11.2026 · **Code:** `crates/mail-model/src/gmail.rs`,
`crates/mail-imap/src/{convert,backend,writer}.rs`, `crates/mail-store/src/gmail.rs`, Migration
`0014_gmail.sql`, `crates/mail-engine/src/gmail.rs`, `app/src/features/reading/GmailLabels.tsx`

## Kontext

Gmail funktioniert seit Stufe 2 über IMAP mit App-Passwort, hat aber Labels statt Ordner. IMAP zeigt
jedes Label als Ordner; dieselbe Mail liegt also gleichzeitig im Posteingang, unter jedem ihrer
Labels und in „Alle Nachrichten“ (`[Gmail]/All Mail`, SPECIAL-USE `\All`). Mit `X-GM-EXT-1` sagt der
Server, welche Orte dieselbe Mail sind (`X-GM-MSGID`, 64 Bit) und welche Labels sie trägt
(`X-GM-LABELS`, modifiziertes UTF-7 wie Ordnernamen). Archivieren heißt bei Gmail „Label Posteingang
entfernen“; gesendete Mails legt Gmail selbst in „Gesendet“ ab.

Bisher wurden Orte über die `Message-ID`-Kopfzeile zu einer Mail zusammengeführt (0003). Das trägt bei
Gmail nicht ganz: Mails ohne Message-ID werden nie zusammengeführt, und zwei verschiedene Mails mit
derselben Message-ID würden fälschlich eine. Archivieren verlangte einen Archiv-Ordner, den Gmail
nicht hat; „Alle Nachrichten“ als Ziel eines MOVE wäre falsch (siehe 6.).

Einen Gmail-Testserver gibt es nicht. imap-proto 0.17 liest `X-GM-MSGID`, `X-GM-THRID` und
`X-GM-LABELS` bereits (`AttributeValue::GmailMsgId`, `GmailLabels`).

## Entscheidungen

1. **Erkennen.** Gmail ist ein Server mit `X-GM-EXT-1` (`Capabilities::gmail`). Der Abgleich
   vermerkt bei jeder Anmeldung, ob er es sah, in der neuen Tabelle `account_server` (Klasse Gerät,
   nicht an `account`, dessen Update-Trigger den Einstellungs-Sync füttert). Planer und
   Gesendet-Einstellung lesen diesen Vermerk, also auch offline. Für die Gesendet-Kopie und die
   Hinweise zählt zusätzlich der Servername (`imap.gmail.com`, `*.googlemail.com`,
   `mail_model::is_gmail_host`), damit ein neues Konto schon vor der ersten Anmeldung stimmt.
2. **Eine Mail über `X-GM-MSGID`.** Der Adapter holt auf Gmail mit den Kopfdaten
   `X-GM-MSGID X-GM-LABELS` (in derselben FETCH-Zeile, kein zusätzlicher Round-Trip). Der Speicher
   führt einen Ort mit der Mail derselben Nummer zusammen (`message.gm_msgid`, Teilindex je Konto);
   eine vor dieser Stufe gespeicherte Mail ohne Nummer wird über die Message-ID gefunden und
   bekommt die Nummer. Verschiedene Nummern sind verschiedene Mails, auch bei gleicher Message-ID.
   Für andere Server bleibt die Regel aus 0003 unverändert (die neuen Spalten bleiben NULL).
   Folgen: eine Zeile je Mail in Suche, Threads und vereintem Posteingang (der ohnehin nur
   Posteingänge zeigt); die Ordnerzähler bleiben je Ordner, wie Gmail sie auch zeigt. Neue Mails
   zählt `newMail` auf Gmail nur, wenn eine neue `message`-Zeile entsteht
   (`StoreReport::new_unread`): eine Mail im Posteingang und in „Alle Nachrichten“ wird einmal
   gemeldet. Auf anderen Servern zählt das Fenster wie bisher.
3. **Labels anzeigen.** `X-GM-LABELS` kommt mit den Kopfdaten und mit jedem Flag-Abgleich (auch mit
   `CHANGEDSINCE`). Beides sind reine Datenelemente von FETCH und laufen über die Nur-Lese-Allowlist
   (EXAMINE), ein Test beweist es. Gespeichert wird die Liste je Mail (`message.gm_labels`, JSON,
   dekodiert, Systemlabels als `\Inbox`, `\Important`, `\Starred`, `\Sent`, `\Draft`, `\Spam`,
   `\Trash`), ergänzt um das Label des Ordners, in dem gelesen wurde: manche Gmail-Server lassen das
   Label des geöffneten Ordners in `X-GM-LABELS` weg. Angezeigt wird die gespeicherte Liste plus die
   Labels, die die Orte der Mail ergeben (so haben auch vor dieser Stufe gespeicherte Mails ihre
   Labels). Die Oberfläche zeigt sie im Lesebereich (Command `get_message_labels`); Benutzerlabels
   und „Wichtig“/„Markiert“ als Chips, die übrigen Systemlabels sind ohnehin eigene Ordner.
4. **Labels ändern, nur über die Outbox.** Ein Label ist ein `Flag::Label` in einer
   `SetFlag`-Operation; dadurch gelten Zustände, Aufheben von Gegenteilen, Rückgängig, Wiederholen
   und die Aktivitätsansicht unverändert. Der Writer sendet `UID STORE uids ±X-GM-LABELS (...)`:
   Systemlabels als Atom, Benutzerlabels in modifiziertem UTF-7 und in Anführungszeichen, ungültige
   Namen (Steuerzeichen, `[Gmail]/…`) nie. Kein `UNCHANGEDSINCE` (bei Gmail nicht dokumentiert), und
   ein Label wird immer gesendet, auch wenn es laut Server schon so ist (idempotent, und siehe die
   Lücke in 3.). Die lokale Wirkung liegt an der Mail, nicht am Ort. Neue Aktionen `addLabel` und
   `removeLabel`; nur auf Gmail-Konten (`ActionError::LabelsUnsupported` → `unsupported_server`),
   nur mit freigeschaltetem Schreibzugriff. Für den Lesebereich wählt der Kern den Ort
   (`set_message_label`, „Alle Nachrichten“ zuerst). **Nur-Lese-Schutz:** STORE ist nicht auf der
   Allowlist; nur die eigene Verbindung der Outbox (`Access::ReadWrite`) darf es, die Verbindung des
   Einstellungs-Syncs lehnt es ab (Tests in `crates/mail-imap/src/tests/gmail.rs`).
5. **Archivieren auf Gmail entfernt `\Inbox`** vom Ort der Mail im Posteingang (gleich, aus welchem
   Ordner archiviert wird), statt in einen Archiv-Ordner zu verschieben; es braucht keinen. Lokal
   verschwindet die Mail sofort aus dem Posteingang wie bei einem Verschieben, ein Fehlschlag oder
   Abbrechen setzt den Ort mit seinen Flags zurück. Rückgängig nach der Bestätigung setzt `\Inbox`
   über den Ort in „Alle Nachrichten“ wieder, denn im Posteingang ist die Mail auf dem Server nicht
   mehr; ohne bekannten Ort dort ist es `not_undoable` mit Bitte um den nächsten Abgleich.
6. **Verschieben aus „Alle Nachrichten“ setzt nur das Ziel-Label.** Ein MOVE ist COPY plus EXPUNGE;
   ein EXPUNGE in „Alle Nachrichten“ kann die Mail bei Gmail in den Papierkorb schicken. Liegen alle
   gewählten Mails dort und ist das Ziel der Posteingang oder ein Benutzerlabel, wird daraus
   `addLabel`. Aus einem Label-Ordner bleibt es ein normales MOVE (Gmail macht daraus Label setzen
   und Label entfernen). Löschen und Spam bleiben MOVE in Papierkorb bzw. Spam.
7. **Gesendet-Kopie.** `sentCopy: auto` heißt „der Server legt selbst ab“, wenn das Konto Gmail ist
   (Servername oder `X-GM-EXT-1`), sonst „die App legt ab“. `AccountSendSettings.gmail` sagt es der
   Oberfläche; die Kontoeinstellungen zeigen bei Gmail einen Hinweis (Labels, Archivieren,
   Gesendet), und wenn der Nutzer `app` gewählt hat, dass dann zwei Kopien entstehen.
8. **Einstellungs-Sync auf Gmail.** Der Sync-Ordner ist dort ein Label; seine Datensätze liegen
   auch in „Alle Nachrichten“. Der Abgleich lässt Kopfdaten weg, deren `X-GM-LABELS` den Sync-Ordner
   nennen, sie erscheinen also nicht als Mails.
9. **Kontoeinrichtung.** Der Gmail-Hinweis erklärt App-Passwort und IMAP (bei privaten Konten seit
   Januar 2025 immer an, bei Google Workspace muss die Verwaltung es erlauben). Er erscheint auch,
   wenn die Online-Suche oder der Nutzer Gmails Server eingibt (Workspace-Domains).

## Migration

`0014_gmail.sql`, nur additiv: `message.gm_msgid` (INTEGER, Bitmuster der vorzeichenlosen Zahl),
`message.gm_labels` (JSON), Teilindex `message_gm_msgid`, Tabelle `account_server`. Die Nummer 0013
hat Stufe 16 belegt.

## Getestet (ohne echtes Gmail-Konto)

- Aufgezeichnete Gmail-Antworten gegen den Adapter (`crates/mail-imap/src/tests/gmail.rs`):
  `gmail_headers_and_flags_carry_message_ids_and_labels_read_only`,
  `other_servers_are_not_asked_for_gmail_items`,
  `labels_are_stored_only_on_the_writer_and_encoded_like_mailbox_names`, `labels_need_gmail`,
  `read_only_allowlist_and_settings_sync_scope_know_gmail_items`.
- Speicher (`crates/mail-store/src/gmail_tests.rs`): eine Mail über Labels und ein Zählen,
  Labels aus dem Flag-Abgleich, alte Zeilen bekommen ihre Nummer, andere Server unverändert, Labels
  über die Outbox mit Zurücknehmen, Archivieren und Rückgängig über „Alle Nachrichten“, Verschieben
  aus „Alle Nachrichten“.
- Outbox gegen den Fake-Server (`crates/mail-engine/src/outbox/tests/gmail.rs`), Abgleich
  (`sync::pass::tests::gmail_settings_sync_records_stay_out_of_all_mail`), Kontoerkennung
  (`discovery::tests::workspace_domains_get_the_gmail_hint_from_their_servers`), Oberfläche
  (`app/tests/gmail-labels.test.tsx`, `account-setup.test.tsx`).
- Für Nicht-Gmail-Server: die vorhandenen Server-Tests gegen Stalwart.

## Nur am echten Gmail-Konto prüfbar (Handprüfung)

1. Anmeldung mit App-Passwort; `X-GM-EXT-1` wird erkannt (Kontoeinstellungen zeigen den Gmail-Hinweis).
2. Eine Mail mit zwei Labels erscheint im Posteingang, in beiden Label-Ordnern und in „Alle
   Nachrichten“ als dieselbe Mail (Gelesen in einem Ordner gilt nach dem Abgleich überall), und
   eine neue Mail meldet sich einmal.
3. Die Labels im Lesebereich stimmen mit der Gmail-Weboberfläche überein, auch nach einer Änderung
   im Web (kommt mit `CHANGEDSINCE`, wenn Gmail dafür die MODSEQ erhöht; sonst erst, wenn sich der
   Ort ändert). Ob Gmail das Label des geöffneten Ordners in `X-GM-LABELS` weglässt, ist offen; die
   App kommt mit beidem zurecht.
4. Label hinzufügen und entfernen aus dem Lesebereich wirkt im Web; Umlaute im Labelnamen.
5. Archivieren aus dem Posteingang: Mail bleibt in „Alle Nachrichten“, verliert nur „Posteingang“,
   kein Fehler bei `STORE -X-GM-LABELS (\Inbox)` im geöffneten Posteingang; Rückgängig bringt sie
   zurück (nach dem nächsten Abgleich wieder im Posteingang).
6. Senden: genau eine Kopie in „Gesendet“.
7. Verschieben aus „Alle Nachrichten“ in ein Label: Mail bleibt in „Alle Nachrichten“, bekommt das Label.
8. Löschen (MOVE nach `[Gmail]/Trash`) und Spam verhalten sich wie im Web.
9. Einstellungs-Sync auf Gmail: die Datensätze erscheinen nicht in „Alle Nachrichten“ der App.

## Grenzen und Nicht in diesem Block

- Alte Datensätze des Einstellungs-Syncs bleiben auf Gmail in „Alle Nachrichten“ (EXPUNGE im
  Label-Ordner entfernt nur das Label); im Web sind sie sichtbar. Ein Aufräumen über
  `X-GM-LABELS`/Papierkorb kommt nur, wenn es stört.
- Labels werden in der Liste und bei Mehrfachauswahl nicht angezeigt oder geändert, nur im
  Lesebereich; Labels anlegen, umbenennen oder löschen geht wie bei Ordnern (anlegen) bzw. im Web.
- `X-GM-THRID` wird nicht genutzt; Threads bleiben wie in 0013.
- imap-proto liest Labels als Atom oder Zeichenkette, nicht als Literal. Gmail schickt Labels in
  modifiziertem UTF-7, also 7-Bit; ein Literal würde die Antwort unlesbar machen (Protokollfehler).
- Anmeldung mit Google (OAuth) und die Gmail-API sind Block C, Microsoft Block B.
