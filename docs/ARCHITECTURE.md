# Architektur

Stand 22.11.2026. Die Versionen sind die an diesem Tag aktuellen stabilen. Stufe 0 bestätigt oder
korrigiert sie mit Wegwerf-Code, bevor Produktcode entsteht.

## Überblick

```
React-Oberfläche (WebView)
   │   typisierte Commands und Events, einziger Weg
app/src-tauri            dünne Shell: Fenster, Commands, Verdrahtung
   │
mail-engine              Konten, Sync-Planung, Outbox, Threads, Kategorien, Regeln
   ├── mail-store        SQLite, Migrationen, FTS5, Abfragen
   ├── mail-imap         IMAP- und SMTP-Adapter, implementiert MailBackend
   └── mail-model        Typen, Fehler, Capabilities, MailBackend-Trait, ohne IO
```

Später kommen `mail-ai` (Stufe 12) und bei Bedarf weitere Adapter (Stufe 14) dazu. Der
Einstellungs-Sync (Stufe 11) beginnt als Modul in `mail-engine` und wird erst ein eigenes Crate,
wenn er groß genug ist.

## Stack

| Bereich | Wahl | Version | Warum |
|---|---|---|---|
| Desktop-Shell | Tauri | 2.12 | System-WebView statt mitgeliefertem Browser: kleine App, wenig Speicher, Rust-Kern |
| Kern | Rust stable, Edition 2024, Tokio | mind. 1.88 | Geschwindigkeit, Speichersicherheit; 1.88 verlangt async-imap |
| Oberfläche | React, TypeScript strict | 19.3 / 7.0 | größtes Ökosystem für die nötigen Bausteine |
| Build | Vite, pnpm | 8.3 | schnell, Standard bei Tauri |
| Styling | Tailwind CSS mit eigenen Tokens | 4.3 | Tokens als CSS-Variablen, kein Laufzeit-CSS |
| Bausteine | Radix UI, Lucide-Icons, cmdk | 1.8 / – / 1.1 | zugängliche Grundbausteine ohne fremdes Aussehen |
| Liste | TanStack Virtual | 3.14 | Virtualisierung für sehr lange Listen |
| Daten im Frontend | TanStack Query, Zustand | 5.108 / 5.1 | Query für Daten aus dem Kern, Zustand für reinen UI-Zustand |
| Editor | TipTap | 3.35 | ausgereifter Rich-Text-Editor mit kontrollierbarem HTML |
| Sprachen | i18next, react-i18next | 27 / 18 | Standard, Englisch und Deutsch |
| Datenbank | SQLite über rusqlite (bundled, FTS5) | 0.43 | ein Prozess, ein Schreiber: synchrones rusqlite ist einfacher und schneller als SQLx |
| Migrationen | rusqlite_migration | 2.7 | klein, nutzt `user_version` |
| IMAP | async-imap (Tokio, rustls) | 0.12 | im Dauereinsatz bei Delta Chat; kann IDLE, CONDSTORE, MOVE, UID EXPUNGE; kein QRESYNC |
| SMTP | lettre (Tokio, rustls) | 0.11 | Standard in Rust |
| MIME lesen / schreiben | mail-parser / mail-builder | 1.0 / 2.0 | robust gegen kaputte Mails, vom Stalwart-Projekt |
| HTML-Bereinigung | ammonia | 4.2 | Allowlist-Sanitizer |
| TLS | rustls, Zertifikatsprüfung über den System-Speicher | – | kein OpenSSL, einfache Windows-Builds; selbst signierte Server-Zertifikate funktionieren, wenn das System ihnen vertraut |
| Schlüsselbund | keyring | 4.2 | macOS-Schlüsselbund und Windows-Anmeldeinformationen |
| Typen über IPC | ts-rs (stabil) oder tauri-specta (2.0 noch RC) | 12.0 / 2.0-rc | Stufe 0 entscheidet; TypeScript-Typen werden aus Rust erzeugt, nie von Hand gepflegt |
| Sync-Verschlüsselung | age | 0.12 | geprüftes Format, keine eigene Kryptografie |
| Logging | tracing | – | strukturierte Logs mit Schwärzung |
| Tests | cargo test, Vitest, WebdriverIO mit `@wdio/tauri-service` | – / 5.1 / – | der Service bringt einen eingebetteten WebDriver mit und läuft damit auch auf macOS |
| Testserver | Stalwart (ein Binary, IMAP+SMTP, auch für Windows) oder Dovecot über Homebrew | 1.1 / 2.4 | Stufe 0 entscheidet; Docker ist auf dem Mac mini nicht vorhanden |
| Windows-Build | cargo-xwin, LLVM, NSIS | 0.24 | Installer vom Mac aus, laut Tauri-Doku mit Einschränkungen möglich |

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
- `mail-engine`: verbindet Adapter und Speicher. Bietet der Shell eine Fassade `AppCore` und einen
  Strom typisierter Ereignisse. Alles Verhalten ist über `AppCore` ohne WebView testbar.
- `app/src-tauri`: reicht Commands an `AppCore` durch und Ereignisse an die Oberfläche weiter.
- Oberfläche: fragt Daten seitenweise ab und lädt neu, wenn ein Ereignis die Ansicht betrifft. Sie pollt nie.

`MailBackend` meldet über eine `Capabilities`-Struktur, was der Server kann (IDLE, CONDSTORE, MOVE,
UIDPLUS, SPECIAL-USE, eigene Schlagwörter). Die Engine und die Oberfläche fragen danach. Was ein
Server nicht kann, wird nicht vorgetäuscht, sondern ausgeblendet oder mit einem klaren Hinweis versehen.

## Datenmodell

| Tabelle | Inhalt | Klasse |
|---|---|---|
| `account` | UUID, Typ, Adresse, Name, Farbe, Servereinstellungen, Verweis auf Schlüsselbund, `writes_enabled` | sync (ohne Geheimnisse) |
| `identity` | Absenderadresse, Name, Reply-To, Signatur, Standard | sync |
| `mailbox` | Servername, Trenner, Eltern, Rolle (Inbox, Sent, Drafts, Archive, Trash, Junk, All, Custom), UIDVALIDITY, UIDNEXT, HIGHESTMODSEQ | Cache |
| `message` | interne ID, Konto, Message-ID-Kopfzeile, Thread, Betreff, Absender, Empfänger, Daten, Größe, Vorschau, Kategorie, Verweis auf den Body | Cache |
| `message_location` | Nachricht, Postfach, UID, Flags, Schlagwörter, MODSEQ | Cache |
| `thread` | Konto, normalisierter Betreff, letzte Nachricht, Anzahl, ungelesen | Cache |
| `attachment` | Nachricht, MIME-Teil, Dateiname, Typ, Größe, Content-ID | Cache |
| `message_fts` | FTS5-Index über Betreff, Absender, Empfänger, Text, Anhangnamen | Cache |
| `operation` | Outbox: Art, Nutzdaten, Idempotenzschlüssel, Zustand, Versuche, letzter Fehler | Gerät |
| `draft` | UUID, Konto, Identität, Kopfzeilen, Inhalt, Anhänge, Revision, UID im Entwürfe-Ordner | Gerät (Abgleich über IMAP) |
| `contact` | Adresse, Name, Quelle, Häufigkeit, zuletzt benutzt | Cache; von Hand bearbeitete Einträge sync |
| `signature`, `template`, `rule`, `saved_view` | wie benannt | sync |
| `snooze`, `pin` | Verweis auf die Mail über Konto-UUID und Message-ID, Zeitpunkt | sync |
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
- Nach erfolgreichem Versand wird eine Kopie in „Gesendet“ abgelegt, außer der Server tut das selbst.

**Nur-Lese-Modus**
- Konten mit `writes_enabled = false` öffnen Postfächer mit EXAMINE. Der Adapter lehnt jeden
  schreibenden Befehl mit einem typisierten Fehler ab. Ein Test beweist das.

**Offline**
- Kopfzeilen aller Mails liegen lokal. Bodies werden beim Öffnen geholt und für einen einstellbaren
  Zeitraum im Voraus geladen (Standard 30 Tage für Posteingang, Gesendet, Entwürfe und Markiertes).
- Bodies liegen komprimiert als Dateien im Datenverzeichnis der App, die Datenbank hält den Verweis.
  Stufe 3 bestätigt das durch Messung.

**Threads**
- Je Konto, über Message-ID, In-Reply-To und References. Rückfall auf den Betreff nur vorsichtig:
  gleicher normalisierter Betreff, gemeinsame Teilnehmer, enges Zeitfenster.

## Sicherheitsgrenzen

**HTML aus Mails**
1. In Rust mit ammonia bereinigen: kein Script, keine Ereignis-Attribute, keine Formulare, kein
   `meta refresh`, kein `base`, keine eingebetteten Objekte oder Frames.
2. Externe Adressen in Bildern, `srcset`, Hintergründen und CSS durch Platzhalter ersetzen, solange
   der Nutzer sie nicht freigegeben hat. Inline-Bilder (`cid:`) kommen aus dem lokalen Speicher.
3. Anzeige in einem `iframe` mit `sandbox` ohne `allow-scripts` und mit eigener Content-Security-Policy.
   Dort läuft kein JavaScript, also ist auch die Tauri-Schnittstelle nicht erreichbar.
4. Freigegebene externe Bilder lädt der Rust-Kern: ohne Cookies, ohne Referrer, mit neutraler
   Kennung, mit Größen- und Typgrenzen.
5. Links öffnen sich nie direkt. Ein Klick zeigt das echte Ziel und warnt bei Abweichung vom
   Linktext oder bei verdächtigen Domains. Geöffnet wird im System-Browser, nur `http`, `https`, `mailto`.

**Shell**
- Tauri-Capabilities minimal: keine Dateisystem-, Shell- oder HTTP-Plugins für das Frontend.
- Strenge Content-Security-Policy für die App selbst, keine entfernten Quellen.
- Der eingebettete WebDriver für Tests ist nur in Test-Builds enthalten, nie in ausgelieferten.

**Geheimnisse**
- Passwörter, Tokens und API-Schlüssel nur im System-Schlüsselbund. Ein Typ `Secret<T>` verhindert,
  dass sie in Logs oder Fehlermeldungen landen; ein Test prüft das.
- Konto entfernen löscht Schlüsselbund-Eintrag, lokale Daten und Dateien dieses Kontos.

**Dateien**
- Dateinamen von Anhängen werden bereinigt: keine Pfadtrenner, keine reservierten Windows-Namen,
  keine Steuerzeichen, keine Richtungsumkehr-Zeichen. Geschrieben wird nur an den Ort, den der
  Nutzer im Systemdialog wählt.

**Erlaubte Netzverbindungen**
- Mailserver, die der Nutzer eingerichtet hat
- Externe Bilder einer Mail, nach Freigabe
- KI-Endpunkte, die der Nutzer eingerichtet und freigegeben hat
- Abmelde-Adressen (List-Unsubscribe) nach Klick
- Update-Prüfung ab Stufe 15, abschaltbar

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

- Trait `AiProvider` mit zwei Umsetzungen: OpenAI-kompatible Chat-Schnittstelle mit frei wählbarer
  Basis-URL (Ollama, LM Studio, llama.cpp-Server, OpenRouter, OpenAI) und Anthropic Messages API.
  Für Rust gibt es kein offizielles Anthropic-SDK, also direktes HTTP nach der offiziellen Doku.
- Mail-Inhalte sind Daten, keine Anweisungen: fester Systemtext im Code, Mailtext klar abgegrenzt im Nutzerteil.
- Das Modell bekommt keine Werkzeuge. Es kann also strukturell nichts senden, löschen oder abrufen.
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
- **Leistung:** erzeugte Postfächer mit 100.000 und 500.000 Mails, Ergebnisse in `docs/testing/performance.md`.

## Entscheidungen

| Entscheidung | Grund | Rückweg |
|---|---|---|
| Sync über einen Ordner im Postfach statt eigenem Server | abzugleichen sind wenige Kilobyte; kein Server, kein Login, kein Pairing, für jeden nutzbar | zweiter Transport über `SettingsTransport` |
| Keine Datenbank-Verschlüsselung in Version 1 | FileVault und BitLocker schützen die Platte; SQLCipher kostet Tempo und erschwert Windows-Builds | rusqlite kann SQLCipher per Feature zuschalten |
| rusqlite statt SQLx | ein Prozess, ein Schreiber; synchron ist einfacher und schneller | Speicherzugriff ist hinter `mail-store` gekapselt |
| KI nur über Schnittstellen | kein Modell-Download, keine GPU-Builds pro Plattform; Ollama deckt „lokal“ ab | eingebettete Laufzeit als weitere `AiProvider`-Umsetzung |
| IMAP zuerst, Microsoft und Gmail-API später | eigene Konten laufen über IMAP; OAuth-Anbieter brauchen Registrierungen | `MailBackend`-Trait |
| Windows-Installer per Cross-Build vom Mac | kein Windows-Build-Rechner nötig | Build auf echtem Windows oder gehosteten Runnern in Stufe 15 |
| Vier Crates statt elf | Grenzen erst ziehen, wenn sie sich bewährt haben | aufteilen, sobald ein Crate zwei klar getrennte Aufgaben hat |
| Lizenz GPL-3.0-or-later (Empfehlung, noch zu bestätigen) | Abwandlungen bleiben offen; alle gewählten Bibliotheken sind kompatibel | vor der Veröffentlichung änderbar |
