# 0076 — Passwörter von einem anderen Gerät übernehmen

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 10. Nach dem Beitreten zum Einstellungs-Sync starten übernommene
  Konten mit „Passwort fehlt“ (0040), und der Mensch tippt jedes Passwort auf jedem Gerät neu ein.
- **Status:** Ablauf mit dem Menschen abgestimmt (26.11.2026), Einzelheiten und die Abweichung
  beim Code (unten) von der ausführenden Sitzung entschieden
- **Code:** `crates/mail-engine/src/settings_sync/transfer.rs` (Einträge, Code, Versiegeln, was ein
  Gerät aus einer Liste entscheidet), `crates/mail-engine/src/settings_sync/handover.rs` (Zustand
  im Speicher, Befehle, Speichern), `settings_sync/crypto.rs` (Präfix `xfer-`), `plan.rs`
  (übersprungen wie Schlüssel-Datensätze), `pass.rs` (`PreparedPass::transfers`),
  `service.rs` (nach jedem Durchlauf, schnelles Nachsehen, Status), `core.rs`
  (`install_password`), `crates/mail-model/src/settings_sync.rs` (`PasswordTransfer`,
  `PasswordRequest`), `app/src-tauri/src/commands/settings_sync.rs` (fünf Befehle),
  `app/src/features/settings-sync/PasswordTransfer.tsx`, `AccountDetail.tsx` (Hinweis)
- **Tests:** `settings_sync/transfer/tests.rs` (7), `settings_sync/tests/password_transfer.rs`
  (7 Fälle über `AppCore` auf zwei Geräten mit einem Ordner im Speicher, dazu ein Eindringling
  mit dem Sync-Schlüssel), `crates/mail-engine/tests/settings_sync.rs`
  (`a_password_goes_to_the_new_device_through_the_folder`, lokaler Testserver),
  `app/tests/password-transfer.test.tsx` (beide Seiten, Tastatur, axe hell und dunkel),
  `app/tests/mcp-sync-ipc.test.ts`

Ergänzt 0014, 0022 und 0040. Die Grenze aus 0040 bleibt: Ein Passwort geht nie an einen Server,
den nur der Ordner nennt.

## Ablauf

1. **Neues Gerät** („Passwörter von einem anderen Gerät übernehmen“): erzeugt ein X25519-
   Schlüsselpaar nur für diese Anfrage (der private Teil nur im Speicher) und schreibt eine
   **Anfrage** in den Sync-Ordner: Anfrage-ID (32 Hex-Zeichen), Gerätename, die Konten, die hier
   auf ihr Passwort warten (Passwort oder API-Token, nie Anmeldungen), mit den Servern dieses
   Geräts, und eine **Festlegung** auf den öffentlichen Schlüssel
   (`SHA-256(Etikett, Anfrage-ID, Schlüssel)`). Gültig 15 Minuten.
2. **Eingerichtetes Gerät:** Ein Durchlauf zeigt die Anfrage in Einstellungen → Einstellungs-Sync
   („… fragt nach Passwörtern“). „Passwörter an ein neues Gerät geben“ macht sofort einen
   Durchlauf und danach alle 3 Sekunden, bis eine Anfrage da ist. „Weiter“ schreibt eine
   **Herausforderung** mit einer Zufallszahl (64 Hex-Zeichen).
3. **Neues Gerät:** nimmt die erste Herausforderung (nach Version, dann UID) und schreibt die
   **Offenlegung**: den öffentlichen Schlüssel und welche Herausforderung sie beantwortet. Beide
   Geräte zeigen jetzt denselben sechsstelligen Code,
   `SHA-256(Etikett, Anfrage-ID, Schlüssel, Zufallszahl)`, als „123 456“.
4. **Eingerichtetes Gerät:** prüft den Schlüssel gegen die Festlegung, zeigt Gerätename, Code,
   „Zeigt … diesen Code?“ und die Konten (vorausgewählt: alle, die hier ein Passwort und dieselben
   Server haben). Erst nach „Codes gleich, Passwörter senden“ verschlüsselt es die gewählten
   Passwörter mit age an diesen Schlüssel, jedes mit den IMAP- und SMTP-Servern des Kontos, und
   schreibt die **Antwort**. „Codes verschieden, ablehnen“ schreibt eine Ablehnung.
5. **Neues Gerät:** öffnet die Antwort, speichert ein Passwort nur für ein Konto, nach dem es
   gefragt hat, das noch auf sein Passwort wartet, mit derselben Art Geheimnis und genau den
   mitgeschickten Servern (im Schlüsselbund, auf macOS im Sammel-Eintrag aus 0075), lässt Schreiben aus, verbindet das
   Konto neu und löscht alle Einträge der Anfrage.

Während eine eigene Anfrage läuft oder eine Herausforderung auf die Offenlegung wartet, schaut das
Gerät alle 2 Sekunden in den Ordner (`password_transfer_poll`).

## Abweichung vom Plan: Festlegung vor dem Code

Der Plan sah einen Code aus öffentlichem Schlüssel und Anfrage-ID vor. Wer mit dem Sync-Schlüssel
in den Ordner schreibt (ein kompromittiertes Gerät, der Anbieter des Sync-Kontos nach dem Knacken
einer schwachen Passphrase), könnte dann die echte Anfrage lesen und so lange eigene Schlüssel
oder Anfrage-IDs durchprobieren, bis sein Code gleich aussieht: bei sechs Ziffern etwa eine
Million Versuche, also weniger als eine Sekunde. Der Mensch würde gleiche Codes sehen und die
Passwörter an den Angreifer schicken.

Deshalb legt sich das neue Gerät zuerst auf seinen Schlüssel fest, das gebende Gerät wählt danach
die Zufallszahl, und erst dann wird der Schlüssel offengelegt (wie beim Vergleich von Codes bei
Bluetooth oder ZRTP). Wer im Ordner sitzt, muss sich festlegen, bevor er die Zufallszahl kennt,
oder die Zufallszahl wählen, bevor er den Schlüssel kennt; ein ausgetauschter Schlüssel zeigt mit
Wahrscheinlichkeit 999.999 zu 1 einen anderen Code. Für den Menschen ändert sich nur, dass der Code
auf dem neuen Gerät erscheint, sobald das andere Gerät „Weiter“ gewählt hat. Kosten: zwei Einträge
mehr und ein paar Sekunden.

## Einzelheiten

- **Einträge:** `xfer-` + 32 Hex-Zeichen (HMAC über Art, Anfrage-ID und Gerät), dieselben beiden
  Kopfzeilen wie Datensätze, Inhalt mit age an den Sync-Schlüssel, gegen die Kopfzeile geprüft.
  Der Mailserver sieht vier Einträge mehr und ihre Zeiten, keinen Inhalt, keine Konten.
- **Keine Geheimnis-Schlüssel im JSON:** Die Regel gegen Objekt-Schlüssel wie `password` gilt für
  Datensätze und bleibt unverändert. Die Passwörter stehen nur in der zweiten age-Datei, unter
  neutralen Namen (`sealed`, `value`).
- **Nur gleiche Server** (0040): Das gebende Gerät bietet ein Konto nur an, wenn Host, Port,
  Sicherheit und Benutzername von IMAP und SMTP auf beiden Geräten gleich sind; das neue Gerät
  prüft dasselbe noch einmal gegen die mitgeschickten Server. Ein Passwort geht so nur an Server,
  für die es auf dem gebenden Gerät eingegeben wurde und die das neue Gerät selbst nutzt.
- **Keine Anmelde-Tokens** (Microsoft, Google): Jedes Gerät meldet sich selbst an.
- **Neustart:** Der private Schlüssel ist weg; der nächste Durchlauf löscht die eigene Anfrage
  (eine Anfrage oder Offenlegung dieses Geräts ohne Schlüssel).
- **Aufräumen:** Das neue Gerät löscht nach dem Austausch alles. Jedes Gerät löscht Antworten
  ohne Anfrage, abgelaufene Einträge (15 Minuten plus 5 Minuten für Uhren) und, ohne sie zu lesen, Einträge,
  deren Version älter als 20 Minuten ist; beschädigte sofort. Je Durchlauf höchstens 64 Einträge
  von höchstens 512 KiB.
- **0.1.2** kann die `xfer-`-IDs nicht lesen und zählt sie, solange sie da sind, als ignorierte
  Einträge. Sie löscht sie nie.
- **Was ein Eindringling mit dem Sync-Schlüssel kann:** eine Übertragung verzögern oder
  abbrechen (Einträge löschen, eine falsche Antwort einschieben, die dann „konnte nicht gelesen
  werden“ zeigt), dem neuen Gerät Passwörter für dessen eigene Server unterschieben (die dann
  falsch sind), aber keine Passwörter bekommen, solange der Mensch die Codes vergleicht.
- **Keine neuen Fehlercodes:** fehlende oder abgelaufene Anfrage `not_found`, „Codes erst
  vergleichen“ und „nichts zu geben“ `invalid_argument`, Sync aus `sync_off`. Wie es ausging, steht
  im Status (`passwordTransfer`, `passwordRequests`).

## Nicht abgedeckt

- Ein Desktop-Test mit zwei App-Instanzen; ein Test mit einer Instanz bräuchte eine Anfrage, die
  mit dem Sync-Schlüssel der Test-App verschlüsselt ist, und dieser liegt in ihrem Schlüsselbund.
  Abgedeckt ist der Ablauf in Rust über zwei Kerne (Speicher-Ordner und lokaler Testserver) und
  die Oberfläche in Vitest.
- Windows: ungeprüft (die Logik ist plattformunabhängig; gespeichert wird über `SecretStore`).
