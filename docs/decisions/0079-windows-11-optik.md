# 0079 — Windows-11-Optik: Mica, Titelleiste in den Farben der App, Andock-Layouts

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 9 (vom Menschen am 26.11.2026 aus den Vorschlägen gewählt):
  „Fensterhintergrund mit Mica wie bei Windows-11-Apps, Titelleiste in den Farben der App, beim
  Überfahren des Maximieren-Knopfs die Andock-Leiste von Windows.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22). Auf dem Mac nicht
  zu sehen; wie es aussieht, prüft der Mensch auf dem PC (`docs/testing/windows-checklist.md`,
  22.1–22.4).
- **Code:** `app/src-tauri/src/backdrop.rs` (Entscheidung je Fenster, Effekt, Durchsichtigkeit),
  `lib.rs` (`open_main_window`), `windows.rs` (`open_window`), `commands/app.rs`
  (`get_window_backdrop`), `crates/mail-model/src/ipc.rs` (`WindowBackdrop`);
  `app/src/features/app-info/useBackdropAttribute.ts`, `queries.ts`, `app/src/app/App.tsx`,
  `AppShell.tsx`, `IconRail.tsx`, `SidebarColumn.tsx` (`data-chrome`), `app/src/styles/app.css`.
- **Tests:** `backdrop::tests` (Wahl nach Build, Transparenz, Kontrastdesign; je Fenster),
  `app/tests/backdrop.test.tsx` (mit dem übersetzten Stylesheet: was mit Mica durchsichtig wird,
  was undurchsichtig bleibt, mit reduzierter Transparenz und erzwungenen Farben, ohne Mica),
  `app/tests/contrast.test.ts` (jedes Text- und Bedienelement-Paar auf `rail` und `canvas` gegen
  Mica in Hell und Dunkel, alle Akzentthemen); `cargo xwin clippy` für Windows.

## Ausgangslage

- Unter Windows hat jedes Fenster die native Titelleiste mit den nativen Knöpfen
  (`titleBarStyle: "Overlay"` und `hiddenTitle` in `tauri.conf.json` wirken nur auf macOS; die
  Oberfläche setzt unter Windows keine Zieh-Bereiche, `app/tests/frame.test.tsx`). Die
  Titelleiste folgt schon der Wahl hell/dunkel der App (`window_theme.rs`, `set_theme`, bei tao
  `DWMWA_USE_IMMERSIVE_DARK_MODE`), hat aber Windows' eigene Farben (Weiß bzw. `#202020`), nicht
  die der App.
- Die Andock-Layouts erscheinen beim Überfahren des Maximieren-Knopfs, solange es der native
  Knopf ist und das Fenster maximierbar ist; beides gilt für alle Fenster außer dem Popup.

## Entscheidung

### Mica über Tauris Fenstereffekte, ohne `unsafe`

- Jedes App-Fenster bekommt unter Windows 11 Tauris Effekt `Mica` (window-vibrancy setzt
  `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_MAINWINDOW` ab Build 22523, davor das ältere Attribut).
  `Mica` statt `MicaDark`/`MicaLight`: das Material richtet sich nach dem Dunkelmodus des
  Fensters, den `window_theme` auf der Wahl der App hält. So ist die Titelleiste im Material, hell
  oder dunkel wie die App, und wechselt mit ihr.
- **Hauptfenster:** zusätzlich durchsichtig gebaut (`transparent(true)`: Fenster mit
  `DwmEnableBlurBehindWindow`, WebView2 mit durchsichtigem Hintergrund), damit Mica hinter der
  Seite liegt. Die Oberfläche fragt `get_window_backdrop` (`mica` oder `none`) und setzt
  `data-backdrop="mica"` auf `<html>`; `app.css` lässt dann `html`, `body` und die Elemente mit
  `data-chrome` durchsichtig: den Rahmen (`AppShell`), die schmale Leiste und die angedockte
  Spalte der Konten und Ordner. Das Material der Titelleiste läuft so links am Fenster herunter,
  ohne Naht, wie bei den Apps von Windows (Einstellungen, Explorer).
- **Undurchsichtig bleiben** Liste, Lesebereich, Composer, Einstellungen, KI-Seitenleiste, Menüs,
  Dialoge, Hinweise und die Spalte, die in einem schmalen Fenster über dem Inhalt liegt (sonst
  schiene der Inhalt durch). Text zum Lesen steht also immer auf den Flächen aus `tokens.css` mit
  ihrem geprüften Kontrast, auf jedem Hintergrundbild.
- **Andere Fenster** (Mail, Composer, `.eml`, Quelltext): Mica nur in der Titelleiste, die Seite
  bleibt undurchsichtig wie bisher (kein durchsichtiges Fenster, nichts zu ändern in der
  Oberfläche). Das Popup für neue Mails behält sein eigenes Aussehen (ohne Rahmen, 0051).
- Kein Win32-Aufruf von uns selbst: `DWMWA_CAPTION_COLOR` (Titelleiste in einer festen Farbe der
  App) bräuchte `unsafe` in `webview2-guard`, verträgt sich nicht mit Mica (eine gesetzte Farbe
  schaltet das Material in der Titelleiste ab) und gibt es auf Windows 10 nicht. Mit Mica sind
  Titelleiste und Seitenrand aus einem Stück; ohne Mica bleibt die Titelleiste, wie sie ist.

### Wann Mica, wann nicht

- `backdrop::choose`: Mica nur ab Windows-Build 22000 (Windows 11), mit eingeschalteten
  Transparenzeffekten (`HKCU\…\Themes\Personalize\EnableTransparency`) und ohne Kontrastdesign
  (`HKCU\Control Panel\Accessibility\HighContrast\Flags`, Bit 0), gelesen mit `windows-registry`
  (sicher) jedes Mal, wenn ein Fenster gebaut wird. Sonst: kein Effekt, ein undurchsichtiges
  Fenster, die Farben von heute (Windows 10, macOS, Linux). Ein nicht lesbarer Build zählt als
  „kein Windows 11“, nicht lesbare Einstellungen als Windows' Standard.
- Ändert der Mensch das bei offenem Fenster, greift CSS: Die Regeln für Mica gelten nur unter
  `(prefers-reduced-transparency: no-preference) and (forced-colors: none)`. Mit ausgeschalteter
  Transparenz oder eingeschaltetem Kontrastdesign sind Rahmen, Leiste und Spalte sofort wieder
  undurchsichtig in den Farben der App (darunter zeichnet Windows Mica dann ohnehin als feste
  Farbe). Kennt die WebView eine der Abfragen nicht, gilt die ganze Regel nicht: lieber
  undurchsichtig als durchsichtig ohne Material dahinter. Das nächste Fenster wird nach der neuen
  Einstellung gebaut.
- Die Oberfläche wartet beim Start auf die Antwort (wie auf Einstellungen und App-Info), damit der
  erste Rahmen nicht erst undurchsichtig und dann durchsichtig erscheint.

### Kontrast auf Mica

- Was auf der Leiste und in der Spalte steht, liegt mit Mica auf dem Material statt auf `rail`.
  Mica ist das Hintergrundbild, weichgezeichnet, mit fester Helligkeit eingefärbt (Windows App
  SDK, MicaController: hell `#f3f3f3` zu 50 %, dunkel `#202020` zu 80 %, Helligkeit zu 100 %);
  vom Bild bleibt nur der Farbton. `contrast.test.ts` prüft jedes Paar auf `rail` und `canvas`
  gegen das dunkelste helle und das hellste dunkle Mica, das das zulässt, mit Spielraum:
  `#e6e6e6` und `#282828`, in Hell und Dunkel und allen acht Akzentthemen (Text ≥ 4,5:1,
  Bedienelemente ≥ 3:1). Alle bestehen; am knappsten sind Violett und Rosa als Akzenttext im
  Dunkeln (4,6:1).
- Überfahren und Auswahl in der Spalte behalten ihre undurchsichtigen Flächen (`rail-hover`,
  `selected`); sie liegen als ruhige Felder auf dem Material.

### Andock-Layouts: native Knöpfe behalten

- Die Andock-Leiste zeigt Windows nur am echten Maximieren-Knopf (oder an einem eigenen Knopf,
  der `WM_NCHITTEST` mit `HTMAXBUTTON` beantwortet). Die App zeichnet keine eigenen Knöpfe, also
  bleibt alles beim nativen Rahmen: Andock-Layouts, Doppelklick, Ziehen an den Rand, Tastatur
  (Win+Pfeil, Win+Z) und Bildschirmleser funktionieren, wie Windows es vorsieht. Eine eigene
  Titelleiste mit Suche oder Tabs im Rahmen (wie das neue Outlook) bräuchte eine Unterklasse des
  Fensters mit `unsafe`; dafür gibt es keinen Auftrag.

## Ungeprüft

- Auf Windows ist nichts davon gesehen. Übersetzt mit `cargo xwin clippy`, Verhalten der Seite
  mit dem echten Stylesheet in Vitest geprüft. Zu sehen ist erst auf dem PC: ob Mica bei
  `transparent(true)` hinter dem durchsichtigen WebView2 wirklich erscheint (so beschreiben es
  Tauri und window-vibrancy), ob die Titelleiste beim Wechsel hell/dunkel in der App mitwechselt,
  ob beim Größenändern Ränder flackern, ob WebView2 `prefers-reduced-transparency` mit der
  Windows-Einstellung „Transparenzeffekte“ verbindet (sonst bleibt die Spalte nach dem
  Ausschalten durchsichtig, Windows zeichnet dahinter dann eine feste Farbe) und wie das Material
  über hellen, dunklen und bunten Hintergrundbildern wirkt.
- Die Andock-Layouts sollten schon in 0.1.2 erschienen sein (native Knöpfe); falls nicht, liegt es
  nicht am Rahmen der App.

## Rückweg

In `backdrop::choose` immer `WindowBackdrop::None` zurückgeben: kein Effekt, kein durchsichtiges
Fenster, die Oberfläche setzt `data-backdrop` nicht, alles ist wie in 0.1.2. Ganz entfernen:
`backdrop.rs`, den Befehl, `useBackdropAttribute`, die `data-chrome`-Attribute und den Block in
`app.css`.
