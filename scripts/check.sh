#!/usr/bin/env bash
# Full project check: formatting, lints, tests, typecheck, frontend build, licences (Rust and
# npm, and the shipped list of them) and security advisories. Stops at the first failing step with
# a non-zero exit code.
#
#   scripts/check.sh
#
# Needs: Rust (rustup), cargo-deny, Node.js, pnpm, and `pnpm --dir app install` once.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# cargo is not on the PATH of every non-interactive shell.
if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck source=/dev/null
  . "$HOME/.cargo/env"
fi

CURRENT=""
STARTED=$SECONDS

on_exit() {
  local status=$?
  if [[ $status -ne 0 && -n "$CURRENT" ]]; then
    printf '\n\033[31mFAILED: %s\033[0m (exit %d)\n' "$CURRENT" "$status" >&2
  fi
}
trap on_exit EXIT

step() {
  CURRENT="$1"
  shift
  printf '\n\033[1m==> %s\033[0m\n    %s\n' "$CURRENT" "$*"
  local start=$SECONDS
  "$@"
  printf '    ok (%ds)\n' "$((SECONDS - start))"
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'missing tool: %s (%s)\n' "$1" "$2" >&2
    exit 1
  fi
}

need cargo "see docs/SETUP.md"
need pnpm "brew install pnpm"
need node "brew install node"
cargo deny --version >/dev/null 2>&1 || {
  echo "missing tool: cargo-deny (cargo install --locked cargo-deny)" >&2
  exit 1
}
if [[ ! -d app/node_modules ]]; then
  echo "app/node_modules is missing; run: pnpm --dir app install --frozen-lockfile" >&2
  exit 1
fi

# `unsafe` only in crates/webview2-guard (decision 0011): every other crate keeps
# `unsafe_code = "forbid"`, and no other code may allow or use `unsafe`. The guard crate itself
# is the positive control: the pattern must find its `#[allow(unsafe_code)]` and `unsafe {`.
check_unsafe() {
  local pattern='\bunsafe[[:space:]]*(\{|fn[[:space:]]|impl[[:space:]<]|extern[[:space:]]|trait[[:space:]])|allow\([^)]*unsafe_code'
  local lint='unsafe_code[[:space:]]*=[[:space:]]*"(allow|warn|deny)"'
  local outside inside
  outside="$( { grep -rnE "$pattern" --include='*.rs' crates app/src-tauri/src || true
                grep -rnE "$lint" --include='Cargo.toml' Cargo.toml crates app/src-tauri || true
              } | grep -v '^crates/webview2-guard/' || true)"
  inside="$(grep -rcE "$pattern" --include='*.rs' crates/webview2-guard | awk -F: '{ n += $2 } END { print n + 0 }')"
  if [[ -n "$outside" ]]; then
    printf 'unsafe outside crates/webview2-guard:\n%s\n' "$outside" >&2
    return 1
  fi
  if [[ "$inside" -lt 2 ]]; then
    echo "the unsafe check found nothing in crates/webview2-guard; is the pattern broken?" >&2
    return 1
  fi
}

# Rust
step "unsafe only in webview2-guard" check_unsafe
step "Rust formatting" cargo fmt --all --check
step "Clippy" cargo clippy --workspace --all-targets --locked -- -D warnings
# The desktop-test build (decision 0005) must stay warning-free too; its WebDriver tests need a
# GUI session and run separately: `pnpm --dir app test:e2e`.
step "Clippy (feature e2e)" cargo clippy -p mail-app --all-targets --locked --features e2e -- -D warnings
# Windows code (`cfg(windows)`: named pipe, credential manager, WebView2 guard) is compiled on the
# Mac only here, with cargo-xwin like scripts/build-windows.sh. Skipped, with a note, when the
# cross tools are missing; the Windows build itself needs them anyway.
clippy_windows() {
  local llvm_bin="${LLVM_BIN:-/opt/homebrew/opt/llvm/bin}"
  LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8 XWIN_CACHE_DIR="${XWIN_CACHE_DIR:-$PWD/.xwin-cache}" XWIN_ARCH=x86_64 \
    PATH="$llvm_bin:$PATH" cargo xwin clippy --target x86_64-pc-windows-msvc --workspace \
    --all-targets --locked -- -D warnings
}
if cargo xwin --version >/dev/null 2>&1 && [[ -x "${LLVM_BIN:-/opt/homebrew/opt/llvm/bin}/clang-cl" ]] &&
  rustup target list --installed 2>/dev/null | grep -qx x86_64-pc-windows-msvc; then
  step "Clippy (Windows target)" clippy_windows
else
  printf '\n==> Clippy (Windows target)\n    skipped: cargo-xwin, LLVM or the Rust target x86_64-pc-windows-msvc missing (docs/SETUP.md)\n'
fi
step "Rust tests" cargo test --workspace --locked
step "Rust tests (feature e2e)" cargo test -p mail-app --locked --features e2e --lib e2e::

# Frontend
step "TypeScript typecheck" pnpm --dir app typecheck
step "ESLint" pnpm --dir app lint
step "Vitest" pnpm --dir app test
step "Vite build" pnpm --dir app build

# Dependencies
step "cargo deny (licences, advisories, bans, sources)" cargo deny check --hide-inclusion-graph
# All npm dependencies, the desktop-test tree included (finding G-1). Fixes for that tree come in
# as exact `overrides`; an advisory without a fixed version is ignored only by its GHSA id with the
# reason, both in app/pnpm-workspace.yaml.
step "pnpm audit (all dependencies)" pnpm --dir app audit
# Stage 15: every shipped Rust crate and npm production package has a licence of the allow-list
# (deny.toml, plus the npm extras in the script), and the list in the app (Settings → About) and
# the installers is up to date. After a dependency change: `node scripts/licenses.mjs`.
step "Third-party licences (allow-list, shipped list up to date)" node scripts/licenses.mjs --check

CURRENT=""
printf '\n\033[32mAll checks passed\033[0m (%ds)\n' "$((SECONDS - STARTED))"
