# Architektur

Stand 22.11.2026. Die Versionen sind die an diesem Tag aktuellen stabilen. Stufe 0 hat die riskanten
Annahmen mit Wegwerf-Code geprüft; Messwerte und Begründungen stehen in `docs/decisions/0001` bis `0005`.

## Überblick

```
React-Oberfläche (WebView)
   │   typisierte Commands und Events, einziger Weg
app/src-tauri            dünne Shell: Fenster, Commands, Verdrahtung
   │
mail-engine              Konten, Sync-Planung, Outbox, Threads, Kategorien, Regeln
   ├── mail-store        SQLite, Migrationen, FTS5, Abfragen
   ├── mail-imap         IMAP- und SMTP-Adapter, implementiert MailBackend
   ├── mail-content      Parsen, HTML-Bereinigung, Link-Prüfung, Dateinamen (rein, ohne IO)
   ├── webview2-guard    nur Windows: Navigationen im Mail-Frame abfangen (einzige Crate mit `unsafe`, 0011)
   └── mail-mcp          MCP-Server: Hilfsprogramm und Werkzeuge gegen `MailAccess` (0015)
   └── mail-model        Typen, Fehler, Capabilities, MailBackend-Trait, ohne IO
```

Später kommen der MCP-Server (Stufe 12), `mail-ai` (Stufe 13, optional) und bei Bedarf weitere Adapter (Stufe 14) dazu. Der
Einstellungs-Sync (Stufe 11) beginnt als Modul in `mail-engine` und wird erst ein eigenes Crate,
wenn er groß genug ist.

## Stack

| Bereich | Wahl | Version | Warum |
|---|---|---|---|
| Desktop-Shell | Tauri | 2.12 (Crate 2.12.7, CLI 2.12.6) | System-WebView statt mitgeliefertem Browser: kleine App, wenig Speicher, Rust-Kern; Spike-App: `.dmg` 2,2 MB, Windows-Installer 1,9 MB (0004) |
| Kern | Rust stable, Edition 2024, Tokio | mind. 1.98, geprüft 1.100 | Geschwindigkeit, Speichersicherheit; 1.98 verlangt mail-parser, 1.90 Tauri 2.12 (0006) |
| Oberfläche | React, TypeScript strict | 19.3.1 / 7.0.2 | größtes Ökosystem für die nötigen Bausteine. Typecheck mit TS 7 (Paket-Alias `typescript7`); `typescript` 6.0.3 nur für typescript-eslint, das die klassische TS-API braucht |
| Build | Vite, pnpm | 8.3 / 12.19 | schnell, Standard bei Tauri |
| Styling | Tailwind CSS mit eigenen Tokens | 4.3.5 | Tokens als CSS-Variablen, kein Laufzeit-CSS |
| Bausteine | Radix UI (`radix-ui`), Lucide-Icons (`lucide-react`), cmdk | 1.8.1 / 1.77 / 1.1 | zugängliche Grundbausteine ohne fremdes Aussehen. Achtung CSP: Radix Dialog, Popover, Select und ScrollArea fügen `<style>`-Elemente ein; vor ihrer Verwendung im Release-Build prüfen |
| Liste | TanStack Virtual (`@tanstack/react-virtual`) | 3.14.26 | Virtualisierung für sehr lange Listen; bei 10.000 Einträgen höchstens etwa 40 Zeilen im DOM. Liste als `listbox` mit `aria-activedescendant` (Stufe 2; mit Schnellaktionen in Stufe 4 neu entscheiden) |
| Daten im Frontend | TanStack Query, Zustand | 5.108.2 / 5.1.0 | Query für Daten aus dem Kern, Zustand für reinen UI-Zustand |
| Editor | TipTap | 3.35 | ausgereifter Rich-Text-Editor mit kontrollierbarem HTML |
| Sprachen | i18next, react-i18next | 27.2 / 18.0 | Standard, Englisch und Deutsch; Textliterale in Komponenten verbietet `eslint-plugin-i18next` (`no-literal-string`) |
| Lint | ESLint, typescript-eslint, eslint-plugin-react-hooks, eslint-plugin-i18next | 10.15 / 8.73 / 7.1 / 6.1 | `eslint-plugin-react` scheidet aus (kein Release seit April 2025, kein ESLint 10) |
| IDs | uuid | 1.29 | Konto-IDs (UUID v4) |
| Datenbank | SQLite über rusqlite (bundled, FTS5, Tokenizer `unicode61 remove_diacritics 2`) | 0.41.1 (SQLite 3.54.1, 0006) | ein Prozess, ein Schreiber: synchrones rusqlite ist einfacher und schneller als SQLx; bei 500.000 Mails Listenseite 0,04 ms, erste Suchtreffer unter 0,2 ms (0003) |
| Migrationen | rusqlite_migration | 2.7.0 | klein, nutzt `user_version`; legt rusqlite auf ^0.41 fest (0006) |
| IMAP | async-imap (Tokio, rustls) plus eigene schmale Befehlsschicht in `mail-imap` | 0.12.0 | im Dauereinsatz bei Delta Chat; Verbindung, Parser, IDLE kommen aus der Bibliothek. Status-Prüfung, Antwortcodes (APPENDUID, COPYUID, MODIFIED), ENABLE, QRESYNC, LIST-STATUS und modifiziertes UTF-7 über die eigene Schicht. 10.000 Kopfdaten in 0,044 s über TLS (0002) |
| SMTP | lettre (nur Protokollschicht, `smtp-transport` + `tokio1`) | 0.11.24 | TLS, Zeitlimits und STARTTLS macht der Adapter selbst wie bei IMAP (Plattform-Verifier); lettre ohne eigenes TLS |
| MIME lesen / schreiben | mail-parser / mail-builder | 1.0.1 / 2.0.1 | robust gegen kaputte Mails, vom Stalwart-Projekt |
| Body-Ablage | zstd (Stufe 3, mit Prüfsumme) | 0.14.4 | gleiches Verhältnis wie Deflate, 4- bis 20-mal schneller beim Packen; Dateien statt BLOBs, damit Datenbank und Migrationssicherungen klein bleiben (0009) |
| HTML-Bereinigung | ammonia, html5ever, cssparser, url, icu_normalizer/icu_properties | 4.2.5 / 0.40 / 0.38 / 2.5 / 2.4 | Allowlist-Sanitizer; CSS wird Token für Token neu geschrieben (kein Regex); Punycode nach RFC 3492 selbst dekodiert, `idna` direkt bewusst nicht (kein Release seit August 2025) |
| TLS | rustls mit Provider `aws-lc-rs`, tokio-rustls, Prüfung über den System-Speicher mit rustls-platform-verifier | 0.23 / 0.26 / 0.7, aws-lc-rs 1.19 | kein OpenSSL; der Standard-Provider von rustls, regelmäßig veröffentlicht, baut auch für Windows ohne CMake und NASM (vorab assemblierte Objekte aus dem Crate, 0002). Provider und Client-Konfiguration entstehen an einer Stelle (`mail_imap::tls`), auch für reqwest und die TLS-Server der Tests. Selbst signierte Server-Zertifikate funktionieren, wenn das System ihnen vertraut. Zusätzliche Wurzel-CA (`new_with_extra_roots`) nur für Tests gegen den Testserver |
| System öffnen, Dateidialog | open (`shellexecute-on-windows`), rfd | 5.4.5 / 0.18.0 | Links und Anhänge im System öffnen, Speichern-Dialog; beide nur in Rust, hinter Traits testbar. Nicht `tauri-plugin-dialog` (spritzt ein Script ein und bringt `tauri-plugin-fs` mit) |
| Benachrichtigungen, eine Instanz | tauri-plugin-notification, tauri-plugin-single-instance (nur Windows) | 2.8.1 / 2.5.5 | Stufe 9 (0020): native Mitteilungen nur aus Rust (keine Capability für die Oberfläche), Klicks über den Aktions-Handler des Plugins; auf Windows reicht ein zweiter Start (`mailto:`-Link) seine Argumente an die laufende App weiter. Beide Apache-2.0 OR MIT |
| WebView2-Zugriff | webview2-com (nur in `webview2-guard`) | =0.39.1 | `FrameNavigationStarting` abonnieren, das wry nicht anbietet (0011) |
| Schlüsselbund | keyring-core mit apple-native-keyring-store und windows-native-keyring-store | 1.1 / 1.0 / 1.1 | macOS-Schlüsselbund (login keychain) und Windows-Anmeldeinformationen; vom `keyring`-Projekt für Apps empfohlen, ohne globalen Standardspeicher und ohne Linux-D-Bus. Dienstname = Bundle-ID, Kontoname = Konto-UUID |
| Kontoerkennung | hickory-resolver, reqwest (rustls, ohne Default-Features), roxmltree | 0.26 / 0.13 / 0.21 | DNS-SRV, HTTPS für die Autokonfiguration der eigenen Domain, XML ohne DTD; reqwest wird später auch für externe Bilder und KI gebraucht |
| Typen über IPC | tauri-specta, specta, specta-typescript, exakt gepinnt | =2.0.0-rc.29 / =2.0.0-rc.28 / =0.0.15 | erzeugt Typen, Commands und Events; `u64` wird nicht still falsch typisiert; Export per Test, der bei veralteter Datei scheitert (0004). TypeScript-Typen werden nie von Hand gepflegt |
| Sync-Verschlüsselung | age (X25519, scrypt), hmac, sha2 | 0.11.5 / 0.13 / 0.11 | geprüftes Format, keine eigene Kryptografie; 0.11 statt 0.12 wegen einer Sicherheitsmeldung im P-256-Zweig von 0.12 (0014) |
| Zeitzone | jiff (`std`, `tz-system`, `tzdb-zoneinfo`, `tzdb-bundle-platform`) | 1.0.2 | Systemzone samt Sommerzeit für Datumsfilter der Suche, auf Windows mit mitgelieferter tzdb; MIT OR Unlicense (0017). `mail-content` nutzt nur die zivilen Datumstypen (Kalendereinladungen: Datum, Wochentag, Datumsrechnung); Zeitzonen löst dort die Engine über den Trait `ZoneResolver`, die tzdb wird in `mail-content` nur in Tests gelesen |
| Logging | tracing, tracing-subscriber, tracing-appender | 0.1.44 / 0.3.23 / 0.2.5 | strukturierte Logs, täglich rotierend (14 Dateien) im Log-Verzeichnis der App; `Secret<T>` schwärzt |
| Allgemein (Rust) | serde, serde_json, thiserror, tokio | 1.0 / 1.0 / 2.0 / 1.54 | Serialisierung, typisierte Fehler, Laufzeit |
| Tests | cargo test, Vitest, WebdriverIO mit `@wdio/tauri-service` und `tauri-plugin-wdio-webdriver` | – / 5.1 / 9.31, 1.5.1, =1.5.1 | eingebetteter WebDriver, nur mit Cargo-Feature `e2e`; läuft auf macOS (Spike D). Nicht `tauri-plugin-wdio` (wartet ohne `switchToWindow` je Befehl 5 s). WebDriver-Klicks sind synthetisch; echte Mausklicks über ein eigenes Test-Command. Desktop-Tests brauchen eine GUI-Sitzung und laufen nicht in `scripts/check.sh`, sondern als `pnpm --dir app test:e2e` |
| Testserver | Stalwart (ein Binary, IMAP+SMTP+JMAP), eingerichtet per Management-API durch `scripts/testmail.sh` | 1.1.1 | ohne Docker und sudo; IDLE, CONDSTORE, QRESYNC, MOVE, UIDPLUS, SPECIAL-USE, eigene Schlagwörter nachgewiesen (0001). Nur Entwicklungswerkzeug, wird nicht ausgeliefert |
| Windows-Build | cargo-xwin, LLVM, NSIS | 0.24.1 / 23.1 / 3.13 | Installer vom Mac aus, in Spike C gebaut (kalt 74 s); `makensis` nur mit UTF-8-Locale (`LANG=en_US.UTF-8`), siehe `docs/SETUP.md` |

Verworfen: SQLx (asynchron ohne Nutzen bei einem Schreiber, SQLCipher nur über Umwege), SQLCipher
(siehe Entscheidungen), Electron (Speicherbedarf), eingebettetes llama.cpp (Build-Aufwand pro Plattform),
eigener Sync-Server mit PostgreSQL.

## Repository

```
vice-mail/
├── app/                    Tauri-App
│   ├── src/                React: app/ features/ components/ services/ stores/ i18n/ styles/ config/
│   ├── src-tauri/          Shell, Commands, Capabilities
│   └── tests/              Vitest und WebdriverIO
├── crates/
│   ├── mail-model/
│   ├── mail-store/
│   ├── mail-imap/
│   └── mail-engine/
├── testmail/               Testserver-Konfiguration und Seed-Daten
├── spikes/                 Wegwerf-Code aus Stufe 0, nie von Produktcode importiert
├── scripts/
├── docs/
├── Cargo.toml              Workspace
└── CLAUDE.md
```

## Modulgrenzen

- `mail-model`: IDs, Konto, Postfach, Nachricht, Flags, Capabilities, Fehlertypen und der Trait
  `MailBackend`. Kein IO, keine Abhängigkeit auf andere eigene Crates.
- `mail-store`: einzige Stelle mit SQL. Ein Schreib-Thread, ein Pool von Lese-Verbindungen, WAL.
  Kein Datenbankaufruf blockiert einen Tokio-Worker.
- `mail-imap`: spricht IMAP und SMTP, liefert Modelltypen. Kennt weder Datenbank noch Oberfläche.
- `mail-content`: macht aus Rohdaten eine angezeigte Mail: Parsen, HTML-Bereinigung mit Pflicht-CSP,
  Link-Platzhalter, Link-Warnungen, sichere Dateinamen, Vorschautext. Rein, ohne IO, ohne Tokio;
  sicherheitskritisch und mit eigenem Korpus getestet (deshalb ein fünftes Crate).
- `mail-engine`: verbindet Adapter und Speicher. Bietet der Shell eine Fassade `AppCore` und einen
  Strom typisierter Ereignisse. Alles Verhalten ist über `AppCore` ohne WebView testbar.
- `app/src-tauri`: reicht Commands an `AppCore` durch und Ereignisse an die Oberfläche weiter.
- Oberfläche: fragt Daten seitenweise ab und lädt neu, wenn ein Ereignis die Ansicht betrifft. Sie pollt nie.
  Die von tauri-specta erzeugten Bindings (`app/src/generated/`) und `@tauri-apps/api` werden nur in
  `app/src/services/ipc.ts` importiert (ESLint `no-restricted-imports`); `invoke` steht nur in der
  erzeugten Datei.

**Regeln für die Brücke (0004)**
- Jedes `u64`/`i64`-Feld legt fest, wie es in TypeScript ankommt: als Zahl, wenn es sicher unter
  2^53 bleibt (Größen, UIDs, Zeitstempel in Millisekunden), sonst als dezimaler String (MODSEQ).
- Fehler des Frameworks (unbekannter Command, falsches Argument, ACL) kommen als String an, nicht als
  `IpcError`. `ipc.ts` erkennt das und bildet es auf `code: "internal"` ab.
- Ereignisse empfängt die Oberfläche über die Capability `core:event:default`; weitere Capabilities nur mit Begründung.

`MailBackend` meldet über eine `Capabilities`-Struktur, was der Server kann (IDLE, CONDSTORE, MOVE,
UIDPLUS, SPECIAL-USE, eigene Schlagwörter). Die Engine und die Oberfläche fragen danach. Was ein
Server nicht kann, wird nicht vorgetäuscht, sondern ausgeblendet oder mit einem klaren Hinweis versehen.

## Datenmodell

| Tabelle | Inhalt | Klasse |
|---|---|---|
| `account` | UUID, Typ, Adresse, Name, Farbe, Servereinstellungen, Verweis auf Schlüsselbund, `writes_enabled` | sync (ohne Geheimnisse) |
| `identity` | Absenderadresse, Name, Reply-To, Signatur, Standard | sync |
| `mailbox` | Servername, Trenner, Eltern, Rolle (Inbox, Sent, Drafts, Archive, Trash, Junk, All, Custom), UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ | Cache |
| `message` | interne ID, Konto, Message-ID-Kopfzeile, Thread, Betreff, Absender, Empfänger, Daten, Größe, Vorschau, Kategorie (nach den Regeln des Nutzers; dazu das automatische Ergebnis mit Begründung, Wichtig-Markierung, normalisierte Absenderadresse, Domain und List-Id, Stufe 9), Verweis auf den Body | Cache |
| `message_location` | Nachricht, Postfach, UID, Flags, Schlagwörter, MODSEQ | Cache |
| `thread` | Konto, normalisierter Betreff, letzte Nachricht, Anzahl, ungelesen | Cache |
| `attachment` | Nachricht, MIME-Teil, Dateiname, Typ, Größe, Content-ID | Cache |
| `message_body`, `message_body_part` | Verweis auf die komprimierte Body-Datei (ganze Mail oder einzelne Teile großer Mails), Größe, Prüfsumme | Cache |
| `remote_content_sender` | Absender, deren externe Bilder immer geladen werden (Konto-UUID, normalisierte Adresse) | sync |
| `message_fts` | FTS5-Index über Betreff, Absender, Empfänger, Text, Anhangnamen | Cache |
| `operation` | Outbox: Art, Nutzdaten, Idempotenzschlüssel, Zustand, Versuche, letzter Fehler | Gerät |
| `draft` | UUID, Konto, Identität, Kopfzeilen, Message-ID (ab Anlage fest), Text, Zitat, Signatur ja/nein, Anhänge als Verweise auf eigene Kopien unter `compose/`, Zustand (`editing`/`sending`), Revision, Server-Kopie (Ordner, UIDVALIDITY, UID, Revision) (0018) | Gerät (Abgleich über IMAP) |
| `contact` | eine Zeile je Adresse: Schlüssel, Schreibweise, Zähler, zuletzt benutzt, Rang (REAL mit Index), Namen, `user_name`, gefaltete Suchwörter (0016, 0018) | Cache; von Hand bearbeitete Einträge sync |
| `signature`, `template`, `rule`, `saved_view` | wie benannt; `rule` sind die lokalen Regeln (Bedingungen und Aktionen als JSON, Reihenfolge) | sync |
| `sender_rule` | Korrektur „immer als …“ und Wichtig-Markierung je Adresse, Domain oder Liste (Stufe 9) | sync |
| `snooze`, `pin`, `task` | Verweis auf die Mail über Konto-UUID und Message-ID, Zeitpunkt; Snooze mit Rückkehrzeit, Aufgabe mit Erledigt-Zeitpunkt (Stufe 9, 0020) | sync |
| `setting` | Schlüssel, Wert, Geltungsbereich, Version | sync, soweit als synchronisierbar registriert |
| `ai_profile` | Anbieter, Basis-URL, Modell, Freigaben | sync (ohne Schlüssel) |
| `sync_record` | zuletzt abgeglichene Version je Datensatz | Gerät |

Klassen: **Cache** lässt sich vom Mailserver neu aufbauen. **Gerät** bleibt auf dem Gerät.
**sync** wandert über den Einstellungs-Sync. Eine Tabelle wird nur abgeglichen, wenn sie
ausdrücklich als synchronisierbar registriert ist.

Die gleiche Mail in zwei Ordnern ist eine Zeile in `message` und zwei in `message_location`.
Flags gehören zum Ort, wie bei IMAP. Datensätze, die zwischen Geräten wandern, verweisen auf Mails
nie über lokale IDs oder UIDs, sondern über Konto-UUID und Message-ID-Kopfzeile.

Migrationen sind nur vorwärts. Vor jeder Migration wird die Datenbankdatei kopiert; die letzten drei
Kopien bleiben erhalten. Eine neuere Datenbank als die App kennt wird nicht geöffnet, sondern mit
einer klaren Meldung abgelehnt.

**Schema und Abfragen (gemessen in Spike B, `docs/decisions/0003-sqlite-schema-und-suche.md`)**
- `message.id` ist das Sortierdatum in Sekunden; ist die Zahl belegt, die nächste freie. Vergeben
  wird sie im Schreib-Thread. Sie ist zugleich die rowid des FTS-Index: Suchtreffer kommen ohne
  Sortieren neueste zuerst, ein Datumsfilter ist ein rowid-Bereich.
- Sortierdatum: die `Date`-Kopfzeile, wenn sie höchstens 24 Stunden nach INTERNALDATE liegt, sonst INTERNALDATE.
- `message_location` ist `WITHOUT ROWID` mit Schlüssel `(mailbox_id, message_id)`, Flags als eigene
  Spalten. Indizes: eindeutig auf UID je Postfach, auf `message_id`, Teilindex auf ungelesene
  (`WHERE seen = 0`); dazu `message` nach Message-ID und Thread, `attachment` nach Nachricht.
- `message_fts` ist contentless mit `contentless_delete=1`, Spalten `subject, from, to, cc, body,
  attachments`, Tokenizer `unicode61 remove_diacritics 2`. Bodies werden nachträglich per
  `INSERT OR REPLACE` indexiert. Teilwortsuche („auszug“ findet „Kontoauszug“) gibt es in Version 1 nicht.
- Listen nur per Keyset, nie per `OFFSET`. Ansichten über mehrere Postfächer (vereinter Posteingang)
  per `UNION ALL` je Postfach, nie per `IN (…)` (gemessen 0,14 ms gegen 393 ms). Feste
  Join-Reihenfolge per `CROSS JOIN`. Für jede Listen- und Suchabfrage prüft ein Test den Abfrageplan.
- Erste Suchtreffer nach `rowid DESC`; `bm25` höchstens als Option.
- Aus Stufe 2: Der Abfrageplaner wählt nach `ANALYZE` mit nur einem Konto für einen Join auf
  `account` einen Scan, trotz `CROSS JOIN`; das Konto eines Postfachs wird deshalb per skalarer
  Unterabfrage geholt. Lese-Verbindungen laden Statistiken erst beim Öffnen, Plan-Tests prüfen daher
  Leser und Schreiber. Ein Fenster wird nach (Sortierdatum, UID) aufsteigend gespeichert. Zusätzliche
  Indizes: `mailbox_role` (eindeutig je Konto, wenn gesetzt), `mailbox_path` (Reihenfolge der
  Seitenleiste). Gemessen über die Store-API: 100.000 Kopfdaten in 1,1 s, Listenseite 0,07 ms.
- FTS-Zeilen werden je Transaktion nach rowid aufsteigend eingefügt, auch wenn der Abgleich von neu
  nach alt läuft (absteigend kostet ein Drittel der Einfügerate).
- Schreib-Verbindung: WAL, `synchronous=NORMAL`, `foreign_keys=ON`, 64 MB Cache, `mmap_size` 256 MB,
  `journal_size_limit` 64 MB, `temp_store=MEMORY`. Lese-Verbindungen (2 bis 4): `query_only`,
  8 MB Cache, `mmap`. Abgleich in Fenstern von 500 Mails mit `BEGIN IMMEDIATE`.
- Aus Stufe 7 (`docs/decisions/0017-suche-threads-integration.md`): Index und Thread werden in der
  Transaktion geschrieben, die Nachricht bzw. Body speichert (Body-Text über eine `BodyTextSource` der
  Engine, höchstens 256 KiB je Mail); Bestandsmails und ein Neuaufbau laufen als Auftrag im
  Hintergrund, der Neuaufbau mit Tausch am Ende. `thread` hält Anzahl und neueste Nachricht, die
  Ungelesenen zählt die Thread-Liste beim Lesen; die Thread-Liste eines Postfachs zeigt je Thread die
  neueste Mail in diesem Postfach. Hervorhebungen rechnet Rust (contentless Index). Kontogruppen
  (sync, über Konto-UUID) und ausgeblendete Konten (Gerät) in eigenen Tabellen. Gemessen in
  `docs/testing/performance.md`: erste Suchtreffer bei 500.000 Mails 0,3–17 ms, Thread-Liste 0,3 ms;
  ein Abgleichsfenster kostet 4,7 ms mehr als in Stufe 2.
- Aus Stufe 9 (`docs/decisions/0020-posteingang-komfort.md`): Neue Mails werden in der
  Transaktion eingeordnet, die sie speichert (Regeln aus `mail-content` über einen `Classifier` der
  Engine, lokale Signale aus der Datenbank); geänderte Absender-Regeln und Bestandsmails laufen als
  Auftrag im Hintergrund, neueste zuerst. Kategorie und Wichtig-Markierung stehen per Trigger auch
  am Ort (`loc_category`, Teilindex `loc_important`), damit Listen nach Kategorie per Keyset laufen.
  Snooze, Pins und Aufgaben verweisen über Konto-UUID und Message-ID; die Posteingangsliste lässt
  sie je Zeile über den eindeutigen Index weg. Gemessen: Einordnen kostet je Fenster von 500
  Kopfdaten 2,5 ms; Listenseiten bleiben unter 0,3 ms.

## Regeln der Mail-Engine

**Synchronisierung je Postfach**
- Erster Abgleich von neu nach alt in Fenstern, damit der Posteingang sofort benutzbar ist.
- Danach: neue UIDs oberhalb der letzten bekannten; Flag-Änderungen über CONDSTORE, sonst über
  fensterweises Nachladen; Löschungen über den Vergleich der UID-Mengen.
- Ändert sich UIDVALIDITY, wird die Zuordnung des Postfachs verworfen und neu aufgebaut. Lokale
  Daten zu Mails bleiben über die Message-ID erhalten.
- IDLE auf dem Posteingang jedes Kontos, andere Ordner im Intervall. Nach Ruhezustand oder
  Netzwechsel neu verbinden, mit exponentiellem Backoff und Abbruchmöglichkeit.
- Es werden nie alle Daten neu geholt, wenn ein gezielter Abgleich reicht.

**Abgleich-Tasks (Stufe 2, 0008)**
- Je Konto ein Aufseher-Task mit dem Zustandsautomaten (`sync_state.rs`). Verbunden: Arbeiter
  (Hauptverbindung, Durchläufe), IDLE-Wächter (eigene Verbindung, Abschnitte von 5 Minuten),
  Abfrager (voller Durchlauf alle 3 Minuten, ohne IDLE der Posteingang jede Minute). Eine
  ausgefallene Verbindung beendet die Sitzung; der Automat entscheidet über Backoff oder Stopp.
- Ein Durchlauf je Postfach: EXAMINE, bei neuer UIDVALIDITY `reset_mailbox`; unverändert (UIDNEXT,
  HIGHESTMODSEQ, Anzahl gleich) wird übersprungen; sonst UID-Abgleich für Löschungen, Flags per
  `CHANGEDSINCE` oder fensterweise, neue UIDs bis zum UIDNEXT des EXAMINE, dann der Rest des
  Erstabgleichs. Verwaiste Nachrichten werden nur nach einem vollen Durchlauf ohne Erstabgleich und
  ohne übersprungenes Postfach gelöscht.
- Ein Panic in einer Sitzung wird zum Kontofehler `protocol` mit Backoff, nie zum App-Ende.
- Ruhezustand: Sprung zwischen Wanduhr und monotoner Uhr (über 20 s) lässt alle Konten neu
  verbinden; dazu `AppCore::resume()`/`network_changed()` für die Shell. Ein OS-Signal gibt es
  nicht (bräuchte `unsafe`).
- Ereignisse: `accountStatusChanged` trägt immer den vollständigen Status; je gespeichertem
  Fenster ein `messagesChanged` und ein `mailboxesChanged`; `newMail` nur außerhalb des
  Erstabgleichs, mit der Zahl neuer ungelesener Mails.

**Outbox**
- Jede schreibende Aktion ändert zuerst den lokalen Zustand, wird als `operation` vermerkt und
  dann ausgeführt. Zustände: `queued`, `in_flight`, `confirmed`, `failed`, `conflict`.
- Operationen sind idempotent formuliert: „Flag setzen“ statt „Flag umschalten“; ein Verschieben
  prüft vor der Wiederholung, ob die Mail schon am Ziel liegt.
- Nach einem Absturz werden `in_flight`-Operationen erst gegen den Server geprüft, dann wiederholt.
- Schlägt etwas fehl, wird der lokale Zustand zurückgenommen und der Fehler angezeigt. Bei einem
  Konflikt gilt der Serverzustand, und der Nutzer erfährt es.

**Senden**
- Die Message-ID entsteht beim Anlegen des Entwurfs und bleibt über alle Versuche gleich.
- Bricht die Verbindung ab, nachdem die Mail übertragen, aber bevor sie bestätigt wurde, wird nicht
  automatisch neu gesendet. Die App sucht die Message-ID im Gesendet-Ordner; bleibt es unklar, fragt sie den Nutzer.
  Achtung: Dort liegt die Mail nur, wenn der Server selbst eine Kopie ablegt (z. B. Gmail); bei allen
  anderen findet die Suche nichts, und die App fragt. Der SMTP-Adapter unterscheidet „nichts
  übertragen“ (sicher zu wiederholen) von „unklar“ (Verbindung nach der Endmarke weg).
- Die erzeugte Nachricht verrät das Programm nicht (kein `User-Agent`/`X-Mailer`, EHLO mit
  `[127.0.0.1]`); `Date` in UTC; derselbe Entwurf ergibt byte-gleiche Nachrichten.
- Nach erfolgreichem Versand wird eine Kopie in „Gesendet“ abgelegt, außer der Server tut das selbst.
- Ab Stufe 5 (0018): Senden ist die Outbox-Operation `send`. Die Bytes werden beim Klick einmal
  gebaut und eingefroren, der Entwurf ist bis zum Ergebnis gesperrt; `not_before` hält die
  Verzögerung zum Rückgängigmachen (Abbrechen löscht die Operation, nichts geht ins Netz).
  Unklar heißt `failed` mit `send_unclear` und eine Frage an den Nutzer (`sendNeedsDecision`);
  erneut gesendet wird nur auf seine ausdrückliche Entscheidung. Nach der Annahme folgen in
  derselben Transaktion Gesendet-Kopie (`append`), Entfernen der Entwurfskopie (`remove_copy`)
  und `\Answered`/`$Forwarded` am Original. Entwürfe gehen als `append` in den Entwürfe-Ordner
  (neue Fassung anhängen, alte nur bei passender Message-ID entfernen).

**Verbindungen (0002)**
- Jede Verbindung hat Lese- und Schreibfristen (Standard 30 s); eine stille Verbindung wird erkannt, nicht abgewartet.
- Anmeldung per LOGIN; enthalten Benutzername oder Passwort Nicht-ASCII-Zeichen, per
  `AUTHENTICATE PLAIN` (UTF-8, mit SASL-IR, wenn angeboten), siehe 0008.
- Nach dem Login `ENABLE CONDSTORE`, bei QRESYNC-fähigen Servern `ENABLE QRESYNC`.
- Ein abgebrochener Befehl schließt die Verbindung; sie wird nie mit halb gelesenen Antworten weiterbenutzt.
- Unaufgeforderte Antworten werden nach jedem Befehl abgeholt (die Bibliothek puffert nur 100).
- Zertifikatsfehler werden typisiert: nicht vertraut, falscher Name, abgelaufen.

**Nur-Lese-Modus**
- Konten mit `writes_enabled = false` öffnen Postfächer mit EXAMINE. Der Adapter lehnt jeden
  schreibenden Befehl mit einem typisierten Fehler ab. Ein Test beweist das.
- Auch bei freigeschaltetem Schreibzugriff verbinden sich Abgleich, IDLE, Lesebereich und
  Verbindungstest immer nur lesend (EXAMINE, PEEK); geschrieben wird ausschließlich über die eigene
  Verbindung der Outbox (Mitschnitt-Test in `crates/mail-engine/tests/actions.rs`).
- Nach jedem Schritt der Outbox stößt ein Hook sofort einen Abgleich genau der betroffenen Ordner an;
  die Oberfläche bekommt je Änderung ein gebündeltes Ereignis `operationsChanged`.

**Offline**
- Kopfzeilen aller Mails liegen lokal. Bodies werden beim Öffnen geholt und für einen einstellbaren
  Zeitraum im Voraus geladen (Standard 30 Tage für Posteingang, Gesendet, Entwürfe und Markiertes).
- Bodies liegen zstd-komprimiert als Dateien unter `bodies/<Konto-UUID>/` im Datenverzeichnis der
  App, atomar geschrieben, die Datenbank hält den Verweis (gemessen in 0009: Lesen gleich schnell wie
  BLOB, Datenbank 2,3 MB statt 57 MB bei 460 Bodies). Kleine vorausgeladene Mails eines Kontos werden
  in Paketen bis 1 MiB geschrieben. Mails über 2 MiB werden teilweise geholt: Kopf und Textteile
  zuerst, Anhänge auf Wunsch. Fehlt eine Datei oder ist sie beschädigt, wird sie neu geholt.

**Threads**
- Je Konto, über Message-ID, In-Reply-To und References. Rückfall auf den Betreff nur vorsichtig:
  gleicher normalisierter Betreff, gemeinsame Teilnehmer, enges Zeitfenster.

## Sicherheitsgrenzen

**HTML aus Mails**
0. Eine lineare Vorprüfung weist HTML ab, das html5ever quadratisch langsam machen oder die Ausgabe
   aufblähen würde (mehr als 150.000 Tags, Tiefe über 1500, zu viele offene Formatierungs-Tags);
   solche Mails werden als Text gezeigt.
1. In Rust mit ammonia bereinigen: kein Script, keine Ereignis-Attribute, keine Formulare, kein
   `meta refresh`, kein `base`, keine eingebetteten Objekte oder Frames.
2. Externe Adressen in Bildern, `srcset`, Hintergründen und CSS durch Platzhalter ersetzen, solange
   der Nutzer sie nicht freigegeben hat. Inline-Bilder (`cid:`) kommen aus dem lokalen Speicher.
3. Anzeige in einem `srcdoc`-`iframe` mit `sandbox="allow-same-origin"` und sonst nichts, nie
   `allow-scripts` (mit beidem zusammen erreichte ein Script in Spike D `parent.__TAURI_INTERNALS__`).
   Jedes `srcdoc` beginnt mit der Pflicht-Meta-CSP `default-src 'none'; img-src data:; style-src
   'unsafe-inline'; base-uri 'none'; form-action 'none'` und `<meta name="referrer"
   content="no-referrer">`. Bilder nur als `data:` mit vom Kern erkannten Rasterformaten, nie SVG. Das Frame erbt zusätzlich die CSP der App; WebKit kennt das
   `csp`-Attribut nicht. Dort läuft kein JavaScript, und Tauri spritzt seine Scripts nur in den
   Hauptframe ein, also ist die Tauri-Schnittstelle nicht erreichbar (0005). Die Höhe des Frames
   misst das Elternfenster: `contentDocument` ist lesbar (same-origin), ein `ResizeObserver` des
   Elternfensters auf dem Wurzelelement im Frame feuert unter WKWebView (Stufe 3 geprüft); nachgemessen
   wird per `setTimeout`, nicht per `requestAnimationFrame` (läuft in verdeckten Fenstern nicht).
4. Freigegebene externe Bilder lädt der Rust-Kern: ohne Cookies, ohne Referrer, mit neutraler
   Kennung, mit Größen- und Typgrenzen.
5. Links öffnen sich nie direkt. Ein Klick zeigt das echte Ziel und warnt bei Abweichung vom
   Linktext oder bei verdächtigen Domains. Geöffnet wird im System-Browser, nur `http`, `https`, `mailto`.
   Einziger Abfangpunkt ist der Navigations-Hook in Rust (`on_navigation`): WebKit ruft in einem Frame
   ohne `allow-scripts` keine Event-Listener auf, auch keine vom Elternfenster. Der Bereiniger schreibt
   deshalb jedes `href` auf `https://link.invalid/<Render-Token>/<Index>` um (das echte Ziel kennt nur
   der Kern; ein Platzhalter mit fremdem Token aus der Mail selbst wird ebenfalls umgeschrieben)
   und entfernt `<link>`, `target`, `ping` und `download`: `<link rel=preconnect>` und Klicks auf
   `target=_blank` öffnen sonst TCP-Verbindungen, die weder Sandbox noch CSP verhindern (0005).
6. Die CSP der App erlaubt für die Mail-Anzeige `style-src 'self' 'unsafe-inline'` (sonst greift kein
   Mail-CSS) und `frame-src 'self' http: https:` (sonst verwirft WebKit Link-Navigationen, bevor der
   Hook sie sieht); Scripts bleiben auf `'self'` beschränkt. Unter WebView2 ist offen, ob der Hook
   Navigationen im Unter-Frame sieht (wry abonniert dort nur `NavigationStarting`); Stufe 6 prüft das.

**Shell**
- Das Hauptfenster entsteht erst, wenn der Kern bereitsteht (`"create": false` in `tauri.conf.json`); so kann die Oberfläche keine Commands senden, bevor `AppCore` läuft.
- `devCsp` greift im Entwicklungsmodus nicht (die WebView lädt die `devUrl` direkt). CSP-Fehler zeigen sich erst im Release- oder e2e-Build; der Desktop-Test zeichnet CSP-Verstöße auf und scheitert daran.
- Tauri-Capabilities minimal: keine Dateisystem-, Shell- oder HTTP-Plugins für das Frontend.
  Das Notification-Plugin (Stufe 9) benutzt nur die Shell; die Oberfläche hat keine Berechtigung
  dafür. Den Standard für `mailto:` fragt und setzt die Shell auf macOS über `osascript` mit festem
  Skript (Launch Services ohne `unsafe`, 0020).
- Strenge Content-Security-Policy für die App selbst, keine entfernten Quellen.
- Der eingebettete WebDriver für Tests ist nur in Test-Builds enthalten, nie in ausgelieferten
  (Cargo-Feature `e2e`; ein Test prüft, dass das Release-Binary ihn nicht enthält, 0005).
- Tauri-Init-Scripts werden nie für alle Frames eingespritzt.
- `unsafe` ist überall verboten (`forbid`), außer in `crates/webview2-guard` (`deny` mit einzeln
  begründeten `#[allow(unsafe_code)]` und `// SAFETY:`); `scripts/check.sh` prüft das per Suche.
- Neue Fenster (`target=_blank`, `window.open`) lehnt die Shell immer ab.

**Geheimnisse**
- Passwörter, Tokens und API-Schlüssel nur im System-Schlüsselbund. Ein Typ `Secret<T>` verhindert,
  dass sie in Logs oder Fehlermeldungen landen; ein Test prüft das.
- Konto entfernen löscht Schlüsselbund-Eintrag, lokale Daten und Dateien dieses Kontos.

**Dateien**
- Dateinamen von Anhängen werden bereinigt: keine Pfadtrenner, keine reservierten Windows-Namen,
  keine Steuerzeichen, keine Richtungsumkehr-Zeichen. Geschrieben wird nur an den Ort, den der
  Nutzer im Systemdialog wählt.

**Erlaubte Netzverbindungen**
- Mailserver, die der Nutzer eingerichtet hat, auch für Bodies und Anhänge (Stufe 3): je Konto eine
  eigene Leseverbindung neben Abgleich und IDLE, immer nur lesend (EXAMINE, `BODY.PEEK[…]`)
- Bei der Einrichtung, nur nach Klick auf „online suchen“: DNS-SRV-Abfragen (RFC 6186) über die
  System-DNS-Server für die Domain der Adresse, danach HTTPS (Port 443) zur Autokonfiguration der
  eigenen Domain (`autoconfig.<domain>`, `<domain>/.well-known/autoconfig/`). Weiterleitungen nur auf
  dieselbe Domain oder ihre Subdomains, höchstens drei, ohne Cookies, Referrer und Proxy, 128 KiB
  höchstens. Kein Mozilla-ISPDB, kein anderes fremdes Verzeichnis. Gefundene Server werden vor der
  Bestätigung angezeigt (SRV und Autoconfig sind nicht DNSSEC-geprüft).
- Externe Bilder einer Mail, nur nach Freigabe („einmal laden“ oder „für diesen Absender immer“).
  Der Kern lädt sie selbst (`mail-engine/src/reading/remote.rs`): reqwest über rustls mit der
  Zertifikatsprüfung des Systems, HTTP/1.1, ohne Cookies, Referrer, Proxy und Wiederholungen, mit
  neutraler Kennung (`Mozilla/5.0`), nur `http`/`https`, Weiterleitungen von Hand (höchstens drei,
  wieder nur `http`/`https`), 5 s Verbindungsaufbau, 15 s je Bild, 30 s je Mail, höchstens 200
  Adressen, 8 MiB je Bild und 32 MiB je Mail. **Nie zu privaten, Loopback- oder Link-Local-Adressen**
  (auch nicht zu CGNAT, Multicast, reservierten und Dokumentationsnetzen, IPv4 in IPv6 eingeschlossen):
  IP-Literale werden vor der Anfrage geprüft, Namen nach der Auflösung, und die Verbindung nutzt nur
  die geprüften Adressen. Eine Ausnahme nur für Loopback gibt es allein in Tests
  (`RemoteConfig::allow_loopback`; im Desktop-Test-Build `MAIL_APP_E2E_LOOPBACK_IMAGES=1`, nur mit
  Feature `e2e`). Grenze: Die `From`-Adresse ist fälschbar; wer einen freigegebenen Absender fälscht,
  bekommt seine Zählpixel geladen, mehr nicht
- Links und Anhänge öffnet nicht die App, sondern das System (Browser, Mail-Programm, Standard-App),
  erst nach Klick und Prüfung, Links nur `http`, `https`, `mailto`
- KI-Endpunkte, die der Nutzer eingerichtet und freigegeben hat (Stufe 13)
- Der MCP-Server nimmt nur lokale Verbindungen an (stdio bzw. `127.0.0.1`) und baut selbst keine auf
- Abmelden (Stufe 9, 0020), nur nach Klick: Ein-Klick nach RFC 8058 als HTTPS-POST
  (`List-Unsubscribe=One-Click`) an die `https`-Adresse aus `List-Unsubscribe`, nur wenn
  `List-Unsubscribe-Post` es anbietet; Client wie bei externen Bildern (gemeinsame TLS-Konfiguration,
  ohne Cookies, Referrer, Proxy und Wiederholungen, neutrale Kennung, nie zu privaten, Loopback- oder
  Link-Local-Adressen), keine Weiterleitungen, 5 s Aufbau, 15 s gesamt. Eine `mailto:`-Abmeldung ist
  ein Entwurf, den der Nutzer sendet; eine Web-Abmeldung öffnet nur der System-Browser
- Update-Prüfung ab Stufe 15, abschaltbar
- Die Zertifikatsprüfung des Betriebssystems kann für öffentliche Zertifikate Sperrinformationen
  (OCSP, CRL) bei der ausstellenden Stelle abfragen. Das geschieht im System, nicht in der App.

Sonst nichts. Keine Telemetrie, keine Absturzberichte, keine Abfrage fremder Verzeichnisse ohne Klick.

## Einstellungs-Sync

Der Mailserver gleicht Mails, Ordner, Flags und Entwürfe ab. Der Einstellungs-Sync gleicht nur ab,
was allein die App kennt, und nutzt dafür das Postfach selbst als Ablage.

- **Ablage:** ein eigener Ordner in einem vom Nutzer gewählten Konto („Sync-Konto“), nicht abonniert,
  damit die meisten anderen Mail-Programme ihn nicht anzeigen. Jeder Datensatz ist eine Nachricht.
- **Verschlüsselung:** Format `age`. Beim Einrichten entsteht ein zufälliger Schlüssel; er liegt
  selbst mit der Passphrase verschlüsselt im Ordner. Datensätze werden an diesen Schlüssel
  verschlüsselt. Kopfzeilen tragen nur eine undurchsichtige ID und eine Version; Art und Inhalt
  stehen im verschlüsselten Teil und werden beim Lesen gegen die Kopfzeilen geprüft.
- **Ändern:** neue Version anhängen, alte löschen. Liegen zwei Versionen vor, entscheidet die
  Zusammenführungsregel der Datenart, das Ergebnis wird geschrieben, die Verlierer gelöscht.
- **Versionen:** hybride logische Uhr je Datensatz (Zeit, Zähler, Geräte-ID).
- **Löschen:** Grabstein-Datensatz, 180 Tage aufbewahrt, damit lange abwesende Geräte davon erfahren.
- **Regeln je Datenart:**

| Datenart | Regel |
|---|---|
| Einstellungen | je Schlüssel gewinnt die neuere Version |
| Signaturen, Vorlagen, Regeln, Ansichten | je Datensatz gewinnt die neuere Version; Bearbeiten schlägt gleichzeitiges Löschen |
| Snooze, Pins | je Mail gewinnt die neuere Version |
| Kontenliste | je Feld gewinnt die neuere Version; ein entferntes Konto wird auf anderen Geräten nicht still gelöscht, sondern der Nutzer wird gefragt |

- **Neues Gerät:** Sync-Konto einrichten, Passphrase eingeben, Vorschau der zu übernehmenden Daten,
  zusammenführen. Übernommene Konten fragen einzeln nach ihrem Passwort.
- **Passphrase vergessen:** nichts geht verloren, jedes Gerät hat seine Daten lokal. Sync zurücksetzen,
  neuer Schlüssel, von diesem Gerät neu hochladen.
- **Sichtbar für den Mailserver:** Anzahl, Größe und Zeitpunkte der Datensätze. Mehr nicht.
- **Transport austauschbar:** ein Trait `SettingsTransport` (auflisten, schreiben, löschen). Erste
  Umsetzung ist der IMAP-Ordner; ein selbst gehosteter Server oder ein Dateiordner können folgen.

## KI

Zwei Wege, beide aus, bis der Nutzer sie einschaltet (`docs/decisions/0010-ki-ueber-mcp.md`):

**MCP-Server (Stufe 12, Hauptweg).** Die App bietet einen lokalen MCP-Server an. Jeder MCP-fähige
KI-Client (Claude Desktop, Claude Code, Cursor, LM Studio …) kann freigegebene Konten durchsuchen,
Threads lesen und Entwürfe anlegen. Die App enthält dafür keinen Anbieter und keinen Schlüssel.
- Nur lokal (0015): Der Client startet das mitgelieferte Hilfsprogramm `mail-mcp` über stdio; es spricht
  mit der laufenden App über einen Unix-Socket (0600, Verzeichnis 0700, Prüfung der Peer-UID) bzw. eine
  Windows-Named-Pipe nur für den Benutzer, mit HMAC-Challenge-Response je Client. Nie TCP (kein
  DNS-Rebinding, kein Zugriff anderer lokaler Prozesse über einen Port).
- Je Konto: unsichtbar, lesen, lesen und Entwürfe. Standard: unsichtbar.
- Kein Werkzeug sendet, löscht, verschiebt, markiert oder ändert Einstellungen.
- Werkzeuge liefern bereinigten Text (kein rohes HTML, keine externen Bilder, Anhänge nur mit Namen),
  als nicht vertrauenswürdige Daten gekennzeichnet, mit Größen- und Mengengrenzen.
- Jeder Zugriff ist in der App sichtbar; Clients einzeln widerrufbar, Not-Aus für alle.
- Grenze, ehrlich benannt: Der Client hat eigene Werkzeuge, und Mailinhalte können ihn mit
  eingeschleusten Anweisungen manipulieren. Die App begrenzt, was er bekommt und tun darf; was ein
  Cloud-Client mit gelesenen Mails macht, liegt außerhalb der App.

**KI in der App (Stufe 13, optional).** Für Funktionen in der Oberfläche (Zusammenfassen,
Antwortvorschläge, Schreibhilfe, Suche in natürlicher Sprache) ein Trait `AiProvider` mit einer
Umsetzung über die OpenAI-kompatible Chat-Schnittstelle mit frei wählbarer Basis-URL (Ollama,
LM Studio, llama.cpp-Server, OpenRouter, OpenAI) und eine zweite für die Anthropic Messages API: Über
die kompatible Schnittstelle sind Ablehnungen nicht erkennbar (0019). Crate `mail-ai`, hängt nur von
`mail-model` und `mail-content` ab; die TLS-Konfiguration wird aus `mail_imap::tls` hineingereicht.
- Mail-Inhalte sind Daten, keine Anweisungen: fester Systemtext im Code, Mailtext klar abgegrenzt im Nutzerteil.
- Das eingebaute Modell bekommt keine Werkzeuge. Es kann also strukturell nichts senden, löschen oder abrufen.
- Ausgaben werden als Text angezeigt. Bilder aus KI-Ausgaben werden nie geladen, Links laufen durch
  dieselbe Prüfung wie Links in Mails.
- Gesendet wird nur, was die Anfrage braucht: der aktuelle Thread ohne doppelte Zitate und ohne
  Anhänge. Nie das ganze Postfach ohne ausdrückliche Bestätigung mit Mengenangabe.

## Tests

- **Unit:** Parser, Normalisierung, Threads, Suchsprache, Zusammenführungsregeln, Kategorien.
- **Integration:** `AppCore` gegen den lokalen Testserver, ohne WebView. Hier liegt das Gewicht.
- **Fehlerinjektion:** ein kleiner TCP-Proxy im Test kappt Verbindungen an bestimmten Stellen
  (während des Abgleichs, nach der Übertragung beim Senden).
- **Oberfläche:** Vitest für Komponenten; WebdriverIO gegen die echte App für wenige durchgehende Abläufe.
- **Sicherheit:** Korpus feindlicher HTML-Mails und Dateinamen; Zähler für ausgehende Verbindungen.
  Der Zähler zählt TCP-Verbindungen, nicht nur HTTP-Anfragen (Preconnect öffnet nur eine Verbindung).
  Jeder Nachweis „null Zugriffe“ hat eine Positivkontrolle, die zeigt, dass der Zähler zählt.
- **Leistung:** erzeugte Postfächer mit 100.000 und 500.000 Mails, Ergebnisse in `docs/testing/performance.md`.

## Entscheidungen

| Entscheidung | Grund | Rückweg |
|---|---|---|
| Sync über einen Ordner im Postfach statt eigenem Server | abzugleichen sind wenige Kilobyte; kein Server, kein Login, kein Pairing, für jeden nutzbar | zweiter Transport über `SettingsTransport` |
| Keine Datenbank-Verschlüsselung in Version 1 | FileVault und BitLocker schützen die Platte; SQLCipher kostet Tempo und erschwert Windows-Builds | rusqlite kann SQLCipher per Feature zuschalten |
| rusqlite statt SQLx | ein Prozess, ein Schreiber; synchron ist einfacher und schneller | Speicherzugriff ist hinter `mail-store` gekapselt |
| KI nur über Schnittstellen | kein Modell-Download, keine GPU-Builds pro Plattform; Ollama deckt „lokal“ ab | eingebettete Laufzeit als weitere `AiProvider`-Umsetzung |
| KI zuerst über MCP (0010) | jede KI passt, keine Anbieter und Schlüssel in der App, wenig Code | eingebaute KI (Stufe 13) ergänzt, MCP bleibt |
| IMAP zuerst, Microsoft und Gmail-API später | eigene Konten laufen über IMAP; OAuth-Anbieter brauchen Registrierungen | `MailBackend`-Trait |
| Windows-Installer per Cross-Build vom Mac | kein Windows-Build-Rechner nötig | Build auf echtem Windows oder gehosteten Runnern in Stufe 15 |
| Vier Crates statt elf | Grenzen erst ziehen, wenn sie sich bewährt haben | aufteilen, sobald ein Crate zwei klar getrennte Aufgaben hat |
| Lizenz GPL-3.0-or-later (vom Menschen bestätigt am 22.11.2026) | Abwandlungen bleiben offen; alle gewählten Bibliotheken sind kompatibel | vor der Veröffentlichung änderbar |
| tauri-specta statt ts-rs (0004) | typisierte Commands und Events, erzwungene Entscheidung bei 64-Bit-Zahlen | ts-rs erzeugt dieselben Typen; nur `ipc.ts` müsste neu geschrieben werden |
| Stalwart als Testserver (0001) | ein Binary, alle nötigen Fähigkeiten, kein Docker | Dovecot über Homebrew mit eigenem MTA |
| FTS5 mit `unicode61` statt `trigram` (0003) | trigram ist 5,2-mal so groß und halb so schnell beim Einfügen | Tokenizer per Migration wechselbar, Index ist Cache |
| rusqlite 0.41 statt 0.43 (0006) | `rusqlite_migration` 2.7 verlangt ^0.41; gleiches SQLite 3.54.1 | eigener Migrations-Runner, dann rusqlite 0.43 |
