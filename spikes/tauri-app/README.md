# Spike C: Tauri 2 app, typed IPC, Windows cross-build

Throwaway code from stage 0. Product code must never import from here.
Decision and measurements are in `docs/decisions/0004-tauri-ipc-typen-und-windows-build.md`.

What it shows:

- Tauri 2 + React + TypeScript (strict), one window, strict CSP, no plugins.
- Commands returning a nested typed struct (`Option`, `Vec`, tagged enums, timestamps,
  `u64`/`i64` with explicit JS mapping) and a typed error `{ code, message, detail }`.
- A typed event stream (`CoreEvent`) from the core to the UI.
- The same Rust types exported with **tauri-specta** (chosen, used by `src/services/ipc.ts`)
  and with **ts-rs** (comparison, `src/services/ipc-tsrs.ts`).
- On start the UI runs a self-test against both variants and reports the result to stdout.

## Layout

```
src/                      React UI
  App.tsx                 view list (add a view = one entry)
  views/IpcView.tsx       IPC self-test
  services/ipc.ts         the only module that calls the core (tauri-specta)
  services/ipc-tsrs.ts    same calls with ts-rs types, comparison only
  generated/              generated bindings, checked in
src-tauri/
  src/model.rs            IPC types (derive both specta::Type and ts_rs::TS)
  src/commands.rs         commands, fixture data, event emitter
  src/lib.rs              tauri-specta builder, app setup, tests
```

## Commands

```sh
pnpm install
pnpm tauri dev                                  # dev mode

# Rust tests; fail if generated bindings are stale
(cd src-tauri && cargo test)
(cd src-tauri && UPDATE_BINDINGS=1 cargo test)  # regenerate bindings

pnpm typecheck

# macOS release (.app + .dmg). CI=true skips the Finder AppleScript step of the
# DMG script, which times out in SSH/tmux sessions.
CI=true pnpm tauri build --bundles app,dmg

# Self-test of the built app: exits 0 when every check passed.
SPIKE_SELFTEST=1 "src-tauri/target/release/bundle/macos/Vice Mail Spike.app/Contents/MacOS/vice-mail-spike"

# Windows installer (NSIS) cross-built on macOS
export LANG=en_US.UTF-8                         # makensis crashes with the C locale
export XWIN_CACHE_DIR="$(git rev-parse --show-toplevel)/.xwin-cache"
export XWIN_ARCH=x86_64
PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

Outputs:

- `src-tauri/target/release/bundle/macos/Vice Mail Spike.app`
- `src-tauri/target/release/bundle/dmg/Vice Mail Spike_0.1.0_aarch64.dmg`
- `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Vice Mail Spike_0.1.0_x64-setup.exe`
