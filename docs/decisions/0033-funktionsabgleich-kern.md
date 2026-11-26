# 0033 — Funktionsabgleich: der Kern (Stufe 16)

Stand 23.11.2026. Kernseite der Stufe 16 (0032): Datenmodell, Engine, Commands und Tests. Die
Oberfläche baut darauf auf; ihre Teile (Ansichten, Tasten, axe) stehen in der Stufendatei.

## Kontext

0032 legt fest, welche Mailbird-Funktionen kommen. Offen war, wie sie in die bestehenden
Mechanismen passen: lokale Markierungen (Stufe 9), Outbox (0012), Einstellungs-Sync (0014, 0022),
Vorlagen (0023) und Benachrichtigungen (0020). Alle Schemaänderungen stehen in einer Migration
`0013_feature_parity.sql`.

## Entscheidungen

1. **Lese-Liste („Zum Lesen“) ist eine lokale Markierung wie Aufgaben,** Tabelle `reading_list`
   (Konto-UUID, Message-ID, hinzugefügt), Datenart `readingList` im Sync (je Mail gewinnt die
   neuere Fassung). Anders als Aufgaben setzt sie **kein Schlagwort** auf dem Server, auch nicht
   bei freigeschaltetem Schreibzugriff: „ändert nichts auf dem Server“. Listenzeilen tragen
   `readingList`; die Ansicht kommt über die Markierungsansichten (`list_reading_list`, neueste
   zuerst, höchstens 1.000). Mails ohne Message-ID lassen sich nicht vormerken
   (`invalid_argument`), wie bei Aufgaben.
2. **Ungelesen.** „Nur ungelesene“ ist der Posteingangsfilter `InboxFilter::Unread` über den
   vorhandenen Teilindex `loc_unseen` (Keyset, `UNION ALL` je Postfach). „Ungelesene oben“ ist ein
   eigener Command `list_unread_first`: erst die ungelesenen (wie oben), dann die gelesenen; der
   Cursor nennt die Phase (`UnreadFirstCursor { phase, before }`). Die gelesenen laufen über den
   Primärschlüssel von `message_location` mit `seen = 1` als Filter: die übersprungenen
   ungelesenen Zeilen kosten je einen Schritt im B-Baum der Tabelle selbst (keine Suche je Zeile);
   ein eigener Teilindex auf gelesene Zeilen wäre fast so groß wie die Tabelle und würde jedes
   Abgleichsfenster verteuern. Ein Plan-Test prüft alle Abfragen (`stage16_tests`). Welche
   Ansichten „Ungelesene oben“ nutzen, ist eine **Geräte-Einstellung** `ui.unread_first` (Liste von
   Ansichtsschlüsseln, die die Oberfläche wählt, höchstens 200), nicht abgeglichen wie die
   übrigen Ansichts- und Layoutzustände (0025).
3. **Lieblingsordner und Ordnerfarben** liegen in `mailbox_pref` (Konto-UUID, dekodierter Pfad,
   Position unter den Favoriten, Farbindex 0–7 der Kontofarben), Datenart `mailboxPref` mit der
   neuen ID-Form `<Konto-UUID>/<Pfad>`. Ein Ordner wird geräteübergreifend so benannt wie bei den
   lokalen Regeln (Konto-UUID und dekodierter Pfad); lokale IDs wandern nie. Eine Zeile gibt es
   nur, solange Favorit oder Farbe gesetzt ist. `Mailbox` trägt `favoritePosition` und `color`
   (beide optional); `list_favorite_mailboxes` liefert die Favoriten aller Konten in ihrer
   Reihenfolge. Die Farbe ist nie allein Bedeutungsträger (DESIGN), die Oberfläche zeigt immer
   den Namen.
4. **Zwischen Konten verschieben und kopieren** läuft als `append` in der Outbox des Zielkontos
   mit der neuen Quelle `AppendSource::Transfer { file, flags, remove }`:
   - Vor dem Holen der Bytes wird geprüft (nichts aufgezeichnet): Zielkonto mit Schreibzugriff,
     beim Verschieben auch das Quellkonto (`read_only`); Ziel vorhanden und wählbar; kein Ordner
     des eigenen Kontos (das ist ein gewöhnliches Verschieben); Message-ID vorhanden (ohne sie
     ließe sich die Kopie nicht bestätigen); höchstens 500 Mails je Aufruf.
   - Die ganze Mail (gespeicherter Body, wenn vollständig, sonst über die Leseverbindung
     geholt) wird unter `compose/outgoing/<id>/transfer.eml` eingefroren, unter der Sperre der
     Compose-Dateien; das Aufräumen behält sie, solange die Operation nicht bestätigt ist.
   - Ausführung wie jedes `append`: vor dem Befehl UIDNEXT des Ziels als Marke, nach einem
     Absturz sucht die Prüfung die Message-ID oberhalb der Marke. Gefunden: bestätigt, **nicht**
     erneut angehängt. Nicht gefunden: erneut angehängt. So entsteht nie eine zweite Kopie, und
     nichts geht verloren. Die Flags `\Seen`, `\Answered`, `\Flagged`, `\Draft` der Quelle gehen
     mit; Schlagwörter nicht (der Adapter hängt ohne Schlagwörter an).
   - Erst die **bestätigte** Kopie (mit gefundenem Ort im Ziel) zeichnet in derselben
     Transaktion im Quellkonto ein `expunge` auf (Aktion `move`, gleicher Batch). Ohne gefundenen
     Ort bleibt die Quelle stehen (Warnung im Log). Wurde die Quelle inzwischen anderswo
     entfernt, gibt es nichts mehr zu tun. `OutboxChange.others` trägt Änderungen anderer Konten;
     der Emitter weckt deren Worker.
   - Kopieren ist die neue Aktionsart `copy`; Verschieben bleibt `move`.
5. **Absender blockieren** ist eine eigene Tabelle `blocked_sender` (normalisierte Adresse),
   Datenart `blockedSender`, nicht eine Regel in `rule`: Liste und Aufheben brauchen einen
   Schlüssel je Adresse, und Regeln laufen erst nach dieser Prüfung. Neue Mail im Posteingang
   von einer blockierten Adresse geht über die Outbox in den Spam-Ordner (`MailAction::Spam`),
   wenn das Konto schreiben darf und einen hat; sonst wird sie **ausgeblendet** (`blocked_hidden`,
   Gerät, Konto-UUID und Message-ID, eine Indexsuche je Listenzeile der Posteingangslisten). In
   beiden Fällen laufen keine Regeln und es gibt keine Benachrichtigung. Aufheben zeigt
   Ausgeblendetes wieder (Trigger). Ausgeblendet wird in den Posteingangslisten (`list_inbox`,
   `list_inbox_filtered`, `list_unread_first`), nicht in Suche, Ordnerliste oder vereinter
   Liste der Stufe 7.


## Senden und archivieren, Lesebestätigung, Vorlagen mit Empfängern, Benachrichtigungstöne

### Senden und archivieren (Aufgabe 5)

- **Je Entwurf und als Einstellung.** `draft.archive_original` (Spalte, Migration 0013) wird mit
  `setDraftSendOptions` gesetzt; die Compose-Einstellung `archiveOnReply` (Schlüssel
  `compose.archive_on_reply`, abgeglichen wie die Sendeverzögerung) schaltet es für neue
  Antworten und Antworten an alle ein, nicht für neue Mails und Weiterleitungen. Eine eigene
  Taste der Oberfläche setzt die Option und sendet.
- **Erst nach der Annahme.** `SendJob.archive_original` wird beim Senden eingefroren. In der
  Transaktion, die die Annahme bestätigt (`send_followups`, auch bei „als gesendet behandeln“
  nach einem unklaren Senden), plant der Store für die Orte des Originals (Konto, Message-ID)
  die normale Aktion `archive` (`plan_action`), als Verschieben-Operationen der Outbox im Batch
  des Sendens. Orte in Papierkorb, Spam, Gesendet und Entwürfe bleiben, ebenso schon archivierte.
  Schlägt das Senden fehl oder wird es abgebrochen, entsteht nichts (Test).
- **Kein Archivordner** ist kein Fehler des Sendens: das Senden bleibt bestätigt, die Nutzdaten
  vermerken `archive_missing`, `OutgoingMail.archive` zeigt `no_archive_mailbox`; das Original
  bleibt. Ein neuer `OperationNotice` hätte die `CHECK`-Bedingung der Tabelle `operation`
  geändert (Neuaufbau der Tabelle); deshalb steht der Hinweis in den Nutzdaten.

### Lesebestätigung (Aufgabe 8, RFC 8098)

- **Anfordern** je Entwurf (`draft.request_receipt`, `setDraftSendOptions`): Kopfzeile
  `Disposition-Notification-To` mit der Adresse der sendenden Identität; auch die Kopie im
  Entwürfe-Ordner trägt sie (neue Revision).
- **Eingehende Anforderungen** werden beim Lesen erkannt (`getReadReceipt`, aus dem gespeicherten
  Body), nicht beim Eintreffen: Es gibt keinen Weg vom Eintreffen zum Senden. Beachtet wird nur
  genau eine gültige Adresse; mehrere Adressen oder mehrere Kopfzeilen gelten als keine
  Anforderung. Eigene Mails und Lesebestätigungen selbst fragen nie (RFC 8098 2.1). Weicht die
  Adresse vom `Return-Path` ab (oder fehlt er), meldet `returnPathDiffers` das für die Frage.
- **Senden nur auf ausdrücklichen Befehl** (`sendReadReceipt`), nie automatisch. Einstellung
  `compose.read_receipts` (abgeglichen): `ask` (Standard) oder `never`; bei `never` bietet die
  Oberfläche nichts an (`answer: off`), und der Befehl wird mit `invalid_argument` abgelehnt
  (kein neuer Fehlercode: der Fall tritt nur bei einem Fehler der Oberfläche ein).
- **Aufbau:** `multipart/report; report-type=disposition-notification` mit lesbarem Text
  (Englisch oder Deutsch, Sprache vom Befehl) und `message/disposition-notification` mit
  `Final-Recipient` (Identität), `Original-Recipient` (wenn vorhanden), `Original-Message-ID`
  und `Disposition: manual-action/MDN-sent-manually; displayed`. Kein `Reporting-UA` (die Mail
  verrät das Programm nicht), keine Kopfzeilen des Originals. An die angeforderte Adresse, von
  der Identität, an die die Mail ging; `In-Reply-To`/`References` auf das Original.
- **Über die Outbox** als `send`-Operation ohne Entwurf (eigene Dateien unter `outgoing/`),
  ohne Kopie in „Gesendet“, ohne Verzögerung. Nach der Annahme: Antwort `sent` und `$MDNSent`
  (RFC 3503) am Original, wo das Konto schreiben darf; andere Programme und Geräte fragen dann
  nicht mehr (`answered_elsewhere`). Ablehnen speichert `declined` und setzt ebenfalls `$MDNSent`.
  Zweimal gedrückt: dieselbe Sendung. Eine fehlgeschlagene Sendung lässt sich erneut versuchen
  (Aktivität) oder neu beantworten.
- **Antworten** (`receipt_answer`) sind Gerätedaten; der Abgleich zwischen Geräten läuft über
  `$MDNSent` auf dem Server.
- **Empfangene Bestätigungen** werden erkannt, wenn ihr Body gespeichert wird (Vorausladen oder
  Öffnen, `BodyTextSource`), und für die gemeldete Message-ID gespeichert (`receipt_received`,
  Cache, je Empfänger die neueste). `getReadReceipt` der gesendeten Mail listet sie;
  `receiptFor` zeigt an der Bestätigung selbst, wofür sie ist. Bestätigungen, deren Body nie
  geladen wird (Vorausladen aus, nie geöffnet), erscheinen erst nach dem Öffnen.
- Beide neuen Tabellen verweisen mit `ON UPDATE CASCADE` auf `account(uuid)`, damit ein im
  Einstellungs-Sync umbenanntes Konto (0022) seine Zeilen mitnimmt.

### Vorlagen mit Empfängern (Aufgabe 7)

- **Speicherung:** drei neue Spalten an `template` (`to_addrs`, `cc_addrs`, `bcc_addrs`, JSON
  `[{"name", "email"}]` wie beim Entwurf), Standard `'[]'`. Bestehende Vorlagen haben damit keine
  Empfänger; es gibt keinen Umbau der Zeilen (Migration 0013, Abschnitt „Templates with recipients“).
- **Prüfen beim Speichern (Kern):** Adressen werden getrimmt, leere fallen weg, Steuerzeichen im Namen
  ebenso; jede Adresse muss gültig sein (`invalid_address`), höchstens 100 Empfänger zusammen
  (`MAX_TEMPLATE_RECIPIENTS`, sonst `invalid_argument`). Eine Adresse steht nur einmal in der Vorlage,
  im ersten Feld (An vor CC vor BCC); verglichen wird der normalisierte Schlüssel (Groß-/Kleinschreibung
  des lokalen Teils, IDNA).
- **Anwenden (`fillTemplate`):** Der Entwurf wird weiterhin nicht vom Kern geändert; die Antwort trägt
  die zusammengeführten Felder `to`, `cc`, `bcc` und `recipientsAdded`, der Composer übernimmt sie.
  Regel: Die Empfänger des Entwurfs bleiben unverändert vorn; Adressen der Vorlage kommen dazu, wenn
  sie in keinem Feld des Entwurfs stehen; insgesamt höchstens `MAX_RECIPIENTS` (500). Begründung: Eine
  Vorlage soll nichts wegnehmen, was der Nutzer schon eingetragen hat, und niemand bekommt die Mail
  doppelt (etwa einmal in An und einmal in BCC). Die Platzhalter füllt der erste An-Empfänger nach dem
  Zusammenführen, damit auch ein leerer Entwurf mit einer Vorlage mit Empfänger „Hallo Bob“ ergibt.
  Der Betreff bleibt wie in Stufe 10: Der Composer übernimmt ihn, wenn sein eigener leer ist.
- **Abgleich:** Der Datensatz der Art `template` trägt `to`, `cc`, `bcc`, jedes Feld nur, wenn es nicht
  leer ist. Datensätze von Vorlagen ohne Empfänger bleiben damit byte-gleich wie in Stufe 10. Die
  Strukturprüfung (`TemplateContent`, `deny_unknown_fields`) prüft Anzahl und Gültigkeit der Adressen.
  Folge: Ein Gerät mit einer Version vor Stufe 16 lehnt eine Vorlage mit Empfängern als unbekannte
  Struktur ab (der Datensatz bleibt im Sync-Zustand, nichts geht verloren); nach dem Update wird sie
  übernommen.

### Benachrichtigungstöne (Aufgabe 9)

- **Einstellung:** `NotificationSettings.sound` (`NotificationSound`): an/aus (Standard an), ein Ton je
  Plattform (`macos`, `windows`, Standard jeweils `Default`), Konten ohne Ton (`silentAccounts`). Teil
  der abgeglichenen Einstellung `notifications.settings`; weil sich die Systemtöne unterscheiden, hat
  jede Plattform ihr eigenes Feld. Alte gespeicherte Einstellungen lesen sich über `serde(default)` mit
  dem Standardton.
- **Feste Liste:** macOS: `Default` und die Warntöne aus `/System/Library/Sounds` (Basso, Blow, Bottle,
  Frog, Funk, Glass, Hero, Morse, Ping, Pop, Purr, Sosumi, Submarine, Tink; auf macOS 26 geprüft).
  Windows: die Toast-Töne `Default`, `IM`, `Mail`, `Reminder`, `SMS`
  (`ms-winsoundevent:Notification.*`). Keine mitgelieferten Tondateien: keine Lizenzfrage, keine
  größere App. Eine unbekannte Auswahl lehnt `setNotificationSettings` mit `invalid_argument` ab; kommt
  sie über den Abgleich (neuere Version), spielt der Kern `Default`.
- **Entscheidung im Kern:** `comfort/notify.rs` setzt an jeder Benachrichtigung (neue Mail,
  fehlgeschlagenes Senden, zurückgekehrtes Snooze) den Ton oder keinen. Ruhezeiten und stummgeschaltete
  Konten verwerfen die Benachrichtigung und damit auch den Ton; ein Konto in `silentAccounts` wird
  angezeigt, aber ohne Ton. Die Shell (`app/src-tauri/src/notifications.rs`) reicht den Namen an das
  Plugin weiter; neuer Befehl `getNotificationSoundChoices` liefert die Liste der laufenden Plattform.
- **Was `tauri-plugin-notification` 2.8.1 kann (geprüft im Quelltext):** Auf dem Desktop gehen Titel,
  Text, Symbol, Ton und Aktionen an `notify-rust` 4.19; `NotificationBuilder::sound(name)` wird zu
  `sound_name`.
  - macOS: Backend `NSUserNotificationCenter` (über `mac-notification-sys` 0.6); der Name wird
    `NSUserNotification.soundName`, also ein Systemton nach Namen. Der Standardton heißt
    `NSUserNotificationDefaultSoundName`; die Shell setzt ihn für `Default`. Ohne Namen ist die
    Benachrichtigung stumm (so war es bis Stufe 15).
  - Windows: `tauri-winrt-notification` 0.8; der Name wird über `Sound::from_str` zu einem Toast-
    `<audio src="ms-winsoundevent:Notification.…">`; gültig sind `Default`, `IM`, `Mail`, `Reminder`,
    `SMS` (und Schleifentöne für Alarme, die wir nicht anbieten). Ohne Namen oder mit einem unbekannten
    setzt das Plugin `<audio silent="true"/>`: Bis Stufe 15 waren Benachrichtigungen unter Windows also
    stumm.
  - Kein Plugin-Befehl spielt einen Ton ohne Benachrichtigung ab; eine Hörprobe in den Einstellungen
    gibt es deshalb nicht. Ob der Nutzer Töne für die App in den Systemeinstellungen abgeschaltet hat,
    entscheidet weiter das System.

## Folgen

- Drei neue Datenarten im Register (`readingList`, `mailboxPref`, `blockedSender`, am Ende der
  Reihenfolge), Erfassungstest erweitert.
- Neue Commands: `set_reading_list`, `list_reading_list`, `list_unread_first`,
  `get_unread_first_views`, `set_unread_first_view`, `set_mailbox_favorite`,
  `set_mailbox_color`, `reorder_favorite_mailboxes`, `list_favorite_mailboxes`, `block_sender`,
  `unblock_sender`, `list_blocked_senders`, `transfer_messages`, `set_draft_send_options`,
  `get_read_receipt`, `send_read_receipt`, `decline_read_receipt`,
  `get_notification_sound_choices`. Neue Felder an `Draft`, `OutgoingMail`, `ComposeSettings`,
  `TemplateInfo`/`TemplateInput`/`FilledTemplateInfo`, `NotificationSettings`, `Mailbox` und
  `MessageListItem`. Keine neuen Fehlercodes; neue Aktionsart `copy`.
- Abgeglichene Einstellungen: `compose.archive_on_reply`, `compose.read_receipts`; die Töne
  liegen in `notifications.settings`. Die Vorlage trägt ihre Empfänger im Datensatz
  (`docs/SYNC.md`).
- `docs/ARCHITECTURE.md`: Datenmodell, Outbox, Einstellungs-Sync.

## Ungeprüft

- Verschieben zwischen zwei echten Konten (der Mensch prüft danach, 0032).
- Server ohne UIDPLUS, auf denen die Suche nach der Message-ID die Kopie nicht findet: dann
  bleibt die Quelle stehen (doppelt statt verloren); nur mit Stalwart getestet.
- Verschieben aus einem Gmail-Konto (Stufe 14, 0034): das `expunge` der Quelle nimmt dort nur
  das Label; die Mail bleibt in „Alle Nachrichten“. Nicht gegen Gmail getestet.
- Lesebestätigungen mit einem anderen Programm (Thunderbird, Outlook) in beide Richtungen; im
  Test nur gegen Stalwart (Mitschnitt der SMTP-Leitung, eine echte MDN an bob) und mit einem
  Thunderbird-ähnlichen Muster geprüft.
- Das Abspielen der Benachrichtigungstöne auf macOS und Windows (Desktop-Test mit dem Menschen);
  die Tests prüfen die Entscheidung im Kern und die Zuordnung der Namen in der Shell.

## Rückweg

Jede Tabelle ist eigenständig; eine Datenart lässt sich aus dem Register nehmen (ihre Datensätze
bleiben dann im Sync-Zustand liegen). „Ungelesene oben“ bekäme bei Bedarf einen Teilindex auf
gelesene Orte per Migration.

## Nachtrag 26.11.2026 (Stufe 22)

Beim Ziehen fragt die App nicht mehr „Verschieben oder Kopieren“: Ablegen verschiebt, mit Wahltaste
bzw. Strg wird kopiert; Zeilen aus mehreren Konten in einem Zug (`dropMessages`). „Verschieben nach
…“ im Menü fragt weiter. Siehe 0081.
