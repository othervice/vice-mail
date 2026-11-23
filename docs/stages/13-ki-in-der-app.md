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
- [ ] **Einstellungen.** KI ist aus, bis der Nutzer sie einschaltet. Anbieter-Profile (Schlüssel im Schlüsselbund). Je Funktion einen Anbieter wählen oder aus. Je Konto festlegen, ob Cloud-Anbieter dessen Mails sehen dürfen. Kennzeichnung „lokal“, wenn die Basis-URL auf diesen Rechner oder das eigene Netz zeigt.
- [ ] **Transparenz.** Vor der ersten Übertragung an einen Cloud-Anbieter erklären, was gesendet wird, und zustimmen lassen. Bei jeder Anfrage anzeigen: wie viele Mails, welche Größe, an wen. Lokales Protokoll der Anfragen (nur Metadaten).
- [ ] **Schutz vor eingeschleusten Anweisungen.** Fester Systemtext im Code. Mailtext nur als klar abgegrenzte Daten. Das eingebaute Modell bekommt keine Werkzeuge. Ausgaben als Text; Bilder aus Ausgaben werden nie geladen; Links durchlaufen die Prüfung aus Stufe 3.
- [ ] **Datensparsamkeit.** Nur der aktuelle Thread bzw. die Suchkandidaten, ohne mehrfach zitierte Passagen und ohne Anhänge (nur deren Namen). Wird gekürzt, erfährt es der Nutzer.
- [ ] **Zusammenfassung, Antwortvorschläge, Schreibhilfe.** Zusammenfassung mit Kernaussagen, offenen Fragen, Fristen nur wenn genannt; jede Aussage verweist auf ihre Mail, Aussagen ohne gültigen Verweis werden verworfen. Antwortvorschläge als bearbeitbarer Entwurf. Schreibhilfe im Composer auf der Auswahl (verbessern, kürzen, umformulieren, Ton, übersetzen) mit Vorschau.
- [ ] **Suche in natürlicher Sprache.** Prüfsatz mit 30 Fragen unter `testmail/corpus/qa/`. Ablauf: Frage → Suchsprache aus Stufe 7 → lokale Kandidaten → Antwort nur aus den Kandidaten, jede Aussage mit Verweis; keine Kandidaten: ehrlich „nichts gefunden“. Embeddings nur, wenn der Prüfsatz es verlangt.
- [ ] **Hinweise auf offene Antworten.** Zuerst ohne KI: gesendete Mail mit Frage und ohne Antwort nach einigen Tagen; empfangene direkte Frage, unbeantwortet. Optional verfeinert das Modell. Anzeige als wegklickbarer Vorschlag, nie als Behauptung.
- [ ] **KI-Panel.** Nach `docs/DESIGN.md`: bezogen auf den gewählten Thread, Ergebnisse als Karten mit „Entwurf öffnen“. Streaming und Abbrechen. Ohne eingerichtete KI ist das Panel nicht sichtbar.

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

**Zwischenstand vom 23.11.2026, Stufe in Arbeit.** Unterbau `crates/mail-ai` (ddac6cc, Entscheidung
0019) und die Verdrahtung im Kern (9ebf727, Entscheidung 0024) sind fertig; die Oberfläche
(Einstellungen, Zustimmung, Anzeige je Anfrage, Protokoll, KI-Panel, Schreibhilfe) entsteht gerade
in einer eigenen Sitzung und ist noch nicht committet. Geprüft von der Hauptsitzung:
`scripts/check.sh` grün auf 255ce74 (947 Rust-, 909 Vitest-Tests in 41 Dateien), darin `mail-ai` mit
55 Unit-, 20 Vertrags-, 11 Ablauf-, 4 Struktur- und 1 TLS-Test und `mail-engine` mit `ai::tests`
samt Prüfsatz. Der Windows-Prototyp `prototype-2026-11-23` enthält Stufe 13 nicht.

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
- Verweise: `tests/flows.rs::summary_end_to_end_keeps_only_valid_references`,
  `answers_come_only_from_candidates`, `summary::tests::invented_references_are_removed`,
  `claims::tests::claims_without_valid_reference_or_text_are_dropped`,
  `ai::tests::a_summary_streams_progress_logs_metadata_and_keeps_only_valid_references`.
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
- Weiter: Datensparsamkeit (`prepare::tests::quotes_of_earlier_messages_are_left_out`,
  `threads_send_names_not_attachments_and_count_everything`,
  `the_budget_keeps_the_newest_and_reports_what_was_cut`); Antwortvorschlag wird erst auf Klick ein
  normaler Entwurf (`ai::tests::a_reply_suggestion_becomes_an_editable_draft_only_when_opened`);
  Schreibhilfe (`writing::tests::*`); „lokal“ aus der Basis-URL (`endpoint::tests::this_device`,
  `own_network`, `internet`); KI-Einstellungen sind Geräte-Einstellung, die der Einstellungs-Sync
  ablehnt (`ai::tests::ai_settings_are_a_device_setting_the_settings_sync_refuses`);
  Anfrage-Protokoll nur mit Metadaten (`ai::tests::the_request_log_pages_and_prunes`).

**Offen**
- Oberfläche: Einstellungen (aus bis eingeschaltet, Profile mit Schlüssel, Anbieter je Funktion,
  Cloud-Freigabe je Konto, Kennzeichnung „lokal“), Zustimmungsdialog vor der ersten Cloud-Anfrage,
  Anzeige je Anfrage (Mails, Größe, Empfänger), Protokoll-Ansicht, KI-Panel mit Karten, „Entwurf
  öffnen“, Fortschritt und Abbrechen, Schreibhilfe im Composer mit Vorschau, Hinweise auf offene
  Antworten als wegklickbare Vorschläge, Hinweis bei gekürzten Daten, Links aus Ausgaben über die
  Prüfung aus Stufe 3. In Arbeit im Arbeitsverzeichnis (`app/src/features/ai/`), nicht committet und
  von der Hauptsitzung nicht geprüft.
- Prüfsatz mit einem echten Modell (Anteil richtiger Fundstellen und Antworten); erst danach lässt
  sich entscheiden, ob Embeddings nötig sind.
- Die optionale Verfeinerung der Hinweise auf offene Antworten durch das Modell ist nicht gebaut
  (0024).
- Nächster Schritt: Oberfläche fertigstellen und committen, Vitest und Desktop-Test dafür, dann der
  Lauf mit Ollama.

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

**Wartet auf**
- Mensch: Ollama (oder einen anderen lokalen Server) einrichten; Prüfsatz mit echtem Modell laufen
  lassen (Befehl in `docs/testing/ai-search.md`, Test `qa_set_with_a_real_model`) und das Ergebnis
  dort eintragen; sobald die Oberfläche da ist, mit Ollama ohne Internet Zusammenfassung und
  Antwortvorschlag prüfen und vermerken; danach an echten Threads beurteilen, ob Zusammenfassungen,
  Vorschläge und Antworten brauchbar sind.
