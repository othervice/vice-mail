# 0004 — IPC-Typen mit tauri-specta, Windows-Installer per Cross-Build

- **Datum:** 22.11.2026
- **Stufe:** 0, Spike C (`spikes/tauri-app/`)
- **Status:** angenommen

## Kontext

`docs/ARCHITECTURE.md` verlangt, dass die Oberfläche nur über typisierte Commands und Events mit
dem Kern spricht und dass die TypeScript-Typen aus Rust erzeugt werden, nie von Hand. Stufe 1
verlangt dazu einen typisierten Fehler `{ code, message, detail }` und einen typisierten
Ereignisstrom vom Kern zur Oberfläche. Offen waren zwei Fragen: Soll das mit `ts-rs` oder mit
`tauri-specta` 2.0-rc geschehen? Und lässt sich der Windows-Installer wirklich vom Mac aus bauen?

Der Spike ist eine kleinste Tauri-2-App mit React (TypeScript strict) und strenger CSP, ohne Plugins.
Sie hat drei Commands und ein Ereignis. Die Command-Antwort ist verschachtelt und enthält `Option`,
`Vec`, ein intern getaggtes Enum mit Daten (`Body`), ein benachbart getaggtes Enum (`Flag`), einen
Zeitstempel (`DateTime<Utc>` als RFC-3339-String), `u64`/`i64` und einen Fehler über
`Result<T, IpcError>`. Das Ereignis `CoreEvent` ist ein nach `type` getaggtes Enum und wird aus einem
Hintergrund-Thread gesendet. Jeder Typ leitet **beide** Generatoren ab (`specta::Type` und `ts_rs::TS`).
So ist die Ausgabe direkt vergleichbar. Beide Varianten laufen in der App gegen dieselben Commands.

## Optionen

| Kriterium | ts-rs | tauri-specta |
|---|---|---|
| Version | 12.0.1, stabil | 2.0.0-rc.29 mit specta 2.0.0-rc.28 und specta-typescript 0.0.15; RC-Reihe seit 2024 |
| Releases und Pflege | letztes Release 31.01.2026 (knapp 10 Monate alt); Commits bis 09.11.2026; 32 offene Issues | rc.29 am 13.11.2026, rc.24 bis rc.29 zwischen März und November 2026; Commits bis 20.11.2026; 4 offene Issues |
| Lizenz | MIT | MIT (alle drei Crates) |
| Kompatibilität | unabhängig von Tauri | verlangt `tauri ^2` mit Feature `specta`; läuft mit Tauri 2.12.7 und Rust 1.100 |
| Typisierte Commands | nein: Command-Name, Argumentnamen und Rückgabetyp stehen von Hand in `invoke<T>("name", { args })` | ja: `commands.getMessage(query)` wird erzeugt, Name, Argumente und Ergebnis kommen aus Rust |
| Typisierte Events | nein: Ereignisname als String-Literal doppelt gepflegt, Payload-Typ von Hand an `listen<T>` | ja: `events.coreEvent.listen(cb)`, Name aus dem Rust-Typ (`core-event`) |
| Fehlerabbildung | Typ `IpcError` wird erzeugt, die Umwandlung von Reject in ein Ergebnis ist Handarbeit | `ErrorHandlingMode::Result` liefert `{ status: "ok", data } \| { status: "error", error: IpcError }` |
| `u64`/`i64` | Standard ist `bigint`. Das ist **falsch**, denn `JSON.parse` liefert `number`. Umstellbar global über `TS_RS_LARGE_INT=number` (stiller Präzisionsverlust) oder pro Feld mit `#[ts(type = "…")]` | Standard: Der Export **bricht ab** (`BigIntForbidden`). Jedes Feld braucht eine Entscheidung: `#[specta(type = specta_typescript::Number)]` oder String über `#[serde(with)]` plus `#[specta(type = String)]` |
| Erzeugung | `#[ts(export)]` legt Tests an, die bei `cargo test` schreiben; oder per API `export_all` / `export_to_string` | per Code: `builder.export(Typescript, pfad)`; die Doku schlägt den Debug-Start vor |
| Aktualität prüfbar | ja, mit eigenem Test (Export in ein Temp-Verzeichnis und Vergleich) | ja, genauso |
| Ausgabe | eine Datei je Typ mit Importen | eine Datei `bindings.ts`: Typen, Commands, Events, Konstanten und eine kleine Laufzeit (rund 40 Zeilen) |
| Serde-Phasen | eine Form je Typ | standardmäßig Paare `X_Serialize` / `X_Deserialize` plus Union `X`; mit `disable_serde_phases()` eine Form |
| Aufwand in `ipc.ts` | je Command eine handgeschriebene Zeile mit String-Name und Argumentobjekt, ungeprüft; Ereignisname als Konstante | je Command eine Zeile Weiterleitung; ein Tippfehler oder ein umbenannter Rust-Parameter bricht `tsc` |
| Frontend-Tests | `mockIPC` von Hand | `.mocks()` erzeugt typisierte `mockCommands` (im Spike nicht ausprobiert) |
| Mehraufwand in Rust | Derive `TS` | Derive `specta::Type`, `#[specta::specta]` an jedem Command, `collect_commands!`/`collect_events!`, Derive `tauri_specta::Event` |

## Entscheidung

**tauri-specta**, Versionen exakt gepinnt:

```toml
specta = { version = "=2.0.0-rc.28", features = ["derive", "chrono"] }   # chrono nur, wenn benutzt
specta-typescript = "=0.0.15"
tauri-specta = { version = "=2.0.0-rc.29", features = ["derive", "typescript"] }
```

Builder-Einstellungen: `.error_handling(ErrorHandlingMode::Result)` und `.disable_serde_phases()`.
Die IPC-Typen sind einseitig, Argumente gehen hinein, Ergebnisse und Ereignisse heraus. Der Export
läuft **per Test** und nicht beim Debug-Start. Die erzeugte Datei wird eingecheckt. Ein Test schlägt
fehl, wenn sie veraltet ist. `UPDATE_BINDINGS=1 cargo test` schreibt sie neu.

## Begründung

1. Stufe 1 verlangt typisierte Commands **und** einen typisierten Ereignisstrom. ts-rs deckt nur
   Typen ab. Command-Namen, Argumentnamen und Ereignisnamen blieben ungeprüfte Strings. Genau diese
   Fehlerklasse soll die Regel „nie von Hand gepflegt“ ausschließen.
2. Bei großen Zahlen zwingt specta zur Entscheidung. ts-rs erzeugt ohne Hinweis einen Typ, der zur
   Laufzeit nicht stimmt. Ein Test im Spike belegt beides (`unannotated_u64_specta_refuses_ts_rs_emits_bigint`).
3. Die Pflege ist sehr aktiv. ts-rs liegt mit dem letzten Release knapp innerhalb der 12-Monats-Regel.
4. Das RC-Risiko ist begrenzt. Die `=`-Pins verhindern stille Updates. Die erzeugte Datei ist eingecheckt
   und versioniert. Alles hängt hinter `app/src/services/ipc.ts`. Der Rückweg ist mechanisch, weil
   beide Werkzeuge dieselben serde-Attribute lesen.

## Nachweise

- `cargo test` (6 Tests): Fehlerform `{ code: "not_found", message, detail }`, `modSeq` als String,
  Größen als Zahl, Tags der Enums, Ereignisname `core-event`, BigInt-Verhalten beider Werkzeuge
  und die Aktualität beider erzeugter Ausgaben. Für die Aktualität wurde absichtlich ein Doc-Kommentar
  geändert. Danach schlugen `tauri_specta_bindings_are_fresh` und `ts_rs_bindings_are_fresh` fehl.
- `tsc --noEmit` (TypeScript 7.0.2, strict, `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`,
  `noUnusedLocals`) läuft fehlerfrei über beide erzeugten Ausgaben, in 0,9 s.
- Gebaute Release-App (macOS) mit `SPIKE_SELFTEST=1` gestartet: Alle 13 Prüfungen der Oberfläche
  bestanden, Exit-Code 0. Geprüft wurden:
  - typisierte Antwort über tauri-specta
  - `modSeq` exakt `"9007199254740993"` (2^53+1). `JSON.parse` derselben Zahl ergibt `9007199254740992`.
  - typisierter Fehler `not_found`
  - die Antwort auf demselben Weg über ts-rs, identisch
  - Ereignisse über beide Wege: 6 Ereignisse, davon 5 Fortschrittsmeldungen
  - alles unter der strengen CSP aus `tauri.conf.json`
- `SPIKE_SELFTEST=1 pnpm tauri dev` (Debug-Build, Vite-Dev-Server, `devCsp` mit `'unsafe-inline'`
  für Styles und `ws://localhost:1420`): Alle Prüfungen bestanden, 43 s kalt.
- `open "…/Vice Mail Spike.app"`: Der Prozess läuft laut `pgrep` mit rund 100 MB RSS plus WebContent-Prozess.
  Danach wurde die App per `osascript … quit` beendet.
- **Framework-Fehler sind keine `IpcError`.** Ein falsches Argument wird mit einem **String**
  abgewiesen, zum Beispiel: `invalid args `query` for command `get_message`: invalid type: string "not-a-number", expected u32`.
  Das gilt auch für unbekannte Commands und ACL-Verweigerungen. Der Result-Modus von tauri-specta
  reicht so einen String als `error` durch, obwohl der Typ `IpcError` behauptet. `ipc.ts` prüft deshalb
  mit `isIpcError` und bildet alles andere auf `{ code: "internal", … }` ab. Die Prüfung im Self-Test besteht.

## Messwerte

**Hardware:** Mac mini M4 (10 Kerne), 16 GB RAM, macOS 27.2.1, Rust 1.100.0, Node 24.22.3,
pnpm 12.19.0, Xcode 27.2. **Parallel liefen andere Builds** anderer Sitzungen (Load 3 bis 7).
Die Zeiten sind deshalb eher Obergrenzen. „Kalt“ heißt: leeres `target/`, Crates schon im
Cargo-Cache, `node_modules` installiert. „Warm“ heißt: `touch src-tauri/src/commands.rs` und neu bauen.

Release-Profil im Spike: `codegen-units = 1`, `lto = true`, `strip = true`.

| Build | gesamt | davon cargo |
|---|---|---|
| macOS `.app` + `.dmg`, kalt | 71 s | 61 s |
| macOS, warm | 49 s | 39 s |
| macOS, Standard-Release-Profil (ohne LTO, 16 CGUs), kalt | 57 s | 45 s |
| macOS, Standard-Release-Profil, warm | 25 s | 14 s |
| Download MSVC-CRT und Windows-SDK (einmalig, nur x86_64) | 35 s | – |
| Windows-NSIS, kalt (eigenes leeres Target-Verzeichnis) | 74 s | 71 s |
| Windows-NSIS, erster Build im gemeinsamen `target/` nach dem Mac-Build | 62 s | 59 s |
| Windows-NSIS, warm | 43 s | 40 s |
| `cargo test`, Debug, kalt | 23 s | 22 s |
| Vite-Build des Frontends | 0,2 s | – |

Der warme Release-Build dauert wegen Fat-LTO mit einer Codegen-Unit lange: 39 s statt 14 s cargo-Zeit.
Dafür halbiert sich die `.app` von 10 MB auf 5,2 MB, das `.dmg` schrumpft von 2,9 auf 2,2 MB. Im Alltag läuft
`tauri dev` mit Debug-Builds, im Spike 43 s kalt bis zum bestandenen Self-Test. Das LTO-Profil ist
deshalb nur für ausgelieferte Builds sinnvoll.

| Artefakt | Größe |
|---|---|
| `Vice Mail Spike.app` | 5,2 MB (Binary 5.365.968 B) |
| `Vice Mail Spike_0.1.0_aarch64.dmg` | 2,2 MB (2.295.292 B) |
| `vice-mail-spike.exe` | 8,3 MB (8.303.104 B) |
| `Vice Mail Spike_0.1.0_x64-setup.exe` (NSIS, WebView2 über `downloadBootstrapper`) | 1,9 MB (1.975.278 B) |
| `.app` / `.dmg` mit Standard-Release-Profil | 10 MB / 2,9 MB (2.999.829 B) |
| `.xwin-cache/` (CRT und SDK, nur x86_64) | 631 MB |
| `target/` nach macOS- und Windows-Build | 1,5 GB |
| JS-Bundle (React und App) | 226 kB, gzip 71 kB |

**Pfade:**

- `spikes/tauri-app/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Vice Mail Spike_0.1.0_x64-setup.exe`
- `spikes/tauri-app/src-tauri/target/release/bundle/dmg/Vice Mail Spike_0.1.0_aarch64.dmg`
- `spikes/tauri-app/src-tauri/target/release/bundle/macos/Vice Mail Spike.app`

Die `.exe` ist ein PE32+-GUI-Programm. Sie enthält Icon, VERSIONINFO und Manifest, erzeugt mit `llvm-rc`
und geprüft mit `llvm-readobj --coff-resources`. Sie lädt nur System-DLLs. Der WebView2-Loader ist
statisch gelinkt, es muss also keine `WebView2Loader.dll` mitgeliefert werden.

## Befehle für den Windows-Cross-Build

Einmalig (siehe `docs/SETUP.md`): `rustup target add x86_64-pc-windows-msvc`,
`cargo install cargo-xwin` (0.24.1), `brew install llvm nsis` (LLVM 23.1.6, makensis 3.13).

```sh
cd "<Projekt>/spikes/tauri-app"          # später: app/
export LANG=en_US.UTF-8                  # Pflicht: makensis stürzt mit der C-Locale ab
export XWIN_CACHE_DIR="<Projekt>/.xwin-cache"   # cargo-xwin legt darunter xwin/ an
export XWIN_ARCH=x86_64                  # nur x64 laden (Standard: x86_64 und aarch64)
PATH="/opt/homebrew/opt/llvm/bin:$PATH" \
  pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
```

Optional vorab, etwa für Offline-Builds: `cargo xwin cache xwin` mit denselben Variablen.
LLVM kommt nur für diesen Aufruf in den PATH. Gebraucht werden daraus `clang-cl`, `llvm-lib`
und `llvm-rc`. `lld-link` liefert cargo-xwin selbst als Symlink auf `rust-lld` aus der Rust-Toolchain.

macOS-Build (`.app` und `.dmg`):

```sh
CI=true pnpm tauri build --bundles app,dmg
```

In `tauri.conf.json` steht `"bundle": { "targets": "all", … }`. Die Auswahl läuft über `--bundles`.

## Stolpersteine

1. **`cc` ist kein C-Compiler.** `~/.local/bin/cc` ist der Sitzungsstarter und liegt im PATH vor
   `/usr/bin/cc`. Deshalb schlug jedes Linken für macOS fehl („Kein Projekt passt zu …“). Die Hauptsitzung
   hat das in `~/.cargo/config.toml` behoben: Linker sowie `CC`/`CXX`/`AR` für `aarch64-apple-darwin`
   und `HOST_CC`/`HOST_CXX` stehen dort auf `/usr/bin/clang`. Den Windows-Build stört das nicht,
   denn cargo-xwin setzt die zielbezogenen Variablen selbst auf `clang-cl`.
2. **makensis 3.13 stürzt mit der C-Locale ab.** Ohne `LANG` (so in SSH- und tmux-Sitzungen) bricht
   jedes Skript bei „Generating language tables…“ mit `std::bad_alloc` ab (Exit 134), selbst ein leeres
   `Section`/`SectionEnd`. `makensis -VERSION` läuft trotzdem, ist als Prüfung also wertlos. Das ist ein
   bekannter NSIS-Fehler (sourceforge nsis bugs/1165). Auch der Test der Homebrew-Formel setzt deshalb
   `LANG=en_GB.UTF-8`. Abhilfe: `LANG=en_US.UTF-8`. Es reicht auch `LC_CTYPE`, ebenso
   `de_DE.UTF-8` oder `C.UTF-8`. Ein echter Test:
   `printf 'OutFile t.exe\nSection\nSectionEnd\n' > t.nsi && LANG=en_US.UTF-8 makensis t.nsi`.
3. **DMG über SSH/tmux:** `bundle_dmg.sh` richtet das DMG-Fenster per AppleScript im Finder ein. Das lief
   in einen Timeout („AppleEvent lieferte eine Zeitüberschreitung“, -1712), der Build brach nach etwa
   2 Minuten ab, und `bundle/macos/rw.*.dmg` blieb liegen. Abhilfe: `CI=true`. Tauri übergibt dann
   `--skip-jenkins`, und das DMG entsteht ohne Fensterlayout. Mit `TAURI_BUNDLER_DMG_IGNORE_CI=true`
   ließe sich das wieder abschalten. Für ein gestaltetes DMG braucht das Terminal eine Automation-Freigabe
   für den Finder (Systemeinstellungen → Datenschutz & Sicherheit → Automation).
4. **`XWIN_CACHE_DIR`** ist eine versteckte Option und erscheint nicht in `--help`. Der Cache liegt dann
   unter `$XWIN_CACHE_DIR/xwin/`. Ohne die Variable landet er unter `~/Library/Caches/cargo-xwin`.
5. **Netz beim ersten NSIS-Build:** Tauri lädt `nsis_tauri_utils.dll` v0.5.3 von GitHub (Hash geprüft)
   nach `~/Library/Caches/tauri/NSIS/`. Der xwin-Download geht an Microsoft. Beides betrifft den Build-Rechner,
   nicht die App.
6. **Rauschen im Log:** etwa 60 Zeilen `lld-link: Cannot use debug info for 'libcmt.lib(…)' [LNK4099]`,
   weil die PDBs der CRT fehlen. Das ist harmlos und ließe sich mit `-C link-arg=/ignore:4099` unterdrücken. Dazu
   kommen die Warnung „Cross-platform compilation is experimental“, Signierung übersprungen
   (`bundle > windows > signCommand` wäre möglich) und `warning 5202: -OUTPUTCHARSET is disabled`.
7. **Icons:** `pnpm tauri icon <svg>` nimmt SVG direkt. Es erzeugt auch Android-, iOS- und
   Store-Logos, die für Desktop nicht nötig sind. Gebraucht werden `32x32.png`, `128x128.png`,
   `128x128@2x.png`, `icon.icns` und `icon.ico`.
8. **Capabilities:** Zum Empfangen von Ereignissen braucht das Fenster `core:event:default`. Eigene
   Commands der App sind ohne Eintrag erlaubt.
9. **CSP:** `default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'`
   funktioniert in der Release-App unter macOS. Inline-`style`-Attribute in React würden daran scheitern;
   Styles gehören in CSS-Dateien. `http://ipc.localhost` ist die IPC-Adresse unter Windows (WebView2). Ob
   die CSP dort greift, ist **ungeprüft**.

## Ungeprüft

- Der Installer auf Windows (Installation, Start, WebView2-Nachinstallation, Self-Test). Das prüft der Mensch.
- typisierte Mocks von tauri-specta (`.mocks()`) mit Vitest
- ein Rust-Test, der ein Ereignis über die Tauri-Mock-Runtime (`tauri::test`) empfängt

## Rückweg

- **Zu ts-rs:** Die Derives tauschen (`specta::Type` → `ts_rs::TS`), die serde-Attribute bleiben. Statt
  `#[specta(type = …)]` kommt `#[ts(type = "…")]`. Die `invoke`-/`listen`-Wrapper in `ipc.ts` werden von Hand
  geschrieben, Vorlage ist `spikes/tauri-app/src/services/ipc-tsrs.ts`. Dazu kommt ein Test, der die
  Command-Namen gegen eine Liste prüft. Aufwand: Stunden, nicht Tage.
- **Bricht ein neuer RC:** auf der gepinnten Version bleiben. Die generierte Datei ist eingecheckt, der
  Build hängt also nicht am Generator.
- **Windows-Cross-Build:** Bauen auf echtem Windows oder auf gehosteten Runnern (Stufe 15). Für MSI (WiX)
  wäre das ohnehin nötig, der Cross-Build kann nur NSIS.

## Nachtrag (Spike D)

Die CSP der Spike-App wurde in Spike D für die Mail-Anzeige erweitert (`style-src 'unsafe-inline'`,
`img-src data:`, `frame-src http: https:`). Gültig für das Produkt ist die Beschreibung in
`docs/decisions/0005-html-sandbox-und-desktop-tests.md` und in `docs/ARCHITECTURE.md`, Sicherheitsgrenzen.
Stufe 1 übernimmt zunächst die strenge CSP aus diesem Dokument; Stufe 3 lockert sie gezielt für die Mail-Anzeige.
