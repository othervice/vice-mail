# Stufe 23 — Rückmeldungen zu 0.1.3 und Feinschliff

**Ziel:** Rückmeldungen zu 0.1.3 sammeln und gesammelt umsetzen; dazu ein Feinschliff der
Oberfläche, damit die App hochwertiger und professioneller wirkt.

**Anlass:** Test von 0.1.3 am 26.11.2026. Wie in Stufe 21 und 22: erst alle Rückmeldungen
sammeln, gebaut wird erst, wenn der Mensch sagt, dass die Liste vollständig ist.

**Stand:** Sammeln (seit 26.11.2026). Noch nichts umgesetzt.

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

### 2 — Feinschliff: hochwertiger, professioneller, „weniger nach KI“

- Gewünscht (26.11.): „Wie man das Ganze noch mehr polishen kann, sodass es professioneller,
  besser aussieht, hochwertiger, weniger nach KI-Slop.“
- Beobachtungen (aus den Bildschirmfotos von 0.1.3):
  - Zu viel Text: lange Beschreibungen unter fast jeder Einstellung, erklärende Sätze in
    Dialogen und Meldungen, Hinweise wie „oder Dateien auf das Fenster ziehen“.
  - Zu viele Kästen und Pillen: Mail als Karte im Lesebereich, Chip „In Personen · Warum hier?“,
    runde Knöpfe im Schnellfilter, Kürzel-Chip neben „Senden“.
  - Composer zeigt alles auf einmal: Rich/Plain-Umschalter, volle Formatleiste, Signatur,
    Zitat-Optionen, Archivieren, Lesebestätigung, Erinnern, „Später senden“ als eigener Knopf.
  - Werkzeugleiste des Lesebereichs mit „Schreibgeschützt · Änderungen erlauben …“ als Text.
  - Seitenleiste mit zehn Ansichten untereinander, Zähler überall gleich laut.
- Vorgeschlagen am 26.11.: weniger Text, neue Mail aufräumen, ruhigere Listen und Lesebereich,
  strenges Design-System. **Gewählt: nur „Weniger Text“**, und zwar mit Entwürfen vorab: Erst
  Vorher/Nachher für einige Bereiche zeigen, nach der Freigabe alle Texte (Deutsch und Englisch)
  so kürzen.
- Plan: eine kurze Schreibregel für UI-Texte (Ton, Länge, was nie erklärt wird) und Entwürfe für
  Einstellungen → Allgemein und Posteingang (mit Spamfilter), Einstellungs-Sync, die neue Mail
  und die häufigsten Meldungen und Dialoge, mit Bildschirmfotos vorher und nachher.

## Ergebnis

Noch offen; die Liste wird gesammelt.
