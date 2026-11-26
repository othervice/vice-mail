# Lernender Spamfilter – Messung

Stufe 22, Punkt 6 (Entscheidung 0077). Gemessen am 26.11.2026 auf dem Mac mini (M4), Debug-Build,
während andere Sitzungen parallel bauten (die Zeiten schwanken deshalb; die Trefferquoten sind
deterministisch).

## Korpus

Öffentlicher SpamAssassin-Korpus (`spamassassin.apache.org/old/publiccorpus/`): `easy_ham`,
`easy_ham_2`, `hard_ham` (4.150 Ham, darunter 250 schwer erkennbare Werbe-Newsletter) und `spam`,
`spam_2` (1.896 Spam), Mails von 2002 bis 2005, überwiegend Englisch. Nur für die Messung in ein
Arbeitsverzeichnis geladen, nicht im Repository, nur gelesen und zerlegt.

Wiederholen:

```text
# Korpus entpacken: je Unterordner ham* bzw. spam*, eine Mail je Datei
VICE_MAIL_SPAM_CORPUS=/pfad/zum/korpus \
  cargo test -p mail-content --locked spam::corpus -- --ignored --nocapture
# mit VICE_MAIL_SPAM_EXPLAIN=1 zusätzlich die stärksten Spam-Hinweise der schlechtesten Ham-Mails
```

Der Test (`crates/mail-content/src/spam/corpus.rs`) mischt fest (immer gleich), lernt im Speicher
mit denselben Funktionen wie die App (`spam_tokens`, `spam_score`) und zählt:

- **falsch als Spam** (Ham ab der Schwelle) – das Wichtigste,
- **übersehen** (Spam unter der Schwelle).

## Trefferquote

### Zehnfache Kreuzvalidierung (gelernt: je 9/10 des Korpus, rund 3.735 Ham und 1.706 Spam)

| Schwelle | falsch als Spam | übersehen |
|---|---|---|
| 0,5 | 22 (0,53 %) | 48 (2,53 %) |
| 0,8 | 17 (0,41 %) | 80 (4,22 %) |
| 0,9 (SpamBayes, Thunderbird) | 7 (0,17 %) | 99 (5,22 %) |
| 0,95 | 4 (0,10 %) | 115 (6,07 %) |
| 0,98 | 3 (0,07 %) | 146 (7,70 %) |
| **0,99 (gewählt)** | **2 (0,05 %)** | **166 (8,76 %)** |
| 0,995 | 2 (0,05 %) | 187 (9,86 %) |

Die Ham-Mails mit den höchsten Werten sind alle aus `hard_ham` (Werbe-Newsletter einer Apotheke,
eines Autohändlers, eines Versands); ihre stärksten Spam-Hinweise sind Wörter wie „click“,
„receive“, „remove“, „offer“ und HTML mit vielen Bildern und Links.

### Nur Kopfdaten und Vorschau (ohne gespeicherten Text)

Dasselbe Modell, bewertet nur mit den Tokens, die die gespeicherten Kopfdaten ergeben:

| Schwelle | falsch als Spam | übersehen |
|---|---|---|
| 0,9 | 7 (0,17 %) | 184 (9,70 %) |
| **0,99** | **1 (0,02 %)** | **501 (26,42 %)** |

Weniger Tokens machen den Filter vorsichtiger, nicht strenger.

### Lernkurve: n Spam und m Ham gelernt, der Rest bewertet (Schwelle 0,9 / 0,99)

| gelernt (Spam / Ham) | falsch als Spam 0,9 | 0,99 | übersehen 0,9 | 0,99 |
|---|---|---|---|---|
| 10 / 10 | 8,41 % | 6,64 % | 14,26 % | 21,74 % |
| 20 / 20 | 3,61 % | 1,89 % | 19,88 % | 32,30 % |
| 20 / 100 | 0 % | 0 % | 76,33 % | 84,38 % |
| 50 / 50 | 2,63 % | 1,24 % | 14,68 % | 22,37 % |
| **50 / 200 (Mindestmaß)** | **0 %** | **0 %** | 42,04 % | 56,39 % |
| 50 / 500 | 0 % | 0 % | 55,69 % | 69,01 % |
| 100 / 100 | 1,60 % | 0,81 % | 8,80 % | 14,25 % |
| 100 / 500 | 0,27 % | 0,08 % | 21,88 % | 32,41 % |
| 200 / 200 | 0,96 % | 0,56 % | 6,60 % | 12,50 % |
| 200 / 400 | 0,56 % | 0,35 % | 10,32 % | 18,22 % |
| 200 / 1.000 | 0,32 % | 0,06 % | 17,28 % | 25,35 % |
| 500 / 500 | 0,77 % | 0,58 % | 6,09 % | 9,89 % |
| 500 / 1.000 | 0,54 % | 0,29 % | 8,74 % | 13,40 % |
| 500 / 2.000 | 0,23 % | 0,14 % | 10,82 % | 15,19 % |

Daraus die Regeln der App (Entscheidung 0077):

- Schwelle **0,99**.
- Er handelt erst ab **50 Spam und 200 Ham**: mit wenigen, gleich vielen Mails je Klasse hält er
  1–7 % der guten Mails für Spam.
- Er hält **mindestens doppelt so viel Ham wie Spam** und lernt Ham aus gelesenen
  Posteingangsmails nach; beim Lernen aus vorhandenen Mails nimmt er bis zu 500 Spam und 1.000 Ham.
  Mehr Ham senkt die Fehlalarme deutlich, auf Kosten übersehenen Spams.

Nicht nachgebildet: Die App wertet Mails eigener Adressen, von Kontakten, denen man geschrieben
hat, Antworten auf eigene Mails, Absender mit erlaubten Bildern und wichtige Absender nie als
Spam. Im Korpus gibt es diese Beziehungen nicht; im Alltag fallen damit weitere Fehlalarme weg.

## Kosten je Mail

| Schritt | Debug-Build |
|---|---|
| Zerlegen in Tokens (Parsen, sichtbarer HTML-Text, Tokens), 6.046 Mails | Median 0,62 ms, p95 4,4 ms, p99 14,8 ms, max 30 ms (ruhig); unter Last Median 0,99 ms, p95 9,8 ms, p99 84 ms, max 256 ms (eine große HTML-Mail) |
| Tokens je Mail | 207 im Mittel (höchstens 2.000) |
| Bewerten mit Zählern im Speicher | Median 37 µs (ruhig), p99 0,2 ms |
| Nachschlagen von 200 Tokens in der Datenbank (Modell aus 1.500 Mails, 178.000 Tokens) | Median 0,58 ms, p95 1,0 ms, max 1,1 ms |
| Eine Mail lernen (im Hintergrund, Stapel zu 25) | 0,60 ms je Mail |
| Eine Mail umlernen (verlernen und neu lernen) | 3,4 ms |

Benchmark: `cargo test -p mail-store --lib spam_tests::bench_spam -- --ignored --nocapture`.

Zusammen also rund 1–2 ms je neuer Mail (Debug-Build, ohne das Holen des Textes), im
Hintergrund nach dem Speichern: Das Speichern eines Abgleichfensters wird nicht teurer (der Filter
läuft erst in `handle_arrival`, nach dem Fenster, und nur, wenn er an ist).

## Listen (Ziele aus `docs/testing/performance.md`)

Jede Listenzeile fragt zusätzlich das Urteil des Filters ab (ein Zugriff über den Primärschlüssel
von `spam_verdict` in `marks::decorate`). Gemessen mit demselben Benchmark (Seite von 100 Zeilen,
1.500 Mails, je 100 Läufe), einmal ohne diese Spalte:

| | Median | p95 |
|---|---|---|
| ohne Spalte (Code vorübergehend zurückgesetzt) | 1,90 ms | 1,98–2,03 ms |
| mit Spalte | 0,93–1,97 ms | 2,05–2,12 ms |

Kein Unterschied über dem Rauschen der parallel bauenden Maschine; die Ziele (Listenseite unter
50 ms im automatischen Test, Wechsel zwischen Mails unter 100 ms) bleiben um Größenordnungen
unterschritten. Ist der Filter aus, ist `spam_verdict` leer.

## Gegen den Testserver

`crates/mail-engine/src/spam/server_tests.rs` (Stalwart, Konto alice mit Schreibzugriff, Mindestmaß
im Test 3/3): drei Angebote vom Admin per SMTP, als Spam gelernt über „Als Spam markieren“ (auf dem
Server in den Spam-Ordner), drei Briefe von Bob als Ham. Ein viertes Angebot per SMTP lag in drei
Läufen **243, 282 und 286 ms** nach dem Absenden im Spam-Ordner des Servers (Zustellung, IDLE,
Abgleich, Text holen, bewerten, Verschieben über die Outbox), als ein Stapel. „Rückgängig“ holte
es in den Posteingang zurück, lernte es als Ham und nahm das Urteil weg. Weil drei Spam gelernt
waren, lernte der Filter danach selbst sechs gelesene Posteingangsmails als Ham nach (zusammen 10).

Der zweite Lauf fand einen Fehler, den die Tests mit dem Testserver im Speicher nicht zeigten: Die
zurückgeholte Mail kommt im Posteingang als neue Mail an, und der Filter urteilte über sie, bevor
er sie als Ham gelernt hatte (das Urteil blieb stehen, mit derselben Anfrage-ID wurde sie nicht
noch einmal verschoben). Seitdem wartet der Filter auf das Gelernte und lässt Mails, die schon
gelernt sind, in Ruhe; jede Verschiebung hat eine eigene Anfrage-ID. Ein Test im Speicher
(`new_spam_goes_to_junk_unannounced_and_undo_learns_it_as_ham`) prüft das nun mit; ohne die
Korrektur schlägt er fehl (gegengeprüft).

## Ungeprüft

- Echte, deutschsprachige Postfächer; neuer Spam (der Korpus ist von 2002–2005).
- Release-Build (die Zeiten oben sind Debug, Release ist erfahrungsgemäß fünf- bis zehnmal
  schneller).
- Windows.
