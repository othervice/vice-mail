# 0082 — Ziehen in der App ohne den Drop-Handler der Shell; Dateien kommen als Bytes

- **Datum:** 26.11.2026
- **Anlass:** Stufe 23, Punkt 1. Rückmeldung zu 0.1.3 auf beiden Geräten: Unter Windows lässt sich
  gar nichts ziehen (weder Konten noch Mails). Auf dem Mac lässt sich ein Konto aufnehmen, bleibt
  aber nicht dort, wo man es loslässt. Gewünscht wie bei Thunderbird: Konto nehmen, ziehen, eine
  Linie zeigt die Einfügestelle, loslassen, neue Reihenfolge; auch in der schmalen Leiste.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 23, Punkt 1). **Ändert
  0018** (Abschnitt „Anhänge und Drag-and-drop“: Die Seite nennt keine Pfade mehr, `DropRegistry`
  entfällt) und ergänzt 0078 (Konten ziehen, jetzt auch in der Leiste) und 0081 (Mails auf Ordner
  ziehen).
- **Code:** `app/src-tauri/src/windows.rs` (`guard`: `disable_drag_drop_handler()`; `finish` ohne
  Drop-Aufzeichnung), `tauri.conf.json` (`dragDropEnabled: false`), `drops.rs` entfernt;
  Commands `beginFileUpload`, `appendFileUpload`, `discardFileUpload`, `addUploadedAttachments`,
  `addUploadedInlineImages`, `addUploadedSignatureImages` statt `addDropped…`; e2e-Command
  `e2e_drop_files` entfernt. Kern `crates/mail-engine/src/compose/uploads.rs` (`Uploads`,
  `SourceFile`), `AppCore::{begin,append,discard}_file_upload`, `append_file_upload_base64`,
  `add_uploaded_*`; Modell `UploadId`, `MAX_UPLOAD_BYTES`, `MAX_UPLOAD_CHUNK_BYTES`. Oberfläche
  `components/fileDrop.ts` (`installFileDrop`, `useWindowFileDrop`), `services/ipc.ts`
  (`uploadFiles`, `attachFiles`, `addInlineImageFiles`, `addSignatureImageFiles`),
  `composer/Composer.tsx`, `snippets/SignatureSettings.tsx`; Konten `accounts/accountDrag.ts`
  (`accountPlaceAt`, `useAccountDropZone`, `useAccountMoveByDrag`), `stores/drag.ts`
  (`accountPlace`, `isOwnDrag`, `installDragReset`), `app/SidebarColumn.tsx`, `app/IconRail.tsx`
  (`AccountDot`), `mailboxes/useDropTarget.ts`, `mailboxes/useDragAutoScroll.ts`; Text
  `accountEdit.railDragHint`.
- **Tests:** Kern `compose::uploads::tests` (5), `compose::stage23_tests` (3); Shell
  `tests::no_command_takes_a_file_path_from_the_page`,
  `tests::no_window_has_the_shells_drag_and_drop_handler`; Vitest `account-drag.test.tsx`,
  `file-drop.test.tsx`, ergänzt `drop-anywhere.test.tsx`, angepasst `composer.test.tsx`,
  `composer-complete.test.tsx`, `snippets-settings.test.tsx`; Desktop `stage23-drag.e2e.mjs`,
  angepasst `contacts.e2e.mjs`.

## Ursache

Tauri baut jedes Fenster mit einem eigenen Drag-and-drop-Handler, der Dateien vom Finder bzw.
Explorer als `DragDrop`-Ereignis mit Pfaden meldet. `tauri-runtime-wry` 2.12.5 (`lib.rs`,
`with_drag_drop_handler`) gibt dabei für jedes Ereignis `true` („erledigt“) zurück.

- **macOS:** wry 0.57.3 (`wkwebview/drag_drop.rs`) reicht `draggingEntered`, `draggingUpdated`
  und `performDragOperation` nur dann an WKWebView weiter, wenn der Handler `false` liefert. Die
  Seite bekam deshalb `dragstart` (das Ziehen beginnt in WebKit selbst), aber nie `dragover` und
  nie `drop`: keine Linie, kein Ablegen. Genau das sah der Mensch.
- **Windows:** wry ersetzt mit dem Handler das Drop-Ziel von WebView2 und schaltet
  `AllowExternalDrop` ab (`webview2/mod.rs`); damit geht HTML-Drag-and-drop in der Seite gar nicht.

Die Desktop-Tests lösen die Zieh-Ereignisse in der Seite aus und gehen am Betriebssystem vorbei;
deshalb fiel es nicht auf. Betroffen war jedes Ziehen in der App: Konten (0078) und Mails auf
Ordner (Stufe 16, 0081).

## Entscheidung

**1. Kein Drop-Handler der Shell, für kein Fenster.** `windows::guard`, durch das jedes Fenster
gebaut wird (Haupt-, Mail-, Entwurfs-, Quelltext-, Popup-Fenster), ruft
`disable_drag_drop_handler()`; das Hauptfenster in `tauri.conf.json` sagt zusätzlich
`dragDropEnabled: false`. Ein Test prüft beides. Damit bekommt WebKit bzw. WebView2 alle
Zieh-Ereignisse selbst, und die Seite ihr `dragover` und `drop`.

**2. Dateien von außen kommen als Bytes.** Ohne den Handler gibt es keine Pfade mehr: Eine Datei,
die auf ein Fenster gezogen wird, erreicht die Seite als HTML-`File` (Name, Größe, Inhalt). Die
Seite gibt den Inhalt **in Stücken** an den Kern: `beginFileUpload(name, size)` liefert eine
Upload-ID (UUID des Kerns), `appendFileUpload(id, offset, stück)` schreibt je höchstens 2 MiB
(`MAX_UPLOAD_CHUNK_BYTES`, über die Brücke als Base64, weil JSON keine Bytes kennt; ein
`Uint8Array` würde Tauri als Objekt mit einer Zahl je Byte schicken), danach nimmt
`addUploadedAttachments`, `addUploadedInlineImages` bzw. `addUploadedSignatureImages` die
vollständigen Uploads **wie Dateien aus dem Systemdialog**: dieselben Grenzen (100 Anhänge, 100 MiB
zusammen, 8 MiB je Inline-Bild, Bildprüfung an den Bytes, Signaturbilder 64/128 KiB), derselbe
MIME-Typ aus dem Namen. So hält weder die Seite noch der Kern eine große Datei auf einmal im
Speicher (die Seite liest mit `File.slice` Stück für Stück), und eine 100-MiB-Datei wird nicht zu
einem 133-MB-String. Bis zur Übernahme liegen die Stücke in `compose/uploads/<Upload-ID>`; der Pfad
entsteht nur aus der ID des Kerns, der Name wird bereinigt und nur als Dateiname des Anhangs
benutzt. Übernommene, verworfene und 10 Minuten untätige Uploads werden entfernt, ein Start räumt
alle weg. Höchstens 100 Uploads gleichzeitig und zusammen höchstens 200 MiB angekündigt; ein Upload
über 100 MiB wird schon beim Ankündigen abgelehnt (`attachment_too_large`). Scheitert ein Stück,
verwirft die Seite alle begonnenen Uploads dieses Ablegens.

Der Empfänger ist wie bisher „das Fenster, solange der Composer bzw. der Signatur-Editor offen ist“.
Die Seite fängt Datei-Ziehen einmal am Fenster ab (Capture-Phase, `components/fileDrop.ts`), vor
jedem Element: Der Rich-Text-Editor würde sonst Name oder Adresse der Datei als Text einfügen. Ohne
Empfänger wird ein Ablegen abgewiesen (`dropEffect` `none`) und trotzdem behandelt, damit die
Web-Ansicht die Datei nicht öffnet. Ziehen, das in der Seite begann (ein Konto, Zeilen, ein Bild im
Editor), zählt nie als Datei, auch wenn die Engine dafür „Files“ meldet. Ordner haben keine Bytes,
die die Seite lesen könnte; sie werden wie ein Pfad, der keine Datei ist, mit `invalid_argument`
gemeldet.

**3. Mindestens so sicher wie 0018.** 0018 ließ die Seite Pfade nennen und nahm nur solche an, die
das Betriebssystem kurz vorher als Drop gemeldet hatte. Jetzt nennt die Seite **nie** einen Pfad,
und kein Command nimmt einen Dateipfad von der Seite (ein Test prüft alle Commands der Bindings;
`createMailbox` nimmt einen IMAP-Ordnerpfad). Die Seite kann nur Bytes übergeben, die sie schon hat:
Ein `File` bekommt sie nur, wenn der Mensch die Datei auf das Fenster zieht; lesen kann sie das
Dateisystem nicht (keine Dateisystem-Capability, CSP ohne fremde Quellen). Eine kompromittierte
Seite könnte also nichts anhängen, was sie nicht ohnehin als Text in den Entwurf schreiben könnte;
`~/.ssh/id_ed25519` erreicht sie nicht. Nach 0018 hätte sie dagegen 2 Minuten lang jede gerade
gezogene Datei per Pfad nennen können, auch eine, die nicht für sie gedacht war, und die Pfade
selbst (mit Benutzername und Ordnern) lagen in der Seite. Die Grenzen gegen eine Seite, die den Kern
mit Bytes füllen will, stehen in Punkt 2.

**4. Konten umsortieren wie in Thunderbird, in der Spalte und in der schmalen Leiste.** Die Liste
der Konten ist ein Ablageziel als Ganzes (`useAccountDropZone`): Für die Zeigerhöhe gilt „vor dem
ersten Konto, dessen Mitte unter dem Zeiger liegt, sonst hinter dem letzten“ (`accountPlaceAt`).
Eine Linie in `accent-fill`, 4 px, zeigt die Lücke; das gezogene Konto bleibt gedimmt an seinem
Platz (50 % Deckkraft). Nur wo sich die Reihenfolge ändert, bietet die Seite das Ablegen an
(`preventDefault`, `dropEffect` `move`); über dem gezogenen Konto selbst und direkt daneben zeigt
sich keine Linie. Verlässt der Zeiger jede Liste von Konten, verschwindet die Linie (ein
`dragover`-Hörer am Fenster, solange ein Konto gezogen wird, weil WebKit `dragleave` ohne
`relatedTarget` meldet); Ablegen dort tut nichts. Loslassen speichert die Reihenfolge im Kern
(`reorderAccounts` aus 0078), sie gilt also auch nach einem Neustart und auf anderen Geräten.
Escape bricht ab: Das System beendet sein Ziehen selbst, die Seite vergisst es zusätzlich. In der
Spalte scrollt die Liste an den Rändern wie in 0081; die Konten der Leiste stehen in ihrer unteren
Gruppe, die nicht scrollt. Tastatur: In der Spalte wie bisher, in der Leiste neu Alt+Pfeil hoch und
Alt+Pfeil runter auf dem Punkt (der Fokus bleibt, eine Statuszeile sagt den neuen Platz).

**5. Eigene Zieh-Vorgänge erkennt der Store, nicht `types`.** WebKit kann eigene Datentypen
(`application/x-mail-…`) bis zum `drop` verbergen; nur `types` ist während `dragover` lesbar, und
auch das nicht für jeden Typ verlässlich. Ob ein Ziehen Zeilen oder ein Konto trägt, entscheidet
deshalb der Zieh-Store der Seite (beim `dragstart` gesetzt); Dateien von außen zählen nie
(`isOwnDrag`). Damit ein verlorenes `dragend` (Quelle beim Ziehen aus der Liste verschwunden) keinen
alten Zustand hinterlässt, beginnt jedes neue Ziehen und jeder Mausdruck mit leerem Store
(`installDragReset`). Mails auf Ordner (0081) bleiben sonst unverändert: Verschieben, Wahltaste
(macOS) bzw. Strg (Windows) kopiert.

**6. Prüfung mit echtem Ziehen über das Betriebssystem: auf diesem Mac nicht möglich.** Versucht
wurde ein kleines Swift-Programm, das per CGEvent Maus-Ereignisse erzeugt (Maus runter, mehrere
`leftMouseDragged`, Maus hoch), als launchd-Job in `gui/501` wie `scripts/e2e-gui.sh` und aus der
tmux-Sitzung, gegen eine eigene AppKit-Test-App mit Zieh-Quelle und -Ziel. Ergebnis:
`CGPreflightPostEventAccess()` = false, `AXIsProcessTrusted()` = false; Ereignisse über
`kCGHIDEventTap`, `kCGSessionEventTap` und `CGEventPostToPid` kamen nicht an (die Test-App
protokollierte kein einziges Maus-Ereignis). macOS verlangt dafür die Bedienungshilfen-Freigabe
(TCC, „Ereignisse senden“) für den sendenden Prozess; die Einstellungen wurden nicht verändert.
Deshalb bleiben die Desktop-Tests bei Zieh-Ereignissen in der Seite (jetzt auch mit echten
`File`-Objekten durch die echte IPC-Brücke), und das Ziehen mit der Maus steht auf den
Prüflisten: macOS beim Menschen, Windows in `docs/testing/windows-checklist.md` (23.1 bis 23.3).

## Verworfen

- **Den Handler nur unter Windows abschalten.** Unter macOS blockiert er `dragover` und `drop`
  genauso; die Rückmeldung vom Mac zeigt es.
- **Den Handler behalten und `false` zurückgeben, wenn keine Datei gezogen wird.** Das geht nur in
  `tauri-runtime-wry` selbst (der Handler ist dort fest verdrahtet), also mit einem eigenen Fork;
  unter Windows ersetzt der Handler trotzdem das Drop-Ziel.
- **Ein Command mit dem ganzen Inhalt als Base64.** Eine 100-MiB-Datei wäre ein 133-MB-String in
  der Seite, im JSON und im Kern.
- **Roher IPC-Body (`tauri::ipc::Request`, `Uint8Array` als Body).** Effizienter, aber ohne
  tauri-specta-Typ (das Argument hat kein `specta::Type`), also ein ungetypter Command neben den
  Bindings, und ohne Stücke hält die Seite die ganze Datei auf einmal.
- **Pfade weiter nutzen (`webkitRelativePath`, `File.path`).** Die Web-Ansichten geben keinen Pfad
  heraus, und eine Seite, die keine Pfade kennt, ist das Ziel von Punkt 3.

## Nachweise

Siehe den Abschnitt „Ergebnis“ der Stufendatei und den Bericht der Sitzung (Tests, Bildschirmfotos
der Linie in Spalte und Leiste, hell und dunkel).
