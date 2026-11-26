# Stufe 22 — Rückmeldungen zu 0.1.2

**Ziel:** Alles, was der Mensch beim Test von 0.1.2 findet, gesammelt umsetzen und in einem Zug
bauen.

**Anlass:** Test von 0.1.2 am 26.11.2026. Wie in Stufe 21: erst alle Rückmeldungen sammeln,
gebaut wird erst, wenn der Mensch sagt, dass die Liste vollständig ist.

**Stand:** Liste vollständig („das reicht dann auch erstmal … los bau 0.1.3“, 26.11.2026).
Ergibt 0.1.3. Aufteilung: 4+10 (Geheimnisse), 6 (Spamfilter), 5+7 (Suche), 3+8 (Oberfläche),
danach 1+2 (Konten) und 9 (Windows); je ein Subagent in eigenem Worktree.

## Rückmeldungen

### 1 — Konten in der linken Spalte verschieben

- Gewünscht: Die Reihenfolge der Konten links selbst festlegen. Heute stehen sie in der
  Reihenfolge, in der sie angelegt wurden (`ORDER BY created_at, id` in
  `crates/mail-store/src/accounts.rs`); eine Reihenfolge gibt es bisher nur für Ordnerfavoriten
  und Regeln.
- Plan: Konten in der linken Spalte per Ziehen verschieben, dazu „Nach oben“/„Nach unten“ im Menü
  des Kontos und in Einstellungen → Konten (Tastatur und Bildschirmleser). Neue Spalte für die
  Position am Konto, Migration übernimmt die heutige Reihenfolge. Dieselbe Reihenfolge überall,
  wo Konten aufgezählt werden: linke Spalte, schmale Leiste, Einstellungen, Kontowahl im
  Composer, „Alle Posteingänge“-Auswahl. Läuft über die Einstellungs-Synchronisierung mit.

### 2 — Anzeigenamen eines Kontos bearbeiten

- Gewünscht: Den Namen, unter dem ein Konto links steht, ändern können.
- Stand im Code: `account.display_name` wird beim Anlegen gesetzt und ist zugleich der
  Absendername der ersten Identität; es gibt keinen Befehl, ihn zu ändern.
- Plan: Eigener Name für die Liste („Name in der Liste“), Standard die Adresse, änderbar per
  Doppelklick bzw. „Umbenennen“ im Menü des Kontos und in Einstellungen → Konten. Der
  Absendername bleibt getrennt bei den Identitäten, damit ein Listenname wie „Arbeit“ nicht in
  gesendeten Mails landet. Leerer Name heißt: Adresse anzeigen. Synchronisiert.

### 3 — Keine Bilder und Initialen mehr

- Gewünscht: Die runden Bilder mit Initialen überall entfernen; eigene Bilder hinterlegt ohnehin
  niemand, sie kosten nur Platz. Kontakte bleiben, nur ohne Bilder. „Alles mit Bildern raus aus
  dem Client.“
- Gemeint sind Absender-, Kontakt- und Kontobilder. Bilder in Mails und Anhänge bleiben.
- Heute zu sehen in: Mail-Liste und Tabellen-/Kompaktzeilen (`MessageRow`, `InboxListRows`),
  Lesebereich (`MessageHeader`), Composer-Empfänger (`RecipientField`), Kontakte (Liste, Detail,
  Gruppen), Popup für neue Mails, schmale Leiste (`IconRail`), Einstellungen → Konten.
- Plan:
  - `Avatar` und die Kontaktbilder aus Stufe 17 entfernen (Auswahl, Zuschnitt, Speicher,
    Synchronisierung, Dekodieren im Hilfsprozess). Gespeicherte Bilder löscht eine Migration;
    vCard-Import übernimmt kein `PHOTO` mehr, der Export schreibt keins. Entscheidung festhalten,
    sie ersetzt den Bild-Teil von Stufe 17.
  - Was der Ring trug, bleibt erhalten: In Listen mit mehreren Konten zeigt ein kleiner Punkt in
    der Kontofarbe, zu welchem Konto die Mail gehört.
  - Ungelesen-Punkt und Auswahlkästchen bekommen eine schmale feste Spalte am Zeilenanfang (das
    Kästchen erscheint beim Überfahren dort), ohne die leere Spalte, die in Stufe 21 Punkt 4
    entfernt wurde.
  - Schmale Leiste: Konten als Punkt in der Kontofarbe mit dem Namen als Tooltip.
  - Abstands-Test aus Stufe 21 (0071) auf die neuen Zeilen anpassen.

Punkte 4 bis 9: vom Menschen am 26.11. aus den Vorschlägen gewählt (nicht gewählt: „Nur bei
Personen melden“, „Ziehen in Finder/Explorer“).

### 4 — Ein Schlüsselbund-Eintrag auf macOS

- Anlass: Nach jedem Update fragt macOS 5–6 Mal nach dem Schlüsselbund, einmal je Eintrag
  (ad-hoc-Signatur, die Zugriffsliste hängt am cdhash). Endgültig behebt das erst die Signatur
  mit Developer ID (Stufe 15).
- Plan: Auf macOS liegen alle Geheimnisse von Vice Mail in einem einzigen Eintrag (Dienst
  `org.vicemail.desktop`), darin eine Tabelle Name → Geheimnis für Konten, OAuth-Tokens,
  MCP-Clients, KI-Profile und den Sync-Schlüssel. Dann gibt es nach einem Update eine Abfrage statt
  einer je Eintrag. Beim ersten Start übernimmt die App die alten Einträge (dabei fragt macOS ein
  letztes Mal je Eintrag), schreibt den Sammel-Eintrag und löscht die alten erst danach.
- Schreiben nur aus einem Prozess mit Sperre, damit gleichzeitige Änderungen nichts überschreiben;
  bricht die Übernahme ab, bleiben die alten Einträge gültig.
- Windows bleibt bei einem Eintrag je Geheimnis: Die Anmeldeinformationsverwaltung fragt nicht,
  und ein Eintrag fasst dort höchstens 2.560 Bytes.
- Entscheidung festhalten; `docs/SECURITY_MODEL.md` und `docs/PRIVACY.md` anpassen. Tests nur
  gegen den Testdienst `org.vicemail.desktop.e2e`.

### 5 — Kontakte in der Suche

- Plan: Beim Tippen in die Suche stehen oben bis zu drei passende Kontakte (Name, Adresse oder
  Domain, auch per Anfang). Auswahl zeigt alle Mails von und an diese Person über alle Konten
  (Absender, Empfänger oder CC, alle Adressen des Kontakts). Mit Tastatur bedienbar; die normale
  Volltextsuche darunter bleibt.

### 6 — Lernender Spamfilter (lokal)

- Plan: Ein Filter, der lokal aus „Spam“ und „Kein Spam“ lernt, wie bei Thunderbird; ohne Server,
  ohne neue Abhängigkeit. Er wertet Kopfzeilen und Text der Mails aus.
- Erkannte Mails gehen über die Outbox in den Spam-Ordner, mit Rückgängig. Bei nur lesenden
  Konten oder ohne Spam-Ordner werden sie nur als „Spam-Verdacht“ markiert.
- Bekannte Kontakte, eigene Adressen und Antworten in eigenen Gesprächen gelten nie als Spam.
  Blockierte Absender bleiben wie bisher.
- Standard aus. Beim Einschalten kann er aus dem vorhandenen Spam-Ordner lernen. Läuft nur,
  solange die App läuft, und ist so beschriftet.
- Größter Punkt der Liste. Die Trefferquote wird mit einem Testkorpus gemessen, und die Zahlen
  werden hier eingetragen.

### 7 — Suche mitten im Wort

- Plan: Zusätzlich zum Wortindex ein Index aus Dreiergruppen von Zeichen (FTS5 `trigram`), mit
  dem `uster` auch „Muster“ findet und `1234` eine Nummer mitten im Wort.
  - Sicher für Absender, Empfänger, Betreff und Dateinamen.
  - Für den Mail-Text erst nach Messung von Größe und Tempo bei 100.000 Mails; das Ziel bleibt
    unter 200 ms bis zu den ersten Treffern.
- Suchbegriffe mit weniger als drei Zeichen nutzen weiter den Wortindex. Der neue Index wird
  im Hintergrund aufgebaut wie der bestehende.

### 8 — Einstellungen durchsuchen

- Plan: Suchfeld oben in den Einstellungen. Es findet jede Option über Titel und Beschreibung in
  der eingestellten Sprache (z. B. „Signatur“, „Ton“), springt dorthin und hebt die Option kurz
  hervor. Mit Tastatur bedienbar (Cmd/Ctrl+F in den Einstellungen).

### 9 — Windows-11-Optik

- Plan: Fensterhintergrund mit Mica wie bei Windows-11-Apps, Titelleiste in den Farben der App,
  beim Überfahren des Maximieren-Knopfs die Andock-Leiste von Windows.
- Auf dem Mac nicht prüfbar. Die CI prüft auf Windows nur den Build; wie es aussieht, muss der
  Mensch auf dem PC ansehen.

### 10 — Passwörter von einem anderen Gerät übernehmen

- Gewünscht (26.11.): Nach dem Beitreten zum Einstellungs-Sync die Passwörter nicht pro Konto neu
  eintippen müssen. Heute starten übernommene Konten mit „Passwort fehlt“ (`docs/SYNC.md`,
  „Accounts taken over“).
- Plan:
  - Der neue Rechner erzeugt ein Schlüsselpaar nur für diese Übernahme. Den öffentlichen Teil
    legt er als Anfrage in den Sync-Ordner. Die Anfrage läuft nach 15 Minuten ab.
  - Beide Geräte zeigen denselben kurzen Code. Auf einem eingerichteten Gerät bestätigt der
    Mensch, dass die Codes gleich sind, und wählt die Konten.
  - Erst dann verschlüsselt das alte Gerät die Passwörter für genau diesen Schlüssel. Der neue
    Rechner holt sie ab, legt sie in den Schlüsselbund und löscht Anfrage und Antwort aus dem
    Ordner.
- Nur für Konten, deren Server auf beiden Geräten genau gleich eingetragen sind (Grenze aus
  Entscheidung 0040). Ein Passwort geht also nie an einen Server, den nur der Ordner nennt.
- Schreibzugriff bleibt auf dem neuen Gerät aus. OAuth-Konten melden sich auf jedem Gerät selbst
  an.
- Der private Schlüssel liegt nur im Speicher, ein Neustart verwirft die Anfrage. Die Passwörter
  liegen nie dauerhaft im Postfach.
- Entscheidung festhalten; `docs/SYNC.md`, `docs/SECURITY_MODEL.md` und `docs/PRIVACY.md`
  anpassen.

## Ergebnis

Noch offen; die Liste wird gesammelt.
