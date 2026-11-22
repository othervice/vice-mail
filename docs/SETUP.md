# Einrichtung eines Entwicklungsrechners

Stand 22.11.2026, geprüft auf dem Mac mini „Jarvis“ (macOS 27.2, Apple M4). Alles ohne `sudo`.

## Voraussetzungen

| Werkzeug | Version (geprüft) | Bezug |
|---|---|---|
| Xcode bzw. Command Line Tools | Xcode unter `/Applications/Xcode.app` | App Store / `xcode-select --install` |
| Homebrew | 7.0 | https://brew.sh |
| Node.js | 24.22 | `brew install node` |
| pnpm | 12.19 | `brew install pnpm` |
| Python 3 (nur Standardbibliothek, für den Testserver-Seed) | 3.15 | mit Xcode bzw. Homebrew |

## Rust

```sh
# rustup ohne Änderung der Shell-Profile installieren
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --default-toolchain stable --profile default --no-modify-path

# cargo in jede Shell holen (auch nicht interaktive, z. B. per SSH)
printf '\n. "$HOME/.cargo/env"\n' >> ~/.zshenv
. "$HOME/.cargo/env"

# Ziel für den Windows-Cross-Build
rustup target add x86_64-pc-windows-msvc
```

Geprüft: `rustc 1.100.0`, `cargo 1.100.0`. Mindestens 1.90 ist nötig (Tauri 2.12, siehe `docs/decisions/0006-rusqlite-version-und-msrv.md`).

### Nur auf dem Mac mini: `cc` ist belegt

Auf dem Mac mini ist `~/.local/bin/cc` das Startwerkzeug für Claude-Sitzungen und steht im PATH vor
`/usr/bin`. Rust würde es als Linker und C-Compiler aufrufen und scheitern
(„linking with `cc` failed … Kein Projekt passt zu …“). Deshalb legt `~/.cargo/config.toml` die
Host-Werkzeuge fest:

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

Auf einem Rechner ohne dieses Werkzeug ist die Datei unnötig, schadet aber nicht.

## Werkzeuge für den Windows-Build und die Prüfungen

```sh
brew install llvm nsis
cargo install --locked cargo-xwin cargo-deny
```

Geprüft: LLVM 23.1 (keg-only unter `/opt/homebrew/opt/llvm/bin`, wird nur für den Windows-Build in
den PATH genommen), NSIS `makensis v3.13`.

**NSIS braucht ein UTF-8-Locale.** Ohne `LANG` (Locale „C“, so in nicht interaktiven Shells und
Claude-Sitzungen) bricht `makensis` 3.13 beim Kompilieren mit `std::bad_alloc` ab (Exit 134, bekannter
NSIS-Fehler, sourceforge nsis bugs/1165), obwohl `makensis -VERSION` funktioniert. Der Windows-Build
setzt deshalb `LANG=en_US.UTF-8`. Echter Test:

```sh
printf 'OutFile t.exe\nSection\nSectionEnd\n' > /tmp/t.nsi && LANG=en_US.UTF-8 makensis /tmp/t.nsi
```

### Windows-Installer vom Mac aus bauen

Geprüft mit der Spike-App (siehe `docs/decisions/0004-tauri-ipc-typen-und-windows-build.md`):

```sh
export LANG=en_US.UTF-8                            # Pflicht für makensis, siehe oben
export XWIN_CACHE_DIR="$PWD/.xwin-cache"           # Windows-SDK-Cache im Projekt (in .gitignore), ca. 630 MB
export XWIN_ARCH=x86_64                            # sonst wird auch das aarch64-SDK geladen
PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

- Der erste Lauf lädt CRT und Windows-SDK (einmalig, etwa 35 s) und `nsis_tauri_utils.dll` von GitHub.
- Aus LLVM werden `clang-cl`, `llvm-lib` und `llvm-rc` gebraucht; den Linker bringt cargo-xwin mit.
- Etwa 60 Linker-Warnungen `LNK4099` (fehlende PDB-Dateien) sind harmlos.
- Ab Stufe 6 kapselt `scripts/build-windows.sh` diese Befehle.

### macOS-App bauen

```sh
CI=true pnpm tauri build --bundles app,dmg
```

`CI=true` ist nötig, wenn über SSH oder tmux gebaut wird: Sonst versucht der DMG-Schritt, das
Finder-Fenster per AppleScript zu gestalten, und bricht mit einer Zeitüberschreitung ab.

## Testserver

Lokaler IMAP/SMTP-Server für alle Tests: Stalwart 1.1.1, nur auf `127.0.0.1`, Daten unter
`testmail/.data/`. Das Binary lädt `scripts/testmail.sh` beim ersten Start selbst herunter (feste
Version, SHA-256 geprüft) nach `testmail/bin/`. Kein Docker, kein `sudo`.

```sh
scripts/testmail.sh reset          # Ausgangszustand: neue Daten, neue Test-CA, Konten anlegen
scripts/testmail.sh start          # startet und wartet, bis alle Ports antworten
scripts/testmail.sh seed 1000      # erzeugt Testmails (deterministisch)
scripts/testmail.sh check          # weist IDLE, CONDSTORE, MOVE, SPECIAL-USE, Schlagwörter, TLS nach
scripts/testmail.sh stop
```

Ports, Konten (`alice@vicemail.test`, `bob@vicemail.test`) und der Pfad zur Test-CA stehen in
`testmail/accounts.env`. Einzelheiten und Eigenheiten des Servers: `testmail/README.md` und
`docs/decisions/0001-testserver.md`.

## Prüfen

```sh
rustc --version
cargo --version
cargo xwin --version
cargo deny --version
makensis -VERSION
```
