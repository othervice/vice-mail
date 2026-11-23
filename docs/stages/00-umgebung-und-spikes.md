# Stufe 00 — Umgebung und Risiko-Spikes

**Ziel:** Die Werkzeuge sind installiert, ein lokaler Testserver läuft, und die vier riskantesten
Annahmen der Architektur sind mit Wegwerf-Code geprüft. Es entsteht kein Produktcode.

**Ausgangslage (22.11.2026):** Der Ordner ist leer und kein Git-Repo. Vorhanden sind Node 24, pnpm,
Homebrew und Xcode. Es fehlen Rust, Docker und Java.

## Aufgaben

- [x] **Repo.** `git init`, privates Repo `vice-mail` in Gitea anlegen (API `POST /api/v1/user/repos`), `origin` setzen, ersten Stand pushen.
- [x] **Werkzeuge.** Rust über rustup (stable, mindestens 1.88), Ziel `x86_64-pc-windows-msvc`, `cargo-xwin`, `brew install llvm nsis`. Alles ohne `sudo`. Die genauen Befehle in `docs/SETUP.md` festhalten, sodass sich ein zweiter Rechner damit einrichten lässt.
- [x] **Testserver.** Unter `testmail/` einen lokalen IMAP/SMTP-Server einrichten. Erster Kandidat ist Stalwart (ein Binary, IMAP und SMTP in einem Prozess, auch für Windows erhältlich), zweiter Dovecot über Homebrew. Anforderungen:
  - `scripts/testmail.sh start|stop|seed|reset`, nur auf `localhost`, Ports über 1024, Daten in `testmail/.data/`
  - zwei Testkonten; eine per SMTP gesendete Mail landet im Posteingang des Empfängers
  - `seed` erzeugt eine wählbare Zahl Mails: mehrteilig, mit Anhängen, mit fremden Zeichensätzen, HTML mit externen Bildern, ein langer Thread, einige absichtlich kaputte
  - unterstützt IDLE, CONDSTORE, MOVE, SPECIAL-USE und eigene Schlagwörter
- [x] **Spike A, IMAP-Bibliothek.** `async-imap` 0.12 mit Tokio und rustls gegen den Testserver: TLS und STARTTLS, Login, Capabilities, Ordnerliste mit SPECIAL-USE, EXAMINE und SELECT, UID FETCH von Envelope, Flags, Größe, Bodystructure und ausgewählten Kopfzeilen, CONDSTORE (`CHANGEDSINCE`), IDLE mit Zeitlimit und Abbruch, UID MOVE und der Rückfall COPY + STORE + UID EXPUNGE, APPEND, eigene Schlagwörter, Wiederverbinden nach gekappter Verbindung. Zeit für die Kopfdaten von 10.000 Mails messen. Lücken notieren. Nur bei einem echten Hindernis `imap-next` / `imap-client` prüfen.
- [x] **Spike B, SQLite.** `rusqlite` 0.43 (bundled) mit WAL und FTS5. Schemaentwurf für Nachrichten und Index. 500.000 erzeugte Mails über fünf Konten einfügen und messen: eine Listenseite (100 Zeilen, Keyset-Paginierung) für ein Postfach und für den vereinten Posteingang, erste Suchtreffer, Einfügerate, Dateigröße. Ziele: Listenseite unter 20 ms, Suche unter 200 ms. Tokenizer wählen (unicode61 oder trigram), Indizes festlegen.
- [x] **Spike C, Tauri und Windows.** Kleinste Tauri-2-App mit React und einem Command, das eine typisierte Struktur zurückgibt. `ts-rs` gegen `tauri-specta` 2.0-rc vergleichen und eines wählen. macOS-App bauen. Windows-Installer per Cross-Build erzeugen (`pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`). Bauzeit und Größe notieren.
- [x] **Spike D, HTML-Sandbox und Desktop-Tests.** In der Spike-App feindliches HTML in einem `iframe` mit `sandbox` ohne `allow-scripts` anzeigen. Nachweisen: kein Script läuft (auch nicht über Ereignis-Attribute, `javascript:`-Links oder SVG), blockierte externe Bilder lösen keine Verbindung aus (lokaler Zähl-Server), Links navigieren die App nicht, die Tauri-Schnittstelle ist nicht erreichbar. Klären, wie Link-Klicks abgefangen werden: `sandbox="allow-same-origin"` ohne Scripts mit Klickbehandlung im Elternfenster, oder ohne same-origin mit Navigationsabfang in Rust. Den Nachweis mit WebdriverIO und `@wdio/tauri-service` (eingebetteter WebDriver) automatisieren; das prüft zugleich, ob Desktop-Tests auf macOS so funktionieren.
- [x] **Festhalten.** Je Spike eine Entscheidung unter `docs/decisions/` mit Messwerten. `docs/ARCHITECTURE.md` (Stack-Tabelle, Regeln) und betroffene Stufendateien an die Ergebnisse anpassen.

## Abnahme

- [x] `docs/SETUP.md` vorhanden; `rustc`, `cargo`, `cargo xwin --version` und `makensis -VERSION` laufen
- [x] `scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000 && scripts/testmail.sh stop` läuft zweimal hintereinander fehlerfrei
- [x] vier Entscheidungsdateien mit gemessenen Zahlen bzw. Nachweisen
- [x] der Windows-Installer liegt unter `spikes/…/bundle/nsis/` und sein Pfad steht im Ergebnis
- [x] alles ist nach Gitea gepusht

## Nicht in dieser Stufe

- Workspace, Crates und App-Struktur des Produkts (Stufe 1)
- irgendetwas mit echten Postfächern

## Der Mensch prüft danach

- Installer auf dem Windows-Rechner starten: öffnet sich das Fenster der Spike-App?
- Lizenz und Bundle-ID festlegen (siehe `docs/SPEC.md`).

## Ergebnis

Ausgeführt am 22.11.2026 von einer Hauptsitzung mit Subagenten (Testserver, Spikes A bis D).
Alle Zahlen stammen vom Mac mini M4 (16 GB), meist unter Last paralleler Builds.

**Erledigt und geprüft**
- Repo: privates Gitea-Repo `othervice/vice-mail`, `origin` gesetzt, gepusht.
- Werkzeuge (Befehle in `docs/SETUP.md`): `rustc 1.100.0`, `cargo 1.100.0`, `cargo-xwin 0.24.1`,
  `cargo-deny 0.22.0`, LLVM 23.1.6, `makensis v3.13`. Zusätzlich ein echter NSIS-Kompiliertest,
  weil `-VERSION` allein den Locale-Absturz nicht zeigt.
- Testserver: Stalwart 1.1.1 (`docs/decisions/0001-testserver.md`). Die Kette
  `reset && start && seed 1000 && stop` lief in der Hauptsitzung zweimal hintereinander mit Exit 0;
  `scripts/testmail.sh check` besteht 32 von 32 Prüfungen (IDLE, CONDSTORE, MOVE, SPECIAL-USE,
  eigene Schlagwörter, TLS, AUTH-Pflicht, keine Weiterleitung nach außen).
- Spike A (`0002-imap-bibliothek.md`): async-imap 0.12.0 bleibt, mit eigener schmaler Befehlsschicht.
  `spikes/imap/run-all.sh` in der Hauptsitzung: 59 von 59 Prüfungen, Herunterfahren 3 von 3,
  kein `openssl-sys` im Baum. Kopfdaten von 9.959 Mails in 0,044 s (Fenster 500, TLS).
- Spike B (`0003-sqlite-schema-und-suche.md`): 500.000 Mails über fünf Konten; Listenseite 0,04 ms,
  vereinter Posteingang 0,14 ms, erste Suchtreffer 0,08 bis 0,19 ms, Präfix 12 ms; Tokenizer
  `unicode61`. Die Messung lief nur im Subagenten (Datenbanken danach gelöscht); die Rohdaten liegen
  unter `spikes/sqlite/results/`.
- Spike C (`0004-tauri-ipc-typen-und-windows-build.md`): tauri-specta gewählt. macOS-Build kalt 71 s,
  `.app` 5,2 MB, `.dmg` 2,2 MB; Windows-Cross-Build kalt 74 s, Installer 1,9 MB. Selbsttest der
  Release-App in der Hauptsitzung: 13 von 13 Prüfungen.
- Spike D (`0005-html-sandbox-und-desktop-tests.md`): 53 feindliche Vektoren in drei Varianten,
  kein Script, keine Verbindung zum Zähl-Server, keine Navigation, kein Zugriff auf Tauri.
  Link-Abfang geht unter WKWebView nur über den Navigations-Hook in Rust. WebdriverIO mit
  `@wdio/tauri-service` funktioniert auf macOS. `pnpm --dir spikes/tauri-app test:e2e` in der
  Hauptsitzung: Exit 0.
- Windows-Installer: `spikes/tauri-app/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Vice Mail Spike_0.1.0_x64-setup.exe`
  (SHA-256 `b9c59323…c81b9`), hochgeladen als Vorab-Release
  `spike-c-0.1.0` im privaten Gitea
- `docs/ARCHITECTURE.md` (Stack, Schema und Abfragen, Brücke, Verbindungen, Sicherheitsgrenzen,
  Entscheidungen) und die Stufen 01 bis 07 sind an die Ergebnisse angepasst.

**Stolpersteine auf diesem Rechner** (in `docs/SETUP.md`)
- `~/.local/bin/cc` ist das Sitzungswerkzeug und verdeckt den C-Compiler; `~/.cargo/config.toml`
  legt Linker und CC auf `/usr/bin/clang` fest.
- `makensis` stürzt ohne UTF-8-Locale ab; der Windows-Build setzt `LANG=en_US.UTF-8`.
- DMG-Bau über SSH/tmux braucht `CI=true`.

**Abweichungen und Annahmen**
- Beim `reset` des Testservers ist dessen Admin-Port 18080 etwa eine Sekunde lang auf allen
  Schnittstellen erreichbar (Stalwart 1.1.1 ignoriert die dokumentierte Bind-Einstellung im
  Recovery Mode), geschützt durch ein Einmal-Zufallspasswort. Danach nur noch `127.0.0.1`.
- `ring` bekommt eine begründete Ausnahme von der 12-Monats-Regel (0002); Stufe 6 prüft `aws-lc-rs`.

**Ungeprüft**
- Alles unter Windows: Installer, Start, WebView2-Nachinstallation, Plattform-Verifier für TLS,
  Link-Abfang im Unter-Frame unter WebView2 (vermutlich eigener Handler nötig).
- Dovecot als zweiter Kandidat (nicht nötig, Stalwart erfüllt alles).
- DNS-Abfragen durch `dns-prefetch` in der Sandbox (lokal nicht messbar).

**Wartet auf**
- Mensch: den Installer aus dem Vorab-Release auf dem Windows-Rechner starten. Öffnet sich das
  Fenster der Spike-App und zeigt es die typisierte Nachricht aus dem Kern?
- Lizenz und Bundle-ID: bereits entschieden (siehe `docs/SPEC.md`).
