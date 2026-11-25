# 0051 — Popup für neue Mails: eigenes Fenster statt Mitteilung des Systems

- **Datum:** 24.11.2026
- **Anlass:** Wunsch des Menschen vom 24.11.2026 nach dem ersten Test von 0.1.0 (Stufe 18,
  Block C): Kommt neue Mail an, soll sich unten rechts am Bildschirmrand ein Popup öffnen, mit
  Absender, kurzem Textausschnitt und den Knöpfen „Lesen“ und „Löschen“.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Block C).
- **Code:** `crates/mail-model/src/popup.rs`, `crates/mail-model/src/inbox.rs`
  (`NewMailStyle`), `crates/mail-engine/src/comfort/{popup,notify}.rs`,
  `app/src-tauri/src/{popup,sound}.rs`, `app/src-tauri/src/commands/popup.rs`,
  `app/src-tauri/src/events.rs`, `app/src-tauri/capabilities/new-mail-popup.json`,
  `app/popup.html`, `app/src/popup.tsx`, `app/src/features/popup/`,
  `app/src/features/comfort/NotificationSettings.tsx`.

## Entscheidung

### Ein eigenes Fenster der App, nicht die Mitteilung des Systems

- **Warum nicht die Mitteilung mit Knöpfen?** `tauri-plugin-notification` 2.8.1 zeigt auf dem
  Desktop zwar Aktionen, aber nicht so, wie der Wunsch es verlangt: Auf macOS erscheinen zwei
  oder mehr Aktionen nur in einem Menü „Optionen“ (ein Klick auf den Menüknopf meldet die erste),
  unter Windows kommt ein Klick nur an, solange der Toast sichtbar ist (nicht aus dem
  Info-Center, 0020), und eine gezeigte Mitteilung lässt sich auf beiden Systemen weder ändern
  noch entfernen (`remove_active` tut dort nichts). „Gelöscht“ mit „Rückgängig“, ein gesperrtes
  „Löschen“ mit Begründung, das Kontaktbild und zwei Zeilen Ausschnitt gingen so nicht; ein
  eigener Weg über `UNUserNotificationCenter` bzw. Toast-Aktivierung über COM bräuchte FFI mit
  `unsafe` außerhalb von `webview2-guard`. Ein Fenster der App sieht auf beiden Systemen gleich
  aus, zeigt Bild, Ausschnitt und Konto wie die Liste und nutzt die vorhandene Outbox.
- **Was der Kern entscheidet, bleibt gleich.** Ob eine Mail gemeldet wird, entscheidet
  `notify::announced` für beide Wege (nur nach dem Erstabgleich, stumme Konten, Ruhezeiten,
  Kategorien, lokale Regeln). Neu ist nur die Darstellung: Einstellung `newMailStyle`
  (`popup` Standard, `system`) in `NotificationSettings`. Ältere gespeicherte Einstellungen
  kennen das Feld nicht und lesen sich als Popup; das Feld reist mit `notifications.settings` im
  Einstellungs-Sync (keine Migration). Fehlgeschlagene Sendungen und zurückgekehrte Snoozes
  bleiben Mitteilungen des Systems.
- **Eine Karte je Mail.** Der Kern hält die Karten (`comfort/popup.rs`): neueste oben, drei
  sichtbar, höchstens 50 gemerkt, darüber „N weitere im Posteingang“. Jede Änderung geht als
  ganzes Popup mit wachsender `revision` an die Senke (`NotificationSink::popup`); ein leeres
  Popup heißt „Fenster schließen“. Datum und Ausschnitt kommen aus der Listenzeile; eine gerade
  angekommene Mail hat meist noch keinen Ausschnitt, er kommt mit dem `messagesChanged` des
  Prefetch nach. Karten verschwinden mit ihrer Mail (anderswo gelöscht) und ihrem Konto.
- **Verborgene Inhalte:** Der Kern behält von der Mail weder Absender noch Betreff noch
  Ausschnitt; die Karte sagt „Neue Nachricht“, nennt bei mehreren Konten das Konto und bietet nur
  „Lesen“.
- **Lesen** ist der Klick auf eine Mitteilung (`openMessageRequested`): Hauptfenster nach vorn,
  Mail im Posteingang. **Löschen** ist genau die Listenaktion (`MailAction::Delete` über die
  Outbox in den Papierkorb); die Karte zeigt „Gelöscht“ mit „Rückgängig“ (Batch der Outbox).
  Nur lesende Konten: der Knopf ist gesperrt und sagt warum; der Kern lehnt ohnehin mit
  `read_only` ab. **„N weitere“** schickt das neue Ereignis `openInboxRequested` (Posteingang des
  Kontos, bei mehreren Konten der vereinte).
- **Ton:** Die Mitteilung des Systems spielte den Ton der Stufe 16 selbst; beim Popup spielt ihn
  die Shell (`sound.rs`), einmal je Abgleichfenster wie zuvor. macOS: `NSSound soundNamed:`,
  „Default“ als `NSBeep` (der Hinweiston des Nutzers). Windows: Die Toast-Töne
  `ms-winsoundevent:Notification.*` sind Ereignisse des Klangschemas; die Shell liest deren Datei
  (`HKCU\AppEvents\Schemes\Apps\.Default\<Ereignis>\.Current`) und spielt sie mit WinRTs
  `MediaPlayer` (Kategorie „Alerts“, nicht in den Mediensteuerungen). Alles über sichere
  Bindungen aus Crates, die schon im Build waren (`objc2-app-kit`, `objc2-quartz-core`,
  `windows`, `windows-registry`); kein `unsafe` außerhalb von `webview2-guard`.

### Fenster, Fokus und Lage

- Label `new-mail-popup`, Seite `popup.html` (eigener kleiner Einstieg des Vite-Builds, ohne
  Editor, Liste und Lizenzen), gebaut über `windows::guard`/`finish` wie jedes Fenster
  (Navigations-Hook, keine neuen Fenster, keine Downloads, WebView2-Rahmenschutz). Es entsteht
  erst mit der ersten Karte und wird mit der letzten zerstört (kein leerer Web-View im Speicher);
  es schließt auch mit dem Hauptfenster.
- Ohne Rahmen, immer oben, nicht in der Taskleiste (`skip_taskbar`), auf allen Spaces bzw.
  Desktops, 360 px breit; die Höhe meldet die Seite (`fit_new_mail_popup`, 48 bis 640 px).
  Ort: unten rechts im **Arbeitsbereich** (macOS `visibleFrame` ohne Menüleiste und Dock,
  Windows `rcWork` ohne Taskleiste) des Bildschirms mit dem Hauptfenster, sonst des
  Hauptbildschirms, 16 px vom Rand (reine Funktion `popup::bottom_right`, getestet).
- **Kein Fokus.** Tauri 2.12 bietet `focused(false)`, `focusable(false)` und
  `accept_first_mouse(true)`. macOS: Das Fenster kann nie Key werden (`canBecomeKeyWindow`
  falsch), es wird ohne Aktivierung der App nach vorn gestellt und erst gezeigt, wenn die Seite
  sich vermessen hat; der erste Klick trifft den Knopf. Über `NSApplication.windows` (sichere
  Suche nach der Adresse, wie im Desktop-Test) setzt die Shell: nicht im Fenstermenü und damit
  nicht in der Fensterliste des Docks, `CanJoinAllSpaces | Transient | IgnoresCycle |
  FullScreenAuxiliary`, durchsichtiges Fenster mit runder Inhaltsebene (Radius 16) und Schatten.
  Windows: `WS_EX_NOACTIVATE`; das Fenster wird **sichtbar erzeugt** (nur die Erzeugung zeigt
  ohne Aktivierung, ein späteres `show()` würde aktivieren), Windows 11 rundet rahmenlose
  Fenster mit Schatten selbst.
- **Grenze, ehrlich:** Ein Klick auf „Löschen“ in einem gewöhnlichen `NSWindow` aktiviert die App
  nach den Regeln des Fensterservers; nur ein `NSPanel` mit `nonactivatingPanel` verhinderte das,
  und das gibt Tauri nicht her (Klassenwechsel nur mit `unsafe`). Erscheinen, Überfahren und das
  Ausblenden nehmen den Fokus nicht.

### Eigene, minimale Rechte

- Die eigenen Befehle der App unterliegen in Tauri 2 keiner Capability, solange die App kein
  ACL-Manifest für alle (rund 250) Befehle führt; ohne weiteres dürfte jedes Fenster der App alle
  Befehle rufen. Statt alle Befehle in `build.rs` doppelt zu führen, prüft die Shell vor dem
  Handler: Aus dem Fenster `new-mail-popup` laufen nur `get_new_mail_popup`, `read_from_popup`,
  `delete_from_popup`, `undo_popup_delete`, `dismiss_popup`, `open_inbox_from_popup` und
  `fit_new_mail_popup` (`popup::restrict`, `POPUP_COMMANDS`); jeder andere Befehl wird
  abgelehnt, bevor er läuft. Ein Test prüft, dass jeder dieser Namen ein Befehl ist, und einer
  über den Mock-Runtime mit der echten Konfiguration, dass das Popup sonst nichts rufen darf.
- Capability `new-mail-popup`: nur `core:event:allow-listen` und `allow-unlisten`. Die
  Ereignisse des Kerns gehen nicht an das Popup (`events::hears_core_events`), es hört nur
  `new-mail-popup-changed`, das die Shell gezielt an sein Fenster schickt. Fenster-, Webview- und
  Plugin-Rechte hat es keine (es schließt sich nie selbst; das macht die Shell).
- Gleiche CSP wie alle Seiten der App; das Bild des Absenders ist das `data:`-JPEG des Helfers
  (Stufe 17), nie aus dem Netz.

### Speicher

Das Fenster lebt nur, solange es Karten gibt (nach zehn Sekunden auf dem Bildschirm ohne Zeiger
darauf blendet die Seite es aus; gelöschte Karten nach fünf Sekunden). Es zählt nur Zeit, in der
die Seite sichtbar ist (`visibilitychange`): Unter dem Sperrbildschirm oder bei schlafendem
Bildschirm hält WebKit die Seite für verborgen und bremst ihre Zeitgeber ohnehin; das Popup
wartet dann auf den Menschen und zählt ab dem Moment, in dem er es sehen kann. Damit es nie stehen bleibt, gilt der Zeiger
nach einer Minute ohne jedes Zeigerereignis als fort, auch wenn kein „Verlassen“ kam. Seine Seite lädt `popup.js` (8 KB) und die
gemeinsamen Teile (React, Übersetzungen, Bindungen), nicht den Editor (415 KB), die Lizenzliste
und den Rest der Oberfläche.

## Verworfen

- **Aktionen der Mitteilung** (Plugin oder eigene FFI, siehe oben): auf macOS nur als Menü, ohne
  „Rückgängig“ und gesperrten Knopf; eigene FFI hieße `unsafe` auf zwei Plattformen.
- **ACL-Manifest für alle Befehle:** dieselbe Wirkung für das Popup, aber jeder neue Befehl müsste
  zusätzlich in `build.rs` und einer Berechtigungsliste stehen; ein vergessener Eintrag fiele erst
  im laufenden Fenster auf.
- **`index.html?window=popup`:** hätte die ganze Oberfläche geladen und dafür die Befehle für
  Einstellungen und Fensterlayout gebraucht.
- **`macOSPrivateApi` für ein durchsichtiges Fenster:** private API für die ganze App; die runde
  Inhaltsebene genügt.

## Rückweg

`newMailStyle: system` stellt das Verhalten der Stufe 9 wieder her. Ohne Popup: `NewMailStyle`
entfernen (der Kern ruft dann immer `notify::new_mail`), `popup.rs`, `commands/popup.rs`, die
Capability und `popup.html` löschen.

## Nachweis

- Kern: `comfort/popup_tests.rs` (Fake-Server: Karten nach denselben Regeln, Ton, Ausschnitt nach
  dem Prefetch, Lesen, Löschen in den Papierkorb und Rückgängig, nur lesendes Konto, „N weitere“,
  verborgene Inhalte, Ruhezeiten, Stil „System“, entferntes Konto), `notify.rs`, `popup.rs`.
- Shell: Lage aus dem Arbeitsbereich (reine Funktion), Rechte des Popups über den Mock-Runtime
  mit der echten Konfiguration, Klangnamen (macOS: jeder Name der Liste ist ein `NSSound`).
- Oberfläche: `app/tests/new-mail-popup.test.tsx` (Karten, Zeit, axe hell und dunkel).
- Desktop-Test `app/tests/e2e/new-mail-popup.e2e.mjs` auf macOS gegen den Testserver: Popup mit
  Absender und Ausschnitt, nie Key, aktiver Zustand der App und Fokus des Hauptfensters unverändert (die App war im Test nicht aktiv),
  Ebene „schwebend“, alle Spaces, nicht im Fenstermenü, Radius 16, 16 pt über dem Dock und vom
  rechten Rand; „Lesen“ per echtem Klick zeigt die Mail im Hauptfenster; „Löschen“ legt sie auf
  dem Server in den Papierkorb, „Rückgängig“ holt sie zurück; nur lesendes Konto sperrt
  „Löschen“; drei Karten und „2 weitere“; fremde Befehle aus dem Popup abgelehnt; keine
  CSP-Verletzung; axe ohne ernste Befunde hell und dunkel.

## Ungeprüft

- Windows: Lage über der Taskleiste, kein Fokuswechsel beim Erscheinen und beim Klick
  (`WS_EX_NOACTIVATE` mit WebView2), runde Ecken unter Windows 11 und der Ton über `MediaPlayer`
  sind nur mit `cargo xwin clippy` übersetzt, nicht auf einem Windows-Rechner gesehen.
- macOS, echter Zeiger: Die Klicks des Desktop-Tests gehen direkt an den Web-View, nicht über den
  Fensterserver. Ob ein echter Klick auf „Löschen“ die App aktiviert, während eine andere App
  vorn ist, und ob der Web-View eines Fensters, das nie Key ist, jedes Betreten und Verlassen des
  Zeigers hört, ist nicht gemessen (deshalb die Minute ohne Zeigerereignis als Rückfall).
  „Lesen“ zeigt im Test die Mail im Hauptfenster, aktiviert die App aber nicht (das Hauptfenster
  ist danach nicht Key): macOS lässt eine App ohne Zutun des Nutzers nicht nach vorn (kooperative
  Aktivierung), und der Testklick ist kein solches Zutun; ein echter Klick ist es.
- Das Ausblenden nach zehn Sekunden auf dem Bildschirm: Der Testrechner zeigt den Sperrbildschirm,
  die Seite ist dort „hidden“; der Desktop-Test prüft daher, dass das Popup wartet, die zehn
  Sekunden selbst nur Vitest.
- Das Bild des Fensters mit runden Ecken und Schatten: `screencapture` darf auf dem Testrechner
  keine Fenster aufnehmen; geprüft sind die Eigenschaften des `NSWindow`, die Bildschirmfotos
  zeigen den Inhalt.
