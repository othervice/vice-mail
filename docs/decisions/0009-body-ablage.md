# 0009 — Bodies holen und ablegen: PEEK, zstd-Dateien statt Datenbank-BLOBs

Stand 22.11.2026, Grundstein von Stufe 3 (Aufgabe „Body holen und ablegen“). Code:
`crates/mail-model/src/body.rs`, `crates/mail-model/src/reading.rs`, `crates/mail-imap/src/bodies.rs`,
`crates/mail-store/src/{body_files,bodies,body_api,reading}.rs`, `crates/mail-store/migrations/0003_body.sql`.
Messungen: `crates/mail-store/src/body_bench.rs` (im Repo, ignorierter Test) und zwei Wegwerf-Benches
außerhalb des Repos (Kompression, Parsen; Aufbau unten beschrieben).

## Kontext

Die Architektur legt fest: Bodies werden beim Öffnen geholt, für einen Zeitraum vorausgeladen und
„komprimiert als Dateien im Datenverzeichnis“ abgelegt, die Datenbank hält den Verweis; Stufe 3 soll
das durch eine Messung gegen die Ablage in der Datenbank bestätigen. Offen waren: welche
Kompression, wie atomar geschrieben wird, was bei sehr großen Mails zuerst kommt und wie der
Nur-Lese-Schutz dabei gewahrt bleibt.

## Entscheidungen

**Holen nur mit PEEK, die Positivliste bleibt unverändert.** `MailBackend` bekommt drei Methoden mit
Standard-Umsetzung `Unsupported` (damit Test-Fakes ohne Bodies weiter kompilieren):

- `fetch_structure(mailbox, uid)` → `UID FETCH n (UID RFC822.SIZE BODYSTRUCTURE)`
- `fetch_messages(mailbox, uids)` → `UID FETCH set (UID BODY.PEEK[])`, für Öffnen und Vorausladen
- `fetch_sections(mailbox, uid, sections)` → `UID FETCH n (UID BODY.PEEK[HEADER] BODY.PEEK[1.2.MIME] BODY.PEEK[1.2] …)`

Alle Formen sind `BODY.PEEK[…]`; die Positivliste aus 0007 lässt sie durch und lehnt `BODY[`,
`BINARY[`, `RFC822` und `RFC822.TEXT` weiter ab. Fehlende UIDs sind kein Fehler (`None` bzw. fehlen
im Ergebnis). Die BODYSTRUCTURE wird ohne Rekursion in eine flache Liste `MimePart` (Vorordnung,
Tiefe, IMAP-Teilnummer nach RFC 3501 6.4.5) übersetzt; Dateinamen werden nach RFC 2231 und 2047
dekodiert, aber nicht bereinigt (das macht `mail-content`).

**Große Mails: Schwelle 2 MiB (RFC822.SIZE), dann Textteile zuerst.** Darunter kommt die ganze
Mail mit einem `BODY.PEEK[]`. Text- und HTML-Mails liegen weit darunter (Newsletter 45 KB im
Beispiel, große 50–200 KB); über 2 MiB sind es praktisch immer Anhänge. 2 MiB kosten bei 50 Mbit/s
etwa 0,3 s. Darüber holt `MessageStructure::fetch_plan` den Kopf, jeden Textteil (`text/plain`,
`text/html`, nicht als Anhang markiert) mit seinem MIME-Kopf – unabhängig von seiner Größe – und
dann Inline-Bilder mit Content-ID, solange die Summe unter der Schwelle bleibt. Teile in einer
angehängten Mail zählen als Anhang. `n.MIME` plus `n` ist ein vollständiges MIME-Teilstück, das ein
Parser allein dekodieren kann. Anhänge kommen auf Wunsch mit `fetch_sections` nach und werden zur
Teil-Ablage hinzugefügt.

**Ablage als Dateien, nicht als BLOB.** Begründung in der Messung unten: Lesen ist gleich schnell,
aber BLOBs blähen die Datenbank auf, jede Migrationssicherung (`VACUUM INTO`, drei werden
aufbewahrt) kopiert sie mit, und gelöschte BLOBs geben keinen Platz frei.

- Ort: `<Datenverzeichnis>/bodies/<Konto-UUID>/<message.id>-<eindeutig>.zst`, neben `mail.sqlite3`.
  Der eindeutige Teil (Zeit und Zähler) macht jede Schreibung zu einer neuen Datei: Nachladen,
  zwei gleichzeitige Abrufe derselben Mail und eine wiederverwendete `message.id` überschreiben nie
  eine Datei, auf die noch eine Zeile zeigt.
- Atomar: temporäre Datei im selben Verzeichnis, schreiben, `sync_all`, `rename`, Verzeichnis
  `sync_all` (Unix; unter Windows nicht möglich, NTFS protokolliert das Umbenennen). Erst danach
  die Datenbankzeile, beim Löschen erst die Zeile, dann die Datei. Ein Absturz hinterlässt also
  höchstens eine Datei ohne Verweis.
- Datenbank (Migration 3): `message_body` (vollständig oder Teil-Ablage, Struktur als JSON,
  Größen, Zeitpunkt) und `message_body_part` (Abschnitt, Datei, Offset, Länge, Anteil am
  Plattenplatz). `''` ist die ganze Mail, sonst der IMAP-Abschnitt (`HEADER`, `1.2`, `1.2.MIME`).
  `message.body_ref` aus Migration 2 (nie benutzt) entfällt, `message.preview` bleibt.
- Was zusammen geschrieben wird, teilt sich eine Datei: die Abschnitte eines Abrufs, und beim
  Vorausladen mehrere kleine vollständige Mails desselben Kontos bis 1 MiB roh
  (`store_bodies`, `BODY_PACK_LIMIT`). Grund ist der `fsync` (Messung unten). Eine Datei wird
  gelöscht, sobald keine Zeile mehr auf sie zeigt (Index `body_part_file`, im selben Commit geprüft).
- Ein Body ist Cache: Fehlt eine Datei oder ist sie beschädigt (zstd-Prüfsumme, Länge), wird das
  protokolliert, der Verweis vergessen und „nicht gespeichert“ gemeldet; die Engine holt neu.
- Aufräumen: Konto entfernen löscht das Kontoverzeichnis, verwaiste Nachrichten löschen ihre
  Dateien. Was ein Absturz oder ein gescheitertes Löschen übrig lässt, entfernt
  `sweep_body_files(min_age)` (Dateien und Kontoverzeichnisse ohne Verweis, nur ältere als
  `min_age`, damit laufende Schreibungen unberührt bleiben). Die Engine ruft es beim Start auf
  (`SWEEP_MIN_AGE` = 10 Minuten).
- Dateiarbeit (Komprimieren, Schreiben, Lesen) läuft im Blocking-Pool von Tokio, nie auf einem
  Datenbank-Thread oder einem Tokio-Worker.

**Kompression: zstd Stufe 3 mit Prüfsumme (Crate `zstd` 0.14.4, C-Bibliothek 1.5.7).** Gleiches
Verhältnis wie flate2/miniz_oxide Stufe 6, aber 4- bis 20-mal schneller beim Packen und 2,5-mal beim
Entpacken; lz4_flex ist schneller, spart aber an base64-Anhängen fast nichts; ruzstd (reines Rust)
kann nur die schnellste Stufe und ist dort langsamer und schlechter als die C-Bibliothek. Stufe 1
spart kaum Zeit, höhere Stufen kaum Platz. Die Prüfsumme im Frame erkennt Beschädigung beim Lesen.
Die C-Bibliothek baut mit `cc`; mit cargo-xwin für Windows ohne Zusatzaufwand (geprüft, s. u.).

| Crate | Version | veröffentlicht | Lizenz |
|---|---|---|---|
| zstd | 0.14.4 | 12.11.2026 | BSD-3-Clause |
| zstd-safe | 8.3.0 | 12.11.2026 | BSD-3-Clause |
| zstd-sys (zstd 1.5.7) | 2.1.3 | 12.11.2026 | BSD-3-Clause AND BSD-2-Clause |

Ohne Standard-Features (keine Legacy-Formate, kein Wörterbuch-Bau). `cargo deny check licenses bans
sources`: ok.

**Vorausladen: Einstellungen im Speicher, Klasse Gerät.** `PrefetchSettings { period, maxMessageSize }`
in `setting` (global, Schlüssel `prefetch.period`, `prefetch.maxMessageSize`); Standard 30 Tage und
5 MiB. Zeitraum 7, 30, 90, 365 Tage oder alles. Mails bis zur Grenze werden ganz vorausgeladen,
größere nur mit ihren Textteilen (offline lesbar, Anhänge auf Wunsch). Gerät statt sync, weil
Plattenplatz und Verbindung je Gerät verschieden sind; Stufe 11 registriert die Schlüssel nicht.
Unlesbare Werte (etwa von einer neueren Version) fallen einzeln auf den Standard zurück.
`prefetch_candidates` liefert je Postfach per Schlüsselbereich (Posteingang, Gesendet, Entwürfe
alles, sonst nur Markiertes über den Teilindex `loc_flagged`) die neuesten Mails ohne Body.

**Externe Inhalte: Absender-Freigabe je Konto, Klasse sync.** Tabelle `remote_content_sender`
(Konto-UUID, normalisierte Absenderadresse, Zeitpunkt), Verweis über die UUID wie bei allen
synchronisierten Datensätzen, `ON DELETE CASCADE` mit dem Konto. Normalisiert wird mit
`normalize_sender`: spitze Klammern und Leerraum weg, ASCII-Kleinschreibung, genau ein `@`.
Grenze: Die `From`-Adresse ist fälschbar; wer einen freigegebenen Absender fälscht, kann das
Laden externer Bilder auslösen (Tracking, kein Code). Das gilt für jede Mail-App mit dieser
Funktion; eine Bindung an DKIM/DMARC wäre eine spätere Verschärfung.

## Messung

**Umgebung:** Mac mini M4 (10 Kerne), 16 GB, macOS 27.2, interne SSD (APFS), Rust 1.100,
Release-Build. Parallel liefen andere Sitzungen (Last 2,5–5,7 auf 10 Kernen); die Werte streuen
zwischen den Läufen um etwa ±10 %. Der Seitencache war warm (ohne `sudo` nicht leerbar).

**Proben.** Vier Mails, erzeugt aus echtem Material (Prosa der Projektdokumente, das echte JPEG aus
`design-reference/`, ein mit `cupsfilter` erzeugtes PDF, für den „Scan“ 3,1 MB Zufallsdaten wie
ein bereits komprimiertes Dokument), mit realistischen Kopfzeilen (Received-Kette, DKIM, ARC):
klein 6,6 KB (Text und HTML), Newsletter 45 KB (Tabellenlayout, Tracking-Links), Foto 472 KB (JPEG,
CSV), groß 5,0 MB (PDF, JPEG, Scan). Dazu der ganze Seed-Korpus (`seed.py 1000 --dump`, 1.022 Mails,
11,7 MB, meist klein und synthetisch). Ohne die Proben erzeugt `body_bench.rs` eigene.

**Kompression** (Median je Mail; Packen / Entpacken; ein Wegwerf-Crate mit den vier Kandidaten):

| Probe | zstd 3 | zstd 1 | flate2 6 | flate2 1 | lz4_flex | ruzstd |
|---|---|---|---|---|---|---|
| klein 6,6 KB | 37,9 %, 0,025 / 0,008 ms | 38,5 %, 0,019 / 0,007 | 37,1 %, 0,027 / 0,009 | 39,4 %, 0,014 / 0,010 | 50,5 %, 0,003 / 0,001 | 40,5 %, 0,044 / 0,014 |
| Newsletter 45 KB | 28,5 %, 0,053 / 0,018 | 29,5 %, 0,035 / 0,018 | 28,2 %, 0,195 / 0,037 | 33,1 %, 0,061 / 0,043 | 38,2 %, 0,020 / 0,005 | 31,1 %, 0,158 / 0,070 |
| Foto 472 KB | 60,1 %, 0,90 / 0,34 | 61,8 %, 0,44 / 0,33 | 59,6 %, 7,4 / 0,90 | 62,8 %, 1,3 / 0,88 | 81,6 %, 0,20 / 0,04 | 68,4 %, 4,4 / 1,4 |
| groß 5,0 MB | 74,5 %, 3,9 / 3,9 | 74,7 %, 3,2 / 3,8 | 74,8 %, 83 / 10,3 | 75,1 %, 12,8 / 9,6 | 98,9 %, 0,5 / 0,13 | 84,2 %, 54 / 16 |
| Seed, 1.022 Mails | 72,2 %, 13,5 / 9,8 ms gesamt | 72,3 % | 72,3 %, 187 / 31 | 72,5 %, 38 / 28 | 96,9 %, 1,9 / 0,7 | 80,6 %, 134 / 38 |

Prozent = gespeicherte Größe zur Rohgröße. zstd 19 spart gegenüber 3 nur 0,4–1,6 Prozentpunkte und
braucht 75-mal so lange (5 MB: 294 ms). Base64-Anhänge lassen sich nur auf rund 75 % bringen, das
begrenzt alle Verfahren bei großen Mails.

**Windows:** `cargo xwin check -p mail-store --all-targets --target x86_64-pc-windows-msvc` ist grün,
zstd-sys wird dabei mit clang-cl übersetzt (`zstd.lib`, 2,6 MB); ein Testprogramm mit zstd ließ sich
mit `cargo xwin build --release` als PE32+-Programm bauen. Ausgeführt auf Windows: nein.

**Datei gegen Datenbank-BLOB** (`body_bench.rs`, vier Läufe; beide Varianten über die
Store-API mit denselben Pragmas, derselben Kompression und zwei Sprüngen: Datenbank-Thread und
Blocking-Pool; 460 Bodies, 84 MB roh, 10.000 Kopfzeilen im Postfach):

| Größe | Schreiben Datei | Schreiben BLOB | Lesen Datei | Lesen BLOB |
|---|---|---|---|---|
| klein 6,6 KB | 6,9–7,8 ms | 0,07–0,12 ms | 0,04–0,07 ms | 0,01–0,04 ms |
| Newsletter 45 KB | 7,7–7,8 ms | 0,12–0,16 ms | 0,05–0,07 ms | 0,03–0,06 ms |
| Foto 472 KB | 8,1–8,7 ms | 1,3–1,4 ms | 0,39–0,42 ms | 0,37–0,40 ms |
| groß 5,0 MB | 13,1–13,9 ms | 13,2–14,3 ms | 4,2–4,3 ms (p95 5,3) | 4,3 ms (p95 5,0) |
| 200 Newsletter, 8 Schreibungen parallel | 4,5–5,2 ms je Mail | 0,07–0,09 ms | | |
| 200 kleine, Pakete zu 50 (`store_bodies`) | 0,15–0,16 ms je Mail | 0,02–0,03 ms | | |
| 200 Newsletter, Pakete zu 50 | 0,45–0,46 ms je Mail | 0,09 ms | | |

| Messgröße | Dateien | BLOBs |
|---|---|---|
| Datenbankdatei vor den Bodies | 2,2 MB | 2,2 MB |
| Datenbankdatei danach (nach Checkpoint) | 2,3 MB | 57,2 MB |
| Body-Dateien | 54,4 MB in 460 Dateien | – |
| größtes WAL beim Schreiben | 4,2 MB | 7,5 MB |
| Listenseite (100 von 10.000), Median, vorher / nachher | 0,059–0,066 / 0,059–0,064 ms | 0,061–0,071 / 0,059–0,073 ms |
| Sicherung vor einer Migration (`VACUUM INTO`) | 5,5–7,7 ms, 2,2 MB | 68–93 ms, 56,9 MB |
| 100 Mails mit Body löschen (verwaiste) | 5,8–6,6 ms | 2,8–3,1 ms |
| Datenbankdatei nach dem Löschen | 2,3 MB | 57,2 MB (kein Platz zurück) |

| Schreiben einer Datei | ohne `sync_all` | mit `sync_all` (macOS: `F_FULLFSYNC`) |
|---|---|---|
| 2,5 KB bis 284 KB komprimiert | 0,13–0,33 ms | 6,8–7,7 ms |
| 3,7 MB komprimiert | 0,48–0,63 ms | 10,4–13,8 ms |

Einordnung:
- **Lesen** ist in beiden Varianten gleich schnell und weit unter dem Ziel „lokal vorhandene Mail
  öffnet sich in unter 100 ms“: Verweis, Datei und Entpacken kosten 0,04 ms (klein) bis 4,3 ms
  (5 MB). Parsen mit mail-parser 1.0.1 kostete 0,004–0,08 ms je Probe (ohne Anhänge zu
  dekodieren). Offen für das Ziel sind Bereinigung und Darstellung (Stufe 3, `mail-content` und UI).
- **Schreiben** einer einzelnen Datei bestimmt der `fsync`: macOS macht `sync_all` zu
  `F_FULLFSYNC` (Plattencache wird geleert), etwa 7 ms, und parallele Aufrufe warten aufeinander.
  SQLite schreibt mit `synchronous=NORMAL` im WAL ohne `fsync` je Commit, daher der Abstand. Beim
  Öffnen spielt das keine Rolle (die Engine kann die Mail aus dem Speicher anzeigen, während sie
  abgelegt wird); beim Vorausladen gleicht das Packen ihn aus: 0,15–0,45 ms je Mail, 30 Tage mit
  etwa 1.500 Mails also unter einer Sekunde Plattenzeit.
- **Platz und Wartung** entscheiden: Mit BLOBs wächst die Datenbank um die Bodies (hier 25-fach),
  jede Migrationssicherung kopiert sie (hochgerechnet 1–1,5 s und gleich viel Platz je GB, mal drei
  aufbewahrte Sicherungen), und gelöschte Bodies geben keinen Platz frei, solange niemand
  `VACUUM` ausführt (das die ganze Datei neu schreibt). Listenabfragen beeinflusst keine Variante.
- Bei „alles vorausladen“ (etwa 50.000 Mails, 2–3 GB komprimiert) wären das mehrere GB Datenbank
  und Sicherungen; mit Dateien bleibt die Datenbank bei wenigen MB je 10.000 Mails.

## Nachweise (Tests)

- `mail-model`: Abschnitte und Teilnummern (Text, JSON, ungültige Formen), `fetch_plan` (ganze
  Mail, Textteile zuerst, Inline-Bilder im Budget, Teile in angehängten Mails), Standard-Methoden des
  Traits melden `Unsupported` mit `Send`-Futures, Drahtformat aller IPC-Typen von Stufe 3,
  Platzhalter-Links, `normalize_sender`.
- `mail-imap`, Fake-Server: Struktur, Abschnitte und ganze Mails (3-MB-Literal) werden richtig
  gelesen; fehlende UIDs, leere Mengen und UID 0 gehen nicht oder ohne Fehler; eine nebenbei
  eintreffende Flag-Änderung meldet das nächste IDLE; eine Ablehnung ist `Rejected` und die
  Verbindung bleibt nutzbar; jede gesendete Zeile ist PEEK, kein STORE, kein `\Seen`.
  BODYSTRUCTURE-Nummerierung nach RFC 3501 mit verschachtelten und angehängten Mails, 40 Ebenen
  ohne Rekursion, Dateinamen nach RFC 2231 (auch Latin-1 und Fortsetzungen) und 2047.
- `mail-imap`, Testserver (`tests/bodies.rs`, `#[ignore]`; `testmail.sh reset`, `start`,
  `seed 1000`): 97 ungelesene Mails aus INBOX und Archive ganz geholt (190 KB, je 50 in 3–6 ms),
  jede mit genau RFC822.SIZE Bytes; die 6,8-MB-Mail (`large/zip-5mb`) in Abschnitten (413 Bytes
  Kopf und Text in 2,4 ms), danach ganz (4,8 ms, Debug-Build, Loopback). Flags vor und nach dem
  Holen gleich, über dieselbe und eine zweite Verbindung; alle 97 bleiben ungelesen. Auf der
  Leitung nur LOGIN, ENABLE, EXAMINE, UID SEARCH, UID FETCH, LOGOUT, jeder Body-Abschnitt mit PEEK
  (Positivkontrolle: der Mitschnitt enthält die `BODY.PEEK[]`-Zeilen). Die übrigen Servertests von
  `mail-imap` (12) und `mail-engine` (8) laufen weiter grün.
- `mail-store`: Datei statt Datenbank, Ersetzen, gleichzeitiges Ablegen, Teil-Ablage wächst und wird
  von der ganzen Mail ersetzt, beschädigte, fehlende und leere Dateien, gepackte Dateien bleiben bis
  zur letzten Mail, Konto entfernen (auch Freigaben), verwaiste Nachrichten, Aufräumen,
  Vorschau, Kopf und Orte, Vorauslade-Kandidaten, Einstellungen, Absender-Freigaben, Migration einer
  Datenbank aus Stufe 2 mit Mails. Abfragepläne aller neuen Abfragen (frisch, nach `ANALYZE`,
  neu geöffnet).

## Ungeprüft

- Windows: zstd nur übersetzt, nicht ausgeführt; Kosten von `sync_all` (`FlushFileBuffers`) und
  `rename` über eine geöffnete Datei unter Windows nicht gemessen.
- Kalter Seitencache, langsame Platten, echte Postfächer.
- Ob `sync_all` nötig bleibt: Mit Prüfsumme erkennt das Lesen eine nach Stromausfall unvollständige
  Datei ohnehin. Ein einfaches `fsync` (Stufe wie SQLite) bräuchte `rustix` oder `unsafe`.

## Rückweg

- BLOBs: Die Store-API (`store_body`, `store_bodies`, `store_body_sections`, `load_message`,
  `load_sections`) verbirgt den Ort; eine Tabelle mit BLOB-Spalte statt `file` wäre eine
  Migration und eine Umsetzung in `body_api.rs`, ohne Änderung an Engine oder Oberfläche.
- Kompression: Der Codec steht an einer Stelle (`body_files::compress`/`decompress`); flate2 wäre
  der reine Rust-Rückweg bei gleichem Verhältnis, falls zstd beim Windows-Build Probleme macht.
- Schneller schreiben: `sync_all` durch `fsync` ohne Plattencache-Leerung ersetzen oder ganz
  weglassen (Prüfsumme), siehe „Ungeprüft“.

## Folgen für die Stufe

- `docs/ARCHITECTURE.md`: in der Stack-Tabelle eine Zeile für zstd (Bodies, 0.14.4, Gründe oben);
  im Datenmodell `message_body`, `message_body_part` (Cache), `remote_content_sender` (sync); im
  Abschnitt Offline den Satz „Stufe 3 bestätigt das durch Messung“ durch einen Verweis auf 0009
  ersetzen.
- Die Engine (nächste Sitzung): `sweep_body_files(SWEEP_MIN_AGE)` beim Start, Vorausladen mit
  `prefetch_candidates` und `store_bodies` (je FETCH höchstens einige MB laut `size`), Öffnen mit
  `body_info` → `load_message`/`load_sections`, sonst `fetch_structure`/`fetch_messages`/
  `fetch_sections` über einen Ort aus `message_locations` (UIDVALIDITY vergleichen), danach
  `set_previews`. Der Fake-Server der Engine (`sync/fake.rs`) braucht die drei Body-Methoden.
