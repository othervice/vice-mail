# Stufe 09 — Posteingang-Komfort

**Ziel:** Der Posteingang sortiert sich selbst (ohne KI), Mails lassen sich zurückstellen und
anpinnen, die App meldet sich mit Benachrichtigungen und ist als Standard-Mail-App nutzbar.

**Voraussetzung:** Stufe 08 fertig. Die Stufe ist groß; die Blöcke sind unabhängig und können auf
zwei Sitzungen verteilt werden (A: Kategorien, Pins, Snooze, Aufgaben, Regeln. B: der Rest).

## Aufgaben A

- [x] **Kategorien.** Beim Eintreffen einordnen anhand der Kopfzeilen: Newsletter (List-Id, List-Unsubscribe, `Precedence: bulk`), Benachrichtigungen (Auto-Submitted, typische Absender ohne Antwortadresse), Personen (alles andere). Korrektur durch den Nutzer je Absender („immer als …“), gespeichert als Regel. Wichtige Absender bilden den Block „Priorität“.
- [x] **Bündel.** Benachrichtigungen und Newsletter als eine aufklappbare Zeile mit Absender-Chips und Zählern, nach `docs/DESIGN.md`. Kategorien abschaltbar; dann ist der Posteingang rein zeitlich.
- [x] **Pins.** Angepinnte Mails stehen oben und in der Ansicht „Angepinnt“.
- [x] **Snooze.** Zeitpunkt wählen (später heute, morgen, Wochenende, nächste Woche, eigener). Die Mail verschwindet aus dem Posteingang und erscheint zum Zeitpunkt wieder oben, mit Kennzeichnung und Benachrichtigung. Lokal gespeichert; zusätzlich ein Schlagwort am Server, wenn er eigene Schlagwörter erlaubt. Die Mail wird auf dem Server nie verschoben. Ansicht „Später“.
- [x] **Aufgaben.** Mail als Aufgabe markieren, Ansicht „Aufgaben“, erledigt-Schalter. Schlagwort am Server, wenn möglich, sonst lokal.
- [x] **Lokale Regeln.** Bedingungen und Aktionen nach `docs/SPEC.md`. Laufen beim Eintreffen, während die App läuft, und sind in der Oberfläche als lokal bezeichnet. Aktionen gehen über die Outbox.

## Aufgaben B

- [x] **Benachrichtigungen.** Native Meldungen für neue Mails (Standard: nur Personen und Priorität), fehlgeschlagene Sendungen, fällige Snoozes. Je Konto schaltbar, Ruhezeiten, Inhalt auf dem Sperrbildschirm ausblendbar. Klick öffnet die Mail. Zähler am Dock- bzw. Taskleisten-Symbol.
- [x] **Standard-Mail-App.** `mailto:` registrieren (Tauri-Deep-Link-Plugin), Einstellung „Als Standard festlegen“ mit dem Weg, den das jeweilige System vorsieht. Ein `mailto:`-Link öffnet einen Entwurf mit allen Feldern. `.eml`-Dateien öffnen.
- [x] **Abmelden.** Bei Mails mit List-Unsubscribe eine Schaltfläche. Ein-Klick-Abmeldung per HTTPS-POST nach RFC 8058 oder ein vorbereiteter Entwurf bei `mailto:`. Immer nur nach Klick.
- [x] **Kalendereinladungen.** `text/calendar` lesen und als Karte zeigen: Titel, Zeit in der eigenen Zeitzone, Ort, Veranstalter. „Im Kalender öffnen“ übergibt die Datei an das System.
- [x] **Drucken und Export.** Bereinigte Mail drucken und als PDF sichern; als `.eml` exportieren.

## Abnahme

- [x] Test der Einordnung an einem vorbereiteten Korpus; Trefferquote steht im Ergebnis; Korrekturen des Nutzers gehen immer vor
- [x] Tests für Snooze mit gestellter Uhr: Zeitzonen, Sommerzeit, Rechner schläft über den Zeitpunkt hinweg, App war geschlossen
- [x] Test: Snooze, Pin und Aufgabe verschieben nichts auf dem Server
- [x] Tests für Regeln: jede Bedingung und Aktion, Reihenfolge, Regel auf nur lesendem Konto wird übersprungen und gemeldet
- [x] Tests für `mailto:` nach RFC 6068, auch mit kodiertem Betreff und Text
- [x] Test: Ohne Klick geht keine Abmelde-Anfrage hinaus (Zähl-Server)
- [x] Tests für Einladungen: ganztägig, wiederkehrend, fremde Zeitzone, kaputte Datei
- [ ] von Hand auf beiden Systemen geprüft und im Ergebnis vermerkt: Benachrichtigungen, Ruhezeiten, Standard-Mail-App
- [x] Leistungswerte neu gemessen; `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Zu- oder Absagen von Einladungen, eigener Kalender
- Kategorien per KI (bei Bedarf nach Stufe 13)
- Abgleich von Snooze und Pins zwischen Geräten (Stufe 11)

## Der Mensch prüft danach

- Passt die Einordnung beim eigenen Posteingang? Welche Absender landen falsch?
- Benachrichtigungen und `mailto:`-Links auf Mac und Windows ausprobieren.

## Ergebnis

Ausgeführt am 23.11.2026 (Hauptsitzung mit Subagenten: Kern, Speicher und Shell für A und B in
fab66da, Oberfläche A in 7acc55c, Oberfläche B in 2152c42; `.eml`-Dateien und der Klick auf eine
Meldung in 24ac670, Auswahl nach ID in e705d75). Geprüft von der Hauptsitzung in sauberen
Worktrees: `scripts/check.sh` grün auf 24ac670 (998 Rust-, 1.111 Vitest-Tests in 55 Dateien, mit
Clippy für das Windows-Ziel, cargo deny und pnpm audit) und auf dem Arbeitsstand, der als e705d75
committet wurde (1.006 Rust-, 1.127 Vitest-Tests in 57 Dateien); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) grün auf 24ac670 (1.076
Tests); `pnpm --dir app test:e2e` grün auf 24ac670 mit den Testständen aus ab594b7 (16
Spec-Dateien, 89 Tests grün, 6 übersprungen, darunter `comfort.e2e.mjs` mit 4 von 4 und
`extras.e2e.mjs` mit 5 von 5 Tests). Entscheidungen: 0016 (Kategorien und Kontakte), 0020 (Kern,
Speicher und Shell), 0027 (`.eml`-Dateien, Klick auf eine Meldung), 0028 (Auswahl nach ID).
Messung: `docs/testing/performance.md` (Stufe 9).

**Nachgewiesen**
- Einordnung: Trefferquote am Seed-Korpus über den echten IMAP-Weg 72 von 73 = 98,6 %
  (`comfort::server_tests::categories_marks_and_invitations_on_the_test_server`, Stalwart; der eine
  Fehler ist die Phishing-Mail `html_hostile.eml`); dazu `tests/classify_seed.rs`
  (`seed_corpus_is_sorted_as_expected`), `classify::tests::table_of_realistic_header_sets`,
  `important_senders_form_the_priority_block`. Korrekturen des Nutzers gehen immer vor:
  `classify::tests::user_corrections_come_first`,
  `comfort::tests::categories_bundles_and_corrections`.
- Snooze mit gestellter Uhr: Zeitzonen und Sommerzeit in
  `comfort::time::tests::menu_times_in_berlin`, `daylight_saving_changes`, `other_zones`,
  `custom_times_are_checked`; der Rechner schläft über den Zeitpunkt (Uhr springt, `resume`) in
  `comfort::tests::snooze_hides_and_returns_on_top_with_a_notification`; App war geschlossen in
  `snoozes_due_while_the_app_was_closed_come_back_at_start`.
- Nichts verschoben: `comfort::tests::snooze_pin_and_task_move_nothing_on_the_server` (Fake-Server)
  und der Stalwart-Test oben (nur `STORE`); in der App (`comfort.e2e.mjs`) „pins, snoozes and marks
  tasks on a read-only account without moving anything on the server“.
- Regeln: `comfort::rules::tests::every_condition`, `every_action_and_the_order`,
  `all_or_any_and_empty`, `comfort::tests::rules_run_on_new_mail_in_order`,
  `rules_on_read_only_accounts_are_skipped_and_reported` (übersprungen und mit `ruleSkipped`
  gemeldet), Store `stage9_tests::local_rules_are_checked_and_ordered`; Oberfläche
  `comfort-rules.test.tsx` (als lokal beschriftet, Prüfung vor dem Kern, Reihenfolge).
- `mailto:` nach RFC 6068: `mailto::tests::rfc6068_basic_examples`, `rfc6068_complicated_addresses`,
  `rfc6068_non_ascii_examples`, `encoded_subject_and_body`, `rfc2047_subjects_and_names`,
  `dangerous_headers_are_never_applied`, `header_injection_stays_text`,
  `blind_copies_are_flagged_first`; Kern `comfort::tests::mailto_links_ask_for_the_composer`,
  Entwurf mit allen Feldern
  `compose::stage10_tests::a_compose_request_fills_every_field_and_threads_the_reply`; Oberfläche
  `extras-mailto.test.tsx` (Link beim Start und während des Laufs, Warteschlange neben einem offenen
  Entwurf, nichts wird gesendet); in der App (`reading.e2e.mjs`) öffnet „Öffnen“ auf einem
  `mailto:`-Link einer Mail einen Entwurf im eigenen Composer, nichts geht an das System.
- Abmelden ohne Klick keine Anfrage:
  `comfort::tests::unsubscribe_requests_go_out_only_after_a_click` (HTTPS-Zähl-Server mit eigener
  Test-CA: 0 Verbindungen nach Abruf, Öffnen und Anzeigen der Wege, 1 nach dem Klick),
  `comfort::unsubscribe::tests::private_targets_are_refused_before_any_connection`,
  `one_click_header`; Oberfläche `extras-unsubscribe.test.tsx`; in der App (`extras.e2e.mjs`) zeigt
  ein Newsletter des Seeds „Abmelden“ mit allen drei Wegen, Öffnen und Abbrechen der Frage erzeugt
  kein Ergebnis und keine Operation.
- Einladungen: ganztägig (`calendar::tests::all_day_single_day_without_end_lasts_one_day`,
  `all_day_over_several_days_has_an_exclusive_end`), wiederkehrend (`weekly_with_count`,
  `weekly_with_until`, `monthly_and_yearly_rules_and_local_until`), fremde Zeitzone
  (`foreign_zone_across_the_dst_boundaries`, `outlook_invite_with_windows_zone_name`,
  `unknown_zone_is_reported_and_read_as_floating`), kaputte Datei (`empty_and_garbage_files`,
  `broken_lines_and_bytes`, `truncated_files_still_show_their_events`,
  `mutated_invitations_never_panic`); Oberfläche `extras-invites.test.tsx`,
  `extras-calendar.test.ts`; in der App (`extras.e2e.mjs`) die Einladung des Seeds als Karte mit
  Titel, Zeit, Veranstalter und „Im Kalender öffnen“ (nicht geklickt).
- Bündel, Pins, Aufgaben, Ansichten: `comfort::tests::bundles_continue_across_pages`,
  `unified_scope_and_groups`; Oberfläche `comfort.test.tsx`, `comfort-logic.test.ts`; in der App
  (`comfort.e2e.mjs`) Bündel klappen an Ort und Stelle auf, „Warum hier?“ im Lesebereich, Sortierung
  aus macht den Posteingang rein zeitlich.
- Benachrichtigungen und Zähler (Entscheidung im Kern):
  `comfort::notify::tests::people_and_priority_by_default`, `several_hidden_muted_quiet`,
  `sends_and_snoozes`, `rules_override_categories`,
  `comfort::tests::notifications_follow_the_settings`, `failed_sends_are_announced_once`,
  `notifications_wait_for_the_shell_s_sink`, `comfort::time::tests::quiet_minutes`; Zählersymbol
  unter Windows `notifications::overlay::tests::badges_have_the_size_and_white_digits`; Oberfläche
  (`comfort.test.tsx`): Einstellungen mit Kategorien, Ruhezeiten, verborgenem Inhalt, stummen Konten
  und Zähler.
- Klick auf eine Meldung öffnet die Mail (neu in 24ac670 und e705d75): Der Ordner der Mail wird
  gezeigt und die Mail dort in der Liste ausgewählt, sobald die Liste geladen ist; zeigt der offene
  Thread sie schon, bleibt sie dort; fehlt sie unter den geladenen Zeilen oder kommt die Liste
  nicht binnen 5 s, öffnet sie sich in einem eigenen Fenster; nur das Hauptfenster antwortet
  (`open-by-id.test.tsx`, `list-select.test.tsx`).
- `.eml`-Dateien öffnen (neu in 24ac670, 0027): Kern
  `reading::eml::tests::opened_files_go_through_the_reading_pipeline_and_never_into_the_store`,
  `attachments_of_a_file_are_saved_exactly_and_opened_only_after_confirmation`,
  `files_that_cannot_be_shown_are_refused_with_typed_errors` (leer, Text, binär, fehlt, Ordner,
  ein Byte zu groß, genau die Grenze), `files_from_the_system_keep_their_error_for_their_window`,
  `the_register_is_bounded_and_closing_forgets_the_file_and_its_links`, `names_and_the_mail_check`;
  Modell `eml::tests::*`; Shell `tests::eml_files_open_in_message_windows_and_messages_come_by_id`
  (Startargumente, Fenster mit Query und Rechten der Mail-Fenster, gemerkter Fehler, falsche ID,
  Schließen vergisst die Datei), `eml_files::tests::eml_files_are_taken_from_the_arguments`,
  `only_file_urls_of_eml_files_count`,
  `windows::tests::eml_windows_are_message_windows_with_their_own_query`; Oberfläche
  (`eml.test.tsx`): schreibgeschützt über die Lese-Pipeline im Sandbox-Frame, externe Inhalte
  gesperrt, Anhänge über den Kern und ausführbare erst nach Warnung, Links über die Prüfung des
  Kerns, Fehler einer Datei vom System im eigenen Fenster, „.eml-Datei öffnen …“ in der Palette.
  Die Anmeldung für `.eml` steht in `bundle.fileAssociations` (`tauri.conf.json`).
- Export und Drucken:
  `comfort::tests::export_writes_the_message_as_on_the_server_and_counts_the_badge`;
  `extras-tools.test.tsx` (Speichern als `.eml`, Druckdialog des Systems, Druck-Styles nur für die
  Mail); in der App (`extras.e2e.mjs`) überleben die Druck-Styles den Build: eine
  `@media print`-Regel blendet alles außer der Mail aus (der Druckdialog wird nicht geöffnet).
- Standard-Mail-App: `default_app::tests::identifiers_and_arguments`, `extras-default-app.test.tsx`
  (macOS setzen oder Weg über Mail erklären, Windows öffnet die Systemeinstellungen); in der App
  (`extras.e2e.mjs`) zeigen die Einstellungen, ob die App Standard ist (der Knopf wird nicht
  gedrückt). `extras.e2e.mjs` hatte keinen CSP-Verstoß.
- Leistung (Stufe 9, Release-Build, nur Kern): die Einordnung kostet je Fenster von 500 rund 2,5 ms
  (100.000 Kopfdaten 2.703 ms ohne, 3.201 ms mit Einordnung); eine Absender-Regel auf 100.000 Mails
  27 ms; Posteingangsseiten 0,21–0,25 ms; Zähler am Symbol 2,3 ms.

**Abweichungen und Annahmen**
- Kategorie als Zahl in `message.category` (Spalte war schon INTEGER), dazu eine Kopie am Ort für
  Listen über einen Index.
- Aufgaben werden lokal gehalten und nur bei Schreibzugriff zusätzlich als `$Task` gespiegelt;
  Snooze setzt `$Snoozed`, Pins bekommen kein Schlagwort. Mails ohne Message-ID lassen sich nicht
  markieren.
- Lokale Regeln laufen nur für neue Mails im Posteingang außerhalb des Erstabgleichs, während die
  App läuft; Verschieben und Markieren werden auf nur lesenden Konten übersprungen und gemeldet.
- Sperrbildschirm: Die App kann den Inhalt aller Meldungen ausblenden; ob Vorschauen auf dem
  Sperrbildschirm erscheinen, entscheidet das System.
- Standard-App unter macOS über `osascript` mit festem Skript (kein `unsafe`), unter Windows wählt
  der Nutzer in den Systemeinstellungen; der Installer trägt die App als `mailto`-Programm ein.
- Benachrichtigungs-Einstellungen werden seit Stufe 11 abgeglichen (0022, Abweichung von 0020).
- `.eml`-Dateien (0027) öffnen in einem eigenen Mail-Fenster, schreibgeschützt, und werden nie
  importiert (kein Konto, kein Ordner, kein Suchindex, keine Datenbankzeile); höchstens 64 MiB je
  Datei und 16 offene Dateien. Unter macOS meldet sich die App mit Rang „Alternate“ an, nimmt Mail
  also nicht die Rolle des Standards weg. „Für diesen Absender immer laden“ gibt es für Dateien
  nicht.
- Die Liste wählt eine Mail nur unter den geladenen Zeilen aus (0028); eine nicht geladene Mail
  oder ein Mitglied eines zugeklappten Bündels öffnet sich in einem eigenen Fenster.

**Ungeprüft / offen**
- Leistungswerte aus Stufe 7 am 23.11. neu gemessen (`docs/testing/performance.md`, Abschnitt
  „Stufe 8/9 – Nachmessung“, nur der Kern): Die Nachmessung fand eine Verschlechterung beim Start
  (13 statt 2 ms, 98 statt 21 MB im Leerlauf; veraltete SQLite-Statistiken ließen eine Prüfung die
  ganze Nachrichtentabelle lesen) und behob sie (Entscheidung 0029): Start 4 ms, Leerlauf 25 MB,
  Suche und Listen unverändert weit unter den Zielen.
- Befund aus dem Desktop-Test: Nachdem alle 9.959 Zeilen des Posteingangs (mit aufgeklappten
  Bündeln) zweimal gerendert worden waren, wurde späteres Scrollen langsam (längster Frame rund
  250 ms statt 25 ms); der Test misst das Scrollen vorher. Wird gerade untersucht.
- `.eml` in der echten App: Doppelklick im Finder und im Explorer, „Öffnen mit“, der zweite Start
  unter Windows mit einer Datei, das Schließen eines echten Fensters (der Mock der Shell sendet
  keine Fensterereignisse), ob macOS die Zuordnung ohne neue Anmeldung bei Launch Services
  übernimmt; einen Desktop-Test für `.eml` gibt es nicht.
- Auswahl nach ID und Klick auf eine Meldung nur in Vitest geprüft; der Desktop-Lauf auf 24ac670
  enthielt e705d75 noch nicht. Ein zugeklapptes Bündel bei der Auswahl nach ID ist nicht geprüft.
- Native Meldungen, Klick auf eine Meldung, Zähler an Dock und Taskleiste, Ruhezeiten, Standard-App,
  ein `mailto:`-Link aus dem Browser, Drucken und „Als PDF sichern“ auf echten Systemen; ob macOS
  beim Setzen des Standards nachfragt.

**Wartet auf**
- Mensch: Passt die Einordnung beim eigenen Posteingang, welche Absender landen falsch?
  Benachrichtigungen, Ruhezeiten, Standard-Mail-App, `mailto:`-Links und `.eml`-Dateien
  (Doppelklick, „Öffnen mit“) auf Mac und Windows von Hand prüfen und hier vermerken. Für Windows
  enthält der Prototyp `prototype-2026-11-23-2` (Gitea-Vorab-Release, gebaut aus 24ac670) Teil A
  und Teil B; die Windows-Checkliste hat einen Abschnitt nur für Teil A (Abschnitt 13), für Teil B
  stehen die Schritte im Release-Hinweis.
