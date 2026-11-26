# 0084 — Neues Erscheinungsbild, Schritt 3: Werkzeugleiste, Liste, Lesebereich, Kontaktspalte

- **Datum:** 26.11.2026
- **Anlass:** Stufe 23, Punkt 2, Schritt 3. Vorlage ist der am 26.11.2026 freigegebene Entwurf
  https://claude.ai/artifact/BuSPYpDvmSDm4s1JJoSZFX („ja das dunkle design finde ich super. das
  nehmen wir.“), besonders seine dunkle Fassung; Farben, Schrift und Größen stehen in 0083.
- **Status:** umgesetzt von der ausführenden Sitzung nach dem freigegebenen Entwurf. Was bewusst
  anders ist, steht unten; der Mensch entscheidet beim Durchsehen. Schritt 2 (Seitenleiste) und
  Schritt 4 (neue Mail als Karte) folgen.
- **Ersetzt:** den Kopf jeder Ansicht über Liste und Lesebereich (`ViewHeader` als eigene Leiste,
  56 px, Entscheidung 0031), die Symbolleiste des Lesebereichs mit feinen Trennlinien (Stufe 4,
  0071), den immer sichtbaren Schnellfilter (0062), die Karte um die Mail und die Antwortzeile
  darunter (0031), den Kontopunkt mit Namen in Zeilen gemischter Listen (Stufe 8, 17, 0072), die
  getönte Auswahl (0083) und `DARK_CANVAS` auf der Karte (0066, 0083).
- **Code:** `app/src/app/AppToolbar.tsx` (neu), `AppShell.tsx`, `SidebarColumn.tsx` (Kopf mit
  Umschalter entfällt), `IconRail.tsx` (Platz für die Ampeln entfällt),
  `app/src/components/ToolbarPortal.tsx`, `ToolbarButton.tsx` (neu), `ViewHeader.tsx`,
  `IconButton.tsx`, `ChoiceMenu.tsx`, `app/src/stores/toolbar.ts`, `contactPanel.ts` (neu),
  `layout.ts`; `features/actions/ActionButtons.tsx`, `OverflowMenu.tsx`;
  `features/mail/MailView.tsx`; `features/views/QuickFilterBar.tsx`, `focus.ts`, `useListView.tsx`,
  `ViewMenu.tsx`; `features/threads/ConversationToggle.tsx`, `ThreadPane.tsx`;
  `features/unified/InboxScopeSwitcher.tsx`, `MixedReading.tsx`, `queries.ts`;
  `features/messages/MessageRow.tsx`, `MessageList.tsx`; `features/layout/useReadingLayout.ts`;
  `features/reading/ReadingPane.tsx`, `MessageHeader.tsx`, `MessageBodyView.tsx`,
  `Attachments.tsx`, `format.ts`; `features/composer/QuickReply.tsx`; `features/ai/AiPanel.tsx`,
  `ReadingAi.tsx`; `features/extras/MessageTools.tsx`; `features/contacts/ContactPanel.tsx` (neu),
  `groups/ContactsTransfer.tsx`; `styles/app.css`, `tokens.css`; `i18n/en.ts`, `de.ts`;
  `app/src-tauri/tauri.conf.json` (`trafficLightPosition`); `crates/mail-model/src/ui.rs`,
  `crates/mail-engine/src/ui_prefs.rs` (`WindowLayout.contact_panel`),
  `crates/mail-content/src/colors.rs`, `html.rs` (`DARK_CANVAS`).
- **Tests:** Vitest `new-look-reading.test.tsx` (neu: Segmente der Leiste, gefüllte und neutrale
  Auswahl, Linien, Anhangskarten je Dateityp, Kontaktspalte mit Daten, Umschalter, Platz, axe),
  `threads.test.tsx` (Gesprächszeilen), `list-views.test.tsx` (Filterknopf), `contrast.test.ts`
  (`selected-muted`), angepasst `actions`, `reading-polish`, `sidebar-keyboard` (Tab-Reihenfolge),
  `sidebar-column`, `account-colours`, `conversations`, `conversation-tree`, `no-avatars`,
  `unified-inbox`, `search`, `dark-mails`, `extras-tools`, `message-list` und weitere; Rust
  `colors::tests` (Kanvas = `--ui-surface`), `html::tests`, `ui::tests`, `ui_prefs::tests`,
  `app/src-tauri/src/tests.rs`; Desktop `frame.e2e.mjs` (Ampeln in der Leiste), `spacing.e2e.mjs`
  (Regeln angepasst, siehe unten), `conversations.e2e.mjs`, `context-menu.e2e.mjs`,
  `stage22-ui.e2e.mjs` (Kontostreifen), `list-views.e2e.mjs`, `search.e2e.mjs`, `actions.e2e.mjs`.

## Werkzeugleiste über die ganze Breite

- **Eine Leiste, 52 px** (`--ui-toolbar-height`, auch `--ui-header-height`), auf `toolbar`, unten
  eine Haarlinie. Darunter stehen Seitenleiste (oder schmale Leiste), Liste, Lesebereich,
  Kontaktspalte und KI-Panel nebeneinander.
- **macOS:** Die Leiste ist die Titelleiste. Die Ampeln sitzen an ihrem Anfang, mittig auf 52 px
  (`trafficLightPosition` y 28 statt 30); Ziehen auf freier Fläche bewegt das Fenster, ein
  Doppelklick zoomt (`data-tauri-drag-region="deep"`). Der erste Abschnitt beginnt nach den Ampeln
  (80 px + 12 px).
- **Windows:** unter der nativen Mica-Titelleiste (0079 bleibt); die Leiste ist Rahmen
  (`data-chrome`), Mica scheint durch wie bei der Seitenleiste.
- **Abschnitte wie im Entwurf:** über der Seitenleiste (so breit wie sie, eingeklappt so breit wie
  nötig) Seitenleiste ein/aus und Neue Mail, gerahmt; über der Liste Titel (13 px halbfett), beim
  Posteingang die Wahl der Konten als Menü („Alle Posteingänge ▾“), das Konto nur, wo es mehrere
  gibt, „98 ungelesen“ und die Bedienung der Liste; über dem Lesebereich die Aktionen der offenen
  Mail als verbundene Gruppen, dann Suche, KI-Panel (wenn nutzbar) und Kontaktspalte.
- **Wie die Ansichten hineinkommen:** Die Leiste stellt zwei Plätze bereit; Ansichten zeichnen
  hinein (`ToolbarPortal`, React-Portal). Steuerung, Zustand und Ereignisse bleiben die der Ansicht,
  im Dokument stehen sie in der Leiste. `ViewHeader` zeichnet so Titel und Knöpfe jeder Ansicht
  (Einstellungen, Aktivität, Ausgang, Kontakte, Mail), der Lesebereich seine Aktionen. Ein
  Nachrichtenfenster hat keine Leiste: Dort bleibt die Leiste des Lesebereichs wie bisher.
- **Ausrichtung:** Der Abschnitt über der Liste endet, wo die Liste neben dem Lesebereich endet
  (gemessen, `ResizeObserver`; die Liste meldet sich über `useReadingLayout`). Ohne Liste daneben
  (Einstellungen, Lesebereich unten oder anstelle der Liste) ist er so breit wie sein Inhalt.
- **Schmale Fenster:** Die Suche zeigt unter 1440 px 180 px, unter 1120 px nur ihr Symbol;
  „n ungelesen“ entfällt unter 1120 px, der Knopf „Gespräche“ unter 1600 px (er steht auch im Menü
  „Listenoptionen“). Aktionen, die nicht passen, wandern ins Menü „Weitere Aktionen“ (wie bisher,
  Archivieren und Löschen zuletzt). Was nicht in die Leiste passt, wird abgeschnitten, nie
  überlagert.
- **Tastatur:** Die Leiste kommt in der Tab-Reihenfolge zuerst (Umschalter, Neue Mail, Kopf der
  Liste, die Aktionen als ein Tab-Stopp mit Pfeiltasten, Suche, Umschalter rechts), dann
  Seitenleiste, Liste, Lesebereich, Kontaktspalte. Escape auf einer Aktion führt wie im
  Lesebereich zur Liste zurück. Der Umschalter der Seitenleiste bleibt stehen, wenn sie
  einklappt, und heißt dann „Seitenleiste einblenden“; der Fokus bleibt auf ihm.

## Aktionen als verbundene Gruppen

- Drei Gruppen mit gemeinsamem Rahmen (`border` auf `button`, Rundung 7, Knöpfe 34 × 28, Symbole
  16 px), wie im Entwurf: Antworten, Allen antworten, Weiterleiten (bei Entwürfen „Entwurf
  bearbeiten“) · Archivieren, Löschen (im Papierkorb endgültig), Später, Spam (im Spam „Kein
  Spam“) · Markieren mit „Weitere Aktionen“ verbunden. Alles andere steht im Menü, darunter
  Gelesen/Ungelesen (vorher in der Leiste; Kürzel U bleibt).
- Nur lesende Konten: Aktionen gesperrt mit Grund wie bisher; „Änderungen erlauben …“ steht in der
  Leiste, 28 px hoch wie die Knöpfe daneben.

## Schnellfilter hinter einem Knopf

Der Schnellfilter (0062) ist eingeklappt. Der Knopf mit dem Filtersymbol über der Liste klappt die
Leiste über der Liste auf (ihr Textfeld bekommt den Fokus) und wieder zu; solange ein Filter gesetzt
ist, bleibt sie und der Knopf zeigt es. Cmd/Ctrl+Umschalt+K zeigt sie ebenfalls. Alle Schalter,
Schlagwort, Text, Zähler und „Zurücksetzen“ bleiben wie sie waren. Das Ansichtsmenü (Sortieren,
Tabelle, Spalten) zeigt in der Leiste nur sein Symbol; sein Name sagt die Reihenfolge.

## Zeilen der Liste

- **Linien:** eine Haarlinie (`border-subtle`) unter jeder Zeile; Bänder „Heute / Gestern / Diese
  Woche / Monat“ 28 px (`band`, 11 px halbfett, 16 px Einzug).
- **Maße:** dreizeilige Zeilen neben dem Lesebereich 72 px (vorher 76), Vorlauf 8 px
  (`--ui-row-lead`, vorher 24): Der Punkt für ungelesen steht 16 px vom Rand, der Text beginnt bei
  28 px wie im Entwurf. Erste Zeile Absender (13 px, ungelesen fett, sonst halbfett) und Zeit
  (11 px, `text-tertiary`, gleich breite Ziffern); zweite Zeile Betreff (13 px, ungelesen
  halbfett) mit Zähler, Anhang, Fahne und Markierungen am Ende; dritte Zeile Vorschau (12 px).
- **Zähler:** Die Pille „n Antworten“ ist ein kleiner gerahmter Zähler (Rundung 4, 16 px hoch);
  ihr Wort weicht wie bisher zuerst, der Pfeil klappt das Gespräch auf.
- **Kontostreifen:** In Listen, die Konten mischen (vereinter Posteingang, Ansichten, Suche über
  Konten), ein 3-px-Streifen in der Kontofarbe am rechten Rand der Zeile (6 px Abstand, 8 px oben
  und unten). Er ersetzt Punkt und Namen vor der Zeit. Wo alle Zeilen einem Konto gehören, gibt es
  ihn nicht (er sagte nichts). Der Name des Kontos bleibt im zugänglichen Namen der Zeile („Konto:
  Arbeit“) und ist der Tooltip des Streifens; die offene Mail nennt ihr Konto über dem Betreff, die
  Tabelle hat ihre Spalte „Konto“. So hängt nichts allein an der Farbe.
- **Auswahl:** Hat die Liste den Fokus, ist die gewählte Zeile mit `accent-fill` gefüllt und alles
  in ihr `text-on-accent` (Text in allen drei Stufen, Punkt, Fahne, Zähler, Fokusring), wie native
  Listen und der Entwurf; der Kontostreifen behält seine Farbe, die Schnellaktionen darüber ihre
  Farben. Ohne Fokus (er ist im Lesebereich, in der Leiste, in einem Menü) eine neutrale Tönung
  (`selected-muted`, hell `#e2e8f1`, dunkel `#2a3341`). Der Zustand kommt aus den Fokus-Ereignissen
  der Liste (`data-list-active`), nicht aus `:focus-within`: WebKit lässt das fallen, solange das
  Fenster nicht vorn ist. Das Orange-Thema schreibt auf seiner Füllung in seiner dunklen Schrift.
- **Kompakt und Tabelle** folgen denselben Linien, Farben und der Auswahl.

## Lesebereich ohne Karte

- **Aufbau wie im Entwurf:** Die Mail liegt auf der Fläche des Lesebereichs, höchstens 720 px breit
  (`--ui-reading-column`), 40 px vom Rand (unter 640 px Breite 24 px). Oben eine leise Zeile:
  Quadrat in der Kontofarbe, „Konto · Ordner“, bei Gesprächen „· 3 Mails“; rechts davon leise
  Sichern als .eml, Drucken und Schließen (28 px, Symbole 16 px). Darunter der Betreff (26 px fett).
- **Gespräch:** Die anderen Mails des Gesprächs stehen als einzeilige Zeilen um die offene Mail
  (Absender, Anfang des Textes, Tag), die früheren darüber, die späteren darunter, je mit
  Haarlinie. Ein Klick öffnet diese Mail im Lesebereich (die offene wird zur Zeile), der Fokus geht
  auf sie. Es bleibt bei genau einer offenen Mail (0049); in der Liste klappt das Gespräch wie
  bisher auf.
- **Kopf:** Name (halbfett) und Adresse (`text-tertiary`), Datum rechts, darunter „an dich …“ zum
  Aufklappen der Empfänger, „…“ mit den übrigen Befehlen. Kategorie („Warum hier?“), Labels,
  Hinweise, Lesebestätigungen, Probleme, Fund im Text, Linkprüfung und das Banner für externe
  Inhalte bleiben, wo sie waren. HTML/Text und „Originalfarben“ sind leise Textschalter über dem
  Text (gedrückt auf `badge`).
- **Text:** Textmails 14 px mit 1,65 (0083). Angepasste HTML-Mails im dunklen Design liegen auf
  `--ui-surface` (`#14181e`) statt auf der Karte: `DARK_CANVAS` in `crates/mail-content`, per Test
  an das Token gebunden; Kontrastregeln der Anpassung (0066) unverändert.
- **Anhänge** stehen unter dem Text: „3 Anhänge · Alle speichern“, dann Karten von 224 px je Datei
  auf der Tönung ihres Typs (`file-*-tint`) mit Plakette (30 px, drei Buchstaben der Endung, Farbe
  des Typs, Schrift `text-on-file`; Programme und Unbekanntes neutral mit Symbol), Name 12 px
  halbfett, Typ und Größe 11 px. Vorschau, Anheften, Speichern und Öffnen erscheinen über dem Ende
  der Karte, solange der Zeiger darüber oder der Fokus in ihr ist; mit Tab sind sie immer
  erreichbar. Ein Klick auf die Karte zeigt die Vorschau (wie bisher, wo es eine gibt).
- **Schnellantwort** unten im Lesebereich, außerhalb des Scrollbereichs, als gerahmtes Feld
  (`surface-raised`, Rundung 10) mit Antwortsymbol, 720 px breit wie der Text; beim Schreiben wächst
  es wie bisher (Senden, Erweitern, Zitat). „Frag etwas …“ der KI schwebt darüber. Die leise Zeile
  „Antworten · Allen antworten · Weiterleiten“ unter der Mail entfällt (dieselben Aktionen stehen
  in der Leiste und haben ihre Kürzel).

## Kontaktspalte

- Rechts neben dem Lesebereich, 264 px auf `panel`, mit Haarlinie. Sie zeigt die andere Seite der
  offenen Mail (den Absender, bei eigener Mail den ersten anderen Empfänger): Name (16 px fett),
  Adresse, „Mail schreiben“ (aus dem Konto der Mail) und „Alle Mails“ (die Suche `kontakt:` aus
  Stufe 22 über alle Konten), Notiz, die neuesten Mails (bis zu vier, ein Klick öffnet sie im
  eigenen Fenster) und die Gruppen, in denen eine Adresse des Kontakts steht. Alles aus dem
  Kontaktverzeichnis dieses Rechners: keine Bilder, kein Netz (0072). Wer noch kein Kontakt ist,
  steht mit dem Namen aus der Mail da, ohne Notiz, Mails und Gruppen.
- Ein- und ausblenden mit dem Knopf rechts in der Leiste (`aria-expanded`, `aria-controls`);
  gespeichert mit dem Layout des Fensters auf diesem Gerät (`WindowLayout.contactPanel`, Standard:
  sichtbar; ältere gespeicherte Layouts lesen sich als sichtbar). Nicht synchronisiert.
- Sie erscheint nur in Mailansichten und nur, wo Platz ist: Seitenleiste (oder schmale Leiste),
  Liste mindestens 320 px, Lesebereich mindestens 360 px, die Spalte und, ab 1100 px, das offene
  KI-Panel. Sonst verschwinden Spalte und Knopf; die Wahl bleibt gespeichert.

## Bewusst anders als der Entwurf

- **Suchfeld:** ein Knopf in Feldgestalt, der die vorhandene Suche über der Liste öffnet (Chips,
  Bereiche, Vorschläge, Speichern bleiben dort). Sein Kürzel ist das der Suche (⌘F); der Entwurf
  zeigte ⌘K, das in der App die Befehlspalette öffnet.
- **„an dich“** statt „an mich“: Die App spricht den Menschen mit „du“ an. Das Datum im Kopf bleibt
  vollständig („Mittwoch, 11. November 2026 um 17:08“) statt „Heute, 10:42“.
- **Gelesen/Ungelesen** steht im Menü „Weitere Aktionen“, nicht in einer Gruppe (der Entwurf hat
  dort nur Markieren und „…“).
- **Zähler in Zeilen** behält Pfeil und Wort („› 1 Antwort“): Er klappt das Gespräch auf (0049);
  der Entwurf zeigt nur die Zahl.
- **Auswahl:** Vorschau und Zeit auf der gefüllten Zeile in voller `text-on-accent` statt 86 %
  (4,5 : 1 auf der Füllung in jedem Akzentthema).
- **Liste:** neben dem Filter das Ansichtsmenü, „Listenoptionen“ und (ab 1600 px) „Gespräche“; der
  Entwurf zeigt nur den Filter. Der Titel kürzt mit „…“, wo der Platz nicht reicht.
- **Lesebereich:** Sichern, Drucken und Schließen leise oben rechts (der Entwurf hat keine), die
  Schalter HTML/Text und Originalfarben leise über dem Text.
- **Kontaktspalte:** Name 16 statt 17 px (Schriftskala); die Mails öffnen im eigenen Fenster.

## Abstände (0071) angepasst

`spacing.e2e.mjs` misst weiter jede Ansicht hell und dunkel bei 1280 × 800 und 900 × 600, mit
diesen Änderungen: Die Leiste oben ist 52 px; die Symbolleiste des Lesebereichs als eigene Leiste
gibt es nicht mehr; gleiche Höhen gelten je Zeile einer Leiste (die Auswahlleiste hat 32-px-Knöpfe
über den 28-px-Gruppen); Kanten: Titel in der Leiste und Bänder der Liste auf einer Linie (16 px),
die Markenspalte der Zeilen auf dem Vorlauf (8 px); der Vergleich „Titel der Aktivität mit ihrem
Inhalt“ entfällt (der Titel steht in der Leiste).

## Nicht geprüft

- Windows: die Leiste unter Mica, die Tab-Reihenfolge mit Bildschirmleser (Narrator), Ziehen des
  Fensters an der Leiste (dort zieht die native Titelleiste).
- macOS: Doppelklick auf die Leiste zum Zoomen nur von Hand.
