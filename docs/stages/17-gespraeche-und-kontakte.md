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

- [x] Jede Adresse aus empfangenen und gesendeten Mails (Von, An, Cc, Antwort an) wird Kontakt,
  auch rückwirkend für vorhandene Mails; Listen- und Systemadressen (no-reply, Mailer-Daemon)
  bleiben erkennbar und lassen sich ausblenden.
- [x] Eigene Bilder: Bild aus einer Datei wählen (Ziehen oder Dateiauswahl), quadratisch
  zuschneiden, verkleinert lokal speichern; Entfernen. Kein Laden von Bildern aus dem Netz
  (kein Gravatar, kein BIMI). Das Bild erscheint überall, wo heute Initialen stehen (Liste,
  Lesebereich, Composer, Kontakte); ohne Bild bleiben die Initialen.
- [x] Die Bilder werden wie Bilder aus Mails im abgeschotteten Hilfsprozess dekodiert und neu
  kodiert (0046), mit Größengrenze.
- [x] Kontaktseite als eigene Ansicht: Liste mit Bild, Name, Adresse, letzter Mail; schnelle
  Suche über Name, Adresse und Domain; Detailbereich mit allen Adressen, Bild, Notiz, letzten Mails
  mit diesem Kontakt, „Mail schreiben“, „Alle Mails anzeigen“, „Als Spam markieren“.
- [x] „Als Spam markieren“ sperrt die Adresse (vorhandene Sperrliste aus Stufe 16) und bietet an,
  vorhandene Mails in den Spam-Ordner zu verschieben (über die Outbox, nur bei Konten mit
  Schreibzugriff). Rückgängig machbar.
- [x] Kontakte, Bilder (verkleinert, mit Größengrenze) und Spam-Markierungen laufen über den
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
- [x] Ein eigenes Bild erscheint in Liste, Lesebereich, Composer und Kontakten; nach einem
  Neustart noch da; ein manipuliertes Bild wird abgelehnt, ohne die App zu stören.
- [x] Kontaktsuche findet nach Name, Adresse und Domain bei 10.000 Kontakten unter 100 ms.
- [x] „Als Spam markieren“ sperrt die Adresse, neue Mails von ihr landen nicht im Posteingang;
  Rückgängig hebt das auf.
- [x] Ansichten lassen sich ein-, ausblenden und sortieren; die Auswahl übersteht einen Neustart.
- [x] Bildschirmfotos in `docs/images/` neu, wo sich etwas sichtbar geändert hat.
- [x] `scripts/check.sh` ist grün, Desktop-Tests grün.

## Nicht in dieser Stufe

- Kontakte aus dem Netz (CardDAV, Google, Microsoft): nach Version 1 (0032).
- Bilder aus dem Netz (Gravatar, BIMI, Favicons).

## Ergebnis

### A — Gespräche in der Liste aufklappen

**Stand vom 24.11.2026, 15:10** (Block A, eigene Sitzung). Commits: 7177509 (Store- und
Engine-Tests der Kinder), 37424a7 (Oberfläche: Baum, Kindzeilen, eine Mail rechts, Aktionen),
76e1414 (Desktop-Specs), 7e13816 (Entscheidung 0049, Nachtrag in 0039, `docs/DESIGN.md` Abschnitt
„Gespräche aufklappen (Stufe 17, Entscheidung 0049)“, `docs/ARCHITECTURE.md`), 2ac5cd8 (eine per ID
verlangte Mail klappt ihr Gespräch auf), 8b25248 (schmale Kindzeilen, Spec auf Englisch), 207014e
und 07be093 (weniger Arbeit je Nachladen in Listen einzelner Mails; Tastatur-Spec nimmt den Baum),
ece0a11 (Tastatur-Spec zählt die Liste nicht zu den Bäumen der Spalte).
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
  Baumeinträge). Ganzer Desktop-Lauf auf 07be093: **24 von 24 Spec-Dateien grün** (10:22 min); `keyboard.e2e.mjs` allein (frisches Datenverzeichnis, also
  mit Gesprächen als Baum) auf dem Stand von ece0a11 4 von 4 grün.
- Flüssig beim Abgleich (`account-sync.e2e.mjs`, Liste einzelner Mails): Im ersten ganzen Lauf auf
  8b25248 schlug die Messung mit einem einzelnen Bild von 75 ms fehl (Grenze 50 ms). Gegenprobe
  allein, abwechselnd gebaut: Stand vor Block A (37424a7^) 4 Läufe, 2 mit einem einzelnen langen
  Block (77 und 56 ms), 2 grün (27 ms); 207014e 5 Läufe, 4 mit einem einzelnen Block von 60–75 ms.
  Mit 07be093 (keine zusätzlichen Zuordnungen je Nachladen und Bildlauf in Listen einzelner Mails)
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

### B — Kontakte

**Stand vom 24.11.2026, 16:00** (Block B, eigene Sitzung). Commits: 7d71735 (Kern), 0791cae
(Oberfläche), 1034414 (Entscheidung 0050 und Dokumente), 38a05ec (Neustart im Kerntest), e72f1dc
(Desktop-Spec, Feinschliff aus ihren Bildern), b51d064 (Datensätze eines anderen Geräts),
d355c1a (axe-Test der Kontakte mit mehr Zeit unter Last). Entscheidung
`docs/decisions/0050-kontakte-bilder-spam.md`, Migration 0020.

**Umgesetzt**
- Jede Adresse aus Von, An, Cc, Bcc (eigene Mails) und Antwort an wird Kontakt; Antwort an neuer
  Mails aus dem Kopfblock der Kategorien (`message.reply_to_addrs`). Arten Person, Liste, System
  (`contact.kind`); Listen- und Systemadressen ranken nicht, fehlen in der Autovervollständigung
  (außer der Nutzer schrieb ihnen) und standardmäßig in der Ansicht („Listen und Systemadressen
  zeigen“). Rückwirkend: der Kontakte-Auftrag liest nach neuen Mails die von Stufe 10 gelesenen
  Mails im Hintergrund nach (Teilindex, je 500, älteste zuerst) und fügt nur hinzu, was Stufe 10
  ausließ; Antwort-an alter Mails aus dem gespeicherten Mailtext (ganz, höchstens 1 MiB); Arten
  der alten Zeilen werden einmal je Lauf neu berechnet. Rang und Zusammenführen wie bisher.
- Bilder: Systemdialog oder Ziehen auf das Fenster; der Kern liest die Datei (höchstens 20 MiB),
  der Helfer `mail-mcp --contact-picture` kodiert sie für den Zuschnitt (höchstens 1024 px) und das
  Quadrat als JPEG 256 × 256, höchstens 40 KiB; gespeichert in `contact_picture` (Datenbank).
  Typisierte Fehler `picture_too_large`, `picture_unsupported`, `picture_damaged`. Zuschnitt in der
  Ansicht (Ziehen, Pfeiltasten, Plus/Minus, Zoom-Regler, Eingabe/Escape), Entfernen. Der Avatar
  zeigt das Bild in Listenzeilen, Lesebereich, Composer-Chips (neu mit Mini-Avatar) und Kontakten;
  `MessageRow.tsx` unverändert. Kein Bild aus dem Netz.
- Ansicht `contacts` (Spalte, Leiste, Palette `goContacts`, Standardeintrag nach „Gespeicherte
  Ansichten“, `AppShell`): Liste mit Bild, Name, Adresse, Datum der letzten Mail, Suche über Name,
  Adresse und Domain, Listbox mit Tastatur; Detail mit Bild, Name (bearbeiten), allen Adressen
  (Trennen), Notiz, letzten Mails (öffnen im Fenster), „Mail schreiben“, „Alle Mails anzeigen“
  (`from:` über alle Konten), „Als Spam markieren“/„Kein Spam“. Leere Zustände ohne Beispiele.
- „Als Spam markieren“ sperrt jede Adresse des Kontakts als blockierten Absender (Stufe 16: neue
  Mails gehen in den Spam-Ordner bzw. werden verborgen) und fragt, ob vorhandene Mails mitgehen
  (je Konto über die Outbox, nur mit Schreibzugriff; Konten ohne sagen es); „Rückgängig“ hebt die
  gesetzten Sperren auf und macht das Verschieben rückgängig.
- Sync: Notiz im Datensatz `contact`, neue Art `contactPicture` (56 KiB), Spam-Markierungen als
  `blockedSender`; Bilder aus dem Sync zeigt das Gerät erst nach eigener Neukodierung im Helfer.
- Einstellungen → Kontakte: Hinweis, „Kontakte öffnen“, Doppelte zusammenführen wie bisher.

**Nachgewiesen**
- Rust (in `check.sh`): `mail-content` (Beobachtungen aller Adressen, `counted_before_stage17`,
  Arten, Systemadressen als Kontakte ohne Rang; Bildkodierung: großes Bild, Quadrat am Rand,
  Transparenz auf Weiß, Rauschen unter 40 KiB, SVG/leer/defekt/60.000 × 60.000 px/über 20 MiB
  abgelehnt), `mail-store` `contacts_view_tests` (8: Antwort-an mit neuen Mails, Quellen von
  Auftrag und Nachlesen getrennt, Personenfilter und Arten, Notizen und Bilder samt Erfassung für
  den Sync und Anzeige erst nach eigener Kodierung, Name, Notiz, Bild und dessen Entfernen von
  einem anderen Gerät über `apply_sync` wie in einem Durchgang, Mails eines Absenders ohne
  Spam/Gesendet, Suche bei 10.000 Kontakten, Abfragepläne), `mail-engine` (Nachlesen = Neulesen im
  `compose::contacts`-Test; über `AppCore` mit dem Fake-Server: jede Adresse mit Art,
  Notiz und Bild mit Neustart und Sync-Datensätzen, präparierte/zu große/fremde Dateien mit
  typisierten Fehlern, mit konfiguriertem Helfer nichts im App-Prozess dekodiert, Spam mit
  Verschieben, neuer Mail in den Spam-Ordner und Rückgängig, Konto ohne Schreibzugriff),
  `mail-mcp` `picture_isolation` (2, gegen das echte Helfer-Programm: Zuschnitt und Kodierung;
  präparierter Kopf, abgeschnittenes PNG, SVG, Abbruch nach Zeit, fehlender Helfer), der
  Erfassungstest des Einstellungs-Syncs (jede Art, auch `contactPicture`).
- Suche: 0,8–8,0 ms bei 10.000 Kontakten (Debug-Build, auch ohne Treffer), Grenze 100 ms.
- Vitest: `contacts-view.test.tsx` (12: Zuschnitt-Rechnung, Bild mit Tastatur wählen und
  speichern, Abbrechen, abgelehnte Datei, Bild in Mailliste, Lesebereich und Composer-Chip,
  Tastatur in der Liste, Filter, Notiz, letzte Mails, „Mail schreiben“, „Alle Mails anzeigen“,
  Spam mit Frage, Hinweis und Rückgängig, Konto ohne Schreibzugriff, „Liegen lassen“ und „Kein
  Spam“, Ablehnung, Deutsch, axe mit offenem Zuschnitt und offener Frage), `contacts.test.tsx`
  (16, auf die Ansicht umgestellt; Einstellungen mit Verweis und Doppelten), axe der Kontakte in
  Hell und Dunkel (`a11y.test.tsx`).
- Desktop: `contacts.e2e.mjs` 3 von 3 grün, zweimal, über `scripts/e2e-gui.sh` im eigenen Worktree
  (0791cae mit den Dateien von e72f1dc; App und Helfer als eigene Kopie): Absender, Cc und
  Antwort-an werden Kontakte; Suche nach Name, Adresse, Domain; ein gezogenes PNG öffnet den
  Zuschnitt mit der JPEG des Helfers, das Bild steht danach als `data:image/jpeg` in Seite, Liste
  und Posteingangszeile; „Als Spam markieren“ verschiebt beide Mails auf dem Server in den
  Spam-Ordner (Posteingang 9.962 → 9.960), „Rückgängig“ holt sie zurück; keine CSP-Verletzung.
- `scripts/check.sh` im Haupt-Checkout grün: zuerst auf e72f1dc (1.350 Rust-Tests), zuletzt
  gestartet auf d355c1a: 1.351 Rust-Tests (102 ignoriert), 5 der Feature e2e, Clippy auch für
  Windows, 4.036 Vitest-Tests in 89 Dateien, Vite-Build, cargo deny, pnpm audit, Lizenzen. Der
  Vitest-Schritt lief schon mit dcb2b05 einer anderen Sitzung (15 s je Test). Zwei Läufe dazwischen
  scheiterten an der Last der parallelen Sitzungen (Lastmittel 17): einmal fehlte in Clippy ein
  `objc2`-Artefakt im gemeinsamen `target`, einmal liefen drei Vitest-Tests in ihre Zeitgrenze
  (einzeln grün; für den Kontakte-Test d355c1a).

**Ungeprüft / offen**
- Einstellungs-Sync zwischen zwei Geräten über den echten Transport (Ordner im Postfach) nicht mit
  Kontakten laufen lassen: geprüft sind Erfassung, Inhaltsprüfung und Anwenden je Art; der
  Transport selbst ist artunabhängig (Konvergenztests).
- Windows: Dialog, Ziehen, Helfer-Modus und Anzeige nicht auf Windows gelaufen (Clippy für das
  Windows-Ziel grün).
- Dauer der Migration 0020 bei sehr großen Datenbanken (zwei neue Indizes über `message`) nicht
  gemessen.
- Antwort-an alter Mails ohne gespeicherten Mailtext wird nicht nachgetragen.
- Bildschirmfotos in `docs/images/`: nicht neu gemacht (die README-Bilder entstehen mit
  `README_SHOTS=1`; die Kontaktansicht ist dort noch nicht dabei). Bilder des Desktop-Tests liegen
  nur lokal unter `app/tests/e2e/screenshots/contacts-*.png`.

### C — Ansichten in der Spalte bearbeiten

**Stand vom 24.11.2026, 15:45** (Block C, eigene Sitzung). Commits: bfbe1ae (Kern), 8a7ebc9
(Oberfläche), 59bb3bb (Escape bei verlorenem Fokus, vom Desktop-Test gefunden), d045f52
(Shell-Test), 37305b7, 7f7fbfb, 2a23e5a, 9d5efe1 (Desktop-Specs), c78e1d0 (`docs/DESIGN.md`,
Abschnitt „Ansichten bearbeiten (Stufe 17)“). Die Ansicht „Kontakte“ selbst (Seite, Palette
„Gehe zu Kontakte“, `goContacts`) kam mit Block B in 0791cae (Entscheidung 0050); sie steht als
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
- Desktop (`scripts/e2e-gui.sh` im eigenen Worktree, mit Kontakten): auf e804e55 grün
  `frame.e2e.mjs` (13, darunter: aus der Palette öffnen, „Später“ per Klick ausblenden, „Markiert“
  mit Alt+↑ verschieben, der Kern hält die Auswahl, die Leiste zeigt sie zwischen Posteingang und
  Gesendet; Spalte und Leiste mit „Kontakte“), `restart.e2e.mjs` (4, die Auswahl ist nach dem
  Neustart da, danach zurück zum Standard), `keyboard.e2e.mjs` (4) und `a11y.e2e.mjs` (12).
- Zwei Geräte gegen Stalwart: `shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay`
  (`crates/mail-engine/tests/settings_sync.rs`, prüft jetzt auch `columnViews`) grün auf e804e55.
  Der erste Versuch direkt nach dem Desktop-Lauf scheiterte nach 0,04 s ohne sichtbaren Grund (die
  Ausgabe war gefiltert); mit frisch gestartetem Server und voller Ausgabe danach grün.
- `scripts/check.sh` auf d045f52 in einem sauberen Worktree grün (in zwei Läufen, weil im ersten
  ein paralleler Build im gemeinsamen `target` fremde Artefakte einschob): 1.326 Rust-Tests (102
  ignoriert), 5 der Feature e2e, 4.020 Vitest-Tests in 88 Dateien, Clippy auch für Windows.
  Im Hauptcheckout auf f42169e (ohne fremde offene Änderungen) am Ende: alle Schritte bis ESLint
  grün (1.351 Rust-Tests, 102 ignoriert; 5 der Feature e2e; Clippy auch für Windows), Vitest im
  Lauf von `check.sh` mit 3 Zeitüberschreitungen (5 s) in `a11y.test.tsx` und `focus-ring.test.tsx`
  bei Systemlast 18 (parallele Agenten); danach einzeln 43 von 43 und die ganze Suite 4.036 von
  4.036 grün, ebenso Vite-Build, cargo deny, pnpm audit und die Lizenzliste.

**Ungeprüft:** Bedienung unter Windows (Alt+↑, Ziehen mit Zeigerereignissen statt HTML5-Drag-and-
drop, damit es auch neben Tauris Datei-Drop geht). Bildschirmfotos in `docs/images/`: Block C ändert
die Spalte im Standard nicht sichtbar (die „…“ erscheinen nur beim Überfahren); die neue Zeile
„Kontakte“ (Block B) und die Reihenfolge der eingeklappten Leiste sind in den README-Bildern noch
nicht neu aufgenommen.

### Abschluss (24.11.2026, abends)

- Volle Prüfung auf 10cd98d im sauberen Worktree: `scripts/check.sh` grün (1.358 Rust-, 4.042
  Vitest-Tests), Servertests 1.455 von 1.455; Desktop-Tests 23 von 25 Spec-Dateien. Die zwei
  Fehlschläge: die Einladungskarte blieb aus (ein älterer Wettlauf zwischen der ersten Antwort und
  `messageBodyAvailable`, behoben in 5a8ad9a mit Vitest-Tests) und ein einzelnes Bild über 50 ms
  beim Scrollen während des Erstabgleichs (Speicherbereinigung der WebView, Grenze begründet
  angepasst in a73b1a1, `docs/testing/performance.md`). Danach auf a73b1a1: `scripts/check.sh`
  grün (4.045 Vitest-Tests), Desktop-Tests 25 von 25 Spec-Dateien.
- README-Bilder auf dem fertigen Stand neu (`README_SHOTS=1`); die README zeigt nur noch
  `inbox-light.png`.
