# Stufe 04 — Aktionen und Outbox

**Ziel:** Markieren, Archivieren, Löschen und Verschieben wirken sofort in der Oberfläche und werden
zuverlässig auf dem Server nachgezogen, auch nach Offline-Phasen, Abstürzen und gleichzeitigen
Änderungen von außen. Keine Mail geht dabei verloren oder verdoppelt sich.

**Voraussetzung:** Stufe 03 fertig. Abschnitt „Outbox“ in `docs/ARCHITECTURE.md` gelesen.

## Aufgaben

- [x] **Outbox.** Tabelle `operation` und die Ausführung in `mail-engine`: lokale Änderung sofort, Operation vermerken, ausführen sobald online. Zustände `queued`, `in_flight`, `confirmed`, `failed`, `conflict`. Reihenfolge je Postfach, Zusammenfassen sich aufhebender Operationen.
- [x] **Absturzfestigkeit.** Beim Start werden `in_flight`-Operationen erst gegen den Server geprüft und nur dann wiederholt, wenn sie nachweislich nicht gewirkt haben.
- [x] **Fehler und Konflikte.** Bei Fehlschlag lokalen Zustand zurücknehmen und anzeigen. Ist die Mail auf dem Server inzwischen woanders oder gelöscht, gilt der Server; die Operation wird `conflict` und der Nutzer erfährt es.
- [x] **Aktionen.** Gelesen/ungelesen, markieren, Schlagwörter, archivieren (fehlt ein Archiv-Ordner: fragen, welcher oder ob einer angelegt wird), löschen (in den Papierkorb; im Papierkorb endgültig, mit Rückfrage), verschieben, Spam/kein Spam.
- [x] **IMAP-Umsetzung.** UID STORE, mit `UNCHANGEDSINCE` wenn CONDSTORE da ist (über die eigene Befehlsschicht, `[MODIFIED]` steht im Antworttext; COPYUID kommt bei MOVE als unaufgeforderte Antwort; Schlagwörter vorher auf gültige Zeichen prüfen, siehe 0002). UID MOVE; ohne MOVE: COPY, `\Deleted` setzen, UID EXPUNGE. Ohne UIDPLUS wird nur dann EXPUNGE gesendet, wenn keine fremden als gelöscht markierten Mails im Ordner liegen; sonst bleibt die Mail markiert und der Nutzer bekommt einen Hinweis.
- [x] **Schreibzugriff freischalten.** Schalter je Konto in den Einstellungen, mit Erklärung. Bestehende Konten bleiben aus. Für die Testkonten an.
- [x] **Beim Öffnen als gelesen markieren.** Einstellbar: sofort, nach zwei Sekunden, nie.
- [x] **Rückgängig.** Hinweis mit „Rückgängig“ für einige Sekunden nach jeder Aktion; löst die Gegenoperation aus.
- [x] **Mehrfachauswahl.** Mit Umschalttaste Bereiche, alles in der Ansicht; eine Aktion auf viele Mails ist ein Stapel.
- [x] **Oberfläche.** Schnellaktionen in der Zeile, Symbolleiste im Lesebereich, Ziehen auf einen Ordner, erste Tastenkürzel (die endgültige Belegung kommt in Stufe 8). Kleines Symbol für ausstehende und fehlgeschlagene Aktionen; Ansicht „Aktivität“ mit Wiederholen und Verwerfen.
- [x] **Fehlerinjektion.** Kleiner TCP-Proxy für Tests, der Verbindungen an wählbaren Stellen kappt.

## Abnahme

- [x] Integrationstests je Aktion: online; offline vorgemerkt und nach dem Verbinden ausgeführt
- [x] Test: Eine zweite IMAP-Sitzung verschiebt die Mail, während eine Operation wartet. Ergebnis ist `conflict`, die Mail existiert genau einmal
- [x] Test: Verbindung wird direkt nach dem Befehl gekappt. Nach dem Neustart wird die Operation nicht doppelt angewendet
- [x] Test: Verschieben ohne MOVE-Fähigkeit; EXPUNGE berührt keine fremden gelöschten Mails
- [x] Test für jede Folge von Aktionen: Die Menge der Message-IDs über alle Ordner ist vorher und nachher gleich, außer bei endgültigem Löschen
- [x] Test: Konten ohne Schreibzugriff lehnen weiterhin alles ab, und die Oberfläche erklärt warum
- [x] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Senden, Entwürfe (Stufe 5)
- Snooze, Pins, Regeln (Stufe 9)

## Der Mensch prüft danach

- Entscheiden, für welches echte Konto Schreibzugriff freigeschaltet wird. Empfehlung: zuerst ein Nebenkonto, das Hauptkonto nach einigen Tagen.

## Ergebnis

Ausgeführt am 22./23.11.2026 (Hauptsitzung mit Subagenten: Outbox und IMAP-Schreibweg, Verdrahtung in
`AppCore`, Oberfläche). Geprüft am 23.11. von der Hauptsitzung in einem sauberen Worktree:
`scripts/check.sh` grün auf 15efcf3 (878 Rust-Tests, 598 Vitest-Tests, cargo deny, pnpm audit);
alle Servertests (`cargo test --workspace -- --include-ignored`, 950 Tests gegen den lokalen
Stalwart) grün auf c861351; `pnpm --dir app test:e2e` (5 Spec-Dateien, 44 Tests, darunter
`actions.e2e.mjs`) und `scripts/check.sh` (878 Rust-, 599 Vitest-Tests) grün auf 8762087.
Entscheidung: 0012 (Outbox).

**Nachgewiesen (Servertests in `crates/mail-engine/src/outbox/server_tests.rs`)**
- `every_action_online_and_offline`: jede Aktion online sowie offline vorgemerkt und nach dem
  Verbinden ausgeführt; der Abgleich bestätigt den Serverzustand.
- `a_message_moved_elsewhere_meanwhile_is_a_conflict_and_exists_once`: eine zweite IMAP-Sitzung
  verschiebt die Mail, die Operation endet als `conflict`, die Mail existiert genau einmal.
- `a_connection_cut_after_the_command_is_not_applied_twice`: Proxy kappt direkt nach dem Befehl,
  nach dem Neustart wird nichts doppelt angewendet (`in_flight` wird erst gegen den Server geprüft).
- `moving_without_move_leaves_other_deleted_mail_alone`: ohne MOVE/UIDPLUS berührt EXPUNGE keine
  fremden als gelöscht markierten Mails; die Mail bleibt markiert, der Hinweis `left_marked_deleted`
  erscheint in der Aktivität.
- `random_action_sequences_keep_every_message`: zufällige Folgen von Aktionen, die Menge der
  Message-IDs über alle Ordner ist vorher und nachher gleich (außer endgültigem Löschen).
- `accounts_without_write_access_refuse_everything`: nur lesende Konten lehnen alles ab, auf dem
  Draht kommt kein schreibender Befehl an.
- `a_missing_archive_is_a_typed_error_and_creating_one_an_operation`: fehlt ein Archiv, fragt die
  Oberfläche; „Ordner anlegen“ ist selbst eine Operation.
- In der App (`actions.e2e.mjs`): nur lesendes Konto erklärt, warum nichts passiert; Schalter
  „Änderungen erlauben“ in den Kontoeinstellungen; `E` archiviert sofort, auf dem Server ebenso,
  „Rückgängig“ holt die Mail zurück; bei gestopptem Server vorgemerkt und nach dem Neustart
  ausgeführt; Mehrfachauswahl mit Umschalt; im Papierkorb fragt „Endgültig löschen“ im einzigen
  Dialog der App (Fokus startet auf „Abbrechen“).
- Oberfläche (Vitest): Aktionsregister mit Kürzeln je Plattform, Symbolleiste, Schnellaktionen in der
  Zeile, Ordnerauswahl, Ziehen auf Ordner, Hinweise mit „Rückgängig“, Symbol für ausstehende und
  fehlgeschlagene Änderungen, Ansicht „Aktivität“ mit Wiederholen und Verwerfen, „Beim Öffnen als
  gelesen markieren“ (sofort, nach zwei Sekunden, nie).

**Abweichungen und Annahmen**
- Die endgültige Tastenbelegung kommt in Stufe 8; jetzt gelten die Kürzel des Aktionsregisters
  (`app/src/features/actions/registry.ts`).
- Der Bestätigungsdialog fängt Escape auch ab, wenn der Fokus die Seite verlassen hat, und holt den
  Fokus zurück (im Desktop-Test nach dem Wechsel des Farbschemas aufgefallen).
- Die Testkonten schalten den Schreibzugriff in den Tests über den Schalter bzw. den Kern frei;
  neue Konten bleiben nur lesend.

**Ungeprüft / offen**
- Windows zur Laufzeit (Ziehen auf Ordner, Kürzel mit Strg, Dialog).
- Bildschirmfotos der neuen Zustände wurden nicht systematisch durchgesehen.

**Wartet auf**
- Mensch: entscheiden, für welches echte Konto der Schreibzugriff freigeschaltet wird (Empfehlung:
  zuerst ein Nebenkonto). Windows-Checkliste Abschnitt 9.
