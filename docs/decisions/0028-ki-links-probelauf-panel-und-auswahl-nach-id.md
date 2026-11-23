# 0028 — Links in KI-Ausgaben, Probelauf vor der Zustimmung, Panel-Zustand, Auswahl nach ID

Stand 23.11.2026. Schließt drei Lücken der Stufe 13 (Links aus Ausgaben über die Prüfung aus
Stufe 3, genaue Größe vor der Zustimmung, Zustand des KI-Panels im Kern statt im Web-Storage) und
eine der Stufe 9 (die Liste wählt eine Mail nach ID aus; offen laut 0027, Punkt 12). Baut auf 0005
(Link-Platzhalter), 0024 (KI-Verdrahtung), 0025 (Oberflächen-Einstellungen) und 0027 auf.

## Entscheidung

### Links in KI-Ausgaben

1. **Der Kern findet die Links selbst**, im Text, den er als Ergebnis zurückgibt: Aussagen und
   Fristen der Zusammenfassung, Aussagen einer Antwort, der Antwortvorschlag und der Text der
   Schreibhilfe. Erkannt wird wie bei Mails im Textformat (`mail_content::render_text`:
   `http(s)://`, `www.`, `mailto:`, nackte Adressen). Jedes Textfeld bekommt dazu `segments`
   (`TextSegment`, dieselbe Form wie der Textkörper einer Mail); `text` bleibt unverändert für
   „Entwurf öffnen“, Übernehmen und Kopieren.
2. **Ein Token je Ergebnis.** Die Links eines Ergebnisses liegen im selben Register wie die
   Links einer Darstellung (`reading::links`, Besitzer `RenderOwner::AiResult` mit den Konten,
   aus deren Mails das Ergebnis entstand), unter einem neuen Zufalls-Token (128 Bit wie die
   Render-Tokens), über alle Felder des Ergebnisses durchnummeriert. Das Ergebnis trägt
   `linkToken` (`null` ohne Links). Die Oberfläche schickt nie eine URL zum Kern, nur Token und
   Index; unbekannte Tokens und Indizes sind `not_found`.
3. **Dieselben Commands, dieselbe Prüfung.** `describeLink`/`openLink` gelten unverändert: dieselbe
   Analyse und Warnungen (`mail_content::check_link`), dieselbe Karte in der Oberfläche
   (`LinkCard`), `http`/`https` in den System-Browser, `mailto:` in die Warteschlange des
   Composers, alles andere abgelehnt. Kein neuer Command, kein neuer Fehlercode.
4. **Eigene Grenze.** Das Register hält höchstens 32 KI-Ergebnisse neben den 32 Darstellungen von
   Mails und Dateien; KI-Ergebnisse verdrängen nie die Links einer gelesenen Mail. Wird ein Konto
   entfernt, vergisst der Kern auch die Ergebnisse aus seinen Mails. Der Navigations-Hook nimmt
   ein KI-Token nicht an (`linkActivated` antwortet `false`): KI-Ausgaben stehen nie in einem
   Frame, eine solche Navigation wäre gefälscht.
5. **Bilder werden nie geladen.** Markdown-Bilder verlieren ihr Ziel schon in `mail-ai`; Markup
   bleibt Text. Eine Bildadresse im Text ist ein gewöhnlicher Link, der durch die Prüfung geht und
   im Browser öffnet, nie ein Bild in der App.
6. **Oberfläche.** Im KI-Panel sind Links eigene Elemente (`role="link"`, Tastatur mit Enter),
   nie `<a href>`; ein Klick zeigt die Linkkarte im Panel bei ihrem Ergebnis (der Lesebereich
   zeigt nur Karten seiner eigenen Darstellung). Der Antwortvorschlag steht in einem Textfeld, in
   dem nichts klickbar ist; seine Links stehen darunter als Liste, jeder über die Prüfung. Die
   Vorschau der Schreibhilfe zeigt Links ebenso. Kennt der Kern das Token nicht mehr, sagt die
   Karte das, ohne „Mail neu laden“.

### Probelauf vor der Zustimmung

7. **`aiPrepareTransfer(input) → AiTransfer`** mit `AiTransferInput` (Tag `function`:
   `summary`/`reply_suggestion` mit `messageId`, `writing_help` mit `passage` und `task`,
   `search` mit `question`). Der Kern bereitet die Anfrage genau so vor wie die echte Anfrage
   (dieselben Funktionen `prepare_summary_in`, `prepare_reply_in`, `mail_ai::prepare_writing`,
   `mail_ai::prepare_query`, dasselbe Budget, dieselbe Bereinigung) und gibt zurück, was
   `aiRequest` vor dem Senden melden würde: Mails, Bytes, Konten, Host, Modell, Kürzungen.
8. **Ohne Zustimmung, ohne Senden.** Der Probelauf braucht eingeschaltete KI und eine nutzbare
   Funktion, aber keine Zustimmung (er ist das, was der Zustimmungsschritt zeigt). Er registriert
   keine Anfrage-ID, sendet kein Ereignis, schreibt nichts ins Protokoll und baut keine Verbindung
   zum Anbieter auf (der Anbieter wird nur gebaut, wie beim Speichern eines Profils). Er scheitert
   wie die Anfrage vor dem Senden: `ai_off`, `ai_not_permitted` vor dem Laden eines Bodys,
   `not_found`, `invalid_argument`. Bodies, die nicht gespeichert sind, holt er wie die Anfrage
   nur lesend vom Mailserver.
9. **Fragen haben zwei Anfragen.** Der Probelauf zeigt die erste (nur die Frage). Die Größe der
   zweiten hängt von der Suche ab, die das Modell erst vorschlägt; der Zustimmungsschritt sagt das,
   und `aiRequest` meldet sie vor dem Senden.
10. **Oberfläche.** Der Zustimmungsschritt fragt den Probelauf ab, sobald er erscheint, und zeigt
    „Diese Anfrage würde senden: …“ mit Lokal/Cloud und den Hinweisen zu Kürzungen. Scheitert der
    Probelauf, zeigt er den Fehler; zustimmen oder ablehnen bleibt möglich.

### Zustand des KI-Panels

11. **Ein Feld der Oberflächen-Einstellungen**: `UiPreferences.aiPanel` (`"open"` oder `"folded"`,
    Standard `"open"`), gespeichert unter `ui.ai_panel` (Tabelle `setting`, `global`). Geprüft
    durch die Form: ein anderer Wert oder ein fehlendes Feld ist beim Schreiben ein Fehler der
    Brücke; ein unlesbarer gespeicherter Wert gilt als `"open"` und wird protokolliert.
12. **Nur auf diesem Gerät, nicht synchronisiert.** Gründe: KI in der App ist je Gerät
    eingerichtet (0024, `ai.settings` ist Geräte-Einstellung); auf einem Gerät ohne KI gibt es kein
    Panel, ein übernommener Zustand hätte dort keine Bedeutung. Das Panel ist ein Teil des Layouts
    eines Fensters, und Layouts bleiben auf dem Gerät (0025: andere Bildschirme). Rückweg:
    `ui.ai_panel` in `SYNCED_SETTINGS` aufnehmen.
13. **Oberfläche.** Der Speicher `stores/aiPanel.ts` übernimmt den Wert aus den
    Oberflächen-Einstellungen (Laden, Speichern, `uiPreferencesChanged` aus einem anderen
    Fenster) und speichert Auf- und Zuklappen über `saveUiPreferences`. Lehnt der Kern ab, kehrt
    der gespeicherte Wert zurück und ein Hinweis sagt es. Der frühere Wert im Web-Storage
    (`layout.aiPanel`) wandert einmal mit der Übernahme aus 0025 in den Kern und wird gelöscht.

### Auswahl nach ID in der Liste (Stufe 9)

14. **Die Liste wählt eine Mail nur unter den geladenen Zeilen aus** (`MessageListHandle.selectById`):
    nur diese Mail, wie ein Klick, in den Blick gescrollt (`scrollToIndex`, `align: auto`), und die
    Ansicht öffnet sie (`onSelectedById`, auch beim Layout „Lesebereich statt Liste“). Ein Mitglied
    eines zugeklappten Bündels ist keine Zeile; dann `false`.
15. **Wer antwortet:** Jede Liste des Hauptfensters meldet sich mit ihrem Bereich an
    (`stores/listSelect.ts`, `{ accountId, mailboxId, inbox }`): Ordner, sortierter Posteingang und
    Ordner in der Konversationsansicht mit Konto und Ordner; Später, Angepinnt und Aufgaben ohne
    Ordnerbereich; vereinter Posteingang und Suche als gemischte Liste.
16. **Klick auf eine Systemmeldung:** Zeigt der offene Thread die Mail schon, bleibt sie dort (wie
    bisher). Sonst wartet die Anfrage, bis die Liste des Ordners (der im selben Moment gewechselt
    wird) geladen ist und keine Abfrage läuft, und wählt die Mail dort aus. Fehlt sie unter den
    geladenen Zeilen oder zeigt sich die Liste nicht binnen 5 s (`LIST_SELECT_WAIT_MS`), öffnet
    sie sich wie bisher in einem eigenen Fenster. Eine neuere Anfrage ersetzt eine ältere; eine
    abgelaufene wird nie mehr beantwortet. Ohne Ordner ist der Posteingang des Kontos gemeint.
17. **Quelle einer KI-Antwort:** zuerst die ausgewählte Mail oder der offene Thread, dann die
    Liste, die das Hauptfenster gerade zeigt, sonst ein eigenes Fenster. Quellen einer
    Zusammenfassung wählen nicht in der Liste aus: eine andere Zeile auszuwählen bände das Panel an
    diese Mail und nähme die Zusammenfassung weg.
18. **Kein Command im Kern.** Seiten nachzuladen, um eine nicht geladene Mail zu finden, ist nicht
    gebaut; eine neue Mail (der Fall der Systemmeldung) steht oben, also auf der ersten Seite.

## Folgen

- `mail-model`: `AiClaim`, `AiDeadline`, `AiReplySuggestion`, `AiWritingResult` mit `segments`;
  `AiSummary`, `AiAnswer`, `AiReplySuggestion`, `AiWritingResult` mit `linkToken`;
  `AiTransferInput`; `AiPanelState`, `UiPreferences.aiPanel`.
- `mail-engine`: `reading::links` (`RenderOwner::AiResult`, `FoundLinks`, getrennte Grenzen),
  `ai` (`ai_open` ohne Zustimmung und Registrierung, `ai_prepare_transfer`, gemeinsame
  Vorbereitung), `ui_prefs` (`ui.ai_panel`).
- Shell: Command `ai_prepare_transfer`. Bindings neu erzeugt.
- `docs/ARCHITECTURE.md`: Sicherheitsgrenzen (Links), KI-Verdrahtung, Oberflächen-Einstellungen.

## Geprüft

- `mail-model`: `ai::tests::transfer_inputs_are_tagged_with_the_function`,
  `claims_carry_their_segments_and_results_their_link_token`,
  `ui::tests::the_ai_panel_state_is_one_of_two_values`, `wire_format_matches_the_contract`.
- `mail-engine`: `reading::links::tests::ai_results_have_their_own_bound_and_go_with_their_accounts`,
  `found_links_are_numbered_across_the_texts_of_a_result`;
  `ai::tests::links_in_results_go_through_the_link_check_of_mail` (Warnungen wie bei Mails,
  `http(s)` an das System, `mailto:` als Composer-Anfrage, unbekannter Index und gefälschtes Token
  `not_found`, `linkActivated` lehnt ab, Markdown-Bild ohne Ziel, andere Schemata bleiben Text,
  keine weitere Verbindung); `ai::tests::the_dry_run_shows_exactly_what_would_be_sent_and_sends_nothing`
  (Attrappe zählt null Verbindungen und null Anfragen, kein Ereignis, kein Protokolleintrag; nach
  der Zustimmung ist `transfer` der echten Anfragen gleich dem Probelauf, für alle vier
  Funktionen); `ui_prefs::tests::the_ai_panel_state_stays_on_this_device`; Sync:
  `settings_sync::tests::capture::*` (der Schlüssel wird nicht erfasst), Servertest
  `shortcuts_and_text_size_reach_the_other_device_history_and_layouts_stay` um das Panel ergänzt
  (gegen den Testserver, in dieser Sitzung nicht gelaufen).
- Oberfläche (Vitest): `ai-links-and-consent.test.tsx` (Links nur über die Karte, keine
  Navigation, Tastatur, vergessenes Token, Links des Antwortvorschlags, Text ohne Links; genaue
  Größe vor der Zustimmung ohne Anfrage an den Anbieter, Hinweis bei Fragen, gescheiterter
  Probelauf; Panel-Zustand im Kern, Ereignis aus anderem Fenster, Ablehnung, Übernahme aus dem
  Web-Storage), `list-select.test.tsx` (Auswahl nach ID, Systemmeldung, KI-Quelle, Wartezeit),
  angepasst `ai-panel.test.tsx`, `open-by-id.test.tsx`, `ui-preferences.test.tsx`.
- **Ungeprüft:** Desktop-Test (WebDriver) für alle vier Teile; der Servertest des Sync mit dem
  Panel-Zustand; ein zugeklapptes Bündel bei der Auswahl nach ID.

## Rückweg

- Links in KI-Ausgaben als reiner Text: `segments` ohne Link-Segmente liefern, Token weglassen.
- Panel-Zustand synchronisieren: `ui.ai_panel` als synchronisierbare Einstellung registrieren.
- Auswahl einer nicht geladenen Mail: ein Command, der die Position einer Mail in einer Liste
  liefert, und Nachladen bis dorthin.
