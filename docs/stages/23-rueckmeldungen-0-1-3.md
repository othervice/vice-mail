# Stufe 23 — Rückmeldungen zu 0.1.3 und neues Erscheinungsbild

**Ziel:** Rückmeldungen zu 0.1.3 sammeln und gesammelt umsetzen.

**Anlass:** Test von 0.1.3 am 26.11.2026. Wie in Stufe 21 und 22: erst alle Rückmeldungen
sammeln, gebaut wird erst, wenn der Mensch sagt, dass die Liste vollständig ist.

**Stand:** Fertig am 27.11.2026, ergibt 0.1.4.

## Rückmeldungen

### 1 — Postfächer links per Ziehen umsortieren (geht nicht)

- Gemeldet (26.11.): „Ganz links in der Leiste muss ich die einzelnen Postfächer nehmen und per
  Drag-and-drop verschieben können, um die Reihenfolge zu ändern.“
- Stand im Code: Seit 0.1.3 lässt sich die Überschrift eines Kontos in der Ordnerspalte ziehen
  (Stufe 22, Punkt 1). In der schmalen Leiste (Konten als Farbpunkte) geht es nicht.
- Gefundene Ursache für Windows: Tauri fängt Dateien, die auf das Fenster gezogen werden, mit
  einem eigenen Drop-Handler ab. Unter Windows ersetzt dieser den Drop-Handler von WebView2 und
  schaltet damit das Ziehen innerhalb der Seite ab (`draggable`, HTML Drag and Drop), so steht
  es in wry 0.57.3, `with_drag_drop_handler`. Unter Windows funktioniert damit **kein** Ziehen in
  der App: weder Konten umsortieren noch Mails in Ordner ziehen (Stufe 16 und Stufe 22,
  Punkt 12). Die Desktop-Tests laufen auf dem Mac und lösen die Ereignisse in der Seite aus,
  deshalb fiel das nicht auf.
- Plan:
  - Den Drop-Handler von Tauri für alle Fenster abschalten (auf beiden Plattformen).
    Dateien, die auf den Composer gezogen werden, kommen dann als HTML-`File` mit ihrem Inhalt
    statt mit ihrem Pfad. Das ist sicherer als heute: Die Seite kann nur Dateien liefern, die der
    Mensch gezogen hat, und nie einen Pfad nennen (Entscheidung 0018 anpassen).
  - Konten zusätzlich in der schmalen Leiste per Ziehen umsortieren.
  - Umsortieren wie bei Thunderbird: Linie an der Einfügestelle, Loslassen ordnet ein, bleibt
    nach einem Neustart.
  - Prüfung mit echtem Ziehen über das Betriebssystem (auf dem Mac per CGEvent, falls macOS das
    erlaubt); Windows auf der Prüfliste.
- Antwort des Menschen (26.11.): auf beiden Geräten mit 0.1.3 probiert. Unter Windows geht es gar
  nicht. Auf dem Mac lässt sich ein Postfach aufnehmen und ziehen, bleibt aber nicht dort, wo
  man es loslässt. Gewünscht wie bei Thunderbird: Postfach nehmen, ziehen, eine Linie zeigt, wo
  es eingeordnet wird, loslassen, neue Reihenfolge.
- Ursache auf dem Mac: `tauri-runtime-wry` 2.12.5 (`lib.rs`, `with_drag_drop_handler`) meldet jedes
  Zieh-Ereignis als erledigt (`true`). wry 0.57.3 (`wkwebview/drag_drop.rs`) reicht es dann nicht
  an WKWebView weiter: Die Seite bekommt `dragstart`, aber kein `dragover` und kein `drop`. Auch
  Mails auf einen Ordner ziehen geht deshalb auf dem Mac nicht.

### 2 — Neues Erscheinungsbild, Richtung Mailbird und Canary

- Gefragt (26.11.): wie die App professioneller, hochwertiger und „weniger nach KI“ wirken kann.
  Ein erster Vorschlag (vor allem Texte kürzen) wurde verworfen („das polishing mit dem weniger
  text ist quatsch“). Gemeint ist das Erscheinungsbild: Der Mensch zeigte Bildschirmfotos von
  Mailbird („super polished, sowas in der Richtung“) und Canary Mail („kannst du da einen guten
  Mix aus beidem machen?“).
- Übernommene Prinzipien (kein Nachbau): von Mailbird helle, ruhige Flächen, eine eigene
  einheitliche Schrift (Inter), dichte saubere Liste, großer Betreff, Anhänge als Karten, neue
  Mail als schwebende Karte mit Formatleiste und Symbolreihe; von Canary die Werkzeugleiste über
  die ganze Breite, die Seitenleiste nach Konten gegliedert mit Symbolen in der Kontofarbe, ein
  Farbstreifen je Konto an der Zeile, kräftig gefüllte Auswahl, Bänder „Heute/Gestern“, eine
  Kontaktspalte rechts (nur Daten der App: Name, Adresse, Notiz, Mails, Gruppen; keine Netzwerke)
  und das dunkle Design. Keine Bilder und Initialen (Stufe 22).
- Entwurf: https://claude.ai/artifact/BuSPYpDvmSDm4s1JJoSZFX (Posteingang hell mit Kontaktspalte,
  neue Mail als Karte, dunkel). **Freigegeben am 26.11.** („ja das dunkle design finde ich super.
  das nehmen wir.“): dunkel wie im Entwurf, hell mit demselben Aufbau. Die Quelle des Entwurfs
  (`Main.dc.html`) ist die Vorlage für Maße und Farben.
- Maße und Farben aus dem Entwurf:
  - Schrift Inter (mitgeliefert, OFL, kein Laden aus dem Netz), Ziffern gleich breit für Zeiten
    und Zähler. Größen: Zeilen 13 px (Absender, Betreff), Vorschau 12 px, Zeit 11 px,
    Abschnittsköpfe 11 px halbfett, Betreff im Lesebereich 26 px fett, Mailtext 14 px mit 1,65.
  - Dunkel: Fläche `#14181e`, Seitenleiste und Werkzeugleiste `#1a1f27`, Bänder `#1b2129`, Linien
    `#29303b`, Text `#e8edf3` / `#bcc5d0` / `#98a3b0`, Akzent als Text `#6aa8ff`, Auswahl und
    „Senden“ `#2563eb`, Auswahl in der Seitenleiste `#2a3341`, Chips `#252c37`, Knöpfe `#232a34`
    mit Rand `#333b48`, Karten `#1b2129`, Kontaktspalte `#171c23`.
  - Hell: Fläche `#ffffff`, Seitenleiste `#f2f4f7`, Bänder `#f6f7f9`, Linien `#e5e8ec`, Text
    `#131a24` / `#455061` / `#5b6676`, Akzent `#1463d8`, Auswahl in der Seitenleiste `#e2e8f1`,
    Chips `#eef1f5`, Werkzeugleiste `#f7f8fa`, Knöpfe weiß mit Rand `#dde1e7`, Kontaktspalte
    `#f8f9fb`.
  - Kontofarben aus der bestehenden Palette, als Farbe der Symbole in der Seitenleiste, als
    Quadrat am Kontonamen und als 3-px-Streifen am rechten Rand jeder Zeile.
- Umsetzung in Schritten:
  1. **Grundlage:** Schrift, Farben, Größen; Werkzeugleiste über die ganze Breite mit
     gruppierten Knöpfen und Suchfeld (macOS in der Titelleiste neben den Ampeln, Windows unter
     der nativen Mica-Titelleiste).
  2. **Seitenleiste:** „Alle Konten“ (Posteingang, Später, Markiert, Gesendet, die übrigen
     Ansichten unter „Mehr“), „Favoriten“, je Konto ein Abschnitt (Posteingang, Gesendet,
     „Weitere Ordner“), Abschnitte per Ziehen umsortieren (Punkt 1), unten „Abgeglichen um …“,
     Abgleichen, Einstellungen. Keine Funktion geht verloren.
  3. **Liste, Lesebereich, Kontaktspalte:** Zeilen mit Kontostreifen, kräftig gefüllte Auswahl,
     Bänder nach Datum; Lesebereich ohne Karte, großer Betreff, ältere Mails einzeilig, Anhänge als
     Karten je Dateityp, Schnellantwort unten; Kontaktspalte rechts (ein- und ausblendbar) mit
     Name, Adresse, Notiz, letzten Mails, Gruppen.
  4. **Neue Mail:** schwebende Karte über dem Lesebereich (eigenes Fenster bleibt möglich),
     Betreff als Titel, Chips für An und Von, kompakte Formatleiste, „Senden ▾“ (später senden),
     Symbolreihe, alles andere im „…“-Menü.

## Ergebnis

Erledigt am 26./27.11.2026, ergibt 0.1.4. Entscheidungen 0082 (Ziehen ohne Drop-Handler der
Shell), 0083 (neues Erscheinungsbild: Inter, Farben, Größen), 0084 (Werkzeugleiste, Liste,
Lesebereich, Kontaktspalte), 0085 (Seitenleiste nach Konten), 0086 (neue Mail als schwebende
Karte), 0087 („Alle Konten“ über alle Konten). Keine Migration; neue Einstellungen auf dem Gerät:
`ui.sidebar_folds`, `contactPanel` im Fensterlayout. Neue Schrift: Inter 4.1 (OFL, mitgeliefert).

- **1:** Tauris Drop-Handler ist für alle Fenster aus. Dateien aus Finder und Explorer kommen als
  Inhalt über Upload-Befehle in den Kern (in Stücken zu 2 MiB, gleiche Grenzen wie die
  Dateiauswahl); die Seite kann keinen Pfad mehr nennen, `DropRegistry` ist weg. Konten in Spalte
  und schmaler Leiste per Ziehen umsortieren wie bei Thunderbird (Linie, gedimmt, Esc,
  Alt+Pfeil). Mails auf Ordner ziehen geht wieder. Nebenbei: Ziehen im Editor und eine
  CSP-Meldung durch `cid:`-Bilder behoben.
- **2:** In vier Schritten nach dem freigegebenen Entwurf umgesetzt und danach zusammengeführt:
  Grundlage (Schrift, Farben, Größen), Seitenleiste, Werkzeugleiste mit Liste, Lesebereich und
  Kontaktspalte, neue Mail als Karte. Beim Zusammenführen: Karte unter der neuen Werkzeugleiste,
  Kontaktspalte weicht beim Schreiben; Später, Markiert und Gesendet unter „Alle Konten“ über alle
  Konten (neue Befehle `listUnifiedSent`/`listUnifiedSentThreads`, 2,17 ms bei 100.000 Mails im
  Debug-Build). Kontrast: alle Farbpaare in 12 Themen bestehen, auch auf Mica.

**Geprüft:**
- `scripts/check.sh` grün auf dem zusammengeführten Stand (1.673 Rust-Tests, 6.277 Vitest),
  einschließlich Clippy für Windows.
- Server-Tests gegen den lokalen Testserver: 1.790 grün.
- Desktop: 45 Spec-Dateien; im letzten Gesamtlauf 44 grün, `background.e2e.mjs` scheiterte an
  einer Pause über Mitternacht („bis morgen, 0:06“), Muster erweitert, einzeln 8/8 grün.
- README-Bilder neu. Bildschirmfotos aller Schritte mit dem Entwurf verglichen; der Mensch fand
  das Ergebnis „super“.

**Ungeprüft:**
- Echtes Ziehen mit der Maus (macOS lässt synthetische Mausereignisse ohne Bedienungshilfen-
  Freigabe nicht zu; die Tests lösen die Ereignisse in der Seite aus).
- Alles auf Windows: Werkzeugleiste unter Mica, Schrift in WebView2, Ziehen, Seitenleiste
  (Prüfliste 23.1–23.4, 24.1–24.2).
- Geschwindigkeit im Release-Build.

**Wartet auf:** Test von 0.1.4 auf Mac und Windows, besonders Ziehen (Konten, Mails, Dateien) und
das neue Erscheinungsbild unter Windows.
