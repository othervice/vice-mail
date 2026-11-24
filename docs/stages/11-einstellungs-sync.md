# Stufe 11 — Einstellungs-Sync

**Ziel:** Zwei Geräte mit denselben Konten gleichen Einstellungen, Signaturen, Vorlagen, Regeln,
Ansichten, Snooze und Pins über einen verschlüsselten Ordner im eigenen Postfach ab. Kein Server von uns.

**Voraussetzung:** Stufen 09 und 10 fertig. Abschnitt „Einstellungs-Sync“ in `docs/ARCHITECTURE.md`
gelesen; er ist der Entwurf für diese Stufe.

## Aufgaben

- [x] **Datensätze.** Modell für einen Sync-Datensatz: Art, ID, Version (hybride logische Uhr aus Zeit, Zähler, Geräte-ID), Inhalt, Grabstein. Verzeichnis der synchronisierbaren Datenarten; nur was dort eingetragen ist, wird abgeglichen.
- [x] **Änderungen erfassen.** Tabelle `sync_record` hält je Datensatz die zuletzt abgeglichene Version. Jede Änderung an einer synchronisierbaren Tabelle erhöht die Version.
- [x] **Zusammenführen.** Die Regeln je Datenart aus der Architektur, als reine Funktionen ohne IO.
- [x] **Transport.** Trait `SettingsTransport` (auflisten, schreiben, löschen). Umsetzung über IMAP: eigenen Ordner anlegen und nicht abonnieren; ein Datensatz je Nachricht; ändern heißt neue Fassung anhängen und alte entfernen; Änderungen über den Ordnerstatus erkennen, im normalen Abgleich mitlaufend.
- [x] **Verschlüsselung.** Mit dem Crate `age`: zufälliger Schlüssel, der mit der Passphrase geschützt im Ordner liegt; Datensätze an diesen Schlüssel verschlüsselt. Art, ID und Version stehen zusätzlich im verschlüsselten Teil und werden beim Lesen gegen die Kopfzeilen geprüft. Keine eigene Kryptografie. Nach der Eingabe der Passphrase liegt der entschlüsselte Sync-Schlüssel im System-Schlüsselbund (nicht die Passphrase, siehe `docs/decisions/0014-einstellungs-sync-logik.md`). Die reine Logik (Datensätze, HLC, Zusammenführung, Planung, age-Verschlüsselung, In-Memory-Transport) ist vorgezogen fertig: `crates/mail-engine/src/settings_sync/`, Format in `docs/SYNC.md`.
- [x] **Einrichten.** Sync einschalten (erlaubt Schreiben nur in den Sync-Ordner dieses Kontos, 0014), Sync-Konto wählen, Passphrase setzen (mit Hinweis zur Stärke und zu den Folgen des Vergessens), hochladen.
- [x] **Zweites Gerät.** Nach dem Einrichten des Sync-Kontos erkennt die App den Ordner und fragt nach der Passphrase. Vorschau, was übernommen wird, dann zusammenführen. Übernommene Konten fragen einzeln nach ihrem Passwort.
- [x] **Konto entfernt.** Wurde ein Konto auf einem Gerät entfernt, fragt das andere nach, statt still zu löschen.
- [x] **Status.** Letzter Abgleich, Liste der Geräte (jedes schreibt einen kleinen Datensatz mit Name, System, zuletzt gesehen), offene Rückfragen, „Sync zurücksetzen“.
- [x] **Passphrase.** Ändern (Schlüssel neu schützen). Vergessen: zurücksetzen mit neuem Schlüssel und Neuaufbau von diesem Gerät.
- [x] **Format dokumentieren.** `docs/SYNC.md` (Englisch): Aufbau des Ordners und der Datensätze, Regeln, und was der Mailserver sehen kann (Anzahl, Größe, Zeitpunkte).

## Abnahme

Alle Tests laufen mit zwei Engine-Instanzen mit getrennten Datenbanken gegen den Testserver.

- [x] zwei Geräte ändern verschiedene Einstellungen: beide Änderungen bleiben
- [x] zwei Geräte ändern dieselbe Einstellung: beide enden beim selben Wert
- [x] ein Gerät löscht eine Signatur, das andere bearbeitet sie: die bearbeitete Fassung bleibt
- [x] ein Gerät war lange offline und bringt alte Änderungen mit: neuere Stände bleiben, Grabsteine wirken
- [x] abgelaufene Grabsteine führen nicht zum Wiederauftauchen gelöschter Datensätze
- [x] ein veränderter Datensatz wird verworfen und gemeldet; die übrigen gleichen weiter ab
- [x] eine alte Fassung, erneut eingespielt, wird ignoriert
- [x] Abbruch zwischen Anhängen und Entfernen: die doppelte Fassung wird beim nächsten Lauf aufgeräumt
- [x] falsche Passphrase: klare Meldung, nichts wird überschrieben
- [x] 1.000 Datensätze: erster Abgleich in vertretbarer Zeit (gemessen)
- [x] Test: Passwörter, Tokens und API-Schlüssel tauchen in keinem Datensatz auf
- [x] Test: Der Ausfall des Sync-Kontos beeinträchtigt Lesen und Senden in keinem Konto
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Abgleich von Passwörtern (bleiben gerätegebunden)
- eigener Sync-Server oder Dateiordner als Transport
- Abgleich von Mails, Flags oder Entwürfen (macht der Mailserver)

## Der Mensch prüft danach

- Auf dem Mac einschalten, auf dem Windows-Rechner verbinden. Eine Signatur auf einem Gerät ändern, auf dem anderen eine Mail zurückstellen: Kommt beides an?
- Taucht der Sync-Ordner in einem anderen Mail-Programm auf, und stört er dort?

## Ergebnis

Ausgeführt am 22./23.11.2026 (Hauptsitzung mit Subagenten: Logik, HLC und age-Verschlüsselung als
Grundstein in a4e9739, Integration in Speicher, Adapter und Kern in 55f50c2, Oberfläche in 4dd8c47).
Geprüft von der Hauptsitzung in sauberen Worktrees: `scripts/check.sh` grün auf 8c70d6e (878 Rust-,
780 Vitest-Tests) und auf bd801aa (947 Rust-, 909 Vitest-Tests in 41 Dateien); alle Servertests
(`cargo test --workspace -- --include-ignored` gegen den lokalen Stalwart) grün auf 2437cbb (950
Tests, darunter `tests/settings_sync.rs` 15 von 15); auf bd801aa 1.021 von 1.022 grün, rot nur
`ai::tests::qa::qa_set_with_a_real_model`, der ohne lokales Modell scheiterte (seit d0fc8bf überspringt er sich ohne Modell);
`pnpm --dir app test:e2e` grün auf 8c70d6e mit den Testständen aus bd801aa (9 Spec-Dateien, 61
Tests, darunter `mcp-sync.e2e.mjs`); auf bd801aa 58 von 61 Tests grün, die Spec dieser Stufe
eingeschlossen; rot sind dort drei Tests in `reading.e2e.mjs` (Stufe 3, Ursache offen).
Entscheidungen: 0014 (Logik), 0022 (Integration). Format: `docs/SYNC.md`.

**Nachgewiesen (zwei Engines mit getrennten Datenbanken gegen Stalwart, `crates/mail-engine/tests/settings_sync.rs`)**
- `different_settings_on_two_devices_both_stay`: verschiedene Einstellungen, beide bleiben.
- `the_same_setting_on_two_devices_ends_equal`: dieselbe Einstellung, beide enden beim selben Wert.
- `a_deleted_signature_edited_elsewhere_keeps_the_edit`: gelöscht gegen bearbeitet, die bearbeitete
  Fassung bleibt.
- `a_device_back_after_long_offline_keeps_newer_states_and_applies_tombstones`: alte Änderungen
  eines lange getrennten Geräts; neuere Stände bleiben, Grabsteine wirken.
- `expired_tombstones_do_not_bring_deleted_records_back`.
- `a_modified_entry_is_rejected_and_reported_while_the_rest_syncs`: veränderter Datensatz wird
  verworfen und gemeldet, die übrigen gleichen weiter ab.
- `an_old_version_played_back_is_ignored`.
- `an_interruption_between_append_and_delete_is_cleaned_up_next_time`.
- `a_wrong_passphrase_is_reported_and_changes_nothing`: klare Meldung, nichts wird überschrieben.
- `a_thousand_records_first_pass_is_measured`: 1.003 Einträge (1.000 Absender-Regeln plus Konto,
  Identität, Gerät), Debug-Build: Einschalten mit erstem Durchlauf und 1.003 APPENDs 0,76 s,
  Beitreten des zweiten Geräts (Vorschau und erster Durchlauf) 1,9 s, Durchlauf ohne Änderungen 37
  ms, tägliche Vollprüfung 0,47 s (`docs/SYNC.md`, Measurements).
- `an_outage_of_the_sync_account_leaves_reading_and_sending_alone`: Ausfall des Sync-Kontos (Proxy)
  beeinträchtigt Lesen und Senden nicht.
- Dazu `an_account_removed_on_one_device_is_asked_about_on_the_other` (Rückfrage statt stillem
  Löschen; übernommene Konten nennen ihr fehlendes Passwort),
  `the_second_device_follows_without_being_asked`, `passphrase_change_and_reset`,
  `after_a_restart_the_sync_goes_on_with_the_stored_key_and_clock`.
- Keine Geheimnisse:
  `settings_sync::tests::scenarios::passwords_tokens_and_api_keys_appear_in_no_record`
  (Inhaltsmodell, versiegelter Datensatz mit Kopfzeilen und Chiffretext, entschlüsselter Inhalt),
  `record::tests::content_refuses_secret_fields_and_key_material`,
  `tests::capture::captured_content_of_every_table_passes_its_kind`.
- Schreiben nur in den Sync-Ordner: `command::tests::folder_scope_writes_only_to_the_sync_folder`,
  gegen Stalwart mit Mitschnitt
  `tests/sync_folder.rs::sync_folder_connection_writes_only_to_its_folder` (Schreibversuche
  außerhalb erreichen den Draht nicht); in der App „turns on with a passphrase and writes only its
  own folder“.
- Logik ohne IO (in `check.sh`): Datensätze und Verzeichnis (`record::tests::*`), HLC
  (`hlc::tests::*`), Zusammenführung (`merge::tests::*`,
  `convergence::two_devices_converge_after_random_histories`,
  `three_devices_converge_after_random_histories`), age (`crypto::tests::*`), Erfassung
  (`settings_sync_tests::every_write_path_marks_its_record_in_the_same_transaction`,
  `capture::every_registered_kind_is_captured_when_it_changes`), Erkennen des Ordners
  (`settings_sync::tests::recognises_the_folder_at_the_top_and_below_inbox`).
- Oberfläche (Vitest `settings-sync.test.tsx`, `mcp-sync-ipc.test.ts`): Einschalten mit zweimal
  getippter Passphrase und Hinweis, dass nur in den Sync-Ordner geschrieben wird; Beitreten mit
  Vorschau und falscher Passphrase; Passwörter übernommener Konten; Status und Geräte; Rückfrage bei
  entferntem Konto (behalten, oder hier auch entfernen erst nach zweitem Klick); Gerät umbenennen;
  Passphrase ändern; Zurücksetzen nach Bestätigung; Ausschalten. In der App „renames the device,
  syncs now and turns off“.

**Abweichungen und Annahmen**
- Änderungen erfassen Trigger auf jeder synchronisierbaren Tabelle und ein Haken der Engine vor dem
  Commit; kein Schreibweg muss vom Sync wissen.
- Ordner `Settings-Sync` (ASCII, oberste Ebene oder unter `INBOX`), nicht abonniert; eine Nachricht
  je Fassung, Körper in age-ASCII-Armor, angehängt mit `\Seen`.
- Signatur als eigener Datensatz (nur so schlägt Bearbeiten das gleichzeitige Löschen);
  Absender-Regeln über ihren Schlüssel; Snooze nur mit `{untilMs}`.
- Benachrichtigungs-Einstellungen werden abgeglichen (Abweichung von 0020); `inbox.settings`,
  `prefetch.*` und die KI-Einstellungen (Stufe 13, 0024) bleiben auf dem Gerät.
- Zweites Gerät: Ein nie abgeglichenes lokales Konto mit derselben Adresse übernimmt die UUID aus
  dem Ordner. Übernommene Konten starten nur lesend und ohne Passwort.
- Konto entfernt: Das andere Gerät fragt nur für das Konto und hält bis zur Antwort auch die
  Löschungen der zugehörigen Datensätze zurück.
- scrypt-Arbeitsfaktor in den Tests 10, im Produkt 18.
- Der Test „keine Geheimnisse“ läuft gegen den In-Memory-Transport, nicht mit zwei Engines gegen
  Stalwart.

**Ungeprüft / offen**
- Ordner unter `INBOX` (Cyrus/Courier) nur als Logik getestet, kein solcher Testserver.
- Zwei gleichzeitig auf zwei Geräten angelegte und schon abgeglichene Konten mit derselben Adresse
  werden nicht verschmolzen (bewusst offen, wird geloggt).
- Windows zur Laufzeit, echte Mailserver, Sichtbarkeit des Ordners in anderen Mail-Programmen.

**Wartet auf**
- Mensch: auf dem Mac einschalten, auf dem Windows-Rechner verbinden (der Prototyp
  `prototype-2026-11-23` enthält Stufe 11; die Windows-Checkliste hat dafür noch keinen Abschnitt).
  Eine Signatur auf einem Gerät ändern, auf dem anderen eine Mail zurückstellen: Kommt beides an?
  Taucht der Sync-Ordner in einem anderen Mail-Programm auf, und stört er dort?
