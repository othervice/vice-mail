# 0027 — `.eml`-Dateien öffnen, Mails nach ID, `aiSettingsChanged`

Stand 23.11.2026. Schließt die letzte offene Aufgabe der Stufe 9 (Teil der Aufgabe
„Standard-Mail-App“: `.eml`-Dateien öffnen, offen laut 0020) und zwei kleine Lücken der Stufe 13
(Quellen der KI benennen und öffnen, KI-Einstellungen in allen Fenstern aktuell halten). Dazu der
Klick auf eine Systemmeldung, der bisher nur den Ordner zeigte.

## Entscheidung

### `.eml`-Dateien

1. **Anmeldung beim System.** `bundle.fileAssociations` in `tauri.conf.json` (nur das, der
   Produktname bleibt, wo er ist): Endung `eml`, macOS-Typ `com.apple.mail.email`, MIME
   `message/rfc822`, Rolle „Viewer“, auf macOS Rang **„Alternate“**: Die App steht unter „Öffnen
   mit“, nimmt Mail aber nicht die Rolle des Standards weg. Unter Windows trägt der NSIS-Installer
   von Tauri die Zuordnung ein (`"<App>.exe" "%1"`); ob sie Standard wird, entscheidet der Nutzer.
2. **Wege in die App.** macOS liefert eine Datei als `file://`-URL in `RunEvent::Opened` (dieselbe
   Stelle wie `mailto:`), Windows als Startargument oder, wenn die App schon läuft, über
   `tauri-plugin-single-instance` mit dem Arbeitsverzeichnis des zweiten Starts. Argumente, die
   mit `-` beginnen, `mailto:`-Links und URLs werden übersprungen; relative Pfade gelten ab dem
   Arbeitsverzeichnis (`app/src-tauri/src/eml_files.rs`). In der App: „.eml-Datei öffnen …“ in
   der Befehlspalette; der Kern zeigt den Öffnen-Dialog des Systems (`rfd`, nur `.eml`). Die
   Oberfläche sieht nie einen Pfad.
3. **Wo die Datei erscheint: in einem Mail-Fenster** aus 0025, nicht im Hauptfenster. Label
   `message-file-<ID>`, Seite `index.html?window=eml&fileId=<ID>`. Weil das Label mit `message-`
   beginnt, gelten Capability (`message-*`), Navigations-Hook, Ablehnung neuer Fenster,
   WebView2-Frame-Guard und das Layout der Mail-Fenster unverändert; kein Label einer
   gespeicherten Mail kann so aussehen (dort folgt eine UUID ohne Bindestriche und eine Zahl).
   Jedes Öffnen bekommt ein eigenes Fenster (dieselbe Datei zweimal: zwei Fenster). Gründe: Die
   Datei gehört zu keinem Konto und keinem Ordner, im Hauptfenster gäbe es keine Liste, in der sie
   stehen könnte; ein eigenes Fenster ist das, was Mail und Outlook beim Doppelklick tun.
4. **Lesen und Grenzen im Kern** (`mail-engine/src/reading/eml.rs`): höchstens
   `MAX_EML_FILE_BYTES` = 64 MiB (auch eine 45-MB-Datei in Base64 passt), geprüft über die Größe
   vor dem Lesen und beim Lesen (`take(limit + 1)`). Eine Mail ist, was der Parser mit
   Kopfzeilen lesen kann und mindestens eine Adresse, einen Betreff, ein Datum oder eine
   Message-ID hat; leere Dateien, Text ohne Kopfzeilen und Binärdaten sind keine.
   **Typisierte Fehler** (neu, `CoreError::Eml`): `eml_too_large`, `eml_invalid`,
   `eml_unreadable` (fehlt, Ordner, keine Rechte), mit Texten auf Englisch und Deutsch.
5. **Nichts wird importiert.** Die Bytes liegen im Speicher unter einer Zufalls-ID (128 Bit wie
   die Render-Tokens), höchstens `MAX_OPEN_EML_FILES` = 16 Dateien, die älteste fällt heraus. Kein
   Konto, kein Ordner, kein Suchindex, keine Kontakte, keine Datenbankzeile, kein Body-Speicher.
   Schließt sich das Fenster (`WindowEvent::Destroyed`), vergisst der Kern Datei und
   Darstellungen; danach antwortet er `not_found`.
6. **Dieselbe Lese-Pipeline.** `Reading::render_body` ist jetzt der gemeinsame Teil von
   gespeicherter Mail und Datei: derselbe Parser (`Content::complete`), dieselbe Bereinigung mit
   Pflicht-CSP, Link-Platzhalter unter einem neuen Render-Token (die Register-Einträge kennen jetzt
   ihren Besitzer, `RenderOwner::Message` oder `RenderOwner::File`), `describeLink`/`openLink`
   unverändert, Anzeige im Sandbox-Frame der Oberfläche. Externe Inhalte bleiben gesperrt;
   „einmal laden“ lädt sie über den Kern wie bei gespeicherter Mail, nur für diese Darstellung.
   „Für diesen Absender immer“ gibt es nicht (keine Absenderliste ohne Konto). Anhänge dekodiert
   der Kern aus der Datei; Speichern über den Dialog des Systems, Öffnen als schreibgeschützte
   Kopie, ausführbare Typen erst nach Bestätigung. Bcc wird gezeigt, wenn die Datei es enthält;
   ohne lesbares `Date` ist das Datum `0` und die Oberfläche zeigt keines.
7. **Fehler beim Start.** Eine Datei vom System, die keine Mail ist, bekommt trotzdem ihr Fenster
   und der Kern merkt sich den Fehler; das Fenster zeigt ihn (Titel und Hinweis des Codes). Beim
   Start gäbe es sonst keinen Ort für die Meldung. Eine Datei aus dem Dialog, die scheitert,
   öffnet kein Fenster; der Fehler kommt als Hinweis im aufrufenden Fenster.
8. **Commands:** `openEmlFile` (Dialog, Kern, Fenster; `cancelled` oder `opened`),
   `getEmlMessage(fileId, format)`, `loadEmlRemoteContent(fileId)`,
   `saveEmlAttachment(fileId, index)`, `openEmlAttachment(fileId, index, confirmedExecutable)`.
   Eine `fileId`, die nicht aus 32 Hex-Ziffern besteht, lehnt die Shell mit `invalid_argument` ab.
   Konstante `MAX_EML_FILE_BYTES` in den Bindings.

### Mails nach ID (Stufe 13)

9. `getMessagesByIds(messageIds) → MessageListItem[]`: in der gefragten Reihenfolge, jede ID
   einmal, jede Mail an ihrem ersten Ort; unbekannte IDs fehlen einfach. Höchstens
   `MAX_MESSAGES_BY_IDS` = 200 IDs (Konstante in den Bindings), sonst `invalid_argument`. Kein
   neuer Fehlercode. Der Speicher nutzt dieselbe Zeile wie die Listen (`lists::entry`).
10. **Quellen der KI.** Das Panel benennt die Quellen einer Antwort („Grace Hopper, 20. Nov.
    2026“) über diesen Command, unbekannte bleiben „Mail n“. Ein Klick zeigt die Mail im
    Hauptfenster, wenn es sie zeigt (die ausgewählte Mail oder eine des offenen Threads, der sie
    dann auswählt); sonst öffnet sie sich in einem eigenen Fenster (`openMessageWindow`). Eine
    Mail, die es nicht mehr gibt, ist ein Hinweis.

### `aiSettingsChanged` (Stufe 13)

11. Der Kern sendet `aiSettingsChanged { availability }` nach jeder gespeicherten Änderung der
    KI-Einstellungen (Schalter, Profil speichern oder löschen, Funktion, Cloud-Freigabe,
    Zustimmung, offene Antworten), mit dem, was danach nutzbar ist. Abgelehnte Änderungen und
    Lesen senden nichts. Die Oberfläche übernimmt die Verfügbarkeit und lädt die Einstellungen
    neu, in jedem Fenster. Dass ein Konto kommt oder geht, ändert nur die Kontoliste der
    Einstellungen; dafür lädt die Oberfläche sie bei `accountsChanged` neu (kein zweites
    Ereignis).

### Klick auf eine Systemmeldung (Stufe 9)

12. Bisher zeigte der Klick nur den Ordner der Mail. Jetzt öffnet er auch die Mail: im offenen
    Thread, wenn das Hauptfenster sie dort schon zeigt, sonst in einem eigenen Fenster. Die Liste
    kann eine Mail nicht nach ID auswählen (sie ist virtualisiert und seitenweise geladen, die Mail
    steht vielleicht auf keiner geladenen Seite); das bleibt offen. Nur das Hauptfenster antwortet
    auf das Ereignis, sonst öffnete jedes Fenster dasselbe noch einmal.

## Folgen

- `docs/ARCHITECTURE.md`: Sicherheitsgrenzen (Shell, Dateien, Netz), KI-Verdrahtung.
- Stufe 9: Die Aufgabe „Standard-Mail-App“ ist im Code vollständig; offen bleibt die Prüfung von
  Hand auf beiden Systemen (Doppelklick im Finder und im Explorer, „Öffnen mit“, zweiter Start
  unter Windows).

## Geprüft

- `mail-model`: `eml::tests` (IDs, Fehlercodes, Drahtformat), Fehlercodes im Drahtformat.
- `mail-engine`: `reading::eml::tests::opened_files_go_through_the_reading_pipeline_and_never_into_the_store`,
  `attachments_of_a_file_are_saved_exactly_and_opened_only_after_confirmation`,
  `files_that_cannot_be_shown_are_refused_with_typed_errors` (leer, Text, binär, fehlt, Ordner,
  ein Byte zu groß, genau die Grenze), `files_from_the_system_keep_their_error_for_their_window`,
  `the_register_is_bounded_and_closing_forgets_the_file_and_its_links`, `names_and_the_mail_check`;
  `ai::tests::every_settings_change_is_announced_with_what_is_usable`,
  `sources_are_named_by_id_in_the_order_asked_and_bounded`.
- `mail-app` (Tauri-Mock mit echter Konfiguration):
  `tests::eml_files_open_in_message_windows_and_messages_come_by_id` (Startargumente, Fenster mit
  Query und Rechten der Mail-Fenster, Ansicht, gemerkter Fehler, falsche ID, Schließen vergisst
  die Datei, Mails nach ID mit Grenze), `eml_files::tests::*` (Argumente, `file://`-URLs),
  `windows::tests::eml_windows_are_message_windows_with_their_own_query`. Für Windows übersetzt
  `cargo xwin clippy` die Shell ohne Warnung.
- Oberfläche (Vitest): `eml.test.tsx`, `open-by-id.test.tsx`, angepasst `ai-panel.test.tsx`.
- **Ungeprüft:** Doppelklick auf eine `.eml` im Finder und im Explorer, „Öffnen mit“, der zweite
  Start unter Windows mit einer Datei, das Schließen eines echten Fensters (der Mock-Runtime
  sendet keine Fensterereignisse; geprüft ist der Hook, den das Fenster bekommt), ob macOS die
  Zuordnung ohne neue Anmeldung bei Launch Services übernimmt (`lsregister`), der Desktop-Test.

## Rückweg

- Datei im Hauptfenster statt im eigenen Fenster: Route `eml` im Hauptfenster als Ansicht,
  Commands bleiben.
- Datei importieren („in Ordner ablegen“): ein `APPEND` über die Outbox (Schreibzugriff nötig),
  nicht Teil dieser Entscheidung.
