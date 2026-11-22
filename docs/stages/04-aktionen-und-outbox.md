# Stufe 04 — Aktionen und Outbox

**Ziel:** Markieren, Archivieren, Löschen und Verschieben wirken sofort in der Oberfläche und werden
zuverlässig auf dem Server nachgezogen, auch nach Offline-Phasen, Abstürzen und gleichzeitigen
Änderungen von außen. Keine Mail geht dabei verloren oder verdoppelt sich.

**Voraussetzung:** Stufe 03 fertig. Abschnitt „Outbox“ in `docs/ARCHITECTURE.md` gelesen.

## Aufgaben

- [ ] **Outbox.** Tabelle `operation` und die Ausführung in `mail-engine`: lokale Änderung sofort, Operation vermerken, ausführen sobald online. Zustände `queued`, `in_flight`, `confirmed`, `failed`, `conflict`. Reihenfolge je Postfach, Zusammenfassen sich aufhebender Operationen.
- [ ] **Absturzfestigkeit.** Beim Start werden `in_flight`-Operationen erst gegen den Server geprüft und nur dann wiederholt, wenn sie nachweislich nicht gewirkt haben.
- [ ] **Fehler und Konflikte.** Bei Fehlschlag lokalen Zustand zurücknehmen und anzeigen. Ist die Mail auf dem Server inzwischen woanders oder gelöscht, gilt der Server; die Operation wird `conflict` und der Nutzer erfährt es.
- [ ] **Aktionen.** Gelesen/ungelesen, markieren, Schlagwörter, archivieren (fehlt ein Archiv-Ordner: fragen, welcher oder ob einer angelegt wird), löschen (in den Papierkorb; im Papierkorb endgültig, mit Rückfrage), verschieben, Spam/kein Spam.
- [ ] **IMAP-Umsetzung.** UID STORE, mit `UNCHANGEDSINCE` wenn CONDSTORE da ist (über die eigene Befehlsschicht, `[MODIFIED]` steht im Antworttext; COPYUID kommt bei MOVE als unaufgeforderte Antwort; Schlagwörter vorher auf gültige Zeichen prüfen, siehe 0002). UID MOVE; ohne MOVE: COPY, `\Deleted` setzen, UID EXPUNGE. Ohne UIDPLUS wird nur dann EXPUNGE gesendet, wenn keine fremden als gelöscht markierten Mails im Ordner liegen; sonst bleibt die Mail markiert und der Nutzer bekommt einen Hinweis.
- [ ] **Schreibzugriff freischalten.** Schalter je Konto in den Einstellungen, mit Erklärung. Bestehende Konten bleiben aus. Für die Testkonten an.
- [ ] **Beim Öffnen als gelesen markieren.** Einstellbar: sofort, nach zwei Sekunden, nie.
- [ ] **Rückgängig.** Hinweis mit „Rückgängig“ für einige Sekunden nach jeder Aktion; löst die Gegenoperation aus.
- [ ] **Mehrfachauswahl.** Mit Umschalttaste Bereiche, alles in der Ansicht; eine Aktion auf viele Mails ist ein Stapel.
- [ ] **Oberfläche.** Schnellaktionen in der Zeile, Symbolleiste im Lesebereich, Ziehen auf einen Ordner, erste Tastenkürzel (die endgültige Belegung kommt in Stufe 8). Kleines Symbol für ausstehende und fehlgeschlagene Aktionen; Ansicht „Aktivität“ mit Wiederholen und Verwerfen.
- [ ] **Fehlerinjektion.** Kleiner TCP-Proxy für Tests, der Verbindungen an wählbaren Stellen kappt.

## Abnahme

- [ ] Integrationstests je Aktion: online; offline vorgemerkt und nach dem Verbinden ausgeführt
- [ ] Test: Eine zweite IMAP-Sitzung verschiebt die Mail, während eine Operation wartet. Ergebnis ist `conflict`, die Mail existiert genau einmal
- [ ] Test: Verbindung wird direkt nach dem Befehl gekappt. Nach dem Neustart wird die Operation nicht doppelt angewendet
- [ ] Test: Verschieben ohne MOVE-Fähigkeit; EXPUNGE berührt keine fremden gelöschten Mails
- [ ] Test für jede Folge von Aktionen: Die Menge der Message-IDs über alle Ordner ist vorher und nachher gleich, außer bei endgültigem Löschen
- [ ] Test: Konten ohne Schreibzugriff lehnen weiterhin alles ab, und die Oberfläche erklärt warum
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Senden, Entwürfe (Stufe 5)
- Snooze, Pins, Regeln (Stufe 9)

## Der Mensch prüft danach

- Entscheiden, für welches echte Konto Schreibzugriff freigeschaltet wird. Empfehlung: zuerst ein Nebenkonto, das Hauptkonto nach einigen Tagen.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
