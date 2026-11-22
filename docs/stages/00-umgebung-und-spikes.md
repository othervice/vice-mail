# Stufe 00 — Umgebung und Risiko-Spikes

**Ziel:** Die Werkzeuge sind installiert, ein lokaler Testserver läuft, und die vier riskantesten
Annahmen der Architektur sind mit Wegwerf-Code geprüft. Es entsteht kein Produktcode.

**Ausgangslage (22.11.2026):** Der Ordner ist leer und kein Git-Repo. Vorhanden sind Node 24, pnpm,
Homebrew und Xcode. Es fehlen Rust, Docker und Java.

## Aufgaben

- [ ] **Repo.** `git init`, privates Repo `vice-mail` in Gitea anlegen (API `POST /api/v1/user/repos`), `origin` setzen, ersten Stand pushen.
- [ ] **Werkzeuge.** Rust über rustup (stable, mindestens 1.88), Ziel `x86_64-pc-windows-msvc`, `cargo-xwin`, `brew install llvm nsis`. Alles ohne `sudo`. Die genauen Befehle in `docs/SETUP.md` festhalten, sodass sich ein zweiter Rechner damit einrichten lässt.
- [ ] **Testserver.** Unter `testmail/` einen lokalen IMAP/SMTP-Server einrichten. Erster Kandidat ist Stalwart (ein Binary, IMAP und SMTP in einem Prozess, auch für Windows erhältlich), zweiter Dovecot über Homebrew. Anforderungen:
  - `scripts/testmail.sh start|stop|seed|reset`, nur auf `localhost`, Ports über 1024, Daten in `testmail/.data/`
  - zwei Testkonten; eine per SMTP gesendete Mail landet im Posteingang des Empfängers
  - `seed` erzeugt eine wählbare Zahl Mails: mehrteilig, mit Anhängen, mit fremden Zeichensätzen, HTML mit externen Bildern, ein langer Thread, einige absichtlich kaputte
  - unterstützt IDLE, CONDSTORE, MOVE, SPECIAL-USE und eigene Schlagwörter
- [ ] **Spike A, IMAP-Bibliothek.** `async-imap` 0.12 mit Tokio und rustls gegen den Testserver: TLS und STARTTLS, Login, Capabilities, Ordnerliste mit SPECIAL-USE, EXAMINE und SELECT, UID FETCH von Envelope, Flags, Größe, Bodystructure und ausgewählten Kopfzeilen, CONDSTORE (`CHANGEDSINCE`), IDLE mit Zeitlimit und Abbruch, UID MOVE und der Rückfall COPY + STORE + UID EXPUNGE, APPEND, eigene Schlagwörter, Wiederverbinden nach gekappter Verbindung. Zeit für die Kopfdaten von 10.000 Mails messen. Lücken notieren. Nur bei einem echten Hindernis `imap-next` / `imap-client` prüfen.
- [ ] **Spike B, SQLite.** `rusqlite` 0.43 (bundled) mit WAL und FTS5. Schemaentwurf für Nachrichten und Index. 500.000 erzeugte Mails über fünf Konten einfügen und messen: eine Listenseite (100 Zeilen, Keyset-Paginierung) für ein Postfach und für den vereinten Posteingang, erste Suchtreffer, Einfügerate, Dateigröße. Ziele: Listenseite unter 20 ms, Suche unter 200 ms. Tokenizer wählen (unicode61 oder trigram), Indizes festlegen.
- [ ] **Spike C, Tauri und Windows.** Kleinste Tauri-2-App mit React und einem Command, das eine typisierte Struktur zurückgibt. `ts-rs` gegen `tauri-specta` 2.0-rc vergleichen und eines wählen. macOS-App bauen. Windows-Installer per Cross-Build erzeugen (`pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`). Bauzeit und Größe notieren.
- [ ] **Spike D, HTML-Sandbox und Desktop-Tests.** In der Spike-App feindliches HTML in einem `iframe` mit `sandbox` ohne `allow-scripts` anzeigen. Nachweisen: kein Script läuft (auch nicht über Ereignis-Attribute, `javascript:`-Links oder SVG), blockierte externe Bilder lösen keine Verbindung aus (lokaler Zähl-Server), Links navigieren die App nicht, die Tauri-Schnittstelle ist nicht erreichbar. Klären, wie Link-Klicks abgefangen werden: `sandbox="allow-same-origin"` ohne Scripts mit Klickbehandlung im Elternfenster, oder ohne same-origin mit Navigationsabfang in Rust. Den Nachweis mit WebdriverIO und `@wdio/tauri-service` (eingebetteter WebDriver) automatisieren; das prüft zugleich, ob Desktop-Tests auf macOS so funktionieren.
- [ ] **Festhalten.** Je Spike eine Entscheidung unter `docs/decisions/` mit Messwerten. `docs/ARCHITECTURE.md` (Stack-Tabelle, Regeln) und betroffene Stufendateien an die Ergebnisse anpassen.

## Abnahme

- [ ] `docs/SETUP.md` vorhanden; `rustc`, `cargo`, `cargo xwin --version` und `makensis -VERSION` laufen
- [ ] `scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed 1000 && scripts/testmail.sh stop` läuft zweimal hintereinander fehlerfrei
- [ ] vier Entscheidungsdateien mit gemessenen Zahlen bzw. Nachweisen
- [ ] der Windows-Installer liegt unter `spikes/…/bundle/nsis/` und sein Pfad steht im Ergebnis
- [ ] alles ist nach Gitea gepusht

## Nicht in dieser Stufe

- Workspace, Crates und App-Struktur des Produkts (Stufe 1)
- irgendetwas mit echten Postfächern

## Der Mensch prüft danach

- Installer auf dem Windows-Rechner starten: öffnet sich das Fenster der Spike-App?
- Lizenz und Bundle-ID festlegen (siehe `docs/SPEC.md`).

## Ergebnis

_Von der ausführenden Sitzung auszufüllen: erledigt, Messwerte, ungeprüft, wartet auf._
