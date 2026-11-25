# 0064 — Kontextmenü für Mails

- **Datum:** 25.11.2026
- **Anlass:** Rückmeldung 5 zu 0.1.1 (Stufe 21): „Rechtsklick auf eine Mail öffnet ein
  ausführliches Menü im Stil von Thunderbird“ (Bild vom 25.11.: oben eine Zeile mit Symbolen,
  darunter Öffnen ›, Verschieben in ›, Kopieren in ›, Schlagwörter ›, Markieren ›, Speichern unter…,
  Drucken…). Plan der Stufendatei: Symbole für Antworten, Archivieren, Spam, Löschen; darunter
  Öffnen ›, Antworten/Allen/Weiterleiten/Als Anhang/Umleiten ›, Als gelesen bzw. ungelesen,
  Markieren ›, Anpinnen, Später ›, Aufgabe, Zum Lesen, Erinnern ohne Antwort ›, Verschieben in ›,
  Kopieren in ›, Schlagwörter ›, Als neu bearbeiten, Quelltext, Als .eml speichern, Drucken; auch
  mit Kontextmenü-Taste und Umschalt+F10, in jeder Liste.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, Punkt 5)
- **Code:** `app/src/components/ContextMenu.tsx` (Menü, `placeMenu`),
  `app/src/features/actions/MessageMenu.tsx` (Einträge aus dem Aktionsverzeichnis,
  `folderMenuScope`), `app/src/features/actions/FolderActions.tsx` (Controller, Verschieben und
  Kopieren, Dialoge der Ordneransicht, aus `MailView.tsx` herausgelöst),
  `app/src/features/messages/MessageList.tsx` (`onRowMenu`, `RowMenuRequest`, Ring der Zielzeile),
  `app/src/features/messages/MessageRow.tsx` (`contextTarget`), `app/src/stores/printRequest.ts`,
  `app/src/features/extras/MessageTools.tsx` (Drucken auf Anfrage),
  `crates/mail-store/src/parity.rs` (`check_transfer`: Kopie innerhalb des Kontos)
- **Tests:** Vitest `context-menu.test.tsx` (Lage an den Rändern, Tastaturwege, Untermenüs,
  gesperrte Einträge, Ziel und Auswahl, Umschalt+F10, Löschen mit Rückgängig, Verschieben, Kopieren
  im Konto, Schlagwörter, .eml, Drucken, Später, nur lesend, axe hell und dunkel),
  `stage16_tests::a_copy_within_the_account_is_an_append_to_the_other_folder_that_leaves_the_source`,
  `mail-engine/tests/transfer.rs` (gegen den Testserver: die Kopie liegt in beiden Ordnern, ein
  Verschieben im Konto wird abgelehnt),
  Desktop `context-menu.e2e.mjs` (echter Rechtsklick über `e2e_native_mouse_event`, Art
  `rightClick`)

## Entscheidung

**1. Aufbau.** Oben eine Gruppe aus vier Symbolen an festen Plätzen: Antworten (bei Entwürfen
„Entwurf bearbeiten“), Archivieren, Als Spam markieren (im Spam „Kein Spam“), Löschen (im Papierkorb
„Endgültig löschen“); was dort nicht passt (Antworten bei mehreren Mails, Archivieren im Archiv),
bleibt gesperrt an seinem Platz. Darunter, durch Linien in Gruppen geteilt:

```
 Öffnen ›                      Im Lesebereich | In neuem Fenster        (nur eine Mail)
 Antworten und Weiterleiten ›  Antworten | Allen antworten | Weiterleiten | Als Anhang weiterleiten | Umleiten
 ─────
 Als gelesen / ungelesen markieren
 Markieren ›                   Als gelesen | Als ungelesen | Markieren | Markierung entfernen
 Anpinnen / Nicht mehr anpinnen
 Zurückstellen ›               Später heute | Morgen | Am Wochenende | Nächste Woche (mit Zeit) | Datum und Uhrzeit wählen … | Nicht mehr zurückstellen
 Als Aufgabe markieren / Aufgabe entfernen, Aufgabe erledigt / wieder öffnen
 Zum Lesen vormerken / Aus „Zum Lesen“ entfernen
 Erinnern, wenn keine Antwort kommt ›   Morgen | In 3 Tagen | Nächste Woche | Datum und Uhrzeit wählen … | Erinnerung entfernen
 ─────
 Verschieben in ›              Ordner wählen … (V) | Ordner des Kontos | andere Konten ›
 Kopieren in ›                 Ordner des Kontos | andere Konten ›
 Schlagwörter ›                Schlagwörter der Ansicht (Häkchen) | Schlagwörter bearbeiten …
 ─────
 Als neu bearbeiten, Quelltext anzeigen (⌘U), Als .eml-Datei speichern …, Drucken …
 ─────
 🔒 Nur lesend: … / Änderungen erlauben …   (nur bei einem Konto ohne Schreibzugriff)
```

Umschalter erscheinen als die Aktion, die sie für alle Ziele täten („Als gelesen markieren“, wenn
eine ungelesen ist), wie in der Symbolleiste. „Markieren ›“ führt beide Richtungen ausdrücklich,
für gemischte Auswahlen, in denen der Umschalter nur eine anbietet. Kürzel stehen leise rechts.
Nicht im Menü: Blockieren und „Endgültig löschen“ außerhalb des Papierkorbs (Symbolleiste, Palette).

**2. Aus dem Aktionsverzeichnis.** Jeder Eintrag ist eine Aktion von `registry.ts` mit ihrem Namen,
Symbol, Kürzel und Zustand (`actionState`): Was die Symbolleiste nicht anbieten würde, fehlt; was
ein nur lesendes Konto nicht darf, steht gesperrt da (`aria-disabled`), mit dem Grund als Tooltip
(„Archivieren: Änderungen sind für dieses Konto ausgeschaltet“) und als Beschreibung
(`aria-describedby`) aus dem Hinweis am Ende, der auch „Änderungen erlauben …“ anbietet. Ein
Untermenü, dessen Einträge alle schreiben (Markieren, Verschieben, Schlagwörter), ist dann als
Ganzes gesperrt. Ein gesperrter Eintrag tut nichts (WAI-ARIA), das Menü bleibt offen. Gewählt läuft
die Aktion durch den Aktions-Controller der Ansicht: dieselbe Outbox, dieselbe Rückgängig-Meldung,
dieselben ausgeblendeten Zeilen und der Fokus auf der nächsten Mail wie aus der Symbolleiste.

**3. Ziel.** Gehört die Zeile zur Auswahl, gilt das Menü der ganzen Auswahl, sonst nur ihr. Diese
Zeile bekommt einen gestrichelten Ring in `focus-ring`, die Auswahl bleibt, wie sie war (wie
Thunderbird): Der Rechtsklick öffnet nichts und markiert nichts als gelesen. Kindzeilen eines
aufgeklappten Gesprächs wirken allein (0049), Gesprächszeilen auf ihr Gespräch (0039). Die
Kontextmenü-Taste und Umschalt+F10 öffnen das Menü der Cursorzeile, an ihrem Anfang.

**4. Bedienung** (WAI-ARIA Menu): Beim Öffnen hat der erste Eintrag den Fokus. ↓ ↑ gehen zeilenweise
(die Symbolgruppe ist ein Halt, ← → gehen in ihr), Pos1 und Ende, ein Buchstabe springt zum
nächsten passenden Eintrag (derselbe noch einmal weiter). →, Eingabe oder Leertaste öffnen ein
Untermenü auf seinem ersten Eintrag, ← oder Escape schließen es auf seinen Eintrag zurück; Escape
im Menü schließt es und gibt der Liste den Fokus zurück (ohne damit etwas zu wählen), Tab ebenso.
Überfahren fokussiert und öffnet Untermenüs, ein Druck außerhalb schließt, ebenso eine Änderung der
Fenstergröße oder das Verlassen des Fensters. Tasten im Menü erreichen die Kürzel der Ansicht nicht.
Gezeichnet in `document.body`, platziert über CSS-Variablen per CSSOM (CSP): am Zeiger, nach links
bzw. oben geklappt, wo es aus dem Fenster ragen würde, sonst an den Rand geschoben; höher als das
Fenster rollt es. Untermenüs neben ihrem Eintrag, links, wo rechts kein Platz ist. Aussehen wie
die Menüs der App: `surface-raised`, Haarlinie, `shadow-floating`, Rundung 10, Einträge 32 px mit
13 px Text und 16-px-Symbol, Fokus und Überfahren in `hover`, Häkchen in Akzentfarbe, Unterordner
16 px je Ebene eingerückt.

**5. Untermenüs ohne Umweg.** Später und Erinnern zeigen die Zeiten der Auswahlfenster und führen
sie direkt aus; „Datum und Uhrzeit wählen …“ öffnet das Auswahlfenster. Verschieben zeigt die
Ordner des Kontos in der Reihenfolge des Ordnerbaums (ohne den eigenen), oben „Ordner wählen …“
(das Fenster mit Filter, wie V), danach die anderen Konten als Untermenüs (Verschieben dorthin
fragt erst, 0033). Schlagwörter zeigt die Schlagwörter der geladenen Mails der Ansicht als
Häkchen (gesetzt, wenn alle Ziele es haben).

**6. Kopieren, auch innerhalb des Kontos.** „Kopieren in“ nutzt den Weg der Übertragung zwischen
Konten (0033): die Bytes der Mail eingefroren, im Ziel per Outbox angehängt, die Kopie über die
Message-ID bestätigt, in Aktivität sichtbar, ohne Rückgängig. Der Kern lehnte bisher jedes Ziel im
eigenen Konto ab; jetzt nur noch Verschieben im eigenen Konto (das ist ein gewöhnliches
Verschieben) und eine Kopie in den Ordner, in dem die Mail liegt. Ohne Rückfrage: Nichts verlässt
seinen Ordner, und „Kopieren“ ist schon die Wahl. Nur lesende Konten dürfen Quelle sein, das Ziel
muss schreiben dürfen.

**7. Speichern und Drucken.** „Als .eml-Datei speichern …“ ist der Befehl des Lesebereichs (Stufe 9);
der gewählte Pfad steht in einer Meldung. Drucken druckt seit Stufe 9 die Mail des Lesebereichs
(Druckstile): Das Menü öffnet die Mail dort und druckt, sobald sie gezeigt ist (`printRequest`).

## Verworfen

- **Das Menü des Systems (Tauri-Menü-API):** sähe nativ aus, kann aber keine Symbolzeile, keine
  gesperrten Einträge mit Grund und keine Zeiten von Später zeigen, ohne für jede Aktion einen
  eigenen Befehl, und ist ohne WebView nicht testbar. Das eigene Menü folgt den Menüs der App.
- **Radix Context Menu:** fügt `<style>`-Elemente ein, die die CSP verbietet (wie bei Dialog, Popover).
- **Rechtsklick wählt die Zeile (Outlook):** öffnete die Mail im Lesebereich und markierte sie mit
  „Beim Öffnen als gelesen markieren“ als gelesen; „Als ungelesen markieren“ aus dem Menü wäre
  dann sofort überholt.
- **Gesperrte Einträge zeigen beim Wählen den Hinweis wie die Symbolleiste:** Ein gesperrter
  Menüeintrag tut nach WAI-ARIA nichts; der Grund steht im Menü selbst, mit dem Weg ihn zu ändern.
- **Kopieren im Konto über IMAP COPY:** wäre schneller und auf dem Server ohne Neuübertragung,
  braucht aber eine neue Operation der Outbox (Absturzsicherheit, Bestätigung, JMAP). Der Weg der
  Übertragung hat das alles schon; siehe Rückweg.

## Folgen

- Bei Gmail legt eine Kopie in einen Label-Ordner eine zweite Mail an statt das Label zu setzen
  (Labels setzt „Schlagwörter“ nicht, 0034); ungeprüft gegen einen echten Gmail-Server.
- Desktop-Tests schicken einen echten Rechtsklick (`e2e_native_mouse_event`, Art `rightClick`:
  rechte Taste gedrückt und los an der WKWebView). Verhindert die Seite das Menü nicht, öffnete
  WebKit sein eigenes und hielte den Hauptfaden fest; der Test bliebe dann hängen.
- Die Ordneransicht hat ihren Controller, ihre Dialoge und das Verschieben über Konten in
  `FolderActions.tsx`; die Dialoge stehen im Baum vor der Liste, damit ihr Schließen den Fokus der
  Liste zurückgeben kann, solange deren Handle noch hängt (React hängt es im selben Commit neu an).

## Rückweg

Ohne Kontextmenü: `onRowMenu` der Listen weglassen; die Liste verhindert dann weiter das Menü der
WebView. Kopieren im Konto wieder sperren: die Bedingung in `check_transfer` zurück auf „kein Ziel
im eigenen Konto“; „Kopieren in“ zeigt dann nur andere Konten.
