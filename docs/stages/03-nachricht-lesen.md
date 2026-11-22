# Stufe 03 — Nachricht lesen

**Ziel:** Mails lassen sich öffnen und sicher lesen, mit Anhängen und auch ohne Netz. HTML aus Mails
kann weder Code ausführen noch unbemerkt nach außen funken.

**Voraussetzung:** Stufe 02 fertig. Abschnitt „Sicherheitsgrenzen“ in `docs/ARCHITECTURE.md` und die
Entscheidung aus Spike D gelesen.

## Aufgaben

- [ ] **Body holen und ablegen.** Beim Öffnen die Mail mit `BODY.PEEK[]` holen (verändert den Gelesen-Status nicht). Bei sehr großen Mails zuerst die Textteile, Anhänge erst auf Wunsch. Ablage komprimiert als Datei im Datenverzeichnis, atomar geschrieben, Verweis in der Datenbank. Kurz gegen Ablage in der Datenbank messen und die Wahl festhalten.
- [ ] **Vorausladen.** Im Hintergrund die Bodies der letzten 30 Tage für Posteingang, Gesendet, Entwürfe und Markiertes laden. Zeitraum einstellbar (7, 30, 90, 365 Tage, alles), mit Größenobergrenze.
- [ ] **Parsen.** Mit `mail-parser` in eine eigene Struktur: Text, HTML, Anhänge, Inline-Teile, Kopfzeilen. Zeichensätze, kodierte Kopfzeilen, kaputte Mails ohne Absturz. Vorschautext erzeugen und in `message` speichern; die Liste zeigt ihn ab jetzt.
- [ ] **HTML-Pipeline.** Wie in der Architektur: bereinigen, externe Adressen ersetzen, `cid:`-Bilder lokal auflösen, Anzeige im abgeschotteten `iframe`.
- [ ] **Externe Inhalte.** Standard blockiert, mit Hinweisleiste: einmal laden oder für diesen Absender immer laden. Freigegebene Bilder lädt der Rust-Kern ohne Cookies und Referrer, mit Größen- und Typgrenzen.
- [ ] **Links.** Ein Klick zeigt das echte Ziel. Warnung, wenn Linktext und Ziel verschiedene Domains nennen oder die Domain Verwechslungszeichen enthält. Öffnen im System-Browser, nur `http`, `https`, `mailto`.
- [ ] **Anhänge.** Liste mit Name, Größe, Typ. Speichern über den Systemdialog, Dateiname bereinigt. Öffnen nur nach Klick; bei ausführbaren Typen eine Warnung. Nichts öffnet sich von selbst.
- [ ] **Lesebereich.** Nach `docs/DESIGN.md`: Kopf mit Absender, Empfängern, Zeit, Kontofarbe; HTML auf eigenem Hintergrund, reiner Text im Thema der App; Umschalter HTML/Text.
- [ ] **Testkorpus.** Feindliche Mails unter `testmail/corpus/hostile/`: Script-Tags, Ereignis-Attribute, `javascript:`-Links, SVG mit Script, CSS mit `url()` und `@import`, `meta refresh`, Formulare, `base`, eingebettete Objekte und Frames, Zählpixel. Dazu Anhänge mit gefährlichen Namen (`../../x`, `C:\…`, `CON`, Null-Bytes, Richtungsumkehr-Zeichen).

## Abnahme

- [ ] eine lokal vorhandene Mail öffnet sich in unter 100 ms (gemessen)
- [ ] Unit-Tests: Die bereinigte Ausgabe des gesamten feindlichen Korpus enthält keines der verbotenen Elemente
- [ ] Desktop-Test mit WebdriverIO: Beim Anzeigen des Korpus läuft kein Script und der lokale Zähl-Server registriert null Zugriffe; nach Freigabe eines Absenders werden dessen Bilder geladen
- [ ] Tests: Jeder gefährliche Dateiname wird zu einem harmlosen; ein gespeicherter Anhang hat dieselbe Prüfsumme wie das Original
- [ ] vorausgeladene Mails lassen sich bei gestopptem Testserver öffnen
- [ ] kaputte Mails aus dem Seed führen zu einer lesbaren Anzeige oder einer klaren Meldung, nie zu einem Absturz
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Als gelesen markieren und alle anderen Aktionen (Stufe 4)
- Thread-Ansicht (Stufe 7), Kalendereinladungen und Abmelden (Stufe 9)
- Drucken und Export (Stufe 9)

## Der Mensch prüft danach

- Auf dem echten Konto verschiedene Mails lesen: Newsletter, Rechnungen mit PDF, Mails mit Umlauten und Bildern. Was sieht falsch aus?

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
