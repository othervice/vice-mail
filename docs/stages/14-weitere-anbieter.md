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

- [x] Anmeldung über den System-Browser mit PKCE und lokaler Rückleitung; Prüfung des `state`-Werts gegen untergeschobene Antworten (0042; gegen Stalwart und eine Attrappe getestet, nicht gegen Microsoft)
- [x] Tokens im System-Schlüsselbund, Erneuerung, klare Meldung bei abgelaufener Freigabe, Widerruf beim Entfernen des Kontos (Microsoft bietet öffentlichen Clients keinen Widerruf: dort nur lokales Löschen)
- [x] Erster Schritt: IMAP und SMTP mit OAuth (XOAUTH2). Das nutzt den vorhandenen Adapter. (Dazu OAUTHBEARER, wo ein Server nur das anbietet.)
- [ ] Nur wenn IMAP nicht reicht oder im Mandanten gesperrt ist: eigener Adapter für Microsoft Graph mit Delta-Abgleich, unveränderlichen IDs, Drosselung und Wiederholung (nicht gebaut: bisher nicht nötig, 0042)
- [x] Die Client-ID ist kein Geheimnis und wird nicht so behandelt; ein Client-Secret wird nicht verwendet

Tests: Anmeldeablauf gegen eine Attrappe (gültig, abgelaufen, widerrufen, falscher `state`); Graph gegen aufgezeichnete Antworten; von Hand an einem echten Konto.

## Block C — Gmail mit Google-Anmeldung

Nur wenn die Google-Verifizierung gewollt ist. Ohne sie gilt die Grenze von 100 Nutzern und eine
Warnseite. Vorher vom Menschen: Projekt in der Google Cloud Console, später Verifizierung mit
Domain, Datenschutzerklärung und Demo-Video.

- [x] Anmeldung wie in Block B
- [x] Entscheiden: IMAP mit OAuth (kleiner Schritt) oder Gmail-API mit Abgleich über die History (IMAP mit OAuth, 0042)
- [x] Ablauf und Anforderungen der Verifizierung in `docs/PROVIDERS.md` festhalten

## Block D — JMAP

- [x] Adapter für JMAP (Fastmail, Stalwart). Der Testserver Stalwart spricht JMAP, also dieselben Integrationstests wie für IMAP (0041; gegen Stalwart getestet, Fastmail ungeprüft)
- [x] Push über EventSource statt IDLE

## Für jeden neuen Adapter

- [x] `MailBackend` nur dort erweitern, wo der zweite Adapter es wirklich verlangt; Unterschiede über `Capabilities` ausdrücken, nicht verstecken (JMAP: `MailBackend` unverändert, 0041)
- [x] Die gemeinsame Test-Suite (Abgleich, Aktionen, Senden, Konflikte) läuft gegen den neuen Adapter (JMAP: dieselben Szenarien in eigenen Servertests, siehe Ergebnis)
- [x] Nur-Lese-Schutz gilt auch hier und ist getestet (JMAP)

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

### Blöcke B und C — Anmeldung mit Microsoft und Google

**Stand vom 23.11.2026, abends: gebaut und lokal getestet, ohne die App-Registrierungen des
Menschen; nicht gegen Microsoft oder Google geprüft.** Entscheidung: 0042 (OAuth 2.0 mit PKCE im
System-Browser, IMAP und SMTP mit Token, für Block C IMAP mit OAuth statt Gmail-API). Commits:
a996289 (Adapter: `XOAUTH2`/`OAUTHBEARER`), 7051ce8 (Kern, Shell, Oberfläche, Tests), dd36247
(Ende-zu-Ende-Test gegen Stalwart, `testmail/provision.py`), 844d082 (Dokumentation), 3045eab
(Test der Profile), 404d18b (Einstellungs-Sync). Die Client-IDs sind Build-Konfiguration
(`MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID`, `MAIL_APP_OAUTH_GOOGLE_CLIENT_ID`,
`MAIL_APP_OAUTH_GOOGLE_CLIENT_SECRET`); ohne sie sagt die Einrichtung bei Microsoft, dass dieser
Build sich dort nicht anmelden kann, und bei Gmail bleibt das App-Passwort. Heutige Builds haben
keine.

**Nachgewiesen**
- Gegen den lokalen Stalwart 1.1.1 (er ist selbst ein OAuth-Server: PKCE nur S256, Rotation der
  Refresh-Tokens, Widerruf nach RFC 7009, offene Client-Registrierung):
  `crates/mail-engine/tests/oauth_server.rs` (2 Tests, mit `--include-ignored` und der Sperre
  `oauth`): Registrierung, Anmeldung über Stalwarts Login-API als „Browser“ mit einer
  untergeschobenen Antwort davor (400, danach die echte 200), Konto über `AppCore` mit dem echten
  IMAP-Adapter, Abgleich bis „aktuell“, Erneuern nach Ablauf (Uhr zwei Stunden vor) mit gespeicherter
  Rotation, `XOAUTH2` über STARTTLS und `OAUTHBEARER` über TLS (Mitschnitt), falsches Token
  `auth_failed`, SMTP-Anmeldung und Senden mit `XOAUTH2`, Widerruf beim Entfernen (das alte
  Refresh-Token ist danach tot), Stalwart verlangt den PKCE-Verifier. Grün vor dem Einchecken
  (Worktree auf 9ad59f0 mit diesen Änderungen) und erneut auf 7f5fc8c (alle Commits eingecheckt).
- Attrappe eines Autorisierungsservers im Testprozess mit Browser-Skript (`oauth::tests`, 12
  Tests): Microsoft- und Google-Profil, PKCE am Token-Endpunkt geprüft, Secret nur bei Google,
  untergeschobener `state` zuerst, nur falscher `state`, falscher Pfad, falscher Port (Zeitlimit),
  Zeitlimit, Abbrechen, Ablehnung (`oauth_denied`), nicht verfügbare Anbieter öffnen keinen Browser,
  Erneuern vor Ablauf und nach Ablehnung, Rotation, `invalid_grant` → `oauth_sign_in_required`,
  Netzfehler vorübergehend, Token nur für die Server des Anbieters und nie als Passwort, Widerruf
  (Google ja, Microsoft nichts), Endpunkte und Scopes der Profile.
- `AppCore` mit dem Fake-IMAP-Server (`oauth::tests::core`, 5 Tests): Anmeldung, jeder Login als
  Bearer, nur das Refresh-Token im Schlüsselbund, **nur lesend** (Aktionen `read_only`, keine
  Schreibverbindung), kein Passwortwechsel, fremde Server vor dem Browser abgelehnt,
  Abbrechen/Ablehnen fügt nichts hinzu, abgelaufene Freigabe stoppt den Abgleich nach genau einer
  Erneuerung, „Erneut anmelden“ heilt, Entfernen widerruft und löscht alle Stücke.
- Unit: PKCE (RFC-7636-Vektor), Loopback-Parser, Token-Antworten, Schlüsselbund in Stücken
  (Windows-Grenze), Fehlercodes (`mail-model`), `XOAUTH2`/`OAUTHBEARER` gegen geskriptete IMAP- und
  SMTP-Server (`mail-imap`, 5 Tests), Diagnose ohne Token (`diagnostics::tests`, erweitert),
  Einstellungs-Sync (Datensatz nennt nur den Anbieter, Übernahme als
  `oauth:<Anbieter>:synced:<zufällig>`).
- Oberfläche (`app/tests/oauth-sign-in.test.tsx`, 10 Tests): Microsoft ohne Passwortfeld, Warten
  mit Abbrechen, Fehler mit Grund, nicht verfügbar; Google neben dem App-Passwort oder verborgen;
  Erkennung am Server; „Erneut anmelden“; deutsche Texte. Neue Fehlercodes in `ERROR_CODES` und
  `app/tests/ipc.test.ts`.
- `scripts/check.sh` im Worktree (HEAD 9ad59f0 plus diese Änderungen vor dem Einchecken): grün,
  1.207 Rust-Tests (93 ignoriert), 3.956 Vitest-Tests in 83 Dateien, Clippy auch für Windows, cargo
  deny, pnpm audit, Lizenzliste. Im Hauptverzeichnis nach dem
  Einchecken (alle Commits bis 404d18b, dazu die nicht eingecheckten Änderungen der anderen
  Sitzungen): grün, 1.267 Rust-Tests (96 ignoriert), 3.961 Vitest-Tests in 84 Dateien.

**Abweichungen und Annahmen**
- Microsoft leitet an `http://localhost:<port>` zurück (so lässt sich `http://localhost` im
  Entra-Portal registrieren), alle anderen an `http://127.0.0.1:<port>/`; der Kern lauscht auf
  `127.0.0.1` und, wo möglich, auf `::1`.
- Nur das Refresh-Token liegt im Schlüsselbund, das Access-Token im Speicher (eine Erneuerung je
  App-Start und Konto). Lange Refresh-Tokens werden geteilt (Windows nimmt 2.560 Byte je Eintrag).
- Ohne ID-Token: meldet sich jemand mit einem anderen Konto an als eingegeben, scheitert die
  IMAP-Anmeldung mit `auth_failed`; `login_hint` macht das unwahrscheinlich.
- Microsoft Graph und die Gmail-API sind nicht gebaut (0042).
- Kein Desktop-Test des Anmeldewegs: der Test-Build öffnet nichts (Aufzeichner statt Browser);
  der Weg über Stalwarts Login-API ist im Rust-Test Ende zu Ende abgedeckt, die Zustände der
  Oberfläche in Vitest.
- `testmail/provision.py` bietet `XOAUTH2`/`OAUTHBEARER` jetzt auch für SMTP an; die laufende
  Testserver-Datenbank wurde dafür über die Management-API angepasst.

**Ungeprüft / offen**
- Alles mit echten Konten bei Microsoft (Outlook.com, Microsoft 365) und Google.
- Windows zur Laufzeit (Schlüsselbund-Grenze, `::1`-Listener, Öffnen des Browsers).
- Zwei echte Geräte über den Einstellungs-Sync mit einem Anmelde-Konto.
- Abnahme der Stufe: offen bis zur Handprüfung.

**Wartet auf**
- Mensch: App-Registrierung bei **Microsoft Entra** (kostenlos): Kontotypen „beliebiges
  Organisationsverzeichnis und persönliche Microsoft-Konten“, Plattform „Mobile und
  Desktopanwendungen“ mit Umleitungs-URI `http://localhost` (Typ öffentlicher Client, „Öffentliche
  Clientflows zulassen“ = Ja), delegierte Berechtigungen `IMAP.AccessAsUser.All`, `SMTP.Send`,
  `offline_access`, kein Client-Secret; die Anwendungs-ID als `MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID`
  beim Bauen setzen. Für fremde Mandanten Herausgeberprüfung empfohlen.
- Mensch: **Google-Cloud-Projekt** mit OAuth-Zustimmungsbildschirm „Extern“ (App-Name,
  Support-Adresse, Scope `https://mail.google.com/`), Status „Testing“ mit eingetragenen Testnutzern
  (höchstens 100; deren Refresh-Tokens verfallen nach 7 Tagen), OAuth-Client vom Typ
  **„Desktop-App“**; Client-ID und Client-Secret als `MAIL_APP_OAUTH_GOOGLE_CLIENT_ID` und
  `MAIL_APP_OAUTH_GOOGLE_CLIENT_SECRET`. Für alle Nutzer später die Verifizierung (Domain,
  Datenschutzerklärung, Demo-Video, Sicherheitsbewertung; `docs/PROVIDERS.md`).
- Mensch: Handprüfung mit einem Outlook.com-, einem Microsoft-365- und einem Gmail-Konto
  (Testnutzer): Anmeldung, Abgleich, Senden nach Freischalten, Erneuern nach über einer Stunde,
  „Erneut anmelden“ nach Widerruf beim Anbieter, Entfernen (bei Google danach unter
  myaccount.google.com/permissions verschwunden), eins davon als Sync-Konto.

### Block D — JMAP (23.11.2026)

**Stand: gebaut und gegen den lokalen Stalwart geprüft; Fastmail und Windows ungeprüft.**
Ausgeführt am 23.11.2026 (Subagent der Hauptsitzung), Commits 9d5664b (Adapter `mail-jmap`,
UID-Nummerierung, Protokoll am Konto, Migration `0017_jmap.sql`, Erkennung, Einstellungs-Sync,
Testserver mit HTTPS), 4cb49b6 (Protokollwahl in der Kontoeinrichtung, Desktop-Test), 085f858
(Clippy), da7b3ff (Desktop-Test unabhängig von der Systemsprache), dazu die Doku. Entscheidung: 0041 (JMAP hinter den IMAP-förmigen Traits).

**Nachgewiesen**
- Einheitentests ohne Server (in `check.sh`): `cargo test -p mail-jmap` (32 Tests: Parser mit
  aufgezeichneten Stalwart-Antworten für Sitzung, Postfächer, `Email/get`, EventSource und
  Ablehnungen; Fehlerabbildung; Allowlist; der ganze Adapter gegen einen geskripteten JMAP-Server:
  Erstabgleich mit Paging, Änderungen über `Email/changes`, Neustart mit gespeicherter
  Nummerierung, `cannotCalculateChanges`, Schreiben, Konflikt über den Zählerstand, Push, Senden mit
  Ablehnung, Nur-Lese-Schutz ohne eine einzige Anfrage, `auth_failed`, `jmap_unavailable`,
  `connection_refused`), Speicher `jmap::tests::the_map_round_trips_changes_and_goes_with_the_account`,
  Engine `protocols::tests::jmap_refuses_read_only_writers_and_the_settings_sync_before_connecting`,
  Erkennung `discovery::autoconfig::tests::jmap_is_found_at_the_well_known_address` und
  `autoconfig_files_may_list_jmap`, Modell `account::tests::jmap_settings_use_one_tls_server_and_imap_stays_the_default`.
  Oberfläche: `app/tests/jmap-setup.test.tsx` (4 Tests).
- Servertests gegen Stalwart über HTTPS (`--include-ignored`, Sperre `jmap`):
  im Worktree auf 085f858 gegen den gemeinsamen Stalwart (der beim Start den HTTPS-Listener
  bekam): `cargo test -p mail-jmap --test server` 2 von 2 grün (Erstabgleich von INBOX, Archive,
  `Projects/2026`, `Projects/Überprüfung` mit denselben Mails, Betreffs und Flags wie über IMAP und
  byte-gleichem Body; Push bei einer über JMAP gesendeten Mail; Flag-Änderung über IMAP als
  geändertes Flag; Schreiben, Konflikt über `unchanged_since`, Verschieben mit COPYUID, Löschen,
  Anhängen mit APPENDUID und Suche nach Message-ID; abgelehnte Einreichung ohne Rest; nur lesende
  Verbindungen sendeten nur Lese-Methoden), `cargo test -p mail-engine --test jmap` 1 von 1 grün
  (über `AppCore`: Verbindungstest und Einrichtung eines JMAP-Kontos, Erstabgleich, Nur-Lese-Schutz
  ohne Writer, neue Mail per Push bei zehn Minuten Abfrageabstand, Änderung über IMAP per Push,
  Markieren, Archivieren und Löschen über die Outbox bis auf den Server, Konflikt `gone`, Senden mit
  Gesendet-Kopie, nur Lese-Methoden auf den Leseverbindungen, Neustart ohne neue Auflistung).
  Dazu `cargo test -p mail-imap -- --include-ignored` (alle Adapter- und Servertests, 107) grün
  nach der Änderung am Testserver.
- Desktop-Test (WebdriverIO, echtes WKWebView, `scripts/e2e-gui.sh --spec ./tests/e2e/jmap.e2e.mjs`)
  im eigenen Worktree auf 085f858 mit der Spec aus da7b3ff, Testserver dort frisch eingerichtet:
  2 von 2 grün (JMAP in den Server-Einstellungen gewählt, Verbindungstest meldet Push, Konto
  hinzugefügt, Posteingang zeigt Bobs Mails, Konto nur lesend mit Protokoll `jmap`, keine
  CSP-Verstöße). Die übrigen Desktop-Specs wurden nicht erneut ausgeführt.
- Im Worktree auf 085f858 außerdem: `cargo test -p mail-jmap -p mail-model -p mail-store -p
  mail-engine --lib` (32, 153, 151, 407 grün), Clippy dieser Crates mit `-D warnings`, und
  TypeScript-Prüfung, ESLint und Vitest (84 Dateien, 3.961 Tests) grün.
- Gesamtprüfung: `scripts/check.sh` im Hauptverzeichnis grün (23.11.2026 gegen 20:50 Uhr, auf
  39ff096 mit den unfertigen Änderungen anderer Sitzungen im Arbeitsverzeichnis): alle Schritte bis
  zur Lizenzliste, 1.283 Rust-Tests (96 Servertests ignoriert), 3.967 Vitest-Tests in 84 Dateien,
  Clippy auch für das Windows-Ziel.

**Abweichungen und Annahmen**
- Keine eigene JMAP-Engine: der Adapter bildet UIDs, UIDVALIDITY und HIGHESTMODSEQ nach und
  speichert seine Nummerierung (Tabellen `jmap_*`); siehe 0041.
- `MailBackend` blieb unverändert; dazu kamen `Protocol`/`ConnectConfig::{protocol, account}`,
  `Capabilities::jmap`, `MailConnector::attach_uid_map_store` mit `UidMapStore`,
  `DiscoveryResult::jmap` und der Fehlercode `jmap_unavailable`.
- Der Testserver hat einen HTTPS-Listener (18443) mit `STALWART_PUBLIC_URL`; ein alter Datenbestand
  bekommt ihn beim nächsten `start`.
- Kopfdaten kaputter Mails können sich zwischen IMAP und JMAP im Absender unterscheiden (0041).
- Das Sync-Konto des Einstellungs-Syncs muss IMAP sprechen.

**Ungeprüft / offen**
- Fastmail (vermutlich nur mit API-Token, also nach 0042 als Bearer) und andere JMAP-Anbieter an
  einem echten Konto.
- Windows zur Laufzeit.
- Mehrere Anbieter parallel im vereinten Posteingang mit einem JMAP-Konto: nicht eigens getestet.
- Einstellungs-Sync auf einem JMAP-Konto (nicht umgesetzt, `unsupported_server`).

**Wartet auf**
- Mensch: falls JMAP bei Fastmail gebraucht wird, ein Fastmail-Testkonto; dann zuerst klären, ob
  ein API-Token nötig ist.
