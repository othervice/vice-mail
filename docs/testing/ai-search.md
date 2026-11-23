# Suche in natürlicher Sprache: Prüfsatz und Ergebnisse (Stufe 13)

Prüfsatz: `testmail/corpus/qa/questions.json`, 30 Fragen (deutsch und englisch) über den festen
Seed-Korpus `testmail/corpus/seed` (ohne die absichtlich kaputten Mails und ohne die erzeugten
Füllmails). 25 Fragen haben eine Antwort im Korpus, 5 nicht („nichts gefunden“ ist dort die
richtige Antwort). Je Frage stehen die Suchanfrage, die ein gutes Modell in der Suchsprache aus
Stufe 7 erzeugen sollte, die erwarteten Mails und die erwartete Aussage.

Harness: `crates/mail-engine/src/ai/tests/qa.rs`. Er spielt die Mails in den Speicher-Mailserver
der Tests ein, synchronisiert, öffnet jede Mail einmal (Body gespeichert), wartet auf den
Suchindex und stellt dann jede Frage über `AppCore::ai_ask` (Frage → Suchanfrage → lokale
Kandidaten → Antwort nur aus den Kandidaten).

## 1. Mit der Modell-Attrappe (läuft in `scripts/check.sh`)

`cargo test -p mail-engine --lib ai::tests::qa::qa_set_with_the_mock_model -- --nocapture`

Die Attrappe gibt im ersten Schritt die Referenz-Suchanfrage des Prüfsatzes zurück und zitiert im
zweiten Schritt alle Kandidaten, die sie bekommen hat (dazu einen erfundenen Verweis `m99`). Das
misst den lokalen Teil (findet die Suche mit einer guten Anfrage die richtigen Mails?) und prüft
den Ablauf, nicht die Qualität eines Modells.

Stand 23.11.2026:

| Messgröße | Ergebnis |
|---|---|
| Fragen mit Antwort im Korpus: erwartete Mail unter den Kandidaten | 25 von 25 |
| … erwartete Mail als erster Kandidat | 24 von 25 (Frage 8: ein Anhangsname enthält das Wort ebenfalls) |
| Fragen ohne Antwort: ehrlich „nichts gefunden“, ohne zweite Anfrage und ohne Mail | 5 von 5 |
| Erfundene Verweise entfernt, jede Quelle eine gespeicherte Mail | 30 von 30 |

Der Test schlägt fehl, wenn weniger als 90 % der beantwortbaren Fragen ihre Mail unter den
Kandidaten haben oder eine Frage ohne Antwort etwas „findet“.

## 2. Mit einem echten Modell (offen, braucht den Menschen)

Anteil richtiger Fundstellen und Antworten mit einem echten Modell. Noch nicht gelaufen: Es
braucht Ollama (oder einen anderen lokalen Server) auf diesem Rechner.

```
ollama pull llama3.2        # oder ein anderes Modell
VICE_MAIL_QA_BASE_URL=http://localhost:11434/v1 VICE_MAIL_QA_MODEL=llama3.2 \
  cargo test -p mail-engine --lib ai::tests::qa::qa_set_with_a_real_model -- --ignored --nocapture
```

Der Lauf verbindet sich nur mit der angegebenen lokalen Adresse. Er druckt je Frage die erzeugte
Suchanfrage, die zitierten Mails und die Aussagen und am Ende, wie oft eine erwartete Mail zitiert
wurde (und als erste) und wie oft bei den fünf Fragen ohne Antwort „nichts gefunden“ kam. Ob die
Aussagen inhaltlich stimmen, beurteilt der Mensch anhand der Spalte `answer` im Prüfsatz.

| Modell | Datum | erwartete Mail zitiert | als erste | „nichts gefunden“ richtig | Aussagen richtig |
|---|---|---|---|---|---|
| _offen_ | | | | | |
