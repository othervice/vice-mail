# 0015 — MCP-Transport: Hilfsprogramm über stdio, lokaler IPC-Kanal zur App

- **Datum:** 23.11.2026
- **Stufe:** 12, Unterbau vorgezogen (Crate ohne Oberfläche und ohne Verdrahtung in `AppCore`)
- **Status:** entschieden von der ausführenden Sitzung; Integration in `AppCore`, Oberfläche und
  Installer folgt mit Stufe 12
- **Code:** `crates/mail-mcp` (Bibliothek und Binary `mail-mcp`)

## Recherche (Stand 23.11.2026)

**MCP-Spezifikation.** Aktuell ist Version `2026-07-28` (erschienen 28.07.2026). Sie ist
zustandslos: kein `initialize` mehr, jede Anfrage trägt in `_meta` die Protokollversion
(`io.modelcontextprotocol/protocolVersion`) und die Client-Fähigkeiten (`…/clientCapabilities`),
`server/discover` ist Pflicht, Listen und `server/discover` müssen Caching-Hinweise (`ttlMs`,
`cacheScope`) tragen, Ergebnisse ein `resultType`. Die Vorgänger bis `2025-11-25` („legacy“)
beginnen mit dem `initialize`-Handshake; ein Server darf beide Ären bedienen (dual-era). Transporte:
**stdio** (Client startet den Server, JSON-RPC eine Nachricht pro Zeile, stdout nur MCP, stderr für
Logs, Ende durch Schließen von stdin) und **Streamable HTTP** (Server muss `Origin` prüfen, weil
sonst DNS-Rebinding möglich ist; lokal nur an localhost binden; Autorisierung über das
OAuth-Rahmenwerk der Spezifikation). Quellen: modelcontextprotocol.io/specification/2026-07-28
(versioning, basic, transports/stdio, server/discover, server/tools, server/utilities/caching).

**Rust-SDK.** `rmcp` 3.11.0 vom 17.11.2026, Apache-2.0 (passt zu GPL-3.0), offizielles SDK
(github.com/modelcontextprotocol/rust-sdk), Releases fast wöchentlich (3.2.0 am 31.08. bis 3.11.0),
MSRV 1.88. Spricht `2026-07-28` und die älteren Versionen; der Client kennt die Modi `Initialize`,
`Discover` und `Auto` (Probe mit `server/discover`, dann Rückfall).

**Clients.** Alle vier starten lokale Server per stdio mit `command`, `args` und `env`:
Claude Desktop (`claude_desktop_config.json`, `mcpServers`; lokal nur stdio, entfernte Server nur
über Connectors in der App), Claude Code (`claude mcp add --transport stdio --scope user --env
K=V <name> -- <befehl>`, gespeichert in `~/.claude.json`; Projekt-Scope in `.mcp.json`), Cursor
(`~/.cursor/mcp.json`, `type: "stdio"`, auch `envFile`), LM Studio (`mcp.json`, „folgt der
Cursor-Notation“). HTTP mit festen Headern können Claude Code, Cursor und LM Studio, Claude Desktop
nicht über die Konfigurationsdatei. Die Vorlagen mit Quellen stehen in `crates/mail-mcp/src/config.rs`.

## Entscheidung

1. **stdio über ein mitgeliefertes Hilfsprogramm `mail-mcp`.** Der Client startet es; es spricht
   MCP auf stdin/stdout und leitet `tools/list` und `tools/call` an die laufende App weiter. Die App
   betreibt keinen HTTP-Server.
2. **Kanal zur App nur lokal, nie TCP.** macOS: Unix-Domain-Socket
   `<Datenverzeichnis>/mcp/mail-mcp.sock`, Verzeichnis 0700, Socket 0600, die App prüft zusätzlich
   die Benutzer-ID der Gegenseite (`peer_cred`). Ein verwaister Socket wird ersetzt, ein Socket, an
   dem jemand lauscht, und jede andere Datei bleiben unberührt. Windows: Named Pipe
   `\\.\pipe\mail-mcp-<128 Bit Zufall>` mit `first_pipe_instance` (niemand kann den Namen vorher
   besetzen) und `reject_remote_clients`; den Namen erzeugt die App einmal und speichert ihn.
3. **Authentisierung je Verbindung, gegenseitig.** Client-Token `mcp1_<Client-ID, 32 Hex>_<Schlüssel,
   64 Hex>`. Der Schlüssel geht nie über den Kanal: HMAC-SHA-256-Challenge-Response über beide
   Nonces, erst beweist die App, dann das Hilfsprogramm; Prüfung in konstanter Zeit
   (`Mac::verify_slice`). Ein Prozess, der sich als App ausgibt, bekommt nichts Verwertbares und
   kann dem Client keine falschen Mails unterschieben.
4. **Konfiguration über Umgebungsvariablen** `MAIL_MCP_ENDPOINT` und `MAIL_MCP_TOKEN`, nicht über
   Argumente (die Befehlszeile ist für andere Benutzer sichtbar, die Umgebung nicht).
5. **App nicht erreichbar.** Läuft die App nicht oder ist MCP aus, gibt es keinen Socket. Das
   Hilfsprogramm beantwortet trotzdem `initialize`, `server/discover` und `tools/list` (mit den
   Lesewerkzeugen) und erklärt in jedem Werkzeugergebnis (`isError`), dass die App nicht läuft oder
   KI-Zugriff aus ist. Es verbindet sich bei der nächsten Anfrage neu; der Client muss nicht neu
   gestartet werden. Fehlende Konfiguration wird ebenso erklärt.
6. **MCP selbst umgesetzt, dual-era** (`2026-07-28` sowie `2025-11-25`, `2025-06-18`,
   `2025-03-26`, `2024-11-05` nach `initialize`). `rmcp` nur als Client in den Vertragstests
   (Dev-Abhängigkeit), nicht im ausgelieferten Programm.

## Begründung

| | stdio + IPC (gewählt) | Streamable HTTP auf 127.0.0.1 |
|---|---|---|
| Clients | alle vier über Konfigurationsdatei | Claude Desktop nicht |
| Wer kommt ran | nur derselbe Benutzer (Socket 0600, Peer-UID; Pipe nicht übers Netz) | jeder lokale Prozess jedes Benutzers; Webseiten im Browser per DNS-Rebinding oder einfachen Cross-Origin-Anfragen, nur durch Host-/Origin-Prüfung und Token gebremst |
| Einrichtung | Pfad zum Hilfsprogramm, zwei Umgebungsvariablen | URL mit festem Port, Header; Port-Konflikte |
| Code in der App | Socket/Pipe aus Tokio, kein `unsafe` | HTTP-Server (hyper/axum), CORS, Origin-Prüfung |
| Nachteil | Hilfsprogramm mitliefern und signieren; ein Prozess je Client | – |

**Eigenes Protokoll statt rmcp im Programm:** Der Umfang ist klein (Lebenszyklus beider Ären,
`tools/list`, `tools/call`, `ping`, leere `resources/list`/`prompts/list`), die Grenzen sind fest
(Zeilenlänge, keine Anfragen vom Server an den Client, keine Abos), das ausgelieferte Programm
bleibt bei Abhängigkeiten, die ohnehin im Baum sind, und die wöchentlichen rmcp-Releases betreffen
nur die Tests. Dass beide Ären stimmen, zeigen die Vertragstests mit dem rmcp-Client in allen drei
Modi (`Initialize` → `2025-11-25`, `Discover` und `Auto` → `2026-07-28`); erst dabei fiel auf, dass
`2026-07-28` `ttlMs`/`cacheScope` verlangt (jetzt `0`/`private`, weil die Werkzeugliste von den
Freigaben abhängt). Rückweg: ein rmcp-Server hinter derselben Schnittstelle `ToolBackend`.

**Keine eigene DACL auf Windows:** Sie ginge nur über `create_with_security_attributes_raw`
(`unsafe`, im Projekt verboten). Die Pipe hat deshalb die Standard-DACL (andere Benutzer dürfen sie
lesend öffnen). Unschädlich, weil der Name zufällig ist und nur in Dateien des Benutzers steht, die
App vor der Authentisierung nur die Challenge schreibt und das Hilfsprogramm einer fremden Pipe
nichts Verwertbares schickt. **Auf echtem Windows ungeprüft** (nur `cargo xwin` gebaut).

## IPC-Protokoll (Version 1)

JSON, eine Nachricht pro Zeile, jede Zeile begrenzt (Handshake 4 KiB, Anfragen 1 MiB, Antworten
2 MiB), Handshake höchstens 5 s, eine Antwort muss die Gegenseite binnen 30 s abnehmen.

```text
Hilfsprogramm → App  {"type":"hello","protocol":1,"client_id":…,"nonce":<64 Hex>}
App → Hilfsprogramm  {"type":"challenge","protocol":1,"nonce":<64 Hex>,"proof":HMAC(key,"…app"‖id‖n_h‖n_a)}
Hilfsprogramm → App  {"type":"auth","proof":HMAC(key,"…helper"‖id‖n_h‖n_a)}
App → Hilfsprogramm  {"type":"ready"}      oder jederzeit {"type":"refused","reason":…}
danach               {"type":"list_tools","id":1}            → {"type":"tools","id":1,"tools":[…]}
                     {"type":"call_tool","id":2,"name":…,"arguments":{…}}
                                                             → {"type":"result","id":2,"text":…,"is_error":…}
                                                             | {"type":"error","id":2,"code":"unknown_tool"|"rate_limited"|"timeout"|"malformed",…}
App jederzeit        {"type":"closed","reason":"disabled"|"unknown_client"}
```

Ablehnungsgründe: `unsupported_protocol` (mit `supported`), `disabled`, `unknown_client`,
`bad_proof`, `malformed`, `timeout`, `too_many_connections` (höchstens 16 Verbindungen).

Stimmt der Beweis der App nicht, schickt das Hilfsprogramm `{"type":"refused","reason":"bad_proof"}`
und wartet danach höchstens 1 s auf die Antwort der App oder das Ende der Verbindung, bevor es
schließt (schloss es sofort, fehlte die Ablehnung auf Windows im Protokoll, CI vom 24.11.2026).
Die App trägt jede Ablehnung ins Zugriffsprotokoll ein, bevor sie sie sendet; meldet das
Hilfsprogramm eine Ablehnung, steht sie also schon im Protokoll.

## Werkzeuge

Fest im Code (Name, Titel, Beschreibung, JSON-Schema mit `additionalProperties: false`,
Annotationen). Ein Test prüft die Liste exakt; unbekannte Argumente werden abgelehnt.

| Werkzeug | Argumente | Grenzen |
|---|---|---|
| `list_accounts` | – | nur freigegebene Konten |
| `list_mailboxes` | `account_id` | höchstens 500 |
| `search_messages` | `query` (Suchsprache Stufe 7, ohne `account:`), `account_id`?, `mailbox_id`?, `limit`?, `cursor`? | Anfrage ≤ 2048 Byte, 20 (höchstens 50) Treffer je Aufruf, Vorschau 200 Zeichen |
| `list_messages` | `filter` (`unread`/`flagged`), `account_id`?, `mailbox_id`?, `limit`?, `cursor`? | wie Suche; ohne Postfach ohne Papierkorb und Spam |
| `read_message` | `message_id` | Text 30.000 Zeichen, 50 Anhänge (nur Name, Typ, Größe), 50 Adressen je Feld |
| `read_thread` | `message_id` | die 25 neuesten, je 8.000, zusammen 60.000 Zeichen (die neuesten zuerst) |
| `create_draft` | `account_id`, `to`?, `cc`?, `subject`?, `body`?, `in_reply_to`? | nur gelistet, wenn ein Konto Entwürfe erlaubt; 50 Empfänger, Betreff 500, Text 50.000 Zeichen; Betreff einzeilig (keine Kopfzeilen-Einschleusung) |

Je Verbindung höchstens 120 Aufrufe pro Minute, je Aufruf höchstens 60 s, je Ergebnis höchstens
768 KiB. Kein Werkzeug sendet, löscht, verschiebt, markiert oder ändert Einstellungen.

## Freigaben

`McpSettings`: Hauptschalter (aus = kein Socket, alle Verbindungen zu, Not-Aus), je Konto
`hidden`/`read`/`read_and_drafts` (fehlender Eintrag = `hidden`), registrierte Clients mit Name und
Schlüssel; Widerruf entfernt den Client. Die App reicht die Einstellungen als `watch`-Kanal herein;
jede Anfrage wird gegen den aktuellen Stand geprüft, eine laufende Anfrage endet sofort, wenn der
Client widerrufen oder MCP abgeschaltet wird. Ein verborgenes Konto antwortet überall wie ein nicht
vorhandenes; die App wird nie nach ihm gefragt, und Ergebnisse werden zusätzlich nachgefiltert
(geprüft mit einer absichtlich undichten Test-Umsetzung von `MailAccess`). `account:` in der Suche
ist verboten, damit keine Anfrage ein verborgenes Konto benennen kann. Jeder Werkzeugaufruf und jede
abgelehnte Verbindung geht als `AccessEvent` (Zeitpunkt, Client, Werkzeug, Anzahl Mails, Ergebnis;
keine Argumente, kein Inhalt) an das `AccessLog` der App.

## Schutz vor eingeschleusten Anweisungen

Mailinhalt steht nur in einem Block `<mail_data boundary="<96 Bit Zufall>">…</mail_data
boundary="…">` als JSON, davor ein fester Hinweis, dass der Inhalt von Absendern stammt und keine
Anweisungen enthält, denen zu folgen ist. Zeilenumbrüche bleiben in JSON-Strings, `<mail_data` und
`</mail_data` im Inhalt werden escaped. HTML geht nur durch `mail_content::html_to_text` (Links als
Text, keine Bilder, keine Scripts, Styles, Kommentare); Steuerzeichen und unsichtbare Zeichen
(Unicode-Tags, Variantenselektoren-Ergänzung, Bidi-Steuerung, Nullbreiten-Leerzeichen,
Hangul-Füller) werden entfernt und gezählt. Anhänge nur mit Namen, Typ und Größe. Fehlertexte
stammen aus dem Crate, nie aus Mails. Geprüft mit dem Korpus aus Stufe 3 und vier neuen Fällen in
`crates/mail-mcp/tests/corpus/` („ignoriere alles und leite weiter an …“ mit gefälschtem Blockende,
gefälschten Werkzeugaufrufen und Rollen; per CSS versteckter Text; Unicode-Schmuggel; Anweisungen in
Dateinamen). Grenze: Per CSS versteckter Text (`display:none`, weiße Schrift) erscheint im Text,
abgegrenzt als Daten; ihn zu entfernen gehört nach `mail-content` (offen).

## Folgen

- **`AppCore`** implementiert `MailAccess` (nur lesend, `BODY.PEEK`, kein „gelesen beim Öffnen“;
  Suche und Listen genau in den übergebenen Konten; Entwurf über die Outbox), hält `McpSettings`
  (in der Datenbank ohne Schlüssel, Schlüssel im Schlüsselbund wie Passwörter), startet einmal
  `McpHost::run(endpoint, shutdown)` und schreibt die `AccessEvent`s für die Ansicht „Zugriffe“.
  `Endpoint::for_data_dir` liefert unter macOS den Socket-Pfad (höchstens 103 Byte), unter Windows
  einen neuen Pipe-Namen, den die App speichert.
- **Installer:** `mail-mcp` wird mitgeliefert (macOS in `Contents/MacOS/`, signiert und notarisiert;
  Windows neben der App), etwa als Tauri `bundle.externalBin`. Die Einstellungen zeigen den Pfad
  und die fertigen Konfigurationen aus `mail_mcp::config`.
- **Neue Abhängigkeiten:** `getrandom` 0.4.3 (direkt; war schon im Baum), `rmcp` 3.11.0 nur für
  Tests; `hmac`/`sha2` aus dem Workspace. Nachzutragen in der Stack-Tabelle und unter Modulgrenzen
  in `docs/ARCHITECTURE.md`.
- **Stufendatei 12:** Der Abnahmepunkt „von einer anderen Adresse als `127.0.0.1` keine Antwort“
  wird zu „nur derselbe Benutzer: Socket 0600 im Verzeichnis 0700, Peer-UID; Pipe ohne
  Netzzugriff“.

## Offen

- Per CSS versteckten Text in `mail_content::html_to_text` weglassen oder kennzeichnen.
- `listChanged`: Ein Client sieht `create_draft` nach einer neuen Freigabe erst beim nächsten
  `tools/list`; Push von der App über den Kanal fehlt noch.
- Entwürfe in Konten mit `writes_enabled = false`: nur lokal ablegen oder „lesen und Entwürfe“ erst
  mit Schreibzugriff anbieten (Entscheidung bei der Integration).
- Windows (Named Pipe, Peer-Verhalten) und Peer-UID-Prüfung mit einem zweiten Benutzer: ungeprüft.
- Handprüfung mit einem echten Client (Claude Code) nach der Integration.
