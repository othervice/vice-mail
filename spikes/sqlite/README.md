# Spike B: SQLite schema and full-text search

Throwaway code for stage 0. It generates 500,000 synthetic mails over five accounts, loads them
like an initial IMAP sync would (folder by folder, newest first, 500 mails per transaction) and
measures list pages, unread counters, FTS5 search with `unicode61` and `trigram`, insert rate,
file size and read latency while a writer is active. Results and the decision are in
`docs/decisions/0003-sqlite-schema-und-suche.md`. Product code never imports this crate.

## Run

```sh
cargo build --release
B=./target/release/spike-sqlite

$B build base        # schema without FTS
$B build u61         # unicode61 remove_diacritics 2, body text for ~20 % of mails
$B build tri         # trigram remove_diacritics 1, same text
$B build u61all      # unicode61, body for every mail (first 120 words)
$B build triall      # trigram, body for every mail (first 120 words)
$B build trihdr      # trigram over headers only (no body)

$B bench u61         # list pages, counters, searches, EXPLAIN QUERY PLAN
$B bench u61 nommap  # same with mmap_size = 0
$B concurrent u61 15 # reader alone vs. reader with a continuous writer, 15 s each
$B reindex u61 20000 # add bodies to already indexed mails (INSERT OR REPLACE)
$B optimize u61      # FTS5 'optimize', size before/after
$B micro             # insert rate by batch size, sorted vs. unsorted FTS inserts (50k mails)
$B sample            # print a few generated mails
```

Environment: `SPIKE_ID_SHIFT` (bits below the seconds in the time-ordered id, default 0),
`SPIKE_CACHE_MB` (writer page cache, default 64), `SPIKE_READER_CACHE_MB` (reader page cache,
default 32), `SPIKE_DB_NAME` (database file name in `.data/`, default: the variant).
`run-all.sh` runs the whole series (about 45 minutes).

Databases go to `.data/` (several GB for all variants; delete it afterwards). Every command
appends its tables to `results/<name>.md` (`<name>` = `SPIKE_DB_NAME` or the variant).
`run-all.sh` runs the whole series and deletes the databases at the end.

`results/` holds the run of 22.11.2026: `micro.md`, `base.md`, `u61*.md`, `tri*.md` from the series
(id = seconds << 8 unless the heading says otherwise), `final-u61.md` and `final-tri.md` from the last
run with the chosen design (id = plain seconds, reader cache 8 MB). Each section notes the machine load;
other sessions were building in parallel.

Dependency: `rusqlite` 0.43 with `bundled` (SQLite 3.54.1, FTS5 and dbstat enabled), MIT.
