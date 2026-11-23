# 0022 — Einstellungs-Sync: Integration in Speicher, Adapter und Kern

- **Datum:** 23.11.2026
- **Stufe:** 11 (Kern, Speicher, Shell; die Oberfläche folgt in einer eigenen Sitzung)
- **Status:** entschieden von der ausführenden Sitzung
- **Baut auf:** 0014 (Logik, Kryptografie, Schreibzugriff nur auf den Sync-Ordner), 0016, 0020
- **Code:** `crates/mail-store/migrations/0009_settings_sync.sql`,
  `crates/mail-store/src/settings_sync.rs`, `crates/mail-imap/src/{command,sync_folder}.rs`,
  `crates/mail-model/src/settings_sync.rs`,
  `crates/mail-engine/src/settings_sync/{local,imap,service}.rs`,
  `app/src-tauri/src/commands/settings_sync.rs`
- **Tests:** `crates/mail-engine/tests/settings_sync.rs` (zwei Engines gegen Stalwart),
  `crates/mail-imap/tests/sync_folder.rs` (WireTap), `crates/mail-store/src/settings_sync_tests.rs`,
  `crates/mail-engine/src/settings_sync/tests/capture.rs`

Hier steht, was die Integration festlegt oder vom Entwurf in `docs/ARCHITECTURE.md` und
`docs/SYNC.md` abweicht. Das Format selbst steht in `docs/SYNC.md`.

## Änderungen erfassen: ein Haken im Store

- **Trigger markieren, ein Haken versioniert.** Migration 0009 legt auf jede synchronisierbare
  Tabelle Trigger (INSERT, UPDATE, DELETE), die den Datensatz in `sync_dirty` vormerken.
  `Store::write` ruft am Ende jeder Schreibtransaktion, vor dem Commit, den Haken
  `SyncCapture` der Engine auf: Der Store liest den Inhalt jedes vorgemerkten Datensatzes aus
  seiner Tabelle (`content_of`) und die gespeicherte `sync_record`-Zeile, die Engine führt die
  Änderung durch `LocalState::put`/`put_fields`/`delete` mit der Uhr des Geräts und gibt die
  neuen Zeilen zurück. Änderung und Version werden zusammen festgeschrieben oder gar nicht.
  Kein Schreibweg muss vom Sync wissen; ein neuer Schreibweg ist automatisch erfasst.
- **Grob markieren, genau vergleichen.** Die Trigger markieren jede Zeilenänderung; ob sich
  etwas Synchronisiertes geändert hat, entscheidet der Vergleich in `LocalState::put`
  (gleicher Inhalt behält seine Version). So erzeugen `writes_enabled`, ein zurückgekehrter
  Snooze oder ein Schlagwort-Spiegel keine Version. Nur `contact` filtert schon im Trigger
  (`user_name`), weil sich die Zähler bei jedem Abgleichsfenster ändern.
- **`NOT EXISTS` statt `INSERT OR IGNORE`** in den Triggern: Die Konfliktklausel der
  auslösenden Anweisung (Upsert, `INSERT OR REPLACE`) überschreibt die des Triggers; ein
  Upsert auf `setting` scheiterte daran.
- **Bestand:** Die Migration markiert alle vorhandenen Zeilen; die Engine erfasst sie beim
  ersten Start, sobald der Haken gesetzt ist (vor allen anderen Schreibern).
- **Was nicht erfasst werden darf, wird nicht erfasst:** Geräte-Einstellungen (nicht in
  `SYNCED_SETTINGS`), Inhalt, den die Registry ablehnt (Geheimnis-Felder, zu groß). Die Änderung
  bleibt dann lokal; geloggt wird der Grund, nie der Wert.

## Eine Uhr, nur kurz gehalten

Die hybride Uhr des Geräts liegt in einem `std::sync::Mutex`, geteilt vom Haken (Schreib-Thread
der Datenbank) und den Durchläufen. Gehalten wird sie nur in synchronen Schritten, nie über IO:
`prepare_pass` ist dafür in `fetch_pass` (auflisten, laden, öffnen; ohne Uhr) und
`FetchedPass::plan` (zusammenführen; mit Uhr) geteilt. Schlüsseloperationen (Einrichten,
Passphrase, Zurücksetzen) laufen mit einer vorübergehenden Uhr, die danach in die geteilte
eingeht; ihre Versionen betreffen nur den Schlüsseldatensatz. Der höchste ausgegebene oder
gesehene Stand liegt in `sync_state.hlc_last` und wird mit jeder Erfassung und jedem Durchlauf
geschrieben.

## Ergebnis eines Durchlaufs anwenden

- **Gegen gleichzeitige lokale Änderungen:** Jeder Datensatz wird nur geschrieben, wenn seine
  `sync_record`-Version noch die ist, gegen die der Durchlauf geplant hat; sonst wird er
  übersprungen und der nächste Durchlauf führt beide zusammen. Die Serverseite des
  Durchlaufs (hochladen, löschen) bleibt dabei richtig: Der nächste Durchlauf lädt die neue
  lokale Fassung hoch und löscht die alte.
- Ein Sicherungspunkt je Datensatz: Ein abgelehnter (Adresse schon vergeben, Konto unbekannt)
  hält die übrigen nicht auf. Konten zuerst, dann Identitäten, dann der Rest.
- Angewendete Datensätze hinterlassen keine Markierung, also keine neue Version. Folgen in
  anderen Datensätzen (eine andere Identität verliert „Standard“, eine Gruppe ein Mitglied)
  werden wie jede Änderung erfasst.
- Datensätze von Konten, die es hier (noch) nicht gibt, werden übersprungen und beim nächsten
  Durchlauf wieder geladen.

## Der Ordner

- **Name `Settings-Sync`** (`mail_model::SETTINGS_SYNC_FOLDER`): nur ASCII ohne
  Hierarchie-Trenner, Leerzeichen oder führenden Punkt (kein modifiziertes UTF-7, keine
  Ebenen, unter Maildir++ kein leerer Elternordner), ohne Produktnamen, für Menschen in einem
  anderen Mail-Programm verständlich. Oberste Ebene, oder `INBOX<Trenner>Settings-Sync`, wenn
  alle vorhandenen Ordner des Kontos unter `INBOX` liegen (Cyrus/Courier-Art; ungeprüft, kein
  solcher Testserver).
- **Nicht abonniert:** CREATE, danach UNSUBSCRIBE (falls der Server neue Ordner selbst
  abonniert). Der Mail-Abgleich der App lässt den Ordner aus der Ordnerliste weg; dass er ihn
  sieht, meldet er dem Einstellungs-Sync („Ordner erkannt“, `detectedAccounts`).
- **Eine Nachricht je Fassung:** Kopfzeilen `X-Settings-Sync-Id` und `X-Settings-Sync-Version`,
  dazu `Date`, ein fester Betreff („Settings sync data (encrypted, do not edit)“) und
  `text/plain`. Der Körper ist der age-Chiffretext in age-ASCII-Armor (Feature `armor` des
  vorhandenen Crates `age`, keine neue Abhängigkeit): 7-Bit-Text, den jeder Server unverändert
  speichert, mit CRLF. Angehängt mit `\Seen`, damit andere Programme keine ungelesenen
  Nachrichten zählen.
- **Fremde Nachrichten bleiben unsichtbar:** Aufgelistet werden nur Nachrichten mit beiden
  Kopfzeilen; nur diese kann der Sync löschen, auch beim Zurücksetzen.
- **Handle = UID.** Ein Transport lebt für einen Durchlauf auf einem ausgewählten Ordner, die
  UIDs können sich darunter nicht ändern. Innerhalb eines Durchlaufs geladene Körper werden
  zwischengespeichert (siehe Zweites Gerät).

## Schreibzugriff nur auf diesen Ordner (0014)

Im Befehlsschicht des Adapters (`mail-imap/src/command.rs`, `FolderScope`): Die Verbindung
meldet sich wie eine nur lesende an und wird dann auf den Ordner beschränkt. Neben der
Nur-Lese-Liste lässt sie genau diese Zeilen durch: `SELECT "<Ordner>"` (auch mit
`(CONDSTORE)`), `CREATE "<Ordner>"`, `UNSUBSCRIBE "<Ordner>"`, `APPEND "<Ordner>" ()` oder
`(\Seen)`, sowie `UID STORE <UIDs> +FLAGS.SILENT (\Deleted)`, `UID EXPUNGE <UIDs>` und
`EXPUNGE` nur, solange der Ordner per SELECT ausgewählt ist (ein EXAMINE oder ein
fehlgeschlagenes SELECT beendet das). Alles andere scheitert mit `read_only`, bevor ein Byte
gesendet wird, unabhängig von `writes_enabled`. Geprüft wird die exakte Zeile, wie bei der
Nur-Lese-Liste. Nachweis: Unit-Test der Zeilen und `tests/sync_folder.rs` gegen Stalwart mit
WireTap (Schreibversuche in INBOX, CREATE, DELETE, COPY, MOVE, STORE nach EXAMINE erreichen den
Draht nicht; die eigenen Schreibbefehle als Positivkontrolle).

Die Engine bekommt die Verbindung über einen dritten Adapter-Trait
`mail_model::SyncFolderConnector` (`Connector` = Lesen + Schreiben + Sync-Ordner); die
Test-Fakes ohne Ordner melden `unsupported`.

## Verzeichnis der Arten

Neu: `task`, `senderRule`, `accountGroup`, `remoteContent`, `contact` (am Ende der Liste,
Reihenfolge der alten unverändert). Inhalt je Art strikt typisiert, siehe `docs/SYNC.md`.

- **Signatur als eigener Datensatz** (`signature`, ID = UUID der Identität), nicht als Feld der
  Identität: Nur so gilt „Bearbeiten schlägt gleichzeitiges Löschen“ für die Signatur (die
  Abnahme verlangt es). Inhalt `{text?, html?}`; `html` ist für die HTML-Signaturen des
  Composers vorgesehen, heute wird nur `text` geschrieben.
- **Identität** `{account, email, name, replyTo, isDefault}`.
- **Absender-Regeln über ihren Schlüssel** (`address:<v>`, `domain:<v>`, `list_id:<v>`), nicht
  über die UUID der Zeile: Der Schlüssel ist lokal eindeutig; zwei Geräte, die denselben
  Absender einordnen, ändern so denselben Datensatz statt zwei, die beim Anwenden kollidieren.
- **Snooze nur `{untilMs}`**: Ob er zurückgekehrt ist, hält jedes Gerät selbst (jeder Zeitgeber
  holt ihn zurück); sonst erzeugte jede Rückkehr auf jedem Gerät eine Version.
- **Einstellungen:** `ui.theme`, `ui.language`, `reading.mark_read_on_open`,
  `compose.send_delay_seconds`, `notifications.settings`. **Abweichung von 0020:** Dort waren
  die Benachrichtigungs-Einstellungen Geräte-Einstellung; auf Vorgabe der Hauptsitzung werden
  sie abgeglichen (stummgeschaltete Konten über die Konto-UUID, Ruhezeiten, Inhalt ausblenden).
  `inbox.settings` und `prefetch.*` bleiben Gerät.
- **Kontakte:** nur der vom Nutzer gesetzte Name (`{userName}`). Zusammenführungen gibt es noch
  nicht (Stufe 10). Grenze: Ein übernommener Name aktualisiert die Suchwörter des Kontakts erst,
  wenn der Kontakt das nächste Mal beobachtet wird (die Faltung liegt in `mail-content`, der
  Store kann sie nicht); ein Kontakt, den dieses Gerät nie gesehen hat, bekommt keinen Namen.
- **Kontogruppen** mit Mitgliedern über die Konto-UUID; unbekannte Mitglieder fehlen hier.
- **Ohne lokale Tabelle** (`template`, `savedView`, `aiSettings`): Datensätze anderer Geräte
  bleiben in `sync_record` erhalten, damit eine spätere Version sie hat.
- **Geräte:** Tabelle `sync_device` (Art `device`), jedes Gerät schreibt seine Zeile beim
  Einschalten und danach höchstens täglich. Name voreingestellt nach System („Mac“,
  „Windows PC“), änderbar (`setSyncDeviceName`), System z. B. „macOS (aarch64)“.

## Zweites Gerät: dasselbe unter zwei IDs

Auf dem zweiten Gerät richtet der Mensch das Sync-Konto vorher selbst ein (sonst gibt es keinen
Ordner zu finden), mit eigener Konto-UUID und eigener Standard-Identität. Beim Beitreten:

- Ein lokales Konto mit der Adresse eines Kontos im Ordner, das noch nie abgeglichen wurde,
  übernimmt die UUID aus dem Ordner (`rekey_account`: Konto-UUID, alle Verweise per UUID,
  UUIDs in Regel-Definitionen und Einstellungswerten; Fremdschlüssel per
  `PRAGMA defer_foreign_keys`). Schlüsselbund-Eintrag, Mails und lokale Daten bleiben; die
  Inhalte aus dem Ordner gelten. Laufende Abgleich- und Outbox-Aufgaben starten unter der
  neuen UUID.
- Ebenso eine nie abgeglichene lokale Identität mit gleichem Konto und gleicher Adresse.
- Ablauf: planen, Treffer umschlüsseln, neu planen (höchstens drei Runden; geladene Körper
  werden im Durchlauf nicht erneut geholt).
- Zwei schon abgeglichene Konten mit gleicher Adresse (gleichzeitig auf zwei Geräten
  angelegt) werden nicht verschmolzen: Der Datensatz aus dem Ordner wird nicht angewendet und
  geloggt. Selten; bewusst offen gelassen.
- **Übernommene Konten** starten nur lesend, ohne Passwort und ohne eigene Standard-Identität
  (die Identitäten kommen aus dem Ordner). `sync_adopted` merkt sie; der Status nennt sie
  (`accountsNeedingPassword`), `updatePassword` löscht die Markierung. Ihr Abgleich steht bis
  dahin im Zustand `error` (Schlüsselbund).
- **Vorschau:** `previewSettingsSyncJoin` prüft die Passphrase, plant gegen die lokalen Daten
  wie ein neues Gerät und schreibt nichts; der Schlüssel wartet im Speicher auf
  `joinSettingsSync`. Nach einem Neustart ist die Vorschau weg.

## Konto entfernt

Entfernt ein Gerät ein Konto, entstehen Grabsteine für das Konto und (über die Kaskade) für
seine Identitäten, Signaturen, Snoozes, Pins, Aufgaben und Bild-Freigaben. Das andere Gerät
fragt nur für das Konto (`SyncQuestion::AccountRemoved`) und **hält bis zur Antwort auch die
Löschungen der zugehörigen Datensätze zurück**. „Behalten“ speichert das Konto und alle diese
Datensätze unter neuen Versionen (sie schlagen die Grabsteine), sodass sie überall
zurückkommen; „hier auch entfernen“ übernimmt den Grabstein und entfernt das Konto wie von
Hand. Die Fragen werden in jedem Durchlauf neu bestimmt (nicht gespeichert). Wird das
Sync-Konto selbst entfernt, schaltet sich der Sync auf diesem Gerät ab.

## Durchläufe, Verbindung, Ausfall

- Ein Arbeiter-Task je Kern. Er läuft nach einer lokalen Änderung (2 s später, damit eine
  Serie ein Durchlauf ist), nach jedem Durchlauf des Mail-Abgleichs des Sync-Kontos und im
  Poll-Abstand des Mail-Abgleichs (3 min). „Im normalen Abgleich-Takt mitlaufend“ heißt also:
  am Ende jedes Mail-Durchlaufs des Sync-Kontos, auf eigener Verbindung.
- **Nichts zu tun erkennen:** Zustand des Ordners (UIDVALIDITY, UIDNEXT, EXISTS,
  HIGHESTMODSEQ) beim SELECT am Anfang jedes Durchlaufs. Gleich wie beim letzten, nichts lokal
  offen und keine zurückgestellten Einträge: fertig. Gespeichert wird der Stand vom Anfang, also
  folgt auf einen schreibenden Durchlauf ein stiller.
- Einmal täglich `PassMode::FullCheck`.
- Eine eigene Verbindung, zwischen den Durchläufen gehalten; eine vom Server geschlossene wird
  einmal neu aufgebaut. Ihr Ausfall setzt nur den Status auf `error`; Lesen und Senden laufen
  über ihre eigenen Verbindungen und warten nie auf den Sync (Test mit Proxy).
- Status-Ereignis `settingsSyncChanged`, wenn ein Durchlauf etwas getan hat oder sich sein
  Ergebnis ändert; übernommene Daten melden die üblichen Ereignisse (`preferencesChanged`,
  `accountsChanged`, `inboxChanged`).

## Fehler und Status

- Neue Fehlercodes: `sync_wrong_passphrase`, `sync_not_set_up`, `sync_already_set_up`,
  `sync_key_changed`, `sync_key_invalid`, `sync_key_missing`, `sync_off`
  (`CoreError::SettingsSync`). Sie brauchen Texte in `app/src/i18n/` (Oberflächen-Sitzung).
- Zustände: `off`, `active`, `needsPassphrase` (Schlüssel fehlt im Schlüsselbund:
  `sync_key_missing`, oder anderswo zurückgesetzt: `sync_key_changed`), `error` (letzter
  Durchlauf gescheitert, mit `lastError`).
- Fragen und abgelehnte Einträge werden nicht gespeichert; der erste Durchlauf nach dem Start
  überspringt deshalb nie (auch bei unverändertem Ordner).
- Passphrase: mindestens 8 Zeichen (`MIN_SYNC_PASSPHRASE_CHARS`); den Hinweis zur Stärke zeigt
  die Oberfläche.
- Der scrypt-Arbeitsfaktor ist konfigurierbar (`SyncConfig::settings_sync_work_factor`,
  Produkt 18, Tests 10), damit die Tests nicht je 13 s im Debug-Build rechnen.

## Rückweg

- Erfassen ohne Trigger: Jeder Schreibweg meldet seine Datensätze selbst (verstreut, war nicht
  gewollt).
- Ordnername: Eine Konstante; ein Umzug bräuchte Erkennung beider Namen.
- Schlüssel der Absender-Regeln: wieder UUID mit Abgleich über den Schlüssel beim Anwenden.
