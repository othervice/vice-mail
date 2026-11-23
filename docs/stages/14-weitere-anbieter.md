# Stufe 14 — Weitere Anbieter

**Ziel:** Konten, die mit reinem IMAP und Passwort nicht oder nur schlecht funktionieren, werden
unterstützt. Diese Stufe wird gezogen, sobald ein solches Konto gebraucht wird; die Blöcke sind
einzeln umsetzbar.

**Voraussetzung:** Stufe 05 fertig. Je Block steht dabei, was der Mensch vorher bereitstellen muss.

## Block A — Gmail über IMAP sauber machen

Gmail funktioniert seit Stufe 2 über IMAP mit App-Passwort. Dieser Block behandelt die Eigenheiten.

- [x] Labels statt Ordner: dieselbe Mail unter mehreren Labels über `X-GM-MSGID` als eine Nachricht erkennen; „Alle Nachrichten“ nicht doppelt zählen
- [x] Labels über `X-GM-LABELS` anzeigen und ändern
- [x] Archivieren heißt Label „Posteingang“ entfernen; keine Kopie in „Gesendet“ ablegen, das tut Gmail selbst
- [x] Hinweise in der Kontoeinrichtung: App-Passwort erzeugen, IMAP einschalten
- [x] `docs/PROVIDERS.md` (Englisch): Was wird bei welchem Anbieter unterstützt und was nicht

Kein Test-Server verfügbar: Verhalten über aufgezeichnete Antworten testen und von Hand an einem echten Konto prüfen.

## Block B — Microsoft 365 und Outlook.com

Vorher vom Menschen: eine App-Registrierung bei Microsoft Entra (kostenlos) mit der Client-ID.

- [ ] Anmeldung über den System-Browser mit PKCE und lokaler Rückleitung; Prüfung des `state`-Werts gegen untergeschobene Antworten
- [ ] Tokens im System-Schlüsselbund, Erneuerung, klare Meldung bei abgelaufener Freigabe, Widerruf beim Entfernen des Kontos
- [ ] Erster Schritt: IMAP und SMTP mit OAuth (XOAUTH2). Das nutzt den vorhandenen Adapter.
- [ ] Nur wenn IMAP nicht reicht oder im Mandanten gesperrt ist: eigener Adapter für Microsoft Graph mit Delta-Abgleich, unveränderlichen IDs, Drosselung und Wiederholung
- [ ] Die Client-ID ist kein Geheimnis und wird nicht so behandelt; ein Client-Secret wird nicht verwendet

Tests: Anmeldeablauf gegen eine Attrappe (gültig, abgelaufen, widerrufen, falscher `state`); Graph gegen aufgezeichnete Antworten; von Hand an einem echten Konto.

## Block C — Gmail mit Google-Anmeldung

Nur wenn die Google-Verifizierung gewollt ist. Ohne sie gilt die Grenze von 100 Nutzern und eine
Warnseite. Vorher vom Menschen: Projekt in der Google Cloud Console, später Verifizierung mit
Domain, Datenschutzerklärung und Demo-Video.

- [ ] Anmeldung wie in Block B
- [ ] Entscheiden: IMAP mit OAuth (kleiner Schritt) oder Gmail-API mit Abgleich über die History
- [ ] Ablauf und Anforderungen der Verifizierung in `docs/PROVIDERS.md` festhalten

## Block D — JMAP

- [ ] Adapter für JMAP (Fastmail, Stalwart). Der Testserver Stalwart spricht JMAP, also dieselben Integrationstests wie für IMAP
- [ ] Push über EventSource statt IDLE

## Für jeden neuen Adapter

- [ ] `MailBackend` nur dort erweitern, wo der zweite Adapter es wirklich verlangt; Unterschiede über `Capabilities` ausdrücken, nicht verstecken
- [ ] Die gemeinsame Test-Suite (Abgleich, Aktionen, Senden, Konflikte) läuft gegen den neuen Adapter
- [ ] Nur-Lese-Schutz gilt auch hier und ist getestet

## Abnahme

- [ ] je umgesetztem Block: Tests wie beschrieben, Handprüfung an einem echten Konto im Ergebnis vermerkt
- [ ] mehrere Anbieter parallel im vereinten Posteingang, jede Mail dem richtigen Konto zugeordnet
- [ ] `docs/PROVIDERS.md` nennt nur als unterstützt, was getestet ist
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Exchange über EWS oder MAPI
- Push-Benachrichtigungen über eigene Server

## Ergebnis

**Stand vom 23.11.2026, 13:30: Block A gebaut, Handprüfung an einem echten Gmail-Konto offen;
Blöcke B, C und D nicht begonnen.** Block A ausgeführt am 23.11.2026 (Hauptsitzung mit Subagenten,
in 4343b32: Adapter, Speicher, Outbox, Abgleich und Oberfläche, Migration `0014_gmail.sql`).
Entscheidung: 0034 (Gmail über IMAP). Geprüft von der Hauptsitzung in sauberen Worktrees:
`scripts/check.sh` grün auf a7dd759 (1.102 Rust-, 3.860 Vitest-Tests in 75 Dateien, mit Clippy für
das Windows-Ziel, cargo deny und pnpm audit; darin alle Gmail-Tests, sie brauchen keinen Server);
alle Servertests (`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) auf
a7dd759: 1.185 von 1.186 grün (der eine Fehler gehört zu Stufe 16, behoben in 1b457f1). Die
Prototypen `prototype-2026-11-23-3` (205c0b8, macOS) und `prototype-2026-11-23-3.1` (76d4da2,
Windows) enthalten Block A.

**Nachgewiesen (ohne echtes Gmail-Konto, in `check.sh`)**
- Erkennen: `capabilities::tests::gmail_is_recognised_by_its_extension` (`X-GM-EXT-1`),
  `gmail::tests::gmail_hosts` (Servername), `discovery::tests::workspace_domains_get_the_gmail_hint_from_their_servers`
  (Workspace-Domains über die gefundenen Server).
- Adapter mit aufgezeichneten Gmail-Antworten (`crates/mail-imap/src/tests/gmail.rs`):
  `gmail_headers_and_flags_carry_message_ids_and_labels_read_only` (`X-GM-MSGID` und `X-GM-LABELS`
  mit den Kopfdaten und dem Flag-Abgleich, über EXAMINE), `other_servers_are_not_asked_for_gmail_items`,
  `labels_are_stored_only_on_the_writer_and_encoded_like_mailbox_names` (STORE nur über die
  Schreibverbindung der Outbox, Benutzerlabels in modifiziertem UTF-7), `labels_need_gmail`,
  `read_only_allowlist_and_settings_sync_scope_know_gmail_items` (Nur-Lese-Schutz: STORE steht nicht
  auf der Allowlist, die Verbindung des Einstellungs-Syncs lehnt es ab). Modell:
  `gmail::tests::label_validity`, `mailboxes_imply_labels`, `system_labels_keep_gmails_spelling`,
  `role::tests::gmail_folders_by_name_and_virtual_roles`.
- Eine Mail über Labels und „Alle Nachrichten“ (`crates/mail-store/src/gmail_tests.rs`):
  `one_gmail_message_across_labels_is_one_row_and_counted_once`,
  `labels_follow_the_flag_sync_and_old_rows_get_their_number` (vor dieser Stufe gespeicherte Mails
  bekommen ihre Nummer über die Message-ID), `other_servers_have_no_labels_and_unchanged_merging`.
- Labels ändern über die Outbox: `labels_change_through_the_outbox_and_revert_on_failure`
  (Speicher); Oberfläche `gmail-labels.test.tsx` (Labels im Lesebereich, Änderung über den Kern bei
  freigeschaltetem Schreibzugriff, nichts für andere Server, keine Änderung bei nur lesenden
  Konten, zum Hinzufügen nur gewöhnliche Ordner).
- Archivieren entfernt `\Inbox`: `archive_on_gmail_removes_the_inbox_label_and_undo_goes_through_all_mail`
  (Speicher), `outbox::tests::gmail::archive_on_gmail_removes_the_inbox_label_instead_of_moving` und
  `other_servers_have_no_labels_and_archive_still_moves` (Outbox gegen den Fake-Server);
  Verschieben aus „Alle Nachrichten“ setzt nur das Label:
  `moving_out_of_all_mail_adds_the_label_and_expunges_nothing`. Keine eigene Kopie in „Gesendet“:
  `compose::tests::gmail_files_sent_mail_itself`.
- Einstellungs-Sync auf Gmail: `sync::pass::tests::gmail_settings_sync_records_stay_out_of_all_mail`.
- Kontoeinrichtung: `account-setup.test.tsx` („finds Gmail offline, warns about the app password and
  adds the account read-only“); Gmails Ordner unter `[Gmail]` in der Seitenleiste:
  `folders.test.tsx`.
- `docs/PROVIDERS.md` (Englisch): Gmail als unterstützt mit dem Hinweis, dass die Eigenheiten nur
  mit aufgezeichneten Antworten getestet sind und die Handprüfung aussteht; iCloud, Yahoo, GMX,
  WEB.DE und andere nur als „expected to work“, nicht getestet; Microsoft, Google-Anmeldung und JMAP
  als nicht unterstützt.
- Andere Server unverändert: alle Servertests gegen Stalwart (siehe oben); die neuen Spalten
  bleiben dort leer.

**Abweichungen und Annahmen**
- Gmail wird an `X-GM-EXT-1` erkannt; der Vermerk liegt je Gerät in `account_server` (nicht an
  `account`, dessen Trigger den Einstellungs-Sync füttert). Für Gesendet-Kopie und Hinweise zählt
  zusätzlich der Servername, damit ein neues Konto schon vor der ersten Anmeldung stimmt.
- Auf Gmail führt `X-GM-MSGID` Orte zu einer Mail zusammen, nicht die Message-ID; die Ordnerzähler
  bleiben je Ordner wie bei Gmail. Für andere Server gilt die Regel aus 0003 unverändert.
- Labels erscheinen und ändern sich nur im Lesebereich, nicht in der Liste und nicht bei
  Mehrfachauswahl; ein Label wird wie ein Ordner angelegt, Umbenennen und Löschen nur im Web.
  `X-GM-THRID` wird nicht genutzt, Threads bleiben wie in 0013.
- Label-Änderungen gehen ohne `UNCHANGEDSINCE` und immer zum Server, auch wenn das Label laut
  Server schon so ist.
- Alte Datensätze des Einstellungs-Syncs bleiben in Gmails „Alle Nachrichten“ (im Web sichtbar);
  die App blendet sie aus.
- Migration 0014 statt 0013, weil Stufe 16 die 0013 belegt hat.

**Ungeprüft / offen**
- Alles an einem echten Gmail-Konto: die neun Punkte der Handprüfung in 0034, dazu, ob Gmail das
  Label des geöffneten Ordners in `X-GM-LABELS` weglässt (die App kommt mit beidem zurecht) und ob
  Änderungen im Web über `CHANGEDSINCE` ankommen.
- Mehrere Anbieter parallel im vereinten Posteingang mit einem Gmail-Konto: die Tests des vereinten
  Posteingangs (Stufe 7) laufen nur mit Stalwart-Konten.
- Der Gmail-Hinweis in den Kontoeinstellungen (Labels, Archivieren, Gesendet, Warnung vor zwei
  Kopien bei „App legt ab“) hat keinen eigenen Oberflächentest.
- Verschieben aus einem Gmail-Konto in ein anderes Konto (Stufe 16): das Entfernen der Quelle nimmt
  dort nur das Label; nicht gegen Gmail getestet.
- Windows zur Laufzeit.
- Abnahme der Stufe: offen, bis die Handprüfung vermerkt ist und die Blöcke, die gebraucht werden,
  umgesetzt sind. Für Block A sind die Tests wie beschrieben vorhanden, `docs/PROVIDERS.md` nennt
  nur Getestetes als unterstützt, und `scripts/check.sh` ist grün.

**Wartet auf**
- Mensch: ein echtes Gmail-Konto (Nebenkonto) mit App-Passwort einrichten und die Handprüfung aus
  `docs/decisions/0034-gmail-ueber-imap.md` durchgehen (die Punkte 4 bis 8 brauchen
  freigeschalteten Schreibzugriff); Ergebnis hier vermerken. Prototyp
  `prototype-2026-11-23-3` (macOS) bzw. `prototype-2026-11-23-3.1` (Windows).
- Mensch: für Block B eine App-Registrierung bei Microsoft Entra mit Client-ID, für Block C ein
  Projekt in der Google Cloud Console, sobald diese Konten gebraucht werden.
