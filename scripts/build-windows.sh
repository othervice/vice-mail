#!/usr/bin/env bash
# Builds the Windows installer (NSIS, x86_64) on the Mac by cross-compiling with cargo-xwin
# (docs/SETUP.md, decision 0004). The MCP helper `mail-mcp.exe` goes next to the app as a Tauri
# sidecar (stage 12, decision 0021). Prints path, size and SHA-256 of the installer at the end.
#
#   scripts/build-windows.sh            version from tauri.conf.json, else the Cargo workspace
#   scripts/build-windows.sh 0.1.0-preview.1
#                                       this version for the app and the installer (semver)
#
# Environment: LLVM_BIN (default /opt/homebrew/opt/llvm/bin) is the LLVM that provides clang-cl,
# llvm-lib and llvm-rc; it goes onto PATH for this build only. CARGO_TARGET_DIR is honoured.
# SOURCE_DATE_EPOCH, if set, is the build date shown in About; otherwise the commit time of a
# clean tree or the current time of a dirty one.
# VICEMAIL_UPDATER=1 also signs the installer as the update artifact (`<installer>.sig`, decision
# 0037) with the signing key from the login keychain (docs/RELEASING.md) and checks the signature
# against plugins.updater.pubkey; without it no key is needed.
set -euo pipefail

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
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
# Windows SDK and CRT for cargo-xwin: in the repository (ignored by git), x86_64 only. CI keeps
# them outside its fresh checkouts and sets XWIN_CACHE_DIR.
export XWIN_CACHE_DIR="${XWIN_CACHE_DIR:-$repo/.xwin-cache}"
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

# Decision 0037: the update signature only on request; the key stays in the keychain and reaches
# only this build's environment.
updater=0
if [[ "${VICEMAIL_UPDATER:-}" == 1 ]]; then
  updater=1
  command -v node >/dev/null || fail "node missing (checks the update signature): brew install node"
  # shellcheck source=updater-signing.sh
  . "$repo/scripts/updater-signing.sh"
  updater_signing_env build-windows || fail "the update signing key is not available"
fi

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

target_dir="${CARGO_TARGET_DIR:-$repo/target}"

# The MCP helper (decision 0021): built first, then handed to Tauri as `bundle.externalBin`
# (`<path>-<target triple>.exe`); the installer puts it next to the app as `mail-mcp.exe`. Only
# for this build (through --config), so plain cargo builds need no helper file.
printf 'build-windows: cargo xwin build --release --locked -p mail-mcp --bin mail-mcp --target %s\n' "$target"
cargo xwin build --release --locked -p mail-mcp --bin mail-mcp --target "$target" \
  --manifest-path "$repo/Cargo.toml" || fail "building the MCP helper failed"
sidecar_dir="$target_dir/sidecar"
mkdir -p "$sidecar_dir"
cp -f "$target_dir/$target/release/mail-mcp.exe" "$sidecar_dir/mail-mcp-$target.exe"

# The version and the sidecar go in through --config (as in build-macos.sh): no project file is
# changed.
updater_config=""
[[ $updater -eq 0 ]] || updater_config=",\"createUpdaterArtifacts\":true"
config="\"bundle\":{\"externalBin\":[\"$sidecar_dir/mail-mcp\"]$updater_config}"
if [[ -n "$version" ]]; then
  config="\"version\":\"$version\",$config"
fi
args=(build --runner cargo-xwin --target "$target" --bundles nsis --config "{$config}")

nsis_dir="$target_dir/$target/release/bundle/nsis"
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

# The MCP helper: next to the app in the build, and in the installer's file list.
[[ -f "$target_dir/$target/release/mail-mcp.exe" ]] ||
  fail "the MCP helper is missing next to the app in $target_dir/$target/release"
nsi="$target_dir/$target/release/nsis/x64/installer.nsi"
[[ -f "$nsi" ]] || fail "the NSIS script $nsi is missing"
grep -q 'mail-mcp\.exe' "$nsi" || fail "the installer does not install mail-mcp.exe ($nsi)"
helper_size="$(du -h "$target_dir/$target/release/mail-mcp.exe" | cut -f1)"

# Decision 0037: the installer is also the update artifact (the updater runs it with /P /UPDATE).
update_line="not signed (VICEMAIL_UPDATER=1 signs it)"
if [[ $updater -eq 1 ]]; then
  [[ -f "$installer.sig" && "$installer.sig" -nt "$tmp/started" ]] ||
    fail "the installer has no new update signature: $installer.sig"
  built_version="$(basename "$installer" | sed -E 's/^.*_([^_]+)_x64-setup\.exe$/\1/')"
  node "$repo/scripts/updater-manifest.mjs" verify "$installer" "$built_version" >/dev/null ||
    fail "the update signature of $installer does not verify"
  update_line="$installer.sig, signature verified for $built_version"
fi

bytes=$(wc -c <"$installer" | tr -d ' ')
sha256=$(shasum -a 256 "$installer" | cut -d' ' -f1)
printf '%s  %s\n' "$sha256" "$(basename "$installer")" >"$installer.sha256"

printf '\nWindows installer built in %d s\n' "$seconds"
printf '  helper:  mail-mcp.exe (%s) next to the app, listed in the installer script\n' "$helper_size"
printf '  path:    %s\n' "$installer"
printf '  size:    %s bytes (%s MB)\n' "$bytes" "$(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 1048576 }')"
printf '  sha256:  %s\n' "$sha256"
printf '  (also in %s)\n' "$installer.sha256"
printf '  update:  %s\n' "$update_line"
