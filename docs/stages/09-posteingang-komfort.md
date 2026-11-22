# Stufe 09 — Posteingang-Komfort

**Ziel:** Der Posteingang sortiert sich selbst (ohne KI), Mails lassen sich zurückstellen und
anpinnen, die App meldet sich mit Benachrichtigungen und ist als Standard-Mail-App nutzbar.

**Voraussetzung:** Stufe 08 fertig. Die Stufe ist groß; die Blöcke sind unabhängig und können auf
zwei Sitzungen verteilt werden (A: Kategorien, Pins, Snooze, Aufgaben, Regeln. B: der Rest).

## Aufgaben A

- [ ] **Kategorien.** Beim Eintreffen einordnen anhand der Kopfzeilen: Newsletter (List-Id, List-Unsubscribe, `Precedence: bulk`), Benachrichtigungen (Auto-Submitted, typische Absender ohne Antwortadresse), Personen (alles andere). Korrektur durch den Nutzer je Absender („immer als …“), gespeichert als Regel. Wichtige Absender bilden den Block „Priorität“.
- [ ] **Bündel.** Benachrichtigungen und Newsletter als eine aufklappbare Zeile mit Absender-Chips und Zählern, nach `docs/DESIGN.md`. Kategorien abschaltbar; dann ist der Posteingang rein zeitlich.
- [ ] **Pins.** Angepinnte Mails stehen oben und in der Ansicht „Angepinnt“.
- [ ] **Snooze.** Zeitpunkt wählen (später heute, morgen, Wochenende, nächste Woche, eigener). Die Mail verschwindet aus dem Posteingang und erscheint zum Zeitpunkt wieder oben, mit Kennzeichnung und Benachrichtigung. Lokal gespeichert; zusätzlich ein Schlagwort am Server, wenn er eigene Schlagwörter erlaubt. Die Mail wird auf dem Server nie verschoben. Ansicht „Später“.
- [ ] **Aufgaben.** Mail als Aufgabe markieren, Ansicht „Aufgaben“, erledigt-Schalter. Schlagwort am Server, wenn möglich, sonst lokal.
- [ ] **Lokale Regeln.** Bedingungen und Aktionen nach `docs/SPEC.md`. Laufen beim Eintreffen, während die App läuft, und sind in der Oberfläche als lokal bezeichnet. Aktionen gehen über die Outbox.

## Aufgaben B

- [ ] **Benachrichtigungen.** Native Meldungen für neue Mails (Standard: nur Personen und Priorität), fehlgeschlagene Sendungen, fällige Snoozes. Je Konto schaltbar, Ruhezeiten, Inhalt auf dem Sperrbildschirm ausblendbar. Klick öffnet die Mail. Zähler am Dock- bzw. Taskleisten-Symbol.
- [ ] **Standard-Mail-App.** `mailto:` registrieren (Tauri-Deep-Link-Plugin), Einstellung „Als Standard festlegen“ mit dem Weg, den das jeweilige System vorsieht. Ein `mailto:`-Link öffnet einen Entwurf mit allen Feldern. `.eml`-Dateien öffnen.
- [ ] **Abmelden.** Bei Mails mit List-Unsubscribe eine Schaltfläche. Ein-Klick-Abmeldung per HTTPS-POST nach RFC 8058 oder ein vorbereiteter Entwurf bei `mailto:`. Immer nur nach Klick.
- [ ] **Kalendereinladungen.** `text/calendar` lesen und als Karte zeigen: Titel, Zeit in der eigenen Zeitzone, Ort, Veranstalter. „Im Kalender öffnen“ übergibt die Datei an das System.
- [ ] **Drucken und Export.** Bereinigte Mail drucken und als PDF sichern; als `.eml` exportieren.

## Abnahme

- [ ] Test der Einordnung an einem vorbereiteten Korpus; Trefferquote steht im Ergebnis; Korrekturen des Nutzers gehen immer vor
- [ ] Tests für Snooze mit gestellter Uhr: Zeitzonen, Sommerzeit, Rechner schläft über den Zeitpunkt hinweg, App war geschlossen
- [ ] Test: Snooze, Pin und Aufgabe verschieben nichts auf dem Server
- [ ] Tests für Regeln: jede Bedingung und Aktion, Reihenfolge, Regel auf nur lesendem Konto wird übersprungen und gemeldet
- [ ] Tests für `mailto:` nach RFC 6068, auch mit kodiertem Betreff und Text
- [ ] Test: Ohne Klick geht keine Abmelde-Anfrage hinaus (Zähl-Server)
- [ ] Tests für Einladungen: ganztägig, wiederkehrend, fremde Zeitzone, kaputte Datei
- [ ] von Hand auf beiden Systemen geprüft und im Ergebnis vermerkt: Benachrichtigungen, Ruhezeiten, Standard-Mail-App
- [ ] Leistungswerte neu gemessen; `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Zu- oder Absagen von Einladungen, eigener Kalender
- Kategorien per KI (bei Bedarf nach Stufe 13)
- Abgleich von Snooze und Pins zwischen Geräten (Stufe 11)

## Der Mensch prüft danach

- Passt die Einordnung beim eigenen Posteingang? Welche Absender landen falsch?
- Benachrichtigungen und `mailto:`-Links auf Mac und Windows ausprobieren.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
