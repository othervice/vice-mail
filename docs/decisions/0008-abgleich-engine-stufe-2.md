# 0008 — Abgleich-Engine in Stufe 2: Tasks je Konto, Ruhezustand, Panics, Anmeldung

Stand 22.11.2026, Engine-Sitzung von Stufe 2. Code: `crates/mail-engine/src/sync/`,
`crates/mail-engine/src/core.rs`, `crates/mail-imap/src/sasl.rs`.

## Entscheidungen

**Ein Aufseher-Task je Konto, eine Sitzung aus drei Tasks.** Der Aufseher besitzt den
Zustandsautomaten (`sync_state.rs`) und tut, was dieser sagt: verbinden, Durchlauf anstoßen,
warten, nach Backoff erneut verbinden, stoppen. Eine verbundene Sitzung besteht aus dem Arbeiter
(Hauptverbindung, Durchläufe), dem IDLE-Wächter (eigene Verbindung, IDLE auf dem Posteingang in
Abschnitten von 5 Minuten) und dem Abfrager (voller Durchlauf alle 3 Minuten; ohne IDLE der
Posteingang jede Minute). Fällt eine Verbindung aus, endet die ganze Sitzung, der Automat
entscheidet über den neuen Versuch. Ein IDLE-Ereignis stößt nur einen Durchlauf über den
Posteingang an, der Abfrager einen vollen mit LIST.

**Panics bleiben im Konto.** Jeder Sitzungs-Task läuft in einem eigenen Tokio-Task. Ein Panic
(imap-proto panikt bei manchen kaputten Antworten, etwa `INTERNALDATE NIL`) beendet nur die
Sitzung und wird zum Kontofehler `protocol` mit Backoff. Die Anmeldeprüfung von
`testConnection`, `addAccount` und `updatePassword` läuft ebenso in einem eigenen Task.

**Ruhezustand und Netzwechsel ohne Betriebssystem-Signal.** Ein Signal vom System bräuchte
`unsafe` oder Plattform-Crates. Stattdessen:
- Ein Wächter vergleicht alle 5 s Wanduhr und monotone Uhr. Weichen sie um mehr als 20 s ab,
  verbinden alle Konten neu. Auf macOS steht die monotone Uhr im Ruhezustand
  (`CLOCK_UPTIME_RAW`), die Differenz ist die Schlafdauer. Nachgewiesen mit einer injizierten
  Uhr; ein echter Ruhezustand wurde nicht ausgelöst.
- Kurze Fristen: 30 s Lesefrist, IDLE-Abschnitte von 5 Minuten, und der Abfrager berührt die
  Hauptverbindung alle 3 Minuten. Ein Netzwechsel ohne Ruhezustand fällt so spätestens nach etwa
  3,5 Minuten (Hauptverbindung) bzw. 5,5 Minuten (IDLE-Verbindung) auf.
- `AppCore::network_changed()` und `AppCore::resume()` verbinden sofort neu; eine Shell mit
  Systemsignal kann sie aufrufen. Die Shell von Stufe 2 ruft sie nicht auf.

**Anmeldung mit Nicht-ASCII-Zeichen über `AUTHENTICATE PLAIN`.** async-imap sendet LOGIN mit
Strings in Anführungszeichen; RFC 3501 erlaubt darin nur 7 Bit. Sind Benutzername und Passwort
ASCII, bleibt es bei LOGIN. Sonst meldet sich der Adapter mit `AUTHENTICATE PLAIN` an (RFC 4616,
UTF-8; jeder IMAP4rev1-Server muss es können), mit der ersten Antwort in der Befehlszeile, wenn
der Server SASL-IR anbietet. Nur ein Server, der nach TLS ausdrücklich kein `AUTH=PLAIN` meldet,
bekommt das Passwort weiter per LOGIN. Geprüft mit dem Fake-Server der Adapter-Tests; der
Testserver hat kein Konto mit Umlaut-Passwort.

**Konto entfernen in fester Reihenfolge.** Abgleich stoppen, Schlüsselbund-Eintrag löschen, dann
die Datenbankzeilen. Verweigert der Schlüsselbund, bleibt das Konto bestehen und sein Abgleich
läuft wieder; so bleibt nie ein Passwort ohne Konto im Schlüsselbund zurück.

**Test-CA im Desktop-Test.** Nur mit Feature `e2e` liest die Shell `MAIL_APP_E2E_EXTRA_CA` (Pfad
zu `testmail/.data/tls/ca.pem`). `pnpm --dir app test:release-clean` prüft, dass der Name im
Release-Binary fehlt und die Variable dort wirkungslos bleibt.

## Gemessen (Mac mini M4, Debug-Build, Testserver auf Loopback)

| Messung | Wert |
|---|---|
| Erster Abgleich `seed 10000` (10.000 Mails, 10 Ordner), `addAccount` bis `idle` | 1,06–1,10 s |
| Erster Abgleich `seed 1000` | 0,15 s |
| SMTP-Zustellung bis Ereignis `newMail` | 42–57 ms nach Start des Versands |
| Flag-Änderung bzw. Löschung von außen bis `messagesChanged` | 1–10 ms |
| Server gestoppt bis `offline` | 1,2 s (davon rund 1 s für `testmail.sh stop`) |
| Server gestartet bis wieder `idle` (Backoff des Produkts) | 2,1 s |

## Ungeprüft

- Echter Ruhezustand und echter Netzwechsel; Verhalten der monotonen Uhr unter Windows (zählt
  sie den Ruhezustand mit, greift nur der Rest).
- `AUTHENTICATE PLAIN` gegen echte Server (nur Fake-Server).
- Server ohne CONDSTORE und ohne IDLE nur mit dem Fake-Server.

## Rückweg

Die Sitzungs-Tasks hängen nur am `MailBackend`-Trait. Ein OS-Signal für Ruhezustand und Netz
lässt sich später in der Shell an `resume()`/`network_changed()` anschließen, ohne die Engine zu
ändern.
