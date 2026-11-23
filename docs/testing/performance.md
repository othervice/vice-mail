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

## Stufe 8/9 – Nachmessung 23.11.2026

**Umgebung:** wie Stufe 7 (Mac mini M4, macOS 27.2.1, Release-Build, warm). „Vorher“ ist Stand
`ab594b7`, „nachher“ derselbe Stand mit den Änderungen unten
(`docs/decisions/0029-start-ohne-breites-lesen.md`). Load 2,5–3,3 bei den Benchmarks; andere
Sitzungen arbeiteten nebenher (zeitweise Load 9, diese Läufe sind verworfen), das Schreiben
schwankt deshalb um bis zu ±2 ms je Fenster. **Wie gemessen:** dieselben Benchmarks wie in Stufe 7 und 9 (`search::bench`,
`comfort::bench`, `mail-store`s `bench_100k_headers_and_list_pages`). Neu in `search::bench`:

- Vor dem Kindprozess wartet der Benchmark, bis die einmalige Nachholarbeit fertig ist
  (Kategorien, Kontaktverzeichnis, Threads und Index), und meldet, wie lange das noch dauerte. Der
  Generator speichert wie in Stufe 7 ohne Einordnung; danach fällt dieselbe Arbeit an wie nach dem
  Umstieg von einer Version vor Stufe 9 (Kategorien) bzw. nach jedem Erstabgleich (Kontakte).
  Bisher lief der Kindprozess je nach Dauer der Messungen mitten in diese Arbeit. Der Warmstart wird
  so wieder wie in Stufe 7 gemessen, als es diese Aufträge noch nicht gab.
- `child_measure` meldet den residenten Speicher auch direkt nach Start und erster Seite und was an
  Hintergrundarbeit offen ist. Es scheitert, wenn der Start allein mehr als 40 MB resident macht
  (die Datenbank ist gemappt: der Start hat sie breit gelesen), die erste Seite länger als 1 s
  braucht oder der Leerlauf über 250 MB liegt.
- Die Ursachen wurden mit einer vorübergehenden Zeitmessung jedes Schritts von `AppCore::start`
  (nicht im Code geblieben) im Kindprozess auf Kopien der 100.000er-Datenbank gesucht, in drei
  Zuständen: direkt nach dem Generator, nach den Messungen (Nachholarbeit offen), nach erledigter
  Nachholarbeit.

### Ergebnis gegen die Ziele

| Ziel (`docs/SPEC.md`) | Stufe 7 (100.000 / 500.000) | vorher | nachher | erfüllt |
|---|---|---|---|---|
| Suche bis erste Treffer unter 200 ms bei 100.000 Mails | 0,24–4,3 / 0,26–16,5 ms | 0,24–4,98 / 0,24–19,5 ms | 0,24–5,27 / 0,25–21,0 ms, schlechteste p95 5,44 / 21,3 ms | ja |
| Wechsel zwischen lokal vorhandenen Mails unter 100 ms | 0,36 / 0,39 ms | 0,34 / 0,35 ms | 0,39 / 0,37 ms (p95 0,51 / 0,55) | ja |
| Warmstart bis benutzbarer Posteingang unter 1 s | 2 / 2 ms | 13 / 14 ms | 5 / 5 ms (Kern gestartet nach 4 ms) | ja (nur Kern) |
| Arbeitsspeicher im Leerlauf unter 250 MB | 21 / 21–54 MB | 98 / 171 MB | 25 / 25 MB | ja (nur Kern) |

Suche und Mail öffnen: Median aller Anfragen bzw. von 200 Mails. Einschränkungen wie in Stufe 7:
nur der Kern (keine WebView, kein Rendern), kein Kaltstart nach Neustart des Rechners.

Listen, Threads und Suchen sind vorher und nachher gleich (Abweichungen 0–15 %, beim zweiten Lauf
war die Last höher; diese Pfade haben sich nicht geändert). Gegenüber Stufe 7 sind die Listen um
0,1–0,4 ms gewachsen (Listenseite 0,10 → 0,18 ms, vereinter Posteingang 0,47 → 0,71 ms); die
Ursache ist nicht getrennt gemessen, vermutlich die Markierungen je Zeile aus Stufe 9
(`comfort::bench`: 0,21 ms für 100 Zeilen samt Markierungen).

| Messung nachher (ms) | 100.000 Median | p95 | 500.000 Median | p95 |
|---|---:|---:|---:|---:|
| Listenseite größter Posteingang (100 Zeilen) | 0,18 | 0,22 | 0,19 | 0,28 |
| … Keyset ab Mitte | 0,18 | 0,25 | 0,17 | 0,22 |
| Vereinter Posteingang, 5 Konten (100) | 0,71 | 0,89 | 0,71 | 1,08 |
| Thread-Liste größter Posteingang (100) | 0,39 | 0,51 | 0,40 | 0,54 |
| … Keyset ab Mitte | 0,43 | 0,52 | 0,42 | 0,55 |
| Thread-Liste vereinter Posteingang (100) | 1,79 | 2,01 | 2,01 | 2,44 |
| Thread-Liste Newsletter (langer Listen-Thread) | 0,92 | 1,17 | 4,60 | 5,02 |
| Thread öffnen (1.461 bzw. 7.620 Mails) | 3,37 | 5,67 | 3,93 | 5,64 |
| Suche häufiges Wort `rechnung` | 0,27 | 0,43 | 0,31 | 0,59 |
| sehr häufiges Wort `und` | 0,28 | 0,37 | 0,26 | 0,41 |
| seltenes Wort | 0,24 | 0,40 | 0,26 | 0,44 |
| beim Tippen, Präfix `rech` | 1,02 | 1,14 | 2,92 | 3,10 |
| Phrase `"viele grüße"` | 0,36 | 0,47 | 0,37 | 0,52 |
| zwei Wörter | 0,32 | 0,37 | 0,33 | 0,48 |
| `from:` Nachname | 0,27 | 0,37 | 0,26 | 0,38 |
| Wort + `account:` (kleinstes Konto) | 0,83 | 1,25 | 0,75 | 1,25 |
| Wort + `in:inbox is:unread` | 0,43 | 0,57 | 0,43 | 0,69 |
| Wort + `newer_than:1y` | 0,27 | 0,34 | 0,28 | 0,33 |
| `filename:rechnung has:attachment` | 0,25 | 0,32 | 0,25 | 0,39 |
| ohne Text: `is:unread in:inbox` | 0,35 | 0,52 | 0,38 | 0,64 |
| nur Ausschluss `-und` | 5,27 | 5,44 | 20,95 | 21,34 |
| `rechnung OR invoice` | 0,34 | 0,37 | 0,37 | 0,46 |
| Mail öffnen, Body lokal (`getMessage`, HTML) | 0,39 | 0,51 | 0,37 | 0,55 |

### Start und Speicher: Ursachen

**1. Der Start las die ganze Tabelle `message` (Hauptursache).** `comfort::start` fragt bei jedem
Start `category_work_pending` (`SELECT EXISTS (… FROM message WHERE category IS NULL)`). Der
Teilindex `message_category_todo` ist nach dem Nachholen leer, aber `PRAGMA optimize` beim
Schließen hatte seine Größe aufgenommen, als noch alle Mails ohne Kategorie waren (`sqlite_stat1`:
100.000 Zeilen). Die Zahl der Mails ändert sich danach nicht, also frischt `PRAGMA optimize` die
Statistik nicht auf, und der Planer wählt `SCAN message`: Um keine Zeile zu finden, liest er die
ganze Tabelle, deren Seiten über `mmap` resident bleiben. Im Produkt kann das jede Datenbank
treffen, die von vor Stufe 9 kommt (alle Mails ohne Kategorie, bis der Auftrag sie einordnet); im
Benchmark trifft es jede, weil der Generator ohne Einordnung speichert.

| 100.000 Mails, Nachholarbeit erledigt, Kindprozess | vorher | nachher |
|---|---:|---:|
| Schritt `comfort::start` (frische Kopie, Seiten nicht im Cache) | 279 ms | 0,07–0,5 ms |
| resident nach Start und erster Seite | 88 MB | 16 MB |
| resident im Leerlauf | 92–94 MB | 25 MB |

Im Benchmark (Seiten warm im Cache) kostete derselbe Scan rund 11 ms: daher 13 statt 2 ms Start
und 98 MB im Leerlauf bei einer Datenbank von 98 MB. Bei 500.000 Mails begrenzt `mmap_size`
(256 MiB) den gemappten Teil, daher 171 MB.

**2. Nachholarbeit beim Start hielt den Schreib-Thread.** Fand der Start Mails ohne Kategorie,
schrieb der Kategorie-Auftrag sofort (500 Mails je Schritt), und die Schreibvorgänge des Starts
selbst warteten dahinter: Schritt „verpasste geplante Sendungen“ (Stufe 10) 114–128 ms, Start
121–134 ms, erste Seite 121–134 ms nach Beginn (Kopie nach den Messungen, 79.000 Mails ohne
Kategorie). Nachher auf denselben Kopien (kalt, Load 5,5): Start 7–8 ms, erste Seite 16–17 ms.

**3. Einmalige Nachholarbeit im Leerlauf.** Direkt nach dem Generator ordnete der
Kategorie-Auftrag in den 2 s Leerlauf rund 96.000 Mails ein; der residente Speicher stieg dabei
gleichmäßig von 24 auf 142 MB (nachher gleich, 123–144 MB). Der Kontakt-Auftrag beginnt 2 s nach
dem Start. Nach den Messungen dauerte die restliche Nachholarbeit noch 3,8 s (100.000) bzw. 12,1 s
(500.000). Das fällt einmal je Datenbank an und läuft im Hintergrund; Listen und Suche bleiben
dabei benutzbar.

**4. Was der Warmstart nachher kostet** (100.000 Mails, Nachholarbeit erledigt, warm, zwei Läufe;
zusammen 4,5–5,7 ms):

| Schritt von `AppCore::start` | ms |
|---|---:|
| Datenbank öffnen (Prüfverbindung, Schreib- und zwei Leseverbindungen) | 2,7–3,5 |
| Einstellungs-Sync: Zustand lesen, Erfassung (Stufe 11) | 0,1 |
| Stufe 9 anlegen (Zeitzone des Systems 0,9–1,8, Abmelde-Client 0,05–0,17) | 0,9–1,1 |
| Suche: offene Arbeit nachsehen (Stufe 7, FTS-Auftrag, Threads) | 0,05 |
| Stufe 9 starten: offene Kategorien, nächster Snooze | 0,07 |
| verpasste geplante Sendungen (Stufe 10) | 0,06–0,08 |
| Konten starten | 0,05 |
| Outbox fortsetzen | 0,3–0,4 |
| Entwürfe, Kontakt-Auftrag (Stufe 5/10) | 0,1 |
| MCP-Host (Stufe 12) | 0,06 |
| Einstellungs-Sync-Worker (Stufe 11) | 0,01–0,04 |
| alles Übrige (Lesen, Outbox anlegen, Ordner der geöffneten Anhänge leeren) | < 0,1 |

Das Öffnen ist teurer als in Stufe 7, weil jede der vier Verbindungen das Schema liest: 161
Objekte (48 Tabellen, 60 Indizes, 53 Trigger, 41 KB SQL) gegenüber 5 Migrationen damals. KI
(Stufe 13) und Oberflächen-Einstellungen tun beim Start nichts; sie lesen erst, wenn die
Oberfläche fragt.

### Kosten je Abgleichsfenster

`mail-store`-Benchmark, 100.000 Kopfdaten in 200 Fenstern ohne Einordnung, je zwei Läufe; für die
Zuordnung wurde vor dem Speichern jeweils ein Trigger bzw. Index entfernt:

| Stand | ms je Fenster |
|---|---:|
| Stufe 7 | 10,3–10,4 |
| vorher | 13,8–14,1 |
| … ohne Trigger `loc_category_new` | 12,0–12,3 |
| … ohne Index `loc_category` | 12,8–13,5 |
| … ohne Teilindizes `message_category_todo`, `message_contacts_todo` | 13,2–13,8 |
| … ohne alle vier | 11,3–11,8 |
| nachher (Load 3,3) | 14,3–14,4 |

- Größter Einzelposten ist der Trigger `loc_category_new` (Stufe 9): Er kopiert Kategorie und
  Wichtig-Markierung mit einem `UPDATE` auf jeden neuen Ort, 1,8 ms je Fenster. Der Index
  `loc_category` kostet 0,6–1,0 ms, die beiden Teilindizes der Nachhol-Aufträge 0,3–0,6 ms. Der
  Rest gegenüber Stufe 7 (rund 1 ms) ist nicht getrennt gemessen (mehr Spalten je Nachricht, Last).
- Nachher bekommt der Ort einer neu eingeordneten Nachricht die Werte im `INSERT`, der Trigger
  läuft dann nicht. Ohne Klassifizierer (dieser Benchmark, `search::bench`, eine Datenbank von vor
  Stufe 9) bleibt der Weg gleich; die Werte nachher liegen in der Schwankung.
- Die Einordnung (`comfort::bench`, mit gegen ohne Klassifizierer) kostete vorher 2,3 ms je
  Fenster (13,69 / 15,97 ms). Im selben Binary abwechselnd gemessen, drei Paare: mit Einordnung
  nachher 15,3 / 15,8 / 15,9 ms, vorher 17,5 / 18,1 / 16,8 ms (ohne Einordnung 13,7–14,6 ms).
  Die Einordnung selbst kostet damit 1,6–1,7 ms je Fenster (Kopfzeilen-Block parsen, eine
  Kontakt- und bis zu zehn Antwort-Abfragen je Mail); weniger ginge nur am Klassifizierer selbst,
  das bleibt.
- `search::bench` (fünf Konten, ohne Einordnung): Stufe 7 18,5 / 29,2 ms, vorher 22,2 / 37,0 ms,
  nachher 25,1 / 35,4 ms je Fenster (100.000 / 500.000). Der Weg ist vorher und nachher derselbe
  (Mails ohne Kategorie, der Trigger läuft); dass 100.000 höher und 500.000 niedriger liegt, ist
  Schwankung, wie der `mail-store`-Benchmark auf demselben Weg zeigt (13,8–14,1 gegen 14,3–14,4 ms).

### Behoben

1. **Teilindex fest:** Die Abfragen auf Mails ohne Kategorie nennen `INDEXED BY
   message_category_todo`, wie `CONTACTS_TODO` schon `message_contacts_todo`. Plan-Test
   `stage9_plan_tests::stale_statistics_of_the_unclassified_index_do_not_cause_a_scan` (Statistik
   bei 3.000 Mails ohne Kategorie, danach alle eingeordnet, neu geöffnet; scheitert ohne
   `INDEXED BY` mit `SCAN message`).
2. **Nachholarbeit nach dem Start:** Kategorie-Auftrag (Stufe 9) und Such-Auftrag (Stufe 7)
   beginnen erst am Ende von `AppCore::start`. Tests
   `comfort::tests::the_core_classifies_older_messages_after_its_start`,
   `search::tests::an_index_job_found_at_start_runs_after_the_start`.
3. **Kategorie gleich beim Einfügen:** Der Ort einer neuen Nachricht bekommt Kategorie und Wichtig
   im `INSERT`; der Trigger greift nur noch für zusammengeführte Orte und Mails ohne Kategorie.
   Tests `stage9_tests::new_locations_take_the_category_without_the_trigger` (Trigger entfernt;
   scheitert mit dem alten `INSERT`), `merged_locations_take_category_and_important_mark`,
   `unclassified_locations_follow_the_later_classification`.
4. **Schwellen im Benchmark:** `child_measure` scheitert über 40 MB Zuwachs durch den Start (vorher
   80 MB, nachher 8 MB), über 1 s bis zur ersten Seite und über 250 MB im Leerlauf.

### Bleibt

- **Öffnen der Datenbank 2,7–3,5 ms**, weil vier Verbindungen das gewachsene Schema lesen. Die
  Leseverbindungen ließen sich nebeneinander öffnen; bei einem Ziel von 1 s lohnt das nicht.
- **Zeitzone des Systems 0,9–1,8 ms** beim Anlegen von Stufe 9. Die erste Posteingangsseite mit
  Datumsabschnitten braucht sie ohnehin.
- **Schreiben je Fenster** bleibt um `loc_category` und die beiden Teilindizes teurer als in
  Stufe 7 (zusammen 1–1,5 ms); sie tragen die Listen nach Kategorie und die Nachhol-Aufträge.
  Ohne Klassifizierer läuft der Trigger weiter.
- **Einmalige Nachholarbeit** nach dem Umstieg bzw. einem Erstabgleich: läuft im Hintergrund und
  macht dabei gemappte Seiten resident (siehe 3.).
- `-und` bei 500.000 Mails (rund 20 ms Median) und der Zähler am Symbol (2,3 ms) sind unverändert.

**Ungeprüft:** wie in Stufe 7 (Kaltstart nach Neustart, die App mit WebView, echte Postfächer,
Windows).

## Konversationen (Entscheidung 0039) – 23.11.2026

**Umgebung:** wie Stufe 7 (Mac mini M4, macOS 27.2.1, Release-Build, warm). Andere Sitzungen bauten
und testeten nebenher (Load 5–8 während der Messung); die Grundwerte dieses Laufs liegen deshalb
etwa doppelt so hoch wie in Stufe 7 (Listenseite 0,19 statt 0,10 ms). **Wie gemessen:** der
Benchmark der Stufe 7 (`crates/mail-engine/src/search/bench.rs`, gleiche Daten: fünf Konten,
40 % Antworten auf eine der letzten 300 Unterhaltungen, langer Listen-Thread) mit neuen Zeilen für
den sortierten Posteingang als Konversationen; alles über `AppCore`.

```text
cargo test -p mail-engine --release --lib search::bench -- --ignored --nocapture --test-threads 1
```

| Messung (ms) | 100.000 Median | p95 | max | 500.000 Median | p95 | max |
|---|---:|---:|---:|---:|---:|---:|
| Listenseite größter Posteingang (100) | 0,19 | 0,24 | 0,38 | 0,20 | 0,28 | 0,37 |
| Sortierter Posteingang, Nachrichten (`listInbox`, 100) | 0,39 | 0,43 | 0,73 | 0,39 | 0,45 | 0,80 |
| **Sortierter Posteingang, Konversationen (100)** | **2,18** | 2,39 | 2,57 | **2,21** | 2,46 | 2,61 |
| … Keyset ab Mitte | 2,12 | 2,22 | 2,33 | 2,08 | 2,26 | 2,41 |
| **Vereinter Posteingang, Konversationen (5 Konten, 100)** | **4,49** | 4,81 | 6,17 | **4,64** | 4,89 | 6,20 |
| Nur ungelesene, Konversationen (100) | 1,64 | 2,13 | 2,30 | 1,88 | 2,12 | 2,18 |
| Ungelesene oben, Konversationen (100) | 1,49 | 1,97 | 2,21 | 1,87 | 2,09 | 2,15 |
| Ziele einer Aktion auf 100 Konversationen (Archivieren) | 1,49 | 1,84 | 1,88 | 1,69 | 1,84 | 1,88 |
| Thread-Liste größter Posteingang (100) | 1,84 | 1,97 | 2,02 | 1,88 | 2,11 | 2,29 |
| Thread-Liste vereinter Posteingang (100) | 2,54 | 2,78 | 3,22 | 2,59 | 2,99 | 3,34 |
| Thread-Liste Newsletter (langer Listen-Thread, 1.461 bzw. 7.620 Mails) | 2,62 | 2,81 | 3,40 | 6,77 | 7,33 | 26,37 |

- Eine Seite Konversationen bleibt bei 100.000 und 500.000 Mails unter 3 ms (vereint unter 5 ms),
  weit unter jedem Ziel; die Seite wächst nicht mit der Größe des Postfachs (Keyset, Plan-Test
  `conversation_tests::every_conversation_query_uses_its_index`).
- **Kosten gegenüber Mails:** rund 2 ms mehr je Seite. Sie kommen aus der Zusammenfassung je Zeile
  (Anzahl und Ungelesene über alle Mails des Threads, Absender und Markierungen über die neuesten
  200) und der Prüfung „keine neuere Mail des Threads“. Die Threads dieses Generators sind groß
  (die letzten 300 Unterhaltungen teilen sich 40 % aller Mails, im Mittel weit über 100 Mails je
  Thread); echte Unterhaltungen sind meist kürzer, der Wert ist eine Obergrenze.
- **Thread-Liste eines Ordners:** nutzt jetzt dieselbe Zusammenfassung (mit Absendern, Anhang und
  Markierung) und kostet deshalb mehr als in Stufe 7 (0,26 → 1,84 ms bei 100 Zeilen). Ein erster
  Entwurf las für Absender und Markierungen jede Mail des Threads: die Zeile des langen
  Listen-Threads kostete bei 500.000 Mails 18,3 ms (Median); mit der Grenze von 200 Mails 6,8 ms.
  Das Maximum von 26 ms ist ein einzelner Ausreißer unter Last (p95 7,3 ms).
- Nicht berührt (gleiche Abfragen wie vorher; die höheren Werte als in Stufe 7 sind die Last):
  Listenseite, vereinter Posteingang als Mails, Suche, Start (4–9 ms bis zur ersten Seite) und
  Speicher im Leerlauf (26–27 MB).
- Der automatische Test `search::bench::small_data_meets_generous_thresholds` (3.000 Mails, Debug)
  prüft jetzt auch den Posteingang als Konversationen (Grenze 200 ms) und vereint (300 ms).

## Inline-Bilder neu kodieren (Sicherheitsprüfung C-4) – 23.11.2026

**Frage:** Was kostet es, die eingebetteten (`cid:`) und freigegebenen externen Bilder einer Mail
durch die Pipeline der Anhangsvorschau zu schicken (in Rust dekodieren, aufrichten, auf höchstens
2048 px skalieren, PNG/JPEG neu kodieren), statt ihre Bytes an die Bilddecoder der WebView zu geben?

**Umgebung:** Mac mini, Apple M4 (10 Kerne), andere Sitzungen bauten gleichzeitig (Load 6–8), die
Werte sind also Obergrenzen. Ein Kern, nacheinander, bester von fünf Durchläufen.

**Wie gemessen:** `crates/mail-content/tests/inline_reencode_cost.rs`

```text
cargo test -p mail-content --test inline_reencode_cost -- --ignored --nocapture
cargo test -p mail-content --test inline_reencode_cost --release -- --ignored --nocapture
```

Ein typischer HTML-Newsletter, im Test erzeugt: 32 Bilder, 7,1 Megapixel, 1.215 KiB. Ein
Titelbild 1200×600 und vier Artikelbilder 1200×800 (2x für Retina), sechs Produktfotos 600×600,
zwei Banner 600×200 (alle JPEG, Qualität 82, fotoähnlicher Inhalt), ein Logo 400×120 und zehn
Symbole 96×96 als PNG mit Transparenz, sechs Social-Symbole 64×64, zwei 1×1-GIFs.

| Build | heute (erkennen, Base64) | neu kodiert | langsamstes Bild |
|---|---|---|---|
| Debug (Abhängigkeiten nicht optimiert) | 20 ms | 1.683 ms | 234 ms (Artikelbild 1200×800) |
| Release | 1,2 ms | 68–69 ms | 9,5 ms (Artikelbild 1200×800) |

- Neu kodiert sind die `data:`-URLs etwa gleich groß (1.647 statt 1.621 KiB).
- Die Kosten wachsen mit den Pixeln: rund 9,5 ms je Megapixel-Foto (Release, M4), auf einem
  langsameren Windows-Laptop eher das Drei- bis Fünffache. Sie fielen beim ersten Öffnen jeder
  bildreichen Mail auf dem Weg zur Anzeige an, ohne Cache bei jedem Öffnen.
- Die Pipeline dekodiert nur das erste Bild: Animierte GIFs und WebP würden still stehen. Sie zu
  erhalten hieße, jedes Einzelbild neu zu kodieren (Kosten mal Bildzahl), animiertes WebP kann
  der `image`-Encoder nicht schreiben.

**Ergebnis:** nicht umgesetzt. 70 ms je Newsletter auf dem M4 (ein Mehrfaches auf langsamer
Hardware, 1,7 s im Debug-Build) auf dem Weg zum Öffnen einer Mail, dazu ein neuer Cache und der
Verlust von Animationen, sind nicht eindeutig vertretbar. Umgesetzt ist nur der kleine Teil:
BMP, AVIF und ICO werden nicht mehr angezeigt. Wie es weitergehen kann, steht in
`docs/SECURITY_MODEL.md` (C-4).
