# 0021 — KI über MCP: Integration in Kern, Speicher, Shell und Installer

- **Datum:** 23.11.2026
- **Stufe:** 12 (Kern und Shell, ohne Oberfläche)
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-engine/src/mcp/` (`mod.rs`, `access.rs`, `log.rs`, `place.rs`, `probe.rs`),
  `crates/mail-engine/src/search/{mod,resolve}.rs` (Suche genau in gegebenen Konten),
  `crates/mail-engine/src/reading/mod.rs` (`content_for_model`), `crates/mail-engine/src/credentials.rs`,
  `crates/mail-store/migrations/0008_mcp.sql`, `crates/mail-store/src/mcp.rs`, `crates/mail-model/src/mcp.rs`,
  `crates/mail-mcp/src/{access,exec}.rs` (`DraftCreated::local_only`), `crates/mail-ai/src/prepare.rs`
  (`visible_text`), `app/src-tauri/src/commands/mcp.rs`, `scripts/build-{macos,windows}.sh`
- **Test:** `crates/mail-engine/tests/mcp.rs` (echtes Hilfsprogramm über den echten Socket gegen den
  Testserver), `crates/mail-engine/src/mcp/tests.rs`, `crates/mail-store/src/mcp.rs`

Setzt die Folgen aus 0015 um. Hier steht, wo die Umsetzung genauer festlegt oder abweicht.

## `MailAccess` über `AppCore`

- **Schwacher Verweis.** `CoreAccess` hält den Kern nur über `WeakCore`; während eines Aufrufs
  wird er kurz hochgestuft. Kein Hintergrund-Task der MCP-Anbindung hält den Kern oder den Store
  dauerhaft (Befund: Ein Protokoll-Task mit eigenem `Store` hielt die Datenbank nach dem Ende des
  Kerns offen, der WAL-Checkpoint fiel aus; `command_error_is_typed` in der Shell scheiterte daran).
- **Genau die freigegebenen Konten.** Die Suche bekommt einen eigenen Bereich
  (`resolve::Base::Exactly`): die übergebenen Konten oder ein Postfach davon. Die in der App
  ausgeblendeten Konten spielen keine Rolle (Ansichtseinstellung, keine Freigabe). `account:` lehnt
  schon das Hilfsprogramm ab; im Kern könnte es den Bereich nur verengen.
- **Papierkorb und Spam** fehlen ohne Postfach und ohne positives `in:` (als ausgeschlossene
  Postfächer im Filter). `list_messages` (ungelesen/markiert) läuft über je einen `UNION ALL`-Zweig
  je Postfach außer Papierkorb und Spam (Treiber `loc_unseen`); über 100 Postfächer wird stattdessen
  ausgeschlossen (SQLite erlaubt 500 Glieder je Verbund).
- **Lesen ohne Spuren.** Body aus dem lokalen Speicher, sonst sofort über die Nur-Lese-Leseverbindung
  des Kontos (`BODY.PEEK`, wie der Lesebereich; der Kern markiert beim Öffnen nie, das tut nur die
  Oberfläche). Offline oder bei einem Abruffehler bekommt der Client die Kopfdaten mit
  `body_format: unavailable` statt eines Fehlers. Anhänge: die Liste des Lesebereichs (vom HTML
  gezeigte eingebettete Bilder fehlen), nur Name, Typ, Größe.
- **Textquelle: was der Lesebereich zeigt.** Beide Wege nehmen den sichtbaren Text des HTML-Teils
  (`mail_content::html_to_visible_text`, mit Zahl des weggelassenen versteckten Texts), nur ohne
  HTML den Text-Teil. MCP tat das schon (`mail_mcp::exec::body_text`); die eingebaute KI
  (`mail_ai::MailInput::from_parsed`) nahm über `ParsedMessage::visible_text()` zuerst den
  Text-Teil und nimmt jetzt `mail_ai::visible_text`. Grund: Eine `text/plain`-Alternative kann
  anderes sagen als das, was der Nutzer sieht, und ist ein Versteck für Anweisungen.

## Entwurf anlegen

- Über die Entwürfe aus Stufe 5: neuer Entwurf mit der **Standard-Identität** des Kontos; mit
  `in_reply_to` ein Entwurf „Antworten“ (Identität, an die die Mail ging, Empfänger, wenn `to`
  leer ist, Betreff, Zitat, In-Reply-To/References). Abweichung von „immer Standard-Identität“:
  Eine Antwort von einer anderen Adresse als der angeschriebenen wäre falsch. Zitatsprache nach der
  App-Sprache (System → Englisch). Format Text. Danach sofort `closeDraft`: die Outbox legt ihn per
  `append` in den Entwürfe-Ordner. Der Kern prüft „lesen und Entwürfe“ ein zweites Mal.
- **Konten ohne Schreibzugriff** (0015, offen): „lesen und Entwürfe“ bleibt wählbar; der Entwurf
  bleibt dann nur in der App auf diesem Gerät (`DraftServerState::LocalOnly`), und das
  Werkzeugergebnis sagt das ausdrücklich (`DraftCreated::local_only`, neues Feld in `mail-mcp`).
  Beobachtet: Wird der Schreibzugriff später eingeschaltet, lädt die App den lokalen Entwurf erst
  hoch, wenn er wieder gespeichert oder geschlossen wird (Verhalten aus Stufe 5).

## Speicher (Migration 0008, nur additiv, Klasse Gerät)

`mcp_state` (eine Zeile: Hauptschalter, Windows-Pipe-Name), `mcp_account` (Freigabe je Konto,
`ON DELETE CASCADE`; keine Zeile = unsichtbar), `mcp_client` (ID, Name, Verweis auf den
Schlüsselbund, angelegt, widerrufen, zuletzt benutzt), `mcp_access` (Zugriffsprotokoll: Zeitpunkt,
Client, Werkzeug, Anzahl Mails, Ergebnis, Ablehnungsgrund). Gerät, weil Schlüssel, Pfad zum
Hilfsprogramm und Endpunkt nur auf diesem Gerät gelten; der Einstellungs-Sync kennt die Tabellen
nicht.

- **Schlüssel.** Der ganze Token liegt im Schlüsselbund unter `mcp-client:<id>` (Dienst = Bundle-ID,
  wie die Passwörter, `CredentialRef::for_mcp_client`), die Datenbank hat nur den Verweis. Gelesen
  wird er erst, wenn MCP an ist (solange es aus ist, kein Schlüsselbund-Zugriff beim Start). Ein
  neuer Client legt zuerst den Schlüssel ab, dann die Zeile; scheitert die Zeile, wird der Schlüssel
  wieder entfernt. Der Token wird genau einmal ausgegeben (`createMcpClient`); ein verlorener Token
  heißt: neuer Client.
- **Widerruf.** Die Zeile bleibt mit `revoked_at` (Name für das Protokoll), der Schlüssel wird
  gelöscht, die Einstellungen ohne den Client gehen sofort an den Host (offene Verbindungen zu).
- **Protokoll.** Der Host meldet synchron; ein Task schreibt in Stapeln (höchstens 200 je
  Transaktion) und meldet `mcpAccess`. Aufbewahrung 30 Tage und höchstens 10.000 Einträge, geprüft
  bei jedem Schreiben (Datum über Index, Anzahl als ID-Bereich, weil IDs je Eintrag um eins
  wachsen). Abgelehnte Verbindungen unbekannter oder widerrufener Clients stehen ohne ID und Namen
  im Protokoll: Die ID im Handshake ist nur eine Behauptung.

## Host und Endpunkt

- Der Host entsteht beim Start des Kerns; sein Task startet beim ersten Einschalten (beim Start,
  wenn gespeichert „an“) und läuft bis `shutdown`. Socket bzw. Pipe gibt es nur, solange MCP an ist
  (Host-Verhalten aus 0015). `shutdown` beendet MCP zuerst, damit kein Aufruf in das Herunterfahren
  fällt; ohne geordnetes Ende bricht `Drop` die Tasks ab.
- **macOS, Pfadlänge.** Standard `<Datenverzeichnis>/mcp/mail-mcp.sock` (0015). Ist der Pfad länger
  als 103 Byte (langer Benutzername), liegt der Socket unter
  `$TMPDIR/mail-mcp-<16 Hex von SHA-256(Datenverzeichnis)>/mail-mcp.sock`. `$TMPDIR` ist unter macOS
  `/var/folders/…/T/` (rund 50 Byte, gehört dem Benutzer, 0700, für alle Prozesse des Benutzers
  gleich und über Neustarts stabil), also bleiben die Client-Konfigurationen gültig. Das
  Cache-Verzeichnis hilft nicht: Es enthält ebenfalls den Benutzernamen und ist nur 13 Byte kürzer.
  Der Hash trennt zwei Datenverzeichnisse (Tests, zweiter Build). Das Verzeichnis des Sockets wird
  weiter 0700 angelegt, ein fremdes abgelehnt (`mail_mcp::Endpoint`). Grenze: Leert macOS `$TMPDIR`
  bei laufender App, ist der Socket bis zum nächsten Einschalten oder Start weg.
- **Windows.** Ein zufälliger Pipe-Name wird beim ersten Start erzeugt und in `mcp_state.endpoint`
  gespeichert (die Konfigurationen enthalten ihn).
- **Hilfsprogramm.** Neben der ausführbaren Datei der App (`Contents/MacOS/mail-mcp`,
  `mail-mcp.exe` neben der App, im Entwicklungsmodus `target/debug/mail-mcp` nach
  `cargo build -p mail-mcp`); `AppCore::set_mcp_helper` für Tests. Läuft die App vom Disk-Image
  oder aus der App-Translocation, meldet der Status `unstableLocation`, weil der Pfad in den
  Konfigurationen dann verschwindet.

## Commands und Ereignisse

`getMcpSettings`, `setMcpEnabled` (aus = Not-Aus), `setMcpAccountAccess`, `createMcpClient`
(Token und fertige Konfigurationen aus `mail_mcp::config` für Claude Desktop, Claude Code, Cursor,
LM Studio), `revokeMcpClient`, `listMcpClients`, `getMcpSetup` (Konfigurationen mit Platzhalter),
`listMcpAccess` (seitenweise), `getMcpStatus`, `testMcpConnection`; Ereignisse `mcpAccess` und
`mcpStatusChanged`. Typen und Regeln für die Oberfläche im IPC-Vertrag der Stufe.

**Verbindung testen** startet das Hilfsprogramm wie ein KI-Client, mit Endpunkt und dem Token eines
temporären Clients, der nur für die Dauer des Tests in den veröffentlichten Einstellungen steht
(nie in Datenbank oder Schlüsselbund, nicht im Protokoll: die App spricht mit sich selbst), und
führt `initialize`, `tools/call list_accounts` (erzwingt Verbindung und Handshake) und `tools/list`
aus. Ergebnis: ok, oder `disabled`, `helper_missing`, `helper_failed`, `endpoint_failed`,
`unreachable`, `refused` (mit Grund aus dem Host), `timeout` (20 s).

## Hilfsprogramm im Installer

- **`bundle.externalBin` nur in den Build-Skripten** (über `--config`), nicht in
  `tauri.conf.json`: `tauri-build` verlangt die Datei `…/mail-mcp-<Zieltriple>` sonst bei jedem
  `cargo build` der Shell, also auch in `scripts/check.sh` und in jeder anderen Sitzung. Die
  Skripte bauen zuerst `mail-mcp` (macOS `cargo build --release`, Windows `cargo xwin build
  --release --target x86_64-pc-windows-msvc`), kopieren es nach `target/sidecar/mail-mcp-<Triple>`
  und übergeben diesen Pfad. Tauri legt es als `Contents/MacOS/mail-mcp` bzw. neben die App.
- **Prüfungen.** `build-macos.sh`: Hilfsprogramm in `Contents/MacOS`, ad hoc signiert,
  `codesign --verify --strict`, antwortet auf `--version`, liegt auch im Disk-Image.
  `build-windows.sh`: `mail-mcp.exe` neben der App im Build und im NSIS-Skript des Installers.
- Signierte und notarisierte Builds (Stufe 15) müssen das Hilfsprogramm mit derselben Identität
  signieren.

## Offen

- Bindings (`app/src/generated/bindings.ts`) neu erzeugen; Oberfläche (Einstellungen „KI über MCP“,
  Ansicht „Zugriffe“, Einrichtungshilfe).
- `mail_content::ParsedMessage::visible_text()` bevorzugt weiter den Text-Teil und nennt sich „Text
  für ein Modell“; kein Produktcode nutzt sie mehr. Angleichen oder umbenennen (nicht im
  Schreibbereich dieser Sitzung).
- `listChanged` (0015): ein Client sieht `create_draft` nach einer neuen Freigabe erst beim nächsten
  `tools/list`.
- Windows (Pipe, Pfad des Hilfsprogramms im installierten Programm): nur gebaut, ungeprüft.
- Handprüfung mit einem echten Client (Claude Code): Suche, Thread lesen, Entwurf anlegen.
