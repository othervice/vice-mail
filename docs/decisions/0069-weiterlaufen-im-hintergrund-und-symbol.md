# 0069 — Weiterlaufen im Hintergrund, Symbol in Menüleiste und Infobereich, Pause

- **Datum:** 25.11.2026
- **Anlass:** Stufe 21, Rückmeldung 10: Der Mensch hat am 25.11. aus den Vorschlägen gewählt:
  „Symbol im Infobereich (Windows) bzw. in der Menüleiste (Mac) mit Zahl und ‚Neue Mail‘; Fenster
  schließen lässt die App weiterlaufen, optional mit dem System starten. Sonst kommen Popup und
  Mitteilungen nur, solange das Fenster offen ist.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, 7+10)
- **Code:** `crates/mail-model/src/desktop.rs`, `crates/mail-engine/src/desktop.rs`,
  `crates/mail-engine/src/comfort/mod.rs` (Pause), `app/src-tauri/src/{lifecycle,tray,badge}.rs`,
  `app/src-tauri/src/commands/desktop.rs`, `events.rs`, `links.rs`, `popup.rs`,
  `notifications.rs`, `lib.rs`; Oberfläche `app/src/features/desktop/`
- **Tests:** `desktop::tests` (Kern und Modell), `comfort::tests::a_pause_keeps_new_mail_quiet_until_it_ends`,
  `lifecycle::tests`, `tray::tests`, `tests::requests_wait_for_the_main_window_s_page`,
  `tests::only_the_main_window_answers_the_close_note`, Vitest `background.test.tsx` (mit axe hell
  und dunkel), Desktop `background.e2e.mjs`

## Entscheidung

### Fenster schließen heißt nicht beenden

- Neue Geräte-Einstellung `desktop.background` (`BackgroundSettings`): `keepRunning` (Standard
  an), `menuBarIcon` (macOS, Standard an). Aus: wie bis 0.1.1, die App endet mit ihrem letzten
  Fenster.
- Mit „Weiterlaufen“ lässt die Shell das Hauptfenster beim Schließen **ganz gehen**, Web-View
  eingeschlossen. Tauri fragt beim letzten Fenster nach dem Beenden (`ExitRequested` ohne Code);
  diese eine Anfrage wird abgelehnt. Beenden mit Code (`AppHandle::exit`: „Beenden“ im Menü des
  Symbols, im Hinweis, der Neustart des Updaters) und das Beenden von macOS selbst (⌘Q, Dock-Menü,
  Abmelden: `applicationWillTerminate`, kein `ExitRequested`) beenden wie bisher.
- macOS: die App bleibt im Dock wie Mac-Apps ohne Fenster; ein Klick aufs Dock-Symbol
  (`RunEvent::Reopen`) oder ein erneuter Start aus dem Finder öffnet das Fenster. Windows: kein
  Taskleisten-Knopf ohne Fenster; das Symbol im Infobereich ist der Weg zurück, ein zweiter Start
  über das Single-Instance-Plugin ebenso.
- **Wieder öffnen** (`lifecycle::show_main_window`): ein neues Hauptfenster aus derselben
  Konfiguration, auf der asynchronen Laufzeit gebaut (aus einem Ereignis des Hauptthreads
  blockiert WebView2), erst unsichtbar, dann an Position und Größe des alten (maximiert bleibt
  maximiert), dann sichtbar. Auslöser: Dock, zweiter Start, Symbol und sein Menü, Klick auf eine
  Mitteilung, „Lesen“ und „N weitere“ im Popup, ein `mailto:`-Link.
- **Anfragen an das Hauptfenster warten:** `openMessageRequested`, `openInboxRequested` und
  `composeRequested`, die kommen, solange die Seite des Hauptfensters noch nicht hört, hält die
  Shell zurück (höchstens 32, die neuesten) und schickt sie, sobald die Seite `mainWindowReady`
  sagt (nach ihrem Abo der Kern-Ereignisse). So zeigt „Lesen“ im Popup die Mail auch, wenn das
  Fenster dafür erst entsteht. `takeLaunchLinks` gilt je Seite neu.
- **Einmaliger Hinweis:** Beim ersten Schließen mit „Weiterlaufen“ bleibt das Fenster offen, die
  Shell schickt `closeToBackgroundAsked` an seine Seite; die zeigt den Dialog der App (Abbrechen,
  „Vice Mail beenden“, „Weiterlaufen“) und antwortet mit `closeMainWindow(quit)`. Der Kern merkt
  sich `desktop.close_hint`; danach schließt das Fenster ohne Frage. Hört keine Seite (lädt noch),
  schließt das Fenster sofort, der Hinweis kommt beim nächsten Mal. Ein Dialog in der Seite statt
  einer Mitteilung des Systems: Mitteilungen können aus sein, der Dialog wird immer gesehen.
- Das **Popup** (0051) schließt nicht mehr mit dem Hauptfenster; es erscheint auch ohne
  Hauptfenster, nur nicht beim Beenden.
- **Start im Hintergrund:** mit `--background` (der Anmeldeeintrag aus 0070) baut die App kein
  Fenster, alles andere läuft.

### Symbol in Menüleiste und Infobereich

- Tauri-Feature `tray-icon` (Crate tray-icon 0.25.1, Tauri-eigen, 16.09.2026, MIT OR Apache-2.0,
  stand als optionale Abhängigkeit von Tauri schon im Lockfile). Keine Capability: die
  Oberfläche erreicht das Symbol nicht, nur die Shell.
- Menü: „Vice Mail öffnen“, „Neue Nachricht“, „Mitteilungen pausieren ›“ (Für 1 Stunde, Bis
  morgen, während einer Pause „Pausiert bis 14:30“ grau, „Mitteilungen fortsetzen“), „Vice Mail
  beenden“. Das Modell ist eine reine Funktion (`tray::model`), getestet.
- Texte: die Shell baut das Menü ohne Seite (Start im Hintergrund). Die Oberfläche schickt die
  Texte bei jedem Start und Sprachwechsel (`setDesktopTexts`, wie die Mitteilungstexte aus Stufe
  9); der Kern bewahrt sie (`desktop.texts`) für den nächsten Start ohne Fenster, vorher gelten
  englische Texte. `{product}` setzt die Shell aus `tauri.conf.json` ein (der Name steht nur
  dort), die Uhrzeit im 12- oder 24-Stunden-Format der Sprache (`hour12`).
- macOS: Glyphe als Vorlagenbild (`icons/tray/macos-template.png`), Zahl als Titel daneben, ein
  Klick öffnet das Menü. Abschaltbar (`menuBarIcon`); das Dock-Symbol bleibt. Windows: immer da,
  App-Symbol mit Zahl (0068), Tooltip mit Zahl, Linksklick öffnet das Fenster, Rechtsklick das Menü.
- „Neue Nachricht“: `AppCore::compose_new` schickt `composeRequested` mit leerer Anfrage; die
  Oberfläche öffnet den Composer wie bei einem `mailto:`-Link (vom gewählten Konto).
- Die Zahl ist die des Dock-Zählers (Einstellung „Zähler“ aus Stufe 9; aus heißt keine Zahl).

### Mitteilungen pausieren

- `notifications.paused_until` (Geräte-Einstellung, Unix-ms oder `null`): „Für 1 Stunde“ oder
  „Bis morgen“ (08:00 Ortszeit des nächsten Kalendertags; eine übersprungene Ortszeit rückt vor
  wie bei Snoozes), „Fortsetzen“ löscht. Jede Änderung: `notificationPauseChanged`.
- Während der Pause meldet der Kern **nichts**: kein Popup, keine Mitteilung, kein Ton, auch keine
  fehlgeschlagenen Sendungen und zurückgekehrten Snoozes, wie in Ruhezeiten; nichts wird
  nachgeholt. Snoozes kehren trotzdem zurück, die Zähler laufen weiter.
- Auch in Einstellungen → Mitteilungen (Status, „Für 1 Stunde“, „Bis morgen früh“, „Fortsetzen“),
  damit die Pause auch ohne Symbol in der Menüleiste erreichbar ist.

### Speicher

Ziel 0048 (Footprint im Leerlauf unter 250 MB). Ohne Hauptfenster gibt es keinen Web-View; der
WebContent-Prozess des Fensters endet. Gemessen wie in 0048 (Footprint der App und der
WebKit-Prozesse, für die macOS sie verantwortlich hält, `support/perf.py memory`) im
Desktop-Test `background.e2e.mjs`: Test-Build (`--debug`, Feature `e2e`), Testserver mit
alices Posteingang von 10.000 Mails, Hauptfenster auf dem Posteingang; je 60 s Leerlauf mit
offenem Fenster und 60 s nach dem Schließen in den Hintergrund (Popup zu). Vier Läufe am
25.11.2026 auf dem Mac mini (M4):

| Lauf | Fenster offen | im Hintergrund | gespart | davon WebContent |
|---|---:|---:|---:|---:|
| 1 | 172,9 MB | 81,7 MB | 91,2 MB | 89,5 MB |
| 2 | 166,0 MB | 69,3 MB | 96,7 MB | – |
| 3 | 163,3 MB | 62,6 MB | 100,7 MB | 86,8 MB |
| 4 | 173,0 MB | 86,4 MB | 86,6 MB | 89,4 MB |

Im Hintergrund bleiben die App selbst (58–65 MB, wie mit Fenster: Kern, Datenbank, Abgleich),
WebKits Netzwerkprozess (5–6 MB) und in zwei von vier Läufen der GPU-Prozess (12–16 MB), den
WebKit erst später beendet. Der WebContent-Prozess (87–90 MB) ist jedes Mal weg: im Hintergrund
63–86 MB statt 163–173 MB, etwa die Hälfte. Ein Debug-Build braucht für die App selbst mehr als
ein Release-Build; die Differenz durch das Schließen ist davon unabhängig. Wieder öffnen baut einen
neuen Web-View (der Desktop-Test wartet darauf; gemessen nicht, gefühlt wie ein Warmstart,
0048: unter 0,5 s bis zum benutzbaren Posteingang).

## Verworfen

- **Fenster nur verstecken** (`hide`): einfacher, behält Zustand der Seite, aber der Web-View
  (WebContent 50–150 MB Footprint, 0048) bliebe die ganze Zeit im Speicher, auch nach einem Start
  im Hintergrund, der gar kein Fenster braucht.
- **Hinweis als Mitteilung des Systems** nach dem Schließen: blockiert nicht, kann aber
  ausgeschaltet oder in einem Fokus untergegangen sein; dann wüsste niemand, wie man beendet.
- **Unter macOS ohne Dock-Symbol im Hintergrund** (`ActivationPolicy::Accessory`): ungewohnt für
  eine Mail-App, und ⌘Q über das App-Menü wäre ohne Fenster nicht erreichbar.

## Rückweg

Schalter „Weiterlaufen“ aus. Ganz zurück: `lifecycle::keeps_running` immer `false`, Popup wieder
mit dem Hauptfenster schließen (`popup::follow_main_window` aus Stufe 18), `tray.rs` und das
Feature `tray-icon` entfernen.

## Nachweis

- Kern: `desktop::tests` (Pause eine Stunde, bis morgen 08:00 in UTC und Berlin, auch kurz nach
  Mitternacht, Ablauf, Ereignis, Fortsetzen, Geräte-Einstellungen nicht im Sync,
  `compose_new`), `comfort::tests::a_pause_keeps_new_mail_quiet_until_it_ends` (Fake-Server:
  während der Pause weder Popup noch Mitteilung, danach wieder, die Mail der Pause nie).
- Shell: `lifecycle::tests` (Entscheidung beim Schließen, nur das letzte Fenster ohne Code wird
  gehalten, Zustandsfolge vom Start bis Beenden, wartende Anfragen in Reihenfolge, höchstens 32,
  `--background`), `tray::tests` (Menü, Zahl in Tooltip und Titel, Pause mit Uhrzeit in 24 und
  12 Stunden und auf Deutsch, jeder wählbare Eintrag hat eine Aktion), über den Mock-Runtime:
  eine Anfrage wartet, bis die Seite hört (`requests_wait_for_the_main_window_s_page`), nur das
  Hauptfenster beantwortet den Hinweis, Standardwerte über die Brücke.
- Oberfläche: `app/tests/background.test.tsx` (13 Tests: Schalter unter macOS und Windows,
  gesperrter Autostart, abgelehnte Registrierung, Pause mit Uhrzeit und Ereignis aus dem Menü,
  Deutsch, Hinweis mit Weiterlaufen/Beenden/Escape, auch während des ersten Starts, Texte des
  Menüs, `mainWindowReady`, axe hell und dunkel).
- Desktop-Test `app/tests/e2e/background.e2e.mjs` auf macOS gegen den Testserver, 8 Schritte,
  dreimal hintereinander grün: Symbol in der Menüleiste mit Menü; erstes Schließen zeigt den
  Hinweis, „Weiterlaufen“ schließt, der Prozess läuft weiter (WebDriver antwortet, kein
  Hauptfenster); neue Mail ohne jedes Fenster öffnet das Popup, das Symbol zählt sie; „Lesen“ per
  echtem Klick baut das Hauptfenster neu und zeigt die Mail (die Anfrage hat gewartet); zweites
  Schließen ohne Hinweis; „Öffnen“ und „Neue Nachricht“ des Menüs bringen das Fenster zurück,
  letzteres mit Composer; Pause aus dem Menü: „Pausiert bis 1:39 PM“, keine Popup-Karte für die
  Mail der Pause, nach „Fortsetzen“ für die nächste; Speicher wie oben.

## Ungeprüft

- Windows: Symbol im Infobereich, Linksklick, Menü, kein Taskleisten-Knopf ohne Fenster, zweiter
  Start, Neuaufbau des Fensters mit WebView2 und der Speicher ohne Fenster sind nur mit
  `cargo xwin clippy` übersetzt, nicht auf einem Windows-Rechner gesehen
  (`docs/testing/windows-checklist.md`, Abschnitt 21).
- macOS, echte Bedienung: Der Desktop-Test löst das Schließen über `WebviewWindow::close` aus
  (dasselbe `CloseRequested` wie der rote Knopf) und wählt Menüeinträge über dieselbe Funktion wie
  der Klick (`tray::perform`); den Klick in die Menüleiste und aufs Dock-Symbol (`Reopen`) kann
  er nicht auslösen. Das Symbol in der Menüleiste ist auf dem Testrechner (Sperrbildschirm) nicht
  per Bildschirmfoto geprüft.
