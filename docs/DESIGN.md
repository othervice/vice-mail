# Oberfläche

## Richtung

Hell, luftig, eine Zeile pro Mail, der Inhalt steht im Vordergrund. Die Vorlage liegt lokal unter
`design-reference/inbox-reference.jpg`. Sie ist ein Screenshot eines fremden Produkts und dient nur
als Orientierung für den Aufbau. Übernommen wird die Struktur, nicht das Aussehen: eigene Farben,
eigene Icons, eigene Typografie. Die Datei wird nicht eingecheckt und nirgends veröffentlicht.

Was aus der Vorlage übernommen wird:

- schmale Icon-Leiste links statt breiter Ordnerspalte
- breite Liste mit einzeiligen Zeilen: Absender, Betreff, Vorschau, Zeit
- Abschnitte nach Tag („Heute“, „Gestern“), darüber ein abgesetzter Block „Priorität“
- gebündelte Zeilen für Benachrichtigungen und Newsletter mit Absender-Chips und Zählern
- Schnellaktionen, die beim Überfahren in der Zeile erscheinen
- KI-Assistent als Seitenpanel rechts, Ergebnisse als Karten mit „Entwurf öffnen“

## Aufbau

```
┌────┬──────────────────────────────────────────────┬───────────────┐
│    │ Posteingang ▾                    ⌕   ✎       │               │
│ I  │ Fokussiert                                   │  KI-Panel     │
│ c  ├──────────────────────────────────────────────┤  (einklappbar)│
│ o  │ ⚡ Priorität                                  │               │
│ n  │  ● Absender   Betreff  Vorschau…      11:50  │               │
│ -  │ Heute                                        │               │
│ L  │  ● Absender   Betreff  Vorschau…      16:09  │               │
│ e  │  ● Benachrichtigungen 116  [Chip] [Chip] …  ›│               │
│ i  │ Gestern                                      │               │
│ s  │  ● Absender   Betreff  Vorschau…     27. Okt │               │
│ t  │                                              │               │
│ e  │                                              │               │
└────┴──────────────────────────────────────────────┴───────────────┘
```

**Icon-Leiste** (56 px). Von oben: vereinter Posteingang, Später, Aufgaben, Angepinnt, Gesendet,
Entwürfe, Archiv, Papierkorb, Ordner, gespeicherte Ansichten. Unten: Konten-Umschalter mit
Kontofarben, Einstellungen. Jedes Icon hat einen Tooltip mit Name und Tastenkürzel. Die Leiste lässt
sich zu einer beschrifteten Seitenleiste mit Ordnerbaum aufklappen.

**Kopf der Liste.** Titel der Ansicht mit Auswahl von Konto oder Kontogruppe, darunter eine
Unterzeile (z. B. „Fokussiert“ oder „Alle“). Rechts: Kategorien an/aus, Suche, Neue Mail.

**Liste.** Abschnitt „Priorität“ auf getönter Fläche, höchstens drei Zeilen und „Alle anzeigen (n)“.
Danach Abschnitte nach Datum: Heute, Gestern, Diese Woche, dann nach Monat. Bündel sind eine Zeile
mit Symbol, Name, Zähler und Chips der häufigsten Absender; ein Klick klappt sie an Ort und Stelle auf.

**Zeile.** Höhe 44 px (komfortabel) oder 34 px (kompakt).

| Element | Verhalten |
|---|---|
| Punkt links | ungelesen; beim Überfahren und bei Auswahl wird er zum Auswahlkästchen |
| Avatar | Initialen auf Farbfläche, Ring in der Kontofarbe. Keine Bilder aus dem Netz. |
| Absender | feste Spalte, halbfett wenn ungelesen |
| Betreff | Haupttext, danach Zähler der Konversation und Anhang-Symbol |
| Vorschau | gedämpft, abgeschnitten |
| Zeit | rechtsbündig, Tabellenziffern; heute Uhrzeit, sonst Datum |
| Schnellaktionen | beim Überfahren und bei Tastaturfokus rechts: Löschen, Archivieren, Gelesen, Anpinnen, Verschieben, Später, Mehr |

Ist die Liste schmaler als 720 px, werden Zeilen zweizeilig: Absender und Zeit oben, Betreff unten.

**Lesebereich.** Öffnet rechts neben der Liste; die Liste wird schmaler und zweizeilig. Alternativ
unten oder anstelle der Liste. Kopf mit Betreff, Absender, Empfängern, Zeit und Kontofarbe; Aktionen
als Symbolleiste. Ältere Mails eines Threads sind eingeklappt, ungelesene offen.

**KI-Panel.** 380 px, rechts, einklappbar. Bezieht sich auf den gewählten Thread. Gespräch oben,
Eingabefeld unten, Ergebnisse als Karten. Bei schmalem Fenster liegt es über dem Lesebereich.
Ohne eingerichtete KI ist das Panel nicht sichtbar.

**Verfassen.** Eigener Bereich über dem Lesebereich oder eigenes Fenster. Empfänger als Chips,
schmale Formatierungsleiste, Anhänge als Leiste unten.

## Tokens

Alle Werte sind CSS-Variablen in `app/src/styles/tokens.css`. Komponenten benutzen nur Tokens, nie
feste Farben oder Abstände.

| Gruppe | Tokens |
|---|---|
| Flächen | `canvas`, `surface`, `surface-raised`, `rail`, `priority-tint`, `selected`, `hover` |
| Linien | `border`, `border-strong`, `focus-ring` |
| Text | `text-primary`, `text-secondary`, `text-tertiary`, `text-on-accent` |
| Akzent | `accent`, `accent-soft`, `danger`, `success`, `warning` |
| Konten | acht unterscheidbare Kontofarben, in beiden Modi geprüft |
| Schrift | Systemschrift (SF Pro, Segoe UI Variable); Größen 12, 13, 14, 16, 20, 28; Gewichte 400, 500, 600 |
| Abstand | Raster aus 4 px: 4, 8, 12, 16, 24, 32, 48 |
| Rundung | 6 (Bedienelemente), 10 (Karten, Zeilen), 16 (Panels, Fenster in der App) |
| Schatten | zwei Stufen: schwebend (Menüs), Panel |
| Bewegung | 120, 180, 240 ms, weich auslaufend; bei „Bewegung reduzieren“ keine |

Startwerte: neutrale, leicht warme Grautöne; ein einzelner Akzent in Blaugrün; die Prioritätsfläche
in warmem Sand. Der Akzent ist ein Token und in einer Zeile änderbar. Der dunkle Modus ist kein
invertierter heller: eigene Flächenstufen, gedämpfter Akzent, Kontrast nach `docs/SPEC.md`.

HTML-Mails werden auf ihrem eigenen Hintergrund gezeigt und im dunklen Modus nicht umgefärbt.
Reine Textmails folgen dem Thema.

## Regeln für Interaktion

- Jede Aktion wirkt sofort in der Oberfläche; der Abgleich mit dem Server läuft dahinter.
- Rückgängig-Hinweis statt Bestätigungsdialog. Ein Dialog nur bei endgültigem Löschen.
- Keine Dialoge für Alltägliches. Einstellungen und Konten öffnen als eigene Ansicht, nicht als Fenster im Fenster.
- Bewegung nur, wenn sie zeigt, wohin etwas geht: Panel schiebt herein, entfernte Zeile klappt zu.
- Der Tastaturfokus ist immer sichtbar und folgt der Auswahl. Nach einer Aktion steht er auf der nächsten Mail.
- Zustände sind gestaltet: leer (mit nächstem Schritt), lädt (Skelett nur beim ersten Laden),
  offline (dezenter Hinweis, alles Lokale bleibt benutzbar), Fehler (an der Stelle, mit Lösungsweg).
- Ausstehende und fehlgeschlagene Aktionen sind sichtbar, aber unaufdringlich: kleines Symbol in der
  Zeile, Sammelanzeige in der Leiste.
