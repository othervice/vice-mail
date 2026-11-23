# Stufe 05 — Verfassen und Senden

**Ziel:** Mails schreiben, beantworten, weiterleiten und senden. Entwürfe gehen nicht verloren, und
keine Mail wird versehentlich zweimal verschickt. Danach ist die App auf dem Mac täglich nutzbar.

**Voraussetzung:** Stufe 04 fertig. Abschnitt „Senden“ in `docs/ARCHITECTURE.md` gelesen.

## Aufgaben

- [x] **Composer.** TipTap mit Grundformatierung (fett, kursiv, unterstrichen, Listen, Zitat, Link) und Umschalter auf reinen Text. Empfänger, CC, BCC als Chips mit Prüfung der Adressen. Autovervollständigung aus Adressen bisheriger Mails. Betreff. Auswahl der Absenderidentität. Eine einfache Text-Signatur je Identität.
- [x] **Anhänge.** Über Dateiauswahl und Drag-and-drop, mit Größenwarnung.
- [x] **Nachricht bauen.** `multipart/alternative` aus Text und mailtauglichem HTML (Stile inline, keine Klassen), bei Anhängen `multipart/mixed`. Kopfzeilen: Message-ID (entsteht beim Anlegen des Entwurfs und bleibt), Date, In-Reply-To, References. Keine Kopfzeile, die das Programm verrät, außer der Nutzer schaltet sie ein. BCC steht nicht in der gesendeten Mail, aber in der Kopie in „Gesendet“.
- [x] **Antworten und Weiterleiten.** Antworten, allen antworten, weiterleiten (mit Anhängen). Zitat des Originals. Die Antwort nimmt die Identität, an die die Mail ging.
- [x] **Entwürfe.** Lokal alle paar Sekunden sichern. In den Entwürfe-Ordner des Servers mit Verzögerung und beim Schließen: neue Fassung anhängen, alte entfernen. Entwürfe aus dem Ordner öffnen, auch solche aus anderen Programmen, so gut es geht. Nach einem Absturz sind ungesendete Entwürfe wieder da.
- [x] **Senden.** Operation `send` in der Outbox. SMTP über `lettre`: STARTTLS (587) oder TLS (465). Zusatz-CA für Tests über `add_root_certificate`; Zertifikatsfehler stecken in der Ursachenkette (0002). Achtung Testserver: Stalwart verwirft eine zweite Mail mit gleicher Message-ID beim Empfänger still (0001). Nach Erfolg Kopie in „Gesendet“, außer der Server legt sie selbst ab (Einstellung je Konto, bei Gmail erkannt).
- [x] **Unklarer Ausgang.** Bricht die Verbindung nach der Übertragung, aber vor der Bestätigung ab: nicht neu senden. Zustand „wird geprüft“, Suche nach der Message-ID in „Gesendet“. Bleibt es unklar, den Nutzer fragen.
- [x] **Senden rückgängig.** Verzögerung vor dem Start (Standard 10 Sekunden, 0 bis 30 einstellbar). Abbrechen führt zurück in den Entwurf.
- [x] **Fehlgeschlagene Sendungen.** Bleiben in der Ansicht „Ausgang“ mit Fehlertext, Wiederholen und Bearbeiten. Dazu ein deutlicher Hinweis in der Oberfläche.
- [x] **Offline.** Senden wird vorgemerkt und beim Verbinden ausgeführt; die Oberfläche zeigt das.

## Abnahme

- [x] Durchgehender Integrationstest: schreiben, senden, beim zweiten Testkonto ankommen, dort antworten, Antwort erscheint im selben Thread-Zusammenhang, archivieren
- [x] Test: Anhang kommt mit gleicher Prüfsumme an; Umlaute und Emoji in Betreff, Namen und Dateinamen bleiben erhalten
- [x] Test mit Fehlerinjektion: Verbindung wird nach der Übertragung gekappt. Beim Empfänger liegt genau eine Mail, oder die App fragt nach. Nie zwei.
- [x] Test: Entwurf überlebt Beenden und Absturz; im Entwürfe-Ordner liegt immer nur die neueste Fassung
- [x] Test: Abbrechen innerhalb der Verzögerung sendet nichts
- [x] Test: BCC-Empfänger erhält die Mail, sein Name steht nicht in den Kopfzeilen der anderen
- [x] Snapshot-Tests für das erzeugte HTML
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- HTML-Signaturen, Vorlagen, Inline-Bilder, Kontaktverwaltung, Später senden (Stufe 10)
- KI-Schreibhilfe (Stufe 13)

## Der Mensch prüft danach

- Vom echten Konto an sich selbst und an eine Adresse bei einem anderen Anbieter senden. Wie sehen die Mails in einem anderen Programm aus?
- Ab hier die App im Alltag benutzen und Reibungspunkte sammeln.

## Ergebnis

Ausgeführt am 22./23.11.2026 (Hauptsitzung mit Subagenten: Nachrichtenbau und SMTP im Adapter,
Entwürfe und Senden in Store und Engine, Composer-Oberfläche, Desktop-Abnahme). Geprüft am 23.11.
von der Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf 5d40e88 (878 Rust-, 780
Vitest-Tests) und auf 255ce74 (947 Rust-, 909 Vitest-Tests in 41 Dateien, cargo deny, pnpm audit);
alle Servertests (`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) grün auf
f81ba9e (950 Tests); auf 255ce74 1.021 von 1.022 grün, rot nur
`ai::tests::qa::qa_set_with_a_real_model`, der ohne lokales Modell scheiterte (seit 66ea375 überspringt er sich ohne Modell);
`pnpm --dir app test:e2e` grün auf 5d40e88 mit den Testständen aus 255ce74 (9 Spec-Dateien, 61
Tests, darunter `compose.e2e.mjs`); auf 255ce74 58 von 61 Tests grün, die Spec dieser Stufe
eingeschlossen; rot sind dort drei Tests in `reading.e2e.mjs` (Stufe 3, Ursache offen).
Entscheidung: 0018 (Entwürfe und Senden).

**Nachgewiesen (Servertests in `crates/mail-engine/tests/compose.rs`, gegen Stalwart)**
- `a_message_goes_out_is_answered_in_the_same_thread_and_archived`: schreiben, senden, beim zweiten
  Testkonto ankommen, dort antworten; die Antwort steht im selben Thread (In-Reply-To, References,
  `\Answered` am Original, genau eine Gesendet-Kopie), danach archiviert.
- `attachments_keep_their_checksum_and_unicode_survives`: Anhang (300 KB) mit gleicher SHA-256;
  Umlaute und Emoji in Betreff, Namen und Dateiname (`Prüfbericht 🧾 Ärger.pdf`) bleiben erhalten,
  keine Kopfzeile nennt das Programm. Im Adapter dazu
  `tests/smtp.rs::arrives_with_attachment_umlauts_and_emoji_over_starttls_and_tls`.
- `a_cut_after_the_data_is_unclear_and_never_sent_twice`: Der Proxy kappt nach dem Datenende. Beim
  Empfänger liegt genau eine Mail, auch nach einem Neustart; „Wiederholen“ wird ohne Bestätigung
  abgelehnt, die App fragt. Legt der Server Gesendetes selbst ab, findet die Prüfung die Kopie und
  fragt nicht. Im Adapter `cut_after_the_end_of_data_is_unclear_and_never_doubled` und
  `cut_during_the_data_is_not_sent`.
- `drafts_survive_quitting_and_a_crash_and_only_the_newest_is_on_the_server`: echter Absturz
  (Kindprozess sichert und ruft `abort()`), Fassungen 1 bis 4, im Entwürfe-Ordner liegt jeweils
  genau eine Kopie.
- `cancelling_within_the_delay_sends_nothing`: 0 Verbindungen und 0 Bytes am Proxy; als
  Positivkontrolle danach genau eine Sendung.
- `a_blind_copy_arrives_and_is_named_nowhere_else`: Der BCC-Empfänger erhält die Mail, die anderen
  sehen ihn nicht, die Kopie in „Gesendet“ nennt ihn. Im Adapter
  `bcc_recipient_gets_the_mail_and_stays_unnamed`.
- `a_read_only_account_keeps_drafts_local_and_refuses_to_send`: Nur lesende Konten behalten Entwürfe
  lokal und senden nichts.
- Erzeugtes HTML und Aufbau (`crates/mail-content`, in `check.sh`): Snapshots
  `compose::tests::mail_html_snapshots`, `reply_quotes_snapshots`,
  `mail_html_is_stable_when_converted_again`; dazu `structure_follows_the_content`,
  `no_header_reveals_the_program_unless_turned_on`, `bcc_only_in_the_sent_copy_and_the_envelope`,
  `message_ids_are_random_and_name_only_the_sender_domain`, `reply_headers_are_written`,
  `reply_and_reply_all_recipients_identity_and_threading`, `forward_takes_the_attachments_along`,
  `hostile_input_cannot_add_headers`.
- Store, Engine, Shell (in `check.sh`):
  `compose_tests::drafts_are_saved_with_revisions_and_frozen_while_sending`,
  `a_draft_survives_a_store_that_was_never_closed`, `an_unclear_send_waits_for_the_user`,
  `a_server_copy_or_a_missing_sent_folder_means_no_append`,
  `compose::tests::gmail_files_sent_mail_itself`; Drag-and-drop nimmt nur Pfade aus Drop-Ereignissen
  des Fensters (`drops::tests::only_recently_dropped_paths_are_accepted_once`); SMTP mit STARTTLS
  und TLS und Zertifikatsprüfung (`smtp::tests::accepted_over_tls_and_starttls`,
  `certificates_are_checked`).
- Offline vorgemerkt und beim Verbinden gesendet: über dieselbe `send`-Operation im Test
  `compose::stage10_tests::offline_at_the_scheduled_time_it_goes_out_once_online` (aus Stufe 10, in
  `check.sh` auf 255ce74).
- In der App (`compose.e2e.mjs`): „Rückgängig“ innerhalb der Verzögerung sendet nichts und bringt
  den Entwurf zurück; eine gesendete Mail kommt genau einmal an, der Hinweis sagt es; ein
  geschlossener Entwurf liegt einmal auf dem Server, auch nachdem er aus dem Entwürfe-Ordner erneut
  bearbeitet wurde; „Ausgang“ listet die Sendung; der Rich-Text-Editor verursacht keinen
  CSP-Verstoß.
- Oberfläche (Vitest, `composer.test.tsx`, `composer-units.test.ts`): Adressen als Chips mit
  Prüfung, Cc/Bcc, Autovervollständigung per Tastatur, Wechsel der Identität mit ihrer Signatur
  (abschaltbar), Anhänge über den Dialog des Kerns mit Größenwarnung und per Drop, Sichern nach der
  Tipp-Pause und beim Verlassen eines Felds, Entwürfe der letzten Sitzung beim Start, Umschalten
  HTML/Text, Antworten mit Zitat, Senden mit „Rückgängig“ und Cmd+Enter, unklare Sendung nur nach
  bestätigter Wahl („angekommen“ schließt ohne Senden ab), fehlgeschlagene Sendung mit Wiederholen,
  Bearbeiten und Ausgang, Ansicht „Ausgang“ mit allen Zuständen (auch vorgemerkt mit
  Verbindungsfehler), Verzögerung einstellbar.

**Abweichungen und Annahmen**
- Lokal sichert die Oberfläche (etwa alle 3 Sekunden beim Tippen und beim Verlassen eines Felds),
  denn nur sie hat den Inhalt. Auf den Server geht der Entwurf als Outbox-Operation `append` nach 30
  Sekunden ohne Sicherung und sofort beim Schließen; die alte Fassung wird erst danach entfernt und
  nur, wenn ihre UID noch die Message-ID des Entwurfs trägt.
- Bei unklarem Ausgang sucht die App die Message-ID in „Gesendet“ nur dort, wo der Server Gesendetes
  selbst ablegt (Einstellung `sentCopy` je Konto, `auto` erkennt Gmail am IMAP-Host); sonst fragt
  sie sofort.
- Migration 6 baut die Tabelle `operation` neu (CHECKs lassen sich in SQLite nicht ändern); Zeilen,
  IDs und der AUTOINCREMENT-Zähler bleiben.
- Anhänge kopiert der Kern beim Hinzufügen; Pfade nimmt er nur aus dem Systemdialog (in Rust) und
  aus Drop-Ereignissen des Fensters. Grenzen 100 Anhänge und 100 MiB, Warnung ab 15 MiB.
- Kontakte für die Autovervollständigung gelten über alle Konten; BCC-Empfänger eigener Mails werden
  nicht gezählt. Das Zitat ist in Stufe 5 nur ein- und ausschaltbar.
- Der eingebettete WebDriver der Desktop-Tests sendet kein `dblclick`; der Test löst es selbst aus.
  Das war kein Fehler der App (im Release-Hinweis des Prototyps noch als möglicher Fehler genannt).
- Neue Konten bleiben nur lesend; Senden setzt den freigeschalteten Schreibzugriff voraus.

**Ungeprüft / offen**
- Windows zur Laufzeit (Systemdialog, Pfade aus Drag-and-drop, Strg+Eingabe). Der Prototyp enthält
  Stufe 5; die Windows-Checkliste hat dafür noch keinen eigenen Abschnitt (nur Schritt 5 im
  Release-Hinweis).
- Echte Server außer Stalwart: die Ablage von Gmail ist nur nachgestellt, Dovecot ohne LITERAL+ und
  APPEND ohne UIDPLUS (Rückfall über die UIDNEXT-Marke ist gebaut, aber nicht gelaufen).
- Entwürfe anderer Programme aus dem Entwürfe-Ordner öffnen (`openDraftFromMessage`): kein Test;
  geprüft ist nur die Bereinigung ihres HTML. Eigene Entwürfe öffnet der Desktop-Test.
- Sofortiges Senden im Offline-Zustand hat keinen eigenen Test; nachgewiesen ist derselbe Weg über
  die geplante Sendung (siehe oben).
- Antworten und Weiterleiten großer, nur teilweise gespeicherter Mails gegen den Server.

**Wartet auf**
- Mensch: vom echten Konto (nach Freischalten des Schreibzugriffs, Empfehlung Nebenkonto) an sich
  selbst und an eine Adresse bei einem anderen Anbieter senden: Wie sehen die Mails in einem anderen
  Programm aus? Ab hier die App im Alltag benutzen und Reibungspunkte sammeln.
- Mensch (Windows): Prototyp `prototype-2026-11-23` (Gitea-Vorab-Release, gebaut aus 5d40e88),
  Schritt 5 des Release-Hinweises: auf einem Nebenkonto Änderungen erlauben und eine Mail an sich
  selbst senden.
