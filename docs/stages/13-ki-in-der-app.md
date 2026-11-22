# Stufe 13 — KI in der App (optional)

**Ziel:** Wer möchte, schaltet KI direkt in der App ein: Thread zusammenfassen, Antworten
vorschlagen, beim Schreiben helfen, Fragen in natürlicher Sprache mit Quellen beantworten. Dafür gibt
es eine einzige schlanke Anbieterschicht. Ohne Einschalten verlässt keine Anfrage das Gerät, und KI
führt nie selbst eine Aktion aus. Hinweise auf offene Antworten funktionieren auch ohne KI.

**Voraussetzung:** Stufen 07 und 12 fertig. `docs/decisions/0010-ki-ueber-mcp.md` und Abschnitt „KI“
in `docs/ARCHITECTURE.md` gelesen. Vor der Arbeit an Anthropic den Skill `claude-api` laden.

## Aufgaben

- [ ] **Schnittstelle.** Neues Crate `mail-ai` mit Trait `AiProvider`: Anfrage mit Streaming, Abbruch, Liste der Modelle.
- [ ] **Ein Anbieter.** OpenAI-kompatible Chat-Schnittstelle mit frei wählbarer Basis-URL: Ollama, LM Studio, llama.cpp-Server, OpenRouter, OpenAI. Prüfen, ob Anthropic über seine OpenAI-kompatible Schnittstelle genügt; wenn nicht, einen zweiten Anbieter für die Messages API ergänzen (Stopp-Grund vor dem Lesen der Antwort prüfen, auch Ablehnung). Entscheidung festhalten.
- [ ] **Einstellungen.** KI ist aus, bis der Nutzer sie einschaltet. Anbieter-Profile (Schlüssel im Schlüsselbund). Je Funktion einen Anbieter wählen oder aus. Je Konto festlegen, ob Cloud-Anbieter dessen Mails sehen dürfen. Kennzeichnung „lokal“, wenn die Basis-URL auf diesen Rechner oder das eigene Netz zeigt.
- [ ] **Transparenz.** Vor der ersten Übertragung an einen Cloud-Anbieter erklären, was gesendet wird, und zustimmen lassen. Bei jeder Anfrage anzeigen: wie viele Mails, welche Größe, an wen. Lokales Protokoll der Anfragen (nur Metadaten).
- [ ] **Schutz vor eingeschleusten Anweisungen.** Fester Systemtext im Code. Mailtext nur als klar abgegrenzte Daten. Das eingebaute Modell bekommt keine Werkzeuge. Ausgaben als Text; Bilder aus Ausgaben werden nie geladen; Links durchlaufen die Prüfung aus Stufe 3.
- [ ] **Datensparsamkeit.** Nur der aktuelle Thread bzw. die Suchkandidaten, ohne mehrfach zitierte Passagen und ohne Anhänge (nur deren Namen). Wird gekürzt, erfährt es der Nutzer.
- [ ] **Zusammenfassung, Antwortvorschläge, Schreibhilfe.** Zusammenfassung mit Kernaussagen, offenen Fragen, Fristen nur wenn genannt; jede Aussage verweist auf ihre Mail, Aussagen ohne gültigen Verweis werden verworfen. Antwortvorschläge als bearbeitbarer Entwurf. Schreibhilfe im Composer auf der Auswahl (verbessern, kürzen, umformulieren, Ton, übersetzen) mit Vorschau.
- [ ] **Suche in natürlicher Sprache.** Prüfsatz mit 30 Fragen unter `testmail/corpus/qa/`. Ablauf: Frage → Suchsprache aus Stufe 7 → lokale Kandidaten → Antwort nur aus den Kandidaten, jede Aussage mit Verweis; keine Kandidaten: ehrlich „nichts gefunden“. Embeddings nur, wenn der Prüfsatz es verlangt.
- [ ] **Hinweise auf offene Antworten.** Zuerst ohne KI: gesendete Mail mit Frage und ohne Antwort nach einigen Tagen; empfangene direkte Frage, unbeantwortet. Optional verfeinert das Modell. Anzeige als wegklickbarer Vorschlag, nie als Behauptung.
- [ ] **KI-Panel.** Nach `docs/DESIGN.md`: bezogen auf den gewählten Thread, Ergebnisse als Karten mit „Entwurf öffnen“. Streaming und Abbrechen. Ohne eingerichtete KI ist das Panel nicht sichtbar.

## Abnahme

- [ ] Test: Bei ausgeschalteter KI registriert der Attrappen-Server null Anfragen, egal was in der Oberfläche passiert
- [ ] Test: Ein Konto ohne Cloud-Freigabe schickt nichts an einen Cloud-Anbieter, auch nicht aus dem vereinten Posteingang heraus
- [ ] Vertragstests gegen einen lokalen Attrappen-Server: Streaming, Abbruch, Fehler, Ratenbegrenzung, Ablehnung
- [ ] Korpus mit eingeschleusten Anweisungen: Die Ausgabe löst keine Aktion und keine Netzverbindung aus
- [ ] Test: Verweise von Zusammenfassung und Suchantwort zeigen auf vorhandene Mails; erfundene werden entfernt
- [ ] Ergebnis des Prüfsatzes in `docs/testing/ai-search.md`: Anteil richtiger Fundstellen und Antworten
- [ ] Test: Hinweise auf offene Antworten an vorbereiteten Fällen, darunter solche ohne Hinweis (Newsletter, automatische Mails, bereits beantwortet)
- [ ] Test: Schlüssel erscheinen weder in Logs noch im Anfrage-Protokoll
- [ ] mit Ollama von Hand geprüft und vermerkt: Zusammenfassung und Antwortvorschlag funktionieren ohne Internet
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- eingebautes Modell
- Werkzeugaufrufe durch das eingebaute Modell
- Kategorien per KI (bei Bedarf später; die Kopfzeilen-Regeln aus Stufe 9 bleiben die Grundlage)

## Der Mensch prüft danach

- Ollama oder einen API-Schlüssel einrichten. Sind Zusammenfassungen, Vorschläge und Antworten an echten Threads brauchbar?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
