# 0020 — Posteingang-Komfort: Kern, Speicher und Shell

- **Datum:** 23.11.2026
- **Stufe:** 9 (Backend und Shell; die Oberfläche folgt in einer eigenen Sitzung)
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-store/migrations/0007_inbox.sql`,
  `crates/mail-store/src/{categories,marks,inbox,rules,comfort_api}.rs`,
  `crates/mail-engine/src/comfort/`, `crates/mail-engine/src/reading/export.rs`,
  `crates/mail-model/src/inbox.rs`, `crates/mail-model/src/{mailto,calendar}.rs`,
  `crates/mail-content/src/{mailto,calendar}/`, `crates/mail-imap/src/convert.rs`,
  `app/src-tauri/src/{commands/comfort,notifications,default_app,links}.rs`,
  `app/src-tauri/Info.plist`, `app/src-tauri/windows/installer-hooks.nsh`
- **Messung:** `docs/testing/performance.md` (Stufe 9)

Setzt 0016 (Kategorien, Bündel) in Speicher und Engine um und baut Snooze, Pins, Aufgaben,
lokale Regeln, Benachrichtigungen, Zähler, Abmelden, Standard-Mail-App, Einladungen und
`.eml`-Export. Hier steht, was vom Plan abweicht oder ihn genauer festlegt.

## Kategorien

- **Spalten.** `message.category` (seit Stufe 2 als INTEGER angelegt) ist die Kategorie nach den
  Regeln des Nutzers, als Zahl (0 Person, 1 Benachrichtigung, 2 Newsletter, NULL noch nicht
  eingeordnet), nicht als Text wie in 0016 vorgeschlagen: Die Spalte war schon INTEGER (STRICT), und
  die Zahl ist im Index kleiner. Daneben `category_auto` und `category_reason` (JSON) als
  automatisches Ergebnis, `important`, die normalisierten Schlüssel `sender_key`, `sender_domain`,
  `list_key` und `unsubscribe_one_click`.
- **Wo eingeordnet wird.** Der Store parst keine Mails; die Engine gibt ihm einen `Classifier`
  (die Regeln aus `mail_content::classify`), wie die `BodyTextSource` aus Stufe 7. Er läuft im
  Schreib-Thread in der Transaktion, die die Nachricht speichert, und bekommt die lokalen Signale,
  die nur die Datenbank kennt: eigene Adressen (Konten und Identitäten), wie oft der Nutzer dem
  Absender geschrieben hat (`contact.sent` aus Stufe 5), und die Absender der Mails, auf die die
  neue antwortet (`In-Reply-To`/`References`, höchstens zehn, über `message_msgid`).
- **Kopfzeilen.** Der Adapter holt `CLASSIFY_HEADER_FIELDS` und `List-Unsubscribe-Post` im selben
  `BODY.PEEK[HEADER.FIELDS (…)]` wie die Felder aus Stufe 2 und reicht den Block (höchstens 16 KiB)
  als `MessageHeader::category_headers` weiter; die Liste ist in `mail-imap` dupliziert, ein Test
  hält sie gleich mit `mail-content` (mail-imap hängt nicht von mail-content ab). Der Store behält
  den Block nicht.
- **Bestandsmails** (vor der Migration gespeichert) ordnet ein Hintergrund-Auftrag aus den
  gespeicherten Kopfdaten ein, neueste zuerst (Teilindex `message_category_todo`), mit den lokalen
  Signalen von heute. Regeln, die die zusätzlichen Kopfzeilen brauchen (Berichte, bekannte Systeme,
  Versanddienste), greifen dort nicht; sie werden nicht nachgeholt (kein erneuter Abruf).
- **Regeln des Nutzers** (`sender_rule`, je normalisiertem Schlüssel eine Zeile mit Korrektur
  und/oder Wichtig-Markierung). Eine Änderung plant einen Durchlauf über alle Mails (neueste
  zuerst, 500 je Schritt): 100.000 Mails in 27 ms. Kein Index auf Absender oder Domain: Domains
  gelten mit Subdomains, das ginge per Index nur über umgedrehte Domains; der Durchlauf ist billiger
  als drei weitere Indizes auf `message`. Nach dem Durchlauf `inboxChanged`.
- **Lokale Regel „Kategorie“** setzt das automatische Ergebnis dieser einen Mail (Begründung
  `localRule`); Korrekturen je Absender gehen weiterhin vor.
- **Kopie am Ort.** Kategorie und Wichtig-Markierung stehen zusätzlich in `message_location`
  (Trigger beim Einfügen eines Orts und bei Änderung der Nachricht), damit Listen nach Kategorie und
  der Block „Priorität“ per Keyset über einen Index laufen (`loc_category`, Teilindex
  `loc_important`, mit `INDEXED BY`, weil der Planer ohne Statistik den Primärschlüssel nimmt).
- **Trefferquote** über den echten IMAP-Weg (Stalwart, Seed): 72 von 73 = 98,6 %; der eine Fehler
  ist die Phishing-Mail aus 0016 (`html_hostile.eml`).

## Posteingang und Bündel

- `listInbox` liefert Zeilen (Mail, Bündel, Fortsetzung eines Bündels) mit Datumsabschnitt
  (heute, gestern, diese Woche ab Montag, dann Monat; in der Zone des Systems, Zukunft zählt als
  heute). Das Limit zählt Mails, nicht Zeilen.
- **`BundleState` reist im Cursor** (`InboxCursor.bundles`, JSON, für die UI undurchsichtig). Nach
  jeder Seite bleiben nur die Bündel des letzten Abschnitts (`BundleState::keep_section`), denn
  frühere Abschnitte bekommen keine Mitglieder mehr; so bleibt der Cursor klein. Die Engine hält
  keinen Zustand je Liste.
- **Oben** (nur erste Seite): angepinnte Mails, zurückgekehrte Snoozes, höchstens drei Mails
  wichtiger Absender mit Gesamtzahl und Ungelesenen. Angepinnte und zurückgekehrte stehen nicht
  noch einmal in den Zeilen; wichtige schon (nie gebündelt). Wartende Snoozes fehlen in allen
  Posteingangslisten.
- Kategorien aus (`inboxSettings.categories`): keine Bündel, kein Prioritätsblock, Zeilen rein
  zeitlich. Einstellungen sind Geräte-Einstellungen (`inbox.settings`).

## Snooze, Pins, Aufgaben

- **Tabellen** `snooze`, `pin`, `task` mit Konto-UUID und Message-ID (Klasse sync, eindeutig je
  Paar), keine lokalen IDs. Alle lokalen Mails des Kontos mit dieser Message-ID tragen die
  Markierung; sie überlebt einen Neuaufbau des Postfachs. **Mails ohne Message-ID** lassen sich
  nicht markieren (`invalid_argument`): Ein Verweis wäre sonst nicht zwischen Geräten übertragbar.
- **Snooze:** Zeitpunkte in der Zone des Systems (morgen 08:00, Samstag 09:00, Montag 08:00,
  später heute = drei echte Stunden, auf die volle Stunde aufgerundet und nur, solange es noch heute
  ist; eigener Zeitpunkt mindestens eine Minute, höchstens fünf Jahre voraus). Eine nicht
  existierende Ortszeit rückt vor (jiff „compatible“). Der Zeitgeber wartet höchstens 15 s am Stück
  und liest dann die Wanduhr (Tokio-Zeitgeber stehen im Ruhezustand), dazu beim Start (App war
  geschlossen) und bei `resume`. Ohne Snooze wartet er nur auf einen Weckruf und fasst den Store nicht
  an. **Zurückgekehrt** heißt: `returned_at` gesetzt, Benachrichtigung, oben im Posteingang mit
  Kennzeichnung, bis der Nutzer die Markierung entfernt (`unsnoozeMessages`), erneut zurückstellt
  oder die Mail den Posteingang verlässt. Die Liste sortiert weiter nach Datum (Keyset); „oben“ ist
  der eigene Block, nicht eine neue Sortierung.
- **Aufgaben** werden **lokal** gehalten (Tabelle `task`, offen oder erledigt mit Zeitpunkt) und,
  wo das Konto schreiben darf, zusätzlich als Schlagwort `$Task` am Server gespiegelt, solange sie
  offen sind. Grund: Die Ansicht „Aufgaben“ braucht eine Abfrage über alle Konten ohne Scan der
  Schlagwort-Spalte, und nur lesende Konten sollen Aufgaben auch haben. Andere Geräte erfahren davon
  über den Einstellungs-Sync (neue Datenart `task`, Stufe 11); das Schlagwort ist für andere
  Programme.
- **Server-Schlagwörter:** `$Snoozed` während des Wartens, `$Task` für offene Aufgaben, beides nur
  bei `writes_enabled` und über die Outbox (`AddKeyword`/`RemoveKeyword` auf allen Orten). Ein
  Server ohne eigene Schlagwörter lässt die Operation scheitern (sichtbar in der Aktivität), die
  lokale Markierung bleibt. Pins bekommen kein Schlagwort. Verschoben wird nie etwas (Test mit
  Fake-Server und Stalwart: nur `STORE`).
- Ansichten „Später“, „Angepinnt“, „Aufgaben“ liefern höchstens 1.000 Zeilen ohne Seiten (wenige
  Einträge erwartet).
- `is:snoozed` findet wartende Snoozes.

## Lokale Regeln

- Bedingungen Absender, Empfänger (To/Cc), Betreff, Liste, Konto; Vergleich ohne Groß/Klein und
  Akzente (`fold`), Adressen auch als ganze Adresse oder Domain mit Subdomains. Alle oder eine
  Bedingung; keine Bedingung = jede Mail. Aktionen verschieben (Ordner per Konto-UUID und Pfad),
  als gelesen markieren, Kategorie, anpinnen, Benachrichtigung an/aus.
- **Reihenfolge:** nach Position; je Aktionsart gewinnt die erste passende Regel, „als gelesen“ und
  „anpinnen“ einmal; `stop` beendet die Auswertung für diese Mail.
- **Wann:** nur für neue Mails im Posteingang, die ein Abgleich außerhalb des Erstabgleichs bringt
  (Rückmeldung der Sync-Sitzung je Fenster), während die App läuft.
- **Nur lesende Konten:** Verschieben und Markieren werden übersprungen und mit `ruleSkipped`
  (Grund `readOnly`, Anzahl Mails) gemeldet; Kategorie und Pin laufen trotzdem (lokal).
- Eine Regel, die verschiebt, unterdrückt die Benachrichtigung, außer sie verlangt sie
  ausdrücklich; eine Regel mit „Benachrichtigung an“ gilt auch für eine Mail, die sie als gelesen
  markiert hat.

## Benachrichtigungen und Zähler

- Der Kern entscheidet (pure Funktionen, getestet), die Shell zeigt an. **Plugin:**
  `tauri-plugin-notification` 2.8.1, nur aus Rust benutzt; die Oberfläche bekommt keine
  Notification-Capability. Gewählt, weil es gepflegt ist, App-Kennung und Klicks auf macOS und
  Windows über `notify-rust` erledigt und ein Klick (`tap`) an einen Rust-Handler geht. Sein
  Init-Script (ein `window.Notification`-Ersatz) läuft nur im Hauptframe; ohne Capability scheitert
  jeder Aufruf daraus.
- Standard: Personen und wichtige Absender, nicht Benachrichtigungen und Newsletter; je Konto
  stummschaltbar, Ruhezeiten (verwerfen, nicht nachholen), fehlgeschlagene und unklare Sendungen
  einmal je Versuch, zurückgekehrte Snoozes. Mehrere neue Mails eines Fensters sind eine Meldung.
- **Texte** kommen aus `app/src/i18n/`: Die Oberfläche schickt sie beim Start
  (`setNotificationTexts`); bis dahin englische Rückfalltexte im Kern (wie bei Fehlermeldungen).
- **Sperrbildschirm, ehrlich:** Die App kann den Inhalt aller Meldungen ausblenden („Neue
  Nachricht“ ohne Absender und Betreff). Ob Vorschauen auf dem Sperrbildschirm erscheinen,
  entscheidet allein das System (macOS: Mitteilungen, „Vorschauen zeigen“; Windows: Benachrichtigungen
  auf dem Sperrbildschirm); eine einzelne Meldung als privat markieren können die Schnittstellen von
  `notify-rust` nicht.
- **Klick:** öffnet das Fenster und sendet `openMessageRequested`. Windows meldet den Klick nur,
  solange die Meldung sichtbar ist (nicht aus dem Info-Center).
- **Zähler:** Tauri `set_badge_count` (macOS, Dock), auf Windows ein Overlay-Symbol am
  Taskleisten-Knopf, das die Shell aus einer 3×5-Ziffernschrift selbst zeichnet (bis 99). Beides ohne
  `unsafe`. Ungelesene im Posteingang oder nur Personen (Einstellung `badge`); entprellt auf die
  Ereignisse des Kerns.

## Abmelden

- Optionen aus `List-Unsubscribe` (gespeichert seit Stufe 2) und `List-Unsubscribe-Post` (seit
  dieser Stufe beim Eintreffen gespeichert; für ältere Mails aus dem gespeicherten Body).
- **Ein-Klick** (RFC 8058) nur mit `List-Unsubscribe=One-Click` und einer `https`-Adresse: ein POST
  mit genau diesem Inhalt über die gemeinsame TLS-Konfiguration, ohne Cookies, Referrer, Proxy und
  Wiederholungen, neutrale Kennung, nie zu privaten, Loopback- oder Link-Local-Adressen (dieselben
  Prüfungen wie externe Bilder, `reading/remote.rs`), keine Weiterleitungen (eine 3xx-Antwort gilt
  als beantwortet), 5 s Aufbau, 15 s gesamt.
- **mailto:** kein automatischer Versand. Die Antwort ist ein `ComposeRequest` (Empfänger, Betreff,
  Text aus dem URI), den die Oberfläche als vorbereiteten Entwurf öffnet; gesendet wird über die
  Outbox wie jede Mail, erst nach dem Klick auf „Senden“. Das hängt nicht vom Stand des
  Stufe-5-Backends ab.
- **Web:** `http`/`https` nur im System-Browser, die Bestätigung fragt die Oberfläche; die App lädt
  die Seite nicht selbst.
- Nachweis „ohne Klick keine Anfrage“: Zähl-Server (HTTPS, eigene Test-CA) zählt TCP-Verbindungen,
  null nach Abruf, Öffnen und Anzeigen der Optionen; Positivkontrolle eine Verbindung nach dem
  Klick.

## Standard-Mail-App

- **macOS:** `mailto` im `Info.plist` (von Tauri zusammengeführt); Links kommen als
  `RunEvent::Opened`. Abfragen und Setzen des Standards sind Launch-Services-Aufrufe; aus Rust
  bräuchten sie `unsafe` (FFI), das außerhalb von `webview2-guard` verboten ist. Die Shell nutzt
  deshalb das System-Werkzeug `osascript -l JavaScript` mit festem Skript (Abfrage über
  `NSWorkspace.URLForApplicationToOpenURL`, Setzen über `LSSetDefaultHandlerForURLScheme`, einziger
  Eingabewert die geprüfte Bundle-ID). Klappt es nicht (nicht installierter Entwicklungs-Build), sagt
  die Oberfläche den Weg über Mail → Einstellungen → Allgemein. Ungeprüft: ob macOS beim Setzen
  nachfragt.
- **Windows:** Der Installer trägt die App für den aktuellen Benutzer als `mailto`-Programm ein
  (NSIS-Hooks: ProgID `<Bundle-ID>.mailto`, `Capabilities`, `RegisteredApplications`). Seit
  Windows 10 wählt nur der Nutzer den Standard; die App öffnet die Einstellungen
  (`ms-settings:defaultapps?registeredAppUser=<Produktname>`). Ein Link kommt als Argument; läuft
  die App schon, reicht `tauri-plugin-single-instance` (nur Windows) die Argumente an den laufenden
  Prozess weiter (verhindert nebenbei zwei Prozesse auf einer Datenbank).
- Links, die vor der Oberfläche ankommen (Start per Link), warten in der Shell; die Oberfläche holt
  sie einmal ab (`takeLaunchLinks`), danach kommt jeder als `composeRequested`.
- **`mailto:`-Parser** (RFC 6068) in `mail-content`: nur `to`, `cc`, `bcc`, `subject`, `body`,
  `in-reply-to`; jede andere Kopfzeile wird ignoriert und gemeldet, `bcc` immer mit Hinweis
  (`bccPresent`), Prozent-Kodierung als UTF-8, `+` bleibt Plus, RFC-2047-Wörter im Betreff, Grenzen
  (64 KiB URI, 32 KiB Text, `MAX_RECIPIENTS`).

## Kalendereinladungen

- Eigener Leser in `mail-content` (RFC 5545 für die Anzeige, ohne neue Abhängigkeit außer `jiff`
  für Kalenderrechnung). Zeitzonen: IANA-Name über die tz-Datenbank des Systems (in der Engine),
  Herstellerpräfixe, eine Tabelle der Windows-Namen (CLDR), zuletzt die `VTIMEZONE` der Datei;
  unbekannt → Problem und schwebende Zeit. Wiederholungen nur als Beschreibung, nicht expandiert.
- `getMessageExtras` liefert die Einladungen nur, wenn der Body vollständig gespeichert ist;
  „Im Kalender öffnen“ (`openInvite`) schreibt den Teil als schreibgeschützte `.ics` in das
  Verzeichnis für geöffnete Dateien und übergibt sie dem System (wie Anhänge).

## Drucken und Export

- `.eml`-Export: genau die Rohdaten (gespeichert oder einmal ganz geholt, nicht gespeichert) an den
  Ort aus dem Speichern-Dialog, atomar.
- Drucken: `printWindow` öffnet den Druckdialog des Systems (dort „Als PDF sichern“); was gedruckt
  wird, bestimmt die Oberfläche mit Druck-Styles.
- **Offen:** `.eml`-Dateien öffnen (Betrachter für eine Datei ohne Konto) ist nicht gebaut; die App
  meldet sich deshalb auch nicht für `.eml` an.

## Ereignisse

`inboxChanged`, `ruleSkipped`, `openMessageRequested`, `composeRequested` (neu). Fehlercodes: keine
neuen; Abmelde-Fehler nutzen die vorhandenen Codes (Verbindung, Zertifikat, `serverRejected` für
HTTP 4xx/5xx, `invalidArgument` für private Ziele).

## Einstellungs-Sync (für Stufe 11 vorgemerkt)

Neue synchronisierbare Datenarten: `snooze` und `pin` (stehen in `docs/SYNC.md`), dazu `task`
(je Mail, neuere Version gewinnt) und die Absender-Regeln als `rule` mit `"type": "sender"` oder als
eigene Art `senderRule` (je Schlüssel, neuere Version gewinnt). Lokale Regeln sind `rule`.
Geräte-Einstellungen bleiben lokal: `inbox.settings`, `notifications.settings`.

## Rückweg

- Kategorie als Text: Spalte neu anlegen und umschreiben; die Codes stehen nur in
  `MessageCategory::code`.
- Kopie am Ort ersetzen: Index `message(account_id, category, id)` und Prüfung des Orts je Zeile
  (langsamer bei großen Archiven neben kleinem Posteingang).
- Notification-Plugin ersetzen: `notify-rust` direkt hinter demselben `NotificationSink`.
- Launch Services über `objc2` (mit `unsafe`, dann als Teil einer Guard-Crate wie `webview2-guard`).
