# 0080 — Zähler mit echter Schrift und Symbole nach dem Update neu laden

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 11. Rückmeldung zu 0.1.2 unter Windows: „Das Taskleisten-Symbol ist
  weiterhin orange statt blau; die Zahl am Symbol bei neuen Mails ist noch pixelig, sie muss super
  hochauflösend sein.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22). Ändert den Teil
  „Der Zähler in der Größe, in der Windows ihn zeigt“ von 0068: Größen, Neuzeichnen bei
  `ScaleFactorChanged`, Rot und „99+“ bleiben, die Ziffern und Formen sind neu.
- **Code:** `app/src-tauri/src/badge.rs` (Ziffern, Formen, Rasterung, Tests),
  `app/src-tauri/fonts/badge-digits.ttf` und `LICENSE.txt`, `scripts/badge-font.py`,
  `notifications.rs` und `tray.rs` (Fehler `BadgeError` werden protokolliert),
  `app/src-tauri/windows/installer-hooks.nsh` (`NSIS_HOOK_POSTINSTALL`), `scripts/licenses.mjs`
  (Eintragsart `font`), `app/src/features/settings/licenses.ts`, `app/src/i18n/`.
- **Tests:** `badge::tests` (Größe je Skalierung, Ziffern auf dem Pixelraster, nur Rot bis Weiß
  und weicher Rand in Rot, Ziffern mit Rand auf vollem Rot, Anteil voller Pixel, Mitte, Abstand
  der Ziffern, größere Ziffern bei größerer Skalierung, Abzeichen im Infobereich),
  `app/tests/licenses.test.tsx` (Schrift mit OFL-Text in der Liste). Bilder aller Zahlen 1–99 und
  „99+“ bei 100/125/150/200 % für Taskleiste und Infobereich, auch 8-fach vergrößert, ansehen mit
  dem ignorierten Test `badge::tests::write_previews` (`BADGE_PREVIEW_DIR`, Rohdaten RGBA).

## Was „pixelig“ war

0068 zeichnete die Ziffern aus zwei Pixelschriften (3×5 und 5×7), nur ganzzahlig vergrößert. Jedes
Pixel war voll weiß oder voll rot, also scharf, aber die Ziffern waren Klötzchen: bei 16 px eine
3×5-Schrift doppelt, bei 32 px eine 5×7-Schrift dreifach. Das ist das „pixelig“.

## Entscheidung: echte Glyphen, auf genau diese Pixelgröße eingepasst

- **Schrift:** Selawik Semibold, Microsofts quelloffener Ersatz für Segoe UI (gleiche Maße, sehr
  ähnliche Ziffern), SIL Open Font License 1.1. Mitgeliefert als Teilmenge von 3,6 kB
  (`fonts/badge-digits.ttf`): die Ziffern und die Großbuchstaben, an denen der Auto-Hinter die
  Ausrichtungszonen misst (oben „T H E Z O C Q S“, Grundlinie „H E Z L O C U S“, Strichstärke
  „O“). Keine Kleinbuchstaben: aus der x-Höhe würde der Hinter die ganze Schrift umskalieren, die
  Ziffern wären dann nicht mehr genau so hoch wie verlangt. `scripts/badge-font.py` schreibt die
  Datei aus `selawksb.ttf` des Releases 1.01 (fontTools).
- **Lizenz:** Die OFL erlaubt ausdrücklich, die Schrift mit Software zu bündeln und einzubetten;
  sie selbst bleibt unter der OFL (ein Nebeneinander, kein gemeinsames Werk mit dem GPL-Code).
  Eine Teilmenge ist eine veränderte Fassung, darf also nicht „Selawik“ heißen (reservierter
  Name): Sie heißt „Badge Digits“, Microsofts Copyright und der Lizenzhinweis bleiben in der
  Datei. Lizenztext in `app/src-tauri/fonts/LICENSE.txt`; `scripts/licenses.mjs` führt die Schrift
  als Eintrag der neuen Art `font` (nur Windows) in `THIRD-PARTY-NOTICES.txt` und unter
  Einstellungen → Über → Lizenzen.
- **Nicht die Systemschrift.** Segoe UI aus `C:\Windows\Fonts` zu lesen wäre dem Original am
  nächsten, aber auf dem Mac nicht prüfbar: Die Tests und die Bilder, die die Sitzung ansieht,
  wären nicht das, was Windows zeigt, und jede Windows-Version brächte andere Umrisse. Unser
  Rasterer zeichnet ohnehin nicht wie DirectWrite. Die mitgelieferte Schrift ergibt auf jedem
  Rechner dieselben Pixel, und die Tests prüfen genau sie. Zeichnen über Windows (GDI,
  DirectWrite) bräuchte `unsafe` und wäre ebenso wenig prüfbar.
- **Einpassen (Hinting):** skrifa (Google fontations, schon über hayro im Programm) liest die
  Umrisse und passt sie mit seinem Auto-Hinter (Nachbau des FreeType-Auto-Hinters; Selawik hat
  keine eigenen Anweisungen) an das Pixelraster an, im stärksten Modus (`Target::Mono`, FreeTypes
  `FT_LOAD_TARGET_MONO`): Grundlinie und Ziffernhöhe, Kanten und Breiten der Striche liegen auf
  ganzen Pixeln, in beiden Richtungen. Verglichen mit „Normal“ und „Light“ an allen Größen: Dort
  bleiben senkrechte Striche 1,4 oder 1,6 px breit, also mit einer grauen Spalte daneben. Die
  Höhe der Ziffern wird gewählt (ppem = Höhe × 2048 / 1434), nicht die Schriftgröße.
- **Rastern:** ein kleiner Scanline-Rasterer in `badge.rs` (nicht-null Windungsregel, 32
  Abtastzeilen je Pixelreihe, quer dazu der genaue Anteil jedes Pixels): Kanten auf ganzen
  Pixeln ergeben genau volle oder leere Pixel, nur Bögen und Schrägen bekommen weiche Pixel. Auch
  Kreis und Rechteck werden so gezeichnet (Viertelkreise als Bézierkurven), nicht mehr 4×4
  abgetastet. `ab_glyph_rasterizer` (ebenfalls über hayro im Baum) wurde ausprobiert und wieder
  entfernt: letztes Release August 2025, außerhalb der Zwölf-Monats-Regel.
- **Weiß auf Rot in linearem Licht gemischt**, wie der Bildschirm Licht addiert: Die weichen
  Randpixel behalten das Gewicht des Strichs, statt ihn dünn und dunkel zu machen. Rot
  `#D93025` und Weiß wie bisher (4,6:1).
- **Gleiche Größe für alle Zahlen gleicher Länge:** Für 1–9, 10–99 und „99+“ gilt je eine
  Ziffernhöhe: die größte, mit der jede dieser Zahlen auf vollem Rot steht. So springt die Zahl
  nicht von 41 auf 42. Jedes Pixel, das Weiß enthält, und seine vier Nachbarn liegen ganz in der
  Form (Rand aus Rot), höchstens 60 % der Bildhöhe.
- **Formen:** Taskleiste: ein Kreis für eine Ziffer; für zwei Ziffern und „99+“ ein Rechteck mit
  runden Ecken so breit wie das Bild (drei Viertel so hoch), wenn es größere Ziffern fasst als
  der Kreis, mit engeren Ecken nur, wo die runden keinen Platz lassen. Bei 100 % sind das 9 px
  hohe Einzelziffern, 8 px bei zwei Ziffern und 6 px bei „99+“ (vorher 9, 7 und 5 px
  Pixelschrift); bei 200 % 19, 19 und 14 px. Infobereich: das Abzeichen ist jetzt 5/8 des Symbols
  hoch (vorher 9/16), ein Kreis oder eine Pille, so breit wie die breiteste Zahl gleicher Länge
  braucht; wo runde Enden zu wenig Platz lassen, mit engeren Ecken, damit eine Ziffer nie kleiner
  ist als zwei. Bei 100 % 7 bis 8 px hohe Ziffern, 6 px bei „99+“ (vorher 5 px).
- **Das „+“** von „99+“ sind zwei Balken auf dem Pixelraster, so dick wie ein Ziffernstrich und
  etwa 60 % der Ziffernhöhe lang, mittig zur Ziffernhöhe. Das „+“ der Schrift ist anderthalb
  Ziffern breit, und der Hinter macht es bei diesen Größen breiter als die Ziffern.
- **Abstand** zwischen den Ziffern: von Tinte zu Tinte, 1 px bis 16 px Ziffernhöhe, wie Ziffern in
  Abzeichen eng stehen; nicht die Tabellenbreite der Schrift.
- **Fehler:** `overlay` und `on_icon` geben `Result<_, BadgeError>` zurück; mit der eingebauten
  Schrift kommt das nie vor (getestet). Die Aufrufer protokollieren es; die Taskleiste zeigt dann
  kein Overlay, der Infobereich das Symbol ohne Zahl, der Tooltip nennt die Zahl weiter.

## Symbol noch orange: der Installer meldet die Änderung

- Geprüft (Stufe 22): `mail-app.exe` und der Installer von 0.1.2 enthalten das blaue Symbol, auch
  jeder Eintrag von `icons/icon.ico` (16–256 px, Farbton um 214°) ist blau. Fenster- und
  Infobereichs-Symbol kommen zur Laufzeit aus dieser Ressource (`from_app_icon_resource`,
  Ausweichweg `default_window_icon`, ebenfalls `icon.ico`).
- Tauris NSIS-Vorlage ersetzt bei einem Update die Exe an Ort und Stelle und legt die
  Verknüpfungen nicht neu an (`$UpdateMode`); eine angeheftete Taskleisten-Verknüpfung zeigt dann
  weiter das Symbol, das Windows für die alte Datei zwischengespeichert hat.
- `NSIS_HOOK_POSTINSTALL` sendet jetzt nach Installation und Update `SHChangeNotify`:
  `SHCNE_UPDATEITEM` (mit `SHCNF_PATHW | SHCNF_FLUSH`) für `mail-app.exe` und jede vorhandene
  Verknüpfung (Startmenü, Desktop, an Taskleiste und Start angeheftet), danach
  `SHCNE_ASSOCCHANGED`, auf das der Explorer seine zwischengespeicherten Symbole verwirft. Über
  das System-Plugin von NSIS, ohne eigenen Code in der App. `$UpdateMode` bleibt unberührt: die
  Meldung kommt in beiden Fällen.
- Syntax mit `makensis` 3.13 (Homebrew) in einem kleinen Rahmenskript geprüft (`-WX`, ohne
  Warnung, das System-Plugin wird eingebunden); nicht in Tauris Vorlage gebaut.

## Ungeprüft

- Auf Windows ist nichts davon gesehen: Overlay und Infobereich sind mit `cargo xwin clippy`
  übersetzt und als Pixel geprüft und angesehen (Bilder bei 100/125/150/200 %). Ob die
  Taskleiste das Overlay wirklich ungeskaliert zeigt, war schon in 0068 ungeprüft.
- Ob `SHChangeNotify` reicht, damit eine angeheftete Verknüpfung das blaue Symbol zeigt, ist nur
  auf Windows zu sehen. Wenn nicht: Verknüpfung lösen und neu anheften; der Symbolspeicher lässt
  sich auch mit `ie4uinit.exe -show` erneuern. Läuft noch 0.1.1, behebt das erst das Update.

## Rückweg

`badge.rs` auf den Stand von 0068 zurücksetzen (Pixelschriften), `skrifa` aus den
Abhängigkeiten, `fonts/` und den Eintrag in `scripts/licenses.mjs` entfernen. Die zwei
`SHChangeNotify`-Zeilen und das Makro im Installer-Hook können unabhängig davon bleiben.
