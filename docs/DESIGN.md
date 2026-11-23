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

**Icon-Leiste** (56 px). Von oben: vereinter Posteingang, Später, Aufgaben, Zum Lesen und
Angeheftete Anhänge (Stufe 16, 0033 und 0036), Angepinnt, Gesendet,
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
| Akzent | `accent` (Text, Symbole, Schalter), `accent-fill` (Füllung des Hauptknopfs und kleiner Marken, dazu `text-on-accent`), `accent-soft`, `danger` (mit `text-on-danger`), `success`, `warning` |
| Bedienelemente | `track` (Spur von Segmentschaltern und Schaltern), `brand-ink` („Vice“ in der Wortmarke) |
| Konten | acht unterscheidbare Kontofarben, in beiden Modi geprüft |
| Avatare | acht Tönungen `avatar-n-bg` / `avatar-n-text`, Wahl per Hash der Absenderadresse, Schrift auf Fläche ≥ 4,5:1 |
| Schrift | Systemschrift (SF Pro, Segoe UI Variable); Größen 12, 13, 14, 16, 20, 28 (9 nur für das Initial im Mini-Avatar); Gewichte 400, 500, 600 |
| Abstand | Raster aus 4 px: 4, 8, 12, 16, 20, 24, 32, 40, 48 |
| Rundung | 6 (Bedienelemente), 8 (Zeilen, Kacheln), 10 (Karten), 16 (Panels, Fenster in der App); Pillen rund |
| Schatten | vier Stufen: Zeile (Haarbreite), Karte (Priorität, leere Zustände), schwebend (Menüs, Schnellaktionen), Panel |
| Bewegung | 120, 180, 240 ms, weich auslaufend; bei „Bewegung reduzieren“ keine |

Startwerte: neutrale, leicht warme Grautöne; ein einzelner Akzent, seit Entscheidung 0038 das Orange
aus dem Logo (Blaugrün und die anderen Farbthemen bleiben wählbar); die Prioritätsfläche in warmem Sand. Der Akzent ist ein Token und in einer Zeile änderbar. Der dunkle Modus ist kein
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

## Gestaltungsdurchgang 23.11.2026 (Orange, alle Ansichten)

Ziel: Jede Ansicht wirkt so fertig wie Liste und Lesebereich nach dem Feinschliff, im neuen
Standard-Orange (Entscheidung 0038), hell und dunkel. Geprüft mit Bildschirmfotos bei 1440 × 900
und 900 × 600 (Harness im Scratchpad, nicht im Repo).

**Orange als Akzent.** Das Thema „orange“ hat jetzt zwei Stufen. `accent-fill` ist reines
Markenorange `#ff7a00` in beiden Modi; darauf steht Schrift im Fast-Schwarz des Logos
(`text-on-accent` `#1a1208`, 7:1). So sieht der Hauptknopf aus wie das App-Symbol: Orange auf Nacht.
`accent` ist für Text, Links, Symbole, Schalter und Ränder: hell ein gebranntes Orange `#ad4700`
(≥ 4,5:1 auf jeder Fläche, auch auf der Leiste), dunkel ein helles `#ff8f33`. Weiche Flächen:
`accent-soft` `#fce8d5` / `#3a2716`, ausgewählte Zeile `selected` `#fbeadb` / `#33261a`, Fokusring
`#cc5a00` / `#ff9a40` (≥ 3:1 auf allen Flächen). Reines Orange erscheint nur auf Hauptknopf,
Auswahlkästchen, Senden-Knopf der KI-Leiste, Farbmuster und dem Balken der aktiven Ansicht; nie
großflächig. Die anderen Themen haben `accent-fill` gleich `accent`. Der Danger-Knopf hat eine
eigene Schriftfarbe `text-on-danger` (hell weiß, dunkel fast schwarz), weil `text-on-accent` jetzt
vom Thema abhängt. `app/tests/contrast.test.ts` prüft alle neuen Paare in jedem Thema.

**Bedienelemente.**

| Element | Gestaltung |
|---|---|
| Hauptknopf | `accent-fill`, Schrift `text-on-accent`, Schatten `shadow-row`, 32 px hoch |
| Zweitknopf | erhabene Fläche (`surface-raised`), Haarlinie `border`, `shadow-row` |
| Leiser Knopf | Text im Akzent, Fläche nur beim Überfahren |
| Gefahr | `danger` mit `text-on-danger` |
| Segmentschalter | eingelassene Spur (`track`) ohne Rand; die gewählte Option ist ein erhabener Knopf mit Haarlinie im Akzent (≥ 3:1 zur Spur), Beschriftung in Primärfarbe. Ersetzt die volle Akzentfläche, die in jedem Einstellungsabschnitt zu laut war |
| Schalter | 40 × 24 px, an: `accent`, aus: Spur mit Rand `border-strong`, Knopf mit Haarschatten |
| Felder, Auswahllisten | 32 px, `surface-raised`, Rand `border-strong` (3:1), Fehler in `danger` |
| Tastenkappen | `badge`-Fläche statt Rahmen (Befehlspalette, Tastenübersicht) |
| Status-Pillen | runde Pillen auf `badge` (Ausgang, Aktivität), „wird gesendet“ auf `accent-soft` |

**Flächen.** Datensätze (Aktivität, KI-Anbieter, MCP-Clients, Sync-Fragen, Identitäten,
Kontogruppen, Signaturen, Vorlagen, Regeln, Serverangaben, Kontenliste) sind Karten wie im
Lesebereich: `surface-raised`, Haarlinie `border-subtle`, `shadow-card`, Rundung 10. Hinweise
(`Notice`, Erklärkästen) bleiben eingelassen auf `canvas` mit `border-subtle`. Die Kontenliste ist
eine Karte mit Haarlinien zwischen den Zeilen.

**Einstellungen.** Die Abschnittsliste liegt auf der Tönung der Listenspalte (`surface-list`, Breite
14 rem), jeder Eintrag mit kleinem Symbol (dekorativ, der Name steht daneben); aktiv: `selected`,
Balken im Akzent, Symbol im Akzent. Der Abschnittstitel ist 20 px halbfett, der Inhalt hat 40 px
seitlichen Abstand. Die Legende der Serverangaben steht in der Karte statt auf ihrem Rand.

**Befehlspalette und Tastenübersicht.** 12 % der Fensterhöhe vom oberen Rand (`palette-top`), Schatten
`shadow-panel`, Zeilen 40 px mit Rundung 8; die Übersicht zweispaltig, sobald Platz für zwei Spalten
mit je 18 rem ist.

**Marke.** `components/Brand.tsx` enthält App-Symbol und Wortmarke aus `docs/brand/` als
Inline-SVG (nur Attribute, kein `<style>`, kein Netz; die IDs der Verläufe bekommen je Instanz ein
Präfix). „Vice“ der Wortmarke nimmt `brand-ink` (hell Ink `#1a1619`, dunkel Weiß), alles Orange
bleibt unverändert. Der Name kommt immer aus `config/product.ts`.

- **Erster Start** (noch kein Konto): Wortmarke (48 px hoch), „Noch kein Konto“, Hauptknopf
  „Konto hinzufügen“, darunter eine Karte mit drei Zusagen, die ab dem ersten Konto stimmen (zuerst
  nur lesend, kein Cloud-Dienst dazwischen, Passwort im Schlüsselbund).
- **Über**: Karte mit App-Symbol (72 px) und dem Schriftzug der Wortmarke (das Zeichen ist
  weggeschnitten, damit das V nicht zweimal neben dem Symbol steht; die Umrisse sind dieselben),
  darunter die Kurzbeschreibung; dann Version, Commit, Build-Datum, Plattform, Lizenz und die
  Adresse des Quellcodes (`PROJECT_URL`) mit „Adresse kopieren“. Die App öffnet keine Webseiten
  selbst, deshalb ist die Adresse kein Link. Der Bereich „Updates“ (Stufe 15) folgt unverändert.

**Überlagerungen.** Befehlspalette, Tastenübersicht und Bestätigungsdialoge liegen über einem
abgedunkelten Hintergrund (`scrim`: hell 24 % warmes Schwarz, dunkel 50 % Schwarz). Er erscheint
ohne Bewegung, also auch bei „Bewegung reduzieren“ gleich; ein Klick darauf schließt wie bisher ein
Klick außerhalb. Das Feld der Palette ist eine Zeile mit Lupe auf der Spur (`track`); der Fokusring
liegt auf dieser Zeile, solange das Feld den Fokus hat, statt als Rahmen am abgeschnittenen Rand
des Panels.

**Schreibbereich.** Der Editor des Verfassens (Rich Text und Text) ist 8 px von den Rändern
eingerückt, gerundet wie eine Karte und reicht bis zur Anhangsleiste (mindestens 12 rem). Der
Fokusring (2 px, `focus-ring`, ≥ 3:1) umrahmt damit die ganze Schreibfläche wie ein Textfeld statt
als eckiger Kasten an den Rändern des Fensters; ein Klick irgendwo in die Fläche erreicht den Text.

**Leiste in niedrigen Fenstern.** Die obere Gruppe der Leiste scrollt (nie seitwärts); ihre Ränder
blenden innerhalb des 12-px-Abstands aus, sodass ein angeschnittener Eintrag als „mehr“ lesbar ist.
Die schmale Leiste zeigt keinen Scrollbalken (unter Windows würde er die Symbole aus der Mitte
schieben); Mausrad, Trackpad und Tab erreichen jeden Eintrag, und die aktive Ansicht wird in den
sichtbaren Bereich gerückt, auch wenn ein Kürzel sie öffnet. Die aufgeklappte Seitenleiste behält
einen schmalen Balken. Einträge in ein „Mehr“-Menü zu falten hätte eine Messung der Höhe gebraucht,
die es in den Tests ohne Layout nicht gibt.

**Rückfragen an Ort und Stelle.** `InlineConfirm` (KI-Zustimmung, MCP, Einstellungs-Sync) und das
Entfernen eines Kontos sind erhabene Karten mit einem Balken in der Tonfarbe am Anfang und dem
Symbol am Titel (Gefahr: Warndreieck, Akzent: Info) statt eines farbigen Rahmens um den ganzen Text.

**Weitere Abschnitte einer Seite.** Auf der Seite „Posteingang“ sind „Ungelesene oben“, „Deine
Einordnungen“, „Blockierte Absender“ und „Lokale Regeln“ Unterabschnitte: 16 px mit Haarlinie
darüber (`SettingsSection sub`), nicht 20 px wie der Seitentitel.

**Auswahllisten.** Native `<select>` bleiben: Ein gestaltetes Auswahlfeld mit eigenem Popup passt
weder in `ComboBox` (Filterfeld mit Liste) noch in `ChoiceGroup` (Segmente) und wäre ein neues
Bedienelement mit eigener Tastaturbedienung. Sie haben einheitlich 32 px, `surface-raised` und den
Rand `border-strong`; das Aufklappmenü zeichnet das System.

**Korrigiert.** Klassen, die es im Token-Theme nicht gibt und die deshalb nichts bewirkten
(`top-16`, `min-h-9`, `h-7`, `h-9`, `w-7`, `gap-1.5`, `py-1.5`, `px-1.5`, `mt-0.5`, `sm:` und `md:`
ohne Breakpoints), sind durch Rasterstufen ersetzt: Die Befehlspalette klebte am oberen Rand, einige
Knöpfe und Reiter hatten keine feste Höhe. Die .eml-Ansicht steht in derselben mittigen Spalte wie
der Lesebereich. Das blaue Löschkreuz der Engine im Suchfeld ist ausgeblendet (die App hat ein
eigenes).

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
