#!/usr/bin/env bash
# Full project check: formatting, lints, tests, typecheck, frontend build, licences and
# security advisories. Stops at the first failing step with a non-zero exit code.
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
cargo deny --version >/dev/null 2>&1 || {
  echo "missing tool: cargo-deny (cargo install --locked cargo-deny)" >&2
  exit 1
}
if [[ ! -d app/node_modules ]]; then
  echo "app/node_modules is missing; run: pnpm --dir app install --frozen-lockfile" >&2
  exit 1
fi

# Rust
step "Rust formatting" cargo fmt --all --check
step "Clippy" cargo clippy --workspace --all-targets --locked -- -D warnings
# The desktop-test build (decision 0005) must stay warning-free too; its WebDriver tests need a
# GUI session and run separately: `pnpm --dir app test:e2e`.
step "Clippy (feature e2e)" cargo clippy -p mail-app --all-targets --locked --features e2e -- -D warnings
step "Rust tests" cargo test --workspace --locked
step "Rust tests (feature e2e)" cargo test -p mail-app --locked --features e2e --lib e2e::

# Frontend
step "TypeScript typecheck" pnpm --dir app typecheck
step "ESLint" pnpm --dir app lint
step "Vitest" pnpm --dir app test
step "Vite build" pnpm --dir app build

# Dependencies
step "cargo deny (licences, advisories, bans, sources)" cargo deny check --hide-inclusion-graph
step "pnpm audit (production dependencies)" pnpm --dir app audit --prod

CURRENT=""
printf '\n\033[32mAll checks passed\033[0m (%ds)\n' "$((SECONDS - STARTED))"
