# Spikes C and D: Tauri 2 app, typed IPC, Windows cross-build, HTML sandbox, desktop tests

Throwaway code from stage 0. Product code must never import from here.
Decisions and measurements: `docs/decisions/0004-tauri-ipc-typen-und-windows-build.md` (C) and
`docs/decisions/0005-html-sandbox-und-desktop-tests.md` (D).

What it shows:

- Tauri 2 + React + TypeScript (strict), one window, strict CSP, no plugins.
- Commands returning a nested typed struct (`Option`, `Vec`, tagged enums, timestamps,
  `u64`/`i64` with explicit JS mapping) and a typed error `{ code, message, detail }`.
- A typed event stream (`CoreEvent`) from the core to the UI.
- The same Rust types exported with **tauri-specta** (chosen, used by `src/services/ipc.ts`)
  and with **ts-rs** (comparison, `src/services/ipc-tsrs.ts`).
- On start the UI runs a self-test against both variants and reports the result to stdout.

Spike D adds a "Sandbox" view that renders a hostile HTML corpus in sandboxed `srcdoc` iframes
(one frame per vector, three variants plus positive controls), a Rust navigation hook that blocks
and reports every non-app navigation (also from sub-frames), and a WebdriverIO run that proves
nothing gets out.

## Layout

```
src/                      React UI
  App.tsx                 view list (add a view = one entry)
  views/IpcView.tsx       IPC self-test
  views/SandboxView.tsx   spike D: frames, detectors, test hook `window.__vmSandbox`
  sandbox/corpus.ts       spike D: hostile snippets, one counter path per vector
  sandbox/frames.ts       spike D: variants A/B/N, positive controls, frame CSP
  services/ipc.ts         the only module that calls the core (tauri-specta)
  services/ipc-tsrs.ts    same calls with ts-rs types, comparison only
  generated/              generated bindings, checked in
public/sandbox-probe.js   spike D: probe script for the script vectors and controls
src-tauri/
  src/model.rs            IPC types (derive both specta::Type and ts_rs::TS)
  src/commands.rs         commands, fixture data, event emitter
  src/sandbox.rs          spike D: navigation hook, breach detector, command log
  src/e2e.rs              spike D, `--features e2e` only: native NSEvent clicks
  src/lib.rs              tauri-specta builder, app setup, tests
  tauri.e2e.conf.json     CSP override for the e2e build (adds the `/allowed/` control paths)
e2e/
  counter-server.mjs      counts connections and requests on 127.0.0.1:18999/:18997, control :18998
  sandbox.e2e.mjs         WebdriverIO spec
  check-release.mjs       proves the embedded WebDriver is not in the release build
wdio.conf.mjs             WebdriverIO + @wdio/tauri-service (embedded WebDriver)
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

# Spike D: HTML sandbox e2e. Builds the test app (debug, `--features e2e`, embedded assets,
# production CSP + tauri.e2e.conf.json), starts the counter server and the app, runs every check.
# Exit code != 0 if any vector runs a script, connects out, navigates or reaches Tauri, or if a
# positive control does not fire. Needs a logged-in GUI session (a window opens for ~40 s).
pnpm test:e2e
pnpm test:e2e:run                               # same, without rebuilding
# Results per vector: e2e/results/sandbox-results.json (not checked in)

# Spike D: the embedded WebDriver is only in test builds (needs both builds to exist)
CI=true pnpm tauri build --bundles app && pnpm test:release-clean

# Counter server by hand (Ctrl+C to stop)
node e2e/counter-server.mjs

# Windows installer (NSIS) cross-built on macOS
export LANG=en_US.UTF-8                         # makensis crashes with the C locale
export XWIN_CACHE_DIR="$(git rev-parse --show-toplevel)/.xwin-cache"
export XWIN_ARCH=x86_64
PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

The embedded WebDriver (`tauri-plugin-wdio-webdriver`) and the e2e helper command are behind the
Cargo feature `e2e`; a normal `cargo build` / `tauri build` does not contain them. The test does not
use `tauri-plugin-wdio` (execute API, mocks); see the decision for the `switchToWindow` workaround.

Outputs:

- `src-tauri/target/release/bundle/macos/Vice Mail Spike.app`
- `src-tauri/target/release/bundle/dmg/Vice Mail Spike_0.1.0_aarch64.dmg`
- `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Vice Mail Spike_0.1.0_x64-setup.exe`
- `src-tauri/target/debug/vice-mail-spike` (e2e test build, about 45 MB, contains the WebDriver)
