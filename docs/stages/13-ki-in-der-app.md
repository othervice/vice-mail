# Stufe 13 — KI in der App (optional)

**Ziel:** Wer möchte, schaltet KI direkt in der App ein: Thread zusammenfassen, Antworten
vorschlagen, beim Schreiben helfen, Fragen in natürlicher Sprache mit Quellen beantworten. Dafür gibt
es eine einzige schlanke Anbieterschicht. Ohne Einschalten verlässt keine Anfrage das Gerät, und KI
führt nie selbst eine Aktion aus. Hinweise auf offene Antworten funktionieren auch ohne KI.

**Voraussetzung:** Stufen 07 und 12 fertig. `docs/decisions/0010-ki-ueber-mcp.md` und Abschnitt „KI“
in `docs/ARCHITECTURE.md` gelesen. Vor der Arbeit an Anthropic den Skill `claude-api` laden.

## Aufgaben

- [x] **Schnittstelle.** Neues Crate `mail-ai` mit Trait `AiProvider`: Anfrage mit Streaming, Abbruch, Liste der Modelle.
- [x] **Unterbau vorgezogen:** Crate `crates/mail-ai` (Trait, beide Anbieter, Prompt-Schutz, Datensparsamkeit, Zusammenfassung und Fragen als Funktionen, Vertragstests gegen eine Attrappe), Entscheidung `docs/decisions/0019-ki-anbieter.md`. Offen: Verdrahtung, Einstellungen, Zustimmung, Protokoll, Antwortvorschläge, Schreibhilfe, offene Antworten, Prüfsatz, Panel.
- [x] **Ein Anbieter.** OpenAI-kompatible Chat-Schnittstelle mit frei wählbarer Basis-URL: Ollama, LM Studio, llama.cpp-Server, OpenRouter, OpenAI. Prüfen, ob Anthropic über seine OpenAI-kompatible Schnittstelle genügt; wenn nicht, einen zweiten Anbieter für die Messages API ergänzen (Stopp-Grund vor dem Lesen der Antwort prüfen, auch Ablehnung). Entscheidung festhalten.
- [x] **Einstellungen.** KI ist aus, bis der Nutzer sie einschaltet. Anbieter-Profile (Schlüssel im Schlüsselbund). Je Funktion einen Anbieter wählen oder aus. Je Konto festlegen, ob Cloud-Anbieter dessen Mails sehen dürfen. Kennzeichnung „lokal“, wenn die Basis-URL auf diesen Rechner oder das eigene Netz zeigt.
- [x] **Transparenz.** Vor der ersten Übertragung an einen Cloud-Anbieter erklären, was gesendet wird, und zustimmen lassen. Bei jeder Anfrage anzeigen: wie viele Mails, welche Größe, an wen. Lokales Protokoll der Anfragen (nur Metadaten).
- [x] **Schutz vor eingeschleusten Anweisungen.** Fester Systemtext im Code. Mailtext nur als klar abgegrenzte Daten. Das eingebaute Modell bekommt keine Werkzeuge. Ausgaben als Text; Bilder aus Ausgaben werden nie geladen; Links durchlaufen die Prüfung aus Stufe 3.
- [x] **Datensparsamkeit.** Nur der aktuelle Thread bzw. die Suchkandidaten, ohne mehrfach zitierte Passagen und ohne Anhänge (nur deren Namen). Wird gekürzt, erfährt es der Nutzer.
- [x] **Zusammenfassung, Antwortvorschläge, Schreibhilfe.** Zusammenfassung mit Kernaussagen, offenen Fragen, Fristen nur wenn genannt; jede Aussage verweist auf ihre Mail, Aussagen ohne gültigen Verweis werden verworfen. Antwortvorschläge als bearbeitbarer Entwurf. Schreibhilfe im Composer auf der Auswahl (verbessern, kürzen, umformulieren, Ton, übersetzen) mit Vorschau.
- [ ] **Suche in natürlicher Sprache.** Prüfsatz mit 30 Fragen unter `testmail/corpus/qa/`. Ablauf: Frage → Suchsprache aus Stufe 7 → lokale Kandidaten → Antwort nur aus den Kandidaten, jede Aussage mit Verweis; keine Kandidaten: ehrlich „nichts gefunden“. Embeddings nur, wenn der Prüfsatz es verlangt.
- [x] **Hinweise auf offene Antworten.** Zuerst ohne KI: gesendete Mail mit Frage und ohne Antwort nach einigen Tagen; empfangene direkte Frage, unbeantwortet. Optional verfeinert das Modell. Anzeige als wegklickbarer Vorschlag, nie als Behauptung.
- [x] **KI-Panel.** Nach `docs/DESIGN.md`: bezogen auf den gewählten Thread, Ergebnisse als Karten mit „Entwurf öffnen“. Streaming und Abbrechen. Ohne eingerichtete KI ist das Panel nicht sichtbar.

## Abnahme

- [x] Test: Bei ausgeschalteter KI registriert der Attrappen-Server null Anfragen, egal was in der Oberfläche passiert
- [x] Test: Ein Konto ohne Cloud-Freigabe schickt nichts an einen Cloud-Anbieter, auch nicht aus dem vereinten Posteingang heraus
- [x] Vertragstests gegen einen lokalen Attrappen-Server: Streaming, Abbruch, Fehler, Ratenbegrenzung, Ablehnung
- [x] Korpus mit eingeschleusten Anweisungen: Die Ausgabe löst keine Aktion und keine Netzverbindung aus
- [x] Test: Verweise von Zusammenfassung und Suchantwort zeigen auf vorhandene Mails; erfundene werden entfernt
- [ ] Ergebnis des Prüfsatzes in `docs/testing/ai-search.md`: Anteil richtiger Fundstellen und Antworten
- [x] Test: Hinweise auf offene Antworten an vorbereiteten Fällen, darunter solche ohne Hinweis (Newsletter, automatische Mails, bereits beantwortet)
- [x] Test: Schlüssel erscheinen weder in Logs noch im Anfrage-Protokoll
- [ ] mit Ollama von Hand geprüft und vermerkt: Zusammenfassung und Antwortvorschlag funktionieren ohne Internet
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- eingebautes Modell
- Werkzeugaufrufe durch das eingebaute Modell
- Kategorien per KI (bei Bedarf später; die Kopfzeilen-Regeln aus Stufe 9 bleiben die Grundlage)

## Der Mensch prüft danach

- Ollama oder einen API-Schlüssel einrichten. Sind Zusammenfassungen, Vorschläge und Antworten an echten Threads brauchbar?

## Ergebnis

**Stand vom 23.11.2026, 09:30.** Unterbau `crates/mail-ai` (3072ea9, Entscheidung 0019),
Verdrahtung im Kern (72c9e8a, Entscheidung 0024), Oberfläche (e0046af, 7ccc4a3), Quellen nach ID
und `aiSettingsChanged` (bd942bc, Entscheidung 0027), Links über die Prüfung aus Stufe 3,
Probelauf vor der Zustimmung und Zustand des Panels im Kern (eed0334, Entscheidung 0028).
Geprüft von der Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf bd942bc (998
Rust-, 1.111 Vitest-Tests in 55 Dateien, mit Clippy für das Windows-Ziel) und auf dem Arbeitsstand,
der als eed0334 committet wurde (1.006 Rust-, 1.127 Vitest-Tests in 57 Dateien, darin die Tests
aus 0028); alle Servertests (`cargo test --workspace -- --include-ignored` gegen den lokalen
Stalwart) grün auf bd942bc (1.076 Tests; `ai::tests::qa::qa_set_with_a_real_model` überspringt sich
ohne konfiguriertes Modell, ist also nicht gelaufen); `pnpm --dir app test:e2e` grün auf bd942bc
mit den Testständen aus ab594b7 (16 Spec-Dateien, 89 Tests grün, 6 übersprungen, darunter
`ai.e2e.mjs` mit 2 von 2 Tests). Der Windows-Prototyp `prototype-2026-11-23-2` (gebaut aus bd942bc)
enthält Stufe 13 ohne die Teile aus eed0334.

**Nachgewiesen (Kern, in `check.sh`)**
- KI aus: `ai::tests::with_ai_off_the_provider_gets_no_request_whatever_is_called` (jede KI-Funktion
  über `AppCore`, auch Modellliste und Verbindungstest; die Attrappe zählt null Anfragen). Die
  Oberfläche erreicht einen Anbieter nur über diese Commands.
- Cloud-Freigabe je Konto:
  `ai::tests::cloud_providers_need_consent_and_see_only_permitted_accounts`;
  `crates/mail-ai/tests/flows.rs::accounts_without_cloud_permission_send_nothing_to_a_cloud_provider`,
  `unified_search_leaves_out_candidates_without_cloud_permission` (vereinter Posteingang),
  `local_providers_need_no_cloud_permission`.
- Vertragstests gegen einen lokalen Attrappen-Server (`crates/mail-ai/tests/contract.rs`): Streaming
  (`openai_streams_and_sends_only_what_the_request_holds`,
  `anthropic_streams_per_the_documentation`), Abbruch
  (`cancelling_closes_the_connection_and_the_server_sees_it`,
  `cancelling_before_the_answer_arrives`, im Kern `ai::tests::a_running_request_can_be_cancelled`),
  Fehler (`server_errors_and_overload`, `anthropic_errors_status_and_in_stream`,
  `broken_event_streams_are_protocol_errors`, `time_limits`), Ratenbegrenzung
  (`rate_limits_carry_retry_after`), Ablehnung
  (`anthropic_refusal_is_checked_before_reading_the_answer`,
  `length_and_content_filter_are_checked_before_the_text`), dazu `redirects_are_never_followed`,
  `cookies_are_never_stored_or_sent`, `models_are_listed`, `anthropic_model_list_is_paged`.
- Eingeschleuste Anweisungen:
  `tests/flows.rs::smuggled_instructions_stay_data_and_cause_no_action_or_connection`,
  `ai::tests::smuggled_instructions_trigger_no_action_and_no_connection`; das Modell bekommt keine
  Werkzeuge (`tests/structure.rs::requests_have_no_tools`,
  `no_file_system_processes_or_own_sockets`), Bildadressen in Ausgaben werden entfernt
  (`text::tests::markdown_images_lose_their_target`), fester Systemtext und Datenblock mit Nonce
  (`prompt::tests::*`).
- Links aus Ausgaben über die Prüfung aus Stufe 3 (neu in eed0334):
  `ai::tests::links_in_results_go_through_the_link_check_of_mail` (dieselben Warnungen wie bei
  Mails, `http(s)` an das System, `mailto:` als Composer-Anfrage, unbekannter Index und gefälschtes
  Token `not_found`, `linkActivated` lehnt ab, Markdown-Bild ohne Ziel, andere Schemata bleiben
  Text, keine weitere Verbindung),
  `reading::links::tests::ai_results_have_their_own_bound_and_go_with_their_accounts`,
  `found_links_are_numbered_across_the_texts_of_a_result`.
- Verweise: `tests/flows.rs::summary_end_to_end_keeps_only_valid_references`,
  `answers_come_only_from_candidates`, `summary::tests::invented_references_are_removed`,
  `claims::tests::claims_without_valid_reference_or_text_are_dropped`,
  `ai::tests::a_summary_streams_progress_logs_metadata_and_keeps_only_valid_references`; Quellen
  nach ID benennen (neu in bd942bc):
  `ai::tests::sources_are_named_by_id_in_the_order_asked_and_bounded`.
- Prüfsatz mit der Modell-Attrappe (`ai::tests::qa::qa_set_with_the_mock_model`,
  `docs/testing/ai-search.md`, 30 Fragen unter `testmail/corpus/qa/`): erwartete Mail unter den
  Kandidaten 25 von 25 (24 als erster Kandidat), ehrlich „nichts gefunden“ 5 von 5, erfundene
  Verweise in 30 von 30 entfernt. Das misst die lokale Suche und den Ablauf, nicht ein Modell.
- Hinweise auf offene Antworten ohne KI:
  `ai::open_replies::tests::a_sent_question_without_answer_is_awaiting_after_some_days`,
  `a_direct_question_to_me_is_unanswered_until_i_reply`,
  `newsletters_and_automatic_mail_never_get_a_hint` (Newsletter, automatische Mails),
  `only_the_newest_message_of_a_side_counts_and_off_is_off` (schon beantwortet),
  `questions_are_found_in_text_people_wrote`,
  `ai::tests::open_reply_hints_read_the_store_and_dismissing_needs_a_message`.
- Schlüssel: `ai::tests::keys_appear_neither_in_logs_nor_in_the_request_log_nor_in_the_database`,
  `profile::tests::keys_never_show_in_debug_output`,
  `contract.rs::unauthorized_errors_never_show_the_key`.
- Probelauf vor der Zustimmung (neu in eed0334):
  `ai::tests::the_dry_run_shows_exactly_what_would_be_sent_and_sends_nothing` (die Attrappe zählt
  null Verbindungen und null Anfragen, kein Ereignis, kein Protokolleintrag; nach der Zustimmung
  ist die Übertragung der echten Anfrage gleich dem Probelauf, für alle vier Funktionen).
- Einstellungen in allen Fenstern aktuell (neu in bd942bc):
  `ai::tests::every_settings_change_is_announced_with_what_is_usable`; Zustand des Panels nur auf
  dem Gerät (eed0334): `ui_prefs::tests::the_ai_panel_state_stays_on_this_device`,
  `ui::tests::the_ai_panel_state_is_one_of_two_values`.
- Weiter: Datensparsamkeit (`prepare::tests::quotes_of_earlier_messages_are_left_out`,
  `threads_send_names_not_attachments_and_count_everything`,
  `the_budget_keeps_the_newest_and_reports_what_was_cut`); Antwortvorschlag wird erst auf Klick ein
  normaler Entwurf (`ai::tests::a_reply_suggestion_becomes_an_editable_draft_only_when_opened`);
  Schreibhilfe (`writing::tests::*`); „lokal“ aus der Basis-URL (`endpoint::tests::this_device`,
  `own_network`, `internet`); Profile geprüft, Löschen schaltet ihre Funktionen aus
  (`ai::tests::profiles_are_validated_and_deleting_one_turns_its_functions_off`); KI-Einstellungen
  sind Geräte-Einstellung, die der Einstellungs-Sync ablehnt
  (`ai::tests::ai_settings_are_a_device_setting_the_settings_sync_refuses`); Anfrage-Protokoll nur
  mit Metadaten (`ai::tests::the_request_log_pages_and_prunes`).

**Nachgewiesen (Oberfläche, Vitest)**
- Einstellungen (`ai-settings.test.tsx`): aus, bis eingeschaltet, getrennt von KI über MCP; lokaler
  Anbieter aus einer Vorlage, Modelle erst auf Wunsch geladen; der Schlüssel geht einmal an den
  Kern und wird nie wieder gezeigt; Prüfung des Formulars und Ablehnung des Kerns am Formular;
  Anbieter oder aus je Funktion, Cloud-Freigabe je Konto (Standard nein), Zustimmung je
  Cloud-Anbieter; Verbindungstest ohne Mail, Löschen nach Rückfrage; Hinweise auf offene Antworten
  und ihre Tage auch bei ausgeschalteter KI; Protokoll nur mit Metadaten; Fehler beim Laden und
  Ändern.
- KI aus und Cloud-Freigabe (`ai-off-and-hints.test.tsx`): Bei ausgeschalteter KI ruft die
  Oberfläche keine KI-Funktion auf, was der Nutzer auch tut; ohne Cloud-Freigabe geht nichts eines
  Kontos an einen Cloud-Anbieter, auch nicht aus dem vereinten Posteingang; Hinweise auf offene
  Antworten als wegklickbare Vorschläge ohne KI, im Panel und über der Mail.
- KI-Panel (`ai-panel.test.tsx`, `ai-logic.test.ts`): nicht sichtbar, solange KI aus ist oder keine
  Funktion einen Anbieter mit Modell hat; Zusammenfassung mit Fortschritt, dann Karten, deren
  Aussagen auf ihre Mail verweisen; Anzeige, was gesendet wird (Mails, Größe, Host, Modell), mit
  Abbrechen; Antwortvorschlag wird erst mit „Entwurf öffnen“ ein Entwurf, mit dem bearbeiteten Text;
  Zustimmung vor der ersten Cloud-Anfrage und bei `ai_consent_required`, vorher wird nichts
  gesendet; Fragen mit Verweisen oder ehrlich „nichts gefunden“, mit Zahl der weggelassenen Konten;
  typisierte Fehler mit Weg in die Einstellungen; Befehle in der Palette.
- Schreibhilfe (`ai-writing-help.test.tsx`): auf der Auswahl mit Vorschau, nur „Übernehmen“ ändert
  den Entwurf; Ton und Übersetzung; ohne Auswahl nichts; Zustimmung vor einem Cloud-Anbieter; ohne
  Anbieter nicht im Composer; Abbruch beim Schließen; Markup in der Antwort wird nie geparst.
- Links, Probelauf, Panel-Zustand (neu in eed0334, `ai-links-and-consent.test.tsx`): Links öffnen
  nur über dieselbe Prüfung wie Links in Mails, nie direkt, auch per Tastatur; ein vergessenes Token
  wird erklärt; Links des Antwortvorschlags stehen unter dem Textfeld; der Zustimmungsschritt zeigt
  die genaue Größe aus dem Probelauf ohne Anfrage an den Anbieter, bei Fragen den Hinweis auf die
  zweite Anfrage, und bleibt bei gescheitertem Probelauf bedienbar; Auf- und Zuklappen liegt im Kern
  und folgt anderen Fenstern.
- Quellen (`open-by-id.test.tsx`, `list-select.test.tsx`): benannt nach Absender und Datum, ein
  Klick zeigt die Mail im offenen Thread oder in der Liste, sonst in eigenem Fenster;
  `aiSettingsChanged` bringt Panel und Befehle in jedem Fenster.
- axe ohne schwere Befunde in Einstellungen → KI (mit Formular, Zustimmung und Protokoll), im Panel
  mit allen Karten und in der Schreibhilfe mit Vorschau, hell und dunkel (`a11y.test.tsx`).

**Nachgewiesen (echte App, `ai.e2e.mjs` auf bd942bc)**
- Bei ausgeschalteter KI fehlen Panel und Schreibhilfe; mit einem Profil, das auf einen Zähl-Server
  zeigt, scheitert die Modellliste mit `ai_off`, der Zähl-Server sieht nichts.
- Eingeschaltet fragt die App die Modelle erst auf „Modelle laden“ ab (eine Anfrage am Zähl-Server).

**Offen**
- Prüfsatz mit einem echten Modell (Anteil richtiger Fundstellen und Antworten); erst danach lässt
  sich entscheiden, ob Embeddings nötig sind. Deshalb bleibt die Aufgabe „Suche in natürlicher
  Sprache“ offen, obwohl Ablauf und Prüfsatz gebaut und mit der Attrappe geprüft sind.
- Die optionale Verfeinerung der Hinweise auf offene Antworten durch das Modell ist nicht gebaut
  (0024).

**Abweichungen und Annahmen**
- Zwei Anbieter hinter `AiProvider`: OpenAI-kompatibel (frei wählbare Basis-URL) und die Anthropic
  Messages API, weil die OpenAI-kompatible Schnittstelle von Anthropic Ablehnungen nicht erkennbar
  meldet (0019).
- Einstellungen als eine Geräte-Einstellung `ai.settings` statt einer Tabelle `ai_profile`;
  Schlüssel im Schlüsselbund unter `ai-profile:<id>`, die Oberfläche erfährt nur `hasKey`.
- Zustimmung gilt je Cloud-Profil für Host und Port; ein neuer Host nimmt sie zurück. Modellliste
  und Verbindungstest senden keine Mail und brauchen keine Zustimmung, aber eingeschaltete KI.
- Gestreamt wird der Fortschritt (Antworten sind JSON), nicht der Text.
- Bei Fragen geht der Text der Frage auch dann hinaus, wenn Konten keine Cloud-Freigabe haben; deren
  Kandidaten werden weggelassen und gezählt.
- Links in Ausgaben (0028) findet der Kern selbst und legt sie unter einem Token je Ergebnis ins
  Register der Links (höchstens 32 Ergebnisse, getrennt von den Darstellungen der Mails); die
  Oberfläche schickt nur Token und Index. Bilder werden nie geladen.
- Der Probelauf zeigt bei Fragen nur die erste Anfrage (die Frage); die Größe der zweiten hängt von
  der Suche ab und wird vor dem Senden gemeldet.
- Der Zustand des Panels (offen oder zugeklappt) bleibt auf dem Gerät und wird nicht abgeglichen.

**Ungeprüft**
- Kein echtes Modell: Zusammenfassung, Antwortvorschlag, Schreibhilfe und Fragen liefen nur gegen
  die Attrappe.
- Der Desktop-Test prüft nur „aus“ und die Modellliste; Panel, Zustimmung, Schreibhilfe, Links
  und Probelauf laufen nicht in der echten App. Die Teile aus eed0334 waren im Desktop-Lauf auf
  bd942bc noch nicht enthalten.
- Der Servertest des Einstellungs-Syncs, den eed0334 um den Panel-Zustand ergänzt hat
  (`shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay`), lief nur in der
  Fassung von bd942bc.
- Windows: Schlüssel im Windows-Anmeldeinformationsspeicher, Panel und Schreibhilfe unter WebView2.

**Wartet auf**
- Mensch: Ollama (oder einen anderen lokalen Server) einrichten; Prüfsatz mit echtem Modell laufen
  lassen (Befehl in `docs/testing/ai-search.md`, Test `qa_set_with_a_real_model`) und das Ergebnis
  dort eintragen; mit Ollama ohne Internet Zusammenfassung und Antwortvorschlag prüfen und
  vermerken; danach an echten Threads beurteilen, ob Zusammenfassungen, Vorschläge und Antworten
  brauchbar sind.
