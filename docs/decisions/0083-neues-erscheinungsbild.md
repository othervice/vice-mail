# 0083 — Neues Erscheinungsbild: Inter, Farben und Maße aus dem freigegebenen Entwurf

- **Datum:** 26.11.2026
- **Anlass:** Stufe 23, Punkt 2. Gefragt war, wie die App professioneller, hochwertiger und
  „weniger nach KI“ wirkt. Der Mensch zeigte Mailbird („super polished, sowas in der Richtung“)
  und Canary Mail („kannst du da einen guten Mix aus beidem machen?“). Der Entwurf
  https://claude.ai/artifact/BuSPYpDvmSDm4s1JJoSZFX (Posteingang hell mit Kontaktspalte, neue Mail
  als Karte, dunkel) wurde am 26.11.2026 freigegeben: „ja das dunkle design finde ich super. das
  nehmen wir.“ Dunkel wie im Entwurf, hell mit demselben Aufbau; die Quelle des Entwurfs
  (`Main.dc.html`) ist die Vorlage für Maße und Farben (Stufendatei, Punkt 2).
- **Status:** entschieden vom Menschen (Freigabe des Entwurfs). Schritt 1 („Grundlage“: Schrift,
  Farben, Größen, Formen) umgesetzt von der ausführenden Sitzung; Schritte 2 bis 4 (Seitenleiste;
  Werkzeugleiste, Liste, Lesebereich, Kontaktspalte; neue Mail als Karte) folgen.
  **Ersetzt** die Systemschrift als Schrift der Oberfläche, die Farbwerte von 0053 (Flächen,
  Linien, Text, Blau; der Charakter „kühl, nie warm“ bleibt), die drei tonalen Stufen von 0031
  (Leiste am dunkelsten, Listenspalte getönt, Lesebereich am hellsten), die Schatten von 0031 für
  Karten, Zeilen und Knöpfe und die Rundungen 6/8/16.
- **Code:** `app/src/assets/fonts/InterVariable.woff2` und `LICENSE.txt`, `app/src/styles/app.css`
  (`@font-face`, Theme: `font-mail`, `text-11`, `text-26`, `font-bold`, `tracking-title`,
  `rounded-tile`, die neuen Farben; `.list-card`), `app/src/styles/tokens.css`,
  `app/src/features/composer/composer.css` und `Composer.tsx` (Schreibfläche, Zitat, Signatur in
  `font-mail`), `features/messages/MessageRow.tsx`, `MessageList.tsx`, `views/TableHeader.tsx`,
  `comfort/InboxListRows.tsx` (Schriftgrößen, Liste ohne Rand), `reading/MessageHeader.tsx`
  (Betreff), `components/ViewHeader.tsx`, `Button.tsx`, `ConfirmDialog.tsx`,
  `features/layout/sidebarStyles.ts`, `app/IconRail.tsx`, `ai/AiPanel.tsx`,
  `extras/UnsubscribeBar.tsx`, `extras/print.css`, `settings/OpenSourceLicenses.tsx` (Farben),
  Rundung `tile` in `IconRail.tsx`, `SettingsSearch.tsx`, `EditViewsDialog.tsx`,
  `CommandPalette.tsx`, `ContactsView.tsx`, `GroupsPane.tsx`, `Promises.tsx`, `FirstStart.tsx`;
  `scripts/licenses.mjs` (Eintrag `Inter`), `crates/mail-content/src/colors.rs` (dunkle
  Mail-Fläche, Schrift, Links), `html.rs` (`MAIL_DEFAULT_STYLE` an `--ui-font-family-mail`
  gebunden).
- **Tests:** Vitest `font.test.ts` (Datei = Release per SHA-256, Lizenz daneben, ein `@font-face`
  aus der eigenen Datei, keine Adresse im Netz, CSP `font-src` = `'self'`, Reihenfolge der
  Schriftliste, Mail-Schrift ohne Inter, Composer in der Mail-Schrift, Zeichenvarianten nur für die
  Oberfläche), `contrast.test.ts` (neue Flächen in allen Paaren, Werte des Entwurfs, Reihenfolge der
  Flächen, Dateitypen, Mica), `licenses.test.tsx` (Inter mit OFL in jeder Ausgabe),
  `spacing-scale.test.ts` (11 und 26 gehören zur Skala, `rounded-tile`), `message-list.test.tsx`,
  `no-avatars.test.tsx` (Gewichte), `extras-tools.test.tsx` (Druck mit den neuen hellen Werten);
  Rust `html::tests::a_mail_without_a_font_gets_the_system_font_and_its_own_font_wins`,
  `colors::tests::the_constants_are_the_dark_theme_tokens`; Desktop `frame.e2e.mjs`,
  `dark-mails.e2e.mjs`, `spacing.e2e.mjs`.

## Schrift: Inter, mitgeliefert

- **Datei:** `InterVariable.woff2` aus dem Release 4.1 (16.11.2024),
  https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip (SHA-256 des Archivs
  `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`), darin `web/InterVariable.woff2`
  unverändert: 352.240 Bytes, SHA-256
  `693b77d4f32ee9b8bfc995589b5fad5e99adf2832738661f5402f9978429a8e3` (geprüft von
  `app/tests/font.test.ts`). Eine Datei für alle Gewichte (100 bis 900) mit optischen Größen;
  Lateinisch mit Deutsch und den erweiterten europäischen Zeichen, dazu Griechisch und Kyrillisch.
  Nur aufrecht: Die Oberfläche setzt nichts kursiv (Kursives gibt es nur im Inhalt von Mails, der
  bei der Systemschrift bleibt).
- **Nicht verkleinert.** Eine Teilmenge (etwa 150 kB) wäre eine veränderte Fassung, die sich nicht
  mehr gegen das Release prüfen lässt; 352 kB sind in einer Desktop-App ohne Belang.
- **Lizenz:** SIL Open Font License 1.1, ohne reservierten Namen. `LICENSE.txt` des Releases liegt
  neben der Datei; `scripts/licenses.mjs` führt „Inter 4.1“ als Eintrag der Art `font` (in jeder
  Ausgabe) in `THIRD-PARTY-NOTICES.txt` und unter Einstellungen → Über → Lizenzen.
- **Pflege:** Das letzte Release ist vom November 2024. Die Zwölf-Monats-Regel für neue
  Abhängigkeiten zielt auf Fehlerbehebungen in Code; eine Schrift ist Daten, wie Selawik (0080).
  Der Mensch hat Inter ausdrücklich gewählt (Stufendatei: „Schrift Inter (mitgeliefert, OFL, kein
  Laden aus dem Netz)“).
- **Kein Netz:** `@font-face` in `app.css` mit der Datei der App; Vite legt sie gehasht in `dist/`,
  die App liefert sie selbst aus. Die CSP erlaubt Schriften nur von `'self'` (`default-src`, ein
  eigenes `font-src` gibt es nicht). `font-display: block`: Die Datei ist lokal, der erste Frame
  wartet die Millisekunden ab, statt kurz in der Systemschrift zu erscheinen.
- **Schriftliste:** `"InterVariable"`, dann die Systemschrift wie bisher (SF Pro, Segoe UI
  Variable, Segoe UI). Zeichenvarianten des Entwurfs: einstöckiges a (`cv11`) und offene Ziffern
  (`ss01`), nur für die Oberfläche (`font-mail` und `font-mono` setzen sie zurück). Gleich breite
  Ziffern (`tabular-nums`) für Zeiten, Daten und Zähler in der Liste, in der Seitenleiste und auf
  Plaketten (waren schon gesetzt, geprüft).
- **Inhalt von Mails bleibt in der Systemschrift** (`--ui-font-family-mail`): HTML-Mails ohne
  eigene Schrift (`MAIL_DEFAULT_STYLE`, per Test an das Token gebunden; die CSP des Rahmens lädt
  ohnehin keine Schriften), die Schreibfläche des Composers (Rich Text und reiner Text), das Zitat
  und die Signatur darunter: Was geschrieben wird, sieht aus, wie es gesendet wird
  (`compose/html.rs` setzt die Systemschrift). Reine Textmails im Lesebereich folgen der
  Oberfläche, wie im Entwurf (14 px, Zeilenhöhe 1,65).

## Farben

Die Werte des Entwurfs, auf die vorhandenen Tokens gelegt, wo sie dasselbe bedeuten; neue Tokens,
wo es keines gab.

| Token | Hell | Dunkel | im Entwurf |
|---|---|---|---|
| `surface`, `surface-list` | `#ffffff` | `#14181e` | Fläche (Liste und Lesebereich sind eine Fläche) |
| `canvas` (Rahmen, Mulden in Karten) | `#f6f7f9` | `#14181e` | – |
| `surface-raised` (Karten, Menüs) | `#ffffff` | `#1b2129` | Karten |
| `rail` (Seitenleiste) | `#f2f4f7` | `#1a1f27` | Seitenleiste |
| `rail-hover` | `#e9edf2` | `#222833` | – |
| `rail-selected` (neu) | `#e2e8f1` | `#2a3341` | Auswahl in der Seitenleiste |
| `toolbar` (neu) | `#f7f8fa` | `#1a1f27` | Werkzeugleiste |
| `band` (neu) | `#f6f7f9` | `#1b2129` | Bänder „Heute/Gestern“ |
| `panel` (neu) | `#f8f9fb` | `#171c23` | Kontaktspalte |
| `button` (neu) | `#ffffff` | `#232a34` | Knöpfe |
| `border` | `#dde1e7` | `#333b48` | Rand der Knöpfe |
| `border-subtle` | `#e5e8ec` | `#29303b` | Linien |
| `badge`, `track` (hell) | `#eef1f5` | `#252c37` | Chips |
| Text primär / sekundär / tertiär | `#131a24` / `#455061` / `#5b6676` | `#e8edf3` / `#bcc5d0` / `#98a3b0` | Text |
| `accent` | `#1360d3` | `#6aa8ff` | Akzent als Text |
| `accent-fill` | `#1463d8` | `#2563eb` | Auswahl und „Senden“ |
| `selected` | `#e3ecfa` | `#172747` | (Schritt 3: gefüllt) |
| `accent-soft` | `#e8f0fc` | `#17243f` | – |

- **Akzent hell:** als Text `#1360d3` statt `#1463d8` des Entwurfs, kaum zu unterscheiden: Auf dem
  dunkelsten hellen Mica (`#e6e6e6`, 0079) erreicht `#1463d8` nur 4,42 : 1, `#1360d3` 4,62 : 1.
  Die Füllung bleibt `#1463d8`.
- **Auswahl in der Liste:** Der Entwurf füllt die gewählte Zeile kräftig mit weißer Schrift. Das
  verlangt, dass die Zeile ihre Textfarben umschaltet, und gehört zu Schritt 3. Bis dahin bleibt
  `selected` eine Tönung, beim Standard-Blau aber deutlich kräftiger als vorher (dunkel 20 % der
  Füllung in der Fläche, hell 12 %), damit sie dem Entwurf nahekommt; alle drei Textstufen,
  Akzent, Warnung und Kontofarben bleiben darauf lesbar.
- **Spur der Segmentschalter dunkel** `#0f1217`, unter der Fläche: Der gewählte Knopf
  (`surface-raised`) hebt sich ab, wie bisher.
- **Andere Akzentthemen** (Stufe 16/18) behalten ihren Farbton und ihre Werte auf den neuen
  Flächen; alle Paare sind geprüft. Der Akzent ist Text und Symbol, `rail-selected` bleibt
  neutral, das Symbol des gewählten Eintrags trägt den Akzent.
- **Reihenfolge der Flächen** (ersetzt 0031): Liste und Lesebereich sind eine Fläche; Seitenleiste,
  Werkzeugleiste, Bänder und Kontaktspalte liegen eine Stufe daneben, hell dunkler, dunkel heller;
  die Fläche ist das Äußerste. Kühl wie bisher (Blau nie unter Rot, geprüft).
- **Dateitypen** (neu, für die Anhangskarten aus Schritt 3): Tönung und Plakette je Typ, PDF
  `#fdeeee`/`#3a2326` mit `#dc2626`, Tabellen `#e8f5ec`/`#1c3326` mit `#15803d` (aus dem
  Entwurf), dazu Dokumente (`#2563eb`), Bilder (`#7c3aed`) und Archive (`#b45309`) in derselben
  Art; Schrift auf den Plaketten weiß (≥ 4,8 : 1).
- **HTML-Mails im dunklen Design** (0066) liegen weiter genau auf der Karte: `DARK_CANVAS`
  `#1b2129`, Schrift `#e8edf3`, Links `#6aa8ff` (Test in `colors.rs` bindet sie an die Tokens).
  Gedruckt wird mit den neuen hellen Werten.
- **Angewandt**, wo es nur Farbe ist: Kopf jeder Ansicht auf `toolbar` (die Leiste über Liste und
  Lesebereich wirkt so schon wie die durchgehende Werkzeugleiste), aktiver Eintrag von Spalte und
  schmaler Leiste auf `rail-selected`, Zweitknöpfe auf `button`, KI-Panel auf `panel`,
  Monatsabschnitte der Liste als Bänder (`band`, Haarlinie, 11 px halbfett).
- **Kontrast** (`app/tests/contrast.test.ts`, 4.500 Paare in 12 Themen und auf Mica): kleinste
  Werte für Text tertiär hell 4,66 : 1 (auf Mica), dunkel 4,98 : 1 (auf `rail-selected`), Akzent
  hell 4,62 : 1 (Mica), dunkel 5,79 : 1 (auf Chips); Weiß auf der Füllung 5,52 / 5,17 : 1.

## Größen und Formen

- **Schrift:** Zeilen der Liste 13 px (Absender ungelesen fett, gelesen halbfett; Betreff
  ungelesen halbfett), Vorschau 12 px, Zeit und Pille „n Antworten“ 11 px, Bänder 11 px halbfett,
  Betreff im Lesebereich 26 px fett mit −0,01 em, Mailtext 14 px mit Zeilenhöhe 1,65. Neue Stufen
  11 und 26 und das Gewicht 700; alles andere wie bisher.
- **Rundungen:** Bedienelemente und Einträge der Seitenleiste 7 px; Zeilen der Liste keine, die
  Auswahl geht über die ganze Breite der Liste (kein Rand mehr, Vorlauf der Zeile 24 px wie der
  Titel; Karten aus Zeilen, Priorität und aufgeklapptes Bündel, behalten 8 px Abstand und 16 px
  Vorlauf); Kacheln, Einträge der schmalen Leiste und schwebender Listen 8 px (`tile`, neu);
  Karten 10 px; schwebende Karten (Dialoge, Befehlspalette, später der Composer) 14 px.
- **Schatten** nur für Schwebendes (Menüs, Tooltips, Schnellaktionen, Dialoge, Panels über dem
  Inhalt). Karten, Knöpfe und Zeilen liegen flach mit Haarlinie; `shadow-card` und `shadow-row`
  zeichnen nichts mehr.

## Was die Schritte 2 bis 4 noch umbauen

Beobachtet beim Vergleich der Bildschirmfotos mit dem Entwurf, bewusst nicht in Schritt 1:

- **Werkzeugleiste über die ganze Breite** (laut Stufendatei Teil der Grundlage, vom Auftrag dieser
  Sitzung Schritt 3 zugeordnet): heute Kopf mit Titel und Symbolen, darunter getrennt Schnellfilter
  und Symbolleiste des Lesebereichs; im Entwurf eine Leiste von 52 px mit Ampeln, gruppierten
  Knöpfen (je Gruppe ein Rahmen `border` auf `button`) und Suchfeld, macOS in der Titelleiste,
  Windows unter Mica. Titel dort 13 px halbfett mit „18 ungelesen“ daneben statt 16 px mit
  Unterzeile.
- **Seitenleiste** (Schritt 2): Abschnitte „Alle Konten“, „Favoriten“, je Konto mit Quadrat in
  der Kontofarbe und Symbolen in der Kontofarbe; Einträge ohne Balken (heute bleibt der Balken als
  Zeichen neben der Farbe), Fuß „Abgeglichen um …“ mit Abgleichen und Einstellungen.
- **Zeilen** (Schritt 3): Trennlinie unter jeder Zeile, Kontostreifen 3 px am rechten Rand, gefüllte
  Auswahl mit weißer Schrift, Zeilenhöhe etwa 72 px dreizeilig (heute 76 px).
- **Lesebereich** (Schritt 3): keine Karte um die Mail (`MessageCard`), Kopfzeile „Konto ·
  Ordner · n Mails“ über dem Betreff, ältere Mails eines Gesprächs einzeilig, Anhänge als Karten
  je Dateityp (Tokens liegen bereit), Schnellantwort als Karte unten (`card`, Rundung 10).
  Fällt die Karte weg, muss `DARK_CANVAS` (0066) auf `--ui-surface` wechseln.
- **Kontaktspalte** (Schritt 3) auf `panel`, Knöpfe auf `button`.
- **Neue Mail** (Schritt 4): schwebende Karte (Rundung 14, Schatten nur dort), Betreff als Titel
  19 px fett, Chips für An und Von auf `badge`, Formatleiste als Spur auf `badge`, „Senden ▾“.
