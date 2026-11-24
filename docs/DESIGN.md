# Oberfläche

> **English note:** the German interface design document, part of the project log. See [README.md](README.md) for how the documentation is organised.

## Richtung

Hell, luftig, eine Zeile pro Mail, der Inhalt steht im Vordergrund. Die Vorlage liegt lokal unter
`design-reference/inbox-reference.jpg`. Sie ist ein Screenshot eines fremden Produkts und dient nur
als Orientierung für den Aufbau. Übernommen wird die Struktur, nicht das Aussehen: eigene Farben,
eigene Icons, eigene Typografie. Die Datei wird nicht eingecheckt und nirgends veröffentlicht.

Was aus der Vorlage übernommen wird:

- schmale Icon-Leiste links statt breiter Ordnerspalte (seit 23.11.2026 ist die Spalte mit Konten und
  Ordnern Standard und die Leiste ihre eingeklappte Form, siehe „Seitenspalte“)
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

**Seitenspalte** (Standard, 240 px, siehe „Seitenspalte“): vereinter Posteingang, Ansichten,
gespeicherte Suchen, Favoriten, dann jedes Konto mit seinen Ordnern, unten Aktivität, Ausgang,
Einstellungen. Eingeklappt bleibt die **Icon-Leiste** (56 px). Von oben: vereinter Posteingang,
Später, Aufgaben, Zum Lesen und Angeheftete Anhänge (Stufe 16, 0033 und 0036), Markiert (die Markierung
des Servers aus allen Ordnern außer Papierkorb und Spam, 24.11.2026), Angepinnt, Gesendet,
Entwürfe, Archiv, Papierkorb, Ordner (klappt die Spalte auf), gespeicherte Ansichten. Unten:
Konten-Umschalter mit Kontofarben, Aktivität, Ausgang, Einstellungen. Jedes Icon hat einen Tooltip
mit Name und Tastenkürzel. Spalte und Leiste stehen nie nebeneinander.

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
| Zeit | rechtsbündig, Tabellenziffern; heute Uhrzeit, dieses Jahr Tag und Monat („9. Okt.“), davor mit Jahr („6. Sept. 2025“) |
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
Nennt eine HTML-Mail keine Schrift, steht sie in der Systemschrift der App statt in Times (siehe
„Nach den README-Bildern“). Reine Textmails folgen dem Thema.

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

- **Ohne Konto** (nach dem geführten ersten Start ohne Konto oder nach „Einrichtung überspringen“,
  siehe „Erster Start“): Wortmarke (48 px hoch), „Noch kein Konto“, Hauptknopf „Konto
  hinzufügen“, darunter eine Karte mit vier Zusagen, die ab dem ersten Konto stimmen (zuerst nur
  lesend, kein Cloud-Dienst dazwischen, keine Telemetrie, Passwort im Schlüsselbund).
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
schieben); Mausrad, Trackpad und die Pfeiltasten erreichen jeden Eintrag, und die aktive Ansicht wird in den
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

## Nach den README-Bildern (23.11.2026)

Die Bildschirmfotos für die README (`docs/images/`, `app/tests/e2e/readme-shots.e2e.mjs`) haben
Mängel gezeigt, die so behoben sind:

- **Schrift in HTML-Mails.** Jedes `srcdoc` bekommt nach der CSP einen Grundstil
  (`mail_content::MAIL_DEFAULT_STYLE`): `font-family` wie `--ui-font-family`, als `:where(html)`
  ohne Spezifität und vor dem Stil der Mail, die ihn so immer überschreibt. Sonst nichts (Größen,
  Farben und Abstände bleiben die der WebView, mit denen Mail-Layouts rechnen).
- **Zeilen ohne Vorschau.** Ältere Mails als die vorab geladenen haben keine Vorschau. In der
  schmalen Liste stehen ihre zwei Zeilen mittig in der festen Höhe von 76 px (die Zeile richtet
  ihren Inhalt mittig aus, das war schon so); kein Platzhalter wie „Keine Vorschau“, der in einer
  langen älteren Liste in jeder Zeile stünde. Die feste Höhe der virtuellen Liste bleibt.
- **Datum in der Zeile.** Vor diesem Jahr mit Jahr und Monatsnamen („6. Sept. 2025“ statt
  „06.09.25“): eine Schreibweise, auch wo keine Monatsüberschrift darübersteht.
- **Suchtreffer** stehen in einer Liste ohne Monatsüberschriften. Sie sind nach Datum geordnet,
  aber über Jahre verteilt, und bekamen sonst oft jeder eine eigene Überschrift; jede Zeile nennt
  ihr Datum.
- **Leiste des Lesebereichs.** Ist kein Platz für das Wort „Nur lesend“, steht nur das Schloss,
  mit Wort und Erklärung als Tooltip (für Screenreader bleibt der Text), statt „Nur l…“. Das Wort
  wird gemessen, auch wenn es nur verborgen ist, damit die Leiste zwischen beiden Zuständen nicht
  springt.
- **KI-Panel.** Wartet ein Zustimmungsschritt (Zusammenfassung, Antwortvorschlag), ist das
  Fragefeld darunter eingeklappt (es bleibt bestehen, eine angefangene Frage geht nicht verloren):
  der Schritt hat die ganze Höhe des Panels. Host und Modell stehen nur in der Zeile „An wen“,
  die Größe der Anfrage ohne sie. Chips, Panel und Befehlspalette benennen dieselbe Aktion gleich:
  „Konversation zusammenfassen“ und „Antwort vorschlagen“ (englisch „Summarise conversation“ und
  „Suggest a reply“). In einer Konversation stehen die Chips unter der geöffneten Mail, auch wenn
  das nicht die Mail der Zeile ist; das Panel arbeitet mit der Konversation der Zeile.
- **Signatur beim Verfassen.** Im Rich Text steht sie direkt unter dem Text, in der Schreibfläche,
  mit ihrer Formatierung (fett, kursiv, Listen, Ausrichtung; Links als Text, ohne Bilder), aus dem
  bereinigten HTML des Kerns als React-Elemente gebaut, nie als rohes HTML. Steht die Antwort über
  dem Original, folgt das zitierte Original darunter in derselben Fläche: Text, Signatur und
  Original scrollen als ein Dokument, statt dass die Schreibfläche zu einem kleinen eigenen
  Scrollbereich schrumpft. Der Text hat dann mindestens 6 rem, die Fläche reicht weiter bis zur
  Anhangsleiste; ein Klick in den leeren Teil setzt den Cursor ans Ende des Texts, und der
  Fokusring umrahmt die ganze Fläche. Reiner Text zeigt Signatur und Original weiter unter dem Feld.
- **KI über MCP.** Pfade im Benutzerordner (Hilfsprogramm, Verbindungspunkt) stehen ab `~/…`, damit
  ein Bildschirmfoto oder eine geteilte Ansicht keinen Benutzernamen zeigt; „Ganzen Pfad kopieren“
  kopiert den vollen Pfad.
- **Befehlspalette.** „Gehe zu Alle Posteingänge“ / „Go to All inboxes“: der Name der Ansicht wie in
  der Seitenspalte, wie bei den anderen „Gehe zu“-Einträgen.

## Seitenspalte: Konten und Ordner (23.11.2026)

Wunsch: eine Spalte links mit den Konten und ihren Ordnern, wie Mailbird, Outlook oder Thunderbird.
Die bisherige ausklappbare Seitenleiste ist dazu umgebaut (`app/src/app/SidebarColumn.tsx`).

**Aufbau von oben.** Kopfzeile so hoch wie der Kopf der Liste (unter macOS liegen dort die
Fensterknöpfe), rechts der Knopf „Seitenleiste ausblenden“. Dann „Alle Posteingänge“ mit der Zahl
der ungelesenen Mails aller nicht ausgeblendeten Konten, darunter die Kontogruppen (nur ab zwei
Konten oder mit Gruppen). Dann „Ansichten“: Später, Aufgaben, Zum Lesen, Angeheftete Anhänge,
Markiert, Angepinnt, Gespeicherte Ansichten. Gesendet, Entwürfe, Archiv und Papierkorb stehen nicht dort, sondern
in jedem Konto: Der Kern kennt vereinte Ansichten nur für den Posteingang (`UnifiedView`). Dann
gespeicherte Suchen, Favoriten, danach je Konto ein Baum. Unten, mit Haarlinie abgesetzt, Aktivität,
Ausgang und Einstellungen mit ihren Zählern. Ohne Konto: Ansichten, der Hinweis „Noch kein Konto“
und die untere Gruppe; keine Zahlen.

**Konto.** Die Zeile des Kontos ist die Wurzel seines Baums: Pfeil, Kontofarbe als Punkt, Name
(halbfett), bei Problemen ein ruhiges Zeichen (offline: Wolke durchgestrichen, gedämpft; Fehler:
Ausrufezeichen in `danger`), rechts die Summe der Ungelesenen in Posteingang und eigenen Ordnern
(nicht Spam, Papierkorb, Gesendet, Entwürfe, nicht Gmails „Alle Nachrichten“ und „Markiert“, die
andere Ordner wiederholen). Name der Zeile und Tooltip: „Name (Adresse), n ungelesen“, bei Problemen
dazu Zustand und Grund („offline: Die Verbindung zum Server wurde unterbrochen.“). Die Farbe steht nie
allein. Ein Klick, Eingabe, Leertaste, ← und → klappen das Konto zu und auf; der Zustand gilt bis
zum Beenden der App.

**Ordner.** Rollen zuerst in fester Reihenfolge Posteingang, Entwürfe, Gesendet, Archiv, Spam,
Papierkorb (dann Markiert und Alle Nachrichten), danach die eigenen Ordner als Baum, je Ebene
16 px eingerückt (als Innenabstand, damit Fläche und Balken über die ganze Zeile gehen). Symbole
16 px, Rollen mit eigenem Symbol. Zähler rechtsbündig mit Tabellenziffern; ein Ordner mit
Ungelesenen hat den Namen in Primärfarbe, der Posteingang zusätzlich einen halbfetten Zähler. Der
„…“-Knopf erscheint beim Überfahren und bei Tastaturfokus über dem Zähler (der so lange verschwindet)
und nimmt dem Namen keine Breite; sein Menü ist 12 rem breit, damit es auch in der schmalsten Spalte
nicht abgeschnitten wird. Ordnerzeilen sind Ziele zum Ablegen gezogener Mails.

**Abschnitte.** „Ansichten“, „Gespeicherte Suchen“ und „Favoriten“ haben ein leises Etikett (12 px,
halbfett, `text-tertiary`), das die erste Zeile ihres Baums ist: Klick, Eingabe, ← und → klappen den
Abschnitt ein. Der Pfeil erscheint beim Überfahren und bei Fokus, eingeklappt immer (wie die
Seitenleisten von macOS). So bleibt die Spalte kompakt, ohne zusätzliche Tab-Stopps. „Alle
Posteingänge“ hat kein sichtbares Etikett (eine verborgene Überschrift für Screenreader).

**Zeilen.** 32 px, Rundung 6, Text 13 px. Ruhend `text-secondary`, beim Überfahren `rail-hover`. Die
gezeigte Zeile: `accent-soft`, Symbol im Akzent, Name in Primärfarbe, Balken in `accent-fill` am
Zeilenanfang (nicht Farbe allein). Fläche der Spalte: `rail`, die dunkelste Stufe, Haarlinie zur
Liste. Abstand zwischen Abschnitten 20 px, Kontobäume 12 px.

**Breite.** Standard 240 px, mit dem Griff am rechten Rand zwischen 200 und 320 px (Doppelklick:
Standard). Der Griff ist ein Fenster-Trenner nach WAI-ARIA (`role="separator"`, Wert in px):
← und → ändern um 16 px, Pos1 und Ende auf Minimum und Maximum, Eingabe klappt die Spalte ein. Die
Breite wird in px bei 100 % Schrift gehalten und wächst mit der Schriftgröße (rem); gesetzt wird sie
per CSSOM an der Spalte, nicht als Stil-Attribut (CSP). Gespeichert wird sie mit dem Layout des
Fensters im Kern (`WindowLayout.sidebarWidth`, nur auf diesem Gerät), sobald sie 400 ms ruht: ein
Ziehen ist eine Speicherung. Die Spalte ist im Kern standardmäßig sichtbar
(`WindowLayout.sidebarExpanded`).

**Ein- und ausklappen.** Knopf in der Kopfzeile, Cmd/Strg+Umschalt+S (Befehl „Seitenleiste ein- oder
ausblenden“ im Aktionsverzeichnis, änderbar, wirkt mit Modifier auch in Textfeldern; im Editor des
Verfassens bleibt es „Durchgestrichen“, der Editor behandelt es zuerst), Eintrag in der
Befehlspalette (Gruppe Layout) und in der Leiste der Eintrag „Ordner“. Gespeichert wird der Zustand
mit dem Layout des Fensters (`sidebarExpanded`). Wird mit der Tastatur umgeschaltet, landet der
Fokus auf dem Gegenstück (Leiste „Ordner“ bzw. „Seitenleiste ausblenden“); nach einem Klick nicht,
damit kein Tooltip aufspringt.

**Schmale Fenster.** Unter 1100 px Fensterbreite mit Lesebereich rechts (880 px mit Lesebereich unten
oder anstelle der Liste; beides mal Schriftgröße/100) klappt die Spalte von selbst zur Leiste, der
gespeicherte Zustand bleibt. „Ordner“ oder das Kürzel öffnen sie dann über dem Inhalt
(`shadow-panel`, die Leiste darunter ist `inert`, kein Griff). Escape schließt sie und gibt den Fokus
an „Ordner“ zurück; ein Klick oder Fokus außerhalb und das Öffnen einer Ansicht schließen sie auch.
Nie eine waagrechte Bildlaufleiste; bei 900 × 600 bleibt alles erreichbar.

**Tastatur.** Jeder Baum ist ein Tab-Stopp (siehe unten); Kopfknopf und Griff sind je einer. Mit
einem Konto kostet die Spalte 5 Stopps (Seitenleiste ausblenden, Griff, Ansichten, Konto mit
Ordnern, untere Gruppe), einen mehr als die Seitenleiste vorher. Mit zwei Konten, Favoriten, Gruppen
und gespeicherten Suchen sind es 8, gleich viele wie vorher: Der Kontoknopf über dem Ordnerbaum ist
jetzt dessen Wurzel.

## Tastatur: Tab-Stopps, Bäume und Leiste

Tab springt von Bereich zu Bereich, die Pfeiltasten bewegen innerhalb eines Bereichs (Roving
Tabindex: genau ein Eintrag je Bereich hat `tabindex="0"`). Vorher war jeder Ordner mit seinem „…“
ein eigener Tab-Stopp; bei aufgeklappter Seitenleiste brauchte die echte App 46 Tastendrücke bis zur
Liste. Mit der Seitenspalte sind es mit einem Konto 11 vom Fensteranfang: Seitenleiste ausblenden,
Griff, Ansichten, Konto mit Ordnern, untere Gruppe, Kopf der Liste (Nur ungelesene, Listenoptionen,
Konversationen, Suche, Neue Mail), Liste. Jeder weitere Baum kostet einen Stopp, jedes weitere Konto einen (sein
Baum mit dem Konto als Wurzel), egal wie viele Ordner es hat.

**Bäume der Seitenspalte** (WAI-ARIA Tree View, Variante Navigation). Posteingänge (vereinter
Posteingang und Kontogruppen), Ansichten, gespeicherte Suchen, Favoriten und jedes Konto mit seinen
Ordnern sind je ein eigener Baum (`role="tree"`), kein gemeinsamer Baum mit Gruppen. Begründung:
Jeder Abschnitt bleibt ein benannter Bereich mit Überschrift, den Screenreader über Bereiche und
Überschriften direkt anspringen; Tab wird zum Sprung „nächster Abschnitt“, statt durch alle Favoriten
und Posteingänge zu den Ordnern des zweiten Kontos zu pfeilen. Seit der Seitenspalte ist das Konto
die Wurzel seines Baums (Ebene 1, seine Ordner ab Ebene 2): Der Kontoknopf hat nur noch eine Aufgabe,
Auf- und Zuklappen, genau die eines Baumknotens. Ebenso ist das Etikett von Ansichten, gespeicherten
Suchen und Favoriten die Wurzel ihres Baums.

| Taste | Wirkung im Baum |
|---|---|
| ↓ / ↑ | nächste / vorige sichtbare Zeile (kein Umlauf); öffnet nichts |
| → | zugeklappten Ordner, Konto oder Abschnitt aufklappen; aufgeklappten: zum ersten Eintrag darunter |
| ← | aufgeklappten Ordner, Konto oder Abschnitt zuklappen; sonst zum übergeordneten (bis zum Konto) |
| Pos1 / Ende | erste / letzte sichtbare Zeile |
| Buchstabe, Ziffer | nächste Zeile, deren Name so beginnt; schnell getippte Zeichen (500 ms) bilden ein Suchwort, findet es nichts, beginnt das Zeichen eine neue Suche; derselbe Buchstabe wiederholt geht weiter; Groß- und Kleinschreibung und Akzente zählen nicht |
| Eingabe, Leertaste | Ordner bzw. Ansicht öffnen (ein Platzhalter ohne Mails, ein Konto und ein Abschnittsetikett klappen auf und zu) |
| Umschalt+F10, Kontextmenütaste | Menü „…“ der Zeile, Fokus auf dem ersten Eintrag; Escape gibt den Fokus an die Zeile zurück; Rechtsklick öffnet dasselbe Menü |
| Alt+↑ / Alt+↓ | nur Favoriten: Eintrag verschieben, der Fokus bleibt auf ihm |
| Tab | verlässt den Baum; Tab hinein landet auf der gezeigten Zeile (`aria-current`), sonst auf der ersten |

Buchstaben sind im Baum Typeahead und keine Kürzel (`c`, `r` usw. wirken dort nicht); Satzzeichen
(`/` Suche, `?` Tastenübersicht) und Kürzel mit Cmd/Strg wirken weiter.

Semantik: Die Zeilen sind flach im DOM (`li role="treeitem"` direkt im Baum), die Hierarchie steht
in `aria-level`, `aria-posinset` und `aria-setsize` (wie FlatTree in Fluent UI); so umrahmt der
Fokusring eine Zeile und nie den aufgeklappten Teilbaum. Ordner mit Unterordnern haben
`aria-expanded`, die gezeigte Zeile `aria-current="page"`. Kein `aria-selected`: Im Navigationsbaum
folgt das Öffnen nicht dem Fokus (ein Ordner lädt erst mit Eingabe, Leertaste oder Klick). Der Name
der Zeile ist der sichtbare Text plus Zustände („Projekte, Farbe Grün, 2 ungelesen“). Der Pfeil vor
dem Namen ist für die Maus (Tooltip „Unterordner … zeigen“); der „…“-Knopf erscheint beim
Überfahren und solange die Zeile den Tastaturfokus hat und steht nicht in der Tab-Reihenfolge.
Gezogene Mails werden auf der Zeile selbst abgelegt.

**Leiste.** Die Ansichten oben und die Gruppe unten (in der schmalen Leiste die Konten, dazu
Aktivität, Ausgang, Einstellungen; in der Seitenspalte nur die drei) sind je eine senkrechte
Symbolleiste (`role="toolbar"`, `aria-orientation="vertical"`, „Mail-Ansichten“ und „Aktivität und
Einstellungen“): ↑/↓ und Pos1/Ende bewegen, Eingabe und Leertaste wirken wie bei jedem Knopf. Tab
kommt auf der aktiven Ansicht an, unten ohne aktive Ansicht auf dem gewählten Konto. Cmd/Strg+1 bis
8 bleiben, Cmd/Strg+Umschalt+S klappt die Seitenspalte ein und aus.

**Geprüft, unverändert.** Die Abschnittsliste der Einstellungen ist schon ein Stopp (Reiter mit
Pfeiltasten), danach kommt der Inhalt des Abschnitts. Im Lesebereich ist die Symbolleiste ein Stopp;
bis zum Text der Mail folgen „Als .eml speichern“, „Drucken“, „Schließen“, Empfänger, „…“ der Karte,
„Warum hier?“ und HTML/Text, zusammen 9 Tastendrücke. Das sind einzelne Bedienelemente eines
Dokuments, keine Liste; Eingabe auf einer Zeile der Liste setzt den Fokus ohnehin in den Lesebereich.

## Konversationsansicht (Entscheidung 0039)

**Standard an.** Listen zeigen eine Zeile je Konversation: Posteingang eines Kontos und vereinter
Posteingang (mit Priorität, Angepinntem, Zurückgekehrtem und Bündeln), normale Ordner, „Nur
ungelesene“, „Ungelesene oben“, „Alle anzeigen“ der Priorität und Suchtreffer. Später, Angepinnt,
Aufgaben und die Lese-Liste bleiben je Mail. Die Einstellung ist eine Oberflächen-Einstellung des
Kerns und wird wie die Schriftgröße zwischen Geräten abgeglichen.

**Zeile.** Steht an der neuesten Mail der Konversation im gezeigten Ordner. Absender-Spalte: bei
mehreren Absendern ihre Namen in der Reihenfolge des Gesprächs („Anna, Ada Lovelace, Bob“), sonst
der Absender. Nach dem Betreff die Anzahl der Mails als Pille (`badge`). Ungelesen (Punkt, halbfett),
Markierung und Büroklammer, sobald eine Mail der Konversation es ist bzw. hat. Der Name der Zeile
enthält den sichtbaren Text in derselben Reihenfolge, dazu „Konversation mit n Mails“. Eine
Konversation steht nur einmal in der Liste: Steht sie im Block „Priorität“, wiederholt sie sich
darunter nicht. Suchtreffer: der neueste Treffer einer Konversation ist ihre Zeile.

**Öffnen.** Die Zeile öffnet die Konversation im Lesebereich (alle Mails, ältere eingeklappt, die
erste ungelesene offen), auch im vereinten Posteingang und in Suchtreffern. Die offene Mail bleibt
offen, wenn die Konversation danach als gelesen gilt.

**Aktionen.** Aktionen an der Zeile (Schnellaktionen, Auswahlleiste, Tastenkürzel, Ziehen auf einen
Ordner, Palette auf der Zeile) gelten der ganzen Konversation; welche Mails, entscheidet der Kern:
Archivieren, Löschen, Verschieben, Spam, Schlagwörter und Zurückstellen nehmen ihre Mails im
gezeigten Ordner; Gelesen, Markierung entfernen, Nicht mehr anpinnen und Nicht mehr zurückstellen
alle ihre Mails; Ungelesen, Markieren, Anpinnen, Aufgabe und Lese-Liste die Mail der Zeile. Die
Symbolleiste im Lesebereich wirkt auf die offene Mail allein. Öffnen markiert (mit „Beim Öffnen als
gelesen markieren“) die ganze Konversation als gelesen, wie „Gelesen“ an der Zeile.

**Umschalten.** Schalter „Konversationen“ im Kopf der Liste (gedrückt = an), Eintrag „Einzelne Mails
zeigen“/„Konversationen zeigen“ im Menü „Listenoptionen“ und in der Befehlspalette (Gruppe
Layout), Einstellungen → Lesen → „Konversationen“. Lehnt der Kern die Änderung ab, gilt der alte
Wert wieder und Lesen nennt den Grund.

## Erster Start (Stufe 15, Entscheidung 0044)

Eine neue Installation (kein Konto, erster Start nie beendet) zeigt statt des Rahmens eine
geführte Einrichtung. Ruhig und kurz: eine Karte, fünf Schritte, jeder überspringbar.

**Aufbau.** Fläche `canvas`; oben ein Band so hoch wie der Kopf der Liste (unter macOS liegen dort
die Fensterknöpfe, das Band verschiebt das Fenster), rechts darin der leise Knopf „Einrichtung
überspringen“ (nicht auf dem letzten Schritt). Darunter mittig eine Karte (`surface-raised`,
Rundung 16, `shadow-panel`, höchstens 40 rem breit und `first-start-height` 44 rem hoch, in
niedrigen Fenstern so hoch wie der Platz): oben die Schrittleiste, in der Mitte der Schritt (scrollt
allein), unten die Fußzeile mit „Zurück“ links und dem Hauptknopf rechts. Die Karte hat auf jedem
Schritt dieselbe Höhe, damit die Schrittleiste nicht springt. Linien in der Karte in `border`
(nicht `border-subtle`, das auf `surface-raised` im dunklen Modus verschwindet).

**Schrittleiste.** Fünf gleich breite Segmente (Willkommen, Konto, Darstellung, Optional, Fertig):
ein 4-px-Balken (erledigt und aktuell `accent-fill`, offen `track`) und der Name darunter (12 px;
aktuell halbfett in Primärfarbe, erledigt sekundär, offen tertiär). Als Liste in einer `nav`
„Schritte der Einrichtung“; der aktuelle Schritt hat `aria-current="step"`, erledigte tragen
„, erledigt“ für Screenreader. Die Leiste ist keine Navigation zum Anklicken: Vor und zurück
geht es über die Fußzeile.

**Schritte.**

| Schritt | Inhalt | Fußzeile |
|---|---|---|
| Willkommen | Wortmarke, „Willkommen“, ein Satz, was die App ist, ein Satz zum Ablauf, die vier Zusagen als Gruppe mit Haarlinien; mittig | „Los geht’s“ |
| Konto | „Dein erstes Konto hinzufügen“ und ein Satz; das Formular der Kontoeinrichtung ohne eigenen Titel, „Zurück zu den Konten“ und „Abbrechen“. Nach dem Hinzufügen ersetzt eine Erfolgsmeldung das Formular („… ist eingerichtet“, liest vorerst nur), der Fokus liegt darauf | Zurück, „Später einrichten“ (Zweitknopf); nach dem Konto „Weiter“ |
| Darstellung | Thema als drei Kacheln mit Symbol (System, Hell, Dunkel) wie die Akzent-Kacheln darunter (`AccentPicker` unverändert); beides gilt sofort | Zurück, Weiter |
| Optional | Eine Gruppe mit drei Zeilen (Symbolkachel, Titel, ein Satz): Einstellungs-Sync mit „Jetzt einrichten“ (ohne Konto nur der Hinweis, dass er eins braucht; eingeschaltet „An“ mit Haken), KI nur als Satz, wo man sie einschaltet (zuerst MCP), Updates als Schalter „Automatisch nach Updates suchen“ (aus, Text wie unter Über; fehlt in Builds ohne Updater). „Jetzt einrichten“ zeigt die Seite des Einstellungs-Sync im Schritt; der erste Knopf der Fußzeile heißt dann „Zurück zur Einrichtung“ | Zurück, Weiter |
| Fertig | Haken in einem Kreis (`accent-soft`), „Alles bereit“, ein Hinweis: das Konto liest nur, Änderungen erlaubt man unter Einstellungen → Konten → Konto → „Änderungen erlauben“ (ohne Konto: wo man eins hinzufügt); „Alles … später in den Einstellungen“; mittig | Zurück, „Posteingang öffnen“ bzw. „… verwenden“ |

**Verhalten.** Der Fokus geht bei jedem Schritt auf dessen Überschrift (`h1`, nicht in der
Tab-Reihenfolge, ohne Ring, weil sie kein Bedienelement ist); der Schritt beginnt oben. Besuchte
Schritte bleiben verborgen erhalten, so behält das Formular bei Zurück und Weiter seine Eingaben.
Escape tut nichts. Die Update-Frage gilt als beantwortet, sobald der Schritt „Optional“ nach vorn
verlassen oder von dort übersprungen wird; danach kommt die Karte nach dem ersten Konto nicht mehr.
Wer vorher überspringt, bekommt sie wie bisher nach dem ersten Konto (0037). Keine Bewegung außer
den Farbübergängen der Bedienelemente; „Bewegung reduzieren“ des Systems schaltet auch die ab
(0030). Kann das Ende nicht gespeichert werden, steht der Fehler über der Fußzeile, und der
Schritt bleibt.

**Geprüft** in Hell und Dunkel bei 1200 × 800 (Standardfenster der Desktop-Tests) und 900 × 600:
nichts scrollt seitwärts, die Karte bleibt im Fenster, Fußzeile und „Einrichtung überspringen“
bleiben sichtbar; bei 900 × 600 scrollen Willkommen, Konto und Optional in der Karte.

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
