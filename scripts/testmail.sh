#!/usr/bin/env bash
# Local IMAP/SMTP test server for Vice Mail development (Stalwart, one binary).
#
#   scripts/testmail.sh start            start the server; downloads and provisions it on first use
#   scripts/testmail.sh stop             stop the server
#   scripts/testmail.sh status           state, ports, memory, data size (exit 3 when stopped)
#   scripts/testmail.sh seed [N] [opts]  store N test messages in alice's account (default 1000)
#   scripts/testmail.sh reset            delete data and certificates, provision from scratch (stays stopped)
#   scripts/testmail.sh check            prove IDLE, CONDSTORE, MOVE, SPECIAL-USE, keywords and SMTP
#   scripts/testmail.sh log [N]          last N lines of the server log (default 50)
#   scripts/testmail.sh lock NAME [MIN]  take the shared lock for exclusive use (waits up to MIN minutes, default 60)
#   scripts/testmail.sh unlock NAME      release the lock taken by NAME
#   scripts/testmail.sh lockinfo         who holds the lock
#
# Several sessions share one server: take the lock before reset/start/seed and tests, release it after.
# Everything listens on 127.0.0.1 only. Data lives in testmail/.data/, the binary in testmail/bin/.
# Ports and accounts: testmail/accounts.env. Details: testmail/README.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TESTMAIL="$ROOT/testmail"
DATA="$TESTMAIL/.data"
BIN_DIR="$TESTMAIL/bin"
TLS_DIR="$DATA/tls"
STORE_DIR="$DATA/store"
TMP_DIR="$DATA/tmp"
CONFIG="$DATA/config.json"
PID_FILE="$DATA/stalwart.pid"
LOG_FILE="$DATA/stalwart.log"
PROVISION_LOG="$DATA/provision.log"
MARKER="$DATA/provisioned"
JMAP_MARKER="$DATA/provisioned-jmap"

STALWART_VERSION="1.1.1"
STALWART_URL="https://github.com/stalwartlabs/stalwart/releases/download/v$STALWART_VERSION"
BIN="$BIN_DIR/stalwart-$STALWART_VERSION"
START_TIMEOUT=30
STOP_TIMEOUT=20
# Closed local port used as HTTP(S) proxy for the server process, so it cannot download anything.
DEAD_PROXY="http://127.0.0.1:9"

PYTHON="${PYTHON:-python3}"
export PYTHONDONTWRITEBYTECODE=1

# shellcheck source=../testmail/accounts.env
. "$TESTMAIL/accounts.env"
BASE_PORTS="$TESTMAIL_IMAP_PORT $TESTMAIL_IMAPS_PORT $TESTMAIL_SUBMISSION_PORT $TESTMAIL_SMTPS_PORT $TESTMAIL_SMTP_PORT $TESTMAIL_HTTP_PORT"
# Stage 14 (decision 0041): JMAP over HTTPS. Stores provisioned before get the listener on start.
ALL_PORTS="$BASE_PORTS $TESTMAIL_HTTPS_PORT"

log() { printf 'testmail: %s\n' "$*"; }
die() { printf 'testmail: error: %s\n' "$*" >&2; exit 1; }

now_ms() { "$PYTHON" -c 'import time; print(int(time.time() * 1000))'; }

# Release asset and its SHA-256 (from the GitHub release metadata of v1.1.1).
platform_asset() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)  echo "stalwart-aarch64-apple-darwin.tar.gz c5183bcd29f635cd7bbdf03714dd1974fec8e6557c1c0636be9d8c8e39ae7fd2" ;;
    Darwin-x86_64) echo "stalwart-x86_64-apple-darwin.tar.gz 37cf083326fed8e931ffdc641a93a288398ac5b143cf69b62d0705f81e318361" ;;
    Linux-x86_64)  echo "stalwart-x86_64-unknown-linux-gnu.tar.gz e15ac79fbc9a80828bc6563bd1576245fcef66497ed5636d466a7f0c79912a19" ;;
    Linux-aarch64|Linux-arm64) echo "stalwart-aarch64-unknown-linux-gnu.tar.gz a278a105537f22a316b0f0653e4f263a0ebf0b8eddbac2e390cb2b87e8dba57b" ;;
    *) die "unsupported platform $(uname -s) $(uname -m); see testmail/README.md" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

ensure_binary() {
  [ -x "$BIN" ] && return 0
  local spec asset expected tmp actual
  spec="$(platform_asset)" || exit 1
  asset="${spec%% *}"
  expected="${spec##* }"
  mkdir -p "$BIN_DIR"
  tmp="$(mktemp -d "$BIN_DIR/.download.XXXXXX")"
  log "downloading Stalwart $STALWART_VERSION ($asset)"
  if ! curl -fL --retry 3 --silent --show-error -o "$tmp/$asset" "$STALWART_URL/$asset"; then
    rm -rf "$tmp"; die "download of $STALWART_URL/$asset failed"
  fi
  actual="$(sha256_of "$tmp/$asset")"
  if [ "$actual" != "$expected" ]; then
    rm -rf "$tmp"; die "SHA-256 mismatch for $asset: expected $expected, got $actual"
  fi
  if ! tar -xzf "$tmp/$asset" -C "$tmp" stalwart; then
    rm -rf "$tmp"; die "could not unpack $asset"
  fi
  chmod 755 "$tmp/stalwart"
  find "$BIN_DIR" -maxdepth 1 -type f -name 'stalwart-*' -delete   # older versions
  mv "$tmp/stalwart" "$BIN"
  rm -rf "$tmp"
  log "installed $("$BIN" --version | head -n 1) at ${BIN#"$ROOT"/} (SHA-256 of the archive verified)"
}

port_open() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

# True while the process exists and is not a zombie.
alive() { kill -0 "$1" 2>/dev/null && ! ps -p "$1" -o stat= 2>/dev/null | grep -q Z; }

ensure_ports_free() {
  local port
  for port in "$@"; do
    if port_open "$port"; then
      die "port $port is already in use: $(lsof -nP -iTCP:"$port" -sTCP:LISTEN 2>/dev/null | awk 'NR==2 {print $1 " pid " $2}')"
    fi
  done
}

# Prints the PID if the server from this checkout is running.
server_pid() {
  local pid cmd
  [ -f "$PID_FILE" ] || return 1
  pid="$(cat "$PID_FILE")"
  case "$pid" in ''|*[!0-9]*) return 1 ;; esac
  kill -0 "$pid" 2>/dev/null || return 1
  cmd="$(ps -p "$pid" -o command= 2>/dev/null)" || return 1
  case "$cmd" in "$BIN_DIR"/stalwart-*) echo "$pid" ;; *) return 1 ;; esac
}

# Waits for all given ports; returns 2 if the process died, 1 on timeout.
wait_for_ports() {
  local pid="$1" deadline=$((SECONDS + START_TIMEOUT)) port
  shift
  for port in "$@"; do
    until port_open "$port"; do
      alive "$pid" || return 2
      [ "$SECONDS" -lt "$deadline" ] || return 1
      sleep 0.1
    done
  done
}

# Sends SIGTERM, waits, then SIGKILL. Reaps the process if it is our child.
stop_pid() {
  local pid="$1" i=0
  kill -TERM "$pid" 2>/dev/null || return 0
  while alive "$pid"; do
    i=$((i + 1))
    if [ "$i" -gt $((STOP_TIMEOUT * 10)) ]; then
      log "server did not stop within ${STOP_TIMEOUT}s, sending SIGKILL"
      kill -KILL "$pid" 2>/dev/null || true
      break
    fi
    sleep 0.1
  done
  wait "$pid" 2>/dev/null || true
}

write_config() {
  mkdir -p "$STORE_DIR"
  # Only the data store lives in the file; everything else is in the store itself.
  "$PYTHON" -c 'import json, sys; print(json.dumps({"@type": "RocksDb", "path": sys.argv[1],
      "bufferSize": 32 * 1024 * 1024, "cacheSize": 64 * 1024 * 1024}))' "$STORE_DIR/" >"$CONFIG"
}

make_tls() {
  if [ -f "$TLS_DIR/ca.pem" ] && [ -f "$TLS_DIR/server.pem" ] && [ -f "$TLS_DIR/server-key.pem" ]; then
    return 0
  fi
  command -v openssl >/dev/null 2>&1 || die "openssl is required to create the test certificates"
  rm -rf "$TLS_DIR"
  mkdir -p "$TLS_DIR"
  cat >"$TLS_DIR/openssl.cnf" <<'EOF'
[req]
distinguished_name = dn
prompt = no
[dn]
CN = unused
[ca]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign, cRLSign
subjectKeyIdentifier = hash
[server]
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature
extendedKeyUsage = serverAuth
subjectAltName = DNS:localhost, IP:127.0.0.1, IP:::1
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid, issuer
EOF
  (
    cd "$TLS_DIR"
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out ca-key.pem
    openssl req -x509 -new -key ca-key.pem -sha256 -days 3650 -config openssl.cnf -extensions ca \
      -subj "/O=Vice Mail testmail/CN=Vice Mail Test CA (local only)" -out ca.pem
    openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out server-key.pem
    openssl req -new -key server-key.pem -config openssl.cnf -subj "/O=Vice Mail testmail/CN=localhost" \
      -out server.csr
    # 825 days: the longest validity Apple platforms accept for certificates from private CAs.
    openssl x509 -req -in server.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -CAserial ca.srl \
      -days 825 -sha256 -extfile openssl.cnf -extensions server -out server.pem
    rm -f server.csr
    chmod 600 ca-key.pem server-key.pem
  ) >/dev/null 2>"$TLS_DIR/openssl.log" || { cat "$TLS_DIR/openssl.log" >&2; die "certificate generation failed"; }
  log "created test CA and server certificate in ${TLS_DIR#"$ROOT"/}"
}

# Starts Stalwart in the background; extra NAME=VALUE arguments go into its environment.
# Sets LAUNCHED_PID (a variable, so the process stays a child of this shell).
launch() {
  local logfile="$1"
  shift
  write_config
  mkdir -p "$TMP_DIR"
  (
    cd "$DATA"
    # The proxy variables point at a closed local port: any HTTP download the server might still
    # attempt (web UI bundle, spam rules, GeoIP data) fails at once instead of reaching the internet.
    exec env -u STALWART_RECOVERY_MODE -u STALWART_RECOVERY_ADMIN TMPDIR="$TMP_DIR" \
      STALWART_HOSTNAME="mail.$TESTMAIL_DOMAIN" \
      STALWART_PUBLIC_URL="https://localhost:$TESTMAIL_HTTPS_PORT" \
      HTTP_PROXY="$DEAD_PROXY" HTTPS_PROXY="$DEAD_PROXY" ALL_PROXY="$DEAD_PROXY" \
      http_proxy="$DEAD_PROXY" https_proxy="$DEAD_PROXY" all_proxy="$DEAD_PROXY" \
      NO_PROXY="127.0.0.1,localhost,::1" no_proxy="127.0.0.1,localhost,::1" \
      "$@" nohup "$BIN" --config "$CONFIG"
  ) >>"$logfile" 2>&1 </dev/null &
  LAUNCHED_PID=$!
}

kill_launched() {
  if [ -n "${LAUNCHED_PID:-}" ] && alive "$LAUNCHED_PID"; then stop_pid "$LAUNCHED_PID"; fi
}

# Starts the server (launch arguments after the log file), waits for the ports, dies on failure.
launch_and_wait() {
  local logfile="$1" ports="$2" rc=0
  shift 2
  launch "$logfile" "$@"
  # shellcheck disable=SC2086
  wait_for_ports "$LAUNCHED_PID" $ports || rc=$?
  if [ "$rc" -ne 0 ]; then
    stop_pid "$LAUNCHED_PID"
    rm -f "$PID_FILE"
    tail -n 30 "$logfile" >&2
    if [ "$rc" -eq 2 ]; then die "server exited during startup (log: ${logfile#"$ROOT"/})"; fi
    die "server did not open its ports within ${START_TIMEOUT}s (log: ${logfile#"$ROOT"/})"
  fi
}

# Fresh store + certificates + configuration. Phase 1 runs in Stalwart's recovery mode (only the
# management API is up); phase 2 is the first normal start, which creates default MTA objects
# that are then adjusted as admin@vicemail.test.
provision() {
  local pw pid t0
  t0="$(now_ms)"
  rm -rf "$STORE_DIR" "$TMP_DIR" "$MARKER" "$JMAP_MARKER"
  mkdir -p "$DATA" "$TMP_DIR"
  make_tls
  # shellcheck disable=SC2086
  ensure_ports_free $ALL_PORTS
  pw="$("$PYTHON" -c 'import secrets; print(secrets.token_urlsafe(32))')"
  : >"$PROVISION_LOG"
  trap 'kill_launched' EXIT   # never leave a provisioning server behind on errors or Ctrl-C
  # Stalwart 1.1.1 ignores STALWART_RECOVERY_MODE_BIND and listens on all interfaces for these
  # few seconds; the random one-time password protects it and is discarded afterwards.
  launch_and_wait "$PROVISION_LOG" "$TESTMAIL_HTTP_PORT" \
    STALWART_RECOVERY_MODE=1 STALWART_RECOVERY_ADMIN="provision:$pw" \
    STALWART_RECOVERY_MODE_PORT="$TESTMAIL_HTTP_PORT" STALWART_RECOVERY_MODE_BIND=127.0.0.1 \
    STALWART_RECOVERY_MODE_LOG_LEVEL=info
  pid=$LAUNCHED_PID
  if ! TESTMAIL_PROVISION_PASSWORD="$pw" "$PYTHON" "$TESTMAIL/provision.py" --phase recovery \
      --url "http://127.0.0.1:$TESTMAIL_HTTP_PORT" --tls-dir "$TLS_DIR"; then
    stop_pid "$pid"
    die "provisioning failed (server log: ${PROVISION_LOG#"$ROOT"/})"
  fi
  stop_pid "$pid"
  launch_and_wait "$PROVISION_LOG" "$ALL_PORTS"
  pid=$LAUNCHED_PID
  if ! "$PYTHON" "$TESTMAIL/provision.py" --phase defaults --url "http://127.0.0.1:$TESTMAIL_HTTP_PORT"; then
    stop_pid "$pid"
    die "provisioning failed (server log: ${PROVISION_LOG#"$ROOT"/})"
  fi
  stop_pid "$pid"
  trap - EXIT
  rm -rf "$TMP_DIR"
  printf 'stalwart %s provisioned %s\n' "$STALWART_VERSION" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$MARKER"
  date -u +%Y-%m-%dT%H:%M:%SZ >"$JMAP_MARKER"
  log "provisioned in $(( $(now_ms) - t0 )) ms"
}

# A store provisioned before stage 14 has no HTTPS listener: add it (as admin) and keep the data.
add_jmap_listener() {
  local pid
  # shellcheck disable=SC2086
  ensure_ports_free $ALL_PORTS
  launch_and_wait "$PROVISION_LOG" "$BASE_PORTS"
  pid=$LAUNCHED_PID
  if ! "$PYTHON" "$TESTMAIL/provision.py" --phase jmap --url "http://127.0.0.1:$TESTMAIL_HTTP_PORT"; then
    stop_pid "$pid"
    die "adding the HTTPS listener failed (server log: ${PROVISION_LOG#"$ROOT"/})"
  fi
  stop_pid "$pid"
  date -u +%Y-%m-%dT%H:%M:%SZ >"$JMAP_MARKER"
  log "added the HTTPS (JMAP) listener on $TESTMAIL_HTTPS_PORT"
}

cmd_start() {
  local pid t0
  ensure_binary
  if pid="$(server_pid)"; then
    log "already running (pid $pid)"
    return 0
  fi
  rm -f "$PID_FILE"
  if [ ! -f "$MARKER" ] || [ ! -d "$STORE_DIR" ]; then
    provision
  elif [ ! -f "$JMAP_MARKER" ]; then
    add_jmap_listener
  fi
  if ! openssl x509 -checkend 86400 -noout -in "$TLS_DIR/server.pem" >/dev/null 2>&1; then
    die "the test certificate is missing, expired or expires within a day; run 'scripts/testmail.sh reset'"
  fi
  # shellcheck disable=SC2086
  ensure_ports_free $ALL_PORTS
  if [ -f "$LOG_FILE" ] && [ "$(wc -c <"$LOG_FILE")" -gt 20000000 ]; then
    mv -f "$LOG_FILE" "$LOG_FILE.1"
  fi
  t0="$(now_ms)"
  launch_and_wait "$LOG_FILE" "$ALL_PORTS"
  echo "$LAUNCHED_PID" >"$PID_FILE"
  log "running (pid $LAUNCHED_PID), ready after $(( $(now_ms) - t0 )) ms"
  log "IMAP 127.0.0.1:$TESTMAIL_IMAP_PORT (STARTTLS) / $TESTMAIL_IMAPS_PORT (TLS), SMTP submission $TESTMAIL_SUBMISSION_PORT (STARTTLS) / $TESTMAIL_SMTPS_PORT (TLS), local SMTP $TESTMAIL_SMTP_PORT, HTTP $TESTMAIL_HTTP_PORT, HTTPS (JMAP) $TESTMAIL_HTTPS_PORT"
}

cmd_stop() {
  local pid
  if ! pid="$(server_pid)"; then
    rm -f "$PID_FILE"
    log "not running"
    return 0
  fi
  stop_pid "$pid"
  rm -f "$PID_FILE"
  log "stopped (pid $pid)"
}

cmd_status() {
  local pid port state
  if pid="$(server_pid)"; then
    log "running, pid $pid, Stalwart $STALWART_VERSION, RSS $(( $(ps -o rss= -p "$pid") / 1024 )) MB"
  else
    log "stopped"
  fi
  for port in $ALL_PORTS; do
    if port_open "$port"; then state=open; else state=closed; fi
    printf '  127.0.0.1:%-6s %s\n' "$port" "$state"
  done
  if [ -d "$STORE_DIR" ]; then
    printf '  data store       %s\n' "$(du -sh "$STORE_DIR" | cut -f1)"
  else
    printf '  data store       not provisioned\n'
  fi
  if [ -f "$TLS_DIR/server.pem" ]; then
    printf '  certificate      %s\n' "$(openssl x509 -noout -enddate -in "$TLS_DIR/server.pem")"
  fi
  [ -n "${pid:-}" ]  || return 3
}

cmd_reset() {
  local pid
  case "$DATA" in */testmail/.data) ;; *) die "refusing to delete unexpected path $DATA" ;; esac
  if pid="$(server_pid)"; then
    log "stopping the running server"
    stop_pid "$pid"
  fi
  rm -rf "$DATA"
  log "deleted ${DATA#"$ROOT"/}"
  ensure_binary
  provision
  log "reset done; the server is stopped (run 'scripts/testmail.sh start')"
}

require_running() {
  server_pid >/dev/null || die "the server is not running; run 'scripts/testmail.sh start' first"
}

# Shared lock: a directory outside .data (reset deletes .data). mkdir is atomic.
LOCK_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/testmail/.lock"

cmd_lock() {
  local name="${1:?usage: testmail.sh lock NAME [MINUTES]}" minutes="${2:-60}" waited=0
  while ! mkdir "$LOCK_DIR" 2>/dev/null; do
    if [ "$(cat "$LOCK_DIR/owner" 2>/dev/null)" = "$name" ]; then
      echo "testmail: lock already held by $name"; return 0
    fi
    if [ "$waited" -ge $((minutes * 60)) ]; then
      echo "testmail: lock still held by $(cat "$LOCK_DIR/owner" 2>/dev/null || echo '?') after $minutes min" >&2
      return 4
    fi
    sleep 5; waited=$((waited + 5))
  done
  printf '%s\n' "$name" > "$LOCK_DIR/owner"
  date -u +%Y-%m-%dT%H:%M:%SZ > "$LOCK_DIR/since"
  echo "testmail: lock taken by $name"
}

cmd_unlock() {
  local name="${1:?usage: testmail.sh unlock NAME}"
  [ -d "$LOCK_DIR" ] || { echo "testmail: no lock held"; return 0; }
  local owner; owner="$(cat "$LOCK_DIR/owner" 2>/dev/null || true)"
  [ "$owner" = "$name" ] || { echo "testmail: lock is held by '$owner', not '$name'" >&2; return 4; }
  rm -rf "$LOCK_DIR"; echo "testmail: lock released by $name"
}

cmd_lockinfo() {
  if [ -d "$LOCK_DIR" ]; then
    echo "held by $(cat "$LOCK_DIR/owner" 2>/dev/null || echo '?') since $(cat "$LOCK_DIR/since" 2>/dev/null || echo '?')"
  else
    echo "free"
  fi
}

usage() {
  sed -n '2,17p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

main() {
  local cmd="${1:-}"
  [ $# -gt 0 ] && shift
  case "$cmd" in
    start) cmd_start ;;
    stop) cmd_stop ;;
    status) cmd_status ;;
    reset) cmd_reset ;;
    seed) require_running; "$PYTHON" "$TESTMAIL/seed.py" "$@" ;;
    check) require_running; "$PYTHON" "$TESTMAIL/check.py" "$@" ;;
    log) tail -n "${1:-50}" "$LOG_FILE" ;;
    lock) cmd_lock "$@" ;;
    unlock) cmd_unlock "$@" ;;
    lockinfo) cmd_lockinfo ;;
    -h|--help|help) usage ;;
    *) usage >&2; exit 2 ;;
  esac
}

main "$@"
