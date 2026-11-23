# Stufe 08 — Bedienung und Barrierefreiheit

**Ziel:** Die App ist vollständig per Tastatur bedienbar, hat eine Command Palette, mehrere Layouts
und erfüllt die Barrierefreiheits-Kriterien aus `docs/SPEC.md`.

**Voraussetzung:** Stufe 07 fertig. `docs/DESIGN.md` gelesen, dazu die Reibungspunkte, die der
Mensch seit Stufe 5 gesammelt hat (Ergebnis-Abschnitte der Stufen 05 bis 07).

## Aufgaben

- [x] **Aktionsverzeichnis.** Eine zentrale Liste aller Aktionen: Kennung, Beschriftung, Standardkürzel je Plattform, Ausführung, Bedingung für „verfügbar“. Menüs, Kürzel, Schnellaktionen und Command Palette speisen sich alle daraus.
- [x] **Tastenkürzel.** Belegung nach verbreiteten Mustern (j/k oder Pfeile für nächste/vorherige, Eingabe öffnet, Esc schließt, e archiviert, # löscht, r antwortet, c schreibt neu, / sucht). In den Einstellungen änderbar, mit Erkennung von Doppelbelegungen. Keine Kürzel, die das System beansprucht. Regel für Tastaturlayouts festlegen und dokumentieren (Zeichen über `event.key`, Positionen über `event.code`). Übersicht aller Kürzel mit `?`.
- [x] **Command Palette.** Cmd+K / Ctrl+K. Enthält alle Aktionen aus dem Verzeichnis, Sprungziele (Ordner, Konten, Ansichten, Einstellungen) und die Übergabe an die Suche. Zuletzt benutzte Befehle oben. Vollständig per Tastatur.
- [ ] **Layouts.** Lesebereich rechts (Standard), unten, anstelle der Liste, in eigenem Fenster. Dichte kompakt und komfortabel. Icon-Leiste oder aufgeklappte Seitenleiste. Sinnvolles Verhalten von 900×600 bis 2560×1440. Je Fenster gespeichert.
- [ ] **Fokus.** Sichtbarer Fokusring überall. Nach einer Aktion steht der Fokus auf der nächsten Mail. Dialoge und Panels geben den Fokus beim Schließen zurück.
- [x] **Screenreader.** Rollen und Beschriftungen für Liste, Zeilen, Schnellaktionen, Lesebereich und Composer. Die virtualisierte Liste meldet Position und Gesamtzahl. Statusmeldungen (gesendet, archiviert, Fehler) werden angesagt.
- [ ] **Darstellung.** Schriftgröße 80 bis 150 Prozent ohne abgeschnittene Inhalte. „Bewegung reduzieren“ schaltet Animationen ab. Kontofarben immer mit Namen.
- [ ] **Bewegung.** Die Bewegungs-Tokens konsequent anwenden; alles entfernen, was keinen Ortswechsel erklärt.
- [ ] **Zustände und Texte.** Alle leeren, ladenden und Fehlerzustände durchgehen. Texte in Englisch und Deutsch vollständig und einheitlich.
- [ ] **Automatische Prüfung.** axe-core in den Desktop-Tests für die Hauptansichten in beiden Themen.

## Abnahme

- [x] axe meldet keine schweren oder kritischen Befunde in Posteingang, Lesebereich, Composer, Einstellungen, jeweils hell und dunkel
- [x] Test: Jede Aktion des Verzeichnisses ist über die Command Palette auffindbar und ausführbar
- [ ] Test: Ein geändertes Kürzel wirkt sofort, bleibt nach Neustart erhalten, eine Doppelbelegung wird abgelehnt
- [ ] Desktop-Test: Der durchgehende Ablauf (Mail öffnen, antworten, senden, archivieren) gelingt ohne Maus
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

**Zwischenstand vom 23.11.2026, Stufe in Arbeit.** Gebaut in 2152c42, b313c22 und fcfae72
(Hauptsitzung mit Subagenten: Command Palette und Kürzel, Layouts, Barrierefreiheit). Geprüft von
der Hauptsitzung: `scripts/check.sh` grün auf 255ce74 (947 Rust-, 909 Vitest-Tests in 41 Dateien,
darunter `action-registry`, `command-palette`, `shortcut-settings`, `keyboard-bindings`, `layout`
und `a11y`). Die Desktop-Specs dieser Stufe (`app/tests/e2e/keyboard.e2e.mjs`, `layouts.e2e.mjs`)
sind geschrieben, stehen aber nicht in `specs` von `app/wdio.conf.mjs` und sind nie gelaufen. Der
Windows-Prototyp `prototype-2026-11-23` enthält Stufe 8 nicht.

**Erledigt und nachgewiesen (Vitest)**
- Aktionsverzeichnis (`app/src/features/actions/registry.ts`): Beschriftung in beiden Sprachen,
  keine Doppelbelegung je Plattform, Bedingung für „verfügbar“ mit Grund
  (`action-registry.test.ts`). Kürzel, Schnellaktionen, Symbolleiste und Command Palette lesen
  daraus.
- Command Palette (`command-palette.test.tsx`): jede Aktion des Verzeichnisses ist per Tastatur
  auffindbar und ausführbar; nicht ausführbare erscheinen mit Grund und laufen nicht; Sprungziele
  (Ansichten, Ordner, Konten, Einstellungen), Übergabe an die Suche, zuletzt benutzte oben, Combobox
  mit Listbox, Escape gibt den Fokus zurück, Tab bleibt im Dialog, Cmd+1 bis 8.
- Kürzel (`shortcut-settings.test.tsx`, `keyboard-bindings.test.ts`): eine Änderung wirkt sofort,
  auch im vereinten Posteingang und in der Palette; abgelehnt werden Doppelbelegungen (mit Namen des
  anderen Befehls), Systemkürzel und die Tasten zum Bewegen; Standardbelegung ohne Konflikte auf
  macOS und Windows; Übersicht mit `?`. Regel für Tastaturlayouts (`event.key`, nicht `event.code`)
  in `docs/ARCHITECTURE.md`, Abschnitt Tastaturkürzel.
- Layouts (`layout.test.tsx`): Lesebereich rechts, unten oder anstelle der Liste (Escape führt
  zurück, eine Aktion setzt den Fokus auf die nächste Mail), kompakte Zeilen, Schriftgröße 80 bis
  150 Prozent, aufgeklappte Seitenleiste bleibt, j/k wie die Pfeiltasten, Wechsel über die Palette.
- Screenreader: Die virtualisierte Liste meldet Position und Gesamtzahl (`aria-posinset`,
  `aria-setsize`; `message-list.test.tsx`, im Desktop-Test `account-sync.e2e.mjs` bis Position 9.959
  von 9.959); Statusmeldungen über `role="status"` (u. a. `status.test.tsx`, `actions.test.tsx`).
- axe-core (`a11y.test.tsx`): keine schweren oder kritischen Befunde in Posteingang, Lesebereich,
  Composer, allen Einstellungsabschnitten, Aktivität, Ausgang, Command Palette und Kürzelübersicht,
  jeweils hell und dunkel.
- Checkliste für VoiceOver und Narrator: `docs/testing/screenreader-checklist.md`.

**Offen**
- Desktop-Suite auf 255ce74 (mit dieser Oberfläche): 58 von 61 Tests grün; rot sind drei Tests in
  `reading.e2e.mjs` (Ordner `mailbox-4` nicht gefunden, danach Script-Timeouts), die auf 5d40e88 mit
  denselben Testständen grün waren. Ursache offen; zu klären, ob die Oberfläche der Stufen 8, 9
  (Teil B) oder 10 sie auslöst.
- Lesebereich in eigenem Fenster; Layout je Fenster gespeichert. Kürzeländerungen, Layout und
  zuletzt benutzte Befehle liegen bis dahin in einem lokalen Ersatzspeicher der WebView
  (`app/src/features/keyboard/localFallback.ts`). Die Kern-Commands dafür
  (`get_ui_preferences`/`set_ui_preferences`, Fenster-Layout, Nachrichtenfenster) sind in Arbeit.
- Abnahme „bleibt nach Neustart erhalten“: in Vitest mit nachgestelltem Neustart grün („keeps the
  change after a restart“), in der echten App nicht geprüft; wird mit dem Kern-Speicher neu
  nachgewiesen.
- Desktop-Test ohne Maus (`keyboard.e2e.mjs`: öffnen, antworten, senden, archivieren, Palette) und
  Layouts bei 900×600 und 2560×1440 mit Bildschirmfotos (`layouts.e2e.mjs`): in `specs` aufnehmen,
  laufen lassen, Bildschirmfotos ins Ergebnis.
- axe in den Desktop-Tests (Aufgabe „Automatische Prüfung“); bisher nur in jsdom.
- Leistungswerte aus Stufe 7 nicht neu gemessen. Dazu der Befund aus dem Desktop-Test: Nachdem alle
  9.959 Zeilen des Posteingangs zweimal gerendert worden waren, wurde späteres Scrollen langsam
  (längster Frame rund 250 ms statt 25 ms). Scrollen während des Erstabgleichs im Desktop-Test:
  längster Frame 25 ms auf 5d40e88, 45 ms auf 255ce74 (Test grün).
- Durchsicht ohne Nachweis: sichtbarer Fokusring überall, Bewegungs-Tokens und „Bewegung
  reduzieren“, Schriftgröße 150 Prozent ohne abgeschnittene Inhalte, alle leeren, ladenden und
  Fehlerzustände. Vollständige und gleiche Schlüssel in Englisch und Deutsch prüft `i18n.test.ts`.
- Nächster Schritt: Kern-Speicher für UI-Einstellungen und Fenster fertigstellen, die beiden
  Desktop-Specs aufnehmen und ausführen, axe im Desktop-Test, Leistung neu messen.

**Abweichungen und Annahmen**
- axe läuft in Vitest/jsdom statt im Desktop-Test. jsdom rechnet keine Farben, deshalb ist die Regel
  `color-contrast` dort aus; Kontraste prüft `tests/contrast.test.ts` an allen Farbpaaren der Tokens
  in beiden Themen (4,5:1 und 3:1). Eingebettete HTML-Mails (Frame) prüft axe nicht.
- Der lokale Ersatzspeicher ist `localStorage` der WebView; er enthält keine Mail-Daten und nichts
  Geheimes, alles Gelesene wird geprüft. Er entfällt, sobald der Kern die Werte speichert.
- Eine eigene Menüleiste gibt es nicht; „Menüs“ sind die Schnellaktionen, die Symbolleiste und die
  Palette.

**Wartet auf**
- Mensch: einen Tag nur mit Tastatur arbeiten (welche Kürzel fehlen oder stören?);
  Screenreader-Checkliste auf beiden Systemen, wenn gewünscht; Windows (Strg-Kürzel) mit einem neuen
  Build, der Stufe 8 enthält.
