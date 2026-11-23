# Stufe 10 — Composer vollständig

**Ziel:** Der Composer hat alles, was im Alltag gebraucht wird: Signaturen, Vorlagen, Inline-Bilder,
Kontakte und geplantes Senden.

**Voraussetzung:** Stufe 05 fertig (Stufe 08 empfohlen).

**Aus Stufe 5 vorhanden (0018):** Tabelle `contact` (eine Zeile je Adresse, `contact_group` für
Zusammenführungen noch ungenutzt), Autovervollständigung, Text-Signatur je Identität
(`identity.signature`), Zitat-Position je Entwurf (`quotePosition`), `not_before` an Operationen
(für „Später senden“ nutzbar: eine `send`-Operation mit spätem `not_before`).

## Aufgaben

- [x] **Formatierung.** Durchgestrichen, Überschriften, Code, Ausrichtung, Rückgängig/Wiederholen. Eingefügter Inhalt aus Word oder Webseiten wird bereinigt.
- [x] **Inline-Bilder.** Einfügen und Hineinziehen, als `cid:`-Teil in der Mail, Größe änderbar.
- [ ] **Rechtschreibprüfung.** Die des Systems. Verhalten und Sprachwahl in WKWebView und WebView2 prüfen und festhalten.
- [x] **Signaturen.** Mehrere, als HTML mit Bildern und Links. Standard je Identität, getrennt für neue Mails und Antworten. Im Composer wechselbar.
- [x] **Vorlagen.** Betreff und Text speichern, mit einfachen Platzhaltern (Vorname des Empfängers). Einfügen über die Command Palette.
- [x] **Kontakte.** Lokales Verzeichnis aus gesendeten und empfangenen Mails, sortiert nach Häufigkeit und Aktualität; Adressen ohne Antwortmöglichkeit ausgenommen. Namen bearbeiten. Dubletten erkennen (gleiche Adresse, verschiedene Namen; gleicher Name, ähnliche Adresse) und zusammenführen, nur auf Bestätigung.
- [x] **Später senden.** Lokal. Die Oberfläche sagt bei der Auswahl und in „Ausgang“ klar: Die Mail wird gesendet, wenn die App zu diesem Zeitpunkt läuft und online ist. War die App zum Zeitpunkt aus und ist mehr als eine Stunde vergangen, fragt sie beim nächsten Start: jetzt senden, neu planen oder als Entwurf behalten.
- [x] **Fenster.** Composer in eigenem Fenster; mehrere Entwürfe gleichzeitig.
- [x] **Größe.** Warnung, wenn Anhänge die Grenze des Servers überschreiten (SMTP-Erweiterung SIZE).
- [x] **Antworten und Weiterleiten.** Einstellung für die Position des Zitats. Weiterleiten als Anhang (`.eml`).

## Abnahme

- [x] Snapshot-Tests für das erzeugte HTML mit Signatur, Inline-Bild und Zitat; der reine Textteil ist lesbar
- [x] Test: Inline-Bild kommt beim zweiten Testkonto an und wird dort angezeigt
- [x] Tests für Kontakte: Rangfolge, Dubletten-Vorschläge, kein Zusammenführen ohne Bestätigung
- [x] Tests für Später senden mit gestellter Uhr: pünktlich, offline zum Zeitpunkt, App war aus (unter und über einer Stunde)
- [x] Test: Eingefügter Inhalt aus einer Word- und einer Web-Vorlage enthält danach kein Script und keine externen Verweise
- [ ] von Hand geprüft und vermerkt: Rechtschreibprüfung auf beiden Systemen
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Abgleich mit externen Adressbüchern (CardDAV)
- Geplantes Senden über einen Server
- KI-Schreibhilfe (Stufe 13)

## Der Mensch prüft danach

- Eigene Signaturen anlegen und eine Mail an einen anderen Anbieter senden: Sieht die Signatur dort richtig aus?

## Ergebnis

Ausgeführt am 23.11.2026 (Hauptsitzung mit Subagenten): Kern, Speicher und Shell in b313c22
(Entscheidung 0023, baut auf 0016 und 0018 auf), Oberfläche in 14e8dda, Composer im eigenen
Fenster und Standard-Position des Zitats in e5e1519, 04e2a88 und 24ac670 (Entscheidung 0025).
Geprüft von der Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf 24ac670 (998
Rust-, 1.111 Vitest-Tests in 55 Dateien, mit Clippy für das Windows-Ziel, cargo deny und pnpm
audit); alle Servertests (`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart)
grün auf 24ac670 (1.076 Tests); `pnpm --dir app test:e2e` grün auf 24ac670 mit den Testständen aus
ab594b7 (16 Spec-Dateien, 89 Tests grün, 6 übersprungen, darunter `composer-complete.e2e.mjs` mit 4
von 4, `send-later.e2e.mjs` mit 3 von 3 und `windows.e2e.mjs` mit 3 von 3 Tests). Der
Windows-Prototyp `prototype-2026-11-23-2` (gebaut aus 24ac670) enthält Stufe 10.

**Nachgewiesen (Kern, in `check.sh` außer wo Stalwart genannt ist)**
- Snapshot des erzeugten HTML mit Signatur, Inline-Bild und Zitat, Textteil lesbar:
  `compose::tests::generated_html_with_signature_inline_image_and_quote`
  (`crates/mail-content/src/compose/snapshots/signature_image_quote_mail.html`).
- Inline-Bild beim zweiten Testkonto:
  `tests/compose.rs::an_inline_image_arrives_and_is_shown_at_the_second_account` (Stalwart: der Teil
  kommt unverändert an, der Lesebereich zeigt ihn als `data:`-Bild und nicht als Anhang), grün im
  Serverlauf auf 24ac670; dazu
  `compose::stage10_tests::inline_images_are_copied_checked_and_sent_as_related_parts`,
  `compose::tests::inline_images_go_out_as_related_parts_and_resolve`,
  `inline_images_keep_their_size_and_only_cid_sources`.
- Kontakte: Rangfolge
  `compose::contacts::tests::the_directory_ranks_by_frequency_and_recency_without_no_reply_addresses`,
  Dubletten-Vorschläge `duplicates_are_suggested_and_a_dismissed_pair_does_not_come_back`, kein
  Zusammenführen ohne Bestätigung `contacts_are_never_merged_without_confirmation`, Namen
  `names_can_be_edited_and_cleared`; Store `contacts_dir_tests::*`, Logik
  `contacts::tests::duplicates_are_suggested_never_merged`,
  `merging_combines_everything_on_confirmation`.
- Später senden mit gestellter Uhr: pünktlich und nicht vorher
  (`compose::stage10_tests::a_scheduled_send_goes_out_on_time_and_not_before`), offline zum
  Zeitpunkt (`offline_at_the_scheduled_time_it_goes_out_once_online`), App war aus unter und über
  einer Stunde mit den drei Antworten
  (`the_app_off_at_the_scheduled_time_below_an_hour_sends_above_an_hour_asks`), Verschieben und
  Abbrechen bis zum Start (`a_scheduled_send_can_be_moved_or_cancelled_until_it_starts`); Store
  `schedule::tests::the_app_off_below_an_hour_sends_and_above_an_hour_asks`,
  `a_send_tried_while_the_app_ran_offline_is_never_a_missed_one`,
  `scheduled_sends_start_only_when_due_and_can_be_moved_until_then`,
  `a_missed_send_can_be_sent_now_or_moved_and_others_cannot_be_moved`.
- Eingefügtes aus Word und von Webseiten ohne Script und externe Verweise:
  `compose::tests::pasted_word_content_is_clean_and_keeps_its_text`,
  `pasted_web_content_is_clean_and_keeps_its_text` (Proben und Snapshots unter
  `crates/mail-content/src/compose/`), `oversized_paste_is_refused`,
  `compose::stage10_tests::pasted_html_comes_back_clean`.
- Größe (SMTP SIZE): `smtp::tests::the_size_limit_is_read_from_ehlo_and_nothing_is_sent`,
  `send::size_tests::size_extension_from_ehlo_lines`,
  `compose::stage10_tests::the_server_size_limit_warns_and_refuses_too_large_messages`; gegen
  Stalwart `tests/smtp.rs::the_size_limit_of_the_server_is_read` (grün im Serverlauf auf 24ac670).
- Weiterleiten als Anhang:
  `compose::tests::a_message_forwarded_as_attachment_goes_in_unencoded_when_it_can`,
  `compose::stage10_tests::a_message_is_forwarded_as_an_attachment`.
- Position des Zitats:
  `compose::stage10_tests::replies_and_forwards_start_with_the_default_quote_position` (neue
  Antworten und Weiterleitungen übernehmen die gespeicherte Position, neue Mails nicht, der Wert
  übersteht einen Neustart).
- Signaturen und Vorlagen:
  `signature_tests::the_text_signature_of_stage_5_becomes_a_signature_with_the_identity_uuid`,
  `signatures_defaults_and_the_signature_of_a_draft`, `templates_are_stored_captured_and_applied`,
  `compose::snippets::tests::a_draft_sends_the_default_of_its_kind_or_the_chosen_signature_with_its_images`,
  `templates_are_saved_and_filled_for_a_draft`,
  `templates::tests::templates_fill_placeholders_as_text`,
  `values_cannot_inject_html_or_placeholders`; Abgleich „Bearbeiten schlägt Löschen“
  `settings_sync::tests::scenarios::deleted_signature_edited_elsewhere_stays_edited`.
- Composer-Fenster: Shell `tests::message_window_opens_once_with_the_main_windows_permissions`
  (auch ein Composer-Fenster je Entwurf, falsche oder unbekannte Entwürfe abgelehnt, Mail- und
  Composer-Fenster dürfen sich schließen, das Hauptfenster nicht; 04e2a88) und
  `windows::tests::composer_windows_have_one_label_and_page_per_draft`.
- `mailto:`-Links in Mails öffnen den eigenen Composer mit allen Feldern:
  `compose::stage10_tests::a_compose_request_fills_every_field_and_threads_the_reply`.

**Nachgewiesen (Oberfläche, Vitest)**
- Formatierung (`composer-complete.test.tsx`): Rückgängig/Wiederholen, Überschriften,
  durchgestrichen, Code und Ausrichtung; eine zentrierte Überschrift wird als das HTML gespeichert,
  das der Kern behält; eingefügtes HTML geht vor dem Editor durch die Bereinigung des Kerns, reiner
  Text nicht; ein abgelehntes Einfügen kommt als Text und wird gemeldet.
- Inline-Bilder: aus dem Dialog des Kerns als `cid:`, angezeigt aus einer `data:`-URL, Größe über
  ein Menü; ein wieder geöffneter Entwurf zeigt sie; abgelegte Bilder gehen in einen
  Rich-Text-Entwurf, andere Dateien werden angehängt; Fehler an Ort und Stelle.
- Größe: Warnung sofort, wenn die Mail größer ist, als der SMTP-Server annimmt; eine abgelehnte
  Sendung sagt, was zu tun ist.
- Signaturen (`snippets-settings.test.tsx`, `composer-complete.test.tsx`): anlegen mit Hinweis, was
  die Bereinigung geändert hat, umbenennen, Bilder als `cid:` hinzufügen und entfernen (auch per
  Drop), löschen nach Bestätigung, Standard je Adresse für neue Mails und für Antworten; der
  Composer beginnt mit dem Standard der Art und wechselt die Signatur.
- Vorlagen: anlegen mit Platzhaltern (unbekannte werden angemahnt), bearbeiten, löschen nach
  Bestätigung; im Composer aus dem Menü für den ersten Empfänger gefüllt einfügen und über die
  Command Palette in den offenen Composer.
- Kontakte (`contacts.test.tsx`): Verzeichnis in der Rangfolge des Kerns mit Nutzung, Suche,
  nächste Seite, Namen ändern und zurücksetzen, Adresse eines zusammengeführten Kontakts trennen;
  Dubletten mit Grund, Zusammenführen nur nach Bestätigung im Dialog, Ablehnung des Kerns ändert
  nichts, Vorschlag dauerhaft verwerfen; deutsche Texte.
- Später senden (`send-later.test.tsx`): Vorschläge in Ortszeit, Prüfung von Vergangenheit und mehr
  als einem Jahr wie im Kern, der Hinweis „nur wenn die App läuft und online ist“ in der Auswahl
  (auch deutsch) und im Ausgang statt des Rückgängig-Countdowns; im Ausgang verschieben, jetzt
  senden (nach der Rückgängig-Verzögerung) und abbrechen (zurück in den Composer); eine verpasste
  Sendung fragt beim Start (auch vor jedem Ereignis) und während der Sitzung: jetzt senden, neu
  planen oder als Entwurf behalten.
- Weiterleiten als Anhang steht neben „Weiterleiten“; Position des Zitats: Standard in
  Einstellungen → Schreiben (gespeichert zusammen mit der Sendeverzögerung), Vorschau und
  Beschriftung im Composer folgen ihr (`composer.test.tsx`, `windows.test.tsx`).
- Composer im eigenen Fenster (`windows.test.tsx`): ein Entwurf füllt das Fenster, mehrere
  Entwürfe gleichzeitig in eigenen Fenstern, Schließen nach Senden oder Verwerfen, bleibt offen,
  solange Rückgängig möglich ist, Rückgängig bringt den Entwurf ins selbe Fenster; scheitert das
  Fenster, bleibt der Composer mit Grund.
- axe ohne schwere Befunde im Composer der Stufe 10 (Symbolleiste, Inline-Bild, Signatur, Vorlagen,
  Später senden, Größe), in den Kontakten mit offenem Namenseditor und Frage zum Zusammenführen, in
  Einstellungen → Schreiben und im Ausgang mit geplanten und verpassten Sendungen, hell und dunkel
  (`a11y.test.tsx`).

**Nachgewiesen (echte App, Desktop-Tests auf 24ac670)**
- `composer-complete.e2e.mjs`: eine zentrierte Überschrift und durchgestrichener Text werden als
  das HTML gespeichert, das der Kern behält, ohne CSP-Verstoß; eingefügtes HTML mit Script und
  externem Bild wird im Kern bereinigt, der Hinweis auf entfernte Bilder erscheint; eine Vorlage
  wird aus dem Menü des Composers für den Empfänger gefüllt eingefügt; „Als Anhang weiterleiten“
  im Lesebereich öffnet einen Entwurf mit der Mail als `.eml`.
- `send-later.e2e.mjs`: eine geplante Mail wartet im Ausgang mit Zeit und Regel und kommt genau
  einmal pünktlich an; „Jetzt senden“ im Ausgang sendet nach der Rückgängig-Verzögerung genau
  einmal; Abbrechen öffnet sie als Entwurf und sendet nichts.
- `windows.e2e.mjs`: zwei Entwürfe öffnen zwei Composer-Fenster; nach dem Verwerfen schließt sich
  jedes Fenster oder meldet, dass es fertig ist (der Test lässt beides zu).

**Abweichungen und Annahmen**
- „Später senden“ ist eine `send`-Operation mit spätem `not_before`; die Nachricht wird beim Planen
  gebaut und mit der geplanten Zeit datiert. Gesendet wird nur, wenn die App zu diesem Zeitpunkt
  läuft und online ist. Liegt der Zeitpunkt beim Start mehr als eine Stunde zurück, wird nicht
  gesendet, sondern gefragt (`send_missed`: jetzt senden, neu planen, als Entwurf behalten);
  darunter geht sie hinaus. Der Worker sieht höchstens alle 30 Sekunden auf die Wanduhr
  (Ruhezustand).
- SIZE liest der Adapter nach der Anmeldung mit einem zweiten EHLO; ohne bekannte Grenze entscheidet
  der Server (552 wird `send_rejected`).
- Inline-Bilder nur PNG, JPEG, GIF und WebP (nie SVG), höchstens 8 MiB, nur aus dem Systemdialog
  oder aus Drop-Ereignissen des Fensters; Content-ID ohne Programmnamen.
- Signaturen sind ein Pool mit Standard je Identität für neue Mails und für Antworten; Bilder liegen
  als Base64 im Datensatz (höchstens 8, je 64 KiB), damit sie mit dem Einstellungs-Sync wandern. Die
  Text-Signatur aus Stufe 5 wird zu einer Signatur mit der UUID der Identität.
- Abgelehnte Dubletten-Paare bleiben auf dem Gerät und werden nicht abgeglichen.
- `QuotePosition` beschreibt, wo die Antwort steht; Standard ist `above` (Antwort über dem Zitat),
  wie vorher (0025, Punkt 11). Der Standard gilt für neue Antworten und Weiterleitungen und wird
  abgeglichen; der Composer kann ihn je Entwurf ändern.
- Größe und Lage der Composer-Fenster werden nicht gespeichert (0025).
- Rechtschreibprüfung: Der Editor setzt `spellcheck="true"` und nutzt die des Systems; Adress-,
  Such- und Namensfelder schalten sie ab.

**Ungeprüft**
- Rechtschreibprüfung in WKWebView und WebView2 (Verhalten und Sprachwahl): nicht geprüft und nicht
  festgehalten.
- Inline-Bilder in der echten App: Der eingebettete WebDriver kann weder den Dateidialog des Systems
  noch das Ablegen einer echten Datei steuern; geprüft in Vitest, im Kern und gegen Stalwart.
  Signaturen und Kontakte haben keinen Desktop-Test.
- Echte Server außer Stalwart (SIZE bei Gmail, Dovecot/Postfix, Exchange); Server, die SIZE erst
  nach STARTTLS nennen, gelten als „keine feste Grenze“. Windows.
- Fremde Entwürfe mit `cid:`-Bildern verlieren beim Öffnen ihre Bilder.

**Wartet auf**
- Mensch: Rechtschreibprüfung auf beiden Systemen prüfen und vermerken (Mac: die App; Windows:
  Prototyp `prototype-2026-11-23-2`, Gitea-Vorab-Release, gebaut aus 24ac670; die
  Windows-Checkliste hat für Stufe 10 noch keinen Abschnitt, die Schritte stehen im
  Release-Hinweis); eigene Signaturen anlegen und eine Mail an einen anderen Anbieter senden: Sieht
  die Signatur dort richtig aus?
