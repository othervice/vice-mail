
## Aufbau `tri` (FTS Some(Trigram), Body Realistic, Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 8, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 1.79 3.20 3.76 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 101879 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 127.5 s): **3922 Mails/s**, 4177 Orte/s; Erzeugung 9.1 s; gesamt 136.7 s
- WAL maximal während des Ladens 34.9 MB / am Ende 0.0 MB; ANALYZE 0.4 s; Datei nach Checkpoint **1657.4 MB**
- Indexierter Text 233.3 MB (subject 17.0 MB, sender 17.6 MB, recipients 32.1 MB, body 163.7 MB, attachments 2.9 MB)
- Größe ohne FTS 335.6 MB / FTS 1318.6 MB
- Objekte: message_fts_data 1306.5 MB, message 231.0 MB, message_msgid 29.9 MB, message_location 27.2 MB, loc_uid 16.4 MB, attachment 11.8 MB, message_fts_docsize 11.3 MB, message_thread 8.3 MB, loc_message 7.5 MB, attachment_message 2.5 MB

## Messung `tri` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 1.58 2.60 3.43 }
Seltenes Wort `laarzeso` steht in 55 Mails.
Diakritika: `über` 76050 Treffer, `uber` 76050 Treffer
Diakritika: `grüße` 72067 Treffer, `gruße` 72067 Treffer
Diakritika: `grüße` 72067 Treffer, `grusse` 0 Treffer
Diakritika: `müller` 12623 Treffer, `mueller` 12259 Treffer

### EXPLAIN QUERY PLAN

- Liste Postfach / Posteingang Konto 1 (124854 Mails), Seite 1: `SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?) | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Liste Postfach / … Keyset ab Mitte: `SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?) | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Liste Postfach / … Keyset ab Ende (letzte Seite): `SEARCH l USING PRIMARY KEY (mailbox_id=? AND message_id<?) | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Liste Postfach / … nur ungelesen, Seite 1: `SEARCH l USING INDEX loc_unseen (mailbox_id=? AND message_id<?) | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Liste Postfach / … zum Vergleich OFFSET Mitte: `SEARCH l USING PRIMARY KEY (mailbox_id=?) | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Liste Postfach / … zum Vergleich OFFSET Ende: `SEARCH l USING PRIMARY KEY (mailbox_id=?) | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / UNION ALL je Postfach (249787 Mails), Seite 1 **[TEMP B-TREE]**: `CO-ROUTINE l |   MERGE (UNION ALL) |     LEFT |       MERGE (UNION ALL) |         LEFT |           CO-ROUTINE (subquery-1) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-1) |           USE TEMP B-TREE FOR ORDER BY |         RIGHT |           CO-ROUTINE (subquery-3) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-3) |           USE TEMP B-TREE FOR ORDER BY |     RIGHT |       MERGE (UNION ALL) |         LEFT |           MERGE (UNION ALL) |             LEFT |               CO-ROUTINE (subquery-5) |                 SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |               SCAN (subquery-5) |               USE TEMP B-TREE FOR ORDER BY |             RIGHT |               CO-ROUTINE (subquery-7) |                 SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |               SCAN (subquery-7) |               USE TEMP B-TREE FOR ORDER BY |         RIGHT |           CO-ROUTINE (subquery-9) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-9) |           USE TEMP B-TREE FOR ORDER BY | SCAN l | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / UNION ALL, Keyset ab Mitte **[TEMP B-TREE]**: `CO-ROUTINE l |   MERGE (UNION ALL) |     LEFT |       MERGE (UNION ALL) |         LEFT |           CO-ROUTINE (subquery-1) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-1) |           USE TEMP B-TREE FOR ORDER BY |         RIGHT |           CO-ROUTINE (subquery-3) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-3) |           USE TEMP B-TREE FOR ORDER BY |     RIGHT |       MERGE (UNION ALL) |         LEFT |           MERGE (UNION ALL) |             LEFT |               CO-ROUTINE (subquery-5) |                 SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |               SCAN (subquery-5) |               USE TEMP B-TREE FOR ORDER BY |             RIGHT |               CO-ROUTINE (subquery-7) |                 SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |               SCAN (subquery-7) |               USE TEMP B-TREE FOR ORDER BY |         RIGHT |           CO-ROUTINE (subquery-9) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-9) |           USE TEMP B-TREE FOR ORDER BY | SCAN l | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / UNION ALL, Keyset ab Ende **[TEMP B-TREE]**: `CO-ROUTINE l |   MERGE (UNION ALL) |     LEFT |       MERGE (UNION ALL) |         LEFT |           CO-ROUTINE (subquery-1) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-1) |           USE TEMP B-TREE FOR ORDER BY |         RIGHT |           CO-ROUTINE (subquery-3) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-3) |           USE TEMP B-TREE FOR ORDER BY |     RIGHT |       MERGE (UNION ALL) |         LEFT |           MERGE (UNION ALL) |             LEFT |               CO-ROUTINE (subquery-5) |                 SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |               SCAN (subquery-5) |               USE TEMP B-TREE FOR ORDER BY |             RIGHT |               CO-ROUTINE (subquery-7) |                 SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |               SCAN (subquery-7) |               USE TEMP B-TREE FOR ORDER BY |         RIGHT |           CO-ROUTINE (subquery-9) |             SEARCH message_location USING PRIMARY KEY (mailbox_id=? AND message_id<?) |           SCAN (subquery-9) |           USE TEMP B-TREE FOR ORDER BY | SCAN l | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / IN (...) Planer-Wahl, Seite 1 **[TEMP B-TREE]**: `SEARCH m USING INTEGER PRIMARY KEY (rowid<?) | SEARCH l USING INDEX loc_message (message_id=? AND mailbox_id=?) | LIST SUBQUERY 1 |   SCAN mailbox | USE TEMP B-TREE FOR ORDER BY`
- Vereinter Posteingang / IN (...) Planer-Wahl, ab Ende: `SEARCH l USING INDEX loc_message (message_id<?) | LIST SUBQUERY 1 |   BLOOM FILTER ON mailbox (role=?) |   SEARCH mailbox USING AUTOMATIC PARTIAL COVERING INDEX (role=?) |   CREATE BLOOM FILTER | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / IN (...) über loc_message, Seite 1: `SEARCH l USING INDEX loc_message (message_id<?) | LIST SUBQUERY 1 |   BLOOM FILTER ON mailbox (role=?) |   SEARCH mailbox USING AUTOMATIC PARTIAL COVERING INDEX (role=?) |   CREATE BLOOM FILTER | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / IN (...) über loc_message, ab Ende: `SEARCH l USING INDEX loc_message (message_id<?) | LIST SUBQUERY 1 |   BLOOM FILTER ON mailbox (role=?) |   SEARCH mailbox USING AUTOMATIC PARTIAL COVERING INDEX (role=?) |   CREATE BLOOM FILTER | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Vereinter Posteingang / IN (...) mit Sortierung (Gegenprobe) **[TEMP B-TREE]**: `SEARCH l USING PRIMARY KEY (mailbox_id=?) | LIST SUBQUERY 1 |   SCAN mailbox | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | USE TEMP B-TREE FOR ORDER BY`
- Zähler / ungelesen, größter Posteingang (Teilindex): `SEARCH message_location USING COVERING INDEX loc_unseen (mailbox_id=?)`
- Zähler / ungelesen, größter Posteingang (ohne Teilindex): `SEARCH message_location USING PRIMARY KEY (mailbox_id=?)`
- Zähler / ungelesen, alle Postfächer (Seitenleiste, Teilindex): `SCAN message_location USING COVERING INDEX loc_unseen`
- Zähler / ungelesen, alle Postfächer (ohne Teilindex): `SCAN message_location`
- Zähler / Gesamtzahl größter Posteingang: `SEARCH message_location USING COVERING INDEX loc_uid (mailbox_id=?)`
- LIKE ohne FTS / Betreff LIKE '%auszug%', neueste 50: `SCAN m`
- LIKE ohne FTS / Betreff/Absender LIKE '%müller%', neueste 50: `SCAN m`
- LIKE ohne FTS / Betreff/Absender LIKE '%laarzeso%' (selten, voller Scan): `SCAN m`
- Suche neueste zuerst / häufig: rechnung: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / sehr häufig: und: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / selten: laarzeso: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Präfix: rech: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Phrase: "vielen dank": `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / zwei Wörter: rechnung bestellung: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Umlaut: grüße: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Spalte: sender:müller: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Teilwort: auszug (Trigramm): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Teilwort: nummer (Trigramm): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + Konto 4 (40k): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + Konto 5 (10k): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + letzte 365 Tage: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5> | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + ungelesen: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | SEARCH l EXISTS USING INDEX loc_message (message_id=?)`
- Suche mit Filter / rechnung + Konto 4 + 365 Tage + ungelesen: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5> | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | SEARCH l EXISTS USING INDEX loc_message (message_id=?)`
- Suche andere Reihenfolge / rechnung, ORDER BY rank (bm25): `SCAN message_fts VIRTUAL TABLE INDEX 32:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche andere Reihenfolge / und, ORDER BY rank (bm25): `SCAN message_fts VIRTUAL TABLE INDEX 32:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche andere Reihenfolge / laarzeso, ORDER BY rank (bm25): `SCAN message_fts VIRTUAL TABLE INDEX 32:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche andere Reihenfolge / rechnung, ORDER BY m.date (Join) **[TEMP B-TREE]**: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | USE TEMP B-TREE FOR ORDER BY`
- Suche andere Reihenfolge / und, ORDER BY m.date (Join) **[TEMP B-TREE]**: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | USE TEMP B-TREE FOR ORDER BY`
- Trefferzahl / count(*) rechnung: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5`
- Trefferzahl / count(*) und: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5`

### Liste Postfach

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| Posteingang Konto 1 (124854 Mails), Seite 1 | 100 | 0.044 | 0.045 | 0.055 | 200 | 0.12 |
| … Keyset ab Mitte | 100 | 0.043 | 0.044 | 0.050 | 200 | 0.10 |
| … Keyset ab Ende (letzte Seite) | 99 | 0.043 | 0.043 | 0.049 | 200 | 0.10 |
| … nur ungelesen, Seite 1 | 100 | 0.059 | 0.060 | 0.066 | 200 | 0.13 |
| … zum Vergleich OFFSET Mitte | 100 | 18.205 | 19.819 | 20.029 | 109 | 23.11 |
| … zum Vergleich OFFSET Ende | 100 | 39.283 | 39.996 | 40.668 | 51 | 45.42 |

### Vereinter Posteingang

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| UNION ALL je Postfach (249787 Mails), Seite 1 | 100 | 0.136 | 0.139 | 0.146 | 200 | 0.28 |
| UNION ALL, Keyset ab Mitte | 100 | 0.135 | 0.138 | 0.149 | 200 | 0.24 |
| UNION ALL, Keyset ab Ende | 99 | 0.099 | 0.105 | 0.110 | 200 | 0.18 |
| IN (...) Planer-Wahl, Seite 1 | 100 | 396.725 | 398.610 | 398.610 | 10 | 402.59 |
| IN (...) Planer-Wahl, ab Ende | 99 | 0.087 | 0.091 | 0.094 | 200 | 0.27 |
| IN (...) über loc_message, Seite 1 | 100 | 0.088 | 0.091 | 0.099 | 200 | 0.21 |
| IN (...) über loc_message, ab Ende | 99 | 0.085 | 0.088 | 0.091 | 200 | 0.17 |
| IN (...) mit Sortierung (Gegenprobe) | 100 | 197.567 | 200.207 | 200.207 | 11 | 202.22 |

### Zähler

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| ungelesen, größter Posteingang (Teilindex) | 1 | 0.062 | 0.065 | 0.069 | 200 | 0.13 |
| ungelesen, größter Posteingang (ohne Teilindex) | 1 | 2.154 | 2.184 | 2.217 | 200 | 3.41 |
| ungelesen, alle Postfächer (Seitenleiste, Teilindex) | 66 | 0.707 | 0.716 | 0.732 | 200 | 1.00 |
| ungelesen, alle Postfächer (ohne Teilindex) | 66 | 8.171 | 8.351 | 9.392 | 200 | 13.26 |
| Gesamtzahl größter Posteingang | 1 | 1.252 | 1.266 | 1.292 | 200 | 2.08 |

### LIKE ohne FTS

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| Betreff LIKE '%auszug%', neueste 50 | 50 | 0.357 | 0.367 | 0.380 | 200 | 0.77 |
| Betreff/Absender LIKE '%müller%', neueste 50 | 50 | 0.354 | 0.363 | 0.373 | 200 | 0.56 |
| Betreff/Absender LIKE '%laarzeso%' (selten, voller Scan) | 6 | 75.851 | 77.077 | 77.481 | 27 | 80.10 |

### Suche neueste zuerst

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| häufig: rechnung | 50 | 0.466 | 0.482 | 0.599 | 200 | 1.03 |
| sehr häufig: und | 50 | 0.082 | 0.084 | 0.092 | 200 | 0.25 |
| selten: laarzeso | 50 | 3.118 | 3.186 | 3.235 | 200 | 3.96 |
| Präfix: rech | 50 | 0.166 | 0.170 | 0.172 | 200 | 0.41 |
| Phrase: "vielen dank" | 50 | 0.680 | 0.695 | 0.717 | 200 | 1.29 |
| zwei Wörter: rechnung bestellung | 50 | 1.163 | 1.186 | 1.348 | 200 | 1.98 |
| Umlaut: grüße | 50 | 0.234 | 0.243 | 0.249 | 200 | 0.52 |
| Spalte: sender:müller | 50 | 0.342 | 0.348 | 0.361 | 200 | 0.67 |
| Teilwort: auszug (Trigramm) | 50 | 0.304 | 0.310 | 0.318 | 200 | 0.59 |
| Teilwort: nummer (Trigramm) | 50 | 0.312 | 0.318 | 0.365 | 200 | 0.56 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 0.803 | 0.813 | 0.826 | 200 | 1.33 |
| rechnung + Konto 5 (10k) | 50 | 1.583 | 1.666 | 1.728 | 200 | 2.27 |
| rechnung + letzte 365 Tage | 50 | 0.467 | 0.478 | 0.668 | 200 | 0.89 |
| rechnung + ungelesen | 50 | 0.549 | 0.558 | 0.616 | 200 | 0.94 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 1.539 | 1.560 | 1.583 | 200 | 2.41 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 129.123 | 129.918 | 130.149 | 16 | 130.85 |
| und, ORDER BY rank (bm25) | 50 | 153.177 | 155.621 | 155.667 | 14 | 154.66 |
| laarzeso, ORDER BY rank (bm25) | 50 | 4.967 | 5.041 | 5.174 | 200 | 6.05 |
| rechnung, ORDER BY m.date (Join) | 50 | 165.658 | 166.758 | 166.871 | 13 | 174.58 |
| und, ORDER BY m.date (Join) | 50 | 247.428 | 248.482 | 248.482 | 10 | 254.97 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 35.499 | 36.056 | 36.560 | 57 | 35.50 |
| count(*) und | 1 | 7.505 | 7.777 | 10.447 | 200 | 8.70 |

## Leser mit und ohne laufenden Schreiber (20 s je Phase)

| Abfrage | ohne Schreiber: n / Median / p95 / p99 / max ms | mit Schreiber: n / Median / p95 / p99 / max ms |
|---|---|---|
| Liste Posteingang Seite 1 | 14481 / 0.04 / 0.05 / 0.05 / 0.14 | 8254 / 0.05 / 0.07 / 0.27 / 4.21 |
| Liste Posteingang ab Mitte | 14481 / 0.04 / 0.05 / 0.05 / 0.08 | 8254 / 0.05 / 0.07 / 0.08 / 0.54 |
| Vereinter Posteingang Seite 1 | 14481 / 0.14 / 0.14 / 0.15 / 0.24 | 8254 / 0.16 / 0.21 / 0.27 / 1.80 |
| Ungelesen-Zähler Seitenleiste | 14481 / 0.69 / 0.71 / 0.72 / 0.89 | 8254 / 1.44 / 2.15 / 2.42 / 6.65 |
| Suche rechnung neueste 50 | 14481 / 0.46 / 0.48 / 0.50 / 1.03 | 8254 / 0.65 / 1.16 / 1.64 / 4.09 |

Schreiber: 38900 neue Mails (mit FTS, Body indexiert) und 77800 Flag-Änderungen in 778 Transaktionen; 1945 Mails/s; Transaktion (100 Mails bzw. 200 Flags) Median 14.5 ms, p95 58.7 ms, max 220.2 ms; WAL maximal 242.9 MB

## FTS optimize `tri`

- Dauer 12.8 s; FTS 1633.7 MB -> 1591.6 MB; Zeilen in message_fts_data 412660 -> 391193; Datei 1999.0 MB davon frei 47.9 MB (VACUUM würde sie freigeben); WAL nach Checkpoint abgeschnitten

## Messung `tri` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.03 2.45 3.21 }
Seltenes Wort `laarzeso` steht in 72 Mails.
Diakritika: `über` 97571 Treffer, `uber` 97571 Treffer
Diakritika: `grüße` 98320 Treffer, `gruße` 98320 Treffer
Diakritika: `grüße` 98320 Treffer, `grusse` 0 Treffer
Diakritika: `müller` 13897 Treffer, `mueller` 13509 Treffer

### EXPLAIN QUERY PLAN

- Suche neueste zuerst / häufig: rechnung: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / sehr häufig: und: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / selten: laarzeso: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Präfix: rech: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Phrase: "vielen dank": `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / zwei Wörter: rechnung bestellung: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Umlaut: grüße: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Spalte: sender:müller: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Teilwort: auszug (Trigramm): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Teilwort: nummer (Trigramm): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + Konto 4 (40k): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + Konto 5 (10k): `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + letzte 365 Tage: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5> | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche mit Filter / rechnung + ungelesen: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | SEARCH l EXISTS USING INDEX loc_message (message_id=?)`
- Suche mit Filter / rechnung + Konto 4 + 365 Tage + ungelesen: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5> | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | SEARCH l EXISTS USING INDEX loc_message (message_id=?)`
- Suche andere Reihenfolge / rechnung, ORDER BY rank (bm25): `SCAN message_fts VIRTUAL TABLE INDEX 32:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche andere Reihenfolge / und, ORDER BY rank (bm25): `SCAN message_fts VIRTUAL TABLE INDEX 32:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche andere Reihenfolge / laarzeso, ORDER BY rank (bm25): `SCAN message_fts VIRTUAL TABLE INDEX 32:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche andere Reihenfolge / rechnung, ORDER BY m.date (Join) **[TEMP B-TREE]**: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | USE TEMP B-TREE FOR ORDER BY`
- Suche andere Reihenfolge / und, ORDER BY m.date (Join) **[TEMP B-TREE]**: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?) | USE TEMP B-TREE FOR ORDER BY`
- Trefferzahl / count(*) rechnung: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5`
- Trefferzahl / count(*) und: `SCAN message_fts VIRTUAL TABLE INDEX 0:M5`

### Suche neueste zuerst

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| häufig: rechnung | 50 | 0.066 | 0.070 | 0.074 | 200 | 0.20 |
| sehr häufig: und | 50 | 0.019 | 0.019 | 0.028 | 200 | 0.07 |
| selten: laarzeso | 50 | 1.522 | 1.569 | 1.611 | 200 | 1.97 |
| Präfix: rech | 50 | 0.031 | 0.032 | 0.036 | 200 | 0.14 |
| Phrase: "vielen dank" | 50 | 0.100 | 0.103 | 0.113 | 200 | 0.25 |
| zwei Wörter: rechnung bestellung | 50 | 0.194 | 0.199 | 0.202 | 200 | 0.40 |
| Umlaut: grüße | 50 | 0.035 | 0.035 | 0.042 | 200 | 0.10 |
| Spalte: sender:müller | 50 | 0.061 | 0.062 | 0.063 | 200 | 0.17 |
| Teilwort: auszug (Trigramm) | 50 | 0.040 | 0.041 | 0.043 | 200 | 0.11 |
| Teilwort: nummer (Trigramm) | 50 | 0.046 | 0.047 | 0.054 | 200 | 0.11 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 12.169 | 12.774 | 13.141 | 164 | 16.21 |
| rechnung + Konto 5 (10k) | 50 | 12.632 | 13.113 | 13.304 | 158 | 17.52 |
| rechnung + letzte 365 Tage | 50 | 0.066 | 0.067 | 0.071 | 200 | 0.22 |
| rechnung + ungelesen | 50 | 0.076 | 0.078 | 0.085 | 200 | 0.19 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 12.738 | 13.655 | 14.043 | 156 | 16.85 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 136.770 | 138.521 | 140.133 | 15 | 141.53 |
| und, ORDER BY rank (bm25) | 50 | 172.051 | 176.295 | 177.476 | 12 | 178.05 |
| laarzeso, ORDER BY rank (bm25) | 50 | 2.781 | 2.830 | 2.927 | 200 | 3.56 |
| rechnung, ORDER BY m.date (Join) | 50 | 188.790 | 190.133 | 190.133 | 11 | 194.38 |
| und, ORDER BY m.date (Join) | 50 | 276.816 | 278.284 | 278.284 | 10 | 286.68 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 32.526 | 32.958 | 33.038 | 62 | 34.41 |
| count(*) und | 1 | 5.152 | 5.309 | 5.417 | 200 | 6.25 |
