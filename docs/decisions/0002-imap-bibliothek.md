# 0002 – IMAP-Bibliothek: async-imap 0.12 mit Tokio und rustls

**Status:** entschieden (Spike A, Stufe 0) · **Datum:** 22.11.2026 · **Code:** `spikes/imap/` (Wegwerf-Code,
Ausgaben des letzten vollständigen Laufs in `spikes/imap/results/`)

## Kontext

Die Architektur setzt für IMAP auf `async-imap` 0.12 mit Tokio und rustls. Spike A sollte gegen den
lokalen Testserver (Stalwart 1.1.1, siehe 0001) prüfen, ob die Bibliothek alles trägt, was die Stufen
2, 4 und 5 brauchen. Dazu gehören TLS und STARTTLS, SPECIAL-USE, EXAMINE und SELECT, Kopfdaten in
Fenstern, CONDSTORE, IDLE, MOVE und der Rückfall darauf, APPEND, eigene Schlagwörter und
Wiederverbinden. Außerdem sollte er die Kopfdaten von 10.000 Mails messen und die Lücken der
Bibliothek festhalten.

**Versionen geprüft (crates.io, 22.11.2026):**

| Crate | Version | veröffentlicht | Lizenz | MSRV |
|---|---|---|---|---|
| async-imap | 0.12.0 (aktuell) | 30.09.2026 | MIT OR Apache-2.0 | 1.88 |
| imap-proto (Parser von async-imap) | 0.17.0 | 28.09.2026 | MIT OR Apache-2.0 | 1.70 |
| tokio | 1.54.1 | 21.10.2026 | MIT | 1.85 |
| tokio-rustls | 0.26.6 | 28.09.2026 | MIT OR Apache-2.0 | 1.71 |
| rustls (Provider `ring`) | 0.23.46 | 14.10.2026 | Apache-2.0 OR ISC OR MIT | 1.85 |
| rustls-platform-verifier | 0.7.1 | 24.09.2026 | MIT OR Apache-2.0 | 1.85 |
| lettre (optional geprüft) | 0.11.24 | 17.11.2026 | MIT | 1.85 |
| ring (Krypto-Provider) | 0.17.14 | 11.03.2025 | Apache-2.0 AND ISC | – |

Features: `async-imap = { default-features = false, features = ["runtime-tokio"] }` (Standard ist
async-std), `rustls = { default-features = false, features = ["ring", "std", "tls12", "logging"] }`,
`tokio-rustls` ebenso mit `ring`, `lettre = { default-features = false, features = ["tokio1-rustls",
"ring", "rustls-platform-verifier", "smtp-transport", "builder", "hostname"] }`.
`cargo tree -i openssl-sys` meldet „did not match any packages“. `native-tls` und `aws-lc-sys` kommen
ebenfalls nicht vor. Alle 151 Pakete der Auflösung (einschließlich plattformspezifischer) haben permissive, mit GPL-3.0 verträgliche Lizenzen
(MIT, Apache-2.0, ISC, BSD-3-Clause, Zlib, 0BSD, Unicode-3.0, CDLA-Permissive-2.0 für Wurzeldaten).

**Pflege von async-imap:** Release 0.12.0 am 30.09.2026, davor 0.11.3 (17.07.2026). Im Repository gab
es Commits bis zum 20.11.2026, darunter einige, die noch in keinem Release stecken: Error::No/Bad
behalten die Statusantwort (API-Bruch, also 0.13), kein Panic mehr bei IDLE ohne Text, `unwrap`
verboten, `unsafe` verboten. Es gibt 30 offene Issues und PRs. Die wichtigsten für uns: #151 (Streams
ignorieren den getaggten Status), PR #146 (NO/BAD statt leerem Ergebnis), PR #152 (QRESYNC), #166
(LIST-EXTENDED), #140 (`has_str` unterscheidet Groß- und Kleinschreibung). Delta Chat nutzt die
Bibliothek im Dauerbetrieb.

## Entscheidung

1. **Es bleibt bei `async-imap` 0.12.0** (Tokio, rustls). Jede Anforderung ließ sich umsetzen und
   nachweisen. Wo die Bibliothek etwas verschweigt, hilft ein eigener schmaler Befehlsweg über die
   öffentlichen Bausteine `run_command` und `read_response` mit den Typen von `imap-proto`. Ein
   echtes Hindernis gab es nicht. `imap-next` und `imap-client` wurden deshalb nicht geprüft.
2. **`mail-imap` bekommt eine eigene Befehlsschicht.** Alles, was den getaggten Status oder einen
   Antwortcode braucht, läuft über sie: FETCH für den Abgleich, STORE, UID EXPUNGE, COPY, APPEND,
   ENABLE, QRESYNC und LIST-STATUS. Antworten werden sofort in Modelltypen übersetzt. Von async-imap
   bleiben Verbindung, Puffer, Parser, LOGIN, SELECT/EXAMINE/STATUS und IDLE.
3. **TLS:** rustls mit dem Provider `ring` und `rustls-platform-verifier` 0.7.1. Im Produkt prüft
   `Verifier::new` gegen den Systemspeicher. In Tests prüft `Verifier::new_with_extra_roots(test_ca)`
   gegen den Systemspeicher plus die Test-CA. Die Test-CA kommt nie in den Schlüsselbund.
4. **Zeitlimits gehören in den Adapter.** async-imap hat keine. Eine kleine Hülle um den Stream wirft
   `TimedOut`, wenn für eine einstellbare Zeit kein Byte kommt. Während IDLE wird die Grenze
   umgestellt.
5. **Eigene kleine Module statt Abhängigkeiten,** weil die Kandidaten gegen die 12-Monats-Regel
   verstoßen: modifiziertes UTF-7 (`utf7-imap`, letztes Release 2022) und Zeitlimits
   (`tokio-io-timeout`, letztes Release 20.07.2025). Jedes Modul hat rund 100 Zeilen.

## Anforderung → Ergebnis → Nachweis

Alle Nachweise lassen sich mit `spikes/imap/run-all.sh` ausführen, einzeln mit
`./target/release/imap-spike <szenario>`. Jede Prüfung druckt eine Zeile `PASS|FAIL <id> <Details>`.
Letzter Lauf: 59 von 59 Prüfungen in `all` bestanden, 3 von 3 in `shutdown`, dazu 2 Unit-Tests.

| Anforderung | Ergebnis | Nachweis (Szenario → Prüfung) |
|---|---|---|
| TLS (993-Ersatz 1993) mit Systemprüfung plus Test-CA | PASS, TLS 1.3, AES-256-GCM | `tls` → `tls.imaps_platform_plus_test_ca` |
| Nur Test-CA über webpki (ohne System) | PASS | `tls.imaps_webpki_test_ca_only` |
| Ohne Test-CA scheitert es typisiert | PASS, `CertUntrusted (UnknownIssuer)`, ebenso über STARTTLS | `tls.untrusted_without_test_ca`, `starttls.untrusted_without_test_ca` |
| Falscher Name, falsches Passwort, Port zu | PASS: `CertNameMismatch`, `AuthFailed` (Passwort nicht in der Meldung), `Refused` | `tls.name_mismatch`, `tls.wrong_password_typed`, `tls.port_closed_typed` |
| STARTTLS (143-Ersatz 1143) | PASS: Klartext-Begrüßung mit `STARTTLS LOGINDISABLED`, Upgrade, LOGIN | `starttls` |
| Capabilities | PASS: IDLE, CONDSTORE, MOVE, UIDPLUS, SPECIAL-USE, ENABLE, QRESYNC, ID, LITERAL+, UTF8=ACCEPT erkannt | `caps.required_present` |
| ID, ENABLE | ID über `Session::id` PASS. ENABLE gibt es nur als Roh-Befehl, `* ENABLED` wird als Capabilities geparst | `caps.id_command`, `caps.enable_condstore_raw` |
| Ordnerliste, Trenner | PASS: 10 Ordner, Trenner `/` | `folders.list_all` |
| SPECIAL-USE | PASS: `LIST "" "*" RETURN (SPECIAL-USE)` (als Muster übergeben) liefert `\Sent \Drafts \Trash \Junk \Archive`. `LIST (SPECIAL-USE) "" "*"` nur als Roh-Befehl | `folders.special_use_return`, `folders.list_selection_special_use` |
| LIST-STATUS in einem Rundlauf | PASS: 10 LIST plus 10 STATUS mit HIGHESTMODSEQ aus einem Befehl (Roh-Befehl) | `folders.list_status_one_round_trip` |
| Modifiziertes UTF-7 | PASS: `Projects/&ANw-berpr&APw-fung` ↔ `Projects/Überprüfung`, EXAMINE über den kodierten Namen. Eigenes Modul, async-imap reicht Namen unverändert durch | `folders.mutf7_decode`, `folders.examine_non_ascii`, `cargo test` |
| UTF8=ACCEPT | PASS: nach `ENABLE UTF8=ACCEPT` kommen die Namen als UTF-8 und werden geparst | `folders.utf8_accept_list` |
| SELECT: UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ, PERMANENTFLAGS | PASS über `select_condstore` | `select.condstore_values` |
| EXAMINE | PASS. Den Code `[READ-ONLY]` gibt es nur über den Roh-Befehl, `Mailbox` hat kein Feld dafür. HIGHESTMODSEQ kommt nur nach `ENABLE CONDSTORE` oder mit `EXAMINE … (CONDSTORE)` | `select.examine_values`, `select.examine_read_only_code`, `select.enable_condstore_then_examine` |
| STORE nach EXAMINE schlägt fehl | PASS: Server antwortet `NO Not permitted in EXAMINE state.`, Flags unverändert. **async-imap meldet dabei `Ok` mit 0 Einträgen** | `select.examine_store_rejected` (INFO-Zeile davor) |
| Nur-Lese-Schutz im Adapter | PASS: Die Sperre lehnt mit `NetError::ReadOnly` ab, bevor etwas gesendet wird | `select.readonly_guard_typed` |
| UID FETCH in 500er-Fenstern von neu nach alt, alle Teile | PASS: 10.000 Mails in 10 Ordnern, bei keiner fehlt UID, FLAGS, RFC822.SIZE, ENVELOPE, BODYSTRUCTURE, INTERNALDATE, MODSEQ oder HEADER.FIELDS | `fetch.no_panic_stream_path`, `fetch.all_parts_parsed` |
| Kaputte Mails (`broken/*`) | PASS: alle 26 vollständig geparst, darunter 40 verschachtelte Multiparts, 8-Bit-Kopfzeilen, kaputte Adressen und unmögliches Datum. Kein Panic (Abruf im Tokio-Task, `JoinError::is_panic` geprüft) | `fetch.broken_messages_parsed` |
| Kopfzeilen List-Id, References usw. | PASS: List-Id bei 988 Mails, References bei 569 | `fetch.header_fields_extracted` |
| Eigener geprüfter FETCH gleichwertig | PASS: dieselben 10.000 (Ordner, UID) | `fetch.raw_path_same_result` |
| Große und kaputte Bodies (Literale) | PASS: 28 vollständige Bodies bis 6,8 MB, Größe = RFC822.SIZE | `fetch.full_bodies_broken_and_large` |
| Stream mitten im Abruf fallen lassen | PASS mit Einschränkung: Das nächste Kommando liest den Rest des alten FETCH mit (NOOP 5 ms bei ~9.900 FLAGS-Zeilen). 100 Antworten landen im Kanal für unaufgeforderte Antworten, der Rest wird verworfen | `fetch.dropped_stream_recovers` |
| CONDSTORE `CHANGEDSINCE` | PASS über `uid_fetch("1:*", "(UID FLAGS) (CHANGEDSINCE n)")`: genau die geänderte Mail, MODSEQ geparst | `condstore.changedsince` |
| `STORE (UNCHANGEDSINCE n)` mit `[MODIFIED]` | PASS nur über den Roh-Befehl. imap-proto kennt den Code nicht, er steht im Text (`[MODIFIED 9950] UID STORE completed`). async-imap meldet `Ok(0)` | `condstore.unchangedsince_modified`, `…_success` |
| QRESYNC (nicht verlangt, für Stufe 2/4) | PASS über Roh-Befehle: `ENABLE QRESYNC` und `SELECT … (QRESYNC (v m))` liefern `VANISHED (EARLIER)` und FETCH, imap-proto parst beides | `condstore.qresync_raw` |
| IDLE mit Zeitlimit | PASS: `wait_with_timeout(2 s)` → `Timeout` nach 2.003 ms | `idle.timeout` |
| IDLE-Abbruch von außen | PASS: Ein anderer Task lässt die `StopSource` fallen → `ManualInterrupt` nach 303 ms | `idle.interrupt_from_outside` |
| EXISTS von außen (SMTP bob → alice über 1587) | PASS: `* 9960 EXISTS` 16 ms nach Sendebeginn. Stalwart schickt vorher ein unaufgefordertes `STATUS` | `idle.exists_from_smtp` |
| Verbindung nach IDLE weiter benutzbar | PASS: DONE, NOOP, FETCH der neuen Mail | `idle.session_usable_after_done` |
| `Send`-Futures | PASS: Session, IDLE-Future und FETCH-Stream laufen in `tokio::spawn` | `idle`, `fetch` (kompiliert und läuft) |
| UID MOVE | PASS: Mail ist am Ziel und nicht mehr in der Quelle. COPYUID nur über den Kanal für unaufgeforderte Antworten | `move.uid_move` |
| COPY + `STORE +FLAGS (\Deleted)` + UID EXPUNGE | PASS: COPYUID über den Roh-Befehl. Eine von einer zweiten Sitzung als gelöscht markierte Mail bleibt erhalten | `move.copy_store_uid_expunge` |
| APPEND mit Flags und Datum | PASS. `Session::append` gibt `()` zurück, kein APPENDUID. Über den Roh-Befehl kommt APPENDUID, Flags und INTERNALDATE stimmen (in UTC) | `append.async_imap`, `append.flags_date_appenduid` |
| Großes APPEND | PASS: 5,1 MB in 6–7 ms | `append.big_literal` |
| Eigene Schlagwörter setzen, lesen, entfernen | PASS: `Spike_Kw $SpikeTwo projekt-y` gesetzt, über eine neue Verbindung gelesen (kleingeschrieben), mit anderer Schreibweise entfernt | `keywords.*` |
| Ungültiges Schlagwort | Server antwortet `BAD`, `uid_store` verschluckt das. Eine Prüfung im Client ist nötig | `keywords.invalid_atom_rejected` |
| Verbindung mitten im FETCH gekappt, Fortsetzung | PASS: `ConnectionLost` 0–1 ms nach dem Kappen. Neu verbunden und unterhalb der letzten vollständig gespeicherten UID fortgesetzt. 9.960/9.960 gespeichert, 96 Datensätze des abgebrochenen Fensters verworfen und neu geholt. Mit eigenem FETCH und mit dem async-imap-Stream | `reconnect.close_mid_fetch_checked`, `…_stream` |
| Stille Verbindung (kein Byte mehr) | PASS: `Timeout` nach der Lesefrist (3,0 s), dann fortgesetzt | `reconnect.stall_mid_fetch_timeout` |
| Verbindungsaufbau abgewiesen, Backoff | PASS: zweimal `ConnectionLost`, Backoff 100/200 ms, im dritten Versuch fertig | `reconnect.refused_then_backoff` |
| IDLE auf gekappter oder stiller Verbindung | PASS: gekappt → `wait` meldet sofort einen Fehler. Still → erst nach Ende des IDLE-Abschnitts (4 s) plus Lesefrist (2 s) = 6,0 s `Timeout` | `reconnect.idle_connection_closed`, `…_silent` |
| Server fährt während IDLE und FETCH herunter | PASS: IDLE bekommt `NewData(BYE)`, beide FETCH-Wege melden `ConnectionLost` nach 1.966 Mails | `shutdown` |
| SMTP mit lettre und Zusatz-CA (optional) | PASS über 1587 mit STARTTLS. Ohne CA scheitert es, lettre meldet aber nur „Connection error“ (`is_tls() = false`). Der rustls-Fehler steckt in der Ursachenkette → `CertUntrusted` | `smtp.*` |

## Messwerte

**Umgebung:** Mac mini, Apple M4 (10 Kerne), 16 GB, macOS 27.2.1, Rust 1.100, Release-Build. Stalwart 1.1.1
lief auf demselben Rechner, Verbindung über Loopback mit TLS 1.3 (Port 1993). Der Datenbestand kam aus
`scripts/testmail.sh reset && start && seed 10000 --filler-folder INBOX` (INBOX 9.959 Mails).
Gemessen am 22.11.2026 in drei getrennten Läufen (3, 5 und 5 Wiederholungen je Variante). Abfrage:
`UID FLAGS RFC822.SIZE ENVELOPE BODYSTRUCTURE INTERNALDATE MODSEQ BODY.PEEK[HEADER.FIELDS (MESSAGE-ID
REFERENCES IN-REPLY-TO LIST-ID LIST-UNSUBSCRIBE AUTO-SUBMITTED PRECEDENCE)]`, jede Antwort wird in einen
eigenen Datensatz übersetzt.

| Kopfdaten von 9.959 Mails, Fenster von neu nach alt | Median (Min–Max) | Mails/s (Median) |
|---|---|---|
| async-imap-Stream, Fenster 100 | 0,059 s (0,058–0,073) | 169.000 |
| eigener geprüfter FETCH, Fenster 100 | 0,057 s (0,054–0,062) | 175.000 |
| async-imap-Stream, Fenster 500 | 0,044 s (0,043–0,045) | 226.000 |
| eigener geprüfter FETCH, Fenster 500 | 0,042 s (0,041–0,042) | 237.000 |
| async-imap-Stream, Fenster 2000 | 0,041 s (0,040–0,042) | 243.000 |
| eigener geprüfter FETCH, Fenster 2000 | 0,038 s (0,038–0,039) | 262.000 |

| Weitere Messung | Wert |
|---|---|
| Nur `UID FLAGS` aller 9.959 Mails (Abgleich ohne CONDSTORE) | 6 ms als `1:*`, 6–8 ms in 2000er-Fenstern |
| Datenmenge über die Leitung für einen Kopfdaten-Durchlauf | 8,6 MB TLS-Daten, 866 Byte je Mail |
| Verbindungsaufbau, TLS, LOGIN, EXAMINE | 15–27 ms |
| Speicher (RSS) beim Start | 7,0 MB |
| Speicher mit 9.959 Datensätzen im Speicher | 18,7–19,2 MB, Spitze 18,9–19,7 MB (`getrusage`), bei allen Fenstergrößen gleich |
| Abbrucherkennung: Verbindung gekappt (FIN, TLS ohne close_notify) | 0–1 ms, `ConnectionLost` |
| Abbrucherkennung: Verbindung still | Lesefrist (3,0 s eingestellt), `Timeout` |
| Abbrucherkennung in IDLE: gekappt / still | sofort / IDLE-Abschnitt + Lesefrist (4 + 2 s = 6,0 s) |
| ganzer Abgleich einschließlich Abbruch, Erkennung und Fortsetzung | 183 ms (gekappt), 3,2 s (still, davon 3,0 s Lesefrist), 0,4 s (2× abgewiesen) |
| SMTP-Versand bis `EXISTS` in IDLE | 16 ms |
| APPEND 5,1 MB | 6–7 ms |

**Einordnung:** Alle Läufe lagen innerhalb von ±5 %, nur der erste Durchlauf nach dem Serverstart war
langsamer (0,069–0,073 s bei Fenster 100). Während der Messung liefen andere Agenten. Die Last lag bei
2,7–3,6 auf 10 Kernen, Client und Server teilten sich die CPU. Eine leichte Verfälschung nach oben ist
möglich, eine starke nicht, weil die Werte über alle Läufe kaum streuen. Die Zahlen zeigen vor allem,
dass Bibliothek und Parser nicht bremsen: rund 4 µs je Mail, Server eingeschlossen. Bei einem echten Server
bestimmen Laufzeit und Bandbreite die Dauer. Eine **Rechnung, keine Messung:** 8,6 MB bei 50 Mbit/s
dauern etwa 1,4 s, 20 Fenster bei 30 ms Laufzeit kosten etwa 0,6 s. Größere Fenster sparen also vor
allem Rundläufe. Der geprüfte eigene FETCH ist 3–7 % schneller als der Stream, weil er
die Antworten direkt übersetzt.

## Lücken und Umgehungen

| Lücke in async-imap 0.12.0 | Folge | Umgehung |
|---|---|---|
| Stream-Befehle (`fetch`, `uid_fetch`, `uid_store`, `uid_expunge`, `list`) prüfen den getaggten Status nicht (#151) | NO/BAD wird zu einem leeren `Ok`. Nachgewiesen bei STORE nach EXAMINE, ungültigem Schlagwort und UNCHANGEDSINCE-Konflikt. Endet die Verbindung genau zwischen zwei Antworten ohne TLS-Fehler, hält der Stream das für ein reguläres Ende. Das stammt aus dem Quelltext; mit TLS und Stalwart kam immer ein Fehler | eigene Befehlsschicht (`raw.rs`: `raw`, `fetch_checked`) prüft das Tag, gleich schnell |
| Antwortcodes werden verschluckt: APPENDUID (`append` gibt `()` zurück), COPYUID bei UID COPY, `[MODIFIED]` (imap-proto kennt ihn nicht), `[READ-ONLY]` (kein Feld in `Mailbox`). Fehler sind formatierte Strings (`Error::No(String)`) | Outbox (Stufe 4) und Gesendet-Ablage (Stufe 5) brauchen diese Codes | Roh-Befehl, Codes aus `imap_proto::ResponseCode`. `[MODIFIED …]` aus dem Text gelesen. Upstream ist eine Änderung, die die Statusantwort im Fehler behält, gemergt, aber noch nicht veröffentlicht |
| Keine API für ENABLE, QRESYNC (`SELECT (QRESYNC …)`, VANISHED), `EXAMINE (CONDSTORE)`, LIST-Auswahloptionen, LIST-STATUS | – | Roh-Befehle, imap-proto parst ENABLED, VANISHED und STATUS. `RETURN (…)` lässt sich über das unquotierte Muster von `list()` einschleusen, das ist aber zerbrechlich. PR #152 (QRESYNC) ist offen |
| Kanal für unaufgeforderte Antworten fasst 100 Einträge, `try_send` verwirft still | Ein liegengebliebenes EXISTS oder EXPUNGE kann verloren gehen (100 von ~9.900 geparkt, nachgewiesen) | nach jedem Befehl leeren. Der Abgleich prüft EXISTS und UIDNEXT ohnehin bei jedem Durchlauf |
| Fallen gelassener Stream: der Rest wird vom nächsten Befehl gelesen | Ein abgebrochener großer Abruf wird trotzdem vollständig übertragen | Abbruch heißt Verbindung schließen, nicht weiter benutzen |
| keine Zeitlimits | eine tote Verbindung hängt für immer | `timeout_io.rs`: Grenze für Lesen und Schreiben ohne Fortschritt, zur Laufzeit änderbar (IDLE) |
| IDLE: `wait` kehrt bei jeder unaufgeforderten Antwort zurück (Stalwart schickt STATUS vor EXISTS, BYE beim Herunterfahren). `idle()` verbraucht die Session. EOF ohne Fehler meldet `ManualInterrupt` (nur im Quelltext gefunden, mit TLS nicht beobachtet). 0.12.0 kann in `Handle::init` bei `BAD` ohne Text paniken (upstream behoben, nicht veröffentlicht) | – | in einer Schleife bis EXISTS/EXPUNGE/FETCH warten, BYE als Verbindungsende behandeln. Nach Fehlern in `done()` neu verbinden. IDLE nur mit der Capability IDLE |
| Postfachnamen: kein modifiziertes UTF-7 | – | eigenes Modul `mutf7.rs` mit Tests |
| `Capabilities::has_str` unterscheidet bei Atomen die Schreibweise (#140), `capabilities()` gibt es vor LOGIN nicht (#45) | – | eigene `Capabilities`-Struktur mit großgeschriebenen Namen; vor LOGIN die `[CAPABILITY]` aus der Begrüßung |
| `ResponseData` wird von öffentlichen Methoden zurückgegeben, ist aber nicht benennbar (`pub(crate)`-Reexport) | – | über den assoziierten Typ `<idle::Handle<S> as Stream>::Item` benennen oder sofort in eigene Typen übersetzen |
| Kann imap-proto eine Antwort nicht parsen, ist die Verbindung danach unbrauchbar | Mit dem Seed-Korpus kam es nicht vor (40-fach verschachtelt, 8-Bit-Kopfzeilen, kaputte Adressen). Bei exotischen Servern möglich | als `Protocol`-Fehler melden, neu verbinden, betroffenes Fenster verkleinern |
| APPEND nutzt nie LITERAL+ (ein Rundlauf mehr). Envelope-Felder sind rohe Bytes (RFC 2047 nicht dekodiert) | – | Roh-APPEND mit `{n+}`, wenn LITERAL+ da ist. Dekodieren mit `mail-parser` (Stufe 3) |
| kein IMAP4rev2, kein UIDONLY | nicht nötig | – |
| transitive Abhängigkeit `stop-token` 0.7.0 (Januar 2022) für den IDLE-Abbruch | verstößt formal gegen die 12-Monats-Regel, ist aber keine direkte Abhängigkeit | beobachten (Issue #164 schlägt eine Future statt StopSource vor) |

Die Server-Eigenheiten aus 0001 bestätigt der Spike: Schlagwörter kommen kleingeschrieben zurück,
`$hasattachment` und `$hasnoattachment` werden ergänzt, INTERNALDATE kommt in UTC. Neu ist: Nach
`SELECT … (CONDSTORE)` liefert Stalwart bei einem späteren EXAMINE ohne Parameter kein HIGHESTMODSEQ,
nach `ENABLE CONDSTORE` schon. Und beim Herunterfahren schickt Stalwart `* BYE` und schließt TCP ohne
TLS-close_notify.

## TLS-Konzept

- **Produkt:** `rustls::ClientConfig` mit dem Provider `ring` (seit 23.11.2026 `aws-lc-rs`, siehe
  Nachtrag unten) und `rustls_platform_verifier::Verifier::new(provider)`. Geprüft wird gegen den Systemspeicher: auf
  macOS über Security.framework (`SecTrust`), auf Windows über CryptoAPI. Ein selbst signiertes
  Serverzertifikat geht, wenn der Nutzer ihm im System vertraut.
- **Tests:** `Verifier::new_with_extra_roots(ca, provider)` mit der CA aus `TESTMAIL_CA_FILE`, geladen
  über `CertificateDer::pem_file_iter`. Auf macOS setzt der Verifier `SecTrustSetAnchorCertificates`
  und erlaubt mit `SetAnchorCertificatesOnly(false)` weiter die Systemwurzeln. Das Zertifikat des
  Testservers (825 Tage) wird angenommen. Nichts kommt in den Schlüsselbund. Wie die Zusatz-Wurzel in
  den Code kommt, legt Stufe 2 fest; eine Testeinstellung oder ein Konstruktorparameter von `mail-imap`
  liegen nahe, nie eine Nutzereinstellung im ausgelieferten Build.
- **Fehler für die Oberfläche:** `rustls::Error::InvalidCertificate(UnknownIssuer | BadSignature | Other)`
  wird zu `CertUntrusted`, `NotValidForName` zu `CertNameMismatch`, `Expired` oder `NotValidYet` zu
  `CertExpired`. Bei tokio-rustls steckt der rustls-Fehler im `io::Error`, bei lettre in der
  Ursachenkette. Der Apple-Verifier meldet eine unbekannte CA als `UnknownIssuer`, andere Systemcodes
  als `Other("…: <Code>")`. Auch die werden zu `CertUntrusted`.
- **webpki-only** (eigener `RootCertStore` nur mit der Test-CA) funktioniert ebenfalls. Das ist der
  Rückweg, falls der Plattform-Verifier auf einem System Probleme macht.
- **lettre 0.11.24** mit dem Feature `rustls-platform-verifier` baut aus `add_root_certificate`
  dasselbe `new_with_extra_roots`. Das Konzept gilt also auch für SMTP.
- **Zu beachten:** Bei öffentlichen Zertifikaten kann die Prüfung durch das Betriebssystem
  Sperrinformationen (OCSP, CRL) oder Zwischenzertifikate bei der Zertifizierungsstelle abrufen. Das
  ist eine Verbindung, die nicht zum Mailserver geht. Sie fehlt in der Liste „Erlaubte
  Netzverbindungen“. Mit der Test-CA kommt sie nicht vor (keine URLs im Zertifikat).
- **Krypto-Provider:** `ring` statt des rustls-Standards `aws-lc-rs`, weil er ohne CMake und NASM
  baut. Das ist einfacher für den Windows-Cross-Build. ring hatte sein letztes Release am 11.03.2025,
  das Repository hat Commits bis November 2026. Das verstößt formal gegen die 12-Monats-Regel, siehe
  „Ausnahme für `ring`“. Der Provider wird an einer Stelle gewählt; ein Wechsel ist ein Feature-Flag.
  **Überholt am 23.11.2026:** Provider ist jetzt `aws-lc-rs`, gewählt in `mail_imap::tls::provider()`
  (Nachtrag „Wechsel zu `aws-lc-rs`“).

## Folgen für Stufe 2

- `mail-imap` = async-imap für Verbindung, Parser, LOGIN, SELECT/EXAMINE/STATUS und IDLE, plus eigene
  Befehlsschicht für geprüftes FETCH und alle Befehle mit Antwortcodes. Antworten sofort in
  Modelltypen übersetzen.
- Nach dem LOGIN `ENABLE CONDSTORE` senden, bei QRESYNC-Servern `ENABLE QRESYNC`. Im Nur-Lese-Modus
  mit `EXAMINE` öffnen (bei Stalwart kommt HIGHESTMODSEQ sonst nicht). Die Schreibsperre sitzt vor
  der Befehlsschicht, getestet wie in `select.readonly_guard_typed`.
- Ordner mit `LIST "" "*" RETURN (SPECIAL-USE STATUS (MESSAGES UNSEEN UIDNEXT UIDVALIDITY
  HIGHESTMODSEQ))`, wenn LIST-STATUS da ist: ein Rundlauf für alle Zähler. Sonst LIST und STATUS je
  Ordner. Namen mit modifiziertem UTF-7, `UTF8=ACCEPT` in Version 1 nicht einschalten (dann bräuchte
  APPEND die UTF8-Literal-Syntax).
- Erster Abgleich in Fenstern von 500 von neu nach alt. Das erste Fenster darf kleiner sein (100), damit
  die Liste sofort Inhalt hat. Jedes Fenster in einer Transaktion speichern, Fortschritt als
  „niedrigste vollständig gespeicherte UID“ je Postfach und UIDVALIDITY. Damit setzt ein Abbruch genau
  dort fort (nachgewiesen).
- Zeitlimits: Lesefrist 30 s für normale Befehle. Große Literale stören nicht, weil die Frist nur ohne
  Fortschritt zählt. IDLE in Abschnitten mit eigener Grenze, danach DONE mit Lesefrist. Stille
  Verbindungen werden spätestens nach Abschnitt plus Lesefrist erkannt; bei 25-Minuten-Abschnitten
  also erst nach bis zu 25 Minuten. Empfehlung: kürzere Abschnitte (etwa 5 Minuten) oder
  TCP-Keepalive über `socket2` (schon transitiv vorhanden). Beides ungeprüft.
- Bei IDLE in einer Schleife warten, bis EXISTS, EXPUNGE oder FETCH kommt. STATUS und andere
  Antworten überspringen, BYE als Verbindungsende behandeln. Den Kanal für unaufgeforderte Antworten
  nach jedem Befehl leeren und an den Abgleich geben.
- Fehlertypen für die Meldungen: `CertUntrusted`, `CertNameMismatch`, `CertExpired`, `AuthFailed`,
  `Refused`, `ConnectTimeout`, `Timeout`, `ConnectionLost`, `Protocol`, `ReadOnly`. `is_transient()`
  steuert den Backoff. Im Spike verdoppelt er sich ab 100 ms bis höchstens 5 s; Streuung kommt in
  Stufe 2 dazu.
- Schlagwörter ohne Rücksicht auf die Schreibweise vergleichen und mit großgeschriebenen
  Capability-Namen arbeiten.

## Folgen für Stufe 4

- `UID STORE … (UNCHANGEDSINCE m)` über die Befehlsschicht. Konflikt = `[MODIFIED uids]` im Text der
  getaggten OK-Antwort (nachgewiesen), dann gilt der Serverzustand.
- UID MOVE: COPYUID kommt als unaufgefordertes `* OK [COPYUID …]` (Kanal oder Befehlsschicht). Der
  Rückfall COPY + `STORE +FLAGS.SILENT (\Deleted)` + `UID EXPUNGE` lässt fremde gelöschte Mails stehen
  (nachgewiesen).
- Schlagwörter im Client auf gültige Atome prüfen, bevor sie gesendet werden.
- Optional QRESYNC (VANISHED) für Löschungen statt UID-Mengenvergleich. Über Roh-Befehle möglich,
  async-imap selbst versteht VANISHED nicht.

## Folgen für Stufe 5

- Gesendet-Kopie und Entwürfe per Roh-APPEND mit APPENDUID; die UID landet in `draft`. Mit LITERAL+
  spart `{n+}` einen Rundlauf.
- lettre 0.11.24 mit `tokio1-rustls`, `ring`, `rustls-platform-verifier` funktioniert mit demselben
  Zusatz-Wurzel-Konzept. Fehler müssen über die Ursachenkette klassifiziert werden, weil `is_tls()`
  bei Zertifikatsfehlern `false` ist.
- Doppelte Message-ID bei SMTP: Stalwart verwirft still (0001). Tests zu „Senden wiederholen“ müssen
  das berücksichtigen.

## Ungeprüft

- Windows: Plattform-Verifier mit Zusatz-Wurzeln auf CryptoAPI und der Cross-Build mit `ring`.
- Echte Server (Dovecot, Gmail, Exchange, Anbieter), Laufzeiten im Internet, Mengen über 10.000 Mails.
  Die Zahlen oben gelten nur für Loopback.
- Systemspeicher mit öffentlich ausgestellten Zertifikaten. Verbindungen nach außen sind in den
  Sitzungen nicht erlaubt.
- Ende einer Verbindung zwischen zwei Antworten ohne TLS-Fehler (stille Kürzung eines Streams) und
  `ManualInterrupt` bei EOF in IDLE: nur aus dem Quelltext, nicht provoziert.
- TCP-Keepalive, COMPRESS=DEFLATE, AUTHENTICATE (OAuth), Provider `aws-lc-rs`.

## Ausnahme für `ring` (Entscheidung der Hauptsitzung, 22.11.2026)

**Aufgehoben am 23.11.2026:** Das Produkt nutzt `aws-lc-rs`; `ring` ist nicht mehr im
Abhängigkeitsbaum (Nachtrag „Wechsel zu `aws-lc-rs`“ unten).

`ring` 0.17.14 hatte kein Release in den letzten 12 Monaten und verstößt damit formal gegen die Regel
für neue Abhängigkeiten. Es bekommt eine begründete Ausnahme:

- `ring` ist der Krypto-Kern von rustls, wird inzwischen im Umfeld des rustls-Projekts gepflegt und hat
  Commits bis November 2026; fehlende Releases bedeuten hier Stabilität, nicht Verwahrlosung.
- Es baut ohne CMake und NASM, was den Windows-Cross-Build mit cargo-xwin einfach hält (in Spike A
  und C ohne Zusatzwerkzeuge gelaufen).
- Der Provider wird an genau einer Stelle gewählt (Feature-Flag von rustls). Rückweg: `aws-lc-rs`
  (1.19.0 vom 28.10.2026). Stufe 6 prüft beim Windows-Build, ob `aws-lc-rs` mit cargo-xwin ohne
  Mehraufwand baut; wenn ja, wird gewechselt.
- `cargo deny` meldet keine Sicherheitsmeldung zu `ring`; das prüft `scripts/check.sh` bei jedem Lauf.

Der Mensch wird darüber informiert und kann die Ausnahme widerrufen.

### Nachtrag Stufe 6 (23.11.2026): `aws-lc-rs` mit cargo-xwin geprüft

**Aufbau.** Wegwerf-Crate außerhalb des Workspace (im Scratchpad, danach gelöscht) mit dem
TLS-Stapel des Produkts: rustls 0.23.46 (`std`, `tls12`, `logging`), tokio-rustls 0.26.6,
rustls-platform-verifier 0.7.1, Client-Konfiguration wie in `crates/mail-imap/src/tls.rs`. Ein
Feature wählt `ring` oder `aws_lc_rs` für rustls und tokio-rustls. Release-Profil wie im Workspace
(fat LTO, eine Codegen-Unit, strip), jede Variante kalt in einem eigenen Target-Verzeichnis. Der
PATH enthielt absichtlich nur `~/.cargo/bin`, `/usr/bin`, `/bin` (für Windows dazu LLVM), damit
CMake und Go aus Homebrew nicht gefunden werden. NASM ist auf dem Mac mini nicht installiert.
Windows-Umgebung wie in `scripts/build-windows.sh` (`XWIN_CACHE_DIR`, `XWIN_ARCH=x86_64`).

**Versionen:** aws-lc-rs 1.19.0 und aws-lc-sys 0.47.0, beide vom 28.10.2026 (davor Releases am
12.10., 01.09. und 07.08.2026), MSRV 1.71.

| | `ring` 0.17.14 | `aws-lc-rs` 1.19.0 |
|---|---|---|
| macOS (`aarch64-apple-darwin`), kalt | 14 s | 22 s |
| Windows (`cargo xwin build --target x86_64-pc-windows-msvc`), kalt | 11 s | 19 s |
| Binary macOS | 1.382.336 B | 2.824.640 B (+1,4 MB) |
| Binary Windows (`.exe`) | 1.609.216 B | 2.636.800 B (+1,0 MB) |
| Target-Verzeichnis macOS / Windows | 82 / 102 MB | 116 / 139 MB |
| Zusätzliche Werkzeuge | keine | keine (siehe unten) |
| TLS-Handshake gegen Stalwart (127.0.0.1:1993, Test-CA, macOS) | TLS 1.3, AES-256-GCM | TLS 1.3, AES-256-GCM |

Während der Messung liefen Builds anderer Sitzungen (Load etwa 10 auf 10 Kernen). Die Zeiten sind
deshalb Obergrenzen; der Unterschied von rund 8 s ist das Kompilieren von AWS-LC in C.

**Werkzeuge.**

- aws-lc-sys 0.47 nimmt von sich aus den `cc`-Builder („Building with: CC“) mit vorgenerierten
  Bindings. CMake, bindgen/libclang und Go (nur für FIPS) werden nicht gebraucht, auch nicht unter
  Windows. Die C-Teile übersetzt dort `clang-cl`, wie cargo-xwin es einstellt.
- Die x86_64-Assemblerquellen für Windows sind in NASM-Syntax. Das rustls-Feature `aws_lc_rs`
  schaltet `aws-lc-rs/prebuilt-nasm` ein. Ohne `nasm` im PATH linkt aws-lc-sys deshalb die im Crate
  mitgelieferten, vorab assemblierten Objektdateien (`builder/prebuilt-nasm/`, 26 Stück; im Log nur
  die Warnung „NASM command not found“). Ihr Inhalt ist über die Prüfsumme in `Cargo.lock` festgelegt.
  Mit installiertem NASM (`brew install nasm`) assembliert aws-lc-sys selbst aus den Quellen.
- Mit aws-lc-rs ist `ring` ganz aus dem Graphen verschwunden: `cargo tree -i ring` findet für beide
  Ziele nichts.

**Lizenzen.** aws-lc-rs: `ISC AND (Apache-2.0 OR ISC)`. aws-lc-sys: `ISC AND (Apache-2.0 OR ISC)
AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR
MIT-0)`. Die alte OpenSSL/SSLeay-Lizenz mit Werbeklausel, die mit der GPL nicht verträglich ist,
kommt nicht mehr vor. Laut `LICENSE` des Crates stehen die von OpenSSL abgeleiteten Teile nach der
Umlizenzierung von OpenSSL und BoringSSL unter Apache-2.0. jitterentropy wird ausdrücklich unter
BSD-3-Clause weitergegeben, nicht unter GPL-2.0. Jede einzelne Lizenz verträgt sich mit
GPL-3.0-or-later: Apache-2.0 mit GPLv3, ISC, MIT und BSD-3-Clause sind freizügig, MIT-0 kommt nur als
Alternative vor. `cargo deny check licenses advisories bans sources` mit unserer unveränderten
`deny.toml` meldet für beide Varianten „ok“. Die Allowlist braucht keinen neuen Eintrag.

**Empfehlung: zu `aws-lc-rs` wechseln.** Die Bedingung dieser Ausnahme ist erfüllt: aws-lc-rs baut mit
cargo-xwin ohne Mehraufwand, ohne Zusatzwerkzeuge und ohne Lizenzprobleme. Damit entfällt die
Ausnahme für `ring`, und das Produkt nutzt den Standard-Provider von rustls, der regelmäßig
veröffentlicht wird. Der Preis: rund 1 bis 1,5 MB mehr im Binary, etwa 8 s mehr im kalten Build und
unter Windows vorab assemblierte Objektdateien aus dem Crate, solange kein NASM installiert ist.

Umstellung (nicht Teil dieser Prüfung, Entscheidung der Hauptsitzung):

1. `Cargo.toml`: Features von `rustls` und `tokio-rustls` von `ring` auf `aws_lc_rs` ändern.
2. Der Provider wird heute nicht an einer Stelle gewählt, sondern an sieben: im Produkt
   `crates/mail-imap/src/tls.rs`, `crates/mail-engine/src/reading/links.rs`, `reading/remote.rs`
   und `discovery/autoconfig.rs`, in Tests `discovery/autoconfig.rs` (Testhilfe),
   `discovery/autoconfig/tests.rs` und `crates/mail-imap/src/smtp/tests.rs`. Zuerst eine gemeinsame
   Funktion schaffen (etwa `mail_imap::tls::provider()` öffentlich machen) und überall verwenden.
3. Anpassen: `deny.toml` (Kommentar zu ISC), `docs/ARCHITECTURE.md` (TLS in der Stack-Tabelle) und
   den Abschnitt „TLS-Konzept“ oben.
4. Danach `scripts/check.sh`, die TLS-Tests gegen den Testserver und beide Builds laufen lassen und
   die Größen von `.dmg` und Installer neu messen.

**Ungeprüft:** die Laufzeit unter Windows (Plattform-Verifier über CryptoAPI mit aws-lc-rs, die
vorab assemblierten Objekte auf echter Hardware) und die Größe der echten App. Gemessen wurde nur
am Wegwerf-Crate.

### Nachtrag Reparatur-Sitzung (23.11.2026): Wechsel zu `aws-lc-rs`, Ausnahme für `ring` aufgehoben

**Eine Stelle.** Provider und Client-Konfiguration entstehen nur noch in `crates/mail-imap/src/tls.rs`
(jetzt öffentlich): `provider()` wählt den Provider, `client_config(Option<&ExtraRootCa>)` baut die
Konfiguration mit Plattform-Verifier (Zusatz-CA nur in Tests), `client_config_with_roots(Vec<CertificateDer>)`
dasselbe mit schon geparsten Wurzeln (Test der Autokonfiguration). Benutzt von IMAP und SMTP
(`connector.rs`, `smtp/mod.rs`), von reqwest für externe Bilder (`reading/remote.rs`) und die
Autokonfiguration (`discovery/autoconfig.rs`, über `tls_backend_preconfigured`), vom Zufall der
Render-Tokens (`reading/links.rs`) und von den TLS-Servern der Tests (`smtp/tests.rs`,
`discovery/autoconfig/tests.rs`). Vorher wurde der Provider an sieben Stellen gewählt.
`mail-engine` hängt nicht mehr direkt an `rustls-platform-verifier`. Warum `mail-imap` und kein
eigenes Crate: Es hält die TLS-Verbindungen und bildet ihre Fehler ab, `mail-engine` hängt ohnehin
daran, `mail-model` bleibt ohne IO und ohne rustls; ein Crate für drei Funktionen zöge keine neue
Grenze. Die KI-Schicht von Stufe 13 (`mail-ai`, in Arbeit) soll ihre Konfiguration laut ihrem
Manifest ebenfalls von dort bekommen.

**Umstellung** (Wurzel-`Cargo.toml`): `rustls` mit `aws_lc_rs` statt `ring` (weiter ohne
Standard-Features: `std`, `tls12`, `logging`; also ohne Vorrang für Post-Quanten-Schlüsseltausch,
X25519MLKEM768 wird aber angeboten), `tokio-rustls` ebenso. `reqwest` bleibt bei
`rustls-no-provider` mit der Konfiguration von oben, `lettre` ohne eigenes TLS. Dazu `rcgen`
(Testzertifikate) ohne Standard-Features mit `aws_lc_rs` und `pem`: Sein Standard-Feature `ring`
hätte `ring` als Entwicklungsabhängigkeit behalten.

**Nachweise** (macOS 27.2, Mac mini M4):

- `cargo tree -i ring --workspace -e normal,build,dev` und dasselbe mit `--target all`,
  `--target aarch64-apple-darwin` und `--target x86_64-pc-windows-msvc`: „nothing to print“.
  `cargo tree -i openssl-sys`: „did not match any packages“. `Cargo.lock` führt `ring` 0.17.14
  und `untrusted` 0.7.1 weiter als *optionale* Abhängigkeiten (hickory-proto: DNSSEC, rustls-webpki:
  Feature `ring`, aws-lc-rs: `ring-io`), keine davon ist eingeschaltet; aws-lc-rs läuft mit
  `aws-lc-sys` und `prebuilt-nasm` (`cargo tree -e features -i aws-lc-rs`). Gebaut wird `ring` nicht
  (kein „Compiling ring“ im Release-Build).
- aws-lc-sys meldet auf macOS „Building with: CC“ (`/usr/bin/clang` aus `~/.cargo/config.toml`);
  CMake liegt zwar unter `/opt/homebrew/bin`, wird aber nicht aufgerufen. Neu im Lockfile: aws-lc-rs
  1.19.0 und aws-lc-sys 0.47.0 (28.10.2026), cmake 0.1.58 (das Rust-Crate, 26.03.2026), fs_extra
  1.3.0 (03.02.2023). fs_extra ist nur Build-Abhängigkeit von aws-lc-sys und verstößt wie
  `stop-token` nur transitiv gegen die 12-Monats-Regel: beobachten.
- `cargo deny check licenses advisories bans sources`: alle vier ok. Neue Lizenzen ISC, Apache-2.0,
  MIT, BSD-3-Clause, MIT-0 (nur als Alternative), alle schon erlaubt; in `deny.toml` nur der
  Kommentar zu ISC angepasst.
- Tests mit `aws-lc-rs` gegen den Testserver: `cargo test -p mail-imap -- --include-ignored`
  (`seed 1000`) grün: 72 Unit-Tests (auch SMTP über TLS/STARTTLS mit eigenem TLS-Server und die
  Abbildung der Zertifikatsfehler), `server.rs` 11 (darunter `tls_and_starttls_with_the_test_ca`,
  `certificate_and_login_errors_are_typed`), `smtp.rs` 6, `read_only.rs`, `bodies.rs`.
  `cargo test -p mail-engine -- --include-ignored` (`seed 10000`, `VICEMAIL_RESTART_TEST=1`) grün:
  206 Unit-Tests (Kontoerkennung über HTTPS mit eigenem Server, externe Bilder, Outbox-, Lese- und
  Suchtests gegen den Testserver), `tests/server.rs` 8 von 8, `tests/actions.rs`. Die Lesetests
  gegen den Testserver zusätzlich mit `seed 1000`: 4 von 4.
- Builds mit `aws-lc-rs` aus einem sauberen Worktree von `9dee241` plus dieser Änderung (im
  Checkout kompilierten halbfertige Änderungen anderer Sitzungen gerade nicht), Abhängigkeiten außer
  dem TLS-Stapel schon übersetzt:
  - macOS, `cargo build -p mail-app --release` (fat LTO): 18.494.224 B in 1:36 min; zum Vergleich
    `ring` (Checkout um 02:04 vor der Umstellung, fast derselbe Code) 17.018.176 B in 1:43 min, also
    +1,48 MB. aws-lc-sys war beim Messlauf schon übersetzt; kalt kostet AWS-LC laut Spike oben rund
    8 s mehr.
  - Windows, `scripts/build-windows.sh`: Installer in 117 s (aws-lc-sys kalt übersetzt),
    `Vice Mail_0.1.0_x64-setup.exe` 7.296.549 B (der `ring`-Installer aus Stufe 6 hatte
    6.893.788 B, also +0,4 MB), `mail-app.exe` 26.414.592 B (Stufe 6: 25.337.344 B; dazwischen liegt
    auch neuer Code). aws-lc-sys baute mit `clang-cl` von cargo-xwin („Building with: CC“) und
    meldete „NASM command not found“, nahm also die vorab assemblierten Objekte. Kein zusätzliches
    Werkzeug.

**Ungeprüft:** die Laufzeit unter Windows (Plattform-Verifier über CryptoAPI mit aws-lc-rs, die vorab
assemblierten Objekte auf echter Hardware); öffentlich ausgestellte Zertifikate.

## Rückweg

Der Adapter steckt hinter dem Trait `MailBackend`, und die eigene Befehlsschicht nutzt von async-imap
nur Verbindung, Puffer und Parser. Ein Wechsel zu `imap-next` 0.3.4 und `imap-client` 0.3.1 (auf
`imap-codec`, vollständige Grammatik einschließlich QRESYNC und aller Antwortcodes) beträfe deshalb vor
allem die Übersetzung der Antworttypen in `mail-imap`, nicht Engine oder Speicher. Anlass dafür wären
Parserfehler bei echten Servern, die sich nicht durch kleinere Fenster umgehen lassen, oder eine
dauerhaft stehende Upstream-Entwicklung. Umgekehrt verringern die offenen PRs #146 (Status in
Streams) und #152 (QRESYNC) sowie die angekündigten strukturierten Fehler in 0.13 die eigene Schicht.
Beim Wechsel auf 0.13 lohnt eine erneute Prüfung.
