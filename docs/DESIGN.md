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

Seit 23.11.2026 gibt es eine zweite Vorlage für die Qualität der Ausführung,
`design-reference/polish-reference.png` (ebenfalls fremd, nicht eingecheckt). Was daraus folgt,
steht im Abschnitt „Feinschliff 23.11.2026“ und in `docs/decisions/0031-feinschliff-der-oberflaeche.md`.

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

**Zeile.** Höhe 44 px (komfortabel) oder 34 px (kompakt). In schmaler Liste 76 px mit drei Zeilen
(komfortabel) oder 52 px mit zwei Zeilen ohne Vorschau (kompakt).

| Element | Verhalten |
|---|---|
| Punkt links | ungelesen; beim Überfahren und bei Auswahl wird er zum Auswahlkästchen |
| Avatar | 32 px, Initialen auf einer weichen Tönung je Absender; Ring in der Kontofarbe nur, wo sich Konten mischen. Keine Bilder aus dem Netz. |
| Absender | feste Spalte, halbfett wenn ungelesen, sonst mittel |
| Betreff | Haupttext (mittel, wenn ungelesen), danach Zähler der Konversation als Pille und Anhang-Symbol |
| Vorschau | gedämpft, abgeschnitten |
| Zeit | rechtsbündig, Tabellenziffern; heute Uhrzeit, sonst Datum |
| Schnellaktionen | beim Überfahren und bei Tastaturfokus als schwebende Leiste am Zeilenende: Löschen, Archivieren, Gelesen, Anpinnen, Verschieben, Später, Mehr |

Ist die Liste schmaler als 720 px, werden Zeilen mehrzeilig: Absender und Zeit oben, darunter der
Betreff, darunter die Vorschau (nur komfortabel).

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
| Flächen | `canvas`, `rail` (dunkelste Stufe), `rail-hover`, `surface-list` (Listenspalte), `surface` (Lesebereich), `surface-raised` (Karten), `priority-tint`, `selected`, `hover`, `badge` (Pillen) |
| Linien | `border`, `border-subtle` (Haarlinien zwischen Bereichen), `border-strong`, `priority-border`, `focus-ring` |
| Text | `text-primary`, `text-secondary`, `text-tertiary`, `text-on-accent` |
| Akzent | `accent`, `accent-soft`, `danger`, `success`, `warning` |
| Konten | acht unterscheidbare Kontofarben, in beiden Modi geprüft |
| Avatare | acht Tönungen `avatar-n-bg` / `avatar-n-text`, Wahl per Hash der Absenderadresse, Schrift auf Fläche ≥ 4,5:1 |
| Schrift | Systemschrift (SF Pro, Segoe UI Variable); Größen 12, 13, 14, 16, 20, 28 (9 nur für das Initial im Mini-Avatar); Gewichte 400, 500, 600 |
| Abstand | Raster aus 4 px: 4, 8, 12, 16, 20, 24, 32, 40, 48 |
| Rundung | 6 (Bedienelemente), 8 (Zeilen, Kacheln), 10 (Karten), 16 (Panels, Fenster in der App); Pillen rund |
| Schatten | vier Stufen: Zeile (Haarbreite), Karte (Priorität, leere Zustände), schwebend (Menüs, Schnellaktionen), Panel |
| Bewegung | 120, 180, 240 ms, weich auslaufend; bei „Bewegung reduzieren“ keine |

Startwerte: neutrale, leicht warme Grautöne; ein einzelner Akzent in Blaugrün; die Prioritätsfläche
in warmem Sand. Der Akzent ist ein Token und in einer Zeile änderbar. Der dunkle Modus ist kein
invertierter heller: eigene Flächenstufen, gedämpfter Akzent, Kontrast nach `docs/SPEC.md`.

HTML-Mails werden auf ihrem eigenen Hintergrund gezeigt und im dunklen Modus nicht umgefärbt.
Reine Textmails folgen dem Thema.

## Feinschliff 23.11.2026

Ziel: die Qualität der zweiten Vorlage erreichen, ohne ihr Aussehen zu übernehmen (Entscheidung 0031).

**Flächen und Tiefe.** Drei tonale Stufen von außen nach innen: Leiste am dunkelsten, Listenspalte
leicht getönt, Lesebereich am hellsten; Karten (`surface-raised`) darüber. Bereiche trennt eine
Haarlinie (`border-subtle`) statt einer harten Linie. Erhabene Elemente bekommen weiche Schatten:
`shadow-card` für die Prioritätskarte und Kacheln leerer Zustände, `shadow-floating` für die
Schnellaktionen. Im dunklen Modus dieselben Stufen, die Leiste fast schwarz, Schatten kräftiger.

**Schrift in der Zeile.** Absender 14 px halbfett (ungelesen) oder mittel, Betreff mittel (ungelesen)
oder normal, Vorschau 13 px gedämpft, Zeit 12 px mit Tabellenziffern, ungelesen dunkler. In der
schmalen Liste drei Zeilen: Absender und Zeit, Betreff mit Plaketten, Vorschau.

**Avatare.** Rund, 32 px, Initialen auf einer weichen Tönung je Absender (acht Töne, hell und dunkel
mit gleicher Zuordnung). Der Kontoring erscheint nur, wo er Konten unterscheidet (vereinter
Posteingang, Suche über Konten, Kontoauswahl, Einstellungen).

**Priorität.** Eine warm getönte Karte mit Haarlinie und weichem Schatten, Titel mit Blitz in der
Warnfarbe, höchstens drei Zeilen, „Alle anzeigen (n)“ mittig als Abschluss.

**Bündel.** Symbolkachel in eigener Tönung (Benachrichtigungen blau, Newsletter bernstein), Name,
Zähler „6 · 2 neu“, Absender-Chips mit 16-px-Mini-Avatar und Anzahl, „+n“ für den Rest, Pfeil.
In schmaler Liste stehen die Chips unter dem Namen und umbrechen.

**Leiste und Kopf.** Schmale Symbolleiste mit Tooltips (Name und Tastenkürzel), aktive Ansicht mit
weicher Akzentfläche und Balken am Rand. Kopf ruhig: Titel 16 px halbfett, darunter eine kleine
Zeile (Konto oder Auswahl), rechts nur Symbole.

**Plaketten.** Kleine runde Pillen (`badge`) für die Zahl der Mails im Thread; Anhang als Symbol.

**Barrierefreiheit.** Bündel, „Alle anzeigen“ und „Zurück zum Posteingang“ sind Einträge der Liste
(Pfeiltasten, Enter, Leertaste), keine Buttons in der `listbox`. Jede Zeile wird über ihren Inhalt
benannt (sichtbarer Text in Lesereihenfolge plus verborgene Zustände), nicht über `aria-label`.

**Nicht übernommen.** Team-Funktionen (Zuweisen, Geteilt, Kommentare, Präsenz), fremde Farben,
Icons, Logos und Illustrationen, eine dunkle Leiste im hellen Modus, Absenderbilder.

### Lesebereich, Konversation und Verfassen

**Karten.** Der Betreff steht als ruhiger Titel (20 px halbfett) über der Mail. Jede Mail ist eine
Karte (`surface-raised`, Haarlinie `border-subtle`, `shadow-card`, Rundung 10). In einer Konversation
sind ältere Mails einzeilige Karten (Avatar, Absender, Anfang des Textes, Datum, Punkt für
ungelesen); die offene Mail ist an ihrer Stelle aufgeklappt, alles in einer Spalte (höchstens
`reading-column`, mittig). Textmails mit Zeilenhöhe 1,6; HTML-Mails auf ihrem weißen Blatt ohne
eigenen Rahmen. Unter der offenen Mail eine leise Zeile: Antworten, Allen antworten, Weiterleiten.

**Kopf der Karte.** Name und echte Adresse des Absenders bleiben immer sichtbar (gefälschte Namen
zeigen so ihre Adresse), daneben Datum und „…“ (Allen antworten, Als Anhang weiterleiten, In neuem
Fenster). Empfänger und Konto sind eine Zeile „an dich, Ethan Bennett +1“, bei mehreren Konten mit
Kontopunkt und Kontoname; eine abweichende Antwortadresse steht in der Warnfarbe in derselben Zeile.
Klick oder Enter klappt die vollständige Liste (An, Cc, Bcc, Antwort an, Konto) auf. Die Kategorie
ist ein kleiner Chip „Noch nicht eingeordnet · Warum hier?“; „Warum hier?“ bleibt ein Button mit
`aria-expanded`. Der Schalter HTML/Text sitzt klein über dem Text der Mail, nicht in der Leiste.

**Symbolleiste.** Höchstens sieben Aktionen in drei Gruppen mit feinen Trennlinien: Antworten,
Allen antworten, Weiterleiten (bei Entwürfen „Entwurf bearbeiten“) | Archivieren, Löschen (im
Papierkorb endgültig löschen, im Spam „Kein Spam“) | Gelesen/Ungelesen, Markieren. Alles andere
steht immer im Menü „Weitere Aktionen“ (…) mit Kürzel am Rand. Ist der Bereich zu schmal, wandern
auch Hauptaktionen ins Menü, Archivieren und Löschen zuletzt. Das Menü gehört zur Pfeiltasten-
Reihenfolge der Leiste (Pfeil nach unten öffnet, Escape schließt und gibt den Fokus zurück); die
Buttons im Menü bleiben verborgen im Dokument, Tastenkürzel und Befehlspalette gelten unverändert.

**KI im Lesebereich.** Darf die KI für das Konto arbeiten, stehen unter der offenen Mail Chips
„Antwort vorschlagen“ und „Konversation zusammenfassen“; sie starten die Funktionen des KI-Panels
(Vorschlag als bearbeitbare Karte, Entwurf erst über „Entwurf öffnen“). Ist das Panel eingeklappt,
schwebt unten rechts „Frag etwas …“ mit Vorschlägen beim Fokus. Die Leiste sendet nichts selbst:
Enter klappt das Panel auf, das die Frage über seinen eigenen Ablauf stellt (Zustimmung vor der
ersten Anfrage an einen Cloud-Anbieter, Anzeige der Übertragung). Ohne KI oder ohne Freigabe des
Kontos gibt es weder Chips noch Leiste.

**Verfassen.** Felder mit Haarlinien und gedämpften Beschriftungen, Empfänger als runde Chips,
Rich Text/Text als kleiner Segmentschalter, Editor mindestens einige Absätze hoch (in niedrigen
Fenstern scrollt der Bereich als Ganzes), Senden als Akzentbutton mit leisem Kürzel daneben.

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
