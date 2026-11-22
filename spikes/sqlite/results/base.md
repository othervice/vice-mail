
## Aufbau `base` (FTS None, Body None, Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 8, Cache 256 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 3.61 4.42 3.92 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 0 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 61.1 s): **8189 Mails/s**, 8723 Orte/s; Erzeugung 11.1 s; gesamt 72.3 s
- WAL maximal während des Ladens 10.8 MB / am Ende 0.0 MB; ANALYZE 0.2 s; Datei nach Checkpoint **335.5 MB**
- Größe ohne FTS 335.5 MB / FTS 0.0 MB
- Objekte: message 231.0 MB, message_msgid 29.9 MB, message_location 27.2 MB, loc_uid 16.4 MB, attachment 11.8 MB, message_thread 8.3 MB, loc_message 7.5 MB, attachment_message 2.5 MB

## Aufbau `base` (FTS None, Body None, Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 8, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 3.22 5.26 4.49 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 0 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 58.7 s): **8512 Mails/s**, 9066 Orte/s; Erzeugung 9.2 s; gesamt 68.1 s
- WAL maximal während des Ladens 10.8 MB / am Ende 0.0 MB; ANALYZE 0.1 s; Datei nach Checkpoint **335.5 MB**
- Größe ohne FTS 335.5 MB / FTS 0.0 MB
- Objekte: message 231.0 MB, message_msgid 29.9 MB, message_location 27.2 MB, loc_uid 16.4 MB, attachment 11.8 MB, message_thread 8.3 MB, loc_message 7.5 MB, attachment_message 2.5 MB

## Messung `base` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.18 4.54 4.28 }

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

### Liste Postfach

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| Posteingang Konto 1 (124854 Mails), Seite 1 | 100 | 0.043 | 0.043 | 0.046 | 200 | 0.11 |
| … Keyset ab Mitte | 100 | 0.042 | 0.043 | 0.049 | 200 | 0.10 |
| … Keyset ab Ende (letzte Seite) | 99 | 0.042 | 0.042 | 0.049 | 200 | 0.09 |
| … nur ungelesen, Seite 1 | 100 | 0.059 | 0.059 | 0.064 | 200 | 0.12 |
| … zum Vergleich OFFSET Mitte | 100 | 9.533 | 11.488 | 13.636 | 200 | 19.98 |
| … zum Vergleich OFFSET Ende | 100 | 32.188 | 34.219 | 34.752 | 62 | 39.59 |

### Vereinter Posteingang

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| UNION ALL je Postfach (249787 Mails), Seite 1 | 100 | 0.138 | 0.141 | 0.146 | 200 | 0.28 |
| UNION ALL, Keyset ab Mitte | 100 | 0.137 | 0.140 | 0.147 | 200 | 0.24 |
| UNION ALL, Keyset ab Ende | 99 | 0.100 | 0.105 | 0.110 | 200 | 0.18 |
| IN (...) Planer-Wahl, Seite 1 | 100 | 382.743 | 384.143 | 384.143 | 10 | 392.25 |
| IN (...) Planer-Wahl, ab Ende | 99 | 0.087 | 0.091 | 0.094 | 200 | 0.26 |
| IN (...) über loc_message, Seite 1 | 100 | 0.091 | 0.096 | 0.100 | 200 | 0.20 |
| IN (...) über loc_message, ab Ende | 99 | 0.085 | 0.086 | 0.099 | 200 | 0.18 |
| IN (...) mit Sortierung (Gegenprobe) | 100 | 168.629 | 169.077 | 171.581 | 12 | 180.69 |

### Zähler

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| ungelesen, größter Posteingang (Teilindex) | 1 | 0.060 | 0.060 | 0.064 | 200 | 0.12 |
| ungelesen, größter Posteingang (ohne Teilindex) | 1 | 2.155 | 2.168 | 2.188 | 200 | 3.25 |
| ungelesen, alle Postfächer (Seitenleiste, Teilindex) | 66 | 0.706 | 0.717 | 0.741 | 200 | 0.87 |
| ungelesen, alle Postfächer (ohne Teilindex) | 66 | 8.384 | 8.559 | 9.246 | 200 | 12.34 |
| Gesamtzahl größter Posteingang | 1 | 1.247 | 1.279 | 1.327 | 200 | 1.94 |

### LIKE ohne FTS

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| Betreff LIKE '%auszug%', neueste 50 | 50 | 0.373 | 0.380 | 0.397 | 200 | 0.74 |
| Betreff/Absender LIKE '%müller%', neueste 50 | 50 | 0.360 | 0.367 | 0.372 | 200 | 0.52 |
| Betreff/Absender LIKE '%laarzeso%' (selten, voller Scan) | 6 | 68.257 | 76.258 | 76.891 | 29 | 76.35 |
