# Stufe 07 — Suche, Threads, mehrere Konten

**Ziel:** Schnelle Volltextsuche mit Filtern, Konversationen statt Einzelmails und ein vereinter
Posteingang über mehrere Konten. Die Leistungsziele aus `docs/SPEC.md` sind gemessen.

**Voraussetzung:** Stufe 05 fertig (Stufe 06 empfohlen). Ergebnis von Spike B gelesen.

## Aufgaben

- [x] **Index.** FTS5 über Betreff, Absender, Empfänger, Text und Anhangnamen, nach Spike B (`docs/decisions/0003-sqlite-schema-und-suche.md`): contentless, Spalten `subject, from, to, cc, body, attachments` (damit `from:` `to:` `cc:` `filename:` direkt aus FTS5 kommen), `unicode61 remove_diacritics 2`, Datumsfilter als rowid-Bereich, erste Treffer nach `rowid DESC`, `bm25` höchstens als Option, kein `optimize`. Teilwortsuche ist nicht Teil von Version 1. Aktualisierung bei jedem Einfügen, Ändern und Löschen. Text wird indiziert, sobald der Body lokal vorliegt; Kopfdaten immer. Neuaufbau im Hintergrund mit Austausch am Ende, ohne die Suche zu unterbrechen.
- [x] **Suchsprache.** Parser für freien Text und die Filter aus `docs/SPEC.md`, Phrasen, Ausschluss mit `-`, Datumsangaben in deutscher und englischer Schreibweise. Fehler werden an der Stelle angezeigt.
- [x] **Suche in der Oberfläche.** Treffer beim Tippen, Filter als Chips, hervorgehobene Fundstellen, letzte Suchen. Suchen speichern: sie erscheinen als Ansicht in der Leiste.
- [x] **Auf dem Server weitersuchen.** Schaltfläche für Mails, deren Inhalt nicht lokal vorliegt (IMAP SEARCH).
- [x] **Threads.** Je Konto über Message-ID, In-Reply-To und References; vorsichtiger Rückfall auf den Betreff wie in der Architektur. Tabelle `thread` wird laufend gepflegt. Zeile in der Liste mit Zähler; im Lesebereich ältere Mails eingeklappt, ungelesene offen. Konversationsansicht abschaltbar.
- [x] **Vereinter Posteingang.** Posteingänge aller Konten nach Zeit gemischt, Kontofarbe immer sichtbar. Selbst definierte Kontogruppen. Konten vorübergehend ausblenden. Umschalten über den Kopf der Liste.
- [x] **Identitäten.** Mehrere Absenderadressen je Konto, eine als Standard. Antworten wählt die Identität, an die die Mail ging.
- [x] **Leistungsmessung.** Erzeugte Postfächer mit 100.000 und 500.000 Mails über fünf Konten. Messen: Listenseite, Thread-Liste, erste Suchtreffer, Start bis zur benutzbaren Liste, Arbeitsspeicher im Leerlauf. Ergebnisse in `docs/testing/performance.md`; großzügige Schwellen als automatischer Test.

## Abnahme

- [x] Tabellentests für den Parser: jeder Filter, Kombinationen, fehlerhafte Eingaben
- [x] Tests für Threads mit vorbereiteten Fällen: Mailingliste, kaputte References, gleicher Betreff ohne Zusammenhang (darf nicht zusammengeführt werden)
- [x] Test: Der Index bleibt nach Einfügen, Ändern, Löschen und Neuaufbau mit der Nachrichtentabelle im Einklang
- [x] Test: Im vereinten Posteingang ist jede Mail ihrem Konto zugeordnet; ausgeblendete Konten fehlen
- [x] gemessen und eingetragen: Suche unter 200 ms bei 100.000 Mails, Wechsel zwischen lokalen Mails unter 100 ms; Abweichungen sind benannt
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Suche in natürlicher Sprache (Stufe 13)
- Threads über mehrere Konten hinweg
- Kategorien und Bündel (Stufe 9)

## Der Mensch prüft danach

- Mit den eigenen Konten suchen: Findet die Suche, was erwartet wird? Sind Threads richtig zusammengeführt?

## Ergebnis

Ausgeführt am 22./23.11.2026 (Hauptsitzung mit Subagenten: Suchsprache und Threads als Grundstein,
Store, Engine und Commands, Leistungsmessung, Oberfläche; letzte und gespeicherte Suchen in
24ac670). Geprüft am 23.11. von der Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün
auf 24ac670 (998 Rust-, 1.111 Vitest-Tests in 55 Dateien, mit Clippy für das Windows-Ziel, cargo
deny und pnpm audit); alle Servertests (`cargo test --workspace -- --include-ignored` gegen den
lokalen Stalwart) grün auf 24ac670 (1.076 Tests, darunter `bench_100k`, `bench_500k` und
`full_seed_threads_like_its_references`); `pnpm --dir app test:e2e` grün auf 24ac670 mit den
Testständen aus ab594b7 (16 Spec-Dateien, 89 Tests grün, 6 übersprungen, darunter
`search.e2e.mjs` mit 5 von 5 Tests). Entscheidungen: 0013 (Suchsprache und Threads), 0017
(Integration in Store und Engine), 0026 (gespeicherte und letzte Suchen). Messung:
`docs/testing/performance.md`.

**Nachgewiesen**
- Parser (`crates/mail-model`, in `check.sh`): Tabellentest `search::tests::parse_table` für jeden
  Filter, dazu `structured_filters`, `contradictions_are_marked`, `spans_point_into_the_input`
  (Fehler an der Stelle), `long_input_is_cut_and_reported`,
  `random_input_never_panics_and_never_injects`, `dates_become_rowid_ranges`, `fts_table`;
  Datumsangaben deutsch und englisch in `search::date::tests::date_forms`, `age_forms`,
  `slash_dates_follow_the_locale`.
- Threads (`threading::tests`): `mailing_list_threads_stay_apart` (Mailingliste),
  `broken_references` (kaputte References), `same_subject_without_relation_is_not_merged` (gleicher
  Betreff ohne Zusammenhang), `subject_fallback_stays_careful`,
  `outlook_replies_without_references_thread_by_subject`, `reply_arriving_before_its_original`,
  `threads_never_cross_accounts`, `the_long_seed_thread_in_any_order`; im Store
  `stage7_tests::threads_match_the_reference_grouping_and_follow_deletes`; am Seed `seed_threads.rs`
  (`full_seed_threads_like_its_references` braucht python3 und läuft im Serverlauf).
- Index im Einklang: `stage7_tests::the_index_follows_inserts_changes_deletes_and_a_rebuild`,
  `existing_messages_are_indexed_and_threaded_after_the_migration`,
  `search::tests::a_rebuild_runs_in_the_background_and_search_keeps_working`,
  `body_text_and_attachment_names_are_indexed_through_the_parser`; Abfragepläne
  `stage7_plan_tests::every_stage_7_query_uses_its_index`.
- Vereinter Posteingang:
  `stage7_tests::unified_inbox_assigns_every_mail_to_its_account_and_leaves_hidden_ones_out`,
  `search::tests::unified_inbox_groups_and_hidden_accounts_through_the_core`; Oberfläche
  (`unified-inbox.test.tsx`): Kontofarbe in jeder Zeile, Aktionen des eigenen Kontos, ausgeblendete
  Konten fehlen, Gruppen aus dem Kopf der Liste und aus der Seitenleiste.
- Auf dem Server weitersuchen:
  `search::server_tests::server_search_finds_what_is_not_stored_locally` (Stalwart, auch mit
  Umlauten), `search::tests::criteria_become_one_safe_line`.
- Identitäten: `stage7_tests::identities_have_one_default_and_unique_addresses`,
  `search::tests::identities_and_the_reply_identity`, `compose::tests::identity_choice_order`;
  Oberfläche `account-groups-identities.test.tsx`.
- Letzte und gespeicherte Suchen (neu in 24ac670, Migration 0012, in `check.sh` außer wo Stalwart
  genannt ist): Modell `saved_search::tests::names_are_cleaned_and_bounded`,
  `queries_are_trimmed_and_bounded`, `errors_have_their_own_codes`, `ids_are_uuids_on_the_wire`;
  Store `saved_searches::tests::saved_searches_keep_their_order_names_and_limit`,
  `recent_searches_move_up_and_keep_the_newest`,
  `saved_searches_are_captured_and_applied_recent_ones_are_not`; Kern
  `search::saved::tests::searches_are_saved_renamed_and_deleted_with_typed_errors`,
  `recent_searches_stay_few_newest_first_and_survive_a_restart`; Sync
  `settings_sync::record::tests::saved_views_are_shaped_like_saved_searches`; Shell
  `tests::saved_and_recent_searches_cross_the_bridge` (Fehlercodes auf der Leitung); zwei Geräte
  gegen Stalwart `tests/settings_sync.rs::saved_searches_reach_the_other_device_recent_ones_stay`
  (gespeicherte Suchen kommen an, letzte bleiben auf dem Gerät). Oberfläche
  (`saved-searches.test.tsx`): letzte Suchen unter dem leeren Feld, per Tastatur und Klick, die
  neueste oben, löschbar, gemerkt beim Absenden und beim Öffnen eines Treffers; „Suche speichern“
  unter einem Namen, vergebener Name und Grenze erklärt; gespeicherte Suchen als Ansicht in der
  Seitenleiste mit Treffern aus allen Konten, umbenennen, löschen nach Bestätigung, Änderungen
  eines anderen Geräts (`savedSearchesChanged`), leerer Zustand, deutsche Texte; axe ohne schwere
  Befunde (`a11y.test.tsx`).
- Leistung (`docs/testing/performance.md`, Release-Build, 100.000 und 500.000 erzeugte Mails über
  fünf Konten, gemessen über `AppCore`): erste Suchtreffer bei 100.000 Mails 0,24–4,3 ms im Median
  (schlechteste p95 4,5 ms), bei 500.000 bis 16,5 ms (p95 17,2 ms); Wechsel zwischen lokalen Mails
  0,36 ms Median, p95 0,47 ms, Maximum 0,72 ms; Listenseite 0,10 ms, Thread-Liste 0,26 ms, Start bis
  zur ersten Seite 2 ms, Arbeitsspeicher im Leerlauf 21 MB. Großzügige Schwellen als automatischer
  Test `search::bench::small_data_meets_generous_thresholds` (in `check.sh`); `bench_100k` und
  `bench_500k` liefen im Serverlauf auf 24ac670 (Debug-Build) grün mit.
- In der App (`search.e2e.mjs`, grün auf 24ac670): Treffer beim Tippen mit markierten Fundstellen,
  ein kaputtes Datum wird an seiner Stelle erklärt, Suche auf dem Server, Konversationen in einem
  normalen Ordner (nicht im sortierten Posteingang) öffnen, Identitäten in den Kontoeinstellungen.
- Oberfläche (Vitest `search.test.tsx`, `search-logic.test.ts`, `threads.test.tsx`): Cmd+F und `/`,
  Bereiche, Filter als Chips (Entfernen nimmt den Term aus der Anfrage), Problemstellen, leere und
  Fehlerzustände, Hinweis während des Indexaufbaus, Zeile je Konversation mit Zähler, Konversation
  öffnet die älteste ungelesene Mail, Konversationsansicht abschaltbar.

**Abweichungen und Annahmen**
- Hervorhebungen rechnet Rust (`highlight()` von FTS5 liefert bei einem contentless Index nichts);
  Treffer tief im Body haben keinen markierten Bereich.
- Höchstens 256 KiB Text je Mail im Index; der Text bleibt im Index, wenn der Body den Cache
  verlässt.
- Ungelesene je Thread werden beim Lesen gezählt, nicht gepflegt. Die Thread-Liste eines Postfachs
  sortiert nach der neuesten Mail in diesem Postfach.
- Konversationen gibt es in normalen Ordnern; der Posteingang ist seit Stufe 9 sortiert und zeigt
  sie nicht. *Nachtrag 23.11.2026:* Seit 0039 hat der sortierte Posteingang (auch vereint, nur
  ungelesene, ungelesene oben) eine Konversationsvariante; die Konversationsansicht ist eine
  synchronisierte Einstellung und standardmäßig an.
- Die Server-Suche liefert nur Mails ohne vollständigen lokalen Body, höchstens die neuesten 5.000
  Kandidaten; ohne Textbedingung wird nichts gesendet.
- Zeitzonen über `jiff` (lag schon im Lockfile). Der Abgleich schreibt je Fenster von 500 Kopfdaten
  4,7 ms länger (Threads und Index).
- Gespeicherte Suchen (0026) sind Name plus Anfrage und suchen immer in allen Konten; eingrenzen
  geht in der Anfrage (`account:`, `in:`). Die Anfrage wird als Text gespeichert, nicht kompiliert.
  Höchstens 100, Namen ohne Rücksicht auf Groß/Klein eindeutig; sie werden abgeglichen (Art
  `savedView`). Letzte Suchen (höchstens 10) bleiben auf dem Gerät und werden nie abgeglichen.
  Reihenfolge ändern ist nicht gebaut (die Position steht schon im Datensatz).

**Ungeprüft / offen**
- Leistung nur im Kern gemessen, nicht mit WebView und Rendern der Liste. Befund aus dem
  Desktop-Test: Nachdem alle 9.959 Zeilen des Posteingangs zweimal gerendert worden waren, wurde
  späteres Scrollen langsam (längster Frame rund 250 ms statt 25 ms); der Test misst das Scrollen
  vorher (auf 24ac670 längster Frame 24 ms während des Erstabgleichs). Der Befund wird gerade
  untersucht (Stufe 8/9). Die Nachmessung der Leistungswerte für Stufe 8 und 9 läuft (siehe
  `docs/testing/performance.md`).
- Gespeicherte und letzte Suchen nur in Vitest und im Kern geprüft; `search.e2e.mjs` enthält sie
  nicht.
- Kaltstart nach Neustart des Rechners, echte Postfächer, Windows.

**Wartet auf**
- Mensch: mit den eigenen Konten suchen. Findet die Suche, was erwartet wird? Sind Threads richtig
  zusammengeführt?
- Mensch (Windows): Prototyp `prototype-2026-11-23-2` (Gitea-Vorab-Release, gebaut aus 24ac670,
  mit letzten und gespeicherten Suchen) nach `docs/testing/windows-checklist.md`, Abschnitt 12
  (Suche und mehrere Konten).
