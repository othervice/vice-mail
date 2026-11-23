#!/usr/bin/env bash
# Builds the Windows installer (NSIS, x86_64) on the Mac by cross-compiling with cargo-xwin
# (docs/SETUP.md, decision 0004). Prints path, size and SHA-256 of the installer at the end.
#
#   scripts/build-windows.sh            version from tauri.conf.json, else the Cargo workspace
#   scripts/build-windows.sh 0.1.0-preview.1
#                                       this version for the app and the installer (semver)
#
# Environment: LLVM_BIN (default /opt/homebrew/opt/llvm/bin) is the LLVM that provides clang-cl,
# llvm-lib and llvm-rc; it goes onto PATH for this build only. CARGO_TARGET_DIR is honoured.
# SOURCE_DATE_EPOCH, if set, is the build date shown in About; otherwise the commit time of a
# clean tree or the current time of a dirty one.
set -euo pipefail

usage() {
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
}

fail() {
  printf 'build-windows: %s\n' "$*" >&2
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
target="x86_64-pc-windows-msvc"

# makensis 3.13 aborts with std::bad_alloc under the C locale (docs/SETUP.md).
export LANG=en_US.UTF-8
export LC_ALL=en_US.UTF-8
# Windows SDK and CRT for cargo-xwin: in the repository (ignored by git), x86_64 only.
export XWIN_CACHE_DIR="$repo/.xwin-cache"
export XWIN_ARCH=x86_64

# --- Preconditions ---------------------------------------------------------------------------

[[ -f "$HOME/.cargo/env" ]] || fail "Rust is not installed (~/.cargo/env missing); see docs/SETUP.md"
# shellcheck source=/dev/null
. "$HOME/.cargo/env"
command -v cargo >/dev/null || fail "cargo not found after loading ~/.cargo/env"
rustup target list --installed 2>/dev/null | grep -qx "$target" ||
  fail "Rust target $target missing: rustup target add $target"
cargo xwin --version >/dev/null 2>&1 ||
  fail "cargo-xwin missing: cargo install --locked cargo-xwin (docs/SETUP.md)"

llvm_bin="${LLVM_BIN:-/opt/homebrew/opt/llvm/bin}"
for tool in clang-cl llvm-lib llvm-rc; do
  [[ -x "$llvm_bin/$tool" ]] ||
    fail "LLVM tool $tool not found in $llvm_bin: brew install llvm, or set LLVM_BIN"
done
export PATH="$llvm_bin:$PATH"

command -v makensis >/dev/null || fail "makensis missing: brew install nsis"
command -v pnpm >/dev/null || fail "pnpm missing: brew install pnpm"
[[ -x "$repo/app/node_modules/.bin/tauri" ]] ||
  fail "app dependencies missing: pnpm --dir app install"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# `makensis -VERSION` works even where compiling fails, so compile a minimal script.
printf 'Unicode true\nOutFile "%s"\nSection\nSectionEnd\n' "$tmp/probe.exe" >"$tmp/probe.nsi"
if ! makensis -V1 "$tmp/probe.nsi" >"$tmp/probe.log" 2>&1; then
  cat "$tmp/probe.log" >&2
  fail "makensis cannot compile a minimal script (UTF-8 locale available? locale -a | grep en_US.UTF-8)"
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

# --- Build -----------------------------------------------------------------------------------

# The version goes in through --config (as in build-macos.sh): no project file is changed.
args=(build --runner cargo-xwin --target "$target" --bundles nsis)
if [[ -n "$version" ]]; then
  args+=(--config "{\"version\":\"$version\"}")
fi

nsis_dir="${CARGO_TARGET_DIR:-$repo/target}/$target/release/bundle/nsis"
touch "$tmp/started"
started=$(date +%s)
printf 'build-windows: SOURCE_DATE_EPOCH=%s pnpm --dir app tauri %s\n' "${SOURCE_DATE_EPOCH:-}" "${args[*]}"
pnpm --dir "$repo/app" tauri "${args[@]}" || fail "tauri build failed (exit $?)"
seconds=$(($(date +%s) - started))

# --- Result ----------------------------------------------------------------------------------

installers=()
while IFS= read -r -d '' file; do
  installers+=("$file")
done < <(find "$nsis_dir" -maxdepth 1 -name '*-setup.exe' -newer "$tmp/started" -print0 2>/dev/null)
[[ ${#installers[@]} -eq 1 ]] ||
  fail "expected one new installer in $nsis_dir, found ${#installers[@]}"
installer="${installers[0]}"
[[ -z "$version" || "$(basename "$installer")" == *"_${version}_"* ]] ||
  fail "$(basename "$installer") does not carry version $version"

bytes=$(wc -c <"$installer" | tr -d ' ')
sha256=$(shasum -a 256 "$installer" | cut -d' ' -f1)
printf '%s  %s\n' "$sha256" "$(basename "$installer")" >"$installer.sha256"

printf '\nWindows installer built in %d s\n' "$seconds"
printf '  path:    %s\n' "$installer"
printf '  size:    %s bytes (%s MB)\n' "$bytes" "$(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 1048576 }')"
printf '  sha256:  %s\n' "$sha256"
printf '  (also in %s)\n' "$installer.sha256"
