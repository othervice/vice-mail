# Stufe 07 — Suche, Threads, mehrere Konten

**Ziel:** Schnelle Volltextsuche mit Filtern, Konversationen statt Einzelmails und ein vereinter
Posteingang über mehrere Konten. Die Leistungsziele aus `docs/SPEC.md` sind gemessen.

**Voraussetzung:** Stufe 05 fertig (Stufe 06 empfohlen). Ergebnis von Spike B gelesen.

## Aufgaben

- [ ] **Index.** FTS5 über Betreff, Absender, Empfänger, Text und Anhangnamen. Aktualisierung bei jedem Einfügen, Ändern und Löschen. Text wird indiziert, sobald der Body lokal vorliegt; Kopfdaten immer. Neuaufbau im Hintergrund mit Austausch am Ende, ohne die Suche zu unterbrechen.
- [ ] **Suchsprache.** Parser für freien Text und die Filter aus `docs/SPEC.md`, Phrasen, Ausschluss mit `-`, Datumsangaben in deutscher und englischer Schreibweise. Fehler werden an der Stelle angezeigt.
- [ ] **Suche in der Oberfläche.** Treffer beim Tippen, Filter als Chips, hervorgehobene Fundstellen, letzte Suchen. Suchen speichern: sie erscheinen als Ansicht in der Leiste.
- [ ] **Auf dem Server weitersuchen.** Schaltfläche für Mails, deren Inhalt nicht lokal vorliegt (IMAP SEARCH).
- [ ] **Threads.** Je Konto über Message-ID, In-Reply-To und References; vorsichtiger Rückfall auf den Betreff wie in der Architektur. Tabelle `thread` wird laufend gepflegt. Zeile in der Liste mit Zähler; im Lesebereich ältere Mails eingeklappt, ungelesene offen. Konversationsansicht abschaltbar.
- [ ] **Vereinter Posteingang.** Posteingänge aller Konten nach Zeit gemischt, Kontofarbe immer sichtbar. Selbst definierte Kontogruppen. Konten vorübergehend ausblenden. Umschalten über den Kopf der Liste.
- [ ] **Identitäten.** Mehrere Absenderadressen je Konto, eine als Standard. Antworten wählt die Identität, an die die Mail ging.
- [ ] **Leistungsmessung.** Erzeugte Postfächer mit 100.000 und 500.000 Mails über fünf Konten. Messen: Listenseite, Thread-Liste, erste Suchtreffer, Start bis zur benutzbaren Liste, Arbeitsspeicher im Leerlauf. Ergebnisse in `docs/testing/performance.md`; großzügige Schwellen als automatischer Test.

## Abnahme

- [ ] Tabellentests für den Parser: jeder Filter, Kombinationen, fehlerhafte Eingaben
- [ ] Tests für Threads mit vorbereiteten Fällen: Mailingliste, kaputte References, gleicher Betreff ohne Zusammenhang (darf nicht zusammengeführt werden)
- [ ] Test: Der Index bleibt nach Einfügen, Ändern, Löschen und Neuaufbau mit der Nachrichtentabelle im Einklang
- [ ] Test: Im vereinten Posteingang ist jede Mail ihrem Konto zugeordnet; ausgeblendete Konten fehlen
- [ ] gemessen und eingetragen: Suche unter 200 ms bei 100.000 Mails, Wechsel zwischen lokalen Mails unter 100 ms; Abweichungen sind benannt
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Suche in natürlicher Sprache (Stufe 13)
- Threads über mehrere Konten hinweg
- Kategorien und Bündel (Stufe 9)

## Der Mensch prüft danach

- Mit den eigenen Konten suchen: Findet die Suche, was erwartet wird? Sind Threads richtig zusammengeführt?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
