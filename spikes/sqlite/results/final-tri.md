
## Aufbau `tri` (FTS Some(Trigram), Body Realistic, Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 0, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 4.47 3.06 3.05 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 101879 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 129.0 s): **3876 Mails/s**, 4129 Orte/s; Erzeugung 9.6 s; gesamt 138.7 s
- WAL maximal während des Ladens 28.7 MB / am Ende 0.0 MB; ANALYZE 0.3 s; Datei nach Checkpoint **1479.5 MB**
- Indexierter Text 233.3 MB (subject 17.0 MB, sender 17.6 MB, recipients 32.1 MB, body 163.7 MB, attachments 2.9 MB)
- Ungelesene Orte gesamt: 40892
- Größe ohne FTS 326.1 MB / FTS 1149.9 MB
- Objekte: message_fts_data 1138.3 MB, message 230.4 MB, message_msgid 28.8 MB, message_location 24.8 MB, loc_uid 14.2 MB, attachment 11.5 MB, message_fts_docsize 10.6 MB, message_thread 7.1 MB, loc_message 6.4 MB, attachment_message 2.1 MB

## Messung `tri` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.63 2.83 2.96 }
Seltenes Wort `laarzeso` steht in 55 Mails.
Diakritika: `über` 76050 Treffer, `uber` 76050 Treffer
Diakritika: `grüße` 72067 Treffer, `gruße` 72067 Treffer
Diakritika: `grüße` 72067 Treffer, `grusse` 0 Treffer
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
| häufig: rechnung | 50 | 0.478 | 0.494 | 0.570 | 200 | 0.97 |
| sehr häufig: und | 50 | 0.084 | 0.088 | 0.095 | 200 | 0.23 |
| selten: laarzeso | 50 | 2.982 | 3.035 | 3.145 | 200 | 3.68 |
| Präfix: rech | 50 | 0.165 | 0.169 | 0.180 | 200 | 0.43 |
| Phrase: "vielen dank" | 50 | 0.712 | 0.733 | 0.821 | 200 | 1.32 |
| zwei Wörter: rechnung bestellung | 50 | 1.181 | 1.213 | 1.302 | 200 | 2.25 |
| Umlaut: grüße | 50 | 0.245 | 0.250 | 0.262 | 200 | 0.60 |
| Spalte: sender:müller | 50 | 0.352 | 0.359 | 0.373 | 200 | 0.75 |
| Teilwort: auszug (Trigramm) | 50 | 0.305 | 0.314 | 0.372 | 200 | 0.64 |
| Teilwort: nummer (Trigramm) | 50 | 0.311 | 0.318 | 0.330 | 200 | 0.67 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 0.797 | 0.811 | 0.838 | 200 | 1.43 |
| rechnung + Konto 5 (10k) | 50 | 1.560 | 1.611 | 1.712 | 200 | 2.45 |
| rechnung + letzte 365 Tage | 50 | 0.467 | 0.478 | 0.588 | 200 | 0.96 |
| rechnung + ungelesen | 50 | 0.537 | 0.546 | 0.564 | 200 | 1.09 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 1.545 | 1.586 | 1.721 | 200 | 2.35 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 131.712 | 132.890 | 134.790 | 16 | 132.40 |
| und, ORDER BY rank (bm25) | 50 | 153.277 | 155.318 | 155.675 | 14 | 155.27 |
| laarzeso, ORDER BY rank (bm25) | 50 | 4.445 | 4.589 | 4.759 | 200 | 5.38 |
| rechnung, ORDER BY m.date (Join) | 50 | 185.946 | 373.927 | 373.927 | 10 | 170.82 |
| und, ORDER BY m.date (Join) | 50 | 251.096 | 255.553 | 255.553 | 10 | 592.09 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 35.813 | 37.953 | 43.696 | 56 | 36.54 |
| count(*) und | 1 | 7.850 | 8.004 | 8.210 | 200 | 8.96 |

RSS des Messprozesses am Ende: 48.484375 MB (Leser-Cache 8 MB, mmap true)
