# Sourced by scripts/build-macos.sh and scripts/build-windows.sh when VICEMAIL_UPDATER=1
# (decision 0037, docs/RELEASING.md). Reads the update signing key and its password from the
# login keychain (service org.vicemail.updater, or VICEMAIL_UPDATER_SERVICE during a key
# rotation) into this build's environment only, where `tauri build` with `createUpdaterArtifacts`
# finds them. Nothing is printed or written.
# shellcheck shell=bash

updater_signing_env() { # caller name for messages
  local who="$1"
  command -v security >/dev/null || {
    printf '%s: VICEMAIL_UPDATER=1 needs the macOS keychain (security missing)\n' "$who" >&2
    return 1
  }
  local key password service="${VICEMAIL_UPDATER_SERVICE:-org.vicemail.updater}"
  key="$(security find-generic-password -s "$service" -a private-key -w 2>/dev/null)" || {
    printf '%s: no update signing key in the keychain (service %s); see docs/RELEASING.md.\n' "$who" "$service" >&2
    printf '%s: over SSH the login keychain may be locked: security unlock-keychain ~/Library/Keychains/login.keychain-db\n' "$who" >&2
    return 1
  }
  password="$(security find-generic-password -s "$service" -a password -w 2>/dev/null)" || {
    printf '%s: the update signing key has no password item in the keychain; see docs/RELEASING.md\n' "$who" >&2
    return 1
  }
  export TAURI_SIGNING_PRIVATE_KEY="$key"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password"
}
