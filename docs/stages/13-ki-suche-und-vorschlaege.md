# Stufe 13 — KI-Suche und Vorschläge

**Ziel:** Fragen in natürlicher Sprache werden aus den lokal vorhandenen Mails beantwortet, mit
Quellen. Die App weist auf Mails hin, die vielleicht noch eine Antwort brauchen.

**Voraussetzung:** Stufen 07 und 12 fertig.

## Aufgaben

- [ ] **Prüfsatz.** 30 Fragen mit erwarteten Fundstellen an einem vorbereiteten Postfach unter `testmail/corpus/qa/` („Was hat uns dieser Lieferant letzte Woche angeboten?“, „Finde die letzte Rechnung des Hosting-Anbieters“, „Wer wartet noch auf Antwort?“). Er entscheidet über die Technik.
- [ ] **Ablauf der Suche.** Frage → das Modell übersetzt sie in die Suchsprache aus Stufe 7 (Begriffe, Filter, Zeitraum) → lokale Suche liefert Kandidaten → das Modell antwortet nur aus diesen Kandidaten, jede Aussage mit Verweis auf die Mail. Keine Kandidaten: ehrlich „nichts gefunden“. Eine Antwort ohne Quelle wird nicht angezeigt.
- [ ] **Embeddings nur bei Bedarf.** Reicht die Volltextsuche im Prüfsatz nicht, zusätzlich Vektorsuche (Embeddings über den eingerichteten Anbieter, Ablage in SQLite). Die Entscheidung mit den Zahlen des Prüfsatzes festhalten.
- [ ] **Assistent über das ganze Postfach.** Im KI-Panel, auf demselben Ablauf. Kann Entwürfe vorbereiten („fasse zusammen und schreibe an …“): Ergebnis ist eine Karte mit „Entwurf öffnen“; gesendet wird nur vom Nutzer.
- [ ] **Hinweise auf offene Antworten.** Zuerst ohne KI: gesendete Mail mit Frage und ohne Antwort nach einigen Tagen; empfangene Mail einer Person mit direkter Frage, unbeantwortet. Optional verfeinert das Modell. Anzeige als wegklickbarer Vorschlag („Vielleicht wartet … auf Antwort“), nie als Behauptung.
- [ ] **Kategorien per KI, optional.** Für Mails, die die Kopfzeilen-Regeln aus Stufe 9 nicht einordnen: Rechnungen, Verträge, Kundenanfragen und eigene Kategorien des Nutzers. Empfohlen mit lokalem Anbieter. Ergebnis je Mail gespeichert. Ein Massenlauf über einen Cloud-Anbieter nur nach Bestätigung mit Angabe der Anzahl.

## Abnahme

- [ ] Ergebnis des Prüfsatzes steht in `docs/testing/ai-search.md`: Anteil richtiger Fundstellen, Anteil richtiger Antworten
- [ ] Test: Jede angezeigte Aussage hat einen Verweis auf eine vorhandene Mail; Antworten ohne Kandidaten enthalten keine Behauptungen
- [ ] Test: An den Anbieter gehen nur die Kandidaten, nie das Postfach; die Anzahl stimmt mit der Anzeige überein
- [ ] Test: Hinweise auf offene Antworten an vorbereiteten Fällen, darunter Fälle, die keinen Hinweis auslösen dürfen (Newsletter, automatische Mails, bereits beantwortet)
- [ ] Test: Ein Massenlauf startet nicht ohne Bestätigung
- [ ] Korpus mit eingeschleusten Anweisungen aus Stufe 12 läuft auch durch die Suche ohne Wirkung
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Aktionen durch das Modell
- Lernen aus dem Verhalten des Nutzers über einfache Absender-Regeln hinaus

## Der Mensch prüft danach

- Eigene Fragen an das eigene Postfach stellen. Stimmen Antworten und Quellen?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
