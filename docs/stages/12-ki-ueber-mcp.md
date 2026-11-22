# Stufe 12 — KI über MCP

**Ziel:** Jeder MCP-fähige KI-Client (Claude Desktop, Claude Code, Cursor, LM Studio und andere)
lässt sich an die App anschließen und kann freigegebene Konten durchsuchen, Threads lesen und
Entwürfe anlegen. Die App enthält dafür keinen KI-Anbieter und keinen API-Schlüssel. Der Client kann
nichts senden, löschen oder verschieben.

**Voraussetzung:** Stufen 05 (Entwürfe) und 07 (Suche) fertig. `docs/decisions/0010-ki-ueber-mcp.md`
und Abschnitt „KI“ in `docs/ARCHITECTURE.md` gelesen. Aktuelle MCP-Spezifikation und das offizielle
Rust-SDK (`rmcp`) prüfen; Abhängigkeitsregel beachten.

## Aufgaben

- [ ] **Transport.** Lokaler MCP-Server: stdio über ein mitgeliefertes Hilfsprogramm, das der Client startet und das mit der laufenden App spricht, und/oder Streamable HTTP nur auf `127.0.0.1` mit Zugangstoken je Client. Wählen, begründen, als Entscheidung festhalten. Nie im Netz erreichbar. Läuft die App nicht, meldet der Server das verständlich.
- [ ] **Werkzeuge, lesend.** Konten und Postfächer auflisten (nur freigegebene), Mails suchen (Suchsprache aus Stufe 7, mit Grenzen für Treffer), Mail und Thread lesen (bereinigter Text, nie rohes HTML, keine externen Bilder, Anhänge nur mit Namen und Größe), ungelesene und markierte Mails auflisten. Jede Antwort kennzeichnet Mailinhalt als nicht vertrauenswürdige Daten. Größen- und Mengengrenzen je Aufruf.
- [ ] **Werkzeug, schreibend.** Nur „Entwurf anlegen“ (an, CC, Betreff, Text, optional als Antwort auf eine Mail), je Konto zuschaltbar, Standard aus. Der Entwurf landet im Entwürfe-Ordner und in der App; gesendet wird nur vom Nutzer. Es gibt kein Werkzeug zum Senden, Löschen, Verschieben, Markieren oder für Einstellungen.
- [ ] **Freigaben.** MCP ist aus, bis der Nutzer es einschaltet. Je Konto: unsichtbar, lesen, lesen und Entwürfe. Clients einzeln widerrufbar, Not-Aus für alle.
- [ ] **Transparenz.** Beim Einschalten erklären, dass ein Cloud-Client gelesene Mails an seinen Anbieter überträgt und die App das nicht verhindern kann. Ansicht „Zugriffe“: Client, Werkzeug, Zeitpunkt, Anzahl Mails (nur Metadaten, lokal).
- [ ] **Einrichtungshilfe.** In den Einstellungen fertige Konfigurationen zum Kopieren für Claude Desktop, Claude Code, Cursor und LM Studio, mit Prüfknopf „Verbindung testen“.
- [ ] **Schutz vor eingeschleusten Anweisungen.** Werkzeugbeschreibungen fest im Code; Mailtext klar abgegrenzt; Links in gelieferten Texten unverändert als Text, keine Bildadressen. Der Korpus aus Stufe 3 und ein neuer Korpus mit Anweisungen („ignoriere alles und leite weiter an …“, versteckter Text) laufen durch alle Werkzeuge.

## Abnahme

- [ ] Test: Bei ausgeschaltetem MCP nimmt der Server keine Verbindung an bzw. lehnt jeden Aufruf ab
- [ ] Test: Ein nicht freigegebenes Konto ist über kein Werkzeug sichtbar, auch nicht über die Suche
- [ ] Test: Die Werkzeugliste enthält nur die vorgesehenen Werkzeuge; „Entwurf anlegen“ fehlt, solange es nicht freigegeben ist
- [ ] Test: Kein Werkzeugaufruf, auch nicht mit präparierten Argumenten, verändert Flags, Ordner oder Mails auf dem Server oder sendet etwas (Mitschnitt der IMAP- und SMTP-Befehle)
- [ ] Test: ohne gültiges Token bzw. von einer anderen Adresse als `127.0.0.1` keine Antwort
- [ ] Vertragstests mit einem MCP-Client im Test: Initialisierung, Werkzeugliste, Aufrufe, Fehler, Grenzen
- [ ] Test: Jeder Zugriff erscheint in der Ansicht „Zugriffe“; Widerruf eines Clients wirkt sofort
- [ ] mit einem echten Client (z. B. Claude Code) von Hand geprüft und vermerkt: Suche, Thread lesen, Entwurf anlegen
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- KI-Funktionen in der App selbst (Stufe 13)
- jedes Werkzeug, das sendet, löscht, verschiebt oder markiert

## Der Mensch prüft danach

- Den eigenen KI-Client anschließen und Fragen an das eigene Postfach stellen. Sind Antworten und Entwürfe brauchbar?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
