# Stufe 05 — Verfassen und Senden

**Ziel:** Mails schreiben, beantworten, weiterleiten und senden. Entwürfe gehen nicht verloren, und
keine Mail wird versehentlich zweimal verschickt. Danach ist die App auf dem Mac täglich nutzbar.

**Voraussetzung:** Stufe 04 fertig. Abschnitt „Senden“ in `docs/ARCHITECTURE.md` gelesen.

## Aufgaben

- [ ] **Composer.** TipTap mit Grundformatierung (fett, kursiv, unterstrichen, Listen, Zitat, Link) und Umschalter auf reinen Text. Empfänger, CC, BCC als Chips mit Prüfung der Adressen. Autovervollständigung aus Adressen bisheriger Mails. Betreff. Auswahl der Absenderidentität. Eine einfache Text-Signatur je Identität.
- [ ] **Anhänge.** Über Dateiauswahl und Drag-and-drop, mit Größenwarnung.
- [ ] **Nachricht bauen.** `multipart/alternative` aus Text und mailtauglichem HTML (Stile inline, keine Klassen), bei Anhängen `multipart/mixed`. Kopfzeilen: Message-ID (entsteht beim Anlegen des Entwurfs und bleibt), Date, In-Reply-To, References. Keine Kopfzeile, die das Programm verrät, außer der Nutzer schaltet sie ein. BCC steht nicht in der gesendeten Mail, aber in der Kopie in „Gesendet“.
- [ ] **Antworten und Weiterleiten.** Antworten, allen antworten, weiterleiten (mit Anhängen). Zitat des Originals. Die Antwort nimmt die Identität, an die die Mail ging.
- [ ] **Entwürfe.** Lokal alle paar Sekunden sichern. In den Entwürfe-Ordner des Servers mit Verzögerung und beim Schließen: neue Fassung anhängen, alte entfernen. Entwürfe aus dem Ordner öffnen, auch solche aus anderen Programmen, so gut es geht. Nach einem Absturz sind ungesendete Entwürfe wieder da.
- [ ] **Senden.** Operation `send` in der Outbox. SMTP über `lettre`: STARTTLS (587) oder TLS (465). Zusatz-CA für Tests über `add_root_certificate`; Zertifikatsfehler stecken in der Ursachenkette (0002). Achtung Testserver: Stalwart verwirft eine zweite Mail mit gleicher Message-ID beim Empfänger still (0001). Nach Erfolg Kopie in „Gesendet“, außer der Server legt sie selbst ab (Einstellung je Konto, bei Gmail erkannt).
- [ ] **Unklarer Ausgang.** Bricht die Verbindung nach der Übertragung, aber vor der Bestätigung ab: nicht neu senden. Zustand „wird geprüft“, Suche nach der Message-ID in „Gesendet“. Bleibt es unklar, den Nutzer fragen.
- [ ] **Senden rückgängig.** Verzögerung vor dem Start (Standard 10 Sekunden, 0 bis 30 einstellbar). Abbrechen führt zurück in den Entwurf.
- [ ] **Fehlgeschlagene Sendungen.** Bleiben in der Ansicht „Ausgang“ mit Fehlertext, Wiederholen und Bearbeiten. Dazu ein deutlicher Hinweis in der Oberfläche.
- [ ] **Offline.** Senden wird vorgemerkt und beim Verbinden ausgeführt; die Oberfläche zeigt das.

## Abnahme

- [ ] Durchgehender Integrationstest: schreiben, senden, beim zweiten Testkonto ankommen, dort antworten, Antwort erscheint im selben Thread-Zusammenhang, archivieren
- [ ] Test: Anhang kommt mit gleicher Prüfsumme an; Umlaute und Emoji in Betreff, Namen und Dateinamen bleiben erhalten
- [ ] Test mit Fehlerinjektion: Verbindung wird nach der Übertragung gekappt. Beim Empfänger liegt genau eine Mail, oder die App fragt nach. Nie zwei.
- [ ] Test: Entwurf überlebt Beenden und Absturz; im Entwürfe-Ordner liegt immer nur die neueste Fassung
- [ ] Test: Abbrechen innerhalb der Verzögerung sendet nichts
- [ ] Test: BCC-Empfänger erhält die Mail, sein Name steht nicht in den Kopfzeilen der anderen
- [ ] Snapshot-Tests für das erzeugte HTML
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- HTML-Signaturen, Vorlagen, Inline-Bilder, Kontaktverwaltung, Später senden (Stufe 10)
- KI-Schreibhilfe (Stufe 13)

## Der Mensch prüft danach

- Vom echten Konto an sich selbst und an eine Adresse bei einem anderen Anbieter senden. Wie sehen die Mails in einem anderen Programm aus?
- Ab hier die App im Alltag benutzen und Reibungspunkte sammeln.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
