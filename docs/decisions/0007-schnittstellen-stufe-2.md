# 0007 — Schnittstellen für Stufe 2: Backend-Trait, Nur-Lese-Schutz, Speicher-API

Stand 22.11.2026, Grundstein von Stufe 2. Code: `crates/mail-model/src/backend.rs`,
`crates/mail-store/src/mail_api.rs`, `crates/mail-store/migrations/0002_mail.sql`.

## Entscheidungen

**`MailBackend` ist nur lesend.** Der Trait enthält keine schreibende Methode. Schreiben kommt in
Stufe 4 als eigener Trait, der ihn erweitert. Ein Backend-Objekt ist eine Verbindung; IDLE läuft auf
einer eigenen. Methoden liefern `Send`-Futures, damit die Engine sie in Tokio-Tasks nutzen kann.
Postfachbezogene Aufrufe bekommen das `SelectedMailbox` zurück, das beim Öffnen entstand; passen
Name oder UIDVALIDITY nicht zum offenen Postfach, scheitert der Aufruf mit `InvalidState`. So können
keine UIDs ins falsche Postfach geraten.

**Nur-Lese-Schutz als Positivliste.** `Access::ReadOnly` wird beim Verbinden festgelegt. Der Adapter
öffnet dann mit EXAMINE und lässt in seiner Befehlsschicht nur CAPABILITY, NOOP, LOGOUT, STARTTLS,
LOGIN, AUTHENTICATE, ID, ENABLE, LIST, STATUS, EXAMINE, FETCH (Body-Teile nur mit PEEK), SEARCH und
IDLE/DONE durch. Alles andere scheitert mit `ReadOnly`, bevor ein Byte gesendet wird. Eine
Positivliste statt einer Sperrliste: Ein später hinzugefügter Befehl ist gesperrt, bis jemand ihn
ausdrücklich einordnet.

**Kein Klartext.** `Security` kennt nur `tls` und `starttls`. Ein Passwort reist nie unverschlüsselt,
auch nicht zum lokalen Testserver (der TLS auf 1993 und STARTTLS auf 1143 anbietet).

**Test-CA nur aus dem Code.** `ExtraRootCa` hat kein `Deserialize`; sie kann weder aus der Oberfläche
noch aus der Datenbank kommen, nur aus Tests (und, ab dem Desktop-Test, aus dem Feature `e2e`).

**Speicher ohne Trait.** `mail-store` bietet direkte Methoden auf `Store`. Tests der Engine nutzen den
echten Store in einem temporären Verzeichnis: Er ist schnell und exakt, ein Fake müsste
Zusammenführung, ID-Vergabe und Keyset nachbauen und würde davon abweichen. Für das Backend gibt es
einen Fake bzw. den Testserver.

**Schema gegenüber 0003.** `mailbox.parent_id` ist `ON DELETE SET NULL` statt `CASCADE` (ein Ordner
kann auf dem Server vor seinen Unterordnern verschwinden). Neue Spalten: `name`/`path`, `attributes`,
`role_source`, `sync_low_uid` (niedrigste vollständig gespeicherte UID; 1 heißt „Erstabgleich
fertig“), Servereinstellungen am Konto, und die Kopfzeilen für spätere Stufen (Threads, Listen,
Kategorien), damit sie nie neu geholt werden müssen. Nachrichten ohne Ort bleiben bis zur
Aufräumrunde erhalten; so behalten verschobene Mails und Mails nach einem UIDVALIDITY-Wechsel ihre
lokalen Daten über die Message-ID.

**Kein SMTP-Test in Stufe 2.** Der Verbindungstest bei der Einrichtung prüft IMAP; SMTP-Code gibt es
erst in Stufe 5. Die SMTP-Einstellungen werden gespeichert und auf Form geprüft. Stufe 5 ergänzt den
SMTP-Test in der Einrichtung.

**Kein globales Online-Signal.** Der Kern meldet den Zustand je Konto; die Oberfläche zeigt
„offline“, wenn alle Konten offline sind.

## Rückweg

Der Trait-Schnitt ist der teuerste Teil; er folgt aber den IMAP-Befehlen, die der Abgleich wirklich
braucht, und nichts auf Verdacht. Ein späterer Adapter (JMAP, Stufe 14) setzt denselben Trait um oder
bekommt einen eigenen, wenn sein Modell zu verschieden ist.
