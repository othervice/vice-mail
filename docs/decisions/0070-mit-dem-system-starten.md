# 0070 — Mit dem System starten: eigener Anmeldeeintrag statt Plugin

- **Datum:** 25.11.2026
- **Anlass:** Stufe 21, Rückmeldung 10: „optional mit dem System starten“ (Standard aus), dann
  ohne Fenster im Hintergrund.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, 7+10)
- **Code:** `app/src-tauri/src/autostart.rs`, `lib.rs` (`autostart`, `set_autostart`),
  `commands/desktop.rs` (`get_autostart`, `set_autostart`), `windows/installer-hooks.nsh`,
  Oberfläche `app/src/features/desktop/BackgroundSettings.tsx`
- **Tests:** `autostart::tests` (Ein/Aus mit einer Attrappe der Registry bzw. des Ordners, der
  Schalter des Systems gewinnt, Reparatur nach Verschieben, Entwicklungs-Builds, Plist hin und
  zurück und mit `plutil -lint`, Run-Wert und `StartupApproved`, App Translocation), Vitest
  `background.test.tsx`, Desktop `background.e2e.mjs` (Teil 6, im Testordner)

## Entscheidung

- **macOS:** ein Launch Agent des Nutzers, `~/Library/LaunchAgents/<Bundle-ID>.plist` mit
  `RunAtLoad`, `ProgramArguments` = ausführbare Datei der App und `--background`,
  `ProcessType Interactive`, `LimitLoadToSessionType Aqua` und `AssociatedBundleIdentifiers`
  (damit Systemeinstellungen → Anmeldeobjekte die App mit Namen und Symbol zeigt). Geschrieben
  daneben und umbenannt; launchd liest ihn bei der nächsten Anmeldung, jetzt startet nichts.
  macOS 13 und neuer zeigen ihn unter „Im Hintergrund erlauben“; dort ausgeschaltet zeigt ihn
  die App weiter als an (nicht ohne private API lesbar, steht in `docs/INSTALL.md`).
- **Windows:** der Wert `<Produktname>` unter `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
  mit der Exe in Anführungszeichen und `--background` (über `windows-registry`, schon im Build).
  Der Schalter des Task-Managers schreibt `…\Explorer\StartupApproved\Run\<Name>` (erstes Byte
  gerade: an, ungerade: aus); die App liest ihn (aus heißt: Schalter aus) und schreibt beim
  Einschalten „an“, so stimmen beide überein. Tauris Deinstallation löscht den Run-Wert desselben
  Namens schon (außer bei Updates), der eigene Hook den `StartupApproved`-Wert ebenso.
- **Die Registrierung ist die Wahrheit:** der Kern speichert nichts dazu; `getAutostart` fragt das
  System. Beim Start wird ein eingeschalteter Eintrag, der eine andere Exe oder andere Argumente
  nennt (App verschoben), für diese neu geschrieben.
- **Entwicklungs-Builds** (`debug` ohne `e2e`) tragen sich nicht ein (`available: false`, der
  Schalter ist gesperrt und sagt warum): ein Eintrag auf `target/debug` startete später einen
  alten Stand. Eine App, die macOS per App Translocation aus einer zufälligen Kopie startet, auch
  nicht (der Pfad wäre beim nächsten Mal weg). **Desktop-Test-Builds** schreiben den Launch Agent
  in den Ordner des Tests (`$MAIL_APP_E2E_DATA_DIR/launch-agents/`), nie in den des Nutzers.

## Verworfen

- **`tauri-plugin-autostart`** 2.7.0 (Apache-2.0 OR MIT, gepflegt): bringt `auto-launch` 0.6 mit
  `smappservice-rs` und `os_info` und registriert Befehle für die Oberfläche (unbenutzt, aber
  vorhanden); sein macOS-Weg über `SMAppService.mainApp` kann kein `--background` übergeben,
  der Weg über Launch Agent tut dasselbe wie hier. Ohne Plugin: keine neue Abhängigkeit, ~150
  Zeilen, mit Attrappe testbar.
- **`SMAppService`** (macOS 13+, der neuere offizielle Weg): `mainApp` startet die App ohne
  Argumente, sie wüsste nicht, dass sie unsichtbar bleiben soll (die Apple-Events-Prüfung
  `keyAELaunchedAsLogInItem` bräuchte FFI); `agentServiceWithPlistName` verlangt eine Plist im
  Bundle. Die Bindung `objc2-service-management` hatte zudem keine Veröffentlichung in den
  letzten 12 Monaten (0.3.2 vom 04.10.2025, CLAUDE.md). Ein Launch Agent ist ebenfalls ein
  dokumentierter Weg von launchd und läuft ab macOS 11 (Mindestversion der App).
- **Aufgabenplanung (Windows)**: mehr Rechte und Schritte als ein Run-Wert, ohne Nutzen hier.

## Rückweg

`autostart.rs` und die beiden Befehle entfernen; vorhandene Einträge löscht der Nutzer über die
Systemeinstellungen bzw. den Task-Manager (Windows: auch die Deinstallation).

## Ungeprüft

- Windows: Run-Wert, `StartupApproved`, Start nach der Anmeldung ohne Fenster und der
  Deinstallations-Hook sind nur übersetzt (`cargo xwin clippy`) und mit einer Attrappe getestet,
  nicht auf einem Windows-Rechner (`docs/testing/windows-checklist.md`, 21.4).
- macOS: Der Start durch launchd nach einer echten Anmeldung ist nicht ausgeführt (der Test
  schreibt nur in seinen eigenen Ordner, kein Eintrag für den Nutzer des Testrechners); geprüft
  sind Inhalt und Gültigkeit der Plist (`plutil -lint`) und, als Unit-Test, die Erkennung von
  `--background` (`lifecycle::tests::the_login_item_starts_without_a_window`); ein ganzer Start mit
  dem Argument ist nicht im Desktop-Test.
