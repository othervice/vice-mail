# 0055 — Quelltext, alle Kopfzeilen, Umleiten, Als neu bearbeiten

- **Datum:** 24.11.2026
- **Anlass:** Stufe 19, Teil B (Auswahl des Menschen vom 24.11.2026 nach dem Vergleich mit
  Thunderbird): „Quelltext anzeigen“ in eigenem Fenster mit „Alle Kopfzeilen“, „Umleiten“ mit
  `Resent-*`-Kopfzeilen über die Outbox, „Als neu bearbeiten“.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 19 B).
- **Code:** `crates/mail-content/src/source.rs` (`source_text`, `header_fields`),
  `crates/mail-content/src/compose/redirect.rs` (`build_redirect`),
  `crates/mail-model/src/source.rs`, `crates/mail-engine/src/reading/source.rs`,
  `crates/mail-engine/src/compose/redirect.rs`, `crates/mail-engine/src/compose/edit_as_new.rs`
  (mit `Compose::copy_content`, aus `open_from_message` herausgelöst),
  `crates/mail-store/src/compose.rs` (`enqueue_redirect`, `cancel_redirect`),
  `app/src-tauri/src/windows.rs` (`source-*`), `app/src-tauri/capabilities/source-window.json`,
  `app/src-tauri/src/commands/source.rs`, `app/src-tauri/src/commands/redirect.rs`,
  `app/src/features/source/SourceWindow.tsx`, `app/src/features/reading/AllHeaders.tsx`,
  `app/src/features/redirect/RedirectDialog.tsx`, `app/src/features/composer/SendNotices.tsx`.

## Entscheidung

### Quelltext in eigenem Fenster

1. **Ein Fenster je Mail** mit dem Label `source-<Konto-UUID ohne Bindestriche>-<Mail-ID>` und der
   Seite `index.html?window=source&accountId=…&messageId=…`, gebaut über `windows::guard` und
   `windows::finish` wie jedes Fenster (Navigations-Hook, keine neuen Fenster, keine Downloads, unter
   Windows der WebView2-Frame-Guard). Ein zweiter Aufruf holt das offene Fenster nach vorn. Der Titel
   ist der Betreff (der Produktname steht weiter nur in `tauri.conf.json`).
2. **Minimale Capability** `source-window` (Fenster `source-*`): nur `core:window:allow-close`
   (Escape schließt). Keine Ereignisse, kein Ziehen, kein Plugin. Das Label passt weder zu
   `message-*` noch zu `composer-*`, erbt also nichts vom Hauptfenster. Die eigenen Commands der App
   brauchen wie bisher keinen Eintrag. Die Seite liest ihre Einstellungen (Thema, Sprache, Akzent,
   Textgröße) einmal beim Öffnen und abonniert keine Kern-Ereignisse; `App` leitet das Fenster an
   `SourceRoot` vorbei an `useCoreEvents`.
3. **Nur Text, nie Markup.** Der Kern liefert die Mail so, wie der Server sie hat (gespeicherter
   Body, sonst über die Leseverbindung ganz geholt, nicht gespeichert), als Text:
   UTF-8 verlustbehaftet dekodiert (Bytes bleiben in „Als .eml sichern“ unverändert), CRLF als LF,
   Steuerzeichen außer Tab und Zeilenwechsel als Symbole des Blocks „Control Pictures“, C1-Zeichen und
   Richtungszeichen (C-8) als U+FFFD. Die Oberfläche zeigt ihn als einen Textknoten in einem `<pre>`
   in Festbreitenschrift (umbrechen schaltbar). Nichts wird interpretiert.
4. **Große Mails:** höchstens `MAX_SOURCE_BYTES` = 1 MiB, an der letzten Zeilengrenze davor
   gekürzt; das Fenster sagt es („Nur die ersten … von … werden gezeigt“) und bietet „Als .eml
   sichern“ (der bestehende Befehl `exportMessage`, Speicherdialog des Systems) für das Ganze.
5. **Tastenkürzel Cmd/Strg+U** (`viewSource` im Aktionsverzeichnis, frei: „U“ mit Umschalt ist
   „Als ungelesen markieren“, Cmd/Strg+U ist keine Systemtaste; im Editor bleibt es Unterstreichen,
   weil Aktionskürzel in Textfeldern nicht greifen). Auch im Menü „…“ der Mailkarte, unter „Weitere
   Aktionen“ und in der Befehlspalette.

### Alle Kopfzeilen

6. Im Menü „…“ der Mailkarte „Alle Kopfzeilen zeigen“: unter dem Kopf der Karte jede Kopfzeile in
   ihrer Reihenfolge, Name wie geschrieben, Wert entfaltet und mit dekodierten Encoded Words,
   höchstens `MAX_HEADER_FIELDS` = 500 Felder und `MAX_HEADER_VALUE_CHARS` = 8.192 Zeichen je Wert,
   unsichtbare Zeichen wie im Quelltext sichtbar gemacht. Quelle: der gespeicherte Body, bei großen
   Mails der gespeicherte Abschnitt `HEADER`; nur ohne beides wird die Mail geholt. Die Anzeige
   gehört zur Mail wie das gewählte Format.

### Umleiten

7. **Format (RFC 5322, 3.6.6):** vor den Kopf der unveränderten Mail kommt ein Block
   `Resent-Date`, `Resent-From`, `Resent-To`, `Resent-Cc`, `Resent-Message-ID` (neue ID in der
   Domain der Identität). `Resent-Bcc` wird nie geschrieben; Blindkopien stehen nur im Umschlag.
   Aus dem Kopf des Originals fallen drei Felder heraus: `Bcc` (eine Kopie aus „Gesendet“ würde
   sonst Blindkopien verraten), `Delivered-To` und `Return-Path` (Spuren der früheren Zustellung;
   ein altes `Delivered-To` mit der neuen Empfängeradresse lässt Server wie Postfix die Mail als
   Schleife abweisen). Sonst bleibt jedes Byte gleich; Zeilenenden werden CRLF (SMTP verlangt es;
   eine Mail aus IMAP hat sie schon).
8. **Absender:** eine Identität des Kontos (Auswahl im Dialog; ohne Wahl die, an die die Mail
   ging, sonst die Standardidentität). Eine Identität eines anderen Kontos wird abgelehnt.
9. **Über die Outbox wie ein Senden:** eine `send`-Operation mit `SendJob.redirect = true`, ohne
   Entwurf, mit der Senden-rückgängig-Verzögerung der Einstellungen (`not_before`). Die
   Sendehinweise zeigen „Umleitung: <Betreff>“ mit Countdown und „Rückgängig“, das
   `cancelRedirect` aufruft (die Operation verschwindet, nichts wird gesendet); danach
   `send_in_progress`. Fehlgeschlagene Umleitungen haben „Erneut versuchen“; „Bearbeiten“ gibt es
   nicht. Dieselbe `requestId` zweimal legt nur eine Operation an.
10. **Keine Kopie in „Gesendet“:** sie sähe aus wie eine Mail des ursprünglichen Absenders und
    würde in der Liste verwirren. Das Original bekommt nach der Annahme `$Forwarded` (wie eine
    Weiterleitung), sofern es eine Message-ID hat.
11. **Nur mit Schreibzugriff:** im Kern (`read_only`, bevor etwas gespeichert wird) und in der
    Oberfläche (die Aktion ist deaktiviert und erklärt warum). Höchstens
    `MAX_REDIRECT_RECIPIENTS` = 100 Empfänger.

### Als neu bearbeiten

12. `NewDraft::EditAsNew { messageId }`: ein neuer lokaler Entwurf (Art „neu“) mit An, Cc, Bcc,
    Betreff, Text und Anhängen der Mail, auch aus „Gesendet“. Neue Message-ID, kein In-Reply-To,
    keine References, keine Serverkopie, die die Mail ersetzen würde; die Mail bleibt, wo sie ist.
    Identität: die, die die Mail geschickt hat, sonst die, an die sie ging, sonst die Standardidentität.
    Keine zusätzliche Signatur (der Text hat seine). Text und Anhänge kommen wie bei Entwürfen anderer
    Programme (0047): HTML für den Editor mit den gezeigten Inline-Bildern als Teile, alles andere als
    Anhang (`Compose::copy_content`, jetzt von beiden genutzt).

## Erwogen, nicht gewählt

- **Quelltext im Lesebereich** statt eigenem Fenster: der Auftrag nennt ein eigenes Fenster; so
  bleibt die Liste sichtbar, und das Fenster braucht keine der Rechte des Hauptfensters.
- **Kopie der Umleitung in „Gesendet“** (Thunderbird legt eine ab): siehe 10.; das Kennzeichen
  `$Forwarded` und der Postausgang zeigen, dass umgeleitet wurde.
- **`Resent-Bcc:` leer schreiben** (RFC 5322 erlaubt es): nicht nötig; weglassen verrät nicht einmal,
  dass es Blindkopien gab.

## Folgen

- `docs/ARCHITECTURE.md`: Fenster und Capabilities (Shell), Outbox (Umleiten), Senden.
- Shortcut-Verzeichnis: `viewSource` belegt Cmd/Strg+U.
