# 0001 – Lokaler Testserver: Stalwart 1.1.1

Stand: 22.11.2026 · Stufe 0 · Status: entschieden

## Kontext

Alle Sitzungen arbeiten nur gegen einen lokalen IMAP/SMTP-Server (siehe `CLAUDE.md`, „Echte
Postfächer schützen“). Gebraucht wird ein Server, der auf dem Mac mini ohne `sudo` und ohne
Docker läuft, nur auf `127.0.0.1` lauscht, zwei Testkonten hat, per SMTP gesendete Mails in den
Posteingang zustellt und IDLE, CONDSTORE, MOVE, SPECIAL-USE und eigene Schlagwörter kann.
`scripts/testmail.sh start|stop|seed|reset` soll aus einem frischen Checkout ohne Handarbeit
funktionieren, `seed 10000` in wenigen Minuten (Spike A misst damit).

## Optionen

**A – Stalwart 1.1.1** (aktuelle Version, veröffentlicht 16.11.2026; die Architektur nannte 1.1).
Ein Binary mit IMAP, SMTP, JMAP und eingebautem Datenspeicher (RocksDB). Fertige Builds für
macOS, Linux und Windows auf GitHub, SHA-256 je Datei in den Release-Metadaten.
Seit 1.0 (26.10.2026) gibt es keine TOML-Konfiguration mehr: `config.json` beschreibt nur den
Datenspeicher, alles andere sind JMAP-Objekte in der Datenbank, die über die Management-API
(`urn:stalwart:jmap`) gesetzt werden. Ohne Weboberfläche geht das im „Recovery Mode“: der Server
startet nur die Management-API, ein Skript schreibt die Konfiguration, danach normaler Start.

**B – Dovecot 2.4.5 über Homebrew.** Bewährter IMAP-Server mit allen geforderten Erweiterungen.
Dovecot ist aber kein MTA: Die Zustellung „SMTP → Posteingang“ bräuchte zusätzlich Postfix oder
ein anderes MTA, und Homebrew hat keine Postfix-Formel. Dazu kommen mehrere Konfigurationsdateien,
die Abhängigkeit von openssl@4, openldap und lua, und es gibt keinen nativen Windows-Build.
Nicht installiert und nicht gemessen, weil Option A alle Anforderungen erfüllt hat.

Ausgeschlossen ohne Prüfung: Docker-Images (Docker fehlt), GreenMail (Java fehlt).

## Entscheidung

Stalwart 1.1.1 als lokaler Testserver. `scripts/testmail.sh` lädt das Binary beim ersten Start
selbst (feste Version, SHA-256 geprüft), richtet den Server im Recovery Mode über die
Management-API ein (`testmail/provision.py`) und startet ihn dann normal.

## Begründung

- Ein Prozess für IMAP und SMTP, lokale Zustellung eingebaut. Eine per Submission gesendete Mail
  liegt 15 ms später im Posteingang des Empfängers und weckt eine IDLE-Verbindung.
- Alle fünf geforderten Fähigkeiten und mehr (QRESYNC, UIDPLUS, LITERAL+, MULTIAPPEND,
  UTF8=ACCEPT, BINARY) sind vorhanden und mit `scripts/testmail.sh check` nachgewiesen.
- Vollständig skriptbar, keine Weboberfläche nötig, Konfiguration reproduzierbar aus dem Code.
- Sehr schnell: Start in rund 0,2 s, `seed 10000` in unter 5 s. Tests werden nicht vom Server gebremst.
- Gleiche Software wie `mail-parser` und `mail-builder` aus unserem Stack (Stalwart Labs);
  ein Windows-Build existiert, falls Tests später auch dort laufen sollen.

## Einrichtung im Überblick

- **Ports**, alle auf `127.0.0.1`: IMAP 1143 (STARTTLS, LOGIN vorher gesperrt), IMAPS 1993,
  Submission 1587 (STARTTLS, AUTH Pflicht), SMTPS 1465 (AUTH Pflicht), SMTP 1025 ohne AUTH nur
  für lokale Empfänger, HTTP 18080 (JMAP und Management-API).
- **Konten:** `alice@vicemail.test`, `bob@vicemail.test`, dazu `admin@vicemail.test` mit
  Admin-Rolle für die Management-API. Werte in `testmail/accounts.env`.
- **TLS:** eigene Test-CA und Serverzertifikat (ECDSA P-256, `localhost`, `127.0.0.1`, `::1`,
  825 Tage) aus dem `openssl`-CLI unter `testmail/.data/tls/`. Ohne die CA lehnt ein Client das
  Zertifikat ab (geprüft). Nichts kommt in den System-Schlüsselbund.
- **Keine Verbindung nach außen:** Weiterleitung an fremde Domains für alle verboten, einzige
  Route ist die lokale Zustellung, Spamfilter, DNSBL, DKIM-/SPF-/DMARC-Prüfung, GeoIP und
  Weboberfläche abgeschaltet. Zusätzlich läuft der Serverprozess mit `HTTP(S)_PROXY` auf einen
  geschlossenen lokalen Port, sodass jeder verbliebene Download sofort scheitert. Während
  `seed 10000` und `check` wurde mit `lsof` keine Verbindung außer zu `127.0.0.1` beobachtet.
- **Für Tests angepasst:** keine Ratenbegrenzung (IMAP, HTTP, SMTP-Drosseln), kein Auto-Ban nach
  Fehlanmeldungen, keine 5-s-Wartezeit nach abgelehntem RCPT oder AUTH, kein automatisches
  Leeren von Papierkorb und Junk nach 30 Tagen, Suchindex auch über alle Kopfzeilen.
- **Einrichtung in zwei Phasen:** Phase 1 im Recovery Mode (Zertifikat, Domain, Konten,
  Listener, Einstellungen), Phase 2 nach dem ersten normalen Start, weil Stalwart erst dabei
  Standardobjekte anlegt (SMTP-Drosseln, ASN-Quelle). Danach liest das Skript die wichtigen
  Einstellungen zurück und bricht ab, wenn eine fehlt.

## Gemessene Zahlen

Mac mini M4, 16 GB, macOS 27, Stalwart 1.1.1, Python 3.15. Gemessen am 22.11.2026.

| Messung | Wert |
|---|---|
| Download beim ersten Start | 49 MB Archiv, Binary 112 MB, rund 2 s |
| Frischer Checkout bis Server bereit (`start` mit Download und Einrichtung) | 5,5 s |
| Einrichtung (`reset` ohne Download) | 2,7–4,0 s, davon rund 1 s im Recovery Mode |
| Start bis alle sechs Ports antworten | 0,14–0,33 s |
| `seed 1000` | 0,5–1,3 s (APPEND 0,2 s, SMTP 0,1 s) |
| `seed 10000` | 3,5–4,1 s (Erzeugen 0,5–0,7 s, APPEND 1,5–1,9 s, SMTP 0,1 s, Suchindex 1,3–1,8 s) |
| Abnahmekette `reset && start && seed 1000 && stop` | 5,0–5,7 s je Durchlauf (7,5 s, wenn `reset` zuerst einen laufenden Server stoppt) |
| Datenmenge nach `seed 1000` (gestoppt) | 14–24 MB je nach RocksDB-Verdichtung (11,7 MB Mails) |
| Datenmenge nach `seed 10000` | 86–87 MB (37,3 MB Mails; nach einer RocksDB-Verdichtung einmal 53 MB) |
| Speicher (RSS) nach dem Start, leer | 113–125 MB |
| Speicher während `seed 10000` (Spitze) | 393 MB |
| Speicher nach Neustart und Abruf aller Kopfdaten von 9.910 Mails | 150 MB |
| `check` (32 Prüfungen) | 1,8–2,2 s (davon rund 1,8 s für fünf absichtlich falsche IMAP-Anmeldungen) |
| IDLE: SMTP-Versand bis `EXISTS` | 15 ms |
| Kopfdaten (FLAGS, Größe, ENVELOPE, BODYSTRUCTURE, drei Kopfzeilen) von 6.610 Mails, Python | 0,02 s, 4,9 MB |

Die Abnahme `scripts/testmail.sh reset && scripts/testmail.sh start && scripts/testmail.sh seed
1000 && scripts/testmail.sh stop` lief zweimal hintereinander mit Exit-Status 0, beide Male mit
identischen Ordnerzahlen und UIDs. Der Generator ist deterministisch: zwei Läufe von
`seed.py 1000 --dump` ergeben byte-gleiche Dateien.

## Nachgewiesene Fähigkeiten

CAPABILITY nach der Anmeldung, wörtlich:

```
IMAP4rev2 IMAP4rev1 ENABLE SASL-IR LITERAL+ ID UTF8=ACCEPT JMAPACCESS IDLE NAMESPACE CHILDREN MULTIAPPEND BINARY UNSELECT ACL UIDPLUS ESEARCH WITHIN SEARCHRES SORT THREAD=REFERENCES LIST-EXTENDED LIST-STATUS ESORT SORT=DISPLAY SPECIAL-USE CREATE-SPECIAL-USE MOVE CONDSTORE QRESYNC UNAUTHENTICATE STATUS=SIZE OBJECTID+ PREVIEW RIGHTS=texkn QUOTA QUOTA=RES-STORAGE UIDONLY UIDBATCHES MESSAGELIMIT=1000000 SAVELIMIT=1000000 METADATA LIST-METADATA
```

Vor der Anmeldung (Begrüßung auf 1993): `IMAP4rev2 IMAP4rev1 ENABLE SASL-IR LITERAL+ ID
UTF8=ACCEPT AUTH=PLAIN AUTH=OAUTHBEARER AUTH=XOAUTH2`; auf 1143 zusätzlich `STARTTLS
LOGINDISABLED`.

`scripts/testmail.sh check` (32 von 32 bestanden) prüft tatsächlich:

- **IDLE:** bob wartet im IDLE, alice sendet über Submission 1587, `* N EXISTS` kommt nach 15 ms, `DONE` wird bestätigt.
- **CONDSTORE:** `SELECT … (CONDSTORE)` liefert `HIGHESTMODSEQ`, `STORE` antwortet mit `MODSEQ`,
  `UID FETCH 1:* (FLAGS) (CHANGEDSINCE m)` liefert genau die geänderte Mail,
  `STORE (UNCHANGEDSINCE 1)` wird mit `[MODIFIED 2]` abgelehnt.
- **MOVE:** `UID MOVE` antwortet mit `COPYUID`, die Mail ist im Ziel und nicht mehr in der Quelle, Schlagwörter bleiben.
- **SPECIAL-USE:** `LIST "" "*" RETURN (SPECIAL-USE)` zeigt `\Sent Sent`, `\Drafts Drafts`,
  `\Trash Trash`, `\Junk Junk`, `\Archive Archive`; `LIST (SPECIAL-USE) "" "*"` liefert genau diese fünf.
- **Eigene Schlagwörter:** `PERMANENTFLAGS (\Deleted \Seen \Answered \Flagged \Draft \*)`,
  `STORE +FLAGS ($test $label1 Custom_Keyword-123)` und Zurücklesen über eine neue Verbindung.
- Außerdem: TLS mit Prüfung gegen die Test-CA (Name `localhost` und IP `127.0.0.1`), Ablehnung
  ohne CA, LOGIN ohne TLS gesperrt, APPENDUID, fünf Fehlanmeldungen abgelehnt, ohne dass 127.0.0.1 danach gesperrt ist,
  AUTH erst nach STARTTLS, Versand ohne AUTH auf 1587 abgelehnt, fremder Absender abgelehnt,
  Weiterleitung nach außen auf 1465 und 1025 abgelehnt, Port 1025 nimmt lokale Empfänger ohne AUTH an.

## Eigenheiten, die spätere Stufen kennen müssen

- **Schlagwörter in Kleinbuchstaben:** Eigene Schlagwörter kommen kleingeschrieben zurück
  (`Custom_Keyword-123` → `custom_keyword-123`), registrierte wie `$Forwarded`, `$MDNSent`,
  `$Junk` behalten ihre Schreibweise. IMAP-Schlagwörter sind ohne Groß-/Kleinschreibung zu
  vergleichen; der Client darf die Schreibweise nicht als Identität benutzen.
- **Eigene Schlagwörter des Servers:** Jede Mail bekommt `$hasattachment` oder `$hasnoattachment`.
  Das betrifft die Flag-Zählung und den Abgleich der Schlagwörter.
- **Suche ist asynchron:** IMAP SEARCH läuft über einen Volltextindex, der nach APPEND oder
  Zustellung einige Millisekunden bis etwa eine Sekunde hinterherläuft. `SEARCH HEADER` fand
  ohne Anpassung nur From/To/Cc/Bcc/Subject; der Testserver indiziert deshalb alle Kopfzeilen.
  Treffer sind wortbasiert, nicht exakte Teilzeichenketten.
- **ENVELOPE ist aufbereitet, nicht roh:** 8-Bit-Bytes in Kopfzeilen erscheinen als U+FFFD in
  einem RFC-2047-Wort (Latin-1 ist damit verloren), ein fehlendes From wird zu
  `unknown@localhost`, ein ungültiges Datum wird „normalisiert“ (`Thu, 32 Nov 2026 25:61:99 +0339`).
  INTERNALDATE kommt immer in UTC. Für robuste Anzeige braucht Stufe 2/3 `BODY.PEEK[HEADER]`.
- **Doppelte Message-ID bei SMTP-Zustellung:** Liegt im Postfach des Empfängers schon eine Mail
  mit derselben Message-ID, nimmt der Server sie mit 250 an und verwirft sie still. APPEND
  speichert Duplikate. Wichtig für Tests zu „Senden wiederholen“ in Stufe 5.
- **Recovery Mode lauscht auf allen Schnittstellen:** Die dokumentierte Variable
  `STALWART_RECOVERY_MODE_BIND` gibt es in 1.1.1 noch nicht. Während der Einrichtung (rund 1 s
  je `reset`) ist Port 18080 im LAN erreichbar, geschützt durch ein einmaliges Zufallspasswort.
  Das Skript setzt die Variable trotzdem, damit eine spätere Version sie beachtet.
- **Wartezeit 0 hängt:** `waitOnFail` mit `0s` lässt Stalwart nach einem abgelehnten RCPT nie
  antworten; der Testserver nutzt `1ms`.
- **Weboberfläche beim Einrichten:** Im Recovery Mode versucht Stalwart beim Start die
  Weboberfläche von GitHub zu laden, bevor sie sich abschalten lässt. Das scheitert am
  geschlossenen Proxy und erscheint als `ERROR … webui.zip` in `testmail/.data/provision.log`.
  Ebenso der erste ASN-Download in Phase 2. Beides ist erwartet.
- **IMAP4rev2 und UIDONLY** werden angeboten, aber nur nach `ENABLE` aktiv; async-imap spricht IMAP4rev1.

## Lizenz

Stalwart steht unter AGPL-3.0-only oder der Stalwart Enterprise License v2 (Doppellizenz). Wir
benutzen das unveränderte Release-Binary nur lokal als Entwicklungswerkzeug; es wird weder
verändert noch mit Vice Mail ausgeliefert noch Dritten über das Netz angeboten. Die Lizenz
berührt den Code von Vice Mail daher nicht. Enterprise-Funktionen werden nicht benutzt.

## Rückweg

Der Server ist nur über Standardprotokolle (IMAP, SMTP) und `testmail/accounts.env` an das
Projekt gebunden. Ein Wechsel auf Dovecot (mit einem MTA) oder einen anderen Server bedeutet:
`scripts/testmail.sh` und `testmail/provision.py` neu schreiben, dieselben Ports und Konten
anlegen. `seed.py` und `check.py` sprechen nur IMAP und SMTP und bleiben. Schlägt ein späteres
Stalwart-Update etwas kaputt, bleibt die Version in `scripts/testmail.sh` auf 1.1.1 festgenagelt.

## Ungeprüft

- Linux-Builds: Die Prüfsummen stammen aus den GitHub-Release-Metadaten, gelaufen ist das Skript nur auf macOS arm64.
- Windows: `scripts/testmail.sh` unterstützt Windows nicht; Stalwart hätte einen Windows-Build.
- Dovecot wurde nicht installiert und nicht gemessen (siehe Option B).
- Verhalten nach Ablauf des Serverzertifikats (825 Tage): `start` bricht dann mit einem Hinweis auf `reset` ab; nicht ausgelöst.
