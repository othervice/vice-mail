
## Aufbau `triall` (FTS Some(Trigram), Body AllShort(120), Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 8, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 1.48 2.04 2.91 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 500000 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 275.3 s): **1817 Mails/s**, 1935 Orte/s; Erzeugung 10.0 s; gesamt 285.4 s
- WAL maximal während des Ladens 42.9 MB / am Ende 0.0 MB; ANALYZE 4.7 s; Datei nach Checkpoint **3036.6 MB**
- Indexierter Text 424.1 MB (subject 17.0 MB, sender 17.6 MB, recipients 32.1 MB, body 354.5 MB, attachments 2.9 MB)
- Größe ohne FTS 349.6 MB / FTS 2682.2 MB
- Objekte: message_fts_data 2669.1 MB, message 245.1 MB, message_msgid 29.9 MB, message_location 27.2 MB, loc_uid 16.4 MB, attachment 11.8 MB, message_fts_docsize 11.6 MB, message_thread 8.3 MB, loc_message 7.5 MB, attachment_message 2.5 MB, message_fts_idx 1.5 MB

## Messung `triall` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 2.41 2.94 3.13 }
Seltenes Wort `laarzeso` steht in 109 Mails.
Diakritika: `über` 191902 Treffer, `uber` 191902 Treffer
Diakritika: `grüße` 161299 Treffer, `gruße` 161299 Treffer
Diakritika: `grüße` 161299 Treffer, `grusse` 0 Treffer
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
| häufig: rechnung | 50 | 0.559 | 0.585 | 0.634 | 200 | 1.15 |
| sehr häufig: und | 50 | 0.103 | 0.105 | 0.111 | 200 | 0.31 |
| selten: laarzeso | 50 | 2.861 | 2.932 | 2.973 | 200 | 3.55 |
| Präfix: rech | 50 | 0.187 | 0.191 | 0.196 | 200 | 0.49 |
| Phrase: "vielen dank" | 50 | 0.814 | 0.821 | 0.838 | 200 | 1.44 |
| zwei Wörter: rechnung bestellung | 50 | 1.478 | 1.525 | 1.634 | 200 | 2.39 |
| Umlaut: grüße | 50 | 0.293 | 0.299 | 0.306 | 200 | 0.68 |
| Spalte: sender:müller | 50 | 0.381 | 0.388 | 0.408 | 200 | 0.75 |
| Teilwort: auszug (Trigramm) | 50 | 0.368 | 0.377 | 0.435 | 200 | 0.77 |
| Teilwort: nummer (Trigramm) | 50 | 0.365 | 0.372 | 0.388 | 200 | 0.71 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 0.871 | 0.891 | 1.010 | 200 | 1.58 |
| rechnung + Konto 5 (10k) | 50 | 1.651 | 1.689 | 1.746 | 200 | 2.57 |
| rechnung + letzte 365 Tage | 50 | 0.540 | 0.551 | 0.616 | 200 | 1.13 |
| rechnung + ungelesen | 50 | 0.639 | 0.649 | 0.660 | 200 | 1.12 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 1.686 | 1.709 | 1.764 | 200 | 2.54 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 273.396 | 276.866 | 276.866 | 10 | 276.02 |
| und, ORDER BY rank (bm25) | 50 | 325.371 | 332.634 | 332.634 | 10 | 331.98 |
| laarzeso, ORDER BY rank (bm25) | 50 | 8.457 | 8.618 | 8.791 | 200 | 10.05 |
| rechnung, ORDER BY m.date (Join) | 50 | 347.924 | 626.654 | 626.654 | 10 | 344.78 |
| und, ORDER BY m.date (Join) | 50 | 465.696 | 498.483 | 498.483 | 10 | 470.12 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 73.432 | 73.862 | 73.919 | 28 | 77.51 |
| count(*) und | 1 | 16.684 | 17.381 | 17.615 | 120 | 18.61 |
