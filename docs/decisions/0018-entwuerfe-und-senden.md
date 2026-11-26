# 0018 — Entwürfe und Senden (Kern und Brücke von Stufe 5)

- **Datum:** 23.11.2026
- **Stufe:** 5 (Backend: Store, Engine, Commands; ohne Oberfläche)
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-store/migrations/0006_compose.sql`, `crates/mail-store/src/{compose,compose_api,contacts}.rs`,
  `crates/mail-model/src/compose.rs`, `crates/mail-engine/src/compose/`, `crates/mail-engine/src/outbox/exec/compose.rs`,
  `crates/mail-imap/src/{writer,command}.rs` (APPEND, Suche nach Message-ID), `app/src-tauri/src/{drops.rs,commands/compose.rs}`

Setzt die Abschnitte „Senden“ und „Outbox“ der Architektur für Stufe 5 um. Hier steht, wo die
Umsetzung vom Plan abweicht oder ihn genauer festlegt.

## Entwürfe

- **Lokal zuerst, gesichert von der Oberfläche.** Den Inhalt hat der Composer, der Kern kann ihn
  nicht abholen; ein Kern-Timer hätte nichts zu sichern. Die Oberfläche ruft `saveDraft` etwa alle
  3 Sekunden während des Tippens (und beim Verlassen eines Felds). Jede Sicherung ist eine kleine
  Schreib-Transaktion (WAL, `synchronous=NORMAL`): nach einem Absturz des Prozesses fehlen höchstens
  die letzten Sekunden. Nachgewiesen mit einem echten Absturz (Kindprozess sichert und ruft
  `abort()`), siehe Nachweise.
- **Drei Teile getrennt** (0016): Text des Nutzers, Text-Signatur der Identität (`includeSignature`,
  wechselt mit der Identität), Zitat (`quoteHtml`/`quoteText`, in Stufe 5 nur ein- und ausschaltbar,
  nicht bearbeitbar). Zusammengesetzt wird beim Sichern auf den Server und beim Senden
  (`mail_content::assemble_draft`; Textmodus gleichwertig im Kern).
- **Server-Sicherung als Outbox-Operation `append`.** Nach 30 Sekunden ohne Sicherung
  (`ComposeConfig::draft_save_delay`) und sofort bei `closeDraft`. Je Entwurf wartet höchstens
  eine Operation; sie baut die neueste Fassung erst beim Ausführen (`\Draft \Seen`, mit `Bcc`,
  ohne Empfänger erlaubt: `mail_content::build_draft_message`). Vor dem APPEND werden UIDNEXT
  des Ordners (Marke) und die zu ersetzende Fassung in der Operation gespeichert. Nach einem
  Abbruch gilt eine Mail mit der Message-ID ab der Marke als die eigene Kopie; APPEND wird nur
  wiederholt, wenn es keine gibt. Die alte Fassung wird erst danach entfernt und nur, wenn ihre
  UID noch die Message-ID des Entwurfs trägt: nie eine fremde Mail.
- **Message-ID ab Anlage fest**, für alle Fassungen und Sendeversuche. Ein Entwurf aus dem
  Entwürfe-Ordner (auch von anderen Programmen, `openDraftFromMessage`) behält seine Message-ID
  und übernimmt die Kopie als Server-Kopie, damit die erste Sicherung sie ersetzt. Ohne
  Message-ID bleibt die fremde Kopie liegen (nicht unsere zu löschen). HTML fremder Entwürfe läuft
  durch die Allowlist (`mail_content::editor_html`), Anhänge werden in den Entwurf kopiert, die
  Signatur steht dann im Text.
- **Nur-Lese-Konten** behalten Entwürfe lokal (`server: localOnly`), ebenso Konten ohne
  Entwürfe-Ordner. Verwerfen eines Entwurfs entfernt seine Server-Kopie über die Outbox
  (`remove_copy`, mit derselben Message-ID-Prüfung); eine Fassung, die beim Verwerfen gerade
  unterwegs war, wird nach ihrer Bestätigung wieder entfernt.

## Senden

- **Einfrieren.** `sendDraft` baut die Nachricht einmal (Datum = Zeitpunkt des Klicks) und legt
  die Bytes als Dateien ab (`compose/outgoing/<Entwurf>/message.eml` ohne `Bcc`, `sent.eml` mit),
  friert den Entwurf ein (`state: sending`, Sicherungen werden mit `send_in_progress`
  abgelehnt) und vermerkt die Operation `send` mit `not_before` = Ende der Verzögerung. Jeder
  Versuch sendet dieselben Bytes. Dieselbe `requestId` zweimal sendet einmal.
- **Senden rückgängig.** Bis `not_before` überspringt die Outbox die Operation (alle anderen
  laufen weiter). `cancelSend` löscht sie in einer Transaktion und gibt den Entwurf frei; der
  Wettlauf mit dem Start ist über den Schreib-Thread entschieden (`begin` setzt nur wartende
  Operationen auf `in_flight`). Danach: `send_in_progress`.
- **SMTP ohne IMAP.** Die Outbox des Kontos sendet `send` selbst, vor dem Aufbau der
  IMAP-Schreibverbindung. Unmittelbar vor dem ersten Byte ist die Operation `in_flight`: ein
  Absturz mittendrin lässt sie `in_flight`, und `in_flight`-Sendungen werden nie neu gesendet,
  sondern geprüft.
- **Ausgänge der Übertragung** (Adapter aus Stufe 5, `SendOutcome`): angenommen → `confirmed`
  mit Folgeoperationen; abgelehnt mit 5xx → `failed` (`send_rejected`); 4xx → zurück nach
  `queued`, höchstens 5 Versuche; nichts übertragen (Verbindung, TLS, Anmeldung) → zurück nach
  `queued` mit dem Fehler, der Worker wartet mit Backoff, ein Wecken (Abgleich verbunden,
  Netzwechsel) versucht sofort: so wird **offline** vorgemerkt und beim Verbinden gesendet.
  Unklar → Prüfung.
- **Unklarer Ausgang.** Nur wo der Server Gesendetes selbst ablegt (Einstellung je Konto
  `sentCopy`: `auto` erkennt Gmail am IMAP-Host, `app`, `server`), sucht die Outbox die Message-ID
  in „Gesendet“ (`SEARCH HEADER Message-ID` plus die neuesten 100 Kopfdaten, weil der Suchindex
  nachhinkt), nach 5, 20 und 60 Sekunden (`ComposeConfig::sent_checks`), Zustand `verifying`.
  Gefunden → gesendet, ohne eigene Kopie. Sonst, und bei allen anderen Servern sofort: `failed`
  mit `send_unclear`, Ereignis `sendNeedsDecision`, Zustand `unclear`. Die Oberfläche fragt;
  `resolveUnclearSend` mit `treatAsSent` (abschließen: Gesendet-Kopie, Entwurf weg) oder
  `sendAgain` (ausdrücklich bestätigt; dieselbe Operation, dieselbe Message-ID). Ein normales
  `retryOperation` auf eine unklare Sendung wird mit `confirmation_required` abgelehnt.
- **Zustände.** `OperationState` bleibt unverändert (keine neuen Werte für Stufe 4). Die Ansicht
  „Ausgang“ bekommt `OutgoingMail` mit abgeleitetem `SendState`: `scheduled` (wartend vor
  `not_before`), `queued`, `sending`, `verifying` (`in_flight` in der Prüfphase), `unclear`
  (`failed` mit `send_unclear`), `failed`, `sent`.
- **Fehlgeschlagene Sendungen** behalten den eingefrorenen Entwurf: „Wiederholen“
  (`retryOperation`) sendet genau das Gescheiterte, „Bearbeiten“ (`cancelSend`) verwirft die
  Operation und gibt den Entwurf frei. Ausschalten des Schreibzugriffs lässt wartende Sendungen
  mit `read_only` scheitern.
- **Nach dem Versand** werden in der bestätigenden Transaktion vermerkt (gleicher Stapel):
  Gesendet-Kopie (`append` der eingefrorenen `sent.eml`, `\Seen`), es sei denn, der Server legt
  selbst ab oder sie lag schon dort; ohne Gesendet-Ordner der Hinweis `no_sent_copy`; Entfernen der
  Entwurfskopie (`remove_copy`); `\Answered` (Antwort) bzw. `$Forwarded` (Weiterleitung) an allen
  Orten des Originals (`set_flag`, gefunden über die Message-ID). Der Entwurf wird gelöscht, seine
  Dateien danach; die Sendedateien, sobald keine offene Operation sie mehr nennt (Aufräumen nach
  jeder Änderung und beim Start, unter einer Sperre, damit gerade geschriebene Dateien bleiben).

## Abweichungen vom Plan

- **Migration 6 baut die Tabelle `operation` neu** (statt nur hinzuzufügen): Die CHECKs auf
  `kind` und `notice` lassen sich in SQLite nicht ändern. Spalten, Zeilen, Ids und der
  AUTOINCREMENT-Zähler bleiben (Test mit gelöschter höchster Id), die Indizes werden gleich wieder
  angelegt; nichts verweist auf `operation`. Neu: `not_before`, `draft_id` (Fremdschlüssel,
  `ON DELETE SET NULL`), Teilindex `operation_send`. Alles andere ist additiv (`draft`, `contact`,
  `message.contacts_seen` mit Teilindex, `identity.signature`, `account.sent_copy`).
- **Sender über `OutboxConfig::sender`** (Trait `mail_model::MailSender`, Produkt: `SmtpSender`)
  statt eines neuen Felds in `CoreServices`: so bleiben die Tests anderer Module, die
  `CoreServices` als Literal bauen, unverändert. Ebenso die Stellschrauben `ComposeConfig`.
- **`ComposeIdentity` statt erweitertem `IdentityInfo`**: die Signatur kommt über
  `listComposeIdentities`/`setIdentitySignature`; die Typen aus Stufe 7 bleiben gleich.
- **Kontakte gelten über alle Konten**; eigene Adressen (alle Identitäten aller Konten, auch
  `+tag`) sind nie Kontakte. Gefüttert wird aus gespeicherten Kopfdaten (gesendet = From ist eine
  eigene Adresse: alle To/Cc; empfangen: der Absender nur bei Kategorie Person nach
  `classify_automatic` auf den gespeicherten Kopfzeilen). Ein Hintergrund-Auftrag liest in Stapeln
  von 500 über den Teilindex `message_contacts_todo`, ausgelöst von `messagesChanged`. **Bcc** der
  eigenen Mails steht nicht in den gespeicherten Kopfdaten und wird nicht gezählt. Suche: jedes
  gefaltete Wort muss den Anfang eines Worts treffen (`instr(search, ' ' || ?)`), entlang des
  Index `contact_rank` mit Abbruch nach `limit` Treffern.

## Anhänge und Drag-and-drop

- Der Kern kopiert jede Datei beim Hinzufügen nach `compose/drafts/<Entwurf>/<n>` (SHA-256 notiert)
  und liest nie wieder den Pfad des Nutzers: Verschieben oder Ändern der Datei danach ändert die
  Mail nicht. Grenzen: 100 Anhänge, zusammen 100 MiB (`attachment_too_large`); die Oberfläche
  warnt ab 15 MiB (`LARGE_ATTACHMENT_WARNING_BYTES`).
- **Pfade nimmt der Kern nur aus zwei Quellen.** `pickAttachments` öffnet den Systemdialog in
  Rust (rfd, wie beim Speichern in Stufe 3) und gibt die Pfade direkt weiter. Für Drag-and-drop
  merkt sich die Shell die Pfade jedes `DragDrop::Drop`-Ereignisses des Fensters (vom
  Betriebssystem, nicht von der Seite) für 2 Minuten; `addDroppedAttachments` nimmt nur Pfade an,
  die so gesehen wurden, jeden einmal (`app/src-tauri/src/drops.rs`). Begründung: Eine Seite, in
  der fremder Code läuft (Fehler, kompromittierte Abhängigkeit), könnte sonst `~/.ssh/…` anhängen
  und versenden. Keine Dateisystem- oder Dialog-Capability für das Frontend.
- **Nachtrag 26.11.2026 (Stufe 23, Entscheidung 0082):** Der Drop-Handler der Shell ist für alle
  Fenster aus, weil er jedes Ziehen in der Seite verschluckte (macOS: kein `dragover`/`drop`,
  Windows: kein HTML-Drag-and-drop). Gezogene Dateien kommen jetzt als Bytes in Stücken
  (`beginFileUpload`, `appendFileUpload`, `addUploadedAttachments`); die Seite nennt nie einen
  Pfad, `DropRegistry` und `addDroppedAttachments` sind entfernt. Begründung, Grenzen und warum das
  mindestens so sicher ist: 0082.

## Weitere Festlegungen

- **SMTP-Test bei der Einrichtung** (0007): `testConnection` prüft nach IMAP die Anmeldung bei SMTP
  (`SmtpSender::verify`, nichts wird gesendet). Fehler tragen `SMTP:` im Detail, damit die
  Oberfläche sagen kann, welcher Server gemeint ist.
- **APPEND im Adapter**: eigene Befehlsschicht mit Literal (`{n+}` bei LITERAL+, sonst mit
  Fortsetzungsanforderung), APPENDUID nur bei UIDPLUS; `[TRYCREATE]` → `MailboxNotFound`. Die
  Nur-Lese-Positivliste lehnt APPEND weiter ab.
- **Neue Fehlercodes**: `send_rejected`, `send_unclear` (jetzt exportiert), `invalid_address`,
  `attachment_too_large`, `send_in_progress`. Neue Ereignisse: `draftsChanged`, `outgoingChanged`,
  `sendNeedsDecision`.

## Nachweise (23.11.2026, Testserver Stalwart 1.1.1, Konten bob, alice, admin)

`cargo test -p mail-engine --test compose -- --include-ignored --test-threads=1`: 8 von 8, dreimal
hintereinander grün (je rund 17–20 s):

| Abnahmepunkt | Test |
|---|---|
| Schreiben, senden, beim zweiten Konto ankommen, antworten, selber Thread, archivieren | `a_message_goes_out_is_answered_in_the_same_thread_and_archived` (auch `\Answered` am Original, In-Reply-To/References, eine Gesendet-Kopie, Autovervollständigung) |
| Anhang mit gleicher Prüfsumme; Umlaute und Emoji in Betreff, Namen, Dateinamen | `attachments_keep_their_checksum_and_unicode_survives` (300 KB, SHA-256, `Prüfbericht 🧾 Ärger.pdf`, keine Programmkennung) |
| Verbindung nach der Übertragung gekappt: genau eine Mail oder Rückfrage, nie zwei | `a_cut_after_the_data_is_unclear_and_never_sent_twice` (Proxy zählt 1 Verbindung, Mitschnitt 1 Datenende, auch nach Neustart; Wiederholen verweigert; danach „angekommen“; Variante mit Server-Ablage ohne Rückfrage) |
| Entwurf überlebt Beenden und Absturz; im Entwürfe-Ordner nur die neueste Fassung | `drafts_survive_quitting_and_a_crash_and_only_the_newest_is_on_the_server` (echter Absturz: Kindprozess `abort()`; Fassungen 1–4, jeweils genau eine Kopie) |
| Abbrechen innerhalb der Verzögerung sendet nichts | `cancelling_within_the_delay_sends_nothing` (0 Verbindungen, 0 Bytes am Proxy; Positivkontrolle: danach 1) |
| BCC-Empfänger erhält die Mail, sein Name steht nicht in den Kopfzeilen der anderen | `a_blind_copy_arrives_and_is_named_nowhere_else` (Gesendet-Kopie nennt ihn) |
| Nur-Lese-Konto | `a_read_only_account_keeps_drafts_local_and_refuses_to_send` |

Dazu `cargo test -p mail-store` (Migration mit Zählerprüfung, Entwürfe, Folgeoperationen,
Rückfragen, Kontakte, Plan-Tests aller neuen Abfragen), `cargo test -p mail-engine -- --include-ignored
--test-threads=1` (alle 212 Tests der Bibliothek, `actions`, `server` mit SMTP-Test in
`testConnection`) und `cargo test -p mail-imap -- --include-ignored --test-threads=1` grün.

## Ungeprüft und bekannte Grenzen

- Echte Server außer Stalwart (Gmail-Ablage nur gespielt: der Test legt die Kopie selbst in
  „Gesendet“), Dovecot ohne LITERAL+, Server ohne UIDPLUS beim APPEND (Rückfall über die Marke
  ist implementiert, aber nur mit UIDPLUS-Server gelaufen).
- Windows (Dateipfade aus Drag-and-drop, Systemdialog).
- Antworten auf große, nur teilweise gespeicherte Mails: das Zitat kommt aus dem Text der
  Leseansicht, Anhänge einer Weiterleitung über den Anhangspfad der Leseansicht (nicht gegen den
  Server getestet).
- Ein Zitat lässt sich in Stufe 5 nicht bearbeiten, nur weglassen.
- Unklar bleibt, ob eine Sendung, die beim Absturz gerade lief, schon übertragen war: sie wird
  geprüft bzw. erfragt, nie still wiederholt; im ungünstigen Fall fragt die App, obwohl nichts
  übertragen war.
