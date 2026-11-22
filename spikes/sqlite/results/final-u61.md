
## Aufbau `u61` (FTS Some(Unicode61), Body Realistic, Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 0, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 3.19 2.89 3.05 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 101879 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 68.2 s): **7327 Mails/s**, 7804 Orte/s; Erzeugung 9.0 s; gesamt 77.3 s
- WAL maximal während des Ladens 13.1 MB / am Ende 0.0 MB; ANALYZE 0.2 s; Datei nach Checkpoint **549.8 MB**
- Indexierter Text 233.3 MB (subject 17.0 MB, sender 17.6 MB, recipients 32.1 MB, body 163.7 MB, attachments 2.9 MB)
- Ungelesene Orte gesamt: 40892
- Größe ohne FTS 326.1 MB / FTS 222.3 MB
- Objekte: message 230.4 MB, message_fts_data 211.1 MB, message_msgid 28.8 MB, message_location 24.8 MB, loc_uid 14.2 MB, attachment 11.5 MB, message_fts_docsize 10.5 MB, message_thread 7.1 MB, loc_message 6.4 MB, attachment_message 2.1 MB

## Messung `u61` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.31 2.68 2.95 }
Seltenes Wort `laarzeso` steht in 55 Mails.
Diakritika: `über` 22953 Treffer, `uber` 22953 Treffer
Diakritika: `grüße` 35190 Treffer, `gruße` 35190 Treffer
Diakritika: `grüße` 35190 Treffer, `grusse` 0 Treffer
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
- Suche neueste zuerst / Präfix: rech*: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Phrase: "vielen dank": `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / zwei Wörter: rechnung bestellung: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Umlaut: grüße: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Spalte: sender:müller: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
- Suche neueste zuerst / Teilwort Schritt 1: fts5vocab LIKE '%auszug%': `SCAN message_fts_vocab VIRTUAL TABLE INDEX 1:`
- Suche neueste zuerst / Teilwort Schritt 2: OR über 61 Terme: `SCAN message_fts VIRTUAL TABLE INDEX 192:M5 | SEARCH m USING INTEGER PRIMARY KEY (rowid=?)`
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
| Posteingang Konto 1 (124854 Mails), Seite 1 | 100 | 0.043 | 0.045 | 0.053 | 200 | 0.12 |
| … Keyset ab Mitte | 100 | 0.043 | 0.044 | 0.048 | 200 | 0.11 |
| … Keyset ab Ende (letzte Seite) | 99 | 0.043 | 0.043 | 0.048 | 200 | 0.09 |
| … nur ungelesen, Seite 1 | 100 | 0.060 | 0.060 | 0.067 | 200 | 0.13 |
| … zum Vergleich OFFSET Mitte | 100 | 16.522 | 17.530 | 18.888 | 121 | 20.78 |
| … zum Vergleich OFFSET Ende | 100 | 35.894 | 36.548 | 36.909 | 56 | 41.27 |

### Vereinter Posteingang

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| UNION ALL je Postfach (249787 Mails), Seite 1 | 100 | 0.138 | 0.143 | 0.150 | 200 | 0.28 |
| UNION ALL, Keyset ab Mitte | 100 | 0.138 | 0.143 | 0.152 | 200 | 0.24 |
| UNION ALL, Keyset ab Ende | 99 | 0.100 | 0.103 | 0.112 | 200 | 0.18 |
| IN (...) Planer-Wahl, Seite 1 | 100 | 393.154 | 397.833 | 397.833 | 10 | 396.30 |
| IN (...) Planer-Wahl, ab Ende | 99 | 0.085 | 0.088 | 0.093 | 200 | 0.25 |
| IN (...) über loc_message, Seite 1 | 100 | 0.088 | 0.091 | 0.148 | 200 | 0.19 |
| IN (...) über loc_message, ab Ende | 99 | 0.085 | 0.088 | 0.094 | 200 | 0.19 |
| IN (...) mit Sortierung (Gegenprobe) | 100 | 186.580 | 188.183 | 188.183 | 11 | 191.31 |

### Zähler

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| ungelesen, größter Posteingang (Teilindex) | 1 | 0.059 | 0.062 | 0.071 | 200 | 0.13 |
| ungelesen, größter Posteingang (ohne Teilindex) | 1 | 2.161 | 2.188 | 2.248 | 200 | 3.30 |
| ungelesen, alle Postfächer (Seitenleiste, Teilindex) | 66 | 0.712 | 0.727 | 0.749 | 200 | 0.97 |
| ungelesen, alle Postfächer (ohne Teilindex) | 66 | 9.932 | 10.131 | 11.083 | 200 | 12.35 |
| Gesamtzahl größter Posteingang | 1 | 1.240 | 1.260 | 1.301 | 200 | 1.92 |

### LIKE ohne FTS

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| Betreff LIKE '%auszug%', neueste 50 | 50 | 0.376 | 0.385 | 0.395 | 200 | 0.81 |
| Betreff/Absender LIKE '%müller%', neueste 50 | 50 | 0.358 | 0.366 | 0.372 | 200 | 0.54 |
| Betreff/Absender LIKE '%laarzeso%' (selten, voller Scan) | 6 | 72.095 | 73.405 | 74.193 | 28 | 77.76 |

### Suche neueste zuerst

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| häufig: rechnung | 50 | 0.094 | 0.097 | 0.137 | 200 | 0.30 |
| sehr häufig: und | 50 | 0.097 | 0.100 | 0.104 | 200 | 0.25 |
| selten: laarzeso | 50 | 0.077 | 0.079 | 0.085 | 200 | 0.27 |
| Präfix: rech* | 50 | 12.372 | 12.561 | 12.741 | 162 | 12.76 |
| Phrase: "vielen dank" | 50 | 0.186 | 0.190 | 0.198 | 200 | 0.45 |
| zwei Wörter: rechnung bestellung | 50 | 0.174 | 0.178 | 0.194 | 200 | 0.38 |
| Umlaut: grüße | 50 | 0.087 | 0.091 | 0.096 | 200 | 0.22 |
| Spalte: sender:müller | 50 | 0.089 | 0.092 | 0.107 | 200 | 0.24 |
| Teilwort Schritt 1: fts5vocab LIKE '%auszug%' | 61 | 772.740 | 776.072 | 776.072 | 10 | 772.69 |
| Teilwort Schritt 2: OR über 61 Terme | 50 | 4.139 | 4.233 | 4.911 | 200 | 5.37 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 0.175 | 0.181 | 0.195 | 200 | 0.49 |
| rechnung + Konto 5 (10k) | 50 | 0.540 | 0.549 | 0.565 | 200 | 1.31 |
| rechnung + letzte 365 Tage | 50 | 0.092 | 0.093 | 0.100 | 200 | 0.21 |
| rechnung + ungelesen | 50 | 0.150 | 0.155 | 0.167 | 200 | 0.34 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 0.445 | 0.460 | 0.523 | 200 | 0.79 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 35.782 | 36.262 | 36.572 | 57 | 37.40 |
| und, ORDER BY rank (bm25) | 50 | 144.243 | 145.331 | 145.475 | 14 | 144.93 |
| laarzeso, ORDER BY rank (bm25) | 50 | 0.162 | 0.167 | 0.172 | 200 | 0.50 |
| rechnung, ORDER BY m.date (Join) | 50 | 78.058 | 79.060 | 79.452 | 26 | 82.52 |
| und, ORDER BY m.date (Join) | 50 | 238.194 | 240.395 | 240.395 | 10 | 243.23 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 2.173 | 2.492 | 3.306 | 200 | 2.38 |
| count(*) und | 1 | 7.751 | 7.982 | 9.088 | 200 | 8.54 |

RSS des Messprozesses am Ende: 50.78125 MB (Leser-Cache 8 MB, mmap true)

## Leser mit und ohne laufenden Schreiber (20 s je Phase)

| Abfrage | ohne Schreiber: n / Median / p95 / p99 / max ms | mit Schreiber: n / Median / p95 / p99 / max ms |
|---|---|---|
| Liste Posteingang Seite 1 | 19757 / 0.04 / 0.05 / 0.05 / 0.16 | 5990 / 0.05 / 0.18 / 0.56 / 9.06 |
| Liste Posteingang ab Mitte | 19757 / 0.04 / 0.05 / 0.05 / 0.19 | 5990 / 0.06 / 0.20 / 0.51 / 10.77 |
| Vereinter Posteingang Seite 1 | 19757 / 0.14 / 0.14 / 0.15 / 0.40 | 5990 / 0.16 / 0.57 / 1.84 / 48.40 |
| Ungelesen-Zähler Seitenleiste | 19757 / 0.68 / 0.70 / 0.72 / 2.15 | 5990 / 2.28 / 5.82 / 11.49 / 69.35 |
| Suche rechnung neueste 50 | 19757 / 0.10 / 0.10 / 0.12 / 0.44 | 5990 / 0.13 / 0.92 / 2.39 / 77.68 |

Schreiber: 70900 neue Mails (mit FTS, Body indexiert) und 141800 Flag-Änderungen in 1418 Transaktionen; 3545 Mails/s; Transaktion (100 Mails bzw. 200 Flags) Median 4.8 ms, p95 48.6 ms, max 120.5 ms; WAL maximal 74.0 MB
