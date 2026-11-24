# Stufe 19 — Sortieren, Schnellfilter, Quelltext, Umleiten, Kontaktgruppen und Export

**Ziel:** Die Funktionen aus Thunderbird, die der Mensch am 24.11.2026 ausgewählt hat: die Liste
sortieren und schnell filtern, Quelltext ansehen, Mails umleiten oder als neu bearbeiten,
Kontaktgruppen sowie Import und Export von Kontakten und Ordnern.

**Anlass:** Vergleich mit Thunderbird vom 24.11.2026. Gewählt: „Sortieren + Schnellfilter“,
„Quelltext, Umleiten, Als neu“, „Kontaktgruppen + Export“. Nicht gewählt und deshalb nicht in
dieser Stufe: stärkere Filterregeln, lernender Spamfilter. Signaturen je Absenderadresse gibt es
seit Stufe 10 (Einstellungen → Schreiben → Signaturen, Standard je Adresse für neue Mails und für
Antworten).

**Voraussetzung:** Stufe 18 abgeschlossen (gleiche Liste und Spalten).

## Aufgaben

### A — Sortieren und Schnellfilter

- [ ] Liste sortieren nach Datum (Standard), Absender, Empfänger (in Gesendet und Entwürfen),
  Betreff, Größe, auf- und absteigend; je Ordner bzw. Ansicht gespeichert. Gespräche sortieren
  nach ihrer neuesten Mail. Leistungsziele aus `docs/SPEC.md` gelten weiter (500.000 Mails).
- [ ] Tabellenansicht als weitere Darstellung der breiten Liste: Spalten mit Kopf zum Sortieren,
  Breiten ziehbar, Spalten wählbar. Tastatur und Bildschirmleser wie in der bisherigen Liste.
- [ ] Schnellfilter-Leiste über der Liste: Ungelesen, Markiert, Mit Anhang, Von Kontakten,
  Schlagwort, Textfeld (Absender, Betreff, Text); kombinierbar, mit „Zurücksetzen“, per Tastatur
  erreichbar; gilt für die aktuelle Ansicht und zeigt die Trefferzahl.

### B — Quelltext, Umleiten, Als neu bearbeiten

- [ ] „Quelltext anzeigen“ (Cmd/Strg+U) in eigenem Fenster: die Mail roh als reiner Text in
  Festbreitenschrift, nie interpretiert; sehr große Mails gekürzt mit „Als .eml sichern“. Dazu im
  Kopf des Lesebereichs „Alle Kopfzeilen“.
- [ ] „Umleiten“: die Mail unverändert an neue Empfänger, mit `Resent-*`-Kopfzeilen
  (RFC 5322, Abschnitt 3.6.6); über die Outbox mit Senden-rückgängig; nur mit Schreibzugriff.
- [ ] „Als neu bearbeiten“: neuer Entwurf mit Empfängern, Betreff, Text und Anhängen der Mail,
  kein Antwortbezug; auch aus Gesendet.

### C — Kontaktgruppen, Import und Export

- [ ] Kontaktgruppen (Verteilerlisten) auf der Kontaktseite: anlegen, umbenennen, Mitglieder
  hinzufügen und entfernen. Im Composer schlägt die Autovervollständigung Gruppen vor; eine Gruppe
  wird zu ihren Mitgliedern, bei vielen Empfängern mit Hinweis auf Bcc.
- [ ] vCard-Import und -Export (`.vcf`, 3.0 und 4.0): einzelne Kontakte, Gruppen, alle; der
  Import führt Dubletten zusammen, Bilder laufen durch den abgeschotteten Hilfsprozess (0046).
- [ ] Ordner als mbox exportieren; mbox in einen Ordner importieren (über die Outbox per APPEND,
  nur mit Schreibzugriff, mit Fortschritt und Abbrechen).
- [ ] Übernahme aus Thunderbird: Profil wählen, Konten (Server, Identitäten, Signaturen) und lokale
  Ordner anzeigen und ausgewählt übernehmen. Passwörter werden nie übernommen; neue Konten
  starten nur lesend.

## Nicht in dieser Stufe

- Stärkere Filterregeln, lernender Spamfilter, Offline-Einstellungen je Ordner.
- OpenPGP, S/MIME, CalDAV, CardDAV, POP3 (nach Version 1).

## Abnahme

- [ ] `scripts/check.sh` grün, Server- und Desktop-Tests grün.
- [ ] Sortieren, Tabellenansicht und Schnellfilter per Desktop-Test, Leistung mit 100.000 Mails
  gemessen.
- [ ] Quelltext, Umleiten (Empfänger erhält die unveränderte Mail mit `Resent-*` auf dem
  Testserver) und Als neu per Test.
- [ ] Gruppen im Composer, vCard- und mbox-Rundweg, Übernahme aus einem Test-Thunderbird-Profil
  per Test.

## Ergebnis

(offen)
