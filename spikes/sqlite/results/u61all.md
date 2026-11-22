
## Aufbau `u61all` (FTS Some(Unicode61), Body AllShort(120), Batch 500, FTS sortiert true, Skala 1, id = Sekunden << 8, Cache 64 MB)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 1.95 2.40 3.18 }
- Mails 500000 / Orte 532598 (davon 32598 per Message-ID zusammengeführt) / Anhänge 150650 / Bodies im Index 500000 / Transaktionen 1098 / ID-Kollisionen 617
- Einfügerate (nur DB-Zeit 87.3 s): **5729 Mails/s**, 6102 Orte/s; Erzeugung 9.1 s; gesamt 96.5 s
- WAL maximal während des Ladens 17.3 MB / am Ende 0.0 MB; ANALYZE 0.2 s; Datei nach Checkpoint **802.1 MB**
- Indexierter Text 424.1 MB (subject 17.0 MB, sender 17.6 MB, recipients 32.1 MB, body 354.5 MB, attachments 2.9 MB)
- Größe ohne FTS 349.6 MB / FTS 450.5 MB
- Objekte: message_fts_data 438.5 MB, message 245.1 MB, message_msgid 29.9 MB, message_location 27.2 MB, loc_uid 16.4 MB, attachment 11.8 MB, message_fts_docsize 11.1 MB, message_thread 8.3 MB, loc_message 7.5 MB, attachment_message 2.5 MB

## Messung `u61all` (mmap true)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 1.39 2.08 2.97 }
Seltenes Wort `laarzeso` steht in 109 Mails.
Diakritika: `über` 51679 Treffer, `uber` 51679 Treffer
Diakritika: `grüße` 72754 Treffer, `gruße` 72754 Treffer
Diakritika: `grüße` 72754 Treffer, `grusse` 0 Treffer
Diakritika: `müller` 12623 Treffer, `mueller` 12259 Treffer

### EXPLAIN QUERY PLAN

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

### Suche neueste zuerst

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| häufig: rechnung | 50 | 0.082 | 0.086 | 0.092 | 200 | 0.25 |
| sehr häufig: und | 50 | 0.073 | 0.075 | 0.080 | 200 | 0.19 |
| selten: laarzeso | 50 | 0.055 | 0.059 | 0.063 | 200 | 0.21 |
| Präfix: rech* | 50 | 21.686 | 22.275 | 22.340 | 93 | 21.52 |
| Phrase: "vielen dank" | 50 | 0.148 | 0.158 | 0.281 | 200 | 0.45 |
| zwei Wörter: rechnung bestellung | 50 | 0.161 | 0.168 | 0.246 | 200 | 0.45 |
| Umlaut: grüße | 50 | 0.076 | 0.079 | 0.089 | 200 | 0.24 |
| Spalte: sender:müller | 50 | 0.077 | 0.080 | 0.085 | 200 | 0.24 |
| Teilwort Schritt 1: fts5vocab LIKE '%auszug%' | 61 | 1428.585 | 1448.091 | 1448.091 | 6 | 1441.54 |
| Teilwort Schritt 2: OR über 61 Terme | 50 | 2.890 | 2.947 | 3.011 | 200 | 4.29 |

### Suche mit Filter

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung + Konto 4 (40k) | 50 | 0.168 | 0.174 | 0.185 | 200 | 0.52 |
| rechnung + Konto 5 (10k) | 50 | 0.688 | 0.698 | 0.713 | 200 | 1.69 |
| rechnung + letzte 365 Tage | 50 | 0.080 | 0.080 | 0.086 | 200 | 0.20 |
| rechnung + ungelesen | 50 | 0.145 | 0.149 | 0.159 | 200 | 0.36 |
| rechnung + Konto 4 + 365 Tage + ungelesen | 50 | 0.486 | 0.497 | 0.543 | 200 | 1.02 |

### Suche andere Reihenfolge

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| rechnung, ORDER BY rank (bm25) | 50 | 48.200 | 49.925 | 50.189 | 42 | 50.95 |
| und, ORDER BY rank (bm25) | 50 | 307.811 | 321.355 | 321.355 | 10 | 316.26 |
| laarzeso, ORDER BY rank (bm25) | 50 | 0.140 | 0.144 | 0.155 | 200 | 0.57 |
| rechnung, ORDER BY m.date (Join) | 50 | 106.184 | 108.386 | 108.879 | 19 | 114.73 |
| und, ORDER BY m.date (Join) | 50 | 453.497 | 466.785 | 466.785 | 10 | 461.89 |

### Trefferzahl

| Abfrage | Zeilen | Median ms | p95 ms | max ms | n | kalt ms |
|---|---:|---:|---:|---:|---:|---:|
| count(*) rechnung | 1 | 3.027 | 3.118 | 3.194 | 200 | 3.36 |
| count(*) und | 1 | 15.185 | 15.407 | 15.568 | 132 | 16.60 |
