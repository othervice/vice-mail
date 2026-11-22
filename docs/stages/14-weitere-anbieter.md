# Stufe 14 — Weitere Anbieter

**Ziel:** Konten, die mit reinem IMAP und Passwort nicht oder nur schlecht funktionieren, werden
unterstützt. Diese Stufe wird gezogen, sobald ein solches Konto gebraucht wird; die Blöcke sind
einzeln umsetzbar.

**Voraussetzung:** Stufe 05 fertig. Je Block steht dabei, was der Mensch vorher bereitstellen muss.

## Block A — Gmail über IMAP sauber machen

Gmail funktioniert seit Stufe 2 über IMAP mit App-Passwort. Dieser Block behandelt die Eigenheiten.

- [ ] Labels statt Ordner: dieselbe Mail unter mehreren Labels über `X-GM-MSGID` als eine Nachricht erkennen; „Alle Nachrichten“ nicht doppelt zählen
- [ ] Labels über `X-GM-LABELS` anzeigen und ändern
- [ ] Archivieren heißt Label „Posteingang“ entfernen; keine Kopie in „Gesendet“ ablegen, das tut Gmail selbst
- [ ] Hinweise in der Kontoeinrichtung: App-Passwort erzeugen, IMAP einschalten
- [ ] `docs/PROVIDERS.md` (Englisch): Was wird bei welchem Anbieter unterstützt und was nicht

Kein Test-Server verfügbar: Verhalten über aufgezeichnete Antworten testen und von Hand an einem echten Konto prüfen.

## Block B — Microsoft 365 und Outlook.com

Vorher vom Menschen: eine App-Registrierung bei Microsoft Entra (kostenlos) mit der Client-ID.

- [ ] Anmeldung über den System-Browser mit PKCE und lokaler Rückleitung; Prüfung des `state`-Werts gegen untergeschobene Antworten
- [ ] Tokens im System-Schlüsselbund, Erneuerung, klare Meldung bei abgelaufener Freigabe, Widerruf beim Entfernen des Kontos
- [ ] Erster Schritt: IMAP und SMTP mit OAuth (XOAUTH2). Das nutzt den vorhandenen Adapter.
- [ ] Nur wenn IMAP nicht reicht oder im Mandanten gesperrt ist: eigener Adapter für Microsoft Graph mit Delta-Abgleich, unveränderlichen IDs, Drosselung und Wiederholung
- [ ] Die Client-ID ist kein Geheimnis und wird nicht so behandelt; ein Client-Secret wird nicht verwendet

Tests: Anmeldeablauf gegen eine Attrappe (gültig, abgelaufen, widerrufen, falscher `state`); Graph gegen aufgezeichnete Antworten; von Hand an einem echten Konto.

## Block C — Gmail mit Google-Anmeldung

Nur wenn die Google-Verifizierung gewollt ist. Ohne sie gilt die Grenze von 100 Nutzern und eine
Warnseite. Vorher vom Menschen: Projekt in der Google Cloud Console, später Verifizierung mit
Domain, Datenschutzerklärung und Demo-Video.

- [ ] Anmeldung wie in Block B
- [ ] Entscheiden: IMAP mit OAuth (kleiner Schritt) oder Gmail-API mit Abgleich über die History
- [ ] Ablauf und Anforderungen der Verifizierung in `docs/PROVIDERS.md` festhalten

## Block D — JMAP

- [ ] Adapter für JMAP (Fastmail, Stalwart). Der Testserver Stalwart spricht JMAP, also dieselben Integrationstests wie für IMAP
- [ ] Push über EventSource statt IDLE

## Für jeden neuen Adapter

- [ ] `MailBackend` nur dort erweitern, wo der zweite Adapter es wirklich verlangt; Unterschiede über `Capabilities` ausdrücken, nicht verstecken
- [ ] Die gemeinsame Test-Suite (Abgleich, Aktionen, Senden, Konflikte) läuft gegen den neuen Adapter
- [ ] Nur-Lese-Schutz gilt auch hier und ist getestet

## Abnahme

- [ ] je umgesetztem Block: Tests wie beschrieben, Handprüfung an einem echten Konto im Ergebnis vermerkt
- [ ] mehrere Anbieter parallel im vereinten Posteingang, jede Mail dem richtigen Konto zugeordnet
- [ ] `docs/PROVIDERS.md` nennt nur als unterstützt, was getestet ist
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Exchange über EWS oder MAPI
- Push-Benachrichtigungen über eigene Server

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
