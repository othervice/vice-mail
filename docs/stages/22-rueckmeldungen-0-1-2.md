# Stufe 22 — Rückmeldungen zu 0.1.2

**Ziel:** Alles, was der Mensch beim Test von 0.1.2 findet, gesammelt umsetzen und in einem Zug
bauen.

**Anlass:** Test von 0.1.2 am 26.11.2026. Wie in Stufe 21: erst alle Rückmeldungen sammeln,
gebaut wird erst, wenn der Mensch sagt, dass die Liste vollständig ist.

**Stand:** Sammeln (seit 26.11.2026). Noch nichts umgesetzt.

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

## Ergebnis

Noch offen; die Liste wird gesammelt.
