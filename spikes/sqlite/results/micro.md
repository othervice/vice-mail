
## Einfügen: Batchgröße und FTS-Reihenfolge (Skala 0.1 = 50.000 Mails)

Rechner: Apple M4 / 10 Kerne / 16 GB RAM / 27.2.1 / SQLite 3.54.1 / Last: { 4.06 3.91 3.59 }
- base, Batch 100, FTS sortiert true: **28795 Mails/s** (DB 1.74 s), Datei+WAL 32.9 MB
- base, Batch 500, FTS sortiert true: **39803 Mails/s** (DB 1.26 s), Datei+WAL 32.9 MB
- base, Batch 2000, FTS sortiert true: **44821 Mails/s** (DB 1.12 s), Datei+WAL 32.9 MB
- u61, Batch 100, FTS sortiert true: **13630 Mails/s** (DB 3.67 s), Datei+WAL 60.3 MB
- u61, Batch 500, FTS sortiert true: **18968 Mails/s** (DB 2.64 s), Datei+WAL 61.2 MB
- u61, Batch 2000, FTS sortiert true: **21036 Mails/s** (DB 2.38 s), Datei+WAL 60.7 MB
- u61, Batch 500, FTS sortiert false: **12891 Mails/s** (DB 3.88 s), Datei+WAL 61.0 MB
- tri, Batch 500, FTS sortiert true: **6814 Mails/s** (DB 7.34 s), Datei+WAL 175.6 MB
