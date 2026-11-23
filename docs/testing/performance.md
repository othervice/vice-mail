# Leistungsmessung

Ziele aus `docs/SPEC.md` (Leistungsziele). Gemessen in Stufe 7; danach bei jeder Stufe mit Listen- oder
Suchänderung erneut.

## Stufe 7 – 23.11.2026

**Umgebung:** Mac mini, Apple M4 (10 Kerne), 16 GB RAM, macOS 27.2.1, Rust 1.100.0, SQLite 3.54.1
(rusqlite 0.41, bundled), Release-Build. Während der Messung liefen keine Builds anderer Sitzungen
(Load 2,7–3,4; ein Windows-Build einer anderen Sitzung war vorher fertig). Warm (Seiten im
SQLite- bzw. Betriebssystem-Cache). Ein Kaltstart nach Neustart des Rechners ist **ungeprüft**.

**Wie gemessen:** `crates/mail-engine/src/search/bench.rs`

```text
cargo test -p mail-engine --release --lib search::bench -- --ignored --nocapture --test-threads 1
```

- Erzeugt über die Store-API wie ein Erstabgleich: fünf Konten (50 / 24 / 16 / 8 / 2 %), je Konto
  INBOX 55 %, Archive 20 %, Sent 12 %, Newsletter 10 % (Liste mit `List-Id`), Trash 3 %; Daten
  2019–2026, zur Gegenwart dichter; Fenster von 500, neueste zuerst, Threads und Index werden mit
  jedem Fenster geschrieben. 40 % der Mails sind Antworten (`In-Reply-To`/`References`) auf eine
  der letzten 300 Unterhaltungen, im Newsletter-Ordner wächst ein langer Listen-Thread
  (100.000: 1.461 Mails, 500.000: 7.620 Mails). Wortschatz 30.000 Wörter (Zipf), 3.000 Absender
  (Zipf), 15 % mit Anhang.
- Bodies lokal (wie Vorausladen und Öffnen): Posteingang und Gesendet der letzten 30 Tage,
  markierte Mails und 15 % der übrigen, 30 % davon HTML; der Indextext kommt über `mail-content`
  wie im Produkt (100.000: 17.935 Bodies, 500.000: 89.631 Bodies).
- Alle Abfragen über `AppCore` (Parser, Auflösung von `account:`/`in:`, Store, Hervorhebung), mit
  einem Fake-Connector, der jede Verbindung ablehnt (kein Netz). Median, p95 und Maximum über
  100 (100.000) bzw. 50 (500.000) Wiederholungen; „Mail öffnen“ über 200 verschiedene Mails.
- „Start bis zur benutzbaren Liste“ und Arbeitsspeicher in einem eigenen Kindprozess (dasselbe
  Testbinary, `child_measure`), damit der Generator nicht mitzählt: `AppCore::start` auf der
  erzeugten Datenbank, erste Seite des größten Posteingangs, dann Thread-Liste, vereinter
  Posteingang und vier Suchen, 2 s Leerlauf, residenter Speicher per `ps -o rss` (gemappte Seiten
  der Datenbank zählen nur, soweit sie gelesen wurden).
- Die Datenbanken lagen in einem temporären Verzeichnis und sind gelöscht.

### Ergebnis gegen die Ziele

| Ziel (`docs/SPEC.md`) | gemessen 100.000 | gemessen 500.000 | erfüllt |
|---|---:|---:|---|
| Suche bis erste Treffer unter 200 ms bei 100.000 Mails | 0,24–4,3 ms (Median, alle Anfragen), schlechteste p95 4,5 ms | 0,26–16,5 ms, schlechteste p95 17,2 ms | ja |
| Wechsel zwischen lokal vorhandenen Mails unter 100 ms | 0,36 ms Median, p95 0,47, max 0,72 | 0,39 / 0,45 / 1,88 ms | ja |
| Warmstart bis benutzbarer Posteingang unter 1 s | 2 ms bis zur ersten Seite (Start und Seite) | 2 ms | ja (nur Kern) |
| Arbeitsspeicher im Leerlauf unter 250 MB | 21 MB | 21 MB | ja (nur Kern) |

Abweichungen und Einschränkungen, benannt:
- **Start und Speicher messen nur den Kern** (Store, Engine, Abgleichs-Tasks ohne Netz), nicht die
  WebView und nicht das Rendern der Liste. Die App als Ganzes ist nicht gemessen.
- **„Mail öffnen“** misst `getMessage` im Kern (Body laden, parsen, bereinigen, rendern), nicht das
  Zeichnen im Frame der Oberfläche.
- **Schreiben beim Abgleich ist messbar langsamer**: Threads und Index kosten je Fenster von 500
  Kopfdaten 4,7 ms zusätzlich (siehe unten). Für den laufenden Abgleich (wenige neue Mails) ist das
  unter einer Millisekunde, für einen Erstabgleich von 100.000 Mails knapp 1 s Datenbankzeit mehr.
- Die erzeugten Daten sind Richtwerte (keine echten Postfächer, keine Zitate, Sprache je Wort
  gemischt). Die Abstände zu den Zielen sind so groß, dass das am Ergebnis nichts ändern dürfte.

### Listen und Threads (ms)

| Messung | 100.000 Median | p95 | max | 500.000 Median | p95 | max |
|---|---:|---:|---:|---:|---:|---:|
| Listenseite größter Posteingang (100 Zeilen) | 0,10 | 0,10 | 0,18 | 0,09 | 0,11 | 0,18 |
| … Keyset ab Mitte | 0,09 | 0,09 | 0,11 | 0,09 | 0,09 | 0,11 |
| Vereinter Posteingang, 5 Konten (100) | 0,47 | 0,49 | 0,77 | 0,46 | 0,49 | 0,82 |
| Thread-Liste größter Posteingang (100) | 0,26 | 0,26 | 0,42 | 0,27 | 0,28 | 0,43 |
| … Keyset ab Mitte | 0,29 | 0,33 | 0,36 | 0,28 | 0,28 | 0,36 |
| Thread-Liste vereinter Posteingang (100) | 1,35 | 1,50 | 1,82 | 1,34 | 1,37 | 1,71 |
| Thread-Liste Newsletter (langer Listen-Thread) | 0,69 | 0,70 | 0,90 | 2,88 | 3,00 | 3,79 |
| Thread öffnen (1.461 bzw. 7.620 Mails, neueste 1.000) | 3,54 | 3,86 | 3,99 | 3,69 | 3,72 | 4,39 |
| Mail öffnen, Body lokal (`getMessage`, HTML) | 0,36 | 0,47 | 0,72 | 0,39 | 0,45 | 1,88 |

Die Thread-Liste zählt die Ungelesenen beim Lesen (`docs/decisions/0017-suche-threads-integration.md`): Die Zeile des langen
Listen-Threads kostet bei 7.620 Mails rund 2 ms mehr; das wächst linear mit der Thread-Größe. Der
vereinte Posteingang sortiert je Konto höchstens 101 Zeilen nach (Plan: `MERGE (UNION ALL)` mit
kleinem temporärem B-Tree je Zweig, wie in 0003).

### Erste Suchtreffer (50, über `AppCore::search`, ms)

| Anfrage | 100.000 Median | p95 | 500.000 Median | p95 |
|---|---:|---:|---:|---:|
| häufiges Wort `rechnung` | 0,27 | 0,33 | 0,28 | 0,34 |
| sehr häufiges Wort `und` | 0,27 | 0,29 | 0,26 | 0,29 |
| seltenes Wort | 0,24 | 0,27 | 0,27 | 0,31 |
| beim Tippen, Präfix `rech` | 0,84 | 0,92 | 2,22 | 2,27 |
| Phrase `"viele grüße"` | 0,30 | 0,33 | 0,32 | 0,35 |
| zwei Wörter | 0,29 | 0,31 | 0,30 | 0,33 |
| `from:` Nachname | 0,26 | 0,28 | 0,27 | 0,28 |
| Wort + `account:` (kleinstes Konto, 2 %) | 0,75 | 0,78 | 0,70 | 0,74 |
| Wort + `in:inbox is:unread` | 0,41 | 0,44 | 0,42 | 0,44 |
| Wort + `newer_than:1y` | 0,27 | 0,28 | 0,28 | 0,30 |
| `filename:rechnung has:attachment` | 0,25 | 0,27 | 0,26 | 0,28 |
| ohne Text: `is:unread in:inbox` | 0,34 | 0,36 | 0,35 | 0,39 |
| nur Ausschluss `-und` | 4,34 | 4,49 | 16,52 | 17,23 |
| `rechnung OR invoice` | 0,28 | 0,31 | 0,30 | 0,33 |

Ein erster Lauf hatte für `in:` noch die Postfächer samt Zählern gelesen (1,5 bzw. 5,5 ms); seitdem
liest die Auflösung nur Namen und Rollen (`Store::mailbox_names`, mit Plan-Test). Der Ausschluss allein
(`-und`) ist der teuerste Fall: SQLite sammelt alle Treffer des Worts und läuft dann die Nachrichten von
neu nach alt durch (`rowid NOT IN …`). Auch bei 500.000 Mails weit unter dem Ziel.

### Schreiben, Index und Größen

| Messung | 100.000 | 500.000 |
|---|---:|---:|
| Kopfdaten speichern (Fenster von 500, mit Threads und Kopfzeilen im Index) | 18,5 ms je Fenster, 25.500 Mails/s | 29,2 ms je Fenster, 16.900 Mails/s |
| Bodies speichern (Pakete, Indextext über `mail-content`) | 7.100 Bodies/s | 4.000 Bodies/s |
| Neuaufbau des Index im Hintergrund (alle Mails, Bodies neu geparst) | 2,7 s | 14,7 s |
| Datenbankdatei | 92 MB | 451 MB |
| Body-Dateien | 10 MB | 51 MB |

**Kosten von Threads und Index beim Abgleich**, gleiche Daten und gleicher Generator
(`crates/mail-store/src/bench.rs`, 100.000 Kopfdaten in 200 Fenstern, je zwei Läufe hintereinander):
Stand vor Stufe 7 (Commit `6017a13`, in eigenem Verzeichnis gebaut) **1,13 s, 5,7 ms je Fenster**;
mit Stufe 7 **2,06–2,08 s, 10,3–10,4 ms je Fenster**. Der Generator dort hat keine References, also
fragt jede Mail auch den Betreff-Rückfall ab (ungünstigster Fall). Die Listenseite blieb bei
0,07–0,09 ms.

### Automatischer Test

`search::bench::small_data_meets_generous_thresholds` läuft mit jedem `cargo test` (Debug-Build,
3.000 Mails über fünf Konten, 5 Wiederholungen) und scheitert, wenn eine Messung über einer großzügigen
Schwelle liegt: Listenseite 50 ms, vereinter Posteingang 100 ms, Thread-Liste 200 ms, langsamste Suche
200 ms (Median), Mail öffnen 500 ms (p95). Gemessen im Debug-Build: 0,4 / 1,4 / 1,0 / 1,8 / 2,7 ms.
Er findet also nur echte Rückschritte (einen Scan, eine Sortierung über ein ganzes Postfach); die
Pläne selbst prüfen `mail-store`s Plan-Tests.

**Ungeprüft:** Kaltstart nach Neustart des Rechners, die App mit WebView, echte Postfächer, Windows.

## Stufe 9 – 23.11.2026

**Umgebung:** wie Stufe 7 (Mac mini M4, Release-Build), Load 2,7–3,6, andere Sitzungen bauten
nebenher. **Wie gemessen:** `crates/mail-engine/src/comfort/bench.rs`

```text
cargo test -p mail-engine --release --lib comfort::bench -- --ignored --nocapture --test-threads 1
```

100.000 Kopfdaten in einem Posteingang, Fenster von 500 wie ein Erstabgleich, einmal ohne und
einmal mit Einordnung (rohe Kopfzeilen-Blöcke wie vom Adapter, ein Drittel Newsletter, ein Sechstel
Benachrichtigungen); fünf Pins, fünf Snoozes, ein wichtiger Absender. Abfragen über die Store-API,
200 Wiederholungen.

| Messgröße | Ergebnis |
|---|---:|
| 100.000 Kopfdaten speichern ohne / mit Einordnung | 2.703 / 3.201 ms (13,5 / 16,0 ms je Fenster) |
| Absender-Regel auf 100.000 Mails anwenden (Hintergrund) | 27 ms |
| `listMessages`, 100 Zeilen samt Markierungen | 0,21 ms Median (p95 0,22) |
| Posteingang ohne Snoozes und Pins, 100 Zeilen | 0,25 ms (p95 0,25) |
| Posteingang einer Kategorie, 100 Zeilen | 0,25 ms (p95 0,26) |
| Block „Priorität“ (3) und Zähler | 0,05 ms |
| Angepinnte und zurückgekehrte oben | 0,09 ms |
| Zähler am Symbol (ungelesene Personen, 11.000 ungelesen) | 2,3 ms |
| Bündeln einer Seite von 100 (Abschnitte, Chips) | 0,01 ms |

- Die Einordnung kostet je Fenster rund 2,5 ms (Parsen des Kopfzeilen-Blocks, eine Kontakt- und
  bis zu zehn Antwort-Abfragen je Mail). Die Basis ohne Einordnung (13,5 ms) ist höher als die
  10,4 ms aus Stufe 7: anderer Generator (alle Mails in einem Postfach), Trigger für die Kopie der
  Kategorie am Ort, Last durch andere Sitzungen; nicht getrennt gemessen.
- Alle Listen bleiben um Größenordnungen unter den Zielen; Pläne prüft
  `crates/mail-store/src/stage9_plan_tests.rs`.
