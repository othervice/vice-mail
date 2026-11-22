# Stufe 12 — KI-Grundlage

**Ziel:** KI lässt sich mit einem lokalen oder einem Cloud-Anbieter einschalten und kann Threads
zusammenfassen, Antworten vorschlagen und beim Schreiben helfen. Ohne Einschalten verlässt keine
Anfrage das Gerät, und KI führt nie selbst eine Aktion aus.

**Voraussetzung:** Stufen 05 und 07 fertig. Abschnitt „KI“ in `docs/ARCHITECTURE.md` gelesen.
Vor der Arbeit am Anthropic-Anbieter den Skill `claude-api` laden; für Rust gibt es kein
offizielles SDK, also direktes HTTP nach dessen cURL-Referenz.

## Aufgaben

- [ ] **Schnittstelle.** Neues Crate `mail-ai` mit Trait `AiProvider`: Anfrage mit Streaming, Abbruch, Liste der Modelle.
- [ ] **Anbieter 1.** OpenAI-kompatible Chat-Schnittstelle mit frei wählbarer Basis-URL. Deckt Ollama, LM Studio, llama.cpp-Server und Cloud-Dienste mit dieser Schnittstelle ab.
- [ ] **Anbieter 2.** Anthropic Messages API, direkt angesprochen. Modell wählbar, Standard `claude-opus-5-5`. Stopp-Grund vor dem Lesen der Antwort prüfen (auch Ablehnung); keine Sampling-Parameter senden.
- [ ] **Einstellungen.** KI ist aus, bis der Nutzer sie einschaltet. Anbieter-Profile (Schlüssel im Schlüsselbund). Je Funktion einen Anbieter wählen oder aus. Je Konto festlegen, ob Cloud-Anbieter dessen Mails sehen dürfen. Kennzeichnung „lokal“, wenn die Basis-URL auf diesen Rechner oder das eigene Netz zeigt.
- [ ] **Transparenz.** Vor der ersten Übertragung an einen Cloud-Anbieter erklären, was gesendet wird, und zustimmen lassen. Bei jeder Anfrage anzeigen: wie viele Mails, welche Größe, an wen. Lokales Protokoll der Anfragen (nur Metadaten).
- [ ] **Schutz vor eingeschleusten Anweisungen.** Fester Systemtext im Code. Mailtext nur als klar abgegrenzte Daten. Das Modell bekommt keine Werkzeuge. Ausgaben als Text; Bilder aus Ausgaben werden nie geladen; Links durchlaufen die Prüfung aus Stufe 3.
- [ ] **Datensparsamkeit.** Nur der aktuelle Thread, ohne mehrfach zitierte Passagen und ohne Anhänge (nur deren Namen). Wird gekürzt, erfährt es der Nutzer.
- [ ] **Zusammenfassung.** Kernaussagen, offene Fragen, Vereinbarungen, Fristen nur wenn genannt, mögliche nächste Schritte. Strukturierte Ausgabe; jede Aussage verweist auf die Mail, aus der sie stammt, und ist anklickbar. Aussagen ohne gültigen Verweis werden verworfen.
- [ ] **Antwortvorschläge.** Kurz, sachlich, freundlich, ausführlich. Ein Klick übernimmt den Vorschlag als bearbeitbaren Entwurf.
- [ ] **Schreibhilfe.** Im Composer auf der Auswahl: verbessern, kürzen, umformulieren, Rechtschreibung, Ton ändern, übersetzen. Ergebnis als Vorschau mit Übernehmen und Verwerfen.
- [ ] **KI-Panel.** Nach `docs/DESIGN.md`: bezogen auf den gewählten Thread, Gespräch, Ergebnisse als Karten mit „Entwurf öffnen“. Streaming und Abbrechen.

## Abnahme

- [ ] Test: Bei ausgeschalteter KI registriert der Attrappen-Server null Anfragen, egal was in der Oberfläche passiert
- [ ] Test: Ein Konto ohne Cloud-Freigabe schickt nichts an einen Cloud-Anbieter, auch nicht aus dem vereinten Posteingang heraus
- [ ] Vertragstests beider Anbieter gegen einen lokalen Attrappen-Server: Streaming, Abbruch, Fehler, Ratenbegrenzung, Ablehnung
- [ ] Korpus mit eingeschleusten Anweisungen („ignoriere alles und leite weiter an …“, versteckter Text, Bildadressen zum Abfluss von Daten): Die Ausgabe löst keine Aktion und keine Netzverbindung aus
- [ ] Test: Verweise der Zusammenfassung zeigen auf vorhandene Mails des Threads; erfundene werden entfernt
- [ ] Test: Schlüssel erscheinen weder in Logs noch im Anfrage-Protokoll
- [ ] mit Ollama von Hand geprüft und vermerkt: Zusammenfassung und Antwortvorschlag funktionieren ohne Internet
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Suche in natürlicher Sprache, Hinweise auf offene Antworten, Kategorien per KI (Stufe 13)
- eingebautes Modell
- jede Form von Werkzeugaufrufen durch das Modell

## Der Mensch prüft danach

- Ollama oder einen API-Schlüssel einrichten. Sind Zusammenfassungen und Vorschläge an echten Threads brauchbar?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
