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
  - Den Drop-Handler von Tauri für die Fenster abschalten (`dragDropEnabled: false`).
    Dateien, die auf den Composer gezogen werden, kommen dann als HTML-`File` mit ihrem Inhalt
    statt mit ihrem Pfad. Das ist sicherer als heute: Die Seite kann nur Dateien liefern, die der
    Mensch gezogen hat, und nie einen Pfad nennen (Entscheidung 0018 anpassen).
  - Konten zusätzlich in der schmalen Leiste per Ziehen umsortieren.
  - Ein Test, der auf Windows echtes Ziehen auslöst (CI-Runner), wenn sich das machen lässt;
    sonst steht es auf der Windows-Prüfliste.
- Frage an den Menschen: Auf welchem Gerät und mit welcher Version wurde es probiert?

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
- Vorschläge stehen in der Antwort vom 26.11.; was gewählt wird, kommt hierher.

## Ergebnis

Noch offen; die Liste wird gesammelt.
