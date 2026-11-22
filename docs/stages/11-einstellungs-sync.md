# Stufe 11 — Einstellungs-Sync

**Ziel:** Zwei Geräte mit denselben Konten gleichen Einstellungen, Signaturen, Vorlagen, Regeln,
Ansichten, Snooze und Pins über einen verschlüsselten Ordner im eigenen Postfach ab. Kein Server von uns.

**Voraussetzung:** Stufen 09 und 10 fertig. Abschnitt „Einstellungs-Sync“ in `docs/ARCHITECTURE.md`
gelesen; er ist der Entwurf für diese Stufe.

## Aufgaben

- [ ] **Datensätze.** Modell für einen Sync-Datensatz: Art, ID, Version (hybride logische Uhr aus Zeit, Zähler, Geräte-ID), Inhalt, Grabstein. Verzeichnis der synchronisierbaren Datenarten; nur was dort eingetragen ist, wird abgeglichen.
- [ ] **Änderungen erfassen.** Tabelle `sync_record` hält je Datensatz die zuletzt abgeglichene Version. Jede Änderung an einer synchronisierbaren Tabelle erhöht die Version.
- [ ] **Zusammenführen.** Die Regeln je Datenart aus der Architektur, als reine Funktionen ohne IO.
- [ ] **Transport.** Trait `SettingsTransport` (auflisten, schreiben, löschen). Umsetzung über IMAP: eigenen Ordner anlegen und nicht abonnieren; ein Datensatz je Nachricht; ändern heißt neue Fassung anhängen und alte entfernen; Änderungen über den Ordnerstatus erkennen, im normalen Abgleich mitlaufend.
- [ ] **Verschlüsselung.** Mit dem Crate `age`: zufälliger Schlüssel, der mit der Passphrase geschützt im Ordner liegt; Datensätze an diesen Schlüssel verschlüsselt. Art, ID und Version stehen zusätzlich im verschlüsselten Teil und werden beim Lesen gegen die Kopfzeilen geprüft. Keine eigene Kryptografie. Die Passphrase liegt nach der Eingabe im System-Schlüsselbund.
- [ ] **Einrichten.** Sync einschalten, Sync-Konto wählen, Passphrase setzen (mit Hinweis zur Stärke und zu den Folgen des Vergessens), hochladen.
- [ ] **Zweites Gerät.** Nach dem Einrichten des Sync-Kontos erkennt die App den Ordner und fragt nach der Passphrase. Vorschau, was übernommen wird, dann zusammenführen. Übernommene Konten fragen einzeln nach ihrem Passwort.
- [ ] **Konto entfernt.** Wurde ein Konto auf einem Gerät entfernt, fragt das andere nach, statt still zu löschen.
- [ ] **Status.** Letzter Abgleich, Liste der Geräte (jedes schreibt einen kleinen Datensatz mit Name, System, zuletzt gesehen), offene Rückfragen, „Sync zurücksetzen“.
- [ ] **Passphrase.** Ändern (Schlüssel neu schützen). Vergessen: zurücksetzen mit neuem Schlüssel und Neuaufbau von diesem Gerät.
- [ ] **Format dokumentieren.** `docs/SYNC.md` (Englisch): Aufbau des Ordners und der Datensätze, Regeln, und was der Mailserver sehen kann (Anzahl, Größe, Zeitpunkte).

## Abnahme

Alle Tests laufen mit zwei Engine-Instanzen mit getrennten Datenbanken gegen den Testserver.

- [ ] zwei Geräte ändern verschiedene Einstellungen: beide Änderungen bleiben
- [ ] zwei Geräte ändern dieselbe Einstellung: beide enden beim selben Wert
- [ ] ein Gerät löscht eine Signatur, das andere bearbeitet sie: die bearbeitete Fassung bleibt
- [ ] ein Gerät war lange offline und bringt alte Änderungen mit: neuere Stände bleiben, Grabsteine wirken
- [ ] abgelaufene Grabsteine führen nicht zum Wiederauftauchen gelöschter Datensätze
- [ ] ein veränderter Datensatz wird verworfen und gemeldet; die übrigen gleichen weiter ab
- [ ] eine alte Fassung, erneut eingespielt, wird ignoriert
- [ ] Abbruch zwischen Anhängen und Entfernen: die doppelte Fassung wird beim nächsten Lauf aufgeräumt
- [ ] falsche Passphrase: klare Meldung, nichts wird überschrieben
- [ ] 1.000 Datensätze: erster Abgleich in vertretbarer Zeit (gemessen)
- [ ] Test: Passwörter, Tokens und API-Schlüssel tauchen in keinem Datensatz auf
- [ ] Test: Der Ausfall des Sync-Kontos beeinträchtigt Lesen und Senden in keinem Konto
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Abgleich von Passwörtern (bleiben gerätegebunden)
- eigener Sync-Server oder Dateiordner als Transport
- Abgleich von Mails, Flags oder Entwürfen (macht der Mailserver)

## Der Mensch prüft danach

- Auf dem Mac einschalten, auf dem Windows-Rechner verbinden. Eine Signatur auf einem Gerät ändern, auf dem anderen eine Mail zurückstellen: Kommt beides an?
- Taucht der Sync-Ordner in einem anderen Mail-Programm auf, und stört er dort?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
