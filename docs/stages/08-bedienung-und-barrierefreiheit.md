# Stufe 08 — Bedienung und Barrierefreiheit

**Ziel:** Die App ist vollständig per Tastatur bedienbar, hat eine Command Palette, mehrere Layouts
und erfüllt die Barrierefreiheits-Kriterien aus `docs/SPEC.md`.

**Voraussetzung:** Stufe 07 fertig. `docs/DESIGN.md` gelesen, dazu die Reibungspunkte, die der
Mensch seit Stufe 5 gesammelt hat (Ergebnis-Abschnitte der Stufen 05 bis 07).

## Aufgaben

- [x] **Aktionsverzeichnis.** Eine zentrale Liste aller Aktionen: Kennung, Beschriftung, Standardkürzel je Plattform, Ausführung, Bedingung für „verfügbar“. Menüs, Kürzel, Schnellaktionen und Command Palette speisen sich alle daraus.
- [x] **Tastenkürzel.** Belegung nach verbreiteten Mustern (j/k oder Pfeile für nächste/vorherige, Eingabe öffnet, Esc schließt, e archiviert, # löscht, r antwortet, c schreibt neu, / sucht). In den Einstellungen änderbar, mit Erkennung von Doppelbelegungen. Keine Kürzel, die das System beansprucht. Regel für Tastaturlayouts festlegen und dokumentieren (Zeichen über `event.key`, Positionen über `event.code`). Übersicht aller Kürzel mit `?`.
- [x] **Command Palette.** Cmd+K / Ctrl+K. Enthält alle Aktionen aus dem Verzeichnis, Sprungziele (Ordner, Konten, Ansichten, Einstellungen) und die Übergabe an die Suche. Zuletzt benutzte Befehle oben. Vollständig per Tastatur.
- [x] **Layouts.** Lesebereich rechts (Standard), unten, anstelle der Liste, in eigenem Fenster. Dichte kompakt und komfortabel. Icon-Leiste oder aufgeklappte Seitenleiste. Sinnvolles Verhalten von 900×600 bis 2560×1440. Je Fenster gespeichert.
- [x] **Fokus.** Sichtbarer Fokusring überall. Nach einer Aktion steht der Fokus auf der nächsten Mail. Dialoge und Panels geben den Fokus beim Schließen zurück.
- [x] **Screenreader.** Rollen und Beschriftungen für Liste, Zeilen, Schnellaktionen, Lesebereich und Composer. Die virtualisierte Liste meldet Position und Gesamtzahl. Statusmeldungen (gesendet, archiviert, Fehler) werden angesagt.
- [x] **Darstellung.** Schriftgröße 80 bis 150 Prozent ohne abgeschnittene Inhalte. „Bewegung reduzieren“ schaltet Animationen ab. Kontofarben immer mit Namen.
- [x] **Bewegung.** Die Bewegungs-Tokens konsequent anwenden; alles entfernen, was keinen Ortswechsel erklärt.
- [x] **Zustände und Texte.** Alle leeren, ladenden und Fehlerzustände durchgehen. Texte in Englisch und Deutsch vollständig und einheitlich.
- [x] **Automatische Prüfung.** axe-core in den Desktop-Tests für die Hauptansichten in beiden Themen.

## Abnahme

- [x] axe meldet keine schweren oder kritischen Befunde in Posteingang, Lesebereich, Composer, Einstellungen, jeweils hell und dunkel
- [x] Test: Jede Aktion des Verzeichnisses ist über die Command Palette auffindbar und ausführbar
- [x] Test: Ein geändertes Kürzel wirkt sofort, bleibt nach Neustart erhalten, eine Doppelbelegung wird abgelehnt
- [x] Desktop-Test: Der durchgehende Ablauf (Mail öffnen, antworten, senden, archivieren) gelingt ohne Maus
- [ ] alle Layouts sind bei 900×600 und 2560×1440 benutzbar (Bildschirmfotos im Ergebnis)
- [x] Checkliste für VoiceOver und Narrator unter `docs/testing/screenreader-checklist.md`
- [x] Leistungswerte aus Stufe 7 neu gemessen, keine Verschlechterung
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- neue Mail-Funktionen
- Kategorien, Snooze, Benachrichtigungen (Stufe 9)

## Der Mensch prüft danach

- Einen Tag nur mit Tastatur arbeiten. Welche Kürzel fehlen oder stören?
- Screenreader-Checkliste auf beiden Systemen durchgehen, wenn gewünscht.

## Ergebnis

**Stand vom 23.11.2026, 13:30.** Ausgeführt am 22./23.11.2026 (Hauptsitzung mit Subagenten).
Gebaut in c7fd173, b73f180, 415667b, 9c8ed68 (Kern: Oberflächen-Einstellungen, Layout je Fenster,
Mail- und Composer-Fenster), e7d7a5c, f19e8c9 und e402edc (Oberfläche dazu), 582bce8 (Zustand des
KI-Panels), 0956418 (Fokusring, „Bewegung reduzieren“, Kontonamen, Zustände), 22ac22a (axe in der
echten WebView, Diagnose der Scroll-Hänger), 874d7c5 (Nachmessung und Behebung), dfa4377 (Liste
nach 0031), 0780273 und e2bb12f (Namen von Ordnern und Zählern der Leiste enthalten den sichtbaren
Text). Entscheidungen: 0025 (Oberflächen-Einstellungen und Mailfenster), 0028 (Punkt 11 bis 13),
0029 (Start ohne breites Lesen), 0030 („Bewegung reduzieren“ nur über das System), 0031
(Feinschliff, darin die axe-Befunde der Liste). Geprüft von der Hauptsitzung in sauberen
Worktrees: `scripts/check.sh` grün auf 3ad835d (1.102 Rust-, 3.860 Vitest-Tests in 75 Dateien, mit
Clippy für das Windows-Ziel, cargo deny und pnpm audit); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) auf 3ad835d: 1.185 von
1.186 grün, der eine Fehler gehört zu Stufe 16 (zwei Servertests teilten sich bobs Konto, behoben in
13020c1); `pnpm --dir app test:e2e` grün auf b3a5f92 mit den Änderungen aus 0780273 (17
Spec-Dateien, 99 Tests grün, 6 übersprungen, darunter `keyboard.e2e.mjs` 4 von 4,
`windows.e2e.mjs` 3 von 3, `a11y.e2e.mjs` 10 von 10, `layouts.e2e.mjs` 7 grün und die 6 Fälle bei
2560×1440 übersprungen, weil der Bildschirm hier 1920×1080 hat). Desktop-Lauf auf dem neuesten
Stand: grün auf 6a2724b mit den Testständen aus 7afaf44 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`). Prototypen: `prototype-2026-11-23-3` (b3a5f92, macOS) und
`prototype-2026-11-23-3.1` (34c4132, Windows) enthalten Stufe 8 ohne 0780273 und e2bb12f;
`prototype-2026-11-23-2` (e402edc) den Stand vor 0956418; `prototype-2026-11-23-4` (7afaf44, Windows
und macOS) enthält den vollständigen Stand dieser Stufe.

**Nachgewiesen**
- Aktionsverzeichnis (`app/src/features/actions/registry.ts`): Beschriftung in beiden Sprachen,
  keine Doppelbelegung je Plattform, Bedingung für „verfügbar“ mit Grund
  (`action-registry.test.ts`). Kürzel, Schnellaktionen, Symbolleiste und Command Palette lesen
  daraus.
- Command Palette (`command-palette.test.tsx`): jede Aktion des Verzeichnisses ist per Tastatur
  auffindbar und ausführbar; nicht ausführbare erscheinen mit Grund und laufen nicht; Sprungziele
  (Ansichten, Ordner, Konten, Einstellungen), Übergabe an die Suche, zuletzt benutzte oben, Combobox
  mit Listbox, Escape gibt den Fokus zurück, Tab bleibt im Dialog, Cmd+1 bis 8.
- Kürzel: Eine Änderung wirkt sofort, auch im vereinten Posteingang und in der Palette; abgelehnt
  werden Doppelbelegungen (mit Namen des anderen Befehls), Systemkürzel und die Tasten zum Bewegen;
  Standardbelegung ohne Konflikte auf macOS und Windows; Übersicht mit `?`
  (`shortcut-settings.test.tsx`, `keyboard-bindings.test.ts`). Regel für Tastaturlayouts
  (`event.key`, nicht `event.code`) in `docs/ARCHITECTURE.md`, Abschnitt Tastaturkürzel.
- Abnahme „geändertes Kürzel wirkt sofort, bleibt nach Neustart, Doppelbelegung abgelehnt“: Kern
  `ui_prefs::tests::set_ui_preferences_stores_returns_and_emits` (gespeichert, Ereignis
  `uiPreferencesChanged`, nach einem Neustart auf derselben Datenbank wieder da),
  `shortcuts_are_stored_compactly_per_platform`,
  `invalid_ui_preferences_are_refused_and_nothing_changes`,
  `unreadable_stored_values_fall_back_to_defaults`; Shell
  `tests::ui_preference_commands_store_validate_and_emit`; Oberfläche „keeps the change after a
  restart“ (die Oberfläche startet neu und lädt das Kürzel aus dem Kern, `e` archiviert nicht mehr,
  `y` schon) und `ui-preferences.test.tsx` (Laden, Speichern mit Rücknahme bei Ablehnung, Ereignis
  aus einem anderen Fenster, einmalige Übernahme aus dem alten Web-Storage); in der echten App
  `windows.e2e.mjs`: ein in den Einstellungen geändertes Kürzel liegt im Kern
  (`get_ui_preferences`), der Web-Storage hat keinen Schlüssel `vice-mail.ui.shortcuts` mehr.
  Kürzel und Schriftgröße gehen über den Einstellungs-Sync
  (`tests/settings_sync.rs::shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay`,
  Stalwart, grün auf e402edc und 3ad835d); zuletzt benutzte Befehle und Layouts bleiben auf dem
  Gerät.
- Ohne Maus (`keyboard.e2e.mjs`, echte App): Tab erreicht die Liste, Pos1 und Eingabe öffnen die
  neueste Mail, `r` antwortet mit dem Fokus im Text, Cmd+Enter sendet, Escape führt zurück, `e`
  archiviert und der Fokus steht auf der nächsten Mail, Cmd+K „Go to Archive“ zeigt die Mail im
  Archiv; auf dem Server ist die Antwort genau einmal gesendet und die Mail im Archiv. Grün auf
  e402edc und b3a5f92. Auf 3ad835d rot: „Tab never reached the message list“ (nach 40
  Tastendrücken), die drei folgenden Fälle bauen darauf auf. Neuester Stand: grün auf 6a2724b mit den Testständen aus 7afaf44 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`).
- Layouts (`layout.test.tsx`): Lesebereich rechts, unten oder anstelle der Liste (Escape führt
  zurück, eine Aktion setzt den Fokus auf die nächste Mail), kompakte Zeilen, Schriftgröße 80 bis
  150 Prozent, aufgeklappte Seitenleiste bleibt, j/k wie die Pfeiltasten, Wechsel über die Palette,
  Wahl je Fenster gespeichert. Je Fenster im Kern:
  `ui_prefs::tests::window_layouts_are_stored_per_slot_on_this_device`, Shell
  `tests::window_layout_belongs_to_the_calling_window` (die Shell nimmt das aufrufende Fenster,
  nie ein Argument). In der echten App (`layouts.e2e.mjs`) bei 900×600 alle drei Positionen in
  beiden Dichten und 150 Prozent Schrift: nichts scrollt seitlich, Liste und Lesebereich liegen im
  Fenster, die Knöpfe der Symbolleiste im Lesebereich, nichts abgeschnitten; grün auf e402edc,
  b3a5f92 und 3ad835d (je 7 Fälle), neuester Stand: grün auf 6a2724b mit den Testständen aus 7afaf44 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`). Die Bildschirmfotos
  (`layout-900x600-*.png`) und `layouts-report.json` entstehen beim Lauf unter
  `app/tests/e2e/screenshots/` (nicht im Repo, `.gitignore`).
- Mail und Composer im eigenen Fenster: Kern
  `ui_prefs::tests::a_message_window_needs_a_message_of_the_account`; Shell
  `tests::message_window_opens_once_with_the_main_windows_permissions` (einmal je Mail bzw.
  Entwurf, Rechte wie das Hauptfenster, keine für fremde Labels, Mail- und Composer-Fenster dürfen
  sich schließen, das Hauptfenster nicht; f19e8c9),
  `windows::tests::message_windows_have_one_label_per_message`,
  `composer_windows_have_one_label_and_page_per_draft`; Oberfläche `windows.test.tsx` (Fenster aus
  der Query, unbekannte Query erklärt, Mail ohne Leiste und Liste, verschwundene Mail, mehrere
  Entwürfe in eigenen Fenstern, Rückgängig im selben Fenster, Schließen nach Senden oder
  Verwerfen); axe ohne schwere Befunde in beiden reduzierten Fenstern (`a11y.test.tsx`); in der
  echten App (`windows.e2e.mjs`, grün auf e402edc, b3a5f92 und 3ad835d) „In neuem Fenster öffnen“
  öffnet die Mail einmal ohne Leiste (ein zweiter Aufruf öffnet kein weiteres Fenster), zwei
  Entwürfe öffnen zwei Composer-Fenster; nach dem Verwerfen schließt sich jedes Fenster oder
  meldet, dass es fertig ist (der Test lässt beides zu).
- Fokus (0956418): `focus-ring.test.tsx` fokussiert in jeder Hauptansicht jedes per Tastatur
  erreichbare Element nacheinander (Posteingang mit Leiste, Kopf, Suche, Liste und Cursor-Zeile;
  Lesebereich und Symbolleiste; Textmail mit Links, Anhängen und Linkprüfung; Composer mit
  Empfängern, Betreff, Leiste und beiden Editoren; Palette und Kürzelübersicht; jeder
  Einstellungsabschnitt; Aktivität, Ausgang und aufgeklappte Seitenleiste; KI-Panel mit Karten) und
  verlangt am kompilierten Stylesheet einen durchgehenden Rahmen von mindestens 2 px im Token
  `focus-ring`, am Element oder an einem umgebenden Element. Gefunden und behoben: Mit Tailwind v4
  zeichnete `outline-none` mit `focus-visible:outline-2` keinen Ring (Lesebereich, `.eml`-Fenster,
  Rückfrage bei fehlendem Ordner). Der Ring hat mindestens 3:1 auf allen Flächen, hell und dunkel,
  in jedem Farbthema (`contrast.test.ts`). Fokus auf der nächsten Mail nach einer Aktion und Rückgabe
  aus Palette und Dialogen siehe oben (`layout.test.tsx`, `command-palette.test.tsx`,
  `keyboard.e2e.mjs`); die Anhangsvorschau der Stufe 16 gibt ihn an den öffnenden Knopf zurück
  (`attachments-stage16.test.tsx`).
- Darstellung: Schriftgröße 80 bis 150 Prozent in `layout.test.tsx`, 150 Prozent bei 900×600 in der
  echten App (`layouts.e2e.mjs`, siehe oben). „Bewegung reduzieren“ folgt der Systemeinstellung
  (0030): `motion.test.tsx` prüft, dass bei `prefers-reduced-motion` jede Transition und Animation
  der gerenderten Seite 0 ms dauert. Kontofarben nie ohne Namen (`account-colours.test.tsx`, 0956418):
  in jeder Zeile des vereinten Posteingangs (sichtbar und im Namen der Zeile) und in Suchergebnissen
  über mehrere Konten; in Ordneransicht, Leiste, Seitenleiste, Aktivität, Ausgang und Einstellungen;
  ohne Namen nur, solange ein einziges Konto gezeigt wird.
- Bewegung: `motion.test.tsx` legt die Token fest (120/180/240 ms, weiches Ausklingen, 0 ms bei
  reduzierter Bewegung) und scheitert an jeder Bewegungsklasse außer `transition-colors`, an
  `@keyframes`, weichem Scrollen und Animation per Skript. Die einzige Bewegung der Oberfläche ist
  damit der kurze Farbübergang; der abgedunkelte Hintergrund des Gestaltungsdurchgangs erscheint
  ohne Bewegung (`docs/DESIGN.md`).
- Zustände und Texte (0956418): leere, ladende und Fehlerzustände durchgesehen; was fehlte, ist
  ergänzt und in `states.test.tsx` geprüft (KI-Panel und Einstellungs-Sync laden ihre Daten nach
  einem Fehler auf Wunsch neu, Composer meldet nicht ladbare Absender und Signaturen mit
  Wiederholen, Aktivität und aufgeklappte Seitenleiste ohne Konto führen zum Hinzufügen, die neuen
  Texte auf Deutsch mit „du“). Gleiche und nicht leere Schlüssel in Englisch und Deutsch:
  `i18n.test.ts`.
- Screenreader: Die virtualisierte Liste meldet Position und Gesamtzahl (`aria-posinset`,
  `aria-setsize`; `message-list.test.tsx`). Seit 0031 sind Bündel und „Alle anzeigen“ Einträge der
  Liste und zählen mit; im Desktop-Test `account-sync.e2e.mjs` bis Position 10.053 von 10.053
  (9.959 Mails und 94 Bündeleinträge, so viele Mails wie auf dem Server; grün auf b3a5f92 und
  3ad835d). Der Name einer Zeile kommt aus ihrem sichtbaren Inhalt plus den Zuständen, die sie nur
  als Symbol zeigt (0031). Statusmeldungen über `role="status"` (u. a. `status.test.tsx`,
  `actions.test.tsx`).
- axe-core in Vitest (`a11y.test.tsx`): keine schweren oder kritischen Befunde in Posteingang,
  Lesebereich, Composer (auch mit allen Teilen der Stufe 10), Mail- und Composer-Fenster, allen
  Einstellungsabschnitten, Aktivität, Ausgang, Command Palette, Kürzelübersicht, Suchen und
  KI-Panel, jeweils hell und dunkel.
- axe-core in der echten WebView (`a11y.e2e.mjs`, seit 22ac22a im Lauf): axe 4.14.0 unter der
  unveränderten CSP mit allen Regeln, auch `color-contrast` an den gerenderten Farben; Posteingang,
  Lesebereich mit der neuesten Mail, Composer (neue Mail und Rückfrage vor dem Verwerfen), jeder
  Einstellungsabschnitt und Command Palette, jeweils hell und dunkel: keine schweren oder
  kritischen Befunde, 10 von 10 grün auf b3a5f92 (mit 0780273) und auf 3ad835d (Akzent Orange,
  0038). Neuester Stand: grün auf 6a2724b mit den Testständen aus 7afaf44 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`). Über diesen Test gefunden und behoben: Buttons in der Listbox
  (`aria-required-children`) und Zeilennamen ohne ihren sichtbaren Text
  (`label-content-name-mismatch`, beide 0031), der Knopf „Name bearbeiten“ der Kontakte (22ac22a),
  der Ordnername mit Zähler (0780273), die Zähler von Aktivität und Ausgang in der Leiste (e2bb12f).
- Checkliste für VoiceOver und Narrator: `docs/testing/screenreader-checklist.md`.
- Leistung (`docs/testing/performance.md`, „Stufe 8/9 – Nachmessung 23.11.2026“, Release-Build,
  nur der Kern): Die Nachmessung fand eine Verschlechterung seit Stufe 7 (Start 13 statt 2 ms,
  Leerlauf 98 statt 21 MB bei 100.000 Mails), Ursache und Behebung in 0029 (874d7c5). Danach bei 100.000 /
  500.000 Mails: Start bis zur ersten Seite 5 / 5 ms (Kern nach 4 ms gestartet), Leerlauf 25 / 25
  MB, erste Suchtreffer 0,24–5,27 / 0,25–21,0 ms im Median, Wechsel zwischen lokalen Mails 0,39 /
  0,37 ms; alle Ziele aus `docs/SPEC.md` eingehalten. Tests der Behebung (in `check.sh`):
  `stage9_plan_tests::stale_statistics_of_the_unclassified_index_do_not_cause_a_scan`,
  `stage9_tests::new_locations_take_the_category_without_the_trigger`,
  `comfort::tests::the_core_classifies_older_messages_after_its_start`,
  `search::tests::an_index_job_found_at_start_runs_after_the_start`; `bench_100k` und `bench_500k`
  liefen im Serverlauf auf 3ad835d (Debug-Build) grün mit.
- Scrollen im Desktop-Test: Die Verlangsamung nach vielen gerenderten Zeilen kommt von WebKit im
  nie sichtbaren Testfenster, nicht von der App. `diag-stall.e2e.mjs` (nicht im Lauf, 22ac22a)
  misst in diesem Fenster (`visibilityState` „hidden“) auf eigenen Elementen ohne App-Code: frisch
  nach dem Start längste Lücke 0 ms beim Scrollen von 2.000 festen Zeilen und 12 ms beim Austausch
  von 30 Zeilen je Frame; nach 40 s Leerlauf 90 bzw. 198 ms beim Scrollen und 210 ms beim
  Austausch, während reine Rechen- und Speicherschleifen ohne Lücke laufen (1 bis 3 ms). Der
  Hauptthread wird also nicht angehalten; die Lücken entstehen in der Arbeit von WebKit zwischen
  zwei Schritten (Scrollen, Layout). Scrollen während des Erstabgleichs
  (`account-sync.e2e.mjs`, gemessen vor dem Rendern aller Zeilen): längster Frame 37 ms, Median
  17 ms, grün auf b3a5f92.

**Abweichungen und Annahmen**
- axe in Vitest/jsdom rechnet keine Farben, deshalb ist dort `color-contrast` aus; Kontraste prüft
  `tests/contrast.test.ts` an allen Farbpaaren der Tokens in beiden Modi und jedem Farbthema (4,5:1
  und 3:1). In der echten WebView laufen alle Regeln; der Test bringt Farbübergänge vorher selbst
  zu Ende, weil das verborgene Testfenster keine Animationsframes ausführt (sonst falsche
  Kontrastbefunde im dunklen Modus). Eingebettete HTML-Mails (Frame) prüft axe nicht: Sie sind
  Inhalt des Absenders, nicht Oberfläche der App.
- Den Fokusring prüft `focus-ring.test.tsx` am kompilierten Stylesheet in jsdom. jsdom erkennt
  `:focus-visible` bei jedem Fokus, die Engines nur nach Tastaturbedienung; ob ein Ring
  abgeschnitten, verdeckt oder außerhalb des Fensters gezeichnet wird, sieht der Test nicht.
- „Bewegung reduzieren“ hat keinen eigenen Schalter in der App (0030); es gibt nur Farbübergänge.
- Zustände: Die Durchsicht ist nicht als Liste aller Zustände festgehalten; getestet sind die
  gefundenen Lücken.
- Leistung, abgehakt mit benannten Zuwächsen gegenüber den Messwerten der Stufe 7 (alle weit
  innerhalb der Ziele): Start 2 → 4 ms (vier Verbindungen lesen das auf 161 Objekte gewachsene
  Schema), Listenseite 0,10 → 0,18 ms, vereinter Posteingang 0,47 → 0,71 ms, Schreiben je Fenster
  von 500 Kopfdaten 10,3 → 14,3 ms (Index `loc_category` und zwei Teilindizes der Stufe 9),
  Leerlauf 21 → 25 MB.
- Kürzel, Schriftgröße, zuletzt benutzte Befehle und Layouts liegen seit 0025 im Kern (Tabelle
  `setting`). Der frühere Ersatzspeicher im Web-Storage (`localFallback.ts`) dient nur noch der
  einmaligen Übernahme (`migrateLocal.ts`); danach wird er gelöscht.
- Layout wird je Art von Fenster gespeichert (Hauptfenster, Mail-Fenster, Composer-Fenster), nicht
  je einzelnem Fenster; Größe und Lage der Mail- und Composer-Fenster werden nicht gespeichert.
- Der eingebettete WebDriver führt Tab nicht selbst aus; `keyboard.e2e.mjs` sendet den Tastendruck
  und bewegt den Fokus wie der Browser (`pressTab` in `support/app.mjs`). Er lässt außerdem den
  Fokus auf angeklickten Knöpfen; die Tests berücksichtigen das (ab594b7).
- Eine eigene Menüleiste gibt es nicht; „Menüs“ sind die Schnellaktionen, die Symbolleiste und die
  Palette.

**Ungeprüft / offen**
- Desktop-Lauf auf 3ad835d (12:58–13:09): 14 von 20 Spec-Dateien grün; rot waren
  `keyboard.e2e.mjs` (siehe oben), `account-sync.e2e.mjs` (Scrollen während des Erstabgleichs:
  längster Frame 53 ms, Grenze 50 ms), `frame.e2e.mjs` (Liste der Leiste), `reading.e2e.mjs`
  (Öffnen eines Anhangs), `actions.e2e.mjs` (ein Fall) und `find-in-message.e2e.mjs` (Stufe 16).
  Ergebnis auf dem neuesten Stand: grün auf 6a2724b mit den Testständen aus 7afaf44 (21 Spec-Dateien, 115 Tests, 6 Fälle bei 2560×1440 übersprungen; Log `r6-full2.log`).
- Scrollen nach vielen gerenderten Zeilen in einem sichtbaren Fenster ist nicht gemessen; der
  Desktop-Test kann es im verborgenen Fenster nicht.
- Fokusring in der echten WebView (WKWebView, WebView2) und im Mail-Frame; Schriftgröße 80 Prozent
  nur in Vitest.
- Layouts bei 2560×1440: der Testfall ist geschrieben, braucht aber einen Bildschirm mit dieser
  Größe.
- Der Zustand des KI-Panels (582bce8) ist nur in Vitest und im Kern geprüft, nicht im Desktop-Test.
- VoiceOver und Narrator: die Checkliste ist nie durchlaufen worden.
- Windows zur Laufzeit: Strg-Kürzel, Mail- und Composer-Fenster unter WebView2, Fokusring und
  „Animationseffekte“ von Windows.

**Wartet auf**
- Mensch: einen Tag nur mit Tastatur arbeiten (welche Kürzel fehlen oder stören?);
  Screenreader-Checkliste mit VoiceOver und Narrator, wenn gewünscht.
- Mensch (Bildschirm 2560×1440): `pnpm --dir app test:e2e` an einem Bildschirm dieser Größe laufen
  lassen (die sechs Fälle in `layouts.e2e.mjs`) und die Bildschirmfotos ansehen.
- Mensch (Windows): Prototyp `prototype-2026-11-23-3.1` (Gitea-Vorab-Release, 34c4132) nach
  `docs/testing/windows-checklist.md`, Abschnitt 15 (Bedienung): Palette mit Strg+K, ein Kürzel
  ändern, eine Mail in eigenem Fenster öffnen; dazu sehen, ob der Fokusring unter WebView2 überall
  erscheint und ob „Animationseffekte“ aus die Farbübergänge abschaltet.
