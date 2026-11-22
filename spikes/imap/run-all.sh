#!/usr/bin/env bash
# Spike A: complete, reproducible run. Fresh test server with ~9959 messages in INBOX,
# measurement, all scenarios, server-shutdown scenario, unit tests, OpenSSL check.
# Leaves the test server stopped. Output also goes to results/.
set -euo pipefail
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
TM="$ROOT/scripts/testmail.sh"
. "$HOME/.cargo/env" 2>/dev/null || true
mkdir -p results

cargo build --release
"$TM" stop >/dev/null 2>&1 || true
trap '"$TM" stop >/dev/null 2>&1 || true' EXIT
"$TM" reset
"$TM" start
"$TM" seed 10000 --filler-folder INBOX

./target/release/imap-spike bench --runs 5 | tee results/bench.txt
./target/release/imap-spike all | tee results/all.txt
./target/release/imap-spike shutdown | tee results/shutdown.txt   # stops and restarts the server
cargo test --release 2>&1 | tail -3

if cargo tree -i openssl-sys >/dev/null 2>&1; then
    echo "FAIL openssl-sys is in the dependency tree"; exit 1
else
    echo "PASS no openssl-sys in the dependency tree"
fi
