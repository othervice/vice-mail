# 0023 — Composer vollständig: Später senden, Inline-Bilder, Signaturen, Vorlagen, Kontakte, Größe

- **Datum:** 23.11.2026
- **Stufe:** 10 (Kern, Speicher, Shell; die Oberfläche folgt in eigenen Sitzungen)
- **Status:** entschieden von der ausführenden Sitzung
- **Baut auf:** 0016 (Logik von Kontakten, Signaturen, Vorlagen), 0018 (Entwürfe und Senden),
  0012 (Outbox), 0022 (Einstellungs-Sync)
- **Code:** `crates/mail-store/migrations/0010_composer.sql` (Signaturen, Vorlagen, Kontakte in einer Migration),
  `crates/mail-store/src/{schedule,signatures,contacts_dir}.rs`,
  `crates/mail-model/src/{compose,snippets,contacts}.rs`,
  `crates/mail-content/src/compose/{html,build,paste}.rs`,
  `crates/mail-engine/src/compose/{schedule,inline,stage10,snippets,contacts}.rs`,
  `crates/mail-imap/src/smtp/mod.rs` (SIZE),
  `app/src-tauri/src/commands/{composer,snippets,contacts}.rs`

## Später senden (lokal)

- **Eine `send`-Operation mit spätem `not_before`**, wie in 0018 vorgesehen; neu ist nur das
  Merkmal `SendJob::scheduled` (vom Nutzer gewählte Zeit, im JSON der Nutzdaten, ohne
  Schemaänderung). Der Entwurf wird beim Planen eingefroren wie beim Senden, die Nachricht sofort
  gebaut und **mit der geplanten Zeit datiert** (sonst sortieren Empfänger sie zur Zeit des
  Klicks ein). Verschieben baut die Nachricht für das neue Datum neu, in neue Dateinamen
  (`message-<ms>.eml`), damit ein Versuch, der die alte Datei schon liest, vollständige Bytes
  liest; Abbrechen ist `cancelSend` (zurück zum Entwurf).
- **Gesendet wird nur, wenn die App zu diesem Zeitpunkt läuft und online ist.** Offline zum
  Zeitpunkt: wie jede Sendung (`queued` mit Fehler, Backoff, Wecken beim Verbinden). Kein Senden
  über einen Server (ausdrücklich nicht in dieser Stufe).
- **App war aus.** Beim Start, bevor die Outbox oder ein Konto startet (`check_missed_sends` vor
  `start_stored_accounts`), werden geplante Sendungen ohne Versuch (`attempts = 0`), deren Zeit
  mehr als **eine Stunde** (`MISSED_SEND_GRACE_MS`) zurückliegt, nicht gesendet, sondern
  `failed` mit dem neuen Code `send_missed` (Zustand `missed`); der Entwurf bleibt eingefroren,
  und `sendNeedsDecision` fragt (wie bei unklaren Sendungen, unterschieden über `mail.state`).
  Bis zu einer Stunde gehen sie beim Start einfach hinaus. Eine Sendung mit Versuchen lief, während
  die App lief (offline), und wird nie zur Frage. Eine abgelaufene Rückgängig-Verzögerung (nicht
  `scheduled`) ist keine Planung und geht hinaus wie in Stufe 5.
- **Antworten:** `resolveMissedSend` mit `send_now` (neu gebaut mit dem jetzigen Datum, nach der
  normalen Verzögerung), `reschedule` (neue Zeit), `keep_as_draft` (`cancelSend`). Ein zweites
  Antworten wird abgelehnt (`send_in_progress`).
- **Wettlauf Verschieben/Start:** Die Outbox startet eine Sendung jetzt mit
  `begin_due_operations` und der Uhr des Workers; das `UPDATE` setzt `in_flight` nur, wenn
  `not_before` erreicht ist. Eine inzwischen verschobene Sendung startet so nicht mit den zuvor
  gelesenen Nutzdaten.
- **Ruhezustand:** Tokios Timer folgen der monotonen Uhr, die im Ruhezustand steht. Der Worker
  wartet deshalb höchstens 30 Sekunden am Stück und sieht dann wieder auf die Wanduhr; eine für
  9:00 geplante Mail wartet nach dem Aufwachen nicht die Stunden des geschlossenen Deckels nach.
- **Uhr injizierbar:** Outbox (`Outbox::with_clock`) und Compose nehmen die Uhr aus
  `CoreServices::clock`; die Tests verschieben sie (App aus über/unter einer Stunde).
- Grenzen: Zeit in der Zukunft, höchstens 366 Tage voraus (`invalid_argument`). Über IPC als
  `SendLater { sendAt }` (Millisekunden), weil specta nackte `i64`-Argumente nicht exportiert.

## SMTP-Größe (SIZE, RFC 1870)

- lettre liest SIZE nicht aus. Der Adapter (`SmtpSender::limits`) meldet sich an, schickt
  **EHLO noch einmal** und liest die Zeile `SIZE n` (fehlt sie oder ist `SIZE 0`: keine feste
  Grenze), dann QUIT; gesendet wird nichts, auch für Nur-Lese-Konten erlaubt. Neue Methode
  `MailSender::limits` mit Standard-Umsetzung „keine Grenze“.
- Der Kern fragt je Konto einmal pro Sitzung (6 Stunden gültig, 15 Sekunden Frist) und nur auf
  Anfrage (`checkDraftSize`); offline heißt `limitKnown: false`, kein Fehler. Die Größe ist die
  der tatsächlich gebauten Nachricht (Base64 eingerechnet).
- `sendDraft`/`scheduleSend` lehnen eine Nachricht über einer **bekannten** Grenze mit dem neuen
  Code `message_too_large` ab, bevor etwas vermerkt wird. Ohne bekannte Grenze entscheidet der
  Server (552 → `send_rejected` wie bisher). Bewusst kein Netzwartezustand im Sendeknopf.

## Inline-Bilder

- Ein Inline-Bild ist eine Datei des Entwurfs mit `content_id` (`DraftFile`, JSON der Spalte
  `draft.attachments`, keine Schemaänderung). Hinzu kommen sie nur aus Pfaden des Systemdialogs
  (`pickInlineImages`, rfd mit Bildfilter) oder eines Drop-Ereignisses des Fensters
  (`addDroppedInlineImages`, dieselbe Prüfung wie für Anhänge, 0018); die Oberfläche liest nie
  Dateien. Der Kern prüft die Bytes (PNG, JPEG, GIF, WebP; nie SVG), höchstens 8 MiB je Bild.
- Content-ID `<32 Hex>@<Domain des Absenders>` (ASCII, sonst `inline`), verrät kein Programm.
- **Allowlist:** `<img>` gibt es im Mail-HTML nur mit `src="cid:…"` (strenge Content-ID-Syntax,
  ohne Escaping in URL und Kopfzeile), `alt`, `width`/`height` (1–4096 px, auch aus
  `style="width:…px"`), in Mail-HTML zusätzlich `max-width:100%`. Nur **Editor-Inhalt** (Composer,
  Signaturen) behält solche Bilder; in HTML anderer Herkunft (zitierte Mail, fremder Entwurf,
  Eingefügtes) zeigt ein `cid:` in eine fremde Nachricht und wird entfernt.
- **Aufbau:** `multipart/related; type="multipart/alternative"` um Text und HTML, darum bei
  Anhängen `multipart/mixed`. Es gehen nur Bilder hinaus, die das HTML noch zeigt (gelöschtes
  `<img>` genügt), jedes einmal, mit `Content-Disposition: inline` und Dateinamen.
- Anzeige im Editor über `inlineImageData` (`data:`-URL); gespeichert wird immer `cid:`.

## Signaturen (Signatur-Sitzung, hier zusammengefasst)

- **Ein Pool** für alle Identitäten (Tabelle `signature`: Name, Editor-HTML, Text, Bilder),
  je Identität ein Standard für neue Mails (`signature_new`) und einer für Antworten und
  Weiterleitungen (`signature_reply`); der Entwurf merkt sich eine gewählte Signatur
  (`draft.signature_uuid`, NULL = Standard der Entwurfsart). „Mehrere je Identität“ heißt: jede
  Signatur ist im Composer wechselbar.
- **Migration der Text-Signatur aus Stufe 5:** je Identität mit Text eine Signatur mit der
  **UUID der Identität**, damit der Sync-Datensatz `signature/<Identitäts-UUID>` seine Id behält;
  beide Standards zeigen darauf. HTML bleibt NULL (aus dem Text abgeleitet). `setIdentitySignature`
  arbeitet weiter auf dieser Signatur.
- **Bilder** stehen als Base64 im Datensatz (höchstens 8, je 64 KiB, zusammen 128 KiB), damit sie
  mit dem Einstellungs-Sync wandern; die Höchstgröße des Sync-Datensatzes `signature` steigt auf
  256 KiB. Sie gehen wie Inline-Bilder als `cid:`-Teile hinaus.
- **Sync:** Inhalt `signature` = `{name?, text?, html?, images?}` (ein Datensatz `{text}` aus
  Stufe 11 bleibt lesbar), `template` = `{name, subject, html}` (strikt statt „beliebiges Objekt“), Identität mit optionalem `signatureNew`/
  `signatureReply` (fehlt, wenn leer: alte Datensätze bleiben gleich). Die Identitäts-Trigger aus
  0009 werden in 0010 ersetzt (ohne die Art `signature`). Eine gelöschte Signatur, die noch
  Standard ist, gilt als „keine“; die Standards werden nicht geleert, damit eine gleichzeitige
  Bearbeitung auf einem anderen Gerät (Bearbeiten schlägt Löschen) ihren Standard behält.

## Vorlagen

- Tabelle `template` (Name, Betreff, Editor-HTML), CRUD über Commands, Bereinigung wie Signaturen
  (`mail_content::Template`). `fillTemplate` füllt Platzhalter mit dem ersten To-Empfänger und der
  Identität des Entwurfs und ändert den Entwurf nicht; Bilder in Vorlagen werden abgelehnt.

## Kontakte (Kontakte-Sitzung, hier zusammengefasst)

- **Rangfolge** wie 0016 (Zerfall, gesendet 1, empfangen 0,25); No-Reply-Adressen erscheinen weder
  im Verzeichnis noch in der Autovervollständigung, auch nicht aus Zeilen vor dieser Stufe.
- **Zusammengeführte Kontakte** teilen `contact_group` (Id der behaltenen Zeile); im Verzeichnis
  und in der Autovervollständigung erscheinen sie einmal mit der behaltenen Adresse. Eingereiht
  werden sie nach ihrer bestplatzierten Adresse (nicht nach dem vereinten Rang), damit das
  Keyset-Blättern auf dem Index `contact_rank` bleibt; der vereinte Rang gilt nur in der
  Dubletten-Logik.
- **Zusammenführen nur mit `confirmed: true`**, sonst `confirmation_required` ohne Änderung;
  Trennen möglich. Der Hintergrund-Auftrag überschreibt `user_name` und `contact_group` nie.
- **Sync:** `contact` = `{userName?, mergedInto?}` (Adress-Schlüssel des behaltenen Kontakts).
  Ein Datensatz für eine hier unbekannte Adresse wartet in `sync_record`, bis sie auftaucht.
  **Abgelehnte Dubletten-Paare bleiben auf dem Gerät** (`contact_dismissed`, Klasse Gerät).

## Weiterleiten als Anhang, Einfügen, Formatierung

- `NewDraft::ForwardAsAttachment`: Entwurf ohne Zitat mit der ganzen Originalnachricht als
  `<Betreff>.eml` (`message/rfc822`); `$Forwarded` wie bei jeder Weiterleitung. 7-Bit-saubere
  Originale gehen **unkodiert** (`7bit`, RFC 2046 5.2.1), 8-Bit oder lange Zeilen als Base64
  (gegen 7-Bit-Relays; Leser kommen damit zurecht). Eine nur teilweise gespeicherte große Mail
  wird abgelehnt (`invalid_argument`, „inline weiterleiten“).
- **Einfügen:** `cleanPastedHtml` (`mail_content::clean_pasted_html`) läuft durch dieselbe
  Allowlist wie Mail-HTML im Modus „fremde Herkunft“: kein Script, kein Stil, kein Bild, keine
  externe Referenz, Links nur http/https/mailto; Word-Aufzählungszeichen (`mso-list:Ignore`)
  fallen weg; höchstens 2 MiB. Die Oberfläche ruft es im Paste-Handler.
- **Formatierung:** Editor-Inhalt behält jetzt Überschriften `h1`–`h3` und die Ausrichtung
  (`text-align` center/right/justify an `p`, `h1`–`h3`); `h4`–`h6` werden fette Absätze.
  Durchgestrichen und Code gab es schon. HTML fremder Herkunft bleibt unverändert vereinfacht
  (Snapshot `editor_hostile` zeigt jetzt die Überschrift des Editors als `h1`).

## mailto-Links (Nachtrag nach der Oberfläche von Stufe 9)

- `NewDraft::Compose { accountId, identityId, request }` legt einen Entwurf mit allen Feldern
  einer `ComposeRequest` an (An, Cc, Bcc, Betreff, Text als Editor-HTML, `In-Reply-To` samt
  `References`); vorher konnte die Oberfläche nur An setzen und `In-Reply-To` gar nicht. Die
  Hinweise der Anfrage zeigt die Oberfläche, gespeichert werden sie nicht. Art bleibt `new`
  (kein `\Answered` am Original: das Original ist über einen fremden Link nicht sicher bestimmt).
- `openLink` gibt `LinkOpened` zurück: `http`/`https` öffnet weiter das System, ein `mailto:`
  einer Mail geht nicht mehr an das System, sondern kommt als `compose` mit der geprüften Anfrage
  zurück (derselbe Parser wie für `composeRequested`). So landet er im eigenen Composer, auch
  wenn die App nicht das Standard-Mailprogramm ist. Die Link-Prüfungen bleiben gleich.
- Tests: `compose::stage10_tests::a_compose_request_fills_every_field_and_threads_the_reply`,
  `reading::tests::links_are_checked_and_opened_only_through_the_core` (`mailto` → `compose`, der System-Öffner sieht nur `https`).

## Neue Fehlercodes, Ereignisse

- `send_missed`, `message_too_large`. Ereignisse `signaturesChanged`, `templatesChanged`;
  `sendNeedsDecision` auch für verpasste Sendungen.

## Nachweise (23.11.2026, Testserver Stalwart 1.1.1)

| Abnahmepunkt | Test |
|---|---|
| Später senden: pünktlich (nicht vorher, Datum = geplante Zeit) | `compose::stage10_tests::a_scheduled_send_goes_out_on_time_and_not_before` |
| offline zum Zeitpunkt | `compose::stage10_tests::offline_at_the_scheduled_time_it_goes_out_once_online`; Store: `schedule::tests::a_send_tried_while_the_app_ran_offline_is_never_a_missed_one` |
| App war aus, unter und über einer Stunde (gestellte Uhr), drei Antworten | `compose::stage10_tests::the_app_off_at_the_scheduled_time_below_an_hour_sends_above_an_hour_asks`; Store: `schedule::tests::the_app_off_below_an_hour_sends_and_above_an_hour_asks` |
| Verschieben, Abbrechen, Start nur wenn fällig | `compose::stage10_tests::a_scheduled_send_can_be_moved_or_cancelled_until_it_starts`, `schedule::tests::scheduled_sends_start_only_when_due_and_can_be_moved_until_then`, `…a_missed_send_can_be_sent_now_or_moved_and_others_cannot_be_moved`, `…open_sends_read_the_send_index` |
| Inline-Bild kommt beim zweiten Konto an und wird angezeigt | `tests/compose.rs::an_inline_image_arrives_and_is_shown_at_the_second_account` (Stalwart: Teil unverändert, `data:` im Lesebereich, kein Anhang) |
| Snapshot HTML mit Signatur, Inline-Bild, Zitat; Textteil lesbar | `compose::tests::generated_html_with_signature_inline_image_and_quote` (`snapshots/signature_image_quote_mail.html`) |
| Eingefügtes aus Word und Web ohne Script und externe Verweise | `compose::tests::pasted_word_content_is_clean_and_keeps_its_text`, `…pasted_web_content_is_clean_and_keeps_its_text` (Proben `samples/paste_{word,web}.html`, Snapshots) |
| SIZE | `mail-imap smtp::tests::the_size_limit_is_read_from_ehlo_and_nothing_is_sent` (Skript-Server), `tests/smtp.rs::the_size_limit_of_the_server_is_read` (Stalwart: 104857600 über STARTTLS und TLS), `mail-model size_tests::size_extension_from_ehlo_lines`, `compose::stage10_tests::the_server_size_limit_warns_and_refuses_too_large_messages` |
| Weiterleiten als Anhang | `compose::tests::a_message_forwarded_as_attachment_goes_in_unencoded_when_it_can`, `compose::stage10_tests::a_message_is_forwarded_as_an_attachment` |
| Kontakte: Rangfolge, Dubletten, kein Zusammenführen ohne Bestätigung | `compose::contacts::tests::the_directory_ranks_by_frequency_and_recency_without_no_reply_addresses`, `…duplicates_are_suggested_and_a_dismissed_pair_does_not_come_back`, `…contacts_are_never_merged_without_confirmation`, Store `contacts_dir_tests::*` |
| Signaturen, Vorlagen, Migration, Sync | Store `signature_tests::*`, `compose::snippets::tests::*`, `settings_sync::tests::capture::*`, Stalwart `tests/settings_sync.rs` (15 von 15, auch `a_deleted_signature_edited_elsewhere_keeps_the_edit`) |

Dazu `cargo test -p mail-engine --test compose -- --include-ignored --test-threads=1` 9 von 9
und `cargo test -p mail-imap -- --include-ignored --test-threads=1` grün.

## Ungeprüft und bekannte Grenzen

- Oberfläche (Composer, Ausgang, Rückfragen, Editor-Darstellung der `cid:`-Bilder), Windows.
- Echte Server außer Stalwart (SIZE von Gmail, Dovecot/Postfix, Exchange ungeprüft); Server,
  die SIZE erst nach STARTTLS oder gar nicht nennen, gelten als „keine feste Grenze“.
- Datum einer wegen Offline später gesendeten geplanten Mail bleibt die geplante Zeit.
- Dubletten-Ablehnungen synchronisieren nicht; die Suche in einem zusammengeführten Kontakt trifft
  den vergebenen Namen nur über die behaltene Adresse.
- Fremde Entwürfe mit `cid:`-Bildern verlieren beim Öffnen ihre Bilder (wie bisher alle Bilder).
