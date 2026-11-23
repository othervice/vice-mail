# Stufe 10 — Composer vollständig

**Ziel:** Der Composer hat alles, was im Alltag gebraucht wird: Signaturen, Vorlagen, Inline-Bilder,
Kontakte und geplantes Senden.

**Voraussetzung:** Stufe 05 fertig (Stufe 08 empfohlen).

**Aus Stufe 5 vorhanden (0018):** Tabelle `contact` (eine Zeile je Adresse, `contact_group` für
Zusammenführungen noch ungenutzt), Autovervollständigung, Text-Signatur je Identität
(`identity.signature`), Zitat-Position je Entwurf (`quotePosition`), `not_before` an Operationen
(für „Später senden“ nutzbar: eine `send`-Operation mit spätem `not_before`).

## Aufgaben

- [ ] **Formatierung.** Durchgestrichen, Überschriften, Code, Ausrichtung, Rückgängig/Wiederholen. Eingefügter Inhalt aus Word oder Webseiten wird bereinigt.
- [ ] **Inline-Bilder.** Einfügen und Hineinziehen, als `cid:`-Teil in der Mail, Größe änderbar.
- [ ] **Rechtschreibprüfung.** Die des Systems. Verhalten und Sprachwahl in WKWebView und WebView2 prüfen und festhalten.
- [ ] **Signaturen.** Mehrere, als HTML mit Bildern und Links. Standard je Identität, getrennt für neue Mails und Antworten. Im Composer wechselbar.
- [ ] **Vorlagen.** Betreff und Text speichern, mit einfachen Platzhaltern (Vorname des Empfängers). Einfügen über die Command Palette.
- [ ] **Kontakte.** Lokales Verzeichnis aus gesendeten und empfangenen Mails, sortiert nach Häufigkeit und Aktualität; Adressen ohne Antwortmöglichkeit ausgenommen. Namen bearbeiten. Dubletten erkennen (gleiche Adresse, verschiedene Namen; gleicher Name, ähnliche Adresse) und zusammenführen, nur auf Bestätigung.
- [ ] **Später senden.** Lokal. Die Oberfläche sagt bei der Auswahl und in „Ausgang“ klar: Die Mail wird gesendet, wenn die App zu diesem Zeitpunkt läuft und online ist. War die App zum Zeitpunkt aus und ist mehr als eine Stunde vergangen, fragt sie beim nächsten Start: jetzt senden, neu planen oder als Entwurf behalten.
- [ ] **Fenster.** Composer in eigenem Fenster; mehrere Entwürfe gleichzeitig.
- [ ] **Größe.** Warnung, wenn Anhänge die Grenze des Servers überschreiten (SMTP-Erweiterung SIZE).
- [ ] **Antworten und Weiterleiten.** Einstellung für die Position des Zitats. Weiterleiten als Anhang (`.eml`).

## Abnahme

- [ ] Snapshot-Tests für das erzeugte HTML mit Signatur, Inline-Bild und Zitat; der reine Textteil ist lesbar
- [ ] Test: Inline-Bild kommt beim zweiten Testkonto an und wird dort angezeigt
- [ ] Tests für Kontakte: Rangfolge, Dubletten-Vorschläge, kein Zusammenführen ohne Bestätigung
- [ ] Tests für Später senden mit gestellter Uhr: pünktlich, offline zum Zeitpunkt, App war aus (unter und über einer Stunde)
- [ ] Test: Eingefügter Inhalt aus einer Word- und einer Web-Vorlage enthält danach kein Script und keine externen Verweise
- [ ] von Hand geprüft und vermerkt: Rechtschreibprüfung auf beiden Systemen
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Abgleich mit externen Adressbüchern (CardDAV)
- Geplantes Senden über einen Server
- KI-Schreibhilfe (Stufe 13)

## Der Mensch prüft danach

- Eigene Signaturen anlegen und eine Mail an einen anderen Anbieter senden: Sieht die Signatur dort richtig aus?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
