# 0025 — Oberflächen-Einstellungen im Kern, Mail und Composer im eigenen Fenster

Stand 23.11.2026. Stufe 8, Backend. Die Oberfläche der Stufe 8 (Befehlspalette, änderbare
Kürzel, Layouts) hielt Kürzel, Schriftgröße, zuletzt benutzte Befehle und das Layout je Fenster
in einem markierten Ersatzspeicher im Web-Storage (`app/src/features/keyboard/localFallback.ts`).
Diese Entscheidung legt fest, wie der Kern sie übernimmt, wie der Lesebereich und der Composer
(Stufe 10) in eigene Fenster kommen, und ergänzt zwei Kleinigkeiten der Stufe 10 (Grenzen als
Konstanten, Standard-Position des Zitats). Die Oberfläche stellt in einer eigenen Sitzung um.

## Entscheidung

1. **Ein Command-Paar für die Oberflächen-Einstellungen.** `getUiPreferences` /
   `setUiPreferences(preferences)` mit `{ shortcuts: { macos, windows }, textSize, recentCommands }`.
   `setUiPreferences` schreibt alles in einer Transaktion, sendet `uiPreferencesChanged` (mit den
   gespeicherten Werten) an alle Fenster und gibt zurück, was gespeichert ist; die Liste je Plattform
   ist nach Befehl sortiert, damit unveränderte Daten ihre Sync-Version behalten. Ein Paar statt je
   Wert ein Command, weil die Oberfläche die Werte ohnehin zusammen lädt und ein Ereignis genügt.
2. **Prüfung im Kern, typisierte Fehler.** `mail_model::UiPreferences::validate` prüft Schriftgröße
   80 bis 150, höchstens 5 zuletzt benutzte Befehle (je 1 bis 200 Zeichen, keine Steuerzeichen,
   keine Dubletten), je Plattform höchstens 100 geänderte Befehle (Kennung 1 bis 64 Zeichen aus
   ASCII-Buchstaben, Ziffern, `.`, `_`, `-`, `:`; keine doppelten Einträge), je Befehl höchstens 4
   Kürzel (Taste 1 bis 20 Zeichen ohne Steuerzeichen, kein Kürzel doppelt). Der Fehler ist
   `UiPreferencesError`; über die Brücke kommt er als `invalid_argument` mit dem Text als `detail`.
   Kein neuer Fehlercode: die Oberfläche prüft dieselben Grenzen vorher und zeigt den Fall
   „Doppelbelegung“ selbst; was trotzdem ankommt, ist ein Programmfehler.
   Welche Befehle es gibt und welche Kürzel das System beansprucht, weiß nur die Oberfläche
   (Aktionsverzeichnis); der Kern prüft Form und Grenzen, nicht die Bedeutung.
3. **Speicher: die Tabelle `setting`, keine Migration.** Schlüssel unter `global`:
   `ui.shortcuts.macos`, `ui.shortcuts.windows` (Liste `{ command, shortcuts: [{ key, shift?, mod? }] }`,
   `shift`/`mod` nur wenn gesetzt), `ui.text_size`, `ui.recent_commands`, `ui.layout.<slot>`.
   Unlesbare oder (etwa von einer neueren Version) nicht mehr zulässige Werte gelten als Standard
   und werden protokolliert, die App startet trotzdem.
4. **Sync: Kürzel und Schriftgröße ja, Verlauf und Layout nein.** `ui.shortcuts.macos`,
   `ui.shortcuts.windows` und `ui.text_size` stehen in `SYNCED_SETTINGS` (Art `setting`, je Schlüssel
   gewinnt die neuere Version). Ein Schlüssel je Plattform, damit eine Änderung auf dem Mac und eine
   auf Windows nicht um denselben Datensatz konkurrieren. `ui.recent_commands` und `ui.layout.*` sind
   Geräte-Einstellungen. Übernimmt ein Sync-Durchlauf Einstellungen, sendet der Kern neben
   `preferencesChanged` auch `uiPreferencesChanged`.
5. **Größengrenze der Art `setting` von 4 auf 16 KiB.** Alle Befehle des Verzeichnisses mit je zwei
   geänderten Kürzeln ergeben rund 5 KB, mehr als ein Datensatz bisher fassen durfte. Der Kern prüft
   jede Liste vor dem Speichern mit derselben Prüfung wie der Sync (`SyncContent::new`) und lehnt
   ab, was der Sync ablehnen würde (`TooLarge` mit Bytes und Grenze); ein gespeicherter Wert passt
   also immer in einen Datensatz. Noch ist keine Version veröffentlicht, ältere Geräte mit 4 KiB gibt
   es nicht.
6. **Layout je Fenster, Fenster aus der Shell.** `getWindowLayout()` / `setWindowLayout(layout)` mit
   `{ readingPane: "right" | "bottom" | "replace", density: "comfortable" | "compact",
   sidebarExpanded }`. Welches Fenster fragt, entnimmt die Shell dem aufrufenden `WebviewWindow`,
   nie einem Argument der Oberfläche. Gespeichert wird je Art von Fenster (`main`, `message`), nicht
   je Label: sonst wüchse die Tabelle mit jedem geöffneten Mail-Fenster. Die Schriftgröße gehört
   nicht zum Layout, sondern zu den Oberflächen-Einstellungen (sie gilt für alle Fenster).
7. **Mail im eigenen Fenster.** `openMessageWindow(accountId, messageId)` öffnet ein Fenster mit dem
   Label `message-<Konto-UUID ohne Bindestriche>-<Mail-ID>` und der Seite
   `index.html?window=message&accountId=<uuid>&messageId=<id>` (dieselbe Einstiegsdatei, die
   Oberfläche liest die Query). Gibt es das Fenster schon, wird es nach vorn geholt. Vorher prüft
   der Kern, dass die Mail existiert und zum Konto gehört (`not_found` sonst, kein leeres Fenster);
   der Betreff wird der Fenstertitel. Normale Titelleiste (kein Overlay), 900×760, mindestens
   560×420. Größe und Lage werden nicht gespeichert (nicht billig genug für diese Sitzung).
8. **Composer im eigenen Fenster.** `openComposerWindow(draftId)` öffnet je Entwurf ein Fenster
   (`composer-<Entwurf-UUID>`, Seite `index.html?window=composer&draftId=<uuid>`, 820×720), mehrere
   Entwürfe gleichzeitig, ein vorhandenes Fenster wird nach vorn geholt. Eine Kennung, die keine
   UUID ist, lehnt die Shell ab (`invalid_argument`), ein unbekannter Entwurf ist `not_found`; der
   Betreff wird der Titel. Das Layout aller Composer-Fenster liegt unter `ui.layout.composer`.
   Abgelegte Dateien merkt sich jedes Fenster (`windows::finish`), so kann der Composer im eigenen
   Fenster sie anhängen wie im Hauptfenster.
9. **Gleiche Schutzregeln für jedes Fenster.** Haupt- und Mail-Fenster entstehen über dieselben
   Funktionen (`windows::guard`, `windows::finish`): derselbe Navigations-Hook (App-URL ja,
   `about:srcdoc`/`about:blank` ja, `https://link.invalid/<Token>/<Index>` abbrechen und dem Kern
   melden, alles andere abbrechen), `window.open`/`target=_blank` abgelehnt, auf Windows der
   WebView2-Frame-Guard (`webview2-guard`), gemerkte abgelegte Dateien. Die CSP kommt aus
   `tauri.conf.json` und gilt für jede ausgelieferte Seite. Die Capability `default` nennt
   `["main", "message-*", "composer-*"]`: Mail- und Composer-Fenster haben genau die Rechte des
   Hauptfensters (Ereignisse hören, Fenster ziehen), keine zusätzlichen; ein Fenster mit anderem
   Label hat keine.
10. **Grenzen der Stufe 10 als Konstanten.** `MISSED_SEND_GRACE_MS`, `MAX_SCHEDULE_AHEAD_MS`,
    `MAX_INLINE_IMAGE_BYTES`, `MAX_SIGNATURE_IMAGES`, `MAX_SIGNATURE_IMAGE_BYTES` und
    `MAX_SIGNATURE_IMAGES_BYTES` gehen als Konstanten in die Bindings, damit die Oberfläche sie
    nicht abschreibt. Millisekunden über 2^32 (`MAX_SCHEDULE_AHEAD_MS`) als gewöhnliche Zahl
    (sicher unter 2^53, Regel aus 0004).
11. **Standard-Position des Zitats.** `ComposeSettings.defaultQuotePosition` (`QuotePosition`,
    `"above"` oder `"below"`) gilt für neue Antworten und Weiterleitungen; der Composer kann sie je
    Entwurf ändern, neue Mails ohne Zitat behalten den Typ-Standard. Standard ist `"above"`
    (Antwort über dem Zitat, also Zitat unten), genau das Verhalten vorher: `QuotePosition`
    beschreibt, wo die *Antwort* steht (`docs/` und `mail-model`). Gemeint war im Auftrag „Zitat
    unten“; soll stattdessen die Antwort unter dem Zitat Standard werden, ändert sich nur
    `QuotePosition::default()` bzw. der Standard in `ComposeSettings`. Gespeichert unter
    `compose.default_quote_position` und wie die Sendeverzögerung synchronisiert. Das Feld ist beim
    Schreiben optional (`#[serde(default)]`), damit eine Oberfläche, die es noch nicht kennt, die
    Sendeverzögerung weiter speichern kann; sie setzt es dann auf den Standard zurück.

## Folgen

- Die Oberfläche ersetzt `localFallback.ts` durch die Commands und hört auf `uiPreferencesChanged`;
  sie liest die Query (`window=message|composer`) und zeigt dann nur die Mail bzw. den Composer.
  Die Schriftgröße wandert aus dem Fenster-Layout in die Oberflächen-Einstellungen.
- Die Bindings sind noch nicht neu erzeugt (die Oberflächen-Sitzung arbeitet parallel in
  `app/src/`); bis dahin scheitert nur `bindings_are_fresh`.
- Der Desktop-Test kann das zweite Fenster erst prüfen, wenn die Oberfläche die Route hat.

## Geprüft

- `mail-model`: Drahtformat, Grenzen aller Felder, Dubletten, Layout-Slots.
- `mail-engine`: Standardwerte, Speichern mit Ereignis und Neustart, kompaktes Format je
  Plattform, Ablehnung ohne Änderung, alle Befehle mit zwei Kürzeln passen in einen
  Sync-Datensatz und zu große Listen werden abgelehnt, unlesbare Werte, Layout je Slot und nicht
  synchronisiert, Erfassungstest des Sync mit den neuen Schlüsseln.
- Zwei Geräte gegen den lokalen Testserver (`tests/settings_sync.rs`,
  `shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay`): Kürzel und
  Schriftgröße kommen auf dem zweiten Gerät an und werden dort mit `uiPreferencesChanged`
  gemeldet, Verlauf und Layout bleiben auf dem ersten.
- `mail-engine` (Composer): neue Antworten und Weiterleitungen übernehmen die gespeicherte
  Position, neue Mails nicht, der Wert übersteht einen Neustart.
- `mail-app` (Tauri-Mock mit echter Konfiguration): Commands und Ereignis, Layout nach
  aufrufendem Fenster, Mail-Fenster einmal je Mail mit Query, `not_found` für fremde oder
  unbekannte Mails, Composer-Fenster einmal je Entwurf, `invalid_argument`/`not_found` für
  falsche oder unbekannte Entwürfe, Rechte wie das Hauptfenster, keine Rechte für fremde Labels,
  Navigationsentscheidung. Für Windows übersetzt `cargo xwin clippy` die Shell mit dem
  Frame-Guard ohne Warnung.
- Nicht geprüft: das echte WebView2 unter Windows (nur die Übersetzung für Windows), ein Klick in
  der Mail des zweiten Fensters in der echten App und ein Composer-Fenster mit echtem Entwurf in
  der App (beides braucht die Route der Oberfläche).
