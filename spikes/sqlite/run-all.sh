#!/usr/bin/env bash
# Runs every measurement of the spike in sequence. Takes about 50 minutes and needs ~6 GB in .data/
# at peak. Tables are appended to results/<name>.md. Databases are deleted at the end.
set -euo pipefail
cd "$(dirname "$0")"
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cargo build --release
B=./target/release/spike-sqlite
rm -rf results && mkdir -p .data

$B micro
# Id density: seconds << 8 and << 20 against the default (plain seconds), effect on FTS size.
SPIKE_ID_SHIFT=8 SPIKE_DB_NAME=u61-shift8 $B build u61 && rm -f .data/u61-shift8.db*
SPIKE_ID_SHIFT=20 SPIKE_DB_NAME=u61-shift20 $B build u61 && rm -f .data/u61-shift20.db*
# Writer page cache 256 MB instead of 64 MB.
SPIKE_CACHE_MB=256 SPIKE_DB_NAME=base-cache256 $B build base && rm -f .data/base-cache256.db*
SPIKE_CACHE_MB=256 SPIKE_DB_NAME=u61-cache256 $B build u61 && rm -f .data/u61-cache256.db*

$B build base && $B bench base mmap list && rm -f .data/base.db*
export SPIKE_READER_CACHE_MB=8
$B build u61 && $B bench u61 mmap all && $B bench u61 nommap all
$B build tri && $B bench tri mmap all
# The following steps change the databases (new mails, bodies), so they run after the benches.
$B concurrent u61 20
$B concurrent tri 20
$B reindex u61 20000
$B optimize u61 && $B bench u61 mmap search && rm -f .data/u61.db*
$B optimize tri && $B bench tri mmap search && rm -f .data/tri.db*
for v in u61all triall trihdr; do $B build $v && $B bench $v mmap search && rm -f .data/$v.db*; done
