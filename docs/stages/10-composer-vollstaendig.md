# Stufe 10 — Composer vollständig

**Ziel:** Der Composer hat alles, was im Alltag gebraucht wird: Signaturen, Vorlagen, Inline-Bilder,
Kontakte und geplantes Senden.

**Voraussetzung:** Stufe 05 fertig (Stufe 08 empfohlen).

**Aus Stufe 5 vorhanden (0018):** Tabelle `contact` (eine Zeile je Adresse, `contact_group` für
Zusammenführungen noch ungenutzt), Autovervollständigung, Text-Signatur je Identität
(`identity.signature`), Zitat-Position je Entwurf (`quotePosition`), `not_before` an Operationen
(für „Später senden“ nutzbar: eine `send`-Operation mit spätem `not_before`).

## Aufgaben

- [ ] **Formatierung.** Durchgestrichen, Überschriften, Code, Ausrichtung, Rückgängig/Wiederholen. Eingefügter Inhalt aus Word oder Webseiten wird bereinigt.
- [ ] **Inline-Bilder.** Einfügen und Hineinziehen, als `cid:`-Teil in der Mail, Größe änderbar.
- [ ] **Rechtschreibprüfung.** Die des Systems. Verhalten und Sprachwahl in WKWebView und WebView2 prüfen und festhalten.
- [ ] **Signaturen.** Mehrere, als HTML mit Bildern und Links. Standard je Identität, getrennt für neue Mails und Antworten. Im Composer wechselbar.
- [ ] **Vorlagen.** Betreff und Text speichern, mit einfachen Platzhaltern (Vorname des Empfängers). Einfügen über die Command Palette.
- [ ] **Kontakte.** Lokales Verzeichnis aus gesendeten und empfangenen Mails, sortiert nach Häufigkeit und Aktualität; Adressen ohne Antwortmöglichkeit ausgenommen. Namen bearbeiten. Dubletten erkennen (gleiche Adresse, verschiedene Namen; gleicher Name, ähnliche Adresse) und zusammenführen, nur auf Bestätigung.
- [ ] **Später senden.** Lokal. Die Oberfläche sagt bei der Auswahl und in „Ausgang“ klar: Die Mail wird gesendet, wenn die App zu diesem Zeitpunkt läuft und online ist. War die App zum Zeitpunkt aus und ist mehr als eine Stunde vergangen, fragt sie beim nächsten Start: jetzt senden, neu planen oder als Entwurf behalten.
- [ ] **Fenster.** Composer in eigenem Fenster; mehrere Entwürfe gleichzeitig.
- [ ] **Größe.** Warnung, wenn Anhänge die Grenze des Servers überschreiten (SMTP-Erweiterung SIZE).
- [ ] **Antworten und Weiterleiten.** Einstellung für die Position des Zitats. Weiterleiten als Anhang (`.eml`).

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

**Zwischenstand vom 23.11.2026, Stufe in Arbeit.** Kern, Speicher und Shell sind fertig (b313c22,
Entscheidung 0023, baut auf 0016 und 0018 auf); die Oberfläche entsteht gerade in einer eigenen
Sitzung und ist noch nicht committet. Geprüft von der Hauptsitzung in einem sauberen Worktree:
`scripts/check.sh` grün auf 255ce74 (947 Rust-, 909 Vitest-Tests in 41 Dateien); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) auf 255ce74: 1.021 von
1.022 grün, rot nur `ai::tests::qa::qa_set_with_a_real_model`, der ohne lokales Modell absichtlich
scheitert. Der Windows-Prototyp `prototype-2026-11-23` enthält Stufe 10 nicht.

**Nachgewiesen (Kern, in `check.sh` außer wo Stalwart genannt ist)**
- Snapshot des erzeugten HTML mit Signatur, Inline-Bild und Zitat, Textteil lesbar:
  `compose::tests::generated_html_with_signature_inline_image_and_quote`
  (`crates/mail-content/src/compose/snapshots/signature_image_quote_mail.html`).
- Inline-Bild beim zweiten Testkonto:
  `tests/compose.rs::an_inline_image_arrives_and_is_shown_at_the_second_account` (Stalwart: der Teil
  kommt unverändert an, der Lesebereich zeigt ihn als `data:`-Bild und nicht als Anhang), grün im
  Serverlauf auf 255ce74; dazu
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
  Stalwart `tests/smtp.rs::the_size_limit_of_the_server_is_read` (grün im Serverlauf auf 255ce74).
- Weiterleiten als Anhang:
  `compose::tests::a_message_forwarded_as_attachment_goes_in_unencoded_when_it_can`,
  `compose::stage10_tests::a_message_is_forwarded_as_an_attachment`.
- Signaturen und Vorlagen:
  `signature_tests::the_text_signature_of_stage_5_becomes_a_signature_with_the_identity_uuid`,
  `signatures_defaults_and_the_signature_of_a_draft`, `templates_are_stored_captured_and_applied`,
  `compose::snippets::tests::a_draft_sends_the_default_of_its_kind_or_the_chosen_signature_with_its_images`,
  `templates_are_saved_and_filled_for_a_draft`,
  `templates::tests::templates_fill_placeholders_as_text`,
  `values_cannot_inject_html_or_placeholders`; Abgleich „Bearbeiten schlägt Löschen“
  `settings_sync::tests::scenarios::deleted_signature_edited_elsewhere_stays_edited`.
- `mailto:`-Links in Mails öffnen den eigenen Composer mit allen Feldern:
  `compose::stage10_tests::a_compose_request_fills_every_field_and_threads_the_reply`.

**Offen**
- Oberfläche aller Aufgaben: Formatierungsknöpfe (durchgestrichen, Überschriften, Code, Ausrichtung,
  Rückgängig/Wiederholen) und Bereinigung beim Einfügen im Editor, Inline-Bilder einfügen,
  hineinziehen und in der Größe ändern, mehrere HTML-Signaturen mit Standards je Identität und
  Wechsel im Composer, Vorlagen über die Command Palette, Kontaktverzeichnis mit Namen und
  Zusammenführen, „Später senden“ mit dem Hinweis in Auswahl und „Ausgang“ und der Frage beim Start,
  Größenwarnung, Zitat-Position, Weiterleiten als Anhang. In Arbeit im Arbeitsverzeichnis, nicht
  committet und von der Hauptsitzung nicht geprüft; dazu eine noch nicht gelaufene Desktop-Spec
  `app/tests/e2e/send-later.e2e.mjs`.
- Composer in eigenem Fenster, mehrere Entwürfe gleichzeitig: nicht gebaut.
- Rechtschreibprüfung in WKWebView und WebView2: nicht geprüft und nicht festgehalten.
- Nächster Schritt: Oberfläche fertigstellen und committen, Vitest und Desktop-Test dafür, Composer-
  Fenster, dann die Handprüfung der Rechtschreibung.

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

**Ungeprüft**
- Echte Server außer Stalwart (SIZE bei Gmail, Dovecot/Postfix, Exchange); Server, die SIZE erst
  nach STARTTLS nennen, gelten als „keine feste Grenze“. Windows.
- Fremde Entwürfe mit `cid:`-Bildern verlieren beim Öffnen ihre Bilder.

**Wartet auf**
- Mensch: Rechtschreibprüfung auf beiden Systemen prüfen und vermerken; eigene Signaturen anlegen
  und eine Mail an einen anderen Anbieter senden (sobald die Oberfläche da ist): Sieht die Signatur
  dort richtig aus?
