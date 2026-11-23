#!/usr/bin/env bash
# Builds the macOS app (.app) and disk image (.dmg) for Apple silicon. The MCP helper `mail-mcp`
# goes into Contents/MacOS as a Tauri sidecar (stage 12, decision 0021). Checks the signatures and
# the disk image, starts the app from the disk image under a temporary HOME, then prints path, size
# and SHA-256 of the .dmg.
#
#   scripts/build-macos.sh              version from tauri.conf.json, else the Cargo workspace
#   scripts/build-macos.sh 0.1.0-preview.1
#                                       this version for the app and the disk image (semver)
#
# Signing (docs/RELEASING.md, "macOS signing and notarisation"): if the keychain holds a valid
# "Developer ID Application" identity (or VICEMAIL_SIGNING_IDENTITY names one, by name or SHA-1),
# the helper, the app and the disk image are signed with it (hardened runtime, secure timestamp,
# app/src-tauri/Entitlements.plist), notarised with the notarytool keychain profile
# `vicemail-notary` (VICEMAIL_NOTARY_PROFILE) and stapled. If that fails, the build fails; it never
# falls back to ad hoc. Without such an identity, or with VICEMAIL_SIGNING_IDENTITY=-, the build is
# ad-hoc signed and not notarised (docs/INSTALL.md). VICEMAIL_SIGNING_KEYCHAIN searches only that
# keychain file. An identity that is not an Apple Developer ID (a self-signed test certificate)
# needs VICEMAIL_SIGNING_TEST=1 and is never sent to Apple: local tests only, never publish.
# VICEMAIL_NOTARY_TIMEOUT limits each notarisation wait (default 2h).
#
# Environment: CARGO_TARGET_DIR is honoured. SOURCE_DATE_EPOCH, if set, is the build date shown in
# About; otherwise the commit time of a clean tree or the current time of a dirty one.
# SKIP_LAUNCH_CHECK=1 skips the start test (it needs a logged-in GUI session).
# VICEMAIL_UPDATER=1 also builds the signed update artifact (`<app>.app.tar.gz` and `.sig`,
# decision 0037) with the signing key from the login keychain (docs/RELEASING.md) and checks
# its signature against plugins.updater.pubkey; without it no key is needed. In a notarised build
# the archive holds the notarised, stapled app.
set -euo pipefail

usage() {
  sed -n '2,29p' "$0" | sed 's/^# \{0,1\}//'
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

# Tauri signs and notarises by itself when it finds these; this script does both itself, with the
# identity and the notarytool profile from the keychain, so none of them may reach the build.
unset APPLE_SIGNING_IDENTITY APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_ID APPLE_PASSWORD \
  APPLE_TEAM_ID APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_PROVIDER_SHORT_NAME \
  APPLE_DEVELOPMENT_TEAM

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
for tool in codesign hdiutil plutil shasum security ditto tar; do
  command -v "$tool" >/dev/null || fail "$tool missing (part of macOS)"
done
command -v pnpm >/dev/null || fail "pnpm missing: brew install pnpm"
[[ -x "$repo/app/node_modules/.bin/tauri" ]] ||
  fail "app dependencies missing: pnpm --dir app install"

# Decision 0037: the signed update artifact only on request; the key stays in the keychain and
# reaches only this build's environment.
updater=0
if [[ "${VICEMAIL_UPDATER:-}" == 1 ]]; then
  updater=1
  command -v node >/dev/null || fail "node missing (checks the update signature): brew install node"
  # shellcheck source=updater-signing.sh
  . "$repo/scripts/updater-signing.sh"
  updater_signing_env build-macos || fail "the update signing key is not available"
fi

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
rw_mountpoint=""
app_pid=""
cleanup() {
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -KILL "$app_pid" 2>/dev/null || true
  fi
  local mounted
  for mounted in "$mountpoint" "$rw_mountpoint"; do
    [[ -z "$mounted" ]] || hdiutil detach -quiet -force "$mounted" 2>/dev/null || true
  done
  rm -rf "$tmp"
}
trap cleanup EXIT

plist() { plutil -extract "$2" raw -o - "$1/Contents/Info.plist"; }

# --- Signing identity ------------------------------------------------------------------------
#
# Three modes: "adhoc" (no identity: today's unsigned build), "notarised" (an Apple Developer ID
# Application identity: signed, notarised, stapled) and "test" (any other identity, only with
# VICEMAIL_SIGNING_TEST=1: signed like a release, checked the same way, but never notarised).

notary_profile="${VICEMAIL_NOTARY_PROFILE:-vicemail-notary}"
notary_timeout="${VICEMAIL_NOTARY_TIMEOUT:-2h}"
entitlements="$repo/app/src-tauri/Entitlements.plist"
keychain_args=()
if [[ -n "${VICEMAIL_SIGNING_KEYCHAIN:-}" ]]; then
  [[ -f "$VICEMAIL_SIGNING_KEYCHAIN" ]] ||
    fail "VICEMAIL_SIGNING_KEYCHAIN: no keychain file at $VICEMAIL_SIGNING_KEYCHAIN"
  keychain_args=(--keychain "$VICEMAIL_SIGNING_KEYCHAIN")
fi

# Code signing identities as "<SHA-1> <name>"; with -v only those macOS trusts (valid chain, not
# expired, not revoked).
identities() { # [-v]
  { security find-identity "$@" -p codesigning ${VICEMAIL_SIGNING_KEYCHAIN:+"$VICEMAIL_SIGNING_KEYCHAIN"} 2>/dev/null || true; } |
    sed -n 's/^ *[0-9][0-9]*) \([0-9A-F]\{40\}\) "\(.*\)".*$/\1 \2/p' | sort -u
}
count_lines() { printf '%s' "$1" | grep -c . || true; }

mode=adhoc
identity=""
identity_name=""
team=""
signing_note=""
requested="${VICEMAIL_SIGNING_IDENTITY:-}"
if [[ "$requested" == - ]]; then
  signing_note="ad-hoc by request (VICEMAIL_SIGNING_IDENTITY=-)"
elif [[ -n "$requested" ]]; then
  pick() { awk -v want="$requested" 'toupper(want) == substr($0, 1, 40) || want == substr($0, 42)'; }
  matches="$(identities -v | pick)"
  [[ -n "$matches" ]] || matches="$(identities | pick)"
  case "$(count_lines "$matches")" in
    0) fail "no code signing identity '$requested' in the keychain (see: security find-identity -p codesigning)" ;;
    1) ;;
    *) fail "several identities match '$requested'; set VICEMAIL_SIGNING_IDENTITY to the SHA-1 of one:
$matches" ;;
  esac
  identity="${matches:0:40}"
  identity_name="${matches:41}"
else
  matches="$(identities -v | awk 'substr($0, 42) ~ /^Developer ID Application: /')"
  case "$(count_lines "$matches")" in
    0) signing_note="no Developer ID Application identity in the keychain" ;;
    1)
      identity="${matches:0:40}"
      identity_name="${matches:41}"
      ;;
    *) fail "several Developer ID Application identities in the keychain (an old and a renewed certificate?); set VICEMAIL_SIGNING_IDENTITY to the SHA-1 of the one to use, or delete the old one:
$matches" ;;
  esac
fi

if [[ -n "$identity" ]]; then
  # A first signature before the long build: proves that the private key is usable (over SSH the
  # login keychain may be locked) and that Apple's timestamp server answers.
  cp /usr/bin/true "$tmp/probe"
  codesign --force --sign "$identity" ${keychain_args[@]+"${keychain_args[@]}"} --options runtime \
    --timestamp "$tmp/probe" 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    fail "cannot sign with '$identity_name' (codesign's message above). If the login keychain is locked, as it often is over SSH: security unlock-keychain ~/Library/Keychains/login.keychain-db"
  }
  authorities="$(codesign -dvv "$tmp/probe" 2>&1 | sed -n 's/^Authority=//p')"
  if [[ "$authorities" == "Developer ID Application: "*$'\nDeveloper ID Certification Authority\nApple Root CA' ]]; then
    mode=notarised
    team="$(codesign -dv "$tmp/probe" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
    [[ "$team" =~ ^[A-Z0-9]{10}$ ]] || fail "the signature names no Apple team ID (got '$team')"
    for tool in notarytool stapler SetFile; do
      xcrun --find "$tool" >/dev/null 2>&1 || fail "xcrun $tool missing: install Xcode (docs/SETUP.md)"
    done
    # The notarisation credentials, checked now rather than after the build. The request lists
    # earlier submissions of the team; nothing of it is printed.
    xcrun notarytool history --keychain-profile "$notary_profile" --output-format json \
      >/dev/null 2>"$tmp/notary.log" || {
      cat "$tmp/notary.log" >&2
      fail "the notarytool profile '$notary_profile' is missing or not accepted by Apple; run scripts/apple-signing-setup.sh (docs/RELEASING.md). To build without notarisation, set VICEMAIL_SIGNING_IDENTITY=- (ad hoc)."
    }
    signing_note="$identity_name, notarised with profile $notary_profile"
  else
    [[ "${VICEMAIL_SIGNING_TEST:-}" == 1 ]] ||
      fail "'$identity_name' is not an Apple Developer ID Application identity, so the build could not be notarised. To sign with it for a local test anyway (not notarised; Gatekeeper rejects it), set VICEMAIL_SIGNING_TEST=1."
    mode=test
    xcrun --find SetFile >/dev/null 2>&1 || fail "xcrun SetFile missing: install Xcode (docs/SETUP.md)"
    signing_note="TEST signature with '$identity_name' (not an Apple Developer ID, not notarised)"
    printf 'build-macos: WARNING: %s. Gatekeeper rejects this build; do not publish it.\n' "$signing_note" >&2
  fi
fi
printf 'build-macos: signing: %s\n' "${signing_note:-$mode}"
if [[ $mode == adhoc ]]; then
  printf 'build-macos: the build is ad-hoc signed and not notarised (docs/RELEASING.md, "macOS signing and notarisation")\n'
fi

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
# A signed build starts from the same ad-hoc bundle, but Tauri lays out only the app: this script
# signs and notarises it, then makes the disk image and the update archive from the stapled app.
bundles=app,dmg
updater_config=""
if [[ $mode == adhoc ]]; then
  [[ $updater -eq 0 ]] || updater_config=",\"createUpdaterArtifacts\":true"
else
  bundles=app
fi
config="\"bundle\":{\"macOS\":{\"signingIdentity\":\"-\"},\"externalBin\":[\"$sidecar_dir/mail-mcp\"]$updater_config}"
if [[ -n "$version" ]]; then
  config="\"version\":\"$version\",$config"
fi
args=(build --bundles "$bundles" --config "{$config}")

bundle_dir="$target_dir/release/bundle"
touch "$tmp/started"
started=$(date +%s)
printf 'build-macos: SOURCE_DATE_EPOCH=%s pnpm --dir app tauri %s\n' "${SOURCE_DATE_EPOCH:-}" "${args[*]}"
pnpm --dir "$repo/app" tauri "${args[@]}" || fail "tauri build failed (exit $?)"
seconds=$(($(date +%s) - started))

newest() { # directory, name pattern, type: the one entry made by this build
  local found=()
  while IFS= read -r -d '' entry; do
    found+=("$entry")
  done < <(find "$1" -mindepth 1 -maxdepth 1 -name "$2" -type "$3" -newer "$tmp/started" -print0 2>/dev/null)
  [[ ${#found[@]} -eq 1 ]] || fail "expected one new $2 in $1, found ${#found[@]}"
  printf '%s\n' "${found[0]}"
}
app="$(newest "$bundle_dir/macos" '*.app' d)"
built_version="$(plist "$app" CFBundleShortVersionString)"
[[ -z "$version" || "$built_version" == "$version" ]] ||
  fail "the app reports version $built_version, expected $version"

# --- Signing and notarisation (signed builds only) -------------------------------------------

sign() { # path, codesign options...: replaces the ad-hoc seal with the identity's signature
  local path="$1"
  shift
  codesign --force --sign "$identity" ${keychain_args[@]+"${keychain_args[@]}"} --timestamp "$@" \
    "$path" 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    fail "codesign failed for $path"
  }
}

# The signature of a code item must be the identity's, with the hardened runtime and a secure
# timestamp (both required for notarisation).
check_signed() { # path, name for messages
  local info
  info="$(codesign -dvv "$1" 2>&1)" || fail "codesign cannot read the signature of $2"
  [[ "$info" == *$'\nAuthority='"$identity_name"$'\n'* ]] ||
    fail "$2 is not signed by '$identity_name': $(printf '%s' "$info" | grep '^Authority=' | head -1)"
  printf '%s\n' "$info" | grep -q '^CodeDirectory .*flags=0x[0-9a-f]*([^)]*runtime' ||
    fail "$2 is signed without the hardened runtime"
  printf '%s\n' "$info" | grep -q '^Timestamp=' || fail "$2 has no secure timestamp"
  if [[ $mode == notarised ]]; then
    [[ "$info" == *$'\nTeamIdentifier='"$team"$'\n'* ]] || fail "$2 does not carry team ID $team"
  fi
}

cdhash() { codesign -dvvv "$1" 2>&1 | sed -n 's/^CDHash=//p'; }

notary_logs=()
notarise() { # file to submit, slug for file names, what it is (for messages)
  local result="$tmp/notary-$2.plist" log="$bundle_dir/notarisation-$2.json" id status
  printf 'build-macos: notarising the %s (most submissions finish within 15 minutes, a first one can take longer)\n' "$3"
  xcrun notarytool submit "$1" --keychain-profile "$notary_profile" --wait --timeout "$notary_timeout" \
    --output-format plist >"$result" 2>"$tmp/notary.log" || true
  id="$(plutil -extract id raw -o - "$result" 2>/dev/null || true)"
  status="$(plutil -extract status raw -o - "$result" 2>/dev/null || true)"
  if [[ "$status" != Accepted ]]; then
    cat "$tmp/notary.log" >&2
    if [[ -n "$id" ]]; then
      if xcrun notarytool log "$id" --keychain-profile "$notary_profile" "$tmp/notary-log.json" >/dev/null 2>&1; then
        cat "$tmp/notary-log.json" >&2
      fi
      fail "notarisation of the $3 ended with status '${status:-unknown}' (submission $id). Later status: xcrun notarytool info $id --keychain-profile $notary_profile"
    fi
    fail "the $3 did not reach Apple's notary service (see the messages above)"
  fi
  # Apple's advice: read the log of accepted submissions too, it may hold warnings.
  if xcrun notarytool log "$id" --keychain-profile "$notary_profile" "$log" >/dev/null 2>&1; then
    notary_logs+=("$log")
  else
    printf 'build-macos: note: the notarisation log of submission %s could not be fetched yet\n' "$id" >&2
  fi
  printf 'build-macos: the %s is notarised (submission %s)\n' "$3" "$id"
}

staple() { # path: attaches the ticket (Apple's CDN may need a moment after acceptance)
  local i
  for i in 1 2 3 4 5 6; do
    if xcrun stapler staple -q "$1" >"$tmp/stapler.log" 2>&1; then
      break
    fi
    [[ $i -lt 6 ]] || {
      cat "$tmp/stapler.log" >&2
      fail "stapler staple failed for $1"
    }
    sleep 10
  done
  xcrun stapler validate -q "$1" >"$tmp/stapler.log" 2>&1 || {
    cat "$tmp/stapler.log" >&2
    fail "stapler validate failed for $1"
  }
}

# The disk image as Tauri makes it (scripts in target/release/bundle/dmg): HFS+, compressed
# (UDZO), volume named like the app, the app, a link to /Applications and the app icon as volume
# icon; no Finder layout (CI=true, decision 0004).
make_dmg() { # app, disk image to write
  local app="$1" out="$2" stage="$tmp/dmg-stage" rw="$tmp/dmg-rw.dmg" size volume icon
  volume="$(plist "$app" CFBundleName)"
  icon="$app/Contents/Resources/$(plist "$app" CFBundleIconFile)"
  [[ -f "$icon" ]] || fail "the app icon $icon is missing"
  rm -rf "$stage" "$rw"
  mkdir "$stage"
  ditto "$app" "$stage/$(basename "$app")"
  size=$(($(du -sm "$stage" | cut -f1) + 20))
  hdiutil create -quiet -srcfolder "$stage" -volname "$volume" -fs HFS+ -fsargs "-c c=64,a=16,e=16" \
    -format UDRW -size "${size}m" "$rw" || fail "hdiutil create failed"
  rm -rf "$stage"
  rw_mountpoint="$tmp/dmg-mount"
  mkdir "$rw_mountpoint"
  hdiutil attach -quiet -readwrite -noverify -noautoopen -nobrowse -mountpoint "$rw_mountpoint" "$rw" ||
    fail "cannot mount the new disk image"
  ln -s /Applications "$rw_mountpoint/Applications"
  cp "$icon" "$rw_mountpoint/.VolumeIcon.icns"
  xcrun SetFile -c icnC "$rw_mountpoint/.VolumeIcon.icns" || fail "SetFile failed"
  xcrun SetFile -a C "$rw_mountpoint" || fail "SetFile failed"
  chmod -Rf go-w "$rw_mountpoint" 2>/dev/null || true
  rm -rf "$rw_mountpoint/.fseventsd"
  hdiutil detach -quiet "$rw_mountpoint" || fail "cannot unmount the new disk image"
  rw_mountpoint=""
  mkdir -p "$(dirname "$out")"
  rm -f "$out" "$out.sha256"
  hdiutil convert -quiet "$rw" -format UDZO -o "$out" || fail "hdiutil convert failed"
  rm -f "$rw"
}

# The update archive as Tauri makes it: a gzip tar with the app bundle as its one top-level entry
# (the updater drops that first path component), here without macOS metadata or AppleDouble files;
# then signed for this version with the update key (decision 0037).
make_update_archive() { # app
  local archive="$bundle_dir/macos/$(basename "$1").tar.gz"
  rm -f "$archive" "$archive.sig"
  COPYFILE_DISABLE=1 tar -czf "$archive" --no-mac-metadata --no-xattrs --no-acls --no-fflags \
    -C "$(dirname "$1")" "$(basename "$1")" || fail "packing the update archive failed"
  pnpm --dir "$repo/app" tauri signer sign --app-version "$built_version" "$archive" \
    >"$tmp/signer.log" 2>&1 || {
    cat "$tmp/signer.log" >&2
    fail "signing the update archive failed"
  }
  [[ -f "$archive.sig" ]] || fail "tauri signer wrote no $archive.sig"
}

sign_seconds=0
helper="$app/Contents/MacOS/mail-mcp"
if [[ $mode != adhoc ]]; then
  sign_started=$(date +%s)
  identifier="$(plist "$app" CFBundleIdentifier)"
  main="$app/Contents/MacOS/$(plist "$app" CFBundleExecutable)"
  [[ ! -e "$app/Contents/Frameworks" && ! -e "$app/Contents/PlugIns" ]] ||
    fail "the app contains frameworks or plug-ins; sign them inside out here before the app"
  # Inside out: every other Mach-O file of the bundle first (today only the MCP helper), with
  # its own identifier and no entitlements, then the bundle, whose seal records their signatures.
  nested=()
  while IFS= read -r -d '' file; do
    [[ "$file" != "$main" ]] || continue
    if file -b "$file" | grep -q '^Mach-O'; then
      nested+=("$file")
    fi
  done < <(find "$app/Contents" -type f -print0)
  [[ ${#nested[@]} -ge 1 ]] || fail "no MCP helper found to sign in $app"
  for file in "${nested[@]}"; do
    sign "$file" --options runtime --identifier "$identifier.$(basename "$file")"
  done
  sign "$app" --options runtime --entitlements "$entitlements"

  codesign --verify --deep --strict "$app" 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    fail "codesign --verify failed for the signed app"
  }
  check_signed "$app" "the app"
  for file in "${nested[@]}"; do
    check_signed "$file" "Contents/MacOS/$(basename "$file")"
  done
  codesign -d --entitlements - --xml "$app" 2>/dev/null >"$tmp/entitlements.xml" || true
  ! grep -q 'get-task-allow' "$tmp/entitlements.xml" ||
    fail "the app carries com.apple.security.get-task-allow, which notarisation refuses"

  dmg_path="$bundle_dir/dmg/$(basename "$app" .app)_${built_version}_aarch64.dmg"
  if [[ $mode == notarised ]]; then
    # Round 1: the app, so that the copy in the disk image and the update archive carry a ticket.
    ditto -c -k --keepParent "$app" "$tmp/app.zip"
    notarise "$tmp/app.zip" app app
    rm -f "$tmp/app.zip"
    staple "$app"
  fi
  make_dmg "$app" "$dmg_path"
  sign "$dmg_path"
  codesign --verify --strict "$dmg_path" 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    fail "codesign --verify failed for the disk image"
  }
  if [[ $mode == notarised ]]; then
    # Round 2: the disk image with the stapled app, so a download is checked offline as well.
    notarise "$dmg_path" dmg "disk image"
    staple "$dmg_path"
  fi
  if [[ $updater -eq 1 ]]; then
    make_update_archive "$app"
  fi
  sign_seconds=$(($(date +%s) - sign_started))
fi

# --- Result ----------------------------------------------------------------------------------

dmg="$(newest "$bundle_dir/dmg" '*.dmg' f)"

# Decision 0037: the update archive of this app, signed for this version with the key that
# tauri.conf.json names.
update_line="not built (VICEMAIL_UPDATER=1 builds it)"
if [[ $updater -eq 1 ]]; then
  update_archive="$(newest "$bundle_dir/macos" '*.app.tar.gz' f)"
  [[ -f "$update_archive.sig" ]] || fail "the update archive has no signature: $update_archive.sig"
  node "$repo/scripts/updater-manifest.mjs" verify "$update_archive" "$built_version" >/dev/null ||
    fail "the update signature of $update_archive does not verify"
  update_line="$update_archive (+ .sig), signature verified for $built_version"
  if [[ $mode != adhoc ]]; then
    # What the updater will install: the one top-level entry, and the very app checked below.
    [[ "$(tar -tzf "$update_archive" | cut -d/ -f1 | sort -u)" == "$(basename "$app")" ]] ||
      fail "the update archive must hold exactly $(basename "$app") at its top level"
    mkdir "$tmp/update"
    tar -xzf "$update_archive" -C "$tmp/update"
    update_app="$tmp/update/$(basename "$app")"
    codesign --verify --deep --strict "$update_app" 2>"$tmp/codesign.log" || {
      cat "$tmp/codesign.log" >&2
      fail "codesign --verify failed for the app in the update archive"
    }
    [[ "$(cdhash "$update_app")" == "$(cdhash "$app")" ]] ||
      fail "the app in the update archive is not the app that was signed"
    if [[ $mode == notarised ]]; then
      xcrun stapler validate -q "$update_app" >/dev/null 2>&1 ||
        fail "the app in the update archive carries no notarisation ticket"
      update_line="$update_line; holds the notarised, stapled app"
    else
      update_line="$update_line; holds the test-signed app"
    fi
    rm -rf "$tmp/update"
  fi
fi

# --- Checks ----------------------------------------------------------------------------------

codesign --verify --deep --strict "$app" 2>"$tmp/codesign.log" || {
  cat "$tmp/codesign.log" >&2
  fail "codesign --verify failed for $app"
}
signature="$(codesign -dv "$app" 2>&1 | sed -n 's/^Signature=//p')"
if [[ $mode == adhoc ]]; then
  [[ "$signature" == adhoc ]] || fail "expected an ad-hoc signature, codesign reports '${signature:-none}'"
else
  check_signed "$app" "the app"
fi

# The MCP helper: in Contents/MacOS, signed like the app, and it runs.
[[ -x "$helper" ]] || fail "the MCP helper is missing in $app/Contents/MacOS"
codesign --verify --strict "$helper" 2>"$tmp/codesign.log" || {
  cat "$tmp/codesign.log" >&2
  fail "codesign --verify failed for the MCP helper"
}
if [[ $mode == adhoc ]]; then
  helper_signature="$(codesign -dv "$helper" 2>&1 | sed -n 's/^Signature=//p')"
  [[ "$helper_signature" == adhoc ]] ||
    fail "expected an ad-hoc signature on the MCP helper, codesign reports '${helper_signature:-none}'"
else
  check_signed "$helper" "the MCP helper"
fi
helper_version="$("$helper" --version)" || fail "the MCP helper does not run"
[[ "$helper_version" == "mail-mcp "* ]] || fail "unexpected answer of the MCP helper: $helper_version"

# Gatekeeper: a notarised build must be accepted as such; every other build is rejected, which is
# expected (docs/INSTALL.md).
gatekeeper="$(spctl --assess -vv --type execute "$app" 2>&1 || true)"
dmg_gatekeeper=""
if [[ $mode == notarised ]]; then
  [[ "$gatekeeper" == *": accepted"* && "$gatekeeper" == *"source=Notarized Developer ID"* ]] ||
    fail "Gatekeeper does not accept the app as notarised: $gatekeeper"
  xcrun stapler validate -q "$app" >/dev/null 2>&1 || fail "the app carries no notarisation ticket"
  dmg_gatekeeper="$(spctl --assess -vv --type open --context context:primary-signature "$dmg" 2>&1 || true)"
  [[ "$dmg_gatekeeper" == *": accepted"* && "$dmg_gatekeeper" == *"source=Notarized Developer ID"* ]] ||
    fail "Gatekeeper does not accept the disk image as notarised: $dmg_gatekeeper"
  xcrun stapler validate -q "$dmg" >/dev/null 2>&1 || fail "the disk image carries no notarisation ticket"
fi

hdiutil verify -quiet "$dmg" || fail "hdiutil verify failed for $dmg"
if [[ $mode != adhoc ]]; then
  codesign --verify --strict "$dmg" 2>"$tmp/codesign.log" || {
    cat "$tmp/codesign.log" >&2
    fail "codesign --verify failed for $dmg"
  }
  dmg_authority="$(codesign -dvv "$dmg" 2>&1 | sed -n 's/^Authority=//p' | head -1)"
  [[ "$dmg_authority" == "$identity_name" ]] ||
    fail "the disk image is not signed by '$identity_name' (got '${dmg_authority:-none}')"
fi
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
if [[ $mode != adhoc ]]; then
  [[ "$(cdhash "$image_app")" == "$(cdhash "$app")" ]] ||
    fail "the app in the disk image is not the app that was signed"
fi
if [[ $mode == notarised ]]; then
  xcrun stapler validate -q "$image_app" >/dev/null 2>&1 ||
    fail "the app in the disk image carries no notarisation ticket"
fi

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
  [[ $mode == adhoc ]] || launch="$launch (hardened runtime, $mode signature)"
fi

hdiutil detach -quiet "$mountpoint" || fail "cannot unmount $mountpoint"
mountpoint=""

bytes=$(wc -c <"$dmg" | tr -d ' ')
sha256=$(shasum -a 256 "$dmg" | cut -d' ' -f1)
printf '%s  %s\n' "$sha256" "$(basename "$dmg")" >"$dmg.sha256"

oneline() { printf '%s' "$1" | tr '\n' ' ' | sed 's/ *$//'; }

done_line="macOS app and disk image built in $seconds s"
case $mode in
  notarised) done_line="$done_line, signed and notarised in $sign_seconds s" ;;
  test) done_line="$done_line, test-signed in $sign_seconds s" ;;
esac
printf '\n%s\n' "$done_line"
printf '  app:       %s (%s)\n' "$app" "$(du -sh "$app" | cut -f1)"
printf '  version:   %s\n' "$built_version"
case $mode in
  adhoc)
    printf '  signature: ad-hoc (%s; not notarised); codesign --verify ok\n' "$signing_note"
    printf '  helper:    Contents/MacOS/mail-mcp (%s, %s), ad-hoc signed, in the disk image\n' \
      "$helper_version" "$(du -h "$helper" | cut -f1)"
    ;;
  notarised)
    printf '  signature: %s (team %s), hardened runtime, secure timestamp; notarised and stapled (app and disk image); codesign --verify ok\n' \
      "$identity_name" "$team"
    printf '  helper:    Contents/MacOS/mail-mcp (%s, %s), signed like the app, in the disk image\n' \
      "$helper_version" "$(du -h "$helper" | cut -f1)"
    ;;
  test)
    printf '  signature: TEST %s, hardened runtime, secure timestamp; NOT notarised, do not publish; codesign --verify ok\n' \
      "$identity_name"
    printf '  helper:    Contents/MacOS/mail-mcp (%s, %s), signed like the app, in the disk image\n' \
      "$helper_version" "$(du -h "$helper" | cut -f1)"
    ;;
esac
printf '  spctl:     %s\n' "$(oneline "$gatekeeper")"
[[ -z "$dmg_gatekeeper" ]] || printf '  spctl dmg: %s\n' "$(oneline "$dmg_gatekeeper")"
[[ ${#notary_logs[@]} -eq 0 ]] || printf '  notary:    logs in %s\n' "${notary_logs[*]}"
printf '  start:     %s\n' "$launch"
printf '  update:    %s\n' "$update_line"
printf '  path:      %s\n' "$dmg"
printf '  size:      %s bytes (%s MB)\n' "$bytes" "$(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 1048576 }')"
printf '  sha256:    %s\n' "$sha256"
printf '  (also in %s)\n' "$dmg.sha256"
