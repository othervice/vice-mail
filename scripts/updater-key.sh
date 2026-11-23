#!/usr/bin/env bash
# The update signing key (decision 0037): a minisign key pair made by `tauri signer generate`.
# The private key and its password live only in the login keychain of the build Mac, as two
# generic passwords of the service `org.vicemail.updater`; the public key is in
# app/src-tauri/tauri.conf.json (plugins.updater.pubkey). See docs/RELEASING.md for backup and
# rotation.
#
#   scripts/updater-key.sh generate   make a new key pair, store it in the keychain, print the
#                                     public key (refuses if a key is stored already)
#   scripts/updater-key.sh check      confirm that both keychain items are readable and that the
#                                     stored public key matches tauri.conf.json
#   scripts/updater-key.sh public     print the stored public key
#
# VICEMAIL_UPDATER_SERVICE names another keychain service (key rotation, docs/RELEASING.md).
#
# Nothing secret is written to a file, a log or the command line of a process: the password is
# made in memory, passed to the Tauri CLI through its in-process Node API and to `security`
# through its standard input.
set -euo pipefail

SERVICE="${VICEMAIL_UPDATER_SERVICE:-org.vicemail.updater}"
[[ "$SERVICE" =~ ^[A-Za-z0-9._-]+$ ]] || { printf 'updater-key: bad service name\n' >&2; exit 1; }
LABEL_PRIVATE="Vice Mail update signing key (private)"
LABEL_PASSWORD="Vice Mail update signing key (password)"
LABEL_PUBLIC="Vice Mail update signing key (public)"

usage() {
  sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'updater-key: %s\n' "$*" >&2
  exit 1
}

repo="$(cd "$(dirname "$0")/.." && pwd)"
conf="$repo/app/src-tauri/tauri.conf.json"

[[ "$(uname -s)" == Darwin ]] || fail "the signing key lives in the macOS keychain; run this on the build Mac"
command -v security >/dev/null || fail "security missing (part of macOS)"

item() { # account: prints the stored value, fails if it is missing
  security find-generic-password -s "$SERVICE" -a "$1" -w 2>/dev/null
}

# Stores one item; the value goes to `security` on its standard input, never as an argument.
store() { # account, label, value
  [[ "$3" =~ ^[A-Za-z0-9+/=_-]+$ ]] || fail "unexpected characters in the value for $1"
  printf 'add-generic-password -U -s %s -a %s -l "%s" -w %s\n' "$SERVICE" "$1" "$2" "$3" |
    security -i >/dev/null || fail "storing $1 in the keychain failed (is the login keychain unlocked?)"
}

config_pubkey() {
  node -e '
    const conf = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    process.stdout.write(conf.plugins?.updater?.pubkey ?? "");
  ' "$conf"
}

case "${1:-}" in
  generate)
    if item private-key >/dev/null; then
      fail "a signing key is stored already (service $SERVICE); see docs/RELEASING.md before replacing it"
    fi
    command -v node >/dev/null || fail "node missing: brew install node"
    [[ -d "$repo/app/node_modules/@tauri-apps/cli" ]] || fail "app dependencies missing: pnpm --dir app install"
    password="$(head -c 32 /dev/urandom | base64 | tr -d '\n=' | tr '+/' '-_')"
    [[ ${#password} -ge 40 ]] || fail "could not make a password"
    # In-process CLI call: the password stays out of every process's argument list.
    output="$(cd "$repo/app" && VICEMAIL_KEYGEN_PASSWORD="$password" node -e '
      const cli = require("@tauri-apps/cli");
      cli.run(["signer", "generate", "--ci", "-p", process.env.VICEMAIL_KEYGEN_PASSWORD], "tauri")
        .catch((err) => { console.error(String(err)); process.exit(1); });
    ')" || fail "tauri signer generate failed"
    private="$(printf '%s\n' "$output" | awk '/^Private:/ { getline; print; exit }')"
    public="$(printf '%s\n' "$output" | awk '/^Public:/ { getline; print; exit }')"
    output=""
    [[ -n "$private" && -n "$public" ]] || fail "could not read the keys from the signer's output"
    store private-key "$LABEL_PRIVATE" "$private"
    store password "$LABEL_PASSWORD" "$password"
    store public-key "$LABEL_PUBLIC" "$public"
    private=""
    password=""
    printf 'updater-key: key pair stored in the login keychain (service %s).\n' "$SERVICE" >&2
    printf 'updater-key: back it up now, see docs/RELEASING.md. Public key for tauri.conf.json:\n' >&2
    printf '%s\n' "$public"
    ;;
  check)
    item private-key >/dev/null || fail "no private key in the keychain (service $SERVICE, account private-key)"
    item password >/dev/null || fail "no key password in the keychain (service $SERVICE, account password)"
    public="$(item public-key)" || fail "no public key in the keychain (service $SERVICE, account public-key)"
    command -v node >/dev/null || fail "node missing: brew install node"
    [[ -n "${VICEMAIL_UPDATER_PUBKEY:-}" || "$(config_pubkey)" == "$public" ]] ||
      fail "the public key in $conf does not match the key in the keychain"
    printf 'updater-key: private key and password readable; public key matches tauri.conf.json\n'
    ;;
  public)
    item public-key || fail "no public key in the keychain (service $SERVICE, account public-key)"
    printf '\n'
    ;;
  -h | --help)
    usage
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
