# Setting up a development machine

Status 2026-11-22, checked on a Mac mini (macOS 27.2, Apple M4). Nothing needs `sudo`.

## Prerequisites

| Tool | Version (checked) | Source |
|---|---|---|
| Xcode or the Command Line Tools | Xcode in `/Applications/Xcode.app` | App Store / `xcode-select --install` |
| Homebrew | 7.0 | https://brew.sh |
| Node.js | 24.22 | `brew install node` |
| pnpm | 12.19 | `brew install pnpm` |
| Python 3 (standard library only, for seeding the test server) | 3.15 | with Xcode or Homebrew |

## Rust

```sh
# install rustup without changing the shell profiles
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --default-toolchain stable --profile default --no-modify-path

# make cargo available in every shell (including non-interactive ones, e.g. over SSH)
printf '\n. "$HOME/.cargo/env"\n' >> ~/.zshenv
. "$HOME/.cargo/env"

# target for the Windows cross-build
rustup target add x86_64-pc-windows-msvc
```

Checked: `rustc 1.100.0`, `cargo 1.100.0`. At least 1.98 is required (mail-parser, Tauri 2.12; see
`docs/decisions/0006-rusqlite-version-und-msrv.md`).

### When `cc` is not the C compiler

On the project's build Mac, `~/.local/bin/cc` is a different program (a session launcher) and comes
before `/usr/bin` in the `PATH`. Rust would call it as linker and C compiler and fail
("linking with `cc` failed …"). `~/.cargo/config.toml` therefore names the host tools explicitly:

```toml
[target.aarch64-apple-darwin]
linker = "/usr/bin/clang"

[env]
CC_aarch64_apple_darwin = "/usr/bin/clang"
CXX_aarch64_apple_darwin = "/usr/bin/clang++"
AR_aarch64_apple_darwin = "/usr/bin/ar"
HOST_CC = "/usr/bin/clang"
HOST_CXX = "/usr/bin/clang++"
```

On a machine where `cc` is the normal compiler the file is unnecessary, but it does no harm.

## Tools for the Windows build and the checks

```sh
brew install llvm nsis
cargo install --locked cargo-xwin cargo-deny
```

Checked: LLVM 23.1 (keg-only in `/opt/homebrew/opt/llvm/bin`, put on the `PATH` only for the
Windows build), NSIS `makensis v3.13`.

**NSIS needs a UTF-8 locale.** Without `LANG` (locale "C", as in non-interactive shells and
automated sessions) `makensis` 3.13 aborts while compiling with `std::bad_alloc` (exit 134, a known
NSIS bug, sourceforge nsis bugs/1165), even though `makensis -VERSION` works. The Windows build
therefore sets `LANG=en_US.UTF-8`. A real test:

```sh
printf 'OutFile t.exe\nSection\nSectionEnd\n' > /tmp/t.nsi && LANG=en_US.UTF-8 makensis /tmp/t.nsi
```

### Building the Windows installer on the Mac

Checked with the spike app (see `docs/decisions/0004-tauri-ipc-typen-und-windows-build.md`):

```sh
export LANG=en_US.UTF-8                            # required by makensis, see above
export XWIN_CACHE_DIR="$PWD/.xwin-cache"           # Windows SDK cache in the project (in .gitignore), about 630 MB
export XWIN_ARCH=x86_64                            # otherwise the aarch64 SDK is downloaded as well
PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

- The first run downloads the CRT and the Windows SDK (once, about 35 s) and `nsis_tauri_utils.dll`
  from GitHub.
- From LLVM, `clang-cl`, `llvm-lib` and `llvm-rc` are needed; cargo-xwin brings the linker.
- About 60 linker warnings `LNK4099` (missing PDB files) are harmless.
- `scripts/build-windows.sh [version]` wraps these commands (see below).

### Building the macOS app

```sh
CI=true pnpm tauri build --bundles app,dmg --config '{"bundle":{"macOS":{"signingIdentity":"-"}}}'
```

`CI=true` is needed when building over SSH or in tmux: otherwise the DMG step tries to lay out the
Finder window via AppleScript and fails with a timeout. `signingIdentity "-"` seals the whole
bundle ad hoc. Without it only the binary carries the linker's signature, `codesign --verify` fails
("code has no resources but signature indicates they must be present"), and a downloaded copy is
reported as damaged. `scripts/build-macos.sh [version]` wraps this (see below).

### Builds for distribution

| Script | Result |
|---|---|
| `scripts/build-macos.sh [version]` | `.app` and `.dmg` for Apple silicon under `target/release/bundle/`, ad-hoc signed; with a "Developer ID Application" identity in the keychain, signed, notarised and stapled (`docs/RELEASING.md`, "macOS signing and notarisation"; set up with `scripts/apple-signing-setup.sh`). Checks the signature and the disk image, starts the app from the image under a temporary `HOME` and quits it again (`SKIP_LAUNCH_CHECK=1` without a GUI session). Prints path, size and SHA-256, plus a `.sha256` file |
| `scripts/build-windows.sh [version]` | NSIS installer under `target/x86_64-pc-windows-msvc/release/bundle/nsis/`, with path, size, SHA-256 and a `.sha256` file |
| `scripts/release-local.sh [--force] [version]` | both builds, then `dist/<version>/<product name>-<version>-macos-arm64.dmg`, `…-windows-x64-setup.exe` and `SHA256SUMS` |

- **Version:** from the argument (semver), otherwise from `tauri.conf.json` (`version`), otherwise
  from the Cargo workspace. It goes into the build via `--config '{"version":"…"}'`; no file is
  changed. Tauri sets `package_info().version` from it; "About" shows it, and so do Info.plist,
  VERSIONINFO of the `.exe` and the file names.
- **Build state:** `app/src-tauri/build.rs` records the commit (`git rev-parse --short=10`, with
  `-dirty` when tracked files differ from the commit) and the build date. The scripts set
  `SOURCE_DATE_EPOCH`: the commit time for a clean tree, otherwise the current time; a value that is
  already set wins. Because cargo reruns `build.rs` only when HEAD, `SOURCE_DATE_EPOCH` or the Tauri
  config changes, this keeps both values current. `release-local.sh` gives both builds the same
  value. It stops if `Cargo.lock` does not match the manifests or if HEAD moves during the builds
  (otherwise the macOS and Windows apps would show different commits in "About").
- **Apple silicon only.** An Intel or universal build would need the Rust target
  `x86_64-apple-darwin`, on a machine with the `cc` problem above also linker and `CC` for that
  target in `~/.cargo/config.toml`, a second full LTO build (about twice the build time) and about
  2.5 GB more in `target/`. That is extra effort, so Intel Macs are left out for now. Whether they
  come with the CI builds (stage 15) is open.
- Installation, Gatekeeper and SmartScreen warnings, data locations and uninstalling:
  `docs/INSTALL.md`.

## Test server

A local IMAP/SMTP server for all tests: Stalwart 1.1.1, only on `127.0.0.1`, data in
`testmail/.data/`. `scripts/testmail.sh` downloads the binary itself on the first start (fixed
version, SHA-256 checked) into `testmail/bin/`. No Docker, no `sudo`.

```sh
scripts/testmail.sh reset          # initial state: new data, new test CA, accounts created
scripts/testmail.sh start          # starts and waits until all ports answer
scripts/testmail.sh seed 1000      # creates test messages (deterministic)
scripts/testmail.sh check          # proves IDLE, CONDSTORE, MOVE, SPECIAL-USE, keywords, TLS
scripts/testmail.sh stop
```

Ports, accounts (`alice@vicemail.test`, `bob@vicemail.test`) and the path to the test CA are in
`testmail/accounts.env`. Details and quirks of the server: `testmail/README.md` and
`docs/decisions/0001-testserver.md`.

## Checking

```sh
rustc --version
cargo --version
cargo xwin --version
cargo deny --version
makensis -VERSION
```

`scripts/check.sh` also compiles the code for Windows (`cargo xwin clippy --target
x86_64-pc-windows-msvc`), so that code behind `cfg(windows)` (named pipe, credentials, WebView2
guard) does not break unnoticed on the Mac. If cargo-xwin, LLVM or the Rust target are missing, the
step is skipped with a note. The Windows SDK lives in `.xwin-cache/` in the repository; in an
additional `git worktree`, link it first (`ln -s <repo>/.xwin-cache .xwin-cache`), otherwise
cargo-xwin downloads it again.
