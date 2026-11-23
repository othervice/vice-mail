#!/usr/bin/env bash
# Builds the macOS app (.app) and disk image (.dmg) for Apple silicon, ad-hoc signed: no developer
# signature and no notarisation (both stage 15, see docs/INSTALL.md). The MCP helper `mail-mcp`
# goes into Contents/MacOS as a Tauri sidecar (stage 12, decision 0021). Checks the signatures and
# the disk image, starts the app from the disk image under a temporary HOME, then prints path, size
# and SHA-256 of the .dmg.
#
#   scripts/build-macos.sh              version from tauri.conf.json, else the Cargo workspace
#   scripts/build-macos.sh 0.1.0-preview.1
#                                       this version for the app and the disk image (semver)
#
# Environment: CARGO_TARGET_DIR is honoured. SOURCE_DATE_EPOCH, if set, is the build date shown in
# About; otherwise the commit time of a clean tree or the current time of a dirty one.
# SKIP_LAUNCH_CHECK=1 skips the start test (it needs a logged-in GUI session).
set -euo pipefail

usage() {
  sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'build-macos: %s\n' "$*" >&2
  exit 1
}

version=""
case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  "") ;;
  *)
    version="$1"
    [[ $# -eq 1 ]] || fail "expected at most one argument (the version), got $#"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]] ||
      fail "version '$version' is not semver (e.g. 0.1.0 or 0.1.0-preview.1)"
    ;;
esac

repo="$(cd "$(dirname "$0")/.." && pwd)"

# Without CI, the DMG step lays out the Finder window by AppleScript and times out over SSH or
# tmux (decision 0004). With it, the image has no window layout but the same content.
export CI=true

# --- Preconditions ---------------------------------------------------------------------------

[[ "$(uname -s)" == Darwin ]] || fail "macOS builds need macOS"
# Apple silicon only: an Intel or universal build needs the x86_64-apple-darwin target and a
# second full build (docs/INSTALL.md, system requirements).
[[ "$(uname -m)" == arm64 ]] || fail "this script builds for Apple silicon and must run on it"

[[ -f "$HOME/.cargo/env" ]] || fail "Rust is not installed (~/.cargo/env missing); see docs/SETUP.md"
# shellcheck source=/dev/null
. "$HOME/.cargo/env"
command -v cargo >/dev/null || fail "cargo not found after loading ~/.cargo/env"
[[ -x /usr/bin/clang ]] || fail "Xcode command line tools missing: xcode-select --install"
for tool in codesign hdiutil plutil shasum; do
  command -v "$tool" >/dev/null || fail "$tool missing (part of macOS)"
done
command -v pnpm >/dev/null || fail "pnpm missing: brew install pnpm"
[[ -x "$repo/app/node_modules/.bin/tauri" ]] ||
  fail "app dependencies missing: pnpm --dir app install"

# Build date and the commit's -dirty mark come from app/src-tauri/build.rs, which cargo reruns
# only when SOURCE_DATE_EPOCH (among a few others) changes. So every build sets it: the commit
# time for a clean tree (reproducible), the current time for a dirty one. A caller's value wins.
if [[ -n "${SOURCE_DATE_EPOCH:-}" ]]; then
  [[ "$SOURCE_DATE_EPOCH" =~ ^[0-9]+$ ]] ||
    fail "SOURCE_DATE_EPOCH '$SOURCE_DATE_EPOCH' is not a Unix time in seconds"
elif git -C "$repo" rev-parse --verify -q HEAD >/dev/null 2>&1; then
  changes="$(git -C "$repo" --no-optional-locks status --porcelain --untracked-files=no)" ||
    fail "git status failed"
  if [[ -z "$changes" ]]; then
    SOURCE_DATE_EPOCH="$(git -C "$repo" log -1 --format=%ct HEAD)"
  else
    SOURCE_DATE_EPOCH="$(date +%s)"
  fi
fi
[[ -z "${SOURCE_DATE_EPOCH:-}" ]] || export SOURCE_DATE_EPOCH

tmp="$(mktemp -d)"
mountpoint=""
app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -KILL "$app_pid" 2>/dev/null || true
  fi
  if [[ -n "$mountpoint" ]]; then
    hdiutil detach -quiet -force "$mountpoint" 2>/dev/null || true
  fi
  rm -rf "$tmp"
}
trap cleanup EXIT

# --- Build -----------------------------------------------------------------------------------

target_dir="${CARGO_TARGET_DIR:-$repo/target}"
triple="$(rustc -vV | sed -n 's/^host: //p')"
[[ "$triple" == aarch64-apple-darwin ]] || fail "unexpected host triple '$triple'"

# The MCP helper (decision 0021): built first, then handed to Tauri as `bundle.externalBin`,
# which wants `<path>-<target triple>` and puts it into Contents/MacOS as `mail-mcp`. Only for
# this build (through --config), so plain cargo builds need no helper file.
printf 'build-macos: cargo build --release --locked -p mail-mcp --bin mail-mcp\n'
cargo build --release --locked -p mail-mcp --bin mail-mcp --manifest-path "$repo/Cargo.toml" ||
  fail "building the MCP helper failed"
sidecar_dir="$target_dir/sidecar"
mkdir -p "$sidecar_dir"
cp -f "$target_dir/release/mail-mcp" "$sidecar_dir/mail-mcp-$triple"

# Tauri's default leaves only the linker's signature on the binary: `codesign --verify` then
# fails ("code has no resources but signature indicates they must be present"), and a downloaded
# copy is reported as damaged. "-" seals the whole bundle ad hoc, without a developer identity.
config="\"bundle\":{\"macOS\":{\"signingIdentity\":\"-\"},\"externalBin\":[\"$sidecar_dir/mail-mcp\"]}"
if [[ -n "$version" ]]; then
  config="\"version\":\"$version\",$config"
fi
args=(build --bundles app,dmg --config "{$config}")

bundle_dir="$target_dir/release/bundle"
touch "$tmp/started"
started=$(date +%s)
printf 'build-macos: SOURCE_DATE_EPOCH=%s pnpm --dir app tauri %s\n' "${SOURCE_DATE_EPOCH:-}" "${args[*]}"
pnpm --dir "$repo/app" tauri "${args[@]}" || fail "tauri build failed (exit $?)"
seconds=$(($(date +%s) - started))

# --- Result ----------------------------------------------------------------------------------

newest() { # directory, name pattern, type: the one entry made by this build
  local found=()
  while IFS= read -r -d '' entry; do
    found+=("$entry")
  done < <(find "$1" -mindepth 1 -maxdepth 1 -name "$2" -type "$3" -newer "$tmp/started" -print0 2>/dev/null)
  [[ ${#found[@]} -eq 1 ]] || fail "expected one new $2 in $1, found ${#found[@]}"
  printf '%s\n' "${found[0]}"
}
dmg="$(newest "$bundle_dir/dmg" '*.dmg' f)"
app="$(newest "$bundle_dir/macos" '*.app' d)"

plist() { plutil -extract "$2" raw -o - "$1/Contents/Info.plist"; }
built_version="$(plist "$app" CFBundleShortVersionString)"
[[ -z "$version" || "$built_version" == "$version" ]] ||
  fail "the app reports version $built_version, expected $version"

# --- Checks ----------------------------------------------------------------------------------

codesign --verify --deep --strict "$app" 2>"$tmp/codesign.log" || {
  cat "$tmp/codesign.log" >&2
  fail "codesign --verify failed for $app"
}
signature="$(codesign -dv "$app" 2>&1 | sed -n 's/^Signature=//p')"
[[ "$signature" == adhoc ]] || fail "expected an ad-hoc signature, codesign reports '${signature:-none}'"

# The MCP helper: in Contents/MacOS, signed (ad hoc, like the app), and it runs.
helper="$app/Contents/MacOS/mail-mcp"
[[ -x "$helper" ]] || fail "the MCP helper is missing in $app/Contents/MacOS"
codesign --verify --strict "$helper" 2>"$tmp/codesign.log" || {
  cat "$tmp/codesign.log" >&2
  fail "codesign --verify failed for the MCP helper"
}
helper_signature="$(codesign -dv "$helper" 2>&1 | sed -n 's/^Signature=//p')"
[[ "$helper_signature" == adhoc ]] ||
  fail "expected an ad-hoc signature on the MCP helper, codesign reports '${helper_signature:-none}'"
helper_version="$("$helper" --version)" || fail "the MCP helper does not run"
[[ "$helper_version" == "mail-mcp "* ]] || fail "unexpected answer of the MCP helper: $helper_version"
# Gatekeeper rejects every app without a notarised Developer ID signature; that is expected here.
gatekeeper="$(spctl --assess --type execute "$app" 2>&1 || true)"

hdiutil verify -quiet "$dmg" || fail "hdiutil verify failed for $dmg"
mountpoint="$tmp/mount"
mkdir "$mountpoint"
hdiutil attach -quiet -readonly -nobrowse -noautoopen -mountpoint "$mountpoint" "$dmg" ||
  fail "cannot mount $dmg"
image_app="$mountpoint/$(basename "$app")"
[[ -d "$image_app" ]] || fail "$(basename "$app") is missing in the disk image"
[[ -x "$image_app/Contents/MacOS/mail-mcp" ]] || fail "the MCP helper is missing in the disk image"
[[ -L "$mountpoint/Applications" ]] || fail "the disk image has no Applications link to drag the app onto"
codesign --verify --deep --strict "$image_app" 2>"$tmp/codesign.log" || {
  cat "$tmp/codesign.log" >&2
  fail "codesign --verify failed for the app inside the disk image"
}

launch="skipped (SKIP_LAUNCH_CHECK=1)"
if [[ "${SKIP_LAUNCH_CHECK:-}" != 1 ]]; then
  # Start the copy users get, under a throwaway HOME: Tauri's directories follow HOME,
  # Foundation and WebKit follow CFFIXED_USER_HOME, so the real data directory stays untouched.
  identifier="$(plist "$image_app" CFBundleIdentifier)"
  home="$tmp/home"
  mkdir "$home"
  data_dir="$home/Library/Application Support/$identifier"
  log_dir="$home/Library/Logs/$identifier"
  HOME="$home" CFFIXED_USER_HOME="$home" \
    "$image_app/Contents/MacOS/$(plist "$image_app" CFBundleExecutable)" >"$tmp/app.out" 2>&1 &
  app_pid=$!
  started_log() { cat "$log_dir"/* 2>/dev/null | grep -m1 ' starting' || true; }
  for ((i = 0; i < 150; i++)); do
    [[ -f "$data_dir/mail.sqlite3" && -n "$(started_log)" ]] && break
    kill -0 "$app_pid" 2>/dev/null || break
    sleep 0.2
  done
  # A moment longer, so a crash right after the window opens shows up too.
  sleep 3
  if ! kill -0 "$app_pid" 2>/dev/null; then
    status=0
    wait "$app_pid" || status=$?
    app_pid=""
    cat "$tmp/app.out" >&2
    fail "the app exited during start (exit $status)"
  fi
  # The group's stderr also takes bash's "Terminated" notice for the job.
  quit=0
  {
    kill -TERM "$app_pid"
    for ((i = 0; i < 50; i++)); do
      kill -0 "$app_pid" || break
      sleep 0.2
    done
    if kill -0 "$app_pid"; then
      kill -KILL "$app_pid" || true
    else
      quit=1
    fi
    wait "$app_pid" || true
  } 2>/dev/null
  app_pid=""
  [[ $quit -eq 1 ]] || fail "the app did not quit within 10 s of SIGTERM"

  [[ -f "$data_dir/mail.sqlite3" ]] || fail "the app created no database in $data_dir"
  line="$(started_log)"
  [[ -n "$line" ]] || fail "the app log has no start line: $(cat "$log_dir"/* 2>/dev/null)"
  [[ "$line" == *"version=$built_version "* ]] ||
    fail "the app log reports another version than $built_version: $line"
  errors="$(cat "$log_dir"/* | grep -E '\bERROR\b' || true)"
  [[ -z "$errors" ]] || fail "the app logged errors during start: $errors"
  launch="ok: started from the disk image, logged version $built_version, quit on SIGTERM"
fi

hdiutil detach -quiet "$mountpoint" || fail "cannot unmount $mountpoint"
mountpoint=""

bytes=$(wc -c <"$dmg" | tr -d ' ')
sha256=$(shasum -a 256 "$dmg" | cut -d' ' -f1)
printf '%s  %s\n' "$sha256" "$(basename "$dmg")" >"$dmg.sha256"

printf '\nmacOS app and disk image built in %d s\n' "$seconds"
printf '  app:       %s (%s)\n' "$app" "$(du -sh "$app" | cut -f1)"
printf '  version:   %s\n' "$built_version"
printf '  signature: ad-hoc (no Developer ID, not notarised); codesign --verify ok\n'
printf '  helper:    Contents/MacOS/mail-mcp (%s, %s), ad-hoc signed, in the disk image\n' \
  "$helper_version" "$(du -h "$helper" | cut -f1)"
printf '  spctl:     %s\n' "$(printf '%s' "$gatekeeper" | tr '\n' ' ' | sed 's/ *$//')"
printf '  start:     %s\n' "$launch"
printf '  path:      %s\n' "$dmg"
printf '  size:      %s bytes (%s MB)\n' "$bytes" "$(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 1048576 }')"
printf '  sha256:    %s\n' "$sha256"
printf '  (also in %s)\n' "$dmg.sha256"
