
## Aufbau `trihdr` (FTS Some(Trigram), Body None, Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 8, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.48 2.90 3.11 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 0 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 83.1 s): **6020 Mails/s**, 6412 Orte/s; Erzeugung 9.2 s; gesamt 92.3 s
- WAL maximal während des Ladens 17.2 MB / am Ende 0.0 MB; ANALYZE 0.3 s; Datei nach Checkpoint **764.3 MB**
- Indexierter Text 69.6 MB (subject 17.0 MB, sender 17.6 MB, recipients 32.1 MB, body 0.0 MB, attachments 2.9 MB)
- Größe ohne FTS 335.6 MB / FTS 427.6 MB
- Objekte: message_fts_data 416.1 MB, message 231.0 MB, message_msgid 29.9 MB, message_location 27.2 MB, loc_uid 16.4 MB, attachment 11.8 MB, message_fts_docsize 11.0 MB, message_thread 8.3 MB, loc_message 7.5 MB, attachment_message 2.5 MB

## Messung `trihdr` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.57 2.74 3.02 }
Seltenes Wort `laarzeso` steht in 6 Mails.
Diakritika: `über` 24152 Treffer, `uber` 24152 Treffer
Diakritika: `grüße` 4074 Treffer, `gruße` 4074 Treffer
Diakritika: `grüße` 4074 Treffer, `grusse` 0 Treffer
Diakritika: `müller` 12623 Treffer, `mueller` 12259 Treffer

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
| häufig: rechnung | 50 | 1.162 | 2.820 | 5.424 | 200 | 1.17 |
| sehr häufig: und | 50 | 0.274 | 0.463 | 0.945 | 200 | 0.31 |
| selten: laarzeso | 6 | 2.050 | 3.384 | 5.672 | 200 | 3.61 |
| Präfix: rech | 50 | 0.174 | 0.434 | 0.537 | 200 | 0.46 |
| Phrase: "vielen dank" | 5 | 9.313 | 16.451 | 24.110 | 199 | 8.94 |
| zwei Wörter: rechnung bestellung | 50 | 9.771 | 16.210 | 24.960 | 195 | 12.48 |
| Umlaut: grüße | 50 | 0.228 | 0.553 | 0.627 | 200 | 0.59 |
| Spalte: sender:müller | 50 | 0.315 | 0.781 | 0.854 | 200 | 0.69 |
| Teilwort: auszug (Trigramm) | 50 | 0.340 | 0.824 | 0.907 | 200 | 0.73 |
| Teilwort: nummer (Trigramm) | 50 | 0.316 | 0.775 | 0.887 | 200 | 0.67 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 0.718 | 1.686 | 1.848 | 200 | 1.52 |
| rechnung + Konto 5 (10k) | 50 | 2.897 | 5.899 | 7.846 | 200 | 3.99 |
| rechnung + letzte 365 Tage | 50 | 1.174 | 2.471 | 6.125 | 200 | 1.19 |
| rechnung + ungelesen | 50 | 0.650 | 1.746 | 1.917 | 200 | 1.29 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 3.457 | 8.396 | 25.868 | 200 | 7.08 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 63.836 | 84.301 | 86.357 | 29 | 67.00 |
| und, ORDER BY rank (bm25) | 50 | 102.220 | 105.629 | 106.627 | 20 | 105.21 |
| laarzeso, ORDER BY rank (bm25) | 6 | 1.417 | 1.444 | 1.519 | 200 | 2.13 |
| rechnung, ORDER BY m.date (Join) | 50 | 92.721 | 94.208 | 94.264 | 22 | 100.91 |
| und, ORDER BY m.date (Join) | 50 | 227.014 | 235.210 | 235.210 | 10 | 203.64 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 16.333 | 16.992 | 20.945 | 122 | 26.11 |
| count(*) und | 1 | 6.388 | 9.251 | 10.324 | 200 | 6.72 |
