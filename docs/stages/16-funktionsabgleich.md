# Stufe 16 — Funktionsabgleich

**Ziel:** Vice Mail hat die Funktionen, die Mailbird bietet und die zu den Grundsätzen passen
(Entscheidung 0032). Jede Aufgabe ist einzeln umsetzbar.

**Voraussetzung:** Stufen 04, 05, 07, 09 und 10 fertig; für Farbthemen der Feinschliff aus 0031.

## Aufgaben

- [x] **Lese-Liste.** „Zum Lesen“ als lokale Markierung neben „Aufgaben“ und „Später“, mit eigener Ansicht, Aktion und Kürzel; abgeglichen über den Einstellungs-Sync wie Aufgaben. Ändert nichts auf dem Server.
- [x] **Ungelesen.** Schalter „Nur ungelesene“ im Listenkopf; Einstellung „Ungelesene oben gruppieren“ je Ansicht.
- [x] **Lieblingsordner und Ordnerfarben.** Ordner als Favorit oben in der Seitenleiste und in der Palette; Farbe je Ordner (aus der Farbpalette der Kontofarben, nie die Farbe allein, immer mit Namen). Abgeglichen.
- [x] **Zwischen Konten verschieben und kopieren.** Über die Outbox: APPEND im Zielkonto, prüfen (Message-ID), erst dann im Quellkonto entfernen; beide Konten brauchen Schreibzugriff. Fehlerfälle: Ziel abgelehnt, Verbindung nach APPEND gekappt (nie doppelt, nie verloren). Ziehen auf einen Ordner eines anderen Kontos fragt nach.
- [x] **Senden und archivieren.** Beim Antworten die Ursprungsmail nach dem bestätigten Senden archivieren; als Einstellung und als eigene Taste.
- [x] **Inline-Schnellantwort.** Antwortfeld unter der geöffneten Mail (reiner Text mit Zitat-Einstellung), wird beim Ausklappen zum vollen Composer; dieselben Entwürfe und dieselbe Outbox.
- [x] **In der Mail suchen.** Strg/Cmd+F im Lesebereich sucht im angezeigten Text (auch im Mail-Frame, ohne Skripte im Frame), mit Treffern und Weiter/Zurück.
- [x] **Anhänge.** Vorschau für Bilder, PDF und Text in einem gesicherten Fenster ohne Netz; „Alle speichern“ in einen Ordner; Anhänge anheften (eigene Ansicht „Angeheftete Anhänge“).
- [x] **Absender blockieren.** Lokale Regel „Absender blockieren“: künftige Mails gehen in den Spam-Ordner (bzw. werden ausgeblendet, wenn das Konto nur lesend ist); Liste der blockierten Absender mit Aufheben; abgeglichen.
- [x] **Benachrichtigungstöne.** Ton an/aus, Auswahl aus den Systemtönen bzw. mitgelieferten Tönen (Lizenz prüfen), je Konto stumm; Ruhezeiten gelten.
- [x] **Farbthemen.** Mehrere Akzentfarben (Vorschlag: sechs), hell und dunkel, alle mit Kontrast nach WCAG AA; als Oberflächeneinstellung, abgeglichen.
- [x] **Vorlagen mit Empfängern.** Vorlagen können An/CC/BCC und Betreff vorbelegen.
- [x] **Lesebestätigung (MDN, RFC 8098).** Beim Senden je Nachricht anfordern; eingehende Anforderungen nur nach Rückfrage beantworten (Einstellung: immer fragen, nie senden); empfangene Bestätigungen an der gesendeten Mail anzeigen. Kein Zählpixel, kein Server.

## Abnahme

- [x] Tests je Aufgabe (Kern und Oberfläche); für „Zwischen Konten verschieben“ Servertests mit Fehlerinjektion: genau eine Kopie im Ziel, die Quelle wird erst nach Bestätigung entfernt
- [x] Test: Eine eingehende Lesebestätigungs-Anforderung führt ohne Zustimmung nie zu einer gesendeten Mail (Mitschnitt SMTP)
- [x] Test: Lese-Liste, Favoriten, Ordnerfarben, blockierte Absender und Farbthema gleichen zwischen zwei Geräten ab
- [x] axe ohne schwere Befunde für alle neuen Ansichten, hell und dunkel, in jedem Farbthema
- [x] Desktop-Tests für Schnellantwort, Suchen in der Mail und Anhangsvorschau
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Zählpixel-Tracking, eingebettete Fremd-Apps (0032)
- Kalender, CalDAV, CardDAV (nach Version 1)
- Kontaktbilder aus dem Netz, LinkedIn-Suche, mobile Apps

## Der Mensch prüft danach

- Zwischen zwei echten Konten verschieben (Nebenkonten); Farbthemen ansehen; eine Lesebestätigung mit einem anderen Programm austauschen.

## Ergebnis

**Stand vom 23.11.2026, 13:30.** Ausgeführt am 23.11.2026 (Hauptsitzung mit Subagenten): Kern in
e0db960 (Entscheidung 0033, Migration `0013_feature_parity.sql`), Oberfläche in cbe8336, Anhänge,
Farbthema im Kern und Zahl der ausgeblendeten Mails in dfa77d9 (0036, Migration
`0015_attachment_pins.sql`), Orange als Standardakzent (0038, 92a2d4c, umgesetzt in dfa77d9),
Gestaltung in 5b0c53b, gemeinsame Sperre der Outbox-Servertests in 011629f. Entscheidungen: 0032,
0033, 0036, 0038. Geprüft von der Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf
dfa77d9 (1.102 Rust-, 3.860 Vitest-Tests in 75 Dateien, mit Clippy für das Windows-Ziel, cargo deny
und pnpm audit); alle Servertests (`cargo test --workspace -- --include-ignored` gegen den lokalen
Stalwart) auf dfa77d9: 1.185 von 1.186 grün. Der eine Fehler,
`outbox::server_tests::every_action_online_and_offline` (`MailboxNotFound("VmTransfer-tr32210-cut")`),
war ein Wettlauf: Er listete bobs Ordner, während ein Servertest der Verschiebung zwischen Konten
seinen Ordner entfernte. Seit 011629f teilen sich beide Dateien eine Sperre; danach lief der
gezielte Lauf gegen Stalwart grün (21 von 21 Tests der Bibliothek `mail-engine`). Desktop-Lauf auf
dem neuesten Stand: grün auf 5b0c53b mit den Testständen aus 4928d71 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`). `prototype-2026-11-23-4` (4928d71, Windows und macOS) enthält die ganze Stufe;
`prototype-2026-11-23-3` (7ce21bd) und `-3.1` (8b357c2) hatten nur den Kern aus e0db960.

**Nachgewiesen (Kern und Speicher in `check.sh`, wo nicht Stalwart genannt ist)**
- Lese-Liste: `stage16_tests::reading_list_marks_rows_lists_them_and_is_captured` (Speicher),
  `comfort::tests::parity::the_reading_list_is_local_and_listed_newest_added_first` (auch mit
  Schreibzugriff kein Schlagwort und kein Schreibversuch auf dem Server); Oberfläche
  `organize.test.tsx`: `Q` merkt die Mail unter dem Cursor vor, eigene Ansicht in der Leiste mit
  leerem Zustand, Cmd+9.
- Ungelesen: `unread_only_and_unread_first_page_through_every_message_once`,
  `every_stage_16_query_uses_its_index` (Abfragepläne), `only_unread_and_unread_first_through_the_core`;
  Oberfläche: Schalter „Nur ungelesene“ im Kopf, die geöffnete Mail bleibt stehen, bis die Auswahl
  weitergeht, leerer Zustand, „Ungelesene oben“ aus dem Kopfmenü und unter Einstellungen →
  Posteingang.
- Lieblingsordner und Ordnerfarben: `favourites_and_colours_show_in_the_folder_list_and_go_away_when_cleared`,
  `favourites_and_colours_come_with_the_folder_list`; Oberfläche: Favorit aus dem Ordnermenü oben in
  der Seitenleiste und in der Palette, Reihenfolge mit Alt+Pfeil und Menü, Farbe aus der Palette der
  Kontofarben neben dem Namen und mit ihm benannt.
- Zwischen Konten verschieben und kopieren, Servertests mit Fehlerinjektion gegen Stalwart
  (`outbox/transfer_server_tests.rs`, grün auf dfa77d9):
  `a_move_between_accounts_removes_the_source_only_after_the_copy_is_confirmed` (Kopieren: Original
  bleibt, eine Kopie im Ziel; Verschieben: erst angehängt, dann entfernt),
  `a_connection_cut_after_append_leaves_exactly_one_copy_and_the_source_until_confirmed` (der Proxy
  kappt die Antwort auf APPEND: genau eine Kopie im Ziel, die Quelle unberührt; nach dem Neustart
  wird die Kopie über die Message-ID gefunden statt erneut angehängt, erst dann wird die Quelle
  entfernt), `a_target_that_refuses_fails_the_transfer_and_the_source_stays` (Zielordner auf dem
  Server verschwunden: fehlgeschlagen, die Mail bleibt in der Quelle); über den Kern
  `tests/transfer.rs::a_move_to_another_account_through_the_core_appends_then_removes_the_source`
  (Stalwart); Speicher `a_transfer_is_checked_recorded_in_the_target_and_removes_the_source_after_the_copy`;
  Kern `a_transfer_is_refused_before_anything_is_fetched_or_recorded`. Oberfläche: „Verschieben
  nach …“ bietet die Ordner des anderen Kontos und fragt vor dem Kopieren, das Zielkonto braucht
  freigeschaltete Änderungen (der Hinweis sagt, wo), Ziehen auf einen Ordner eines anderen Kontos
  fragt.
- Senden und archivieren: `compose::tests::with_core::send_and_archive_moves_the_original_only_once_the_send_is_accepted`,
  `send_and_archive_without_an_archive_mailbox_keeps_the_original`; Oberfläche
  `send-archive-templates.test.tsx`: Menü neben „Senden“, Cmd+Shift+Enter, Cmd+Enter allein ändert
  nichts an den Optionen, neue Mails haben nichts zu archivieren, Antworten übernehmen die
  Einstellung, leiser Hinweis ohne Archiv-Ordner.
- Inline-Schnellantwort (`quick-reply.test.tsx`): kein Entwurf vor dem ersten Tastendruck, danach
  genau einer als reiner Text; Cmd+Enter sendet über die Outbox mit der üblichen Verzögerung,
  „Rückgängig“ öffnet denselben Entwurf; „Ausklappen“ öffnet den vollen Composer mit demselben
  Entwurf; Escape verlässt das Feld; Zitat abschaltbar; nur lesendes Konto erklärt; Verwerfen;
  abgelehnter Start fragt den Kern nicht bei jeder Taste neu. In der echten App
  (`quick-reply.e2e.mjs`) 4 von 4 grün im Desktop-Lauf auf dfa77d9 (kein Entwurf vor der ersten
  Taste, Escape, Senden mit Countdown und genau einer Kopie in „Gesendet“, Ausklappen). Neuester
  Stand: grün auf 5b0c53b mit den Testständen aus 4928d71 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`).
- In der Mail suchen (`find-in-message.test.tsx`): Treffer über Inline-Elemente hinweg, ohne
  Groß/Klein, nie über Absätze; Verborgenes und die Bedienelemente der App zählen nicht; das
  Dokument des Mail-Frames ist ein eigener Abschnitt; Hervorheben über die Highlight-Registry des
  Frames und eine Style-Angabe, nie ein Skript, ohne Highlight-API über die Auswahl im Frame;
  Cmd+F im Lesebereich öffnet die Leiste, zählt, springt, Escape schließt; außerhalb des
  Lesebereichs bleibt Cmd+F die Mailsuche; Deutsch; axe und Fokusring. Desktop-Test
  `find-in-message.e2e.mjs`: auf dfa77d9 rot im `before all` (das Testwerkzeug `testmail.py`
  scheiterte beim Anlegen des Ordners, bevor es die zwei Testmails anhängte), kein Fall lief.
  Neuester Stand: grün auf 5b0c53b mit den Testständen aus 4928d71 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`).
- Anhänge (0036): Vorschau im Kern (`mail-content`, `attachment_preview::tests::`):
  `kinds_come_from_type_and_name_and_never_cover_active_documents`,
  `images_that_are_not_what_they_claim_are_refused`, `images_are_decoded_and_encoded_again_by_the_core`,
  `large_images_are_scaled_and_huge_ones_refused`,
  `pdf_pages_are_rasterised_one_at_a_time_without_running_scripts`, `text_is_decoded_cleaned_and_cut`;
  `reading::tests::attachments_are_previewed_by_the_core_and_html_or_svg_never`,
  `save_all_writes_every_attachment_and_never_replaces_a_file`, `attachments_are_pinned_listed_and_unpinned`;
  Speicher `attachment_pins_are_listed_captured_and_applied_from_the_sync`,
  `attachment_pins::tests::ids_split_at_the_first_and_the_last_slash`; Sync
  `settings_sync::record::tests::attachment_pins_check_their_ids_and_content`. Oberfläche
  (`attachments-stage16.test.tsx`): Vorschau eines vom Kern kodierten Bilds, Escape schließt und
  gibt den Fokus zurück, Öffnen auch per Klick oder Leertaste auf der Karte, keine Vorschau, wo der
  Kern keine hat, PDF seitenweise, Text als Text, typisierte Fehler mit Speichern und Öffnen, nur
  PNG- oder JPEG-`data:`-Bilder; „Alle speichern“ ab zwei Anhängen, mit Zahl und umbenannten
  Dateien, auch für `.eml`-Dateien; Anheften, Ansicht „Angeheftete Anhänge“, Änderung eines anderen
  Geräts, Palette. Desktop-Test `attachment-preview.e2e.mjs`: im Lauf auf dfa77d9 nicht enthalten
  (stand nicht in `specs`). Neuester Stand: grün auf 5b0c53b mit den Testständen aus 4928d71 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`).
- Absender blockieren: `blocked_senders_hide_mail_on_read_only_accounts_until_unblocked` (Speicher),
  `blocked_senders_are_hidden_on_read_only_accounts_and_not_announced`,
  `blocked_senders_go_to_junk_where_the_account_may_write`; Oberfläche: `blocked-senders.test.tsx`
  (Liste mit Datum, Aufheben, leerer Zustand, Fehler mit Wiederholen, Änderung eines anderen
  Geräts, Deutsch), `organize.test.tsx` (aus dem Menü der Symbolleiste mit Rückgängig, Ablehnung
  erklärt), `attachments-stage16.test.tsx` (Hinweis auf ausgeblendete Mails im Posteingang und im
  vereinten Posteingang).
- Benachrichtigungstöne: `comfort::notify::tests::notifications_carry_the_sound_of_their_account`,
  `mail-model` `inbox::tests::the_sound_depends_on_switch_account_and_platform` und
  `notification_settings_of_stage_9_get_the_default_sound`, Shell
  `notifications::tests::sounds_map_to_the_plugin_names_of_the_platform`; Oberfläche
  (`themes-sounds.test.tsx`): Systemtöne von macOS bzw. Toast-Töne von Windows nur für die eigene
  Plattform, keine Auswahl ohne Liste, Ton aus, einzelne Konten ohne Ton, Deutsch.
- Farbthemen: sechs Themen (Petrol, Blau, Violett, Rosé, Orange, Grün), Standard Orange (0038);
  `contrast.test.ts` prüft alle Farbpaare in jedem Thema hell und dunkel (4,5:1 für Text, 3:1 für
  Bedienelemente und Fokusring); Kern `ui_prefs::tests::the_accent_theme_is_stored_validated_and_synced`
  (gespeichert, unbekannte Werte an der Brücke abgelehnt, Wert eines neueren Geräts gilt als
  Standard); als Datensatz erfasst in
  `settings_sync::tests::capture::every_registered_kind_is_captured_when_it_changes` (Grün);
  Oberfläche: sechs Themen mit Namen, Orange als Standard markiert, Pfeiltasten und Leertaste,
  folgt `uiPreferencesChanged`, setzt bei Ablehnung zurück, einmalige Übernahme aus dem
  Web-Storage.
- Vorlagen mit Empfängern: `compose::snippets::tests::template_recipients_are_checked_and_prefill_the_draft`,
  `merging_recipients_keeps_the_draft_and_the_limit`, Speicher
  `signature_tests::template_recipients_are_stored_captured_and_applied`,
  `templates_of_stage_10_migrate_to_no_recipients`, Sync
  `settings_sync::record::tests::template_records_carry_checked_recipients`; Oberfläche: An, Cc und
  Bcc im Editor, abgelehnte Adresse markiert, beim Einfügen kommen die Empfänger nach denen des
  Entwurfs, ohne Doppelte.
- Lesebestätigung: Aufbau `mail-content` `compose::mdn::tests::a_built_receipt_is_a_disposition_report_and_parses_back`;
  Kern `compose::tests::with_core::a_read_receipt_request_never_sends_anything_without_the_users_answer`,
  `declined_requests_send_nothing_and_received_receipts_show_at_the_sent_message`. Abnahme mit
  Mitschnitt der SMTP-Leitung gegen Stalwart:
  `tests/compose.rs::a_read_receipt_goes_out_only_on_the_users_answer_smtp_capture` (bob fordert
  an, alice empfängt und öffnet die Mail zweimal, auf der Leitung kein weiteres `MAIL FROM`; erst
  `sendReadReceipt` sendet, die Bestätigung kommt bei bob an und steht an seiner gesendeten Mail).
  Oberfläche (`read-receipts.test.tsx`): leise Frage, ohne Klick nichts gesendet, „Bestätigung
  senden“ einmal und in der Sprache der App, „Nicht senden“, Warnung bei abweichendem Return-Path,
  nichts angeboten bei „Nie senden“ oder einer Antwort von einem anderen Gerät, empfangene
  Bestätigungen an der gesendeten Mail.
- Zwei Geräte gegen Stalwart:
  `tests/settings_sync.rs::reading_list_favourites_colours_and_blocked_senders_reach_the_other_device`
  (Lese-Liste, Favorit, Ordnerfarbe, blockierter Absender und angehefteter Anhang kommen auf dem
  zweiten Gerät an, ihr Entfernen ebenso; grün auf dfa77d9).
- axe ohne schwere Befunde, hell und dunkel, in Vitest: Lese-Liste, Kopf mit „Nur ungelesene“,
  Favoriten, Farben und Rückfrage beim Verschieben (`organize.test.tsx`); Lesebereich mit Frage nach
  einer Lesebestätigung und offener Schnellantwort, empfangene Bestätigungen, Sendeoptionen und
  Sendemenü, Vorlagen mit Empfängern (`stage16-writing-a11y.test.tsx`, dazu Fokusring);
  Suchleiste; Anhänge mit Anheften, „Alle speichern“ und Vorschau-Dialog, Ansicht „Angeheftete
  Anhänge“, Hinweis auf ausgeblendete Mails; blockierte Absender; Benachrichtigungstöne; der
  Abschnitt Darstellung in jedem der sechs Farbthemen.

**Abweichungen und Annahmen**
- Die Anhangsvorschau ist ein Dialog über der App, kein eigenes Fenster (0036): Die Sicherheit liegt
  in den Daten. Bilder dekodiert und kodiert der Kern neu (nie SVG), PDF-Seiten rastert hayro in
  Rust (keine Skripte, keine Formulare, kein Netz), Text erscheint als Text; HTML, XML und SVG
  bekommen nie eine Vorschau. Keine neue Capability, keine Änderung an CSP oder Fenstern.
- Töne nur aus dem System (macOS-Warntöne, Windows-Toast-Töne), keine mitgelieferten Dateien, also
  keine Lizenzfrage. Eine Hörprobe in den Einstellungen gibt es nicht: Das Plugin spielt keinen Ton
  ohne Benachrichtigung. Unter Windows waren Benachrichtigungen bis zu dieser Stufe stumm.
- Die Lese-Liste setzt nie ein Schlagwort auf dem Server, auch nicht mit Schreibzugriff.
  „Ungelesene oben“ ist eine Einstellung des Geräts und wird nicht abgeglichen.
- Blockierte Absender: Neue Mail geht in den Spam-Ordner, wo das Konto schreiben darf und einen
  hat, sonst wird sie ausgeblendet, und zwar in den Posteingangslisten, nicht in Suche und
  Ordnerlisten. Keine Regeln, keine Benachrichtigung für sie.
- Verschieben zwischen Konten: Es braucht die Message-ID, höchstens 500 Mails je Aufruf; die Flags
  `\Seen`, `\Answered`, `\Flagged`, `\Draft` gehen mit, Schlagwörter nicht. Ohne gefundenen Ort im
  Ziel bleibt die Quelle stehen (lieber doppelt als verloren).
- Lesebestätigungen werden beim Lesen erkannt, nicht beim Eintreffen; empfangene erscheinen erst,
  wenn ihr Body geladen ist. Keine `Reporting-UA`-Zeile, die Mail nennt das Programm nicht.
- Ein Gerät mit einer Version vor dieser Stufe lehnt Vorlagen mit Empfängern als unbekannte Struktur
  ab; der Datensatz bleibt erhalten und wird nach dem Update übernommen.
- Wer schon ein Farbthema gewählt hat, behält es; neu ist der Standard Orange (0038).
- Der Index eines angehefteten Anhangs ist seine Position in der Leseansicht (0036, Punkt 3).
- axe „in jedem Farbthema“: In jsdom ist `color-contrast` aus, ein Farbthema ändert dort also nichts;
  per Thema laufen axe für den Abschnitt Darstellung und `contrast.test.ts` für alle Farbpaare. In
  der echten WebView prüft `a11y.e2e.mjs` die Hauptansichten nur im Standardthema Orange.

**Ungeprüft / offen**
- Farbthema zwischen zwei Geräten: Im Stalwart-Test
  (`shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay`) setzt das erste Gerät
  Orange, seit 0038 der Standard; dass das zweite Gerät Orange zeigt, beweist also nichts. Erfasst
  wird der Schlüssel `ui.accent_theme` (siehe oben), die Ankunft eines anderen Themas ist nicht
  geprüft. Seit d95da63 setzt der Test Grün (nicht der Standard); der Lauf gegen Stalwart war
  danach grün (`tests/settings_sync.rs` 18 von 18). Der Abnahmepunkt ist damit erfüllt.
- Desktop-Tests für Schnellantwort, Suchen in der Mail und Anhangsvorschau auf dem neuesten Stand:
  grün auf 5b0c53b mit den Testständen aus 4928d71 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`).
- Ein vollständiger Serverlauf nach 011629f und ein `scripts/check.sh` auf 5b0c53b (nur Klassen und
  CSS in fünf Komponenten) bzw. 011629f liegen nicht vor.
- axe in der echten WebView für die neuen Ansichten (Suchleiste, Vorschau-Dialog, Lese-Liste,
  Angeheftete Anhänge) und in den anderen Farbthemen.
- Vorschau unter WebView2; echte PDFs aus dem Alltag (im Test eine kleine selbst gebaute PDF); keine
  Zeitgrenze beim Rendern einer bösartigen PDF; die Annahme zum Anhangsindex zwischen vollständigem
  und teilweise geladenem Body.
- Verschieben zwischen zwei echten Konten; Server ohne UIDPLUS; Verschieben aus einem Gmail-Konto
  (das Entfernen der Quelle nimmt dort nur das Label). Nur mit Stalwart getestet.
- Lesebestätigungen mit einem anderen Programm (Thunderbird, Outlook) in beide Richtungen.
- Das Abspielen der Töne auf macOS und Windows; getestet sind die Entscheidung im Kern und die
  Zuordnung der Namen in der Shell.
- Windows zur Laufzeit: `prototype-2026-11-23-4` enthält die Stufe; geprüft ist sie unter Windows
  noch nicht (Windows-Checkliste, Abschnitt 20).

**Wartet auf**
- Mensch: zwischen zwei echten Konten verschieben und kopieren (Nebenkonten mit freigeschaltetem
  Schreibzugriff); die Farbthemen hell und dunkel ansehen; eine Lesebestätigung mit einem anderen
  Programm in beide Richtungen austauschen; die Benachrichtigungstöne hören (Ton an und aus, ein
  Konto ohne Ton, Ruhezeiten), auf macOS und Windows.
- Mensch (Windows): ein Prototyp mit dieser Stufe (noch nicht gebaut) und ein Abschnitt der
  Windows-Checkliste dafür.
