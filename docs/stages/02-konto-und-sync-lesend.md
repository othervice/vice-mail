# Stufe 02 — Konto und Synchronisierung, nur lesend

**Ziel:** Ein IMAP-Konto lässt sich einrichten. Ordner und Nachrichtenliste erscheinen, bleiben
aktuell und überstehen Netzabbrüche. Es wird nichts auf dem Server verändert.

**Voraussetzung:** Stufe 01 fertig. Regeln der Mail-Engine in `docs/ARCHITECTURE.md` gelesen.

## Aufgaben

- [ ] **Modell.** In `mail-model`: Konto, Identität, Postfach mit Rolle, Nachrichten-Kopfdaten, Flags und Schlagwörter, `Capabilities`, Fehlertypen, Trait `MailBackend`. Der Trait deckt ab, was IMAP braucht; nichts auf Verdacht für andere Anbieter.
- [ ] **Speicher.** Migration für `account`, `identity`, `mailbox`, `message`, `message_location`. Schema, Indizes und Regeln nach Spike B (`docs/decisions/0003-sqlite-schema-und-suche.md`, Abschnitt „Schema und Abfragen“ in der Architektur): `message.id` als Sortierdatum mit Vergabe im Schreib-Thread (Test mit vielen Mails gleichen Datums), Zusammenführen per Message-ID nur, wenn die Mail im Zielpostfach noch nicht liegt, Mails ohne Message-ID bekommen eine eigene Zeile. Abfrage der Liste mit Keyset-Paginierung, Abfrageplan per Test geprüft.
- [ ] **Adapter.** Neues Crate `mail-imap`:
  - Verbindung über TLS (993) oder STARTTLS (143), Zertifikatsprüfung über den System-Speicher, Login
  - Capabilities erkennen
  - Ordner mit SPECIAL-USE; sonst Namensheuristik, auch deutsche Namen („Gesendet“, „Entwürfe“, „Papierkorb“); Rollen vom Nutzer änderbar
  - erster Abgleich von neu nach alt in Fenstern: Envelope, Flags, Größe, Bodystructure, Kopfzeilen Message-ID, References, In-Reply-To, List-Id, List-Unsubscribe, Auto-Submitted, Precedence
  - laufender Abgleich: neue UIDs, Flag-Änderungen (CONDSTORE oder fensterweise), Löschungen über UID-Vergleich
  - UIDVALIDITY-Wechsel: Zuordnung neu aufbauen
  - IDLE auf dem Posteingang, spätestens alle 25 Minuten erneuert; nach Spike A besser in kürzeren Abschnitten (etwa 5 Minuten) oder mit TCP-Keepalive, damit eine stille Verbindung früh auffällt
  - nach Spike A (`docs/decisions/0002-imap-bibliothek.md`, „Folgen für Stufe 2“): nach dem Login `ENABLE CONDSTORE`/`QRESYNC`, Ordner per LIST-STATUS in einem Rundlauf, Fenster von 500 je Transaktion, Fortschritt als niedrigste gespeicherte UID, Lesefrist 30 s, eigene Befehlsschicht für geprüftes FETCH; für Tests gegen den Testserver die Test-CA über `new_with_extra_roots`
- [ ] **Nur-Lese-Schutz.** `writes_enabled` ist bei neuen Konten aus. Der Adapter öffnet dann mit EXAMINE und lehnt jeden schreibenden Befehl mit einem typisierten Fehler ab.
- [ ] **Engine.** Kontenverwaltung, ein Abgleich-Task je Konto, exponentieller Backoff mit Streuung, Abbruch, Wiederverbinden nach Ruhezustand und Netzwechsel. Ereignisse: Fortschritt, Postfach geändert, neue Mail, Fehler, online/offline.
- [ ] **Zugangsdaten.** Passwort in den System-Schlüsselbund (`keyring`), in der Datenbank nur der Verweis. Konto entfernen löscht Schlüsselbund-Eintrag und alle lokalen Daten des Kontos.
- [ ] **Einrichtung in der Oberfläche.** Adresse und Passwort eingeben. Servereinstellungen ermitteln über eine kleine eingebaute Tabelle bekannter Anbieter, DNS-SRV und die Autokonfiguration der eigenen Domain; sonst Formular von Hand. Kein Abruf fremder Verzeichnisse ohne Klick. Verbindungstest mit verständlichen Meldungen (Passwort falsch, Zertifikat nicht vertraut, Server nicht erreichbar). Hinweis bei Gmail und iCloud auf App-Passwörter.
- [ ] **Liste in der Oberfläche.** Konten und Ordner mit Ungelesen-Zählern. Virtualisierte Nachrichtenliste mit echten Daten nach `docs/DESIGN.md` (noch ohne Vorschautext, der kommt mit Stufe 3). Statusanzeige für Abgleich, offline und Fehler.

## Abnahme

- [ ] Testkonto in der App einrichten: Ordner und 10.000 Mails erscheinen; die Liste lässt sich während des Abgleichs flüssig scrollen
- [ ] eine neu zugestellte Mail erscheint ohne Zutun innerhalb weniger Sekunden
- [ ] Testserver stoppen: Anzeige „offline“, die Liste bleibt benutzbar; nach dem Start erholt sich der Abgleich von selbst
- [ ] Integrationstests gegen den Testserver: erster Abgleich, neue Mail, Flag-Änderung von außen, Löschung von außen, Löschung während des Abgleichs, UIDVALIDITY-Wechsel, gekappte Verbindung mitten im Abgleich, falsches Passwort
- [ ] Test: im Nur-Lese-Modus verlässt kein schreibender Befehl den Adapter
- [ ] Unit-Tests: Rollenerkennung der Ordner, Zustandsautomat des Abgleichs
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Inhalt von Mails anzeigen (Stufe 3)
- Aktionen, Verfassen, Suche, Threads, vereinter Posteingang
- OAuth-Anbieter

## Der Mensch prüft danach

- Ein echtes Konto in der App einrichten. Es bleibt nur lesend. Stimmen Ordner und Anzahl der Mails mit dem bisherigen Mail-Programm überein?
- Welcher Mailserver ist es (Anbieter oder Software)? Das hilft bei späteren Eigenheiten.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
