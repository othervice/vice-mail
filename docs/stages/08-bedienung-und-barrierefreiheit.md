# Stufe 08 — Bedienung und Barrierefreiheit

**Ziel:** Die App ist vollständig per Tastatur bedienbar, hat eine Command Palette, mehrere Layouts
und erfüllt die Barrierefreiheits-Kriterien aus `docs/SPEC.md`.

**Voraussetzung:** Stufe 07 fertig. `docs/DESIGN.md` gelesen, dazu die Reibungspunkte, die der
Mensch seit Stufe 5 gesammelt hat (Ergebnis-Abschnitte der Stufen 05 bis 07).

## Aufgaben

- [ ] **Aktionsverzeichnis.** Eine zentrale Liste aller Aktionen: Kennung, Beschriftung, Standardkürzel je Plattform, Ausführung, Bedingung für „verfügbar“. Menüs, Kürzel, Schnellaktionen und Command Palette speisen sich alle daraus.
- [ ] **Tastenkürzel.** Belegung nach verbreiteten Mustern (j/k oder Pfeile für nächste/vorherige, Eingabe öffnet, Esc schließt, e archiviert, # löscht, r antwortet, c schreibt neu, / sucht). In den Einstellungen änderbar, mit Erkennung von Doppelbelegungen. Keine Kürzel, die das System beansprucht. Regel für Tastaturlayouts festlegen und dokumentieren (Zeichen über `event.key`, Positionen über `event.code`). Übersicht aller Kürzel mit `?`.
- [ ] **Command Palette.** Cmd+K / Ctrl+K. Enthält alle Aktionen aus dem Verzeichnis, Sprungziele (Ordner, Konten, Ansichten, Einstellungen) und die Übergabe an die Suche. Zuletzt benutzte Befehle oben. Vollständig per Tastatur.
- [ ] **Layouts.** Lesebereich rechts (Standard), unten, anstelle der Liste, in eigenem Fenster. Dichte kompakt und komfortabel. Icon-Leiste oder aufgeklappte Seitenleiste. Sinnvolles Verhalten von 900×600 bis 2560×1440. Je Fenster gespeichert.
- [ ] **Fokus.** Sichtbarer Fokusring überall. Nach einer Aktion steht der Fokus auf der nächsten Mail. Dialoge und Panels geben den Fokus beim Schließen zurück.
- [ ] **Screenreader.** Rollen und Beschriftungen für Liste, Zeilen, Schnellaktionen, Lesebereich und Composer. Die virtualisierte Liste meldet Position und Gesamtzahl. Statusmeldungen (gesendet, archiviert, Fehler) werden angesagt.
- [ ] **Darstellung.** Schriftgröße 80 bis 150 Prozent ohne abgeschnittene Inhalte. „Bewegung reduzieren“ schaltet Animationen ab. Kontofarben immer mit Namen.
- [ ] **Bewegung.** Die Bewegungs-Tokens konsequent anwenden; alles entfernen, was keinen Ortswechsel erklärt.
- [ ] **Zustände und Texte.** Alle leeren, ladenden und Fehlerzustände durchgehen. Texte in Englisch und Deutsch vollständig und einheitlich.
- [ ] **Automatische Prüfung.** axe-core in den Desktop-Tests für die Hauptansichten in beiden Themen.

## Abnahme

- [ ] axe meldet keine schweren oder kritischen Befunde in Posteingang, Lesebereich, Composer, Einstellungen, jeweils hell und dunkel
- [ ] Test: Jede Aktion des Verzeichnisses ist über die Command Palette auffindbar und ausführbar
- [ ] Test: Ein geändertes Kürzel wirkt sofort, bleibt nach Neustart erhalten, eine Doppelbelegung wird abgelehnt
- [ ] Desktop-Test: Der durchgehende Ablauf (Mail öffnen, antworten, senden, archivieren) gelingt ohne Maus
- [ ] alle Layouts sind bei 900×600 und 2560×1440 benutzbar (Bildschirmfotos im Ergebnis)
- [ ] Checkliste für VoiceOver und Narrator unter `docs/testing/screenreader-checklist.md`
- [ ] Leistungswerte aus Stufe 7 neu gemessen, keine Verschlechterung
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- neue Mail-Funktionen
- Kategorien, Snooze, Benachrichtigungen (Stufe 9)

## Der Mensch prüft danach

- Einen Tag nur mit Tastatur arbeiten. Welche Kürzel fehlen oder stören?
- Screenreader-Checkliste auf beiden Systemen durchgehen, wenn gewünscht.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
