# 0040 — Server-Änderungen aus dem Einstellungs-Sync warten auf den Nutzer

- **Datum:** 23.11.2026
- **Anlass:** Sicherheitsprüfung Stufe 15, Befund F1 (`docs/SECURITY_MODEL.md`, 7.1). Ein
  Konto-Datensatz aus dem Sync-Ordner, der Host, Port, Sicherheit oder Benutzername von IMAP oder
  SMTP eines vorhandenen Kontos ändert, wurde unverändert übernommen (auch wenn ein lokales Konto
  beim Beitreten über die Adresse zugeordnet wurde). Danach ging das gespeicherte Passwort an den
  neuen Server, und `writes_enabled` blieb, wie es war. Wer den Sync-Schlüssel hat (ein
  kompromittiertes Gerät, oder der Anbieter des Sync-Kontos nach dem Knacken einer schwachen
  Passphrase), bekam so die Passwörter aller anderen Konten auf allen Geräten.
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-store/migrations/0016_account_server_change.sql`,
  `crates/mail-store/src/settings_sync.rs` (`put_table` für `account`, `content_of`,
  `account_server_changes`, `confirm_account_server_change`, `dismiss_account_server_change`),
  `crates/mail-engine/src/core.rs` (`apply_server_change`),
  `crates/mail-engine/src/settings_sync/service.rs` (`resolve_server_change`, Status),
  `crates/mail-model/src/settings_sync.rs` (`SyncServerChange`, `SettingsSyncStatus.serverChanges`),
  `app/src-tauri/src/commands/settings_sync.rs` (`resolve_server_change`),
  `app/src/features/settings/accounts/AccountDetail.tsx` (`ServerChangeNotice`)
- **Tests:** `crates/mail-engine/src/settings_sync/tests/server_change.rs` (vier Fälle über
  `AppCore` mit einem Connector, der jede Anmeldung mit Host und Passwort aufzeichnet, und einem
  Sync-Ordner im Speicher; ein Angreifer mit dem Sync-Schlüssel schreibt in denselben Ordner),
  `crates/mail-store/src/settings_sync_tests.rs` (`other_servers_from_the_folder_wait_for_the_user`,
  `adopted_accounts_get_a_fresh_keychain_reference`), `app/tests/server-change.test.tsx`

Ergänzt 0014 und 0022 (Einstellungs-Sync). `writes_enabled` wurde schon nie abgeglichen; jetzt
gilt dasselbe für die Frage, an welchen Server das Passwort dieses Geräts geht.

## Grundsatz

**Die Server eines Kontos sind eine Grenze für Zugangsdaten.** Das Passwort, das der Nutzer auf
diesem Gerät eingegeben hat, gehört zu den Servern, für die er es eingegeben hat. Kein Datensatz aus
dem Ordner kann es an andere Server schicken. Andere Server werden erst nach einer Entscheidung des
Nutzers verwendet, und dann ohne das alte Passwort.

## Was ein Durchlauf tut

- **Vorhandenes Konto, gleiche Server:** wie bisher, alle Felder werden geschrieben.
- **Vorhandenes Konto, andere Server** (irgendein Unterschied in Host, Port, Sicherheit oder
  Benutzername von IMAP oder SMTP, exakt verglichen): Adresse, Name und Farbe werden geschrieben,
  die Server bleiben. Die Server aus dem Datensatz kommen in die neue Tabelle
  `account_server_change` (Klasse Gerät). Der Durchlauf startet das Konto nicht neu; seine
  Verbindungen bleiben, wie sie sind. Das gilt auch für ein lokales Konto, das beim Beitreten
  über die Adresse die ID aus dem Ordner übernimmt (0022): Ein zweites Gerät, auf dem dasselbe
  Postfach mit einem anderen Hostnamen eingerichtet ist, bekommt die Frage statt der Server.
- **Neues Konto aus dem Ordner:** wie bisher nur lesend und ohne Passwort, mit den Servern aus
  dem Ordner. Neu: Der Name seines Schlüsselbund-Eintrags (`credential_ref`) ist zufällig
  (`synced:` und 32 Hex-Zeichen), nicht mehr die Konto-UUID. Ein Eintrag, den dieses Gerät unter
  der UUID schon hat, wird nie benutzt. Das ist kein theoretischer Fall: Ein lokales Konto, das
  beim Beitreten die ID aus dem Ordner übernimmt, behält seine alte UUID als Eintragsnamen. Ein
  Datensatz mit dieser alten UUID hätte vorher ein „neues“ Konto mit fremden Servern und dem
  echten Passwort angelegt.
- **Der Ordner stimmt wieder mit dem Gerät überein:** Eine wartende Änderung entfällt.

## Was der Sync sieht

Solange eine Zeile in `account_server_change` steht (wartend oder abgelehnt), liefert
`content_of` für das Konto die Server aus dieser Zeile, also die des Ordners. Tabelle und
Datensatz stimmen damit aus Sicht des Syncs überein. Eine lokale Änderung an einem anderen Feld
(Schreibzugriff ein/aus, später Name oder Farbe) erzeugt deshalb keine neue Version der Server
und schickt die Server dieses Geräts nicht in den Ordner zurück.

## Was der Nutzer entscheidet

Unter Einstellungen → Konten zeigt das Konto „Server-Einstellungen auf einem anderen Gerät
geändert“ mit den neuen Servern (`SettingsSyncStatus.serverChanges`, auch bei ausgeschaltetem
Sync, solange die Änderung wartet). Befehl `resolveServerChange(accountId, apply)`:

- **Übernehmen** (`apply = true`), in dieser Reihenfolge: Abgleich des Kontos anhalten, offene
  Leseverbindungen verwerfen, **Passwort aus dem Schlüsselbund löschen** (schlägt das fehl, ändert
  sich nichts und der Abgleich läuft mit den alten Servern weiter), Schreibzugriff aus (wartende
  Schreibvorgänge scheitern mit `read_only`, wie beim Schalter), dann in einer Transaktion die
  neuen Server schreiben, `writes_enabled = 0`, Zeile löschen, Konto als „Passwort fehlt“
  markieren (`sync_adopted`). Danach fragt das Konto nach dem Passwort für die neuen Server; die
  Anmeldung prüft es dort (`updatePassword`). Zwischen dem Löschen des Passworts und dem Schreiben
  der Server gibt es keinen Zustand, in dem das alte Passwort mit den neuen Servern zusammenkommt.
- **Behalten** (`apply = false`): `dismissed = 1`. Dieses Gerät behält seine Server und sein
  Passwort. Dieselbe Änderung wird nicht noch einmal angeboten; eine andere Änderung aus dem
  Ordner schon (die Zeile wird ersetzt, `dismissed = 0`).

Antwortet der Nutzer, ohne dass eine Änderung wartet: `not_found`. Neue Fehlercodes gibt es nicht.

## Verworfene Varianten

- **„Behalten“ schreibt die Server dieses Geräts zurück in den Ordner.** Hätte den Ordner
  „repariert“, aber zwei Geräte mit verschiedenen Hostnamen für dasselbe Postfach (ein legitimer
  Fall beim Beitreten) würden sich gegenseitig immer wieder fragen. Gegen einen Angreifer mit dem
  Schlüssel hilft es nicht: Er kann jederzeit wieder schreiben. Der Schutz liegt in der
  Entscheidung je Gerät.
- **Server-Felder gar nicht mehr abgleichen.** Einfacher, aber ein neues Gerät könnte Konten
  nicht mehr übernehmen, und die Frage beim Beitreten (gleiche Adresse, andere Server) würde
  still übergangen.
- **Ändern erlauben, nur das Passwort löschen.** Ein Angreifer könnte dann auf allen Geräten den
  Abgleich jedes Kontos abschalten, und der Nutzer gäbe sein Passwort an den neuen Server, ohne
  zu wissen, dass sich etwas geändert hat. Die Frage zeigt die neuen Server, bevor etwas passiert.
- **Hostnamen ohne Groß/Klein vergleichen.** Exakt ist strenger; eine reine Schreibweisen-
  Änderung kostet höchstens eine Frage.

## Was bleibt

- Ein neues Gerät übernimmt Konten mit den Servern aus dem Ordner. Gibt der Nutzer dort das
  Passwort ein, geht es an diese Server. Die Kontoseite zeigt sie, bevor er es eingibt; einen
  Vergleich mit „richtigen“ Servern gibt es auf einem neuen Gerät nicht.
- Wer den Sync-Schlüssel hat, kann weiter Namen, Farben, Identitäten, Signaturen, Regeln usw.
  ändern (0014: der Ordner vertraut jedem Schlüsselinhaber). Zugangsdaten bekommt er nicht mehr.
