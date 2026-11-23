# Stufe 07 — Suche, Threads, mehrere Konten

**Ziel:** Schnelle Volltextsuche mit Filtern, Konversationen statt Einzelmails und ein vereinter
Posteingang über mehrere Konten. Die Leistungsziele aus `docs/SPEC.md` sind gemessen.

**Voraussetzung:** Stufe 05 fertig (Stufe 06 empfohlen). Ergebnis von Spike B gelesen.

## Aufgaben

- [x] **Index.** FTS5 über Betreff, Absender, Empfänger, Text und Anhangnamen, nach Spike B (`docs/decisions/0003-sqlite-schema-und-suche.md`): contentless, Spalten `subject, from, to, cc, body, attachments` (damit `from:` `to:` `cc:` `filename:` direkt aus FTS5 kommen), `unicode61 remove_diacritics 2`, Datumsfilter als rowid-Bereich, erste Treffer nach `rowid DESC`, `bm25` höchstens als Option, kein `optimize`. Teilwortsuche ist nicht Teil von Version 1. Aktualisierung bei jedem Einfügen, Ändern und Löschen. Text wird indiziert, sobald der Body lokal vorliegt; Kopfdaten immer. Neuaufbau im Hintergrund mit Austausch am Ende, ohne die Suche zu unterbrechen.
- [x] **Suchsprache.** Parser für freien Text und die Filter aus `docs/SPEC.md`, Phrasen, Ausschluss mit `-`, Datumsangaben in deutscher und englischer Schreibweise. Fehler werden an der Stelle angezeigt.
- [ ] **Suche in der Oberfläche.** Treffer beim Tippen, Filter als Chips, hervorgehobene Fundstellen, letzte Suchen. Suchen speichern: sie erscheinen als Ansicht in der Leiste.
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
Store, Engine und Commands, Leistungsmessung, Oberfläche). Geprüft am 23.11. von der Hauptsitzung in
sauberen Worktrees: `scripts/check.sh` grün auf 5d40e88 (878 Rust-, 780 Vitest-Tests) und auf
255ce74 (947 Rust-, 909 Vitest-Tests in 41 Dateien); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) grün auf f81ba9e (950
Tests); auf 255ce74 1.021 von 1.022 grün, rot nur `ai::tests::qa::qa_set_with_a_real_model`, der ohne lokales Modell scheiterte (seit 66ea375 überspringt er sich ohne Modell); `pnpm --dir app test:e2e` grün auf 5d40e88 mit den
Testständen aus 255ce74 (9 Spec-Dateien, 61 Tests, darunter `search.e2e.mjs`); auf 255ce74 58 von 61
Tests grün, die Spec dieser Stufe eingeschlossen; rot sind dort drei Tests in `reading.e2e.mjs`
(Stufe 3, Ursache offen). Entscheidungen: 0013 (Suchsprache und Threads), 0017 (Integration in Store
und Engine). Messung: `docs/testing/performance.md`.

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
  (`full_seed_threads_like_its_references` braucht python3 und lief nur im Serverlauf).
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
- Leistung (`docs/testing/performance.md`, Release-Build, 100.000 und 500.000 erzeugte Mails über
  fünf Konten, gemessen über `AppCore`): erste Suchtreffer bei 100.000 Mails 0,24–4,3 ms im Median
  (schlechteste p95 4,5 ms), bei 500.000 bis 16,5 ms (p95 17,2 ms); Wechsel zwischen lokalen Mails
  0,36 ms Median, p95 0,47 ms, Maximum 0,72 ms; Listenseite 0,10 ms, Thread-Liste 0,26 ms, Start bis
  zur ersten Seite 2 ms, Arbeitsspeicher im Leerlauf 21 MB. Großzügige Schwellen als automatischer
  Test `search::bench::small_data_meets_generous_thresholds` (in `check.sh`); `bench_100k` und
  `bench_500k` liefen in den Serverläufen auf f81ba9e und 255ce74 (Debug-Build) grün mit.
- In der App (`search.e2e.mjs`): Treffer beim Tippen mit markierten Fundstellen, ein kaputtes Datum
  wird an seiner Stelle erklärt, Suche auf dem Server, Konversationen in einem normalen Ordner
  (nicht im sortierten Posteingang) öffnen, Identitäten in den Kontoeinstellungen.
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
  sie nicht.
- Die Server-Suche liefert nur Mails ohne vollständigen lokalen Body, höchstens die neuesten 5.000
  Kandidaten; ohne Textbedingung wird nichts gesendet.
- Zeitzonen über `jiff` (lag schon im Lockfile). Der Abgleich schreibt je Fenster von 500 Kopfdaten
  4,7 ms länger (Threads und Index).

**Ungeprüft / offen**
- „Letzte Suchen“ und „Suchen speichern“ (als Ansicht in der Leiste) sind nicht gebaut; im Kern
  fehlt die Tabelle `saved_view` (IPC-Vertrag der Stufe). Damit ist die Aufgabe „Suche in der
  Oberfläche“ offen.
- Leistung nur im Kern gemessen, nicht mit WebView und Rendern der Liste. Befund aus dem
  Desktop-Test: Nachdem alle 9.959 Zeilen des Posteingangs zweimal gerendert worden waren, wurde
  späteres Scrollen langsam (längster Frame rund 250 ms statt 25 ms); der Test misst das Scrollen
  jetzt vorher. Offener Leistungspunkt für Stufe 8/9.
- Kaltstart nach Neustart des Rechners, echte Postfächer, Windows.

**Wartet auf**
- Mensch: mit den eigenen Konten suchen. Findet die Suche, was erwartet wird? Sind Threads richtig
  zusammengeführt?
- Mensch (Windows): Der Prototyp `prototype-2026-11-23` enthält Stufe 7; die Windows-Checkliste hat
  dafür noch keinen Abschnitt.
