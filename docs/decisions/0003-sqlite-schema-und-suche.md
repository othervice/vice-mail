# 0003 – SQLite: Schema, Indizes, Pragmas und Volltextsuche

**Status:** entschieden (Spike B, Stufe 0) · **Datum:** 22.11.2026 · **Code:** `spikes/sqlite/` (Wegwerf-Code,
Rohdaten aller Läufe in `spikes/sqlite/results/`)

## Kontext

Die Architektur setzt auf SQLite über `rusqlite` (bundled) mit WAL und FTS5, ein Schreiber, mehrere
Leser. Stufe 0 sollte prüfen, ob das bei 500.000 Mails über fünf Konten die Ziele trägt
(Listenseite unter 20 ms, Suche unter 200 ms), und Schema, Indizes, Pragmas und Tokenizer festlegen.

**Versionen geprüft (crates.io, 22.11.2026):** `rusqlite` 0.43.0 (15.11.2026, MIT) ist die aktuelle
Version. Sie zieht `libsqlite3-sys` 0.40.0 mit **SQLite 3.54.1**. Das Feature `bundled` übersetzt mit
`SQLITE_ENABLE_FTS5`, `SQLITE_ENABLE_DBSTAT_VTAB`, `SQLITE_ENABLE_STAT4`, `SQLITE_ENABLE_JSON1`,
`SQLITE_DEFAULT_FOREIGN_KEYS=1` und `SQLITE_THREADSAFE=1`. FTS5 ist also enthalten, ebenso
`contentless_delete` (ab 3.43) und `trigram … remove_diacritics` (ab 3.45). Die Stack-Tabelle stimmt.
Einzige Abhängigkeit des Spikes ist `rusqlite` (MIT); Zufall, Zipf-Verteilung und Messung sind selbst geschrieben.

## Entscheidung

1. **Ziele erreicht, mit großem Abstand.** Listenseite (100 Zeilen) 0,04 ms im größten Postfach und
   0,14 ms im vereinten Posteingang, auf tiefen Seiten genauso. Erste 50 Suchtreffer „neueste zuerst“
   0,08–0,19 ms, Präfix 12 ms, alle Filterkombinationen unter 1 ms (Ziele: 20 bzw. 200 ms).
2. **Tokenizer: `unicode61 remove_diacritics 2`.** Der Trigram-Index ist 5,2-mal so groß (1,15 GB statt
   222 MB), das Einfügen dauert doppelt so lange, und er bringt nur die Teilwortsuche. Die bleibt für
   Version 1 außen vor (Begründung und Rückweg unten).
3. **`message.id` ist zeitlich geordnet:** das Sortierdatum in Unix-Sekunden, bei Kollision die nächste
   freie Zahl darüber. Damit ist `ORDER BY id DESC` „neueste zuerst“ – für Listen und für FTS5, dessen
   `rowid` gleich `message.id` ist. Suchtreffer kommen ohne Sortierung in Datumsreihenfolge, die Suche
   bricht nach 50 Treffern ab, ein Datumsfilter ist ein `rowid`-Bereich. Das ist der wichtigste Baustein.
   Dichte Zahlen halten den Index klein (Sekunden statt Sekunden × 2^20: 222 statt 303 MB).
4. **`message_location` ist `WITHOUT ROWID` mit Schlüssel `(mailbox_id, message_id)`**: eine Listenseite
   ist ein zusammenhängender Bereich samt Flags. Ein Datum in `message_location` braucht es nicht, die
   `message_id` ist das Datum.
5. **FTS5 contentless (`content=''`, `contentless_delete=1`)**: der Index hält nur Tokens, keinen Text
   (spart 233 MB gegenüber einer Kopie). Kommt der Body später, wird die Zeile per `INSERT OR REPLACE`
   neu indexiert. Hervorhebungen im Treffer berechnet Rust aus Betreff, Vorschau bzw. Body-Datei.
6. **Keyset-Paginierung immer, `OFFSET` nie. Vereinte Ansichten als `UNION ALL` je Postfach**, nie als
   `mailbox_id IN (…)`: dafür wählt der Planer trotz Statistik einen Plan mit 400 ms.
7. **Erste Suchtreffer immer nach `rowid DESC`.** `bm25` und Sortieren nach `m.date` kosten bei sehr
   häufigen Wörtern 140–450 ms und verfehlen das Ziel, sobald viel Text im Index liegt.

## Schemaentwurf

Nur die Tabellen, die der Spike gemessen hat; `identity`, `thread`, `operation`, `draft` usw. wie in
`docs/ARCHITECTURE.md`.

```sql
CREATE TABLE account (
  id             INTEGER PRIMARY KEY,
  uuid           TEXT NOT NULL UNIQUE,
  address        TEXT NOT NULL,
  display_name   TEXT NOT NULL,
  color          TEXT,
  writes_enabled INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE mailbox (
  id            INTEGER PRIMARY KEY,
  account_id    INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  name          TEXT NOT NULL,
  delimiter     TEXT,
  parent_id     INTEGER REFERENCES mailbox(id) ON DELETE CASCADE,
  role          TEXT,              -- inbox, sent, drafts, archive, trash, junk, all; NULL = eigener Ordner
  uidvalidity   INTEGER,
  uidnext       INTEGER,
  highestmodseq INTEGER,
  UNIQUE (account_id, name)
) STRICT;
CREATE INDEX mailbox_parent ON mailbox(parent_id);

-- Eine Zeile je Mail, auch wenn sie in mehreren Ordnern liegt.
-- id = Sortierdatum in Unix-Sekunden; ist die Zahl belegt, die nächste freie darüber.
-- Größer = neuer. id < 2^33, in JavaScript exakt darstellbar. Die id ändert sich nie.
CREATE TABLE message (
  id              INTEGER PRIMARY KEY,
  account_id      INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  message_id_hdr  TEXT,
  thread_id       INTEGER,
  date            INTEGER NOT NULL,  -- Sortierdatum (Unix-Sekunden), zur Anzeige
  subject         TEXT NOT NULL,
  from_name       TEXT,
  from_addr       TEXT,
  to_addrs        TEXT,
  cc_addrs        TEXT,
  size            INTEGER NOT NULL,
  preview         TEXT,
  has_attachments INTEGER NOT NULL DEFAULT 0,
  category        INTEGER,
  body_ref        TEXT               -- Datei des komprimierten Bodys, NULL = nicht lokal
) STRICT;
CREATE INDEX message_msgid  ON message(account_id, message_id_hdr);  -- Abgleich, Threads, FK auf account
CREATE INDEX message_thread ON message(thread_id);

-- Wo eine Mail liegt. Flags gehören zum Ort, wie bei IMAP.
CREATE TABLE message_location (
  mailbox_id  INTEGER NOT NULL REFERENCES mailbox(id) ON DELETE CASCADE,
  message_id  INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  uid         INTEGER NOT NULL,
  seen        INTEGER NOT NULL DEFAULT 0,
  flagged     INTEGER NOT NULL DEFAULT 0,
  answered    INTEGER NOT NULL DEFAULT 0,
  draft       INTEGER NOT NULL DEFAULT 0,
  deleted     INTEGER NOT NULL DEFAULT 0,
  keywords    TEXT,
  modseq      INTEGER,
  PRIMARY KEY (mailbox_id, message_id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX loc_uid ON message_location(mailbox_id, uid);                     -- Abgleich per UID
CREATE INDEX loc_message ON message_location(message_id);                              -- Mail -> Orte, FK-Kaskade
CREATE INDEX loc_unseen  ON message_location(mailbox_id, message_id) WHERE seen = 0;   -- Zähler, Filter

CREATE TABLE attachment (
  id          INTEGER PRIMARY KEY,
  message_id  INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  part        TEXT NOT NULL,
  filename    TEXT,
  mime_type   TEXT,
  size        INTEGER,
  content_id  TEXT
) STRICT;
CREATE INDEX attachment_message ON attachment(message_id);

-- Volltext: nur Tokens, kein Text. rowid = message.id.
-- Stufe 7 teilt sender/recipients in from/to/cc auf (siehe unten).
CREATE VIRTUAL TABLE message_fts USING fts5(
  subject, sender, recipients, body, attachments,
  content='', contentless_delete=1,
  tokenize="unicode61 remove_diacritics 2"
);
CREATE TRIGGER message_fts_delete AFTER DELETE ON message BEGIN
  DELETE FROM message_fts WHERE rowid = old.id;
END;
```

Boolesche Flags als eigene Spalten kosten nichts: SQLite speichert 0 und 1 ohne Nutzdaten, nur im
Kopf des Datensatzes. Sie erlauben Teilindizes wie `loc_unseen`.

**Indizes und wofür:** `message` (rowid) für Listen-Join und Suche; `message_msgid` zum Zusammenführen
per Message-ID und als Index für den Fremdschlüssel auf `account`; `message_thread` für Threads;
`message_location` (Schlüssel) für Listenseiten und `count(*)`; `loc_uid` für den Abgleich (Flags,
Löschungen per UID); `loc_message` für „Orte einer Mail“ und das kaskadierende Löschen;
`loc_unseen` für Ungelesen-Zähler und den Filter „nur ungelesen“; `attachment_message` für Anhänge
und die Kaskade. Größen bei 500.000 Mails: `message` 230 MB, `message_msgid` 29 MB,
`message_location` 25 MB, `loc_uid` 14 MB, `attachment` 12 MB, `message_thread` 7 MB,
`loc_message` 6 MB, `attachment_message` 2 MB, `loc_unseen` unter 1 MB.

**Regel für das Zusammenführen per Message-ID (Stufe 2):** Ein weiterer Ort derselben Mail wird nur
zusammengeführt, wenn Konto und Message-ID gleich sind und die Mail in diesem Postfach noch nicht
liegt. Doppelte Message-IDs im selben Postfach (Mailingliste plus Kopie) und Mails ohne Message-ID
bekommen eine eigene `message`-Zeile; sonst verletzt der Schlüssel `(mailbox_id, message_id)` die
Eindeutigkeit.

## Abfragen

Alle mit `EXPLAIN QUERY PLAN` geprüft (vollständig in `spikes/sqlite/results/`): kein Scan von
`message` oder `message_location`, kein temporärer B-Tree über ein ganzes Postfach.

```sql
-- Listenseite eines Postfachs. Erste Seite: ?2 = 9223372036854775807, danach die letzte id der Seite.
SELECT l.message_id, l.uid, l.seen, l.flagged, l.answered,
       m.subject, m.from_name, m.from_addr, m.date, m.preview, m.has_attachments, m.thread_id, m.account_id
FROM message_location l CROSS JOIN message m ON m.id = l.message_id
WHERE l.mailbox_id = ?1 AND l.message_id < ?2
ORDER BY l.message_id DESC LIMIT 100;
-- Plan: SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?) | SEARCH m USING INTEGER PRIMARY KEY

-- Nur ungelesen: zusätzlich "AND l.seen = 0"  ->  SEARCH l USING INDEX loc_unseen

-- Vereinter Posteingang (n Postfächer): je Postfach die nächsten 100, dann mischen.
SELECT … FROM (
    SELECT * FROM (SELECT mailbox_id, message_id, uid, seen, flagged, answered FROM message_location
                   WHERE mailbox_id = ?1 AND message_id < ?k ORDER BY message_id DESC LIMIT 100)
    UNION ALL SELECT * FROM (… ?2 …) UNION ALL …
    ORDER BY message_id DESC LIMIT 100
  ) l CROSS JOIN message m ON m.id = l.message_id
ORDER BY l.message_id DESC;
-- Plan: je Postfach SEARCH USING PRIMARY KEY; SQLite sortiert dabei höchstens n × 100 Zeilen
-- (MERGE UNION ALL mit kleinem TEMP B-TREE je Zweig). Gleichwertig: n Einzelabfragen, in Rust mischen.

-- Ungelesen-Zähler: SELECT count(*) FROM message_location WHERE mailbox_id = ?1 AND seen = 0;
-- Seitenleiste:     SELECT mailbox_id, count(*) FROM message_location WHERE seen = 0 GROUP BY mailbox_id;
-- Plan: COVERING INDEX loc_unseen, Aufwand proportional zur Zahl der ungelesenen Mails.

-- Suche, erste 50 Treffer, neueste zuerst
SELECT m.id, m.subject, m.from_name, m.from_addr, m.date, m.preview, m.has_attachments, m.account_id
FROM message_fts CROSS JOIN message m ON m.id = message_fts.rowid
WHERE message_fts MATCH ?1
  [AND message_fts.rowid >= ?von AND message_fts.rowid < ?bis]           -- after:/before:/newer_than:
  [AND m.account_id = ?konto]                                            -- account:
  [AND EXISTS (SELECT 1 FROM message_location l
               WHERE l.message_id = m.id AND l.seen = 0)]                -- is:unread
ORDER BY message_fts.rowid DESC LIMIT 50;
-- Plan: SCAN message_fts VIRTUAL TABLE INDEX 192:M5 (FTS5 liefert rowid absteigend und bricht nach
--       50 Treffern ab) | SEARCH m USING INTEGER PRIMARY KEY
```

`CROSS JOIN` legt die Reihenfolge der Tabellen fest. Das ist Absicht: Mit `JOIN` und
`mailbox_id IN (SELECT id FROM mailbox WHERE role = 'inbox')` hat der Planer trotz `ANALYZE` (STAT4)
einen Plan gewählt, der `message` rückwärts liest und dann sortiert – 393 ms statt 0,14 ms; mit
anderem Startwert derselben Abfrage war er wieder schnell. `mail-store` braucht deshalb für jede Listen-
und Suchabfrage einen Test, der den Plan prüft.

## Pragmas und Verbindungen

```sql
-- Schreib-Verbindung (genau eine, eigener Thread)
PRAGMA page_size = 4096;              -- vor dem ersten CREATE; Standard
PRAGMA journal_mode = WAL;            -- bleibt in der Datei gespeichert
PRAGMA synchronous = NORMAL;          -- in WAL sicher gegen Beschädigung; nach Stromausfall fehlen
                                      -- höchstens die letzten Transaktionen (Cache, vom Server wiederholbar)
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -65536;           -- 64 MB; 256 MB brachten beim Laden nichts
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;         -- 256 MB
PRAGMA journal_size_limit = 67108864; -- WAL nach Checkpoint auf 64 MB kürzen
PRAGMA wal_autocheckpoint = 1000;     -- Standard
PRAGMA busy_timeout = 5000;

-- Lese-Verbindungen (kleiner Pool, 2–4)
PRAGMA query_only = 1;
PRAGMA cache_size = -8192;            -- 8 MB; die Seiten kommen über mmap
PRAGMA mmap_size = 268435456;
PRAGMA temp_store = MEMORY;
PRAGMA busy_timeout = 5000;
```

- Schreib-Transaktionen mit `BEGIN IMMEDIATE`, 500 Mails je Abgleichsfenster.
- FTS5-Zeilen **je Transaktion nach `rowid` aufsteigend einfügen.** FTS5 schreibt seinen Puffer jedes
  Mal als neues Segment weg, wenn eine `rowid` kleiner als die vorige ist (`sqlite3Fts5IndexBeginWrite`).
  Der erste Abgleich läuft aber von neu nach alt, also absteigend. Sortiert: 19.000 statt 12.900 Mails/s.
- Nach dem ersten Abgleich eines Kontos und danach regelmäßig `PRAGMA optimize` (führt `ANALYZE` aus,
  wenn nötig; ein volles `ANALYZE` dauerte 0,2–0,3 s, bei Trigram mit allen Bodies 4,7 s).
- Leser halten Transaktionen kurz (eine Abfrage). Mit dauernd lesendem Leser und dauernd schreibendem
  Schreiber wuchs das WAL auf 74 MB (Trigram: 243 MB), weil der Checkpoint es nicht zurücksetzen
  konnte. Der Schreiber sollte in Ruhephasen `PRAGMA wal_checkpoint(TRUNCATE)` ausführen.
- mmap an oder aus änderte Listen- und Suchzeiten nicht; volle Scans (OFFSET, LIKE) waren mit mmap
  17–30 % schneller. mmap bleibt an, damit der Leser-Cache klein bleiben kann: der Messprozess hatte
  nach allen Abfragen 51 MB RSS (inklusive gemappter Seiten). Wichtig für das Ziel „250 MB im Leerlauf“.
- `PRAGMA shrink_memory` auf allen Verbindungen, wenn die App in den Leerlauf geht (nicht gemessen).

## Tokenizer

| | unicode61 remove_diacritics 2 | trigram remove_diacritics 1 |
|---|---|---|
| Index, realistischer Umfang (233 MB Text) | **222 MB** | 1.150 MB |
| Index, Text aller Mails (424 MB Text)¹ | 451 MB | 2.682 MB |
| Einfügen 500k Mails, realistisch | 7.300 Mails/s | 3.900 Mails/s |
| Wort häufig / sehr häufig / selten, erste 50 | 0,09 / 0,10 / 0,08 ms | 0,48 / 0,08 / 3,0 ms |
| Präfix `rech*` | 12 ms (Wortanfang) | 0,17 ms (Teilstring `rech`, auch „berechnen“, „sprechen“) |
| Phrase `"vielen dank"` | 0,19 ms | 0,71 ms |
| Teilwort `auszug` in „Kontoauszug“ | nicht direkt; über `fts5vocab` 773 ms | 0,31 ms |
| Suchwörter unter 3 Zeichen („KI“, „Q3“) | normal | über den Index nicht auffindbar |
| `bm25` | sinnvoll (Wörter) | wenig aussagekräftig (Trigramme) |
| `über` = `uber` | ja | ja |
| `grüße` = `grusse` | nein (ß bleibt ß) | nein |

¹ gemessen mit `id` = Sekunden × 2^8; mit reinen Sekunden rund 12 % kleiner (siehe ID-Dichte).

**Gewählt: unicode61.** Ein Fünftel der Indexgröße, doppelte Einfügerate, Wort- und Präfixsuche wie in
Apple Mail, Gmail und Thunderbird, Suchwörter mit zwei Zeichen funktionieren, `bm25` ist brauchbar.
Die Teilwortsuche (deutsche Komposita) ist der einzige Vorteil von Trigram; `docs/SPEC.md` verlangt
sie nicht, und sie rechtfertigt den Preis für Version 1 nicht.

**Teilwortsuche später (Rückweg, ohne Schemabruch):**
- Kopfzeilen per `LIKE` auf `message` mit `ORDER BY id DESC LIMIT 50`: häufige Teilwörter in 0,4 ms,
  weil der Scan bei den neuesten Mails beginnt und nach 50 Treffern endet; ein seltenes Teilwort
  scannt alles (72 ms bei 500k). Taugt als Ergänzung für `subject:`/`from:`.
- Ein zweiter Trigram-Index nur über Kopfzeilen kostet 428 MB, mehr als der ganze unicode61-Index samt
  Text. Lohnt nicht.
- Ein eigener Tokenizer mit Kompositazerlegung über die FTS5-Tokenizer-API (`rusqlite` hat dafür keine
  Hülle, also über `ffi`) – nur, wenn Nutzer es vermissen. Derselbe Weg löst `ß`/`ss`; einfacher ist,
  die Suchanfrage um die zweite Schreibweise zu ergänzen (`grüße OR grüsse`).

## Messwerte

**Umgebung:** Apple M4 (10 Kerne), 16 GB RAM, macOS 27.2.1, SQLite 3.54.1, Release-Build. Parallel liefen
Builds anderer Sitzungen (Load 1,5–9, je Lauf in `spikes/sqlite/results/` vermerkt). Latenzen: Median
und p95 über bis zu 200 Wiederholungen (mindestens 10, höchstens 8 s), warm, über `prepare_cached`,
alle Spalten aller Zeilen gelesen. **Kalt** = neue Verbindung mit leerem SQLite-Cache bei warmem
Betriebssystem-Cache. Ein Kaltstart nach Neustart des Rechners (`purge` braucht sudo) ist **ungeprüft**.
Die Tabellen zeigen den letzten Lauf mit dem gewählten Entwurf (`results/final-u61.md`,
`results/final-tri.md`, Load 2,3–4,5). Die Lese-Latenzen anderer Läufe wichen meist um weniger als
15 % ab, einzelne um bis zu 25 %, die Einfügeraten um bis zu 30 % (Fremdlast).

### Listen und Zähler (500.000 Mails, ms)

| Abfrage | Median | p95 | kalt |
|---|---:|---:|---:|
| Größter Posteingang (124.854 Mails), Seite 1 | 0,043 | 0,045 | 0,12 |
| … Keyset ab Mitte | 0,043 | 0,044 | 0,11 |
| … Keyset ab Ende (letzte Seite) | 0,043 | 0,043 | 0,09 |
| … nur ungelesen, Seite 1 | 0,060 | 0,060 | 0,13 |
| … Gegenprobe `OFFSET` Mitte | 16,5 | 17,5 | 20,8 |
| … Gegenprobe `OFFSET` Ende | 35,9 | 36,5 | 41,3 |
| Vereinter Posteingang (5 Konten, 249.787 Mails), `UNION ALL`, Seite 1 | 0,138 | 0,143 | 0,28 |
| … Keyset ab Mitte | 0,138 | 0,143 | 0,24 |
| … Keyset ab Ende | 0,100 | 0,103 | 0,18 |
| … Gegenprobe `IN (…)`, Plan nach Wahl des Planers, Seite 1 | 393 | 398 | 396 |
| … Gegenprobe `IN (…)` über `INDEXED BY loc_message` | 0,088 | 0,091 | 0,19 |
| Ungelesen-Zähler größter Posteingang (`loc_unseen`) | 0,059 | 0,062 | 0,13 |
| … ohne Teilindex | 2,16 | 2,19 | 3,30 |
| Ungelesen-Zähler aller 76 Postfächer (`loc_unseen`) | 0,71 | 0,73 | 0,97 |
| … ohne Teilindex | 9,9 | 10,1 | 12,4 |
| Gesamtzahl größter Posteingang | 1,24 | 1,26 | 1,92 |

### Suche, erste 50 Treffer (500.000 Mails, realistischer Umfang, ms Median / p95)

| Suche | unicode61 | trigram |
|---|---:|---:|
| häufig `rechnung` | 0,094 / 0,097 | 0,478 / 0,494 |
| sehr häufig `und` | 0,097 / 0,100 | 0,084 / 0,088 |
| selten `laarzeso` (55 Mails) | 0,077 / 0,079 | 2,98 / 3,04 |
| Präfix `rech*` (trigram: Teilstring `rech`) | 12,4 / 12,6 | 0,165 / 0,169 |
| Phrase `"vielen dank"` | 0,186 / 0,190 | 0,712 / 0,733 |
| zwei Wörter `rechnung bestellung` | 0,174 / 0,178 | 1,18 / 1,21 |
| Umlaut `grüße` | 0,087 / 0,091 | 0,245 / 0,250 |
| Spalte `sender:müller` | 0,089 / 0,092 | 0,352 / 0,359 |
| Teilwort `auszug` | 773 (`fts5vocab`) + 4,1 | 0,305 / 0,314 |
| `rechnung` + Konto 4 (40k Mails) | 0,175 / 0,181 | 0,797 / 0,811 |
| `rechnung` + Konto 5 (10k Mails) | 0,540 / 0,549 | 1,56 / 1,61 |
| `rechnung` + letzte 365 Tage | 0,092 / 0,093 | 0,467 / 0,478 |
| `rechnung` + ungelesen | 0,150 / 0,155 | 0,537 / 0,546 |
| `rechnung` + Konto 4 + 365 Tage + ungelesen | 0,445 / 0,460 | 1,55 / 1,59 |
| `rechnung`, `ORDER BY rank` (bm25) | 35,8 / 36,3 | 132 / 133 |
| `und`, `ORDER BY rank` (bm25) | 144 / 145 | 153 / 155 |
| `rechnung`, `ORDER BY m.date` über den Join | 78 / 79 | 186 / 374 |
| `und`, `ORDER BY m.date` über den Join | 238 / 240 | 251 / 256 |
| Trefferzahl `count(*)` `rechnung` / `und` | 2,2 / 7,8 | 35,8 / 7,9 |
| ohne FTS: `LIKE '%…%'` auf Betreff/Absender, seltenes Wort | 72 / 73 | – |

Kalt (neue Verbindung): unicode61 0,2–0,5 ms (Präfix 13 ms), trigram 0,2–3,7 ms.

**Obergrenze, Text aller Mails (je 120 Wörter, 424 MB Text):** unicode61 „neueste zuerst“ 0,06–0,16 ms,
Präfix 22 ms, `bm25` für `und` 308 ms, `ORDER BY m.date` für `und` 453 ms, `fts5vocab`-Scan 1,4 s.
Trigram „neueste zuerst“ 0,1–2,9 ms, `bm25` 273–325 ms, `ORDER BY m.date` 348–466 ms. „Neueste zuerst“
bleibt also in jedem Fall weit unter dem Ziel, jede andere Reihenfolge verfehlt es.

**Nach 84.000 neuen Mails eines Kontos** (aus dem Schreiber-Test) stieg `rechnung` + Konto 4 von 0,18 auf
3,6 ms: Die neuesten Treffer gehören dann alle zu Konto 1 und werden übersprungen. Im schlimmsten Fall
(Wort häufig in einem Konto, selten im gefilterten) läuft die Suche über alle Treffer des Worts.
Abhilfe, falls es nötig wird: das Konto als Token in eine eigene FTS-Spalte (`account:` wird dann von
FTS5 selbst geschnitten).

### Größen und Einfügen (500.000 Mails, 532.598 Orte, 150.650 Anhänge, Fenster 500)

| Variante | Text im Index | FTS | Datei | Einfügen (nur DB-Zeit) |
|---|---:|---:|---:|---:|
| ohne FTS | – | – | 326 MB | 8.500 Mails/s |
| unicode61, realistisch | 233 MB | 222 MB | 550 MB | 7.300 Mails/s (andere Läufe 5.900) |
| trigram, realistisch | 233 MB | 1.150 MB | 1.480 MB | 3.900 Mails/s |
| unicode61, Text aller Mails¹ | 424 MB | 451 MB | 802 MB | 5.700 Mails/s |
| trigram, Text aller Mails¹ | 424 MB | 2.682 MB | 3.037 MB | 1.800 Mails/s |
| trigram nur Kopfzeilen¹ | 70 MB | 428 MB | 764 MB | 6.000 Mails/s |

¹ mit `id` = Sekunden × 2^8 gemessen. WAL beim Laden höchstens 13 MB (unicode61) bzw. 43 MB (trigram),
nach dem Checkpoint 0. Die erzeugten Daten brauchen rund 9–11 s je Lauf und sind nicht mitgezählt.

**ID-Dichte (unicode61, realistisch):** FTS5 speichert in jeder Trefferliste den Abstand zur vorigen
`rowid` als Varint. `id` = Sekunden: FTS 222 MB, Datei 550 MB. Sekunden × 2^8: 254 / 591 MB.
Sekunden × 2^20: 303 / 649 MB. Darum reine Sekunden mit „nächste freie Zahl“ bei Kollision
(617 Kollisionen bei 500.000 Mails).

**Einfügen im Detail (50.000 Mails):** ohne FTS 28.800 / 39.800 / 44.800 Mails/s bei 100 / 500 / 2.000
je Transaktion; unicode61 13.600 / 19.000 / 21.000; unicode61 unsortiert 12.900; trigram 6.800. Bei
500.000 Mails fällt die Rate ohne FTS auf 8.500/s. Vermutete Ursache (nicht weiter untersucht): Indizes
mit zufälligen Schlüsseln (`message_msgid`) treffen je Zeile eine andere Seite, jedes Fenster schreibt
Tausende Seiten ins WAL. 256 statt 64 MB Schreib-Cache änderten nichts (8.200 statt 8.500, 6.200 statt
7.300 Mails/s). Für den ersten Abgleich reicht das: 500.000 Mails brauchen gut eine Minute DB-Zeit.

### Lesen während eines laufenden Schreibers (unicode61, je 20 s)

Ein Thread liest im Kreis die fünf Abfragen unten, ein anderer schreibt ohne Pause: je Transaktion 100
neue Mails samt FTS mit Body, dann 200 Flag-Änderungen.

| Abfrage | allein: Median / p95 / p99 / max ms | mit Schreiber: Median / p95 / p99 / max ms |
|---|---|---|
| Posteingang Seite 1 | 0,04 / 0,05 / 0,05 / 0,16 | 0,05 / 0,18 / 0,56 / 9,1 |
| Posteingang ab Mitte | 0,04 / 0,05 / 0,05 / 0,19 | 0,06 / 0,20 / 0,51 / 10,8 |
| Vereinter Posteingang Seite 1 | 0,14 / 0,14 / 0,15 / 0,40 | 0,16 / 0,57 / 1,84 / 48 |
| Ungelesen-Zähler alle Postfächer | 0,68 / 0,70 / 0,72 / 2,2 | 2,3 / 5,8 / 11,5 / 69 |
| Suche `rechnung`, erste 50 | 0,10 / 0,10 / 0,12 / 0,44 | 0,13 / 0,92 / 2,4 / 78 |

Schreiber dabei: 3.545 neue Mails/s und 7.090 Flag-Änderungen/s; Transaktion Median 4,8 ms, p95 49 ms,
max 121 ms; WAL bis 74 MB. Ein früherer Lauf unter geringerer Fremdlast hatte mit Schreiber höchstens
4,2 ms (p99 unter 3,5 ms), der Schreiber 4.200 Mails/s. Die einzelnen Ausreißer bis 78 ms im letzten
Lauf fallen vermutlich auf die parallelen Builds; auch sie liegen unter dem Ziel. Trigram: Schreiber
1.945 Mails/s, WAL bis 243 MB, Suche p99 1,6 ms, max 4,1 ms.

### Weitere Messungen

- **Body nachträglich indexieren** (`INSERT OR REPLACE` mit allen Spalten, 500 je Transaktion): 20.000
  Mails in 7,0 s = **2.900 Mails/s**. Probe: ein Wort aus dem neuen Body findet die Mail.
- **FTS `optimize`**: unicode61 3,5 s, Index −1 %; trigram 12,8 s, −2,6 % (gemessen nach dem
  Schreiber-Test, also mit 584.000 Mails). Die eingebaute Zusammenführung (`automerge`) hält den Index
  kompakt; `optimize` ist unnötig.
- **Umlaute:** `über` und `uber` finden dasselbe (22.953 Mails), ebenso `grüße` und `gruße` (35.190);
  `grusse` findet nichts.

## Annahmen des Datengenerators

- 500.000 Mails: Konten mit 250k / 120k / 80k / 40k / 10k. Je Konto INBOX (50 %), Archive (20 %),
  Sent (12 %), Drafts (0,5 %), Trash (2 %), Junk (1,5 %), Newsletter (6 %) und 3–14 weitere eigene
  Ordner (zusammen 8 %); 76 Ordner insgesamt. Der größte Posteingang hat 124.854 Mails, alle
  Posteingänge zusammen 249.787.
- 8 % der Mails aus Posteingang, Archiv und eigenen Ordnern liegen zusätzlich in einem zweiten Ordner
  (32.598 Orte, beim Abgleich über die Message-ID zusammengeführt).
- Datum 01.01.2019 bis 22.11.2026, die Dichte steigt zur Gegenwart (letzte 30 Tage ≈ 1,7 %).
- Gelesen: Posteingang älter als 30 Tage 96 %, jünger 60 %; Newsletter 40 %; Junk 20 %; sonst 97–100 %.
  Markiert 2 %. Insgesamt 40.892 ungelesene Orte.
- Absender aus 20.000 Adressen (70 % Personen mit deutschen und anderen Namen samt Umlauten, 30 % Firmen
  und Newsletter), Zipf-verteilt (s = 1,0), je Konto mit anderer Rangfolge.
- Wortschatz 50.000 Wörter, Zipf (s = 1,0): oben 330 häufige deutsche und englische Wörter mit
  Umlauten, dann 1.600 Komposita (Kontoauszug, Rechnungsnummer …) gemischt mit Silbenwörtern, dann
  der lange Schwanz. Dazu Zahlen, Beträge, Datumsangaben und Bestellnummern (1,5 % der Wörter) und in
  30 % der Bodies Tracking-Links mit Zufallskennung, die wie in echten Mails das Vokabular aufblähen.
  Die Sprache wechselt je Wort, nicht je Mail (Vereinfachung).
- Betreff 2–8 Wörter, 30 % mit „Re:“/„AW:“/„Fwd:“, 15 % mit Nummer. Vorschau (160 Zeichen) für alle Mails.
- Body: 20 + Exp(200) Wörter, im Mittel rund 220 Wörter bzw. 1,6 KB, mit Gruß- und Dankesformeln.
- 15 % der Mails mit 1–3 Anhängen (Dateinamen wie `Rechnung_2024-00815.pdf`, `Präsentation Q3 2025.pptx`).
- **Realistischer Umfang des Volltexts:** Kopfdaten und Anhangnamen für alle Mails; der Text für
  Mails der letzten 30 Tage in Posteingang, Gesendet und Entwürfe, für markierte Mails und für 18 %
  der übrigen (früher geöffnet, Body liegt im Cache). Das sind 101.879 Mails (20 %) mit 164 MB Text.
- **Obergrenze:** Text für alle 500.000 Mails, gekürzt auf die ersten 120 Wörter (355 MB Text).
- Nicht nachgebildet: HTML, Zitate früherer Mails, lange Threads, Sprachen außer Deutsch und Englisch.
  Echte Wortverteilungen weichen ab. Die Größen sind Richtwerte; die Latenzen liegen so weit unter den
  Zielen, dass Abweichungen der Daten daran nichts ändern dürften.

## Folgen für Stufe 2 und Stufe 7

**Stufe 2 (Konto und Sync)**
- Schema und Indizes wie oben (ohne FTS), Migration über `rusqlite_migration`.
- Sortierdatum festlegen: Date-Kopfzeile, wenn lesbar und höchstens 24 h nach INTERNALDATE, sonst
  INTERNALDATE; auf 0 bis 2^33 begrenzen.
- `id` im Schreib-Thread vergeben: Kandidat = Sortierdatum; ist er belegt, die nächste freie Zahl
  darüber (z. B. `SELECT id FROM message WHERE id >= ?1 ORDER BY id` bis zur ersten Lücke). Unit-Test
  für viele Mails mit gleichem Datum (Importe).
- Zusammenführen per Message-ID nach der Regel oben.
- Abgleichsfenster von 500 Mails je Transaktion, `BEGIN IMMEDIATE`.
- Listen nur per Keyset (`message_id < letzte id`), nie `OFFSET`.
- Ungelesen-Zähler über `loc_unseen`; ein eigener Zähler in `mailbox` ist nicht nötig.
- Plan-Tests in `mail-store` für jede Abfrage (kein `SCAN` auf `message`/`message_location`, kein
  `TEMP B-TREE` außer beim Mischen von n × 100 Zeilen). `CROSS JOIN` für feste Join-Reihenfolge.
- Pragmas und Verbindungsaufbau wie oben (ein Schreiber, 2–4 Leser).

**Stufe 7 (Suche, Threads, Konten)**
- FTS-Tabelle wie oben mit unicode61. Spalten nach den Filtern aus `docs/SPEC.md` aufteilen:
  `subject, from, to, cc, body, attachments` statt `sender`/`recipients`. Dann sind `from:`, `to:`, `cc:`,
  `subject:` und `filename:` Spaltenfilter von FTS5. Der Text bleibt derselbe, der Platzbedarf auch.
- Erste Treffer immer `ORDER BY rowid DESC LIMIT 50`. Eine Sortierung „Relevanz“ (`bm25`) höchstens
  als Option, mit Abbruch beim Weitertippen, denn bei sehr häufigen Wörtern kostet sie 140–310 ms.
- `before:`/`after:`/`older_than:`/`newer_than:` als `rowid`-Bereich, `account:` und `is:unread` als
  Filter im Join wie oben. `in:` über `message_location`, `has:attachment` über `m.has_attachments`.
- Trefferzahl per `count(*)`: 2–8 ms (Obergrenze 15 ms), darf nebenher laufen.
- Body nachträglich indexieren per `INSERT OR REPLACE` mit allen Spalten (2.900 Mails/s). FTS-Zeilen je
  Transaktion nach `rowid` sortieren.
- Neuaufbau im Hintergrund: neue FTS-Tabelle befüllen, dann in einer Transaktion umbenennen. `optimize`
  ist nicht nötig.
- Vereinter Posteingang und Kontogruppen per `UNION ALL` je Postfach (0,14 ms); ausgeblendete Konten
  fallen aus der Liste der Postfächer.
- Thread-Liste: dasselbe Muster mit `thread.last_message_id` (zeitlich geordnet) und einem Index
  `(account_id, last_message_id)`. Im Spike nicht gemessen.
- Das Leistungsziel „Suche unter 200 ms bei 100.000 Mails“ ist mit „neueste zuerst“ schon bei 500.000
  um drei Größenordnungen unterschritten.

## Rückweg

- Alles SQL liegt in `mail-store`. Ein Tokenizer-Wechsel heißt: neue FTS-Tabelle befüllen und tauschen.
  Der Index ist Cache-Klasse und lässt sich aus `message` und den Body-Dateien neu aufbauen.
- Erweist sich die zeitlich geordnete `id` als hinderlich (etwa weil nach einem anderen Datum sortiert
  werden soll), bleibt sie als eindeutiger Schlüssel gültig. Eine andere Sortierung braucht dann einen
  Index `(mailbox_id, sort_key)` in `message_location`, und Suchtreffer müssen sortiert werden (Kosten
  siehe `ORDER BY m.date`).
- Reicht unicode61 nicht, steht Trigram mit gemessenen Kosten bereit.
