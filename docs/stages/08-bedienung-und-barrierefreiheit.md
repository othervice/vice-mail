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
- [ ] **Fokus.** Sichtbarer Fokusring überall. Nach einer Aktion steht der Fokus auf der nächsten Mail. Dialoge und Panels geben den Fokus beim Schließen zurück.
- [x] **Screenreader.** Rollen und Beschriftungen für Liste, Zeilen, Schnellaktionen, Lesebereich und Composer. Die virtualisierte Liste meldet Position und Gesamtzahl. Statusmeldungen (gesendet, archiviert, Fehler) werden angesagt.
- [ ] **Darstellung.** Schriftgröße 80 bis 150 Prozent ohne abgeschnittene Inhalte. „Bewegung reduzieren“ schaltet Animationen ab. Kontofarben immer mit Namen.
- [ ] **Bewegung.** Die Bewegungs-Tokens konsequent anwenden; alles entfernen, was keinen Ortswechsel erklärt.
- [ ] **Zustände und Texte.** Alle leeren, ladenden und Fehlerzustände durchgehen. Texte in Englisch und Deutsch vollständig und einheitlich.
- [ ] **Automatische Prüfung.** axe-core in den Desktop-Tests für die Hauptansichten in beiden Themen.

## Abnahme

- [x] axe meldet keine schweren oder kritischen Befunde in Posteingang, Lesebereich, Composer, Einstellungen, jeweils hell und dunkel
- [x] Test: Jede Aktion des Verzeichnisses ist über die Command Palette auffindbar und ausführbar
- [x] Test: Ein geändertes Kürzel wirkt sofort, bleibt nach Neustart erhalten, eine Doppelbelegung wird abgelehnt
- [x] Desktop-Test: Der durchgehende Ablauf (Mail öffnen, antworten, senden, archivieren) gelingt ohne Maus
- [ ] alle Layouts sind bei 900×600 und 2560×1440 benutzbar (Bildschirmfotos im Ergebnis)
- [x] Checkliste für VoiceOver und Narrator unter `docs/testing/screenreader-checklist.md`
- [ ] Leistungswerte aus Stufe 7 neu gemessen, keine Verschlechterung
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- neue Mail-Funktionen
- Kategorien, Snooze, Benachrichtigungen (Stufe 9)

## Der Mensch prüft danach

- Einen Tag nur mit Tastatur arbeiten. Welche Kürzel fehlen oder stören?
- Screenreader-Checkliste auf beiden Systemen durchgehen, wenn gewünscht.

## Ergebnis

**Zwischenstand vom 23.11.2026, 09:30, Stufe in Arbeit.** Gebaut in 2152c42, b313c22, fcfae72,
e5e1519 (Kern: Oberflächen-Einstellungen, Layout je Fenster, Mail- und Composer-Fenster), 14e8dda,
04e2a88 und 24ac670 (Oberfläche dazu), e705d75 (Zustand des KI-Panels in den
Oberflächen-Einstellungen); Hauptsitzung mit Subagenten. Entscheidungen: 0025
(Oberflächen-Einstellungen und Mailfenster), 0028 (Punkt 11 bis 13). Geprüft von der
Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf 24ac670 (998 Rust-, 1.111
Vitest-Tests in 55 Dateien, mit Clippy für das Windows-Ziel) und auf dem Arbeitsstand, der als
e705d75 committet wurde (1.006 Rust-, 1.127 Vitest-Tests in 57 Dateien); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) grün auf 24ac670 (1.076
Tests); `pnpm --dir app test:e2e` grün auf 24ac670 mit den Testständen aus ab594b7: 16
Spec-Dateien, 89 Tests grün, 6 übersprungen, darunter die Specs dieser Stufe `keyboard.e2e.mjs` (4
von 4), `windows.e2e.mjs` (3 von 3) und `layouts.e2e.mjs` (7 grün, die 6 Fälle bei 2560×1440
übersprungen, weil der Bildschirm hier 1920×1080 hat). Der Windows-Prototyp
`prototype-2026-11-23-2` (gebaut aus 24ac670) enthält Stufe 8.

**Erledigt und nachgewiesen**
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
  Stalwart, grün auf 24ac670); zuletzt benutzte Befehle und Layouts bleiben auf dem Gerät.
- Ohne Maus (`keyboard.e2e.mjs`, echte App): Tab erreicht die Liste, Pos1 und Eingabe öffnen die
  neueste Mail, `r` antwortet mit dem Fokus im Text, Cmd+Enter sendet, Escape führt zurück, `e`
  archiviert und der Fokus steht auf der nächsten Mail, Cmd+K „Go to Archive“ zeigt die Mail im
  Archiv; auf dem Server ist die Antwort genau einmal gesendet und die Mail im Archiv.
- Layouts (`layout.test.tsx`): Lesebereich rechts, unten oder anstelle der Liste (Escape führt
  zurück, eine Aktion setzt den Fokus auf die nächste Mail), kompakte Zeilen, Schriftgröße 80 bis
  150 Prozent, aufgeklappte Seitenleiste bleibt, j/k wie die Pfeiltasten, Wechsel über die Palette,
  Wahl je Fenster gespeichert. Je Fenster im Kern:
  `ui_prefs::tests::window_layouts_are_stored_per_slot_on_this_device`, Shell
  `tests::window_layout_belongs_to_the_calling_window` (die Shell nimmt das aufrufende Fenster,
  nie ein Argument). In der echten App (`layouts.e2e.mjs`) bei 900×600 alle drei Positionen in
  beiden Dichten und 150 Prozent Schrift: nichts scrollt seitlich, Liste und Lesebereich liegen im
  Fenster, die Knöpfe der Symbolleiste im Lesebereich, nichts abgeschnitten. Die Bildschirmfotos
  (`layout-900x600-*.png`) und `layouts-report.json` entstehen beim Lauf unter
  `app/tests/e2e/screenshots/` (nicht im Repo, `.gitignore`).
- Mail und Composer im eigenen Fenster: Kern
  `ui_prefs::tests::a_message_window_needs_a_message_of_the_account`; Shell
  `tests::message_window_opens_once_with_the_main_windows_permissions` (einmal je Mail bzw.
  Entwurf, Rechte wie das Hauptfenster, keine für fremde Labels, Mail- und Composer-Fenster dürfen
  sich schließen, das Hauptfenster nicht; 04e2a88),
  `windows::tests::message_windows_have_one_label_per_message`,
  `composer_windows_have_one_label_and_page_per_draft`; Oberfläche `windows.test.tsx` (Fenster aus
  der Query, unbekannte Query erklärt, Mail ohne Leiste und Liste, verschwundene Mail, mehrere
  Entwürfe in eigenen Fenstern, Rückgängig im selben Fenster, Schließen nach Senden oder
  Verwerfen); axe ohne schwere Befunde in beiden reduzierten Fenstern (`a11y.test.tsx`); in der
  echten App (`windows.e2e.mjs`) „In neuem Fenster öffnen“ öffnet die Mail einmal ohne Leiste (ein
  zweiter Aufruf öffnet kein weiteres Fenster), zwei Entwürfe öffnen zwei Composer-Fenster; nach
  dem Verwerfen schließt sich jedes Fenster oder meldet, dass es fertig ist (der Test lässt beides
  zu).
- Screenreader: Die virtualisierte Liste meldet Position und Gesamtzahl (`aria-posinset`,
  `aria-setsize`; `message-list.test.tsx`, im Desktop-Test `account-sync.e2e.mjs` bis Position 9.959
  von 9.959); Statusmeldungen über `role="status"` (u. a. `status.test.tsx`, `actions.test.tsx`).
- axe-core (`a11y.test.tsx`): keine schweren oder kritischen Befunde in Posteingang, Lesebereich,
  Composer (auch mit allen Teilen der Stufe 10), Mail- und Composer-Fenster, allen
  Einstellungsabschnitten, Aktivität, Ausgang, Command Palette, Kürzelübersicht, Suchen und
  KI-Panel, jeweils hell und dunkel.
- Checkliste für VoiceOver und Narrator: `docs/testing/screenreader-checklist.md`.

**Offen**
- Fokus: Dass der Fokus nach einer Aktion auf der nächsten Mail steht und Palette und Dialoge ihn
  zurückgeben, ist nachgewiesen (siehe oben); ein sichtbarer Fokusring überall ist nicht geprüft
  (kein Test, keine dokumentierte Durchsicht).
- Darstellung: Schriftgröße 150 Prozent ist bei 900×600 in der echten App geprüft; „Bewegung
  reduzieren“ (Regeln für `prefers-reduced-motion` in `tokens.css` und `app.css`) und „Kontofarben
  immer mit Namen“ haben keinen Test über alle Ansichten.
- Bewegung (Tokens konsequent, nur Bewegungen, die einen Ortswechsel erklären) und Zustände und
  Texte (alle leeren, ladenden und Fehlerzustände durchgehen): keine Durchsicht mit Nachweis.
  Vollständige und gleiche Schlüssel in Englisch und Deutsch prüft `i18n.test.ts`.
- Automatische Prüfung: axe läuft nur in Vitest/jsdom, nicht in den Desktop-Tests.
- Leistungswerte aus Stufe 7 am 23.11. neu gemessen (`docs/testing/performance.md`, nur der Kern):
  die gefundene Verschlechterung beim Start ist behoben (0029); der Start braucht 4 statt 2 ms, weil
  das Schema auf 161 Objekte gewachsen ist (benannte Abweichung, Ziel 1 s weit eingehalten). Offen
  bleibt die Verlangsamung beim Scrollen in der WebView nach sehr vielen Zeilen (wird untersucht);
  deshalb ist der Abnahmepunkt noch nicht abgehakt.
- Nächster Schritt: axe in den Desktop-Tests für die Hauptansichten in beiden Themen, Durchsicht
  von Fokusring, Bewegung und Zuständen mit Nachweis, dann die Nachmessung eintragen.

**Abweichungen und Annahmen**
- axe läuft in Vitest/jsdom statt im Desktop-Test. jsdom rechnet keine Farben, deshalb ist die Regel
  `color-contrast` dort aus; Kontraste prüft `tests/contrast.test.ts` an allen Farbpaaren der Tokens
  in beiden Themen (4,5:1 und 3:1). Eingebettete HTML-Mails (Frame) prüft axe nicht.
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
- Scrollen nach vielen gerenderten Zeilen: Nachdem alle 9.959 Zeilen des Posteingangs zweimal
  gerendert worden waren, wurde späteres Scrollen langsam (längster Frame rund 250 ms statt 25 ms).
  Wird gerade untersucht. Scrollen während des Erstabgleichs im Desktop-Test: längster Frame 24 ms
  auf 24ac670 (Test grün).
- Layouts bei 2560×1440: der Testfall ist geschrieben, braucht aber einen Bildschirm mit dieser
  Größe.
- Der Zustand des KI-Panels (e705d75) ist nur in Vitest und im Kern geprüft, nicht im Desktop-Test.
- Windows zur Laufzeit: Strg-Kürzel, Mail- und Composer-Fenster unter WebView2.

**Wartet auf**
- Mensch: einen Tag nur mit Tastatur arbeiten (welche Kürzel fehlen oder stören?);
  Screenreader-Checkliste auf beiden Systemen, wenn gewünscht.
- Mensch (Bildschirm 2560×1440): `pnpm --dir app test:e2e` an einem Bildschirm dieser Größe laufen
  lassen (die sechs Fälle in `layouts.e2e.mjs`) und die Bildschirmfotos ansehen.
- Mensch (Windows): Prototyp `prototype-2026-11-23-2` (Gitea-Vorab-Release, gebaut aus 24ac670):
  Palette mit Strg+K, ein Kürzel ändern, eine Mail in eigenem Fenster öffnen (Schritte im
  Release-Hinweis; die Windows-Checkliste hat für Stufe 8 noch keinen Abschnitt).
