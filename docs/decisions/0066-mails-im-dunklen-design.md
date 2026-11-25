# 0066 — Mails im dunklen Design

- **Datum:** 25.11.2026
- **Anlass:** Stufe 21, Rückmeldung 8 zu 0.1.1: „Manche Mails haben einen dunklen, manche einen
  weißen Hintergrund. Warum? Wie wird das einheitlich?“ Geplant: Einstellung „Mails im dunklen
  Modus“ (angepasst als Standard, oder immer hell); angepasst rechnet der Kern die Farben beim
  Aufbereiten um (Helligkeit umgekehrt, Farbton bleibt, Bilder unverändert), im bestehenden
  Sicherheitsweg, ohne Skript; Mails, die selbst `color-scheme` angeben, werden respektiert; pro
  Mail „Original ansehen“.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, Punkt 8)
- **Code:** `crates/mail-content/src/colors.rs` (Farbdurchgang, Abbildung, `prefers-color-scheme`),
  `crates/mail-content/src/html.rs` (`RenderOptions::scheme`, `RenderedHtml::colors`, Standardstil),
  `crates/mail-model/src/reading.rs` (`MailScheme`, `MailColors`), `crates/mail-model/src/ipc.rs`
  (`DarkMails`, `ReadingPreferences::dark_mails`), `crates/mail-engine/src/reading/mod.rs`
  (`render_body`, Einstellung `reading.dark_mails`), `app/src-tauri/src/commands/reading.rs` und
  `eml.rs` (Parameter `scheme`), `app/src/features/reading/MessageBodyView.tsx` (`HtmlBody`,
  `ColorsToggle`, `whenFrameShows`), `ReadingPane.tsx`, `find.ts`,
  `app/src/features/eml/EmlWindow.tsx`, `app/src/features/settings/ReadingSettings.tsx`,
  `app/src/features/preferences/theme.ts` (`useAppliedTheme`)
- **Tests:** `colors::tests` (Syntax, Rollen, Kontrast auf einem Raster aus 568 × 568 Paaren,
  Farbton, Idempotenz, `color-scheme`, `prefers-color-scheme`, Konstanten gegen `tokens.css`),
  `html::tests` (Newsletter dunkel, eigene dunkle Farben, nur hell, Textersatz, nichts Neues
  gegenüber dem Sanitiser), `tests/hostile_corpus.rs` und `tests/fuzz.rs` (jede Mail auch dunkel),
  `reading::tests::html_mail_is_adapted_in_the_dark_theme_unless_the_setting_says_light`,
  Vitest `dark-mails.test.tsx`, Desktop `dark-mails.e2e.mjs`

## Warum es gemischt aussah

Seit Stufe 3 bekommt jede HTML-Mail ihr eigenes weißes Blatt (`--ui-mail-canvas`, Rahmen mit
`color-scheme: light`, `docs/DESIGN.md` „HTML-Mails auf ihrem weißen Blatt“), auch im dunklen
Design: Ihre Farben sind für Weiß gemacht, dunkle Schrift auf dunklem Grund wäre unlesbar. Reiner
Text dagegen steht in den Farben der App. Im dunklen Design lag deshalb jede Textmail dunkel und
jede HTML-Mail, auch eine ganz ohne eigene Farben, als weißes Blatt in der dunklen Karte. Dazu kam
eine Unschärfe: Eine Mail mit `color-scheme: light dark` konnte im weißen Blatt der Darstellung des
Systems folgen (helle Standardschrift auf Weiß), weil der Rahmen sein Schema nicht festlegte.

## Entscheidung

**1. Einstellung.** Einstellungen → Lesen → „Nachrichten im dunklen Design“: „An das dunkle Design
angepasst“ (Standard) oder „Immer auf weißem Grund“ (wie bisher). Schlüssel `reading.dark_mails`
(`"adapted"`/`"light"`), global, mit dem Einstellungs-Sync; Teil der `ReadingPreferences`, ein
älterer Aufrufer ohne das Feld bekommt „angepasst“. Eine Änderung zeigt die offene Mail sofort neu.
Im hellen Design ändert sich nichts.

**2. Wer fragt.** Die Oberfläche fragt `getMessage` (und `loadRemoteContent`, `getEmlMessage`,
`loadEmlRemoteContent`) mit dem Schema der Fläche: `dark` im dunklen Design, sonst und bei
„Originalfarben“ `light`. Ohne Angabe gilt `light` (ältere Aufrufer, Tests, KI, Composer). Der Kern
entscheidet mit der Einstellung und dem, was die Mail selbst sagt, und meldet es zurück:
`MessageBody::Html.colors` ist `original` (wie geschrieben, weißes Blatt), `adapted` (umgerechnet,
dunkle Karte) oder `ownDark` (eigene dunkle Farben, dunkle Karte). Die Oberfläche legt danach den
Grund hinter den Rahmen (`bg-mail-canvas` oder `bg-surface-raised`) und dessen Schema
(`scheme-light`/`scheme-dark`).

**3. Was die Mail selbst sagt.** Gelesen werden `<meta name="color-scheme">`,
`<meta name="supported-color-schemes">` und die Eigenschaft `color-scheme` (im Stylesheet und am
`style` von `<html>`/`<body>`).
- Nennt eine davon `dark`: **eigene dunkle Farben**. Nichts wird umgefärbt; der Rahmen bekommt den
  dunklen Standardgrund und die dunkle Standardschrift; die `@media (prefers-color-scheme: dark)`
  der Mail gelten.
- Sagt eine `only light` (ohne `dark`): **wie geschrieben** auf weißem Grund. Der Absender bittet
  ausdrücklich darum, meist weil Logos mit dunkler Schrift auf Transparenz sonst verschwinden.
- Sonst: **angepasst**.

`prefers-color-scheme` entscheidet der Kern selbst, in jedem Modus: Eine Abfrage, die zum Grund
passt, verliert diese Bedingung (`screen and (prefers-color-scheme: dark)` wird `screen`, allein
`all`), eine, die nicht passen kann, fällt weg; mit `not` oder `or` daneben und für andere Werte
(`no-preference`) gilt sie als nicht passend. Das Schema des Rahmens im Web-View folgt sonst der
Darstellung des Systems, nicht der Fläche, auf der die Mail liegt. Aus demselben Grund nimmt der
Durchgang die `color-scheme`-Deklarationen der Mail heraus; das Schema setzt der Kern (dunkler
Standardstil `MAIL_DARK_STYLE`: `color-scheme:dark`, Grund `#212830`, Schrift `#e6edf3`, Links
`#4493f8`, alles mit `:where()` ohne Spezifität, jede Farbe der Mail gewinnt).

**4. Die Umrechnung.** Läuft im Kern beim Aufbereiten, im Sicherheitsweg nach dem Sanitiser
(Architektur, „Security boundaries“), ohne Skript im Rahmen. Der Durchgang liest das bereinigte CSS
und die bereinigten Attributwerte mit demselben Tokenizer und ersetzt nur Farbwerte (benannte
Farben, `#rgb[a]`, `#rrggbb[aa]`, `rgb()`/`rgba()`, `hsl()`/`hsla()`) durch Farben, die er selbst
schreibt (`#rrggbb`, mit Transparenz `rgba(r, g, b, a)`); alles andere bleibt Byte für Byte.

- **Rollen.** Hintergründe (`background*`, `bgcolor`) und, weil sie auf einem Hintergrund liegen,
  Linien (`border*`, `outline`, `column-rule`, `<hr color>`) und Schatten werden dunkel, wenn sie
  hell sind. Schrift (`color`, `<font color>`, `text`/`link`/`vlink`/`alink` am `<body>`,
  Textdekoration, `-webkit-text-fill/stroke`, `caret-color`) wird hell, wenn sie dunkel ist. Dunkle
  Hintergründe und helle Schrift bleiben.
- **Skala.** Umgekehrt wird die relative Leuchtdichte `Y` nach WCAG auf der Skala
  `v = ln(Y + 0,05)`, auf der ein Kontrastverhältnis ein Abstand ist. Hintergründe mit
  `Y > 0,14` werden unter diesen Punkt gespiegelt und so gestaucht, dass Weiß genau auf der dunklen
  Karte (`#212830`, `--ui-surface-raised` dunkel) landet; Schrift mit `Y < 0,24` wird über diesen
  Punkt gespiegelt, höchstens bis zur Textfarbe der App (`#e6edf3`). Mit diesen Punkten gilt für
  jedes Paar aus einer Schrift- und einer Hintergrundfarbe der Mail: Der Kontrast danach ist
  mindestens der kleinere Wert aus dem Kontrast vorher und 4,5 : 1. Was lesbar war, bleibt lesbar
  (WCAG AA), und kein Paar wird darunter schlechter. Der Test prüft das auf einem Raster aus 568
  Farben in beiden Rollen.
- **Farbton.** Farbton und Buntheit bleiben (OKLab); nur wo es die neue Helligkeit in sRGB nicht
  gibt, sinkt die Buntheit. Graue Farben bekommen die Tönung des dunklen Designs, damit eine weiße
  Mail und ihre Karte eine Fläche sind und schwarze Schrift genau die Schrift der App ist. Die
  Farbe wird per Bisektion gesucht, sodass Schrift nie dunkler und ein Hintergrund nie heller wird,
  als die Abbildung sagt; die Zusicherung hält damit auch nach dem Runden auf 8 Bit.
- **Idempotent.** Ein umgerechneter Hintergrund liegt unter, eine umgerechnete Schrift über dem
  Punkt, ab dem umgerechnet wird; ein zweiter Durchgang ändert nichts.
- **Verläufe** (`linear-gradient()` und Verwandte) werden wie Hintergründe umgerechnet, Stopp für
  Stopp. Der Plan sah vor, sie zu lassen; dann stünde umgerechnete helle Schrift auf einem hellen
  Verlauf (eine helle Karte mit Verlauf ist in Newslettern häufig). Ein Verlauf ist CSS, kein Bild.
- **Unverändert:** Bilder, auch `background-image: url()` und `background` mit Bild (der Durchgang
  sieht in `url()` nicht hinein), Systemfarben (`Canvas`, `CanvasText`, `windowtext` folgen dem
  dunklen Schema des Rahmens von selbst), `transparent`, `currentColor`, `inherit`, Farben in
  `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`, `color()`, `color-mix()`.
- **Mails als Text** (zu komplex für den Parser, Selbstprüfung fehlgeschlagen) stehen im dunklen
  Design auf dem dunklen Grund.

**5. „Originalfarben“.** Über der Mail neben „HTML/Text“ ein Umschalter „Originalfarben“
(`aria-pressed`), nur im dunklen Design und nur, wenn der Kern umgerechnet hat oder er gedrückt ist.
Gedrückt fragt die Oberfläche `light`: die Mail wie geschrieben auf weißem Grund. Der Zustand gehört
zur Mail und zu dieser Sitzung, wie das gewählte Format; die nächste Mail ist wieder angepasst.
Nicht pro Absender gemerkt (siehe Verworfen). Das `.eml`-Fenster hat denselben Umschalter.

**6. Drucken.** Papier ist weiß: Drucken einer umgerechneten Mail schaltet sie zuerst auf
„Originalfarben“, wartet, bis der Rahmen die Originalfassung geladen hat, und öffnet dann den
Druckdialog. Die Mail bleibt danach in Originalfarben; ein Klick schaltet zurück.

**7. Suchen in der Mail.** Auf dem dunklen Grund nimmt die Hervorhebung die Suchfarben des Designs
(`--ui-find-match`, `--ui-find-current` mit `--ui-find-current-text`), auf weißem Grund wie bisher
das `mail-*`-Paar.

## Verworfen

- **CSS-Filter auf dem Rahmen** (`filter: invert(1) hue-rotate(180deg)`, wie manche
  Browser-Erweiterungen): kehrt auch Bilder um (sie müssten ein zweites Mal umgekehrt werden),
  verschiebt Farben unvorhersehbar, kennt keinen Kontrast und läge nicht im Kern.
- **`light-dark()` in den geschriebenen Werten** (Original und Umrechnung zugleich, Drucken ohne
  Umschalten): erst ab Safari 17.5; die App unterstützt macOS 11, dessen Web-View es nicht kennt.
  Attribute wie `bgcolor` können keine Funktion tragen.
- **Ein Skript im Rahmen:** ausgeschlossen (Sandbox ohne `allow-scripts`, 0005).
- **Lichtwert `L` von OKLCH einfach umkehren:** erhält den Kontrast nicht. Grau `#767676` auf Weiß
  hat 4,5 : 1, nach `L → 1 − L` auf Schwarz nur noch 2,6 : 1. Deshalb die Leuchtdichte-Skala.
- **Auch dunkle Hintergründe aufhellen** (volle Umkehrung, wie Outlook sie teils macht): Ein dunkles
  Kopfband würde hell; Mails, die im Dunkeln schon gut aussehen, würden verändert.
- **Merken pro Absender:** eine weitere Tabelle mit Sync für einen seltenen Fall; wer nie
  umgerechnet haben will, stellt „Immer auf weißem Grund“ ein. Kann später kommen.
- **Linien in beide Richtungen** (dunkle Linien hell): nicht idempotent; die häufigen hellen
  Trennlinien bleiben mit der Hintergrund-Rolle dezent.
- **Große weiße Bilder abdunkeln:** nicht verlangt; bräuchte eine Analyse der Bilder im Helfer.

## Grenzen

Ausdrücklich, damit niemand mehr erwartet:
- Bilder behalten ihre Farben. Ein Logo mit dunkler Schrift auf transparentem Grund liegt danach
  auf dunklem Grund; Text, den die Mail auf ein Hintergrundbild setzt, wird wie jede Schrift hell.
- Die Zusicherung zum Kontrast gilt für die Farben selbst; halbtransparente Farben behalten ihre
  Deckkraft, ihr Ergebnis hängt vom Grund darunter ab.
- Sehr dunkle Linien (etwa schwarze Tabellenrahmen) bleiben dunkel und sind auf dem dunklen Grund
  schwach.
- `(prefers-color-scheme)` ohne Wert gilt als nicht passend.
- In allen Fällen zeigt „Originalfarben“ die Mail, wie der Absender sie gemacht hat.

## Folgen

- `docs/DESIGN.md` (Lesebereich), `docs/ARCHITECTURE.md` (Security boundaries: was der
  Farbdurchgang ändern darf), Kommentar zu `--ui-mail-canvas` in `tokens.css`.
- Die Konstanten `DARK_CANVAS`, `DARK_TEXT`, `DARK_LINK` sind die Werte von `--ui-surface-raised`,
  `--ui-text-primary` und `--ui-accent` im dunklen Design; ein Test liest `tokens.css` und hält sie
  gleich. Ändert sich die Farbwelt, ändern sich beide zusammen.
- Ausgehende Mails: 0067.
