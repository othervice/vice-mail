#!/usr/bin/env bash
# Builds the macOS disk image (scripts/build-macos.sh) and the Windows installer
# (scripts/build-windows.sh) from this checkout and puts both, named by version, into
# dist/<version>/ with a SHA256SUMS file. Unsigned local builds; see docs/INSTALL.md.
#
#   scripts/release-local.sh                    version from tauri.conf.json, else the Cargo workspace
#   scripts/release-local.sh 0.1.0-preview.1    this version (semver) for both builds
#   scripts/release-local.sh --force [version]  replace an existing dist/<version>/
#
# Result, with the product name from app/src-tauri/tauri.conf.json (spaces become hyphens):
#   dist/<version>/<Product-Name>-<version>-macos-arm64.dmg
#   dist/<version>/<Product-Name>-<version>-windows-x64-setup.exe
#   dist/<version>/SHA256SUMS                   check with: shasum -a 256 -c SHA256SUMS
# Both builds get the same SOURCE_DATE_EPOCH, so About shows the same build date on both systems.
#
# VICEMAIL_UPDATER=1 (decision 0037, docs/RELEASING.md) signs the update artifacts with the key
# from the login keychain and adds them and the update manifest:
#   dist/<version>/<Product-Name>-<version>-macos-arm64.app.tar.gz (+ .sig)
#   dist/<version>/<Product-Name>-<version>-windows-x64-setup.exe.sig
#   dist/<version>/latest.json                  upload all of them to the release v<version>
# VICEMAIL_RELEASE_NOTES=<file> holds the notes (plain text); VICEMAIL_UPDATE_BASE_URL overrides
# where the manifest says the files are (default: the release v<version> next to the endpoint in
# tauri.conf.json).
set -euo pipefail

usage() {
  sed -n '2,23p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'release-local: %s\n' "$*" >&2
  exit 1
}

semver='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'
force=0
version=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --force) force=1 ;;
    -*) fail "unknown option $1 (see --help)" ;;
    *)
      [[ -z "$version" ]] || fail "expected at most one version, got '$version' and '$1'"
      version="$1"
      [[ "$version" =~ $semver ]] || fail "version '$version' is not semver (e.g. 0.1.0 or 0.1.0-preview.1)"
      ;;
  esac
  shift
done

repo="$(cd "$(dirname "$0")/.." && pwd -P)"
conf="$repo/app/src-tauri/tauri.conf.json"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"

# --- Preconditions ---------------------------------------------------------------------------

[[ -f "$HOME/.cargo/env" ]] || fail "Rust is not installed (~/.cargo/env missing); see docs/SETUP.md"
# shellcheck source=/dev/null
. "$HOME/.cargo/env"
command -v node >/dev/null || fail "node missing: brew install node"
command -v git >/dev/null || fail "git missing"
[[ -f "$conf" ]] || fail "$conf not found"

# Prints the string at a dotted key of a JSON file, nothing if the key is missing.
json_string() {
  node -e '
    const [file, key] = process.argv.slice(1);
    const data = JSON.parse(require("fs").readFileSync(file, "utf8"));
    const value = key.split(".").reduce((o, k) => (o == null ? undefined : o[k]), data);
    if (value !== undefined && typeof value !== "string") {
      console.error(`${key} in ${file} is not a string`);
      process.exit(1);
    }
    process.stdout.write(value ?? "");
  ' "$1" "$2"
}

product="$(json_string "$conf" productName)" || fail "cannot read productName from $conf"
[[ -n "$product" ]] || fail "productName missing in $conf"
file_product="${product// /-}"
[[ "$file_product" =~ ^[A-Za-z0-9._-]+$ ]] ||
  fail "productName '$product' does not make a safe file name"

# Same order as Tauri: tauri.conf.json "version" (a version or the path of a package.json),
# else the version of the app's Cargo package (the workspace version).
if [[ -z "$version" ]]; then
  version="$(json_string "$conf" version)" || fail "cannot read version from $conf"
  if [[ "$version" == *.json ]]; then
    version="$(json_string "$(dirname "$conf")/$version" version)" ||
      fail "cannot read the version from $version"
  fi
  if [[ -z "$version" ]]; then
    manifest="$repo/app/src-tauri/Cargo.toml"
    version="$(cargo metadata --no-deps --format-version 1 --manifest-path "$manifest" | node -e '
      const manifest = process.argv[1];
      let text = "";
      process.stdin.on("data", (d) => (text += d)).on("end", () => {
        const pkg = JSON.parse(text).packages.find((p) => p.manifest_path === manifest);
        if (!pkg) {
          console.error(`no Cargo package for ${manifest}`);
          process.exit(1);
        }
        process.stdout.write(pkg.version);
      });
    ' "$manifest")" || fail "cannot read the version from $manifest"
  fi
  [[ "$version" =~ $semver ]] || fail "version '$version' from the project files is not semver"
fi

# A release build must not quietly change Cargo.lock.
cargo metadata --locked --format-version 1 --manifest-path "$repo/Cargo.toml" >/dev/null ||
  fail "Cargo.lock is not up to date with the manifests; update and commit it first"

out="$repo/dist/$version"
if [[ -e "$out" ]]; then
  [[ $force -eq 1 ]] || fail "$out exists already; remove it or pass --force"
  rm -rf "$out"
fi

# Build state for About, shared by both builds (see app/src-tauri/build.rs).
git -C "$repo" rev-parse --verify -q HEAD >/dev/null || fail "not a git checkout with a commit"
changes="$(git -C "$repo" --no-optional-locks status --porcelain --untracked-files=no)" ||
  fail "git status failed"
head="$(git -C "$repo" rev-parse HEAD)"
commit="$(git -C "$repo" rev-parse --short=10 "$head")"
if [[ -n "${SOURCE_DATE_EPOCH:-}" ]]; then
  [[ "$SOURCE_DATE_EPOCH" =~ ^[0-9]+$ ]] ||
    fail "SOURCE_DATE_EPOCH '$SOURCE_DATE_EPOCH' is not a Unix time in seconds"
elif [[ -z "$changes" ]]; then
  SOURCE_DATE_EPOCH="$(git -C "$repo" log -1 --format=%ct "$head")"
else
  SOURCE_DATE_EPOCH="$(date +%s)"
fi
export SOURCE_DATE_EPOCH
if [[ -n "$changes" ]]; then
  commit="$commit-dirty"
  printf 'release-local: the working tree has uncommitted changes; About will show %s\n' "$commit" >&2
fi

# --- Build -----------------------------------------------------------------------------------

marker="$(mktemp)"
trap 'rm -f "$marker"' EXIT
started=$(date +%s)

"$repo/scripts/build-macos.sh" "$version" || fail "macOS build failed"
"$repo/scripts/build-windows.sh" "$version" || fail "Windows build failed"

# Each build reads the commit itself; a commit in between would give the two apps different ones.
now="$(git -C "$repo" rev-parse HEAD)"
[[ "$now" == "$head" ]] ||
  fail "HEAD moved from ${head:0:10} to ${now:0:10} during the builds, so About differs between them; build again"

# The one file of each kind made by this run, with the version in Tauri's file name.
built() { # directory, name pattern
  local found=()
  while IFS= read -r -d '' file; do
    found+=("$file")
  done < <(find "$1" -maxdepth 1 -type f -name "$2" -newer "$marker" -print0 2>/dev/null)
  [[ ${#found[@]} -eq 1 ]] || fail "expected one new $2 in $1, found ${#found[@]}"
  [[ "$(basename "${found[0]}")" == *"_${version}_"* ]] ||
    fail "$(basename "${found[0]}") does not carry version $version"
  printf '%s\n' "${found[0]}"
}
dmg="$(built "$target_dir/release/bundle/dmg" '*.dmg')"
exe="$(built "$target_dir/x86_64-pc-windows-msvc/release/bundle/nsis" '*-setup.exe')"

# Decision 0037: the signed update artifacts (the build scripts verified their signatures).
updater="${VICEMAIL_UPDATER:-0}"
if [[ "$updater" == 1 ]]; then
  archives=()
  while IFS= read -r -d '' file; do
    archives+=("$file")
  done < <(find "$target_dir/release/bundle/macos" -maxdepth 1 -type f -name '*.app.tar.gz' -newer "$marker" -print0 2>/dev/null)
  [[ ${#archives[@]} -eq 1 ]] || fail "expected one new .app.tar.gz, found ${#archives[@]}"
  update_archive="${archives[0]}"
  [[ -f "$update_archive.sig" && -f "$exe.sig" ]] || fail "update signatures missing next to the builds"
  endpoint="$(node -e '
    const conf = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
    process.stdout.write(conf.plugins?.updater?.endpoints?.[0] ?? "");
  ' "$conf")" || fail "cannot read plugins.updater.endpoints from $conf"
  base_url="${VICEMAIL_UPDATE_BASE_URL:-}"
  if [[ -z "$base_url" ]]; then
    [[ "$endpoint" == */releases/latest/download/latest.json ]] ||
      fail "the endpoint '$endpoint' is not a GitHub 'latest' download; set VICEMAIL_UPDATE_BASE_URL"
    base_url="${endpoint%/releases/latest/download/latest.json}/releases/download/v$version"
  fi
fi

# --- Result ----------------------------------------------------------------------------------

mkdir -p "$out"
mac_name="$file_product-$version-macos-arm64.dmg"
win_name="$file_product-$version-windows-x64-setup.exe"
cp "$dmg" "$out/$mac_name"
cp "$exe" "$out/$win_name"
sums=("$mac_name" "$win_name")
if [[ "$updater" == 1 ]]; then
  update_name="$file_product-$version-macos-arm64.app.tar.gz"
  cp "$update_archive" "$out/$update_name"
  cp "$update_archive.sig" "$out/$update_name.sig"
  cp "$exe.sig" "$out/$win_name.sig"
  notes_args=()
  [[ -z "${VICEMAIL_RELEASE_NOTES:-}" ]] || notes_args=(--notes-file "$VICEMAIL_RELEASE_NOTES")
  node "$repo/scripts/updater-manifest.mjs" manifest --version "$version" --base-url "$base_url" \
    --out "$out/latest.json" --darwin-aarch64 "$out/$update_name" --windows-x86_64 "$out/$win_name" \
    ${notes_args[@]+"${notes_args[@]}"} || fail "writing $out/latest.json failed"
  sums+=("$update_name" "$update_name.sig" "$win_name.sig" latest.json)
fi
(cd "$out" && shasum -a 256 "${sums[@]}" >SHA256SUMS && shasum -a 256 -c --quiet SHA256SUMS) ||
  fail "writing or checking $out/SHA256SUMS failed"

printf '\nRelease %s built in %d s\n' "$version" "$(($(date +%s) - started))"
printf '  commit:     %s\n' "$commit"
printf '  build date: %s (SOURCE_DATE_EPOCH=%s)\n' \
  "$(date -u -r "$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ)" "$SOURCE_DATE_EPOCH"
printf '  directory:  %s\n' "$out"
while read -r sum name; do
  printf '  %s  %s bytes  %s\n' "$sum" "$(wc -c <"$out/$name" | tr -d ' ')" "$name"
done <"$out/SHA256SUMS"
