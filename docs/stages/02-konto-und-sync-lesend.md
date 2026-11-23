# Stufe 02 — Konto und Synchronisierung, nur lesend

**Ziel:** Ein IMAP-Konto lässt sich einrichten. Ordner und Nachrichtenliste erscheinen, bleiben
aktuell und überstehen Netzabbrüche. Es wird nichts auf dem Server verändert.

**Voraussetzung:** Stufe 01 fertig. Regeln der Mail-Engine in `docs/ARCHITECTURE.md` gelesen.

## Aufgaben

- [x] **Modell.** In `mail-model`: Konto, Identität, Postfach mit Rolle, Nachrichten-Kopfdaten, Flags und Schlagwörter, `Capabilities`, Fehlertypen, Trait `MailBackend`. Der Trait deckt ab, was IMAP braucht; nichts auf Verdacht für andere Anbieter.
- [x] **Speicher.** Migration für `account`, `identity`, `mailbox`, `message`, `message_location`. Schema, Indizes und Regeln nach Spike B (`docs/decisions/0003-sqlite-schema-und-suche.md`, Abschnitt „Schema und Abfragen“ in der Architektur): `message.id` als Sortierdatum mit Vergabe im Schreib-Thread (Test mit vielen Mails gleichen Datums), Zusammenführen per Message-ID nur, wenn die Mail im Zielpostfach noch nicht liegt, Mails ohne Message-ID bekommen eine eigene Zeile. Abfrage der Liste mit Keyset-Paginierung, Abfrageplan per Test geprüft.
- [x] **Adapter.** Neues Crate `mail-imap`:
  - Verbindung über TLS (993) oder STARTTLS (143), Zertifikatsprüfung über den System-Speicher, Login
  - Capabilities erkennen
  - Ordner mit SPECIAL-USE; sonst Namensheuristik, auch deutsche Namen („Gesendet“, „Entwürfe“, „Papierkorb“); Rollen vom Nutzer änderbar
  - erster Abgleich von neu nach alt in Fenstern: Envelope, Flags, Größe, Bodystructure, Kopfzeilen Message-ID, References, In-Reply-To, List-Id, List-Unsubscribe, Auto-Submitted, Precedence
  - laufender Abgleich: neue UIDs, Flag-Änderungen (CONDSTORE oder fensterweise), Löschungen über UID-Vergleich
  - UIDVALIDITY-Wechsel: Zuordnung neu aufbauen
  - IDLE auf dem Posteingang, spätestens alle 25 Minuten erneuert; nach Spike A besser in kürzeren Abschnitten (etwa 5 Minuten) oder mit TCP-Keepalive, damit eine stille Verbindung früh auffällt
  - nach Spike A (`docs/decisions/0002-imap-bibliothek.md`, „Folgen für Stufe 2“): nach dem Login `ENABLE CONDSTORE`/`QRESYNC`, Ordner per LIST-STATUS in einem Rundlauf, Fenster von 500 je Transaktion, Fortschritt als niedrigste gespeicherte UID, Lesefrist 30 s, eigene Befehlsschicht für geprüftes FETCH; für Tests gegen den Testserver die Test-CA über `new_with_extra_roots`
- [x] **Nur-Lese-Schutz.** `writes_enabled` ist bei neuen Konten aus. Der Adapter öffnet dann mit EXAMINE und lehnt jeden schreibenden Befehl mit einem typisierten Fehler ab.
- [x] **Engine.** Kontenverwaltung, ein Abgleich-Task je Konto, exponentieller Backoff mit Streuung, Abbruch, Wiederverbinden nach Ruhezustand und Netzwechsel. Ereignisse: Fortschritt, Postfach geändert, neue Mail, Fehler, online/offline.
- [x] **Zugangsdaten.** Passwort in den System-Schlüsselbund (`keyring`), in der Datenbank nur der Verweis. Konto entfernen löscht Schlüsselbund-Eintrag und alle lokalen Daten des Kontos.
- [x] **Einrichtung in der Oberfläche.** Adresse und Passwort eingeben. Servereinstellungen ermitteln über eine kleine eingebaute Tabelle bekannter Anbieter, DNS-SRV und die Autokonfiguration der eigenen Domain; sonst Formular von Hand. Kein Abruf fremder Verzeichnisse ohne Klick. Verbindungstest mit verständlichen Meldungen (Passwort falsch, Zertifikat nicht vertraut, Server nicht erreichbar). Hinweis bei Gmail und iCloud auf App-Passwörter.
- [x] **Liste in der Oberfläche.** Konten und Ordner mit Ungelesen-Zählern. Virtualisierte Nachrichtenliste mit echten Daten nach `docs/DESIGN.md` (noch ohne Vorschautext, der kommt mit Stufe 3). Statusanzeige für Abgleich, offline und Fehler.

## Abnahme

- [x] Testkonto in der App einrichten: Ordner und 10.000 Mails erscheinen; die Liste lässt sich während des Abgleichs flüssig scrollen
- [x] eine neu zugestellte Mail erscheint ohne Zutun innerhalb weniger Sekunden
- [x] Testserver stoppen: Anzeige „offline“, die Liste bleibt benutzbar; nach dem Start erholt sich der Abgleich von selbst
- [x] Integrationstests gegen den Testserver: erster Abgleich, neue Mail, Flag-Änderung von außen, Löschung von außen, Löschung während des Abgleichs, UIDVALIDITY-Wechsel, gekappte Verbindung mitten im Abgleich, falsches Passwort
- [x] Test: im Nur-Lese-Modus verlässt kein schreibender Befehl den Adapter
- [x] Unit-Tests: Rollenerkennung der Ordner, Zustandsautomat des Abgleichs
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Inhalt von Mails anzeigen (Stufe 3)
- Aktionen, Verfassen, Suche, Threads, vereinter Posteingang
- OAuth-Anbieter

## Der Mensch prüft danach

- Ein echtes Konto in der App einrichten. Es bleibt nur lesend. Stimmen Ordner und Anzahl der Mails mit dem bisherigen Mail-Programm überein?
- Welcher Mailserver ist es (Anbieter oder Software)? Das hilft bei späteren Eigenheiten.

## Ergebnis

Ausgeführt am 22./23.11.2026; `scripts/check.sh` und `pnpm --dir app test:e2e` am 23.11. von der
Hauptsitzung in einem sauberen Worktree (Commit 034eaed) grün ausgeführt. (Hauptsitzung mit Subagenten: Grundstein, Speicher, IMAP-Adapter,
Kontoerkennung/Schlüsselbund, Oberfläche, Engine, Desktop-Abnahme). Entscheidungen: 0007
(Schnittstellen), 0008 (Abgleich-Engine).

**Nachgewiesen**
- In der echten App (`pnpm --dir app test:e2e`, Spec `account-sync.e2e.mjs`, zweimal grün): Testkonto
  über die Oberfläche eingerichtet (erst falsches Passwort mit verständlicher Meldung), alle 10 Ordner
  mit Rolle, Gesamt- und Ungelesen-Zahl gleich wie auf dem Server, 10.000 Mails (Posteingang 9.959);
  Ordner nach 72–86 ms, erste Mails nach 108–124 ms, alles nach 1,26–1,38 s. Neu zugestellte Mail
  erscheint 55–61 ms nach der SMTP-Annahme. Server gestoppt: Hinweis „offline“ nach 16–17 ms, Liste
  bleibt scroll- und auswählbar; nach dem Start wieder synchron nach 0,2–3,2 s.
- Scrollen während eines gedrosselten Erstabgleichs (17,6 s): Median 17 ms je Schritt, längste
  Blockade des Hauptthreads 14–31 ms, keine leeren Bilder. Einschränkung: Das Fenster war für WebKit
  verborgen (Bildschirm gesperrt), belegt sind JS-, Render- und Layoutkosten, nicht echte 60 Bilder/s.
- Integrationstests gegen den Testserver (`crates/mail-engine/tests/server.rs`, in der Hauptsitzung
  mit `seed 10000` ausgeführt, 8/8): erster Abgleich, neue Mail/Flag-Änderung/Löschung von außen
  über IDLE, Löschung während des Abgleichs, UIDVALIDITY-Wechsel, gekappte Verbindung mitten im
  Abgleich, falsches Passwort, Erholung nach Server-Neustart, Nur-Lese-Mitschnitt eines kompletten
  Abgleichs mit IDLE (nur Befehle der Positivliste, EXAMINE, Bodies nur mit PEEK).
- Adapter (`crates/mail-imap`, in der Hauptsitzung ausgeführt): 42 Unit-Tests, 12 Server-Tests; im
  Nur-Lese-Modus verlassen 21 schreibende Befehle den Adapter nicht (Wire-Tap und Byte-Zähler).
- Unit-Tests: Rollenerkennung (11 Fälle inkl. deutscher, Outlook- und Gmail-Namen), Zustandsautomat
  mit Backoff (12 + Ereignis `CredentialsChanged`).
- Speicher: 100.000 Kopfdaten in 1,1 s, Listenseite 0,07 ms, Abfragepläne per Test geprüft.
- Kontoerkennung: eingebaute Anbietertabelle (offline), DNS-SRV und Autokonfiguration der eigenen
  Domain nur nach Klick; Passwort im Schlüsselbund, Konto entfernen löscht Eintrag und lokale Daten
  (in der Desktop-Abnahme geprüft: kein Eintrag bleibt zurück).

**Nachtrag 23.11.2026:** Der Servertest `new_mail_flag_change_and_deletion_from_outside_arrive_by_idle`
war ab dem Vorausladen aus Stufe 3 zeitabhängig rot (er hielt das Vorschau-Ereignis des Vorausladens
für die Flag-Änderung); die Engine war korrekt. Seit `edd380b` wartet der Test auf die Wirkung statt auf
das nächste Ereignis; 8/8 Servertests mit `seed 10000` wieder grün.

**Behobene Fehler aus der Abnahme**: Rahmen verrutschte durch versteckte Radix-Eingaben; veralteter
Fehlerhinweis nach „Konto entfernen“; hängende Tooltips beim Einklappen der Leiste.

**Abweichungen und Annahmen**
- Kein Klartext-IMAP; SMTP wird bei der Einrichtung erst ab Stufe 5 getestet (0007).
- IDLE in Abschnitten von etwa 5 Minuten statt 25. Netzwechsel ohne Ruhezustand fällt erst nach
  etwa 3,5 Minuten (Hauptverbindung) bzw. 5,5 Minuten (IDLE) auf; Ruhezustand wird über einen Sprung
  der Wanduhr erkannt (0008).
- Bei Nicht-ASCII-Zugangsdaten meldet sich der Adapter mit `AUTHENTICATE PLAIN` an.
- Schlüsselbund über `keyring-core` mit den nativen Speichern statt `keyring` 4.2.
- Auf der Kontoseite meldet WebKit je nativem `<select>` einen `style-src-attr`-Verstoß, den WebKit
  selbst auslöst (leeres `createElement("select")` genügt); als belegte Ausnahme im Test geführt.

**Ungeprüft**
- Windows zur Laufzeit (nur Cross-Build und Cross-Clippy), echter Ruhezustand und Netzwechsel,
  `AUTHENTICATE PLAIN` gegen echte Server, echte Bildraten bei sichtbarem Fenster.
- imap-proto kann bei `INTERNALDATE NIL` paniken; die Engine fängt das als Kontofehler ab (Test mit
  Fake-Backend), ein solcher Server wird aber nicht abgeglichen.

**Wartet auf**
- Mensch: ein echtes Konto in der App einrichten (bleibt nur lesend). Stimmen Ordner und Anzahl der
  Mails mit dem bisherigen Mail-Programm überein? Welcher Mailserver ist es?
- Mensch: `pnpm --dir app test:e2e` einmal angemeldet am Bildschirm laufen lassen; der Test misst dann
  echte Bildabstände beim Scrollen.
