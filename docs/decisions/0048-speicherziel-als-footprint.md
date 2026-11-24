# 0048 — Speicherziel im Leerlauf gemessen als Footprint, nicht als Summe der RSS

- **Datum:** 24.11.2026
- **Anlass:** Messung der Leistungsziele in der gebauten App (`docs/testing/performance.md`,
  Abschnitt „Desktop-App, gemessen 24.11.2026“). Alle Ziele aus `docs/SPEC.md` sind erreicht, nur
  „Arbeitsspeicher im Leerlauf unter 250 MB“ hängt davon ab, wie gemessen wird: als Footprint
  (Spalte „Speicher“ der Aktivitätsanzeige) 155–201 MB, als Summe der `ps`-RSS aller Prozesse der
  App 384–747 MB.
- **Status:** entschieden von der ausführenden Sitzung (Claude), der Mensch kann es umstoßen

## Entscheidung

Das Ziel gilt für den **Footprint** der App und ihrer WebKit- bzw. WebView2-Prozesse
(macOS: `phys_footprint`, wie ihn die Aktivitätsanzeige zeigt; Windows: „Arbeitsspeicher
(privater Arbeitssatz)“ im Task-Manager), 60 s nach einem Warmstart.

## Begründung

- RSS zählt geteilte Systembibliotheken in jedem Prozess erneut und enthält Seiten der Datenbank,
  die über Memory-Mapping gelesen wurden und die das System jederzeit verwerfen kann. Bei 10.000
  Mails zeigt die App allein 135 MB RSS, aber 43 MB Footprint.
- Der Footprint ist das, was Nutzer in der Aktivitätsanzeige bzw. im Task-Manager sehen, und das,
  was das System bei Speicherdruck tatsächlich zurückfordern muss.
- Die RSS-Summe wird weiter gemessen und berichtet, damit Ausreißer (WebContent bis 924 MB nach
  starker Nutzung, freigegeben, aber noch nicht zurückgegeben) sichtbar bleiben.

## Folgen

- `docs/SPEC.md`, Leistungsziele: Zeile „Arbeitsspeicher im Leerlauf“ nennt die Messart.
- Offen und beobachtet: Footprint nach starker Nutzung (eine Probe 306 MB bei 10.000 Mails) und
  Windows (nicht gemessen).
