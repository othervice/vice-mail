# Handprüfung mit Screenreader (VoiceOver und Narrator)

Für Stufe 8. Die automatische Prüfung (axe in `app/tests/a11y.test.tsx`) findet fehlende Namen,
falsche Rollen und kaputtes ARIA, aber nicht, ob sich die App mit Screenreader gut bedienen lässt.
Das prüft diese Liste. Jeder Schritt hat ein Feld **Ergebnis**: `ok`, oder was stattdessen
angesagt wurde oder passiert ist. Weicht etwas ab, einfach weitermachen.

Der Ablauf ist auf beiden Systemen derselbe. Wo sich die Tasten unterscheiden, steht macOS zuerst.
Gearbeitet wird nur mit dem lokalen Testserver (`scripts/testmail.sh start` und `seed`) oder einem
echten Konto, das nur lesend eingerichtet ist.

## 0. Vorbereitung

**0.1 Screenreader einschalten**
- **Tun:** macOS: VoiceOver mit Cmd+F5 (Systemeinstellungen → Bedienungshilfen → VoiceOver).
  Windows: Sprachausgabe mit Win+Strg+Eingabe.
- **Erwartet:** Der Screenreader spricht. Im Folgenden heißt die VoiceOver-Taste „VO“ (Ctrl+Option),
  die Sprachausgabe-Taste „SA“ (Feststell- oder Einfg-Taste).
- **Ergebnis:** System und Version: ____ · Screenreader-Version: ____ · Sprache der App: ____

**0.2 App mit Testdaten starten**
- **Tun:** App starten, ein Konto mit mindestens 30 Mails im Posteingang, davon einige ungelesen,
  eine mit Anhang, eine markiert. Fenster etwa 1280 × 800.
- **Erwartet:** Der Posteingang ist offen.
- **Ergebnis:**

## 1. Rahmen und Orientierung

**1.1 Bereiche**
- **Tun:** VoiceOver: VO+U, dann zu „Orientierungspunkte“. Narrator: SA+F7 (Liste der Orientierungspunkte).
- **Erwartet:** Mindestens „Ansichten“ (Navigation, die Icon-Leiste) und der Hauptbereich. Bei
  geöffneter Mail zusätzlich „Nachricht“.
- **Ergebnis:**

**1.2 Icon-Leiste**
- **Tun:** Mit Tab in die Icon-Leiste, dann mit den Pfeiltasten durch die Einträge.
- **Erwartet:** Jedes Symbol hat einen Namen („Posteingang“, „Gesendet“, „Einstellungen“ …), der
  aktive Eintrag wird als ausgewählt oder aktuell angesagt, die Ansichten nennen ihr Kürzel
  (Cmd+1 bzw. Strg+1 für den Posteingang). Kontofarben kommen nie ohne Kontonamen.
- **Ergebnis:**

## 2. Nachrichtenliste

**2.1 Liste betreten**
- **Tun:** Mit Tab in die Liste.
- **Erwartet:** Angesagt wird die Liste mit Ordnernamen („Nachrichten in Posteingang“), dann die
  erste Mail mit Absender, Betreff, Zeit, Zustand (ungelesen, markiert, Anhang) und ihrer Position
  („1 von 30“), obwohl nur ein Teil der Zeilen im DOM ist.
- **Ergebnis:**

**2.2 Bewegen**
- **Tun:** Pfeil ab, Pfeil auf, dann j und k, dann Pos1 und Ende.
- **Erwartet:** Jede Mail wird beim Erreichen angesagt, mit stimmender Position. Am Ende der
  geladenen Seite werden weitere Mails nachgeladen, ohne dass die Ansage abreißt oder springt.
- **Ergebnis:**

**2.3 Mehrfachauswahl**
- **Tun:** Umschalt+Pfeil ab zweimal, dann x.
- **Erwartet:** Die Auswahl wird angesagt („3 ausgewählt“), die Auswahlleiste erscheint. Die
  Schnellaktionen in der Zeile werden **nicht** angesagt (sie sind nur für die Maus; die Tastatur hat
  die Kürzel und die Symbolleiste).
- **Ergebnis:**

## 3. Lesebereich

**3.1 Öffnen**
- **Tun:** Eine Mail in der Liste wählen und Eingabe drücken.
- **Erwartet:** Der Fokus geht in den Lesebereich, angesagt wird „Nachricht“ und der Betreff. Kopf
  (Absender, Empfänger, Zeit) ist als Text lesbar, die Aktionen bilden eine Symbolleiste
  („Nachrichtenaktionen“), in der die Pfeiltasten wandern und jede Schaltfläche ihr Kürzel nennt.
- **Ergebnis:**

**3.2 Inhalt einer HTML-Mail**
- **Tun:** Mit den normalen Lesebefehlen (VO+Pfeile, Narrator: Scanmodus) durch den Text der Mail.
- **Erwartet:** Der Text der Mail ist lesbar, Links werden als Links angesagt. Externe Bilder sind
  blockiert; der Hinweis dazu ist erreichbar.
- **Ergebnis:**

**3.3 Zurück**
- **Tun:** Esc.
- **Erwartet:** Der Fokus ist wieder in der Liste, auf derselben Mail.
- **Ergebnis:**

## 4. Aktionen und Statusmeldungen

**4.1 Archivieren per Kürzel**
- **Tun:** In der Liste e drücken (bei schreibgeschütztem Konto: nur die Ansage prüfen).
- **Erwartet:** Angesagt wird „Archiviert“ mit dem Hinweis auf Rückgängig, ohne dass der Fokus die
  Liste verlässt. Der Fokus steht danach auf der nächsten Mail, die auch angesagt wird. Bei einem
  schreibgeschützten Konto wird erklärt, dass Änderungen aus sind.
- **Ergebnis:**

**4.2 Rückgängig**
- **Tun:** z drücken.
- **Erwartet:** „Rückgängig gemacht“ wird angesagt, die Mail ist wieder in der Liste.
- **Ergebnis:**

**4.3 Fehler**
- **Tun:** Netz trennen (WLAN aus), eine Aktion auslösen, Netz wieder verbinden.
- **Erwartet:** Ein Fehler oder „wartet“ wird angesagt (als Warnung, nicht nur farbig), mit Ort und
  nächstem Schritt.
- **Ergebnis:**

## 5. Verfassen

**5.1 Neue Mail**
- **Tun:** c drücken.
- **Erwartet:** Der Fokus steht im Feld „An“, angesagt als Eingabefeld mit Vorschlagsliste.
- **Ergebnis:**

**5.2 Empfänger und Felder**
- **Tun:** Eine Adresse tippen, mit Pfeil ab einen Vorschlag wählen, Eingabe. Dann mit Tab zu Betreff
  und Text.
- **Erwartet:** Vorschläge werden beim Wandern angesagt, der gewählte Empfänger als Chip mit
  Entfernen-Schaltfläche. Jedes Feld hat einen Namen; die Formatierungsleiste ist eine Symbolleiste.
- **Ergebnis:**

**5.3 Senden**
- **Tun:** Cmd+Eingabe bzw. Strg+Eingabe (nur gegen den Testserver).
- **Erwartet:** „Wird gesendet“ und danach „Gesendet“ werden angesagt, mit Rückgängig, solange es geht.
  Der Fokus geht dorthin zurück, wo er vor dem Verfassen war.
- **Ergebnis:**

## 6. Befehlspalette und Kürzel

**6.1 Palette öffnen**
- **Tun:** Cmd+K bzw. Strg+K.
- **Erwartet:** Angesagt wird „Befehlspalette“ und das Eingabefeld „Befehl“ mit aufgeklappter Liste.
  Der aktive Eintrag wird beim Öffnen und bei jedem Pfeil angesagt, mit Gruppe und Kürzel.
- **Ergebnis:**

**6.2 Suchen und ausführen**
- **Tun:** „arch“ tippen, Eingabe.
- **Erwartet:** Die Zahl der Treffer wird angesagt. Eingabe führt „Archivieren“ aus (mit Ansage wie in
  4.1). Ein nicht verfügbarer Eintrag wird als nicht verfügbar angesagt; Eingabe darauf sagt den
  Grund („Wähle zuerst eine Nachricht aus“).
- **Ergebnis:**

**6.3 Schließen**
- **Tun:** Palette öffnen, Esc.
- **Erwartet:** Die Palette ist zu, der Fokus steht wieder dort, wo er vorher war.
- **Ergebnis:**

**6.4 Übersicht der Kürzel**
- **Tun:** ? drücken (nicht in einem Textfeld), mit Tab durch, Esc.
- **Erwartet:** Dialog „Tastenkürzel“ mit Listen aus Befehl und Kürzel. Tab bleibt im Dialog, Esc
  schließt ihn und gibt den Fokus zurück.
- **Ergebnis:**

**6.5 Kürzel ändern**
- **Tun:** Einstellungen → Tastatur, bei „Archivieren“ „Hinzufügen“, dann y drücken. Danach bei
  „Antworten“ „Hinzufügen“ und y drücken.
- **Erwartet:** Die Aufnahme wird angesagt („Drück das neue Kürzel …“). Nach y: „Y führt jetzt
  Archivieren aus.“ Beim zweiten Mal: „Y ist schon „Archivieren“ zugewiesen …“, die Aufnahme bleibt
  offen, Esc bricht ab.
- **Ergebnis:**

## 7. Einstellungen

**7.1 Bereiche**
- **Tun:** Einstellungen öffnen, mit Pfeil ab durch die Bereiche.
- **Erwartet:** Die Bereiche sind Tabs mit Namen, der gewählte wird als ausgewählt angesagt. Jede
  Auswahl (Thema, Lesebereich, Dichte) ist eine Optionsgruppe mit Namen und Beschreibung.
- **Ergebnis:**

**7.2 Layout**
- **Tun:** Darstellung → Lesebereich „Unten“, dann „Anstelle der Liste“; jeweils eine Mail öffnen
  und mit Esc zurück.
- **Erwartet:** In beiden Layouts gelten 3.1 und 3.3 unverändert.
- **Ergebnis:**

## 8. Darstellung

**8.1 Bewegung reduzieren**
- **Tun:** macOS: Bedienungshilfen → Anzeige → Bewegung reduzieren. Windows: Barrierefreiheit →
  Visuelle Effekte → Animationseffekte aus. Dann Palette öffnen, eine Mail archivieren.
- **Erwartet:** Keine Animationen.
- **Ergebnis:**

**8.2 Kontrast und Fokus**
- **Tun:** Mit Tab durch Leiste, Liste, Lesebereich und Einstellungen, in hell und dunkel.
- **Erwartet:** Der Fokusring ist überall deutlich sichtbar.
- **Ergebnis:**

## Notizen

Was fehlte, störte oder anders angesagt wurde als erwartet:

- ____
