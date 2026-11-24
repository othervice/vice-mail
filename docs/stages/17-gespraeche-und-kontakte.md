# Stufe 17 — Gespräche wie in Thunderbird, Kontakte mit Bildern, eigene Ansichten

**Ziel:** Gespräche lassen sich in der Liste aufklappen, rechts steht immer genau eine Mail.
Kontakte sind eine eigene, schön gestaltete Ansicht mit eigenen Bildern, guter Suche und
„Als Spam markieren“. Die Ansichten in der linken Spalte stellt jeder selbst zusammen.

**Anlass:** Wunsch des Menschen vom 24.11.2026 (mit Bildschirmfoto von Thunderbird), vor dem ersten
öffentlichen Release umzusetzen.

**Voraussetzung:** Stufen 07 (Gespräche), 08 (Spalte, Tastatur), 10 (Kontakte), 16 und 0039.

## Aufgaben

### A — Gespräche in der Liste aufklappen

- [x] Eine Zeile je Gespräch wie bisher (0039), mit dem Absender einmal und der Zahl der Antworten
  („3 Antworten“) samt Pfeil zum Aufklappen. Aufgeklappt stehen alle Mails des Gesprächs
  eingerückt darunter, mit einer feinen Linie links, älteste zuerst, eigene gesendete Mails
  eingeschlossen, auch aus anderen Ordnern desselben Kontos.
- [x] Rechts im Lesebereich steht **nur die gewählte Mail**, nicht mehr das ganze Gespräch
  untereinander. Ein Klick auf die Gesprächszeile zeigt die neueste (bzw. die älteste ungelesene)
  Mail des Gesprächs. Die Ansicht als ganzes Gespräch rechts entfällt; Entscheidung festhalten.
- [x] Tastatur: Pfeil rechts klappt auf, Pfeil links klappt zu bzw. springt zur Gesprächszeile,
  Pfeil hoch/runter läuft durch sichtbare Zeilen; Bildschirmleser hören Ebene, Position und
  „aufgeklappt/zugeklappt“. Aktionen auf der Gesprächszeile wirken wie bisher auf das Gespräch,
  auf einer Mailzeile nur auf diese Mail.
- [x] Virtualisierte Liste bleibt flüssig (Leistungsziele aus `docs/SPEC.md`), aufgeklappte
  Gespräche bleiben beim Nachladen und Sortieren stabil.

### B — Kontakte

- [ ] Jede Adresse aus empfangenen und gesendeten Mails (Von, An, Cc, Antwort an) wird Kontakt,
  auch rückwirkend für vorhandene Mails; Listen- und Systemadressen (no-reply, Mailer-Daemon)
  bleiben erkennbar und lassen sich ausblenden.
- [ ] Eigene Bilder: Bild aus einer Datei wählen (Ziehen oder Dateiauswahl), quadratisch
  zuschneiden, verkleinert lokal speichern; Entfernen. Kein Laden von Bildern aus dem Netz
  (kein Gravatar, kein BIMI). Das Bild erscheint überall, wo heute Initialen stehen (Liste,
  Lesebereich, Composer, Kontakte); ohne Bild bleiben die Initialen.
- [ ] Die Bilder werden wie Bilder aus Mails im abgeschotteten Hilfsprozess dekodiert und neu
  kodiert (0046), mit Größengrenze.
- [ ] Kontaktseite als eigene Ansicht: Liste mit Bild, Name, Adresse, letzter Mail; schnelle
  Suche über Name, Adresse und Domain; Detailbereich mit allen Adressen, Bild, Notiz, letzten Mails
  mit diesem Kontakt, „Mail schreiben“, „Alle Mails anzeigen“, „Als Spam markieren“.
- [ ] „Als Spam markieren“ sperrt die Adresse (vorhandene Sperrliste aus Stufe 16) und bietet an,
  vorhandene Mails in den Spam-Ordner zu verschieben (über die Outbox, nur bei Konten mit
  Schreibzugriff). Rückgängig machbar.
- [ ] Kontakte, Bilder (verkleinert, mit Größengrenze) und Spam-Markierungen laufen über den
  Einstellungs-Sync, wenn er an ist.

### C — Ansichten in der Spalte bearbeiten

- [x] Neue Ansicht „Kontakte“ in der Spalte und in der Befehlspalette.
- [x] „Ansichten bearbeiten …“: Ansichten ein- und ausblenden und sortieren (Später, Aufgaben,
  Leseliste, Markiert, Angeheftet, Angeheftete Anhänge, Gespeicherte Suchen, Kontakte, …); per
  Maus und Tastatur; Zurücksetzen auf die Standardauswahl. Gespeichert als UI-Einstellung im Kern,
  mit dem Einstellungs-Sync.

## Abnahme

- [x] Gespräche lassen sich in der Liste mit Maus und Tastatur auf- und zuklappen; rechts steht
  immer genau eine Mail; Desktop-Test und axe ohne schwere Befunde.
- [ ] Ein eigenes Bild erscheint in Liste, Lesebereich, Composer und Kontakten; nach einem
  Neustart noch da; ein manipuliertes Bild wird abgelehnt, ohne die App zu stören.
- [ ] Kontaktsuche findet nach Name, Adresse und Domain bei 10.000 Kontakten unter 100 ms.
- [ ] „Als Spam markieren“ sperrt die Adresse, neue Mails von ihr landen nicht im Posteingang;
  Rückgängig hebt das auf.
- [x] Ansichten lassen sich ein-, ausblenden und sortieren; die Auswahl übersteht einen Neustart.
- [ ] Bildschirmfotos in `docs/images/` neu, wo sich etwas sichtbar geändert hat.
- [ ] `scripts/check.sh` ist grün, Desktop-Tests grün.

## Nicht in dieser Stufe

- Kontakte aus dem Netz (CardDAV, Google, Microsoft): nach Version 1 (0032).
- Bilder aus dem Netz (Gravatar, BIMI, Favicons).

## Ergebnis

### A — Gespräche in der Liste aufklappen

**Stand vom 24.11.2026, 15:10** (Block A, eigene Sitzung). Commits: 5a19494 (Store- und
Engine-Tests der Kinder), f56fc3a (Oberfläche: Baum, Kindzeilen, eine Mail rechts, Aktionen),
e8732b6 (Desktop-Specs), 69f8aa1 (Entscheidung 0049, Nachtrag in 0039, `docs/DESIGN.md` Abschnitt
„Gespräche aufklappen (Stufe 17, Entscheidung 0049)“, `docs/ARCHITECTURE.md`), 4ecccba (eine per ID
verlangte Mail klappt ihr Gespräch auf), a28215d (schmale Kindzeilen, Spec auf Englisch), d73c421
und 7edb9eb (weniger Arbeit je Nachladen in Listen einzelner Mails; Tastatur-Spec nimmt den Baum),
0bbb77d (Tastatur-Spec zählt die Liste nicht zu den Bäumen der Spalte).
Kein neuer Command, keine Migration, Bindings unverändert.

**Umgesetzt**
- Die Liste der Gespräche ist ein mehrfach auswählbarer `tree` (sonst bleibt die `listbox`). Die
  Gesprächszeile nennt „n Antworten“ mit Pfeil (Pille `badge`, aufgeklappt `accent-soft`/`accent`);
  ein Klick darauf klappt auf und wählt nichts. Aufgeklappt folgen alle Mails des Threads im Konto,
  älteste zuerst, eine Ebene tiefer, einzeilig (36/28 px), mit Mini-Avatar auf einer feinen
  gepunkteten Linie unter der Avatar-Spalte; Mails aus anderen Ordnern nennen ihn („Sent“,
  „Inbox“). Geladen beim Aufklappen über `getThread(Mail der Zeile, Ordner der Zeile)`; Lade-,
  Fehler- (Eingabe versucht erneut) und Kürzungszeile; die Liste bleibt bedienbar.
- Rechts genau eine Mail: die Gesprächszeile öffnet die älteste ungelesene, sonst die neueste Mail
  des Gesprächs und behält sie, solange die Zeile gewählt ist; eine Kindzeile ihre Mail. Die
  gestapelte Ansicht ist entfernt; Titel, KI-Chips, Antwortzeile und Schnellantwort bleiben, eine
  KI-Quelle aus dem Gespräch klappt es auf und wählt ihre Zeile (ebenso eine per ID verlangte Mail,
  z. B. aus einer Benachrichtigung). „Beim Öffnen als gelesen markieren“ trifft die gezeigte Mail.
- Tastatur und Bildschirmleser: → klappt auf bzw. springt hinein, ← klappt zu bzw. springt zur
  Gesprächszeile, ↑/↓ über alle sichtbaren Zeilen, Umschalt erweitert über Gesprächs- und
  Kindzeilen; `aria-level`, `aria-posinset`, `aria-setsize`, `aria-expanded` (auch an Bündeln),
  Namen aus dem sichtbaren Text („Ada Lovelace, in Sent, Gern, …“).
- Aktionen: Gesprächszeile wie bisher auf das Gespräch (`conversationTargets`), Kindzeile nur auf
  ihre Mail mit den Aktionen ihres Ordners (Schnellaktionen, Kürzel, Palette, Auswahlleiste, Ziehen,
  Symbolleiste), gemischte Auswahl in einem Aufruf (`Exactness`).
- Stabil: Die Identität einer Gesprächszeile ist ihr Thread; aufgeklappt, gewählt und rechts offen
  bleibt sie beim Nachladen, Umsortieren und wenn eine neue Antwort die Zeile übernimmt.

**Nachgewiesen**
- Store: `conversation_tests::an_expanded_row_lists_every_message_of_its_conversation_oldest_first`
  (Posteingang, Gesendet, Archiv; bevorzugter Ort; Zahl der Zeile = Zahl der Kinder; ohne Thread;
  `not_found`), `an_expanded_row_of_a_very_long_conversation_holds_its_newest_messages` (1.005
  Mails: die neuesten 1.000, älteste zuerst). Engine (Fake-Server):
  `comfort::tests::an_expanded_conversation_row_lists_its_messages_from_every_folder`.
- Vitest: neu `app/tests/conversation-tree.test.tsx` (16 Tests: Baumlogik rein; Aufklappen mit der
  Maus mit Ebenen, Positionen, Namen, Ordner, Linie und axe; Tastatur → ← ↑ ↓; Zuklappen mit dem
  Cursor auf einer Mail; Laden blockiert die Liste nicht; Fehler und Wiederholen mit Eingabe;
  stabil bei neuer Antwort und neuer Reihenfolge; per ID verlangte Mail; eine Mail rechts (Zeile und
  Kindzeile, axe); Gelesen beim Öffnen nur die gezeigte; Kürzel, Schnellaktion und gemischte
  Auswahl auf Zeile und Kindzeile; Umschalt-Bereich), angepasst `conversations.test.tsx`,
  `threads.test.tsx`, `ai-panel.test.tsx`.
- `scripts/check.sh` in der Hauptarbeitskopie (mit den noch nicht eingecheckten Dateien der Blöcke
  B und C) grün: 1.355 Rust-Tests (102 ignoriert, Feature e2e eingeschlossen), 4.036 Vitest-Tests in
  89 Dateien, Clippy auch für Windows, cargo deny, pnpm audit, Lizenzen. Ein erster Lauf scheiterte
  nur an den Doc-Tests von `mail-app` („can't find crate for `tauri`“): ein paralleler Build einer
  anderen Sitzung im gemeinsamen `target`; einzeln wiederholt grün, der zweite ganze Lauf grün.
- Desktop (`scripts/e2e-gui.sh` im eigenen Worktree): neu `conversations.e2e.mjs` (4 Tests auf dem
  60-Mail-Thread des Seeds im Archiv: Baum, „59 replies“, Kinder Ebene 2 mit Set-Größe 60, „Sent“,
  36 px, gepunktete Linie 1 px, axe hell und dunkel ohne schwere Befunde; eine Mail rechts für Zeile
  und Kindzeile; nur Tastatur → → ↓ ↓ ← ←; flüssiges Scrollen aufgeklappt: Median 17 ms, kein
  leeres Bild), angepasst `search.e2e.mjs` (Baum, „n replies“, eine Karte rechts),
  `keyboard.e2e.mjs` (Liste auch als Baum), `readme-shots.e2e.mjs` und `support/scroll.mjs` (lesen
  Baumeinträge). Ganzer Desktop-Lauf auf 7edb9eb: **24 von 24 Spec-Dateien grün** (10:22 min); `keyboard.e2e.mjs` allein (frisches Datenverzeichnis, also
  mit Gesprächen als Baum) auf dem Stand von 0bbb77d 4 von 4 grün.
- Flüssig beim Abgleich (`account-sync.e2e.mjs`, Liste einzelner Mails): Im ersten ganzen Lauf auf
  a28215d schlug die Messung mit einem einzelnen Bild von 75 ms fehl (Grenze 50 ms). Gegenprobe
  allein, abwechselnd gebaut: Stand vor Block A (f56fc3a^) 4 Läufe, 2 mit einem einzelnen langen
  Block (77 und 56 ms), 2 grün (27 ms); d73c421 5 Läufe, 4 mit einem einzelnen Block von 60–75 ms.
  Mit 7edb9eb (keine zusätzlichen Zuordnungen je Nachladen und Bildlauf in Listen einzelner Mails)
  3 von 3 grün (längstes Bild 33, 34, 28 ms; Median 17 ms; kein leeres Bild), im ganzen Lauf
  ebenfalls grün. Der einzelne lange Block tritt also auch ohne Block A auf (Rechner mit parallelen
  Builds anderer Sitzungen und einem iOS-Simulator, Last 4–16); die Ursache ist nicht geklärt.
- Bildschirmfotos angesehen (hell und dunkel, 1200 × 800): `conversations-unfolded-*`,
  `conversations-reading-row*`, `conversations-reading-child`, `conversations-keyboard`.

**Ungeprüft / offen**
- Windows (Darstellung der gepunkteten Linie, Tastatur) nicht geprüft.
- Bildschirmfotos in `docs/images/` nicht neu aufgenommen (die Pille „n Antworten“ ist dort sichtbar
  anders); sinnvoll zusammen mit den Kontaktbildern aus Block B (`README_SHOTS=1`, eigener Seed).
- In der schmalen Liste beginnt die Linie erst an der ersten Kindzeile (unter dem Avatar der
  zweizeiligen Gesprächszeile hängt kein Stück, dessen Höhe von Schriftgröße und Zeilen abhängt).
- Gespräche mit 1.000 Kindern in der echten App nicht gesondert gemessen (gemessen: 60).
- Vereinter Posteingang und Suchtreffer mit aufgeklappten Gesprächen nur in Vitest geprüft.

### C — Ansichten in der Spalte bearbeiten

**Stand vom 24.11.2026, 15:45** (Block C, eigene Sitzung). Commits: c027e0e (Kern), 55ec9b9
(Oberfläche), 01d868b (Escape bei verlorenem Fokus, vom Desktop-Test gefunden), cdef21e
(Shell-Test), dd1954f, 1f82811, ada744d, 2897e5e (Desktop-Specs), d14315a (`docs/DESIGN.md`,
Abschnitt „Ansichten bearbeiten (Stufe 17)“). Die Ansicht „Kontakte“ selbst (Seite, Palette
„Gehe zu Kontakte“, `goContacts`) kam mit Block B in a8670d9 (Entscheidung 0050); sie steht als
`contacts` in der einen Liste `COLUMN_VIEW_IDS` und ist damit in Spalte, Leiste und Editor, im
Standard eingeblendet hinter den gespeicherten Ansichten. Keine neue Entscheidung: Die Stufe sah
eine UI-Einstellung im Kern vor.

**Umgesetzt**
- Kern: `UiPreferences.columnViews` (Liste von `{ id, shown }` in der Reihenfolge des Nutzers),
  Standard `[]` (alle Ansichten in der Reihenfolge der Oberfläche, auch nach „Auf Standard
  zurücksetzen“), höchstens 32 Einträge, einfache IDs, keine doppelt. Schlüssel `ui.column_views`
  in `SYNCED_SETTINGS`; ein Wert, den diese Version nicht lesen kann, gilt als Standard; die
  Diagnose nennt „default“ oder „chosen, n hidden“. Die Oberfläche kennt die Ansichten (eine Liste,
  `COLUMN_VIEW_IDS` in `app/src/features/layout/columnViews.ts`); eine Ansicht, die eine spätere
  Version bringt, erscheint an ihrem Standardplatz, Einträge unbekannter Ansichten (neueres Gerät)
  bleiben beim Speichern erhalten.
- „Ansichten bearbeiten …“ über das „…“ am Etikett „Ansichten“, das „…“ jeder Ansicht (dort auch
  „In der Seitenleiste ausblenden“), Umschalt+F10/Kontextmenütaste/Rechtsklick und die
  Befehlspalette (Gruppe Layout). Dialog mit Ein-/Ausblenden, Verschieben per Griff (Ziehen mit
  Vorschau in Spalte und Leiste, gespeichert beim Loslassen), Pfeilknöpfen und Alt+↑/Alt+↓ (Fokus
  bleibt, Ansage der neuen Stelle), „Auf Standard zurücksetzen“, „Fertig“; Englisch und Deutsch.
- Spalte und Leiste zeigen nur die gewählten Ansichten in gewählter Reihenfolge; in der Leiste
  stehen sie zwischen Posteingang und Gesendet (gespeicherte Ansichten rücken damit im Standard vor
  Gesendet), die festen Einträge bleiben. Ausgeblendete Ansichten bleiben in der Palette und behalten
  ihre Kürzel. Keine Ansicht gewählt: leiser Hinweis mit „Ansichten bearbeiten …“.

**Nachgewiesen**
- Rust (in `check.sh`): `ui::tests::column_views_default_to_empty_and_are_checked` (Standard, Grenzen,
  Drahtform), `ui_prefs::tests::the_column_views_default_round_trip_and_sync` (Standard, Speichern
  in der Reihenfolge des Nutzers, Ereignis, Neustart, Ablehnung ändert nichts, unlesbare Werte,
  Zurücksetzen speichert `[]`, Schlüssel im Sync), der Erfassungstest des Einstellungs-Syncs (der
  Schlüssel wird ein Datensatz) und `tests::ui_preference_commands_store_validate_and_emit` (Shell).
- Vitest `app/tests/column-views.test.tsx`, 16 Tests: Standard und Zusammenführen, Öffnen aus
  Spalte, Menü (Umschalt+F10) und Palette, Fokus hin und zurück, Tab bleibt im Dialog, Escape auch
  ohne Fokus im Dialog, Ein- und Ausblenden mit Vorschau in Spalte und Leiste, Alt+Pfeil mit Ansage,
  Pfeilknöpfe, Ziehen mit Vorschau vor dem Speichern und Abbruch, Zurücksetzen, Neustart gegen den
  gespeicherten Stand, Änderung von einem anderen Gerät (`uiPreferencesChanged`), Ablehnung durch
  den Kern, ausgeblendete Ansichten in Palette und Kürzel, leere Auswahl mit Hinweis, Ausblenden
  aus dem Zeilenmenü, axe ohne schwere Befunde (Dialog offen, leere Auswahl, Deutsch unter Windows).
  Angepasst: `frame.test.tsx`, `sidebar-keyboard.test.tsx` (Reihenfolge der Leiste).
- Desktop (`scripts/e2e-gui.sh` im eigenen Worktree, mit Kontakten): auf 8b375b7 grün
  `frame.e2e.mjs` (13, darunter: aus der Palette öffnen, „Später“ per Klick ausblenden, „Markiert“
  mit Alt+↑ verschieben, der Kern hält die Auswahl, die Leiste zeigt sie zwischen Posteingang und
  Gesendet; Spalte und Leiste mit „Kontakte“), `restart.e2e.mjs` (4, die Auswahl ist nach dem
  Neustart da, danach zurück zum Standard), `keyboard.e2e.mjs` (4) und `a11y.e2e.mjs` (12).
- Zwei Geräte gegen Stalwart: `shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay`
  (`crates/mail-engine/tests/settings_sync.rs`, prüft jetzt auch `columnViews`) grün auf 8b375b7.
  Der erste Versuch direkt nach dem Desktop-Lauf scheiterte nach 0,04 s ohne sichtbaren Grund (die
  Ausgabe war gefiltert); mit frisch gestartetem Server und voller Ausgabe danach grün.
- `scripts/check.sh` auf cdef21e in einem sauberen Worktree grün (in zwei Läufen, weil im ersten
  ein paralleler Build im gemeinsamen `target` fremde Artefakte einschob): 1.326 Rust-Tests (102
  ignoriert), 5 der Feature e2e, 4.020 Vitest-Tests in 88 Dateien, Clippy auch für Windows.
  Im Hauptcheckout auf 36db98a (ohne fremde offene Änderungen) am Ende: alle Schritte bis ESLint
  grün (1.351 Rust-Tests, 102 ignoriert; 5 der Feature e2e; Clippy auch für Windows), Vitest im
  Lauf von `check.sh` mit 3 Zeitüberschreitungen (5 s) in `a11y.test.tsx` und `focus-ring.test.tsx`
  bei Systemlast 18 (parallele Agenten); danach einzeln 43 von 43 und die ganze Suite 4.036 von
  4.036 grün, ebenso Vite-Build, cargo deny, pnpm audit und die Lizenzliste.

**Ungeprüft:** Bedienung unter Windows (Alt+↑, Ziehen mit Zeigerereignissen statt HTML5-Drag-and-
drop, damit es auch neben Tauris Datei-Drop geht). Bildschirmfotos in `docs/images/`: Block C ändert
die Spalte im Standard nicht sichtbar (die „…“ erscheinen nur beim Überfahren); die neue Zeile
„Kontakte“ (Block B) und die Reihenfolge der eingeklappten Leiste sind in den README-Bildern noch
nicht neu aufgenommen.
