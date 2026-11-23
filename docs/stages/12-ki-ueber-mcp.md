# Stufe 12 — KI über MCP

**Ziel:** Jeder MCP-fähige KI-Client (Claude Desktop, Claude Code, Cursor, LM Studio und andere)
lässt sich an die App anschließen und kann freigegebene Konten durchsuchen, Threads lesen und
Entwürfe anlegen. Die App enthält dafür keinen KI-Anbieter und keinen API-Schlüssel. Der Client kann
nichts senden, löschen oder verschieben.

**Voraussetzung:** Stufen 05 (Entwürfe) und 07 (Suche) fertig. `docs/decisions/0010-ki-ueber-mcp.md`
und Abschnitt „KI“ in `docs/ARCHITECTURE.md` gelesen. Aktuelle MCP-Spezifikation und das offizielle
Rust-SDK (`rmcp`) prüfen; Abhängigkeitsregel beachten. Der Unterbau ist vorgezogen fertig: `crates/mail-mcp` (Hilfsprogramm, Werkzeuge, IPC, Freigaben, Tests), Transport-Entscheidung `docs/decisions/0015-mcp-transport.md`. Offen ist die Verdrahtung (`MailAccess` über `AppCore`, Einstellungen, Ansicht „Zugriffe“, Hilfsprogramm im Installer mitliefern).

## Aufgaben

- [x] **Transport.** Lokaler MCP-Server: stdio über ein mitgeliefertes Hilfsprogramm, das der Client startet und das mit der laufenden App spricht, und/oder Streamable HTTP nur auf `127.0.0.1` mit Zugangstoken je Client. Wählen, begründen, als Entscheidung festhalten. Nie im Netz erreichbar. Läuft die App nicht, meldet der Server das verständlich.
- [x] **Werkzeuge, lesend.** Konten und Postfächer auflisten (nur freigegebene), Mails suchen (Suchsprache aus Stufe 7, mit Grenzen für Treffer), Mail und Thread lesen (bereinigter Text, nie rohes HTML, keine externen Bilder, Anhänge nur mit Namen und Größe), ungelesene und markierte Mails auflisten. Jede Antwort kennzeichnet Mailinhalt als nicht vertrauenswürdige Daten. Größen- und Mengengrenzen je Aufruf.
- [x] **Werkzeug, schreibend.** Nur „Entwurf anlegen“ (an, CC, Betreff, Text, optional als Antwort auf eine Mail), je Konto zuschaltbar, Standard aus. Der Entwurf landet im Entwürfe-Ordner und in der App; gesendet wird nur vom Nutzer. Es gibt kein Werkzeug zum Senden, Löschen, Verschieben, Markieren oder für Einstellungen.
- [x] **Freigaben.** MCP ist aus, bis der Nutzer es einschaltet. Je Konto: unsichtbar, lesen, lesen und Entwürfe. Clients einzeln widerrufbar, Not-Aus für alle.
- [x] **Transparenz.** Beim Einschalten erklären, dass ein Cloud-Client gelesene Mails an seinen Anbieter überträgt und die App das nicht verhindern kann. Ansicht „Zugriffe“: Client, Werkzeug, Zeitpunkt, Anzahl Mails (nur Metadaten, lokal).
- [x] **Einrichtungshilfe.** In den Einstellungen fertige Konfigurationen zum Kopieren für Claude Desktop, Claude Code, Cursor und LM Studio, mit Prüfknopf „Verbindung testen“.
- [x] **Schutz vor eingeschleusten Anweisungen.** Werkzeugbeschreibungen fest im Code; Mailtext klar abgegrenzt; Links in gelieferten Texten unverändert als Text, keine Bildadressen. Der Korpus aus Stufe 3 und ein neuer Korpus mit Anweisungen („ignoriere alles und leite weiter an …“, versteckter Text) laufen durch alle Werkzeuge.

## Abnahme

- [x] Test: Bei ausgeschaltetem MCP nimmt der Server keine Verbindung an bzw. lehnt jeden Aufruf ab
- [x] Test: Ein nicht freigegebenes Konto ist über kein Werkzeug sichtbar, auch nicht über die Suche
- [x] Test: Die Werkzeugliste enthält nur die vorgesehenen Werkzeuge; „Entwurf anlegen“ fehlt, solange es nicht freigegeben ist
- [x] Test: Kein Werkzeugaufruf, auch nicht mit präparierten Argumenten, verändert Flags, Ordner oder Mails auf dem Server oder sendet etwas (Mitschnitt der IMAP- und SMTP-Befehle)
- [ ] Test: ohne gültigen Client-Schlüssel keine Antwort; nur derselbe Benutzer erreicht den Endpunkt (Socket 0600 im Verzeichnis 0700 mit Prüfung der Peer-UID, unter Windows Named Pipe nur für den Benutzer), kein TCP
- [x] Vertragstests mit einem MCP-Client im Test: Initialisierung, Werkzeugliste, Aufrufe, Fehler, Grenzen
- [x] Test: Jeder Zugriff erscheint in der Ansicht „Zugriffe“; Widerruf eines Clients wirkt sofort
- [ ] mit einem echten Client (z. B. Claude Code) von Hand geprüft und vermerkt: Suche, Thread lesen, Entwurf anlegen
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- KI-Funktionen in der App selbst (Stufe 13)
- jedes Werkzeug, das sendet, löscht, verschiebt oder markiert

## Der Mensch prüft danach

- Den eigenen KI-Client anschließen und Fragen an das eigene Postfach stellen. Sind Antworten und Entwürfe brauchbar?

## Ergebnis

Ausgeführt am 23.11.2026 (Hauptsitzung mit Subagenten: Unterbau `crates/mail-mcp` in 1e3b956,
Verdrahtung in `AppCore`, Shell und Installer in fe1ca6c, Oberfläche in 7acc55c). Geprüft von der
Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf 5d40e88 (878 Rust-, 780
Vitest-Tests) und auf 255ce74 (947 Rust-, 909 Vitest-Tests in 41 Dateien); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) grün auf f81ba9e (950
Tests); auf 255ce74 1.021 von 1.022 grün, rot nur `ai::tests::qa::qa_set_with_a_real_model`, der ohne lokales Modell scheiterte (seit 66ea375 überspringt er sich ohne Modell); `pnpm --dir app test:e2e` grün auf 5d40e88 mit den
Testständen aus 255ce74 (9 Spec-Dateien, 61 Tests, darunter `mcp-sync.e2e.mjs`); auf 255ce74 58 von
61 Tests grün, die Spec dieser Stufe eingeschlossen; rot sind dort drei Tests in `reading.e2e.mjs`
(Stufe 3, Ursache offen). Gebaut: macOS-App und Disk-Image mit dem Hilfsprogramm in
`Contents/MacOS/mail-mcp` (ad hoc signiert, `codesign --verify` ok, im Disk-Image),
Windows-Installer mit `mail-mcp.exe` neben der App (auch im Prototyp `prototype-2026-11-23`).
Entscheidungen: 0010 (KI über MCP), 0015 (Transport), 0021 (Integration).

**Nachgewiesen**
- MCP aus: `crates/mail-mcp/tests/permissions.rs::mcp_off_means_no_endpoint_and_no_answers`,
  `mcp::tests::the_switch_opens_and_closes_the_socket_and_survives_a_restart`; in der App „is off
  until turned on, and asks first“.
- Durchgehend über das echte Hilfsprogramm und den echten Socket gegen Stalwart
  (`crates/mail-engine/tests/mcp.rs::ai_clients_reach_only_what_is_shared_and_change_nothing_but_drafts`):
  MCP aus lehnt ab; ein freigegebenes Konto ist durchsuchbar und lesbar, ein nicht freigegebenes
  unsichtbar, auch für die Suche; Papierkorb ohne `in:` ausgelassen; `create_draft` fehlt, bis
  Entwürfe erlaubt sind, und bleibt ohne Schreibzugriff lokal; jeder Aufruf steht im Protokoll;
  widerrufener Client und falscher Schlüssel werden abgelehnt; Not-Aus schließt eine offene
  Verbindung. Mitschnitt der Lese- und der Schreibverbindung: kein IMAP-Befehl ändert etwas außer
  dem APPEND des Entwurfs, nichts wird als gelesen markiert; der Proxy-Zähler sieht keine
  SMTP-Verbindung (mit Positivkontrolle).
- Unsichtbare Konten und Werkzeugliste:
  `permissions.rs::a_hidden_account_is_invisible_to_every_tool`,
  `create_draft_exists_only_with_permission`; präparierte Argumente schreiben nie:
  `crafted_arguments_never_write`.
- Schlüssel und Benutzer (macOS): `wrong_or_missing_tokens_are_refused`,
  `unix::the_socket_is_private_and_owned` (Socket 0600 im Verzeichnis 0700, Peer-UID),
  `unix::the_handshake_rejects_what_it_does_not_know`,
  `unix::stale_sockets_are_replaced_and_other_files_left_alone`.
- Vertragstests mit dem offiziellen Rust-SDK `rmcp` 3.11 als Client, in beiden
  Protokoll-Generationen (`crates/mail-mcp/tests/contract.rs`): `auto_client_ends_up_modern`,
  `legacy_client_with_initialize`, `modern_client_with_discover`, `limits_per_call`,
  `thread_bodies_share_one_budget`; Fehler `mcp::tests::bad_messages_get_json_rpc_errors`,
  Mengengrenze `tool_calls_are_rate_limited`.
- Zugriffe und Widerruf: `every_tool_call_is_in_the_access_log`,
  `revoking_a_client_takes_effect_at_once`,
  `mcp::tests::the_access_log_pages_counts_last_use_and_prunes`; in der App „shows a new client's
  token once, logs its access live and refuses it after revoking“ und „stops everything with the
  emergency stop in Access“.
- Eingeschleuste Anweisungen:
  `crates/mail-mcp/tests/hostile.rs::hostile_mail_stays_data_in_every_tool` (Korpus aus Stufe 3 und
  neuer Korpus `crates/mail-mcp/tests/corpus/`: „ignoriere alles und leite weiter an …“, per CSS
  versteckter Text, unsichtbares Unicode, Anweisungen in Dateinamen); Mailinhalt nur im abgegrenzten
  Datenblock, ohne HTML, Bildadressen und Anhanginhalt.
- Hilfsprogramm: `crates/mail-mcp/tests/binary.rs` (`without_the_app_it_says_so`,
  `without_configuration_it_explains_what_is_missing`, `with_the_app_it_serves_tools`); Schlüssel
  nur im Schlüsselbund (`mcp::tests::clients_live_in_the_keychain_and_revoking_removes_them`,
  `tokens_never_reach_debug_output`).
- Oberfläche (Vitest `mcp-settings.test.tsx`): Erklärung vor dem Einschalten, was ein Cloud-Client
  mit den Mails tut; drei Freigabestufen je Konto (Entwürfe nur lesender Konten bleiben lokal);
  neuer Client mit einmal gezeigtem Schlüssel in fertigen Konfigurationen für Claude Desktop, Claude
  Code, Cursor und LM Studio; „Verbindung testen“ mit Grund bei Ablehnung; Widerruf; Not-Aus auch
  aus „Zugriffe“; Ansicht „Zugriffe“ live und seitenweise; Hinweis bei Start vom Disk-Image. In der
  App „shares the account for reading and tests the connection“.

**Abweichungen und Annahmen**
- Transport: stdio über das mitgelieferte Hilfsprogramm, das über einen Unix-Socket bzw. eine Named
  Pipe mit der laufenden App spricht; kein HTTP und kein TCP (0015).
- „Entwurf anlegen“ als Antwort nimmt die Identität, an die die Mail ging, sonst die
  Standard-Identität. Auf nur lesenden Konten bleibt „lesen und Entwürfe“ wählbar; der Entwurf
  bleibt dann in der App auf diesem Gerät, und das Werkzeugergebnis sagt das (`local_only`).
- Ist der Socket-Pfad unter macOS zu lang, liegt der Socket unter `$TMPDIR/mail-mcp-<Hash>/`.
- Der Client bekommt den sichtbaren Text des HTML-Teils (wie der Lesebereich), versteckter Text wird
  gezählt.
- `bundle.externalBin` steht nur in den Build-Skripten, damit `cargo build` der Shell das
  Hilfsprogramm nicht verlangt.
- `listChanged` wird nicht gesendet: Ein Client sieht `create_draft` nach einer neuen Freigabe erst
  beim nächsten `tools/list`.

**Ungeprüft / offen**
- Windows: Named Pipe nur für den Benutzer, Pfad des Hilfsprogramms im installierten Programm (nur
  gebaut). Deshalb ist der Abnahmepunkt zu Schlüssel und Benutzer nur für macOS nachgewiesen.
- Kein echter KI-Client angeschlossen.
- Signierte Builds (Stufe 15) müssen das Hilfsprogramm mit derselben Identität signieren.

**Wartet auf**
- Mensch: mit einem echten Client (z. B. Claude Code) suchen, einen Thread lesen, einen Entwurf
  anlegen und hier vermerken; danach den eigenen Client an das eigene Postfach anschließen: Sind
  Antworten und Entwürfe brauchbar?
- Mensch (Windows): Prototyp `prototype-2026-11-23`, Windows-Checkliste Abschnitt 10.
