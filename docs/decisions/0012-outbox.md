# 0012 — Outbox und Schreibzugriff (Kern von Stufe 4)

**Status:** Entwurf der Outbox-Sitzung, 22.11.2026 · **Code:** `crates/mail-model/src/{writer,outbox}.rs`,
`crates/mail-imap/src/writer.rs`, `crates/mail-store/{migrations/0004_operation.sql,src/outbox*.rs}`,
`crates/mail-engine/src/outbox/`

Vorgezogener Kern von Stufe 4, ohne Oberfläche und ohne Verdrahtung in `AppCore`/Commands. Hier steht,
was die Architektur offen ließ oder wovon der Code bewusst abweicht.

## Entscheidungen

**Schreib-Trait getrennt vom Lese-Trait.** `MailWriter: MailBackend` (`mailbox_status`, `store_flags`,
`move_messages`, `expunge_messages`, `create_mailbox`) und `WriteConnector::connect_writer`.
`MailBackend` ist unverändert (0007). `ImapWriter` ist ein eigener Typ um `ImapBackend`;
`connect_writer` lehnt eine Konfiguration mit `Access::ReadOnly` mit `ReadOnly` ab, bevor eine
Verbindung entsteht. Die Positivliste des Nur-Lese-Modus bleibt, `tests/read_only.rs` ist grün.
Geschrieben wird nur über die eigene Verbindung der Outbox. **Für die Integration:** Der Abgleich sollte
auch bei freigeschaltetem Konto nur lesend verbinden (EXAMINE, Positivliste); `sync/account.rs` übergibt
heute `writes_enabled` an `connect_config` und würde dann mit SELECT öffnen.

**Eine Operation je Mail und Änderung.** Eine Aktion auf viele Mails ist ein Stapel (`batch` = Id der
ersten Operation); Zustände gibt es je Mail. Beim Ausführen gehen aufeinanderfolgende Operationen
desselben Stapels, Postfachs, Typs und Werts (Flag und Wert bzw. Ziel) als ein Befehl hinaus,
höchstens 500 UIDs. Reihenfolge: nach `operation.id` je Konto (damit auch je Postfach). „Wiederholen“
legt die Operation neu ans Ende (neue Id, gleicher Stapel), damit sie nach dem läuft, was der Nutzer
inzwischen getan hat.

**Planen im Modell, Schreiben in einer Transaktion.** `mail_model::plan_action` ist reine Logik. Der
Store liest die Orte frisch, plant, wendet die lokale Wirkung an und vermerkt die Operationen in einer
`BEGIN IMMEDIATE`-Transaktion. Ein Konto ohne `writes_enabled` lehnt vorher mit
`ActionError::ReadOnly` ab; nichts wird vermerkt, keine Verbindung geöffnet.

**Lokale Wirkung.**
- Flags: sofort am Ort. Wartende Flag-Operationen gewinnen gegen Abgleich-Schreibvorgänge:
  `store_headers` und `update_flags` wenden sie nach dem Schreiben erneut an (eine Index-Abfrage je
  Fenster, wenn nichts wartet).
- Verschieben und endgültiges Löschen: Der Ort wird sofort entfernt, die Nachricht bleibt. Die Nutzdaten
  behalten UID und Flags für das Zurücknehmen. **Abweichung:** Im Ziel erscheint die Mail erst, wenn der
  Server bestätigt hat, sofort mit COPYUID (UIDPLUS), sonst nach dem nächsten Abgleich über die
  Zusammenführung per Message-ID. Ein Platzhalter im Ziel hätte den Abgleich (UID-Mengenvergleich,
  Zusammenführung) verfälscht.
- Nachrichten, auf die eine Operation in `queued`, `in_flight` oder `confirmed` verweist, löscht die
  Aufräumrunde (`delete_orphan_messages`) nicht; bestätigte werden nach 24 Stunden entfernt.
- Gegenläufiges hebt sich auf: Hat eine noch wartende (`queued`) Operation auf demselben Ort und Flag
  als `before` den neuen Wert, werden beide verworfen. Laufende (`in_flight`) werden nie aufgehoben.
  Ein Flag, das lokal schon so steht, erzeugt keine Operation.

**Ausführen.** Vor jedem Befehl prüft die Outbox den Server: Quellpostfach mit der UIDVALIDITY der
Operation (sonst `conflict: mailbox_reset`, fehlt es: `mailbox_gone`), Mail noch da (sonst
`conflict: gone`). Ein Flag, das auf dem Server schon so steht, wird ohne STORE bestätigt.
Die Operationen werden unmittelbar vor dem Senden `in_flight`.

**CONDSTORE.** `UID STORE … (UNCHANGEDSINCE m)` mit der MODSEQ der Mail, wie die App sie zuletzt gesehen
hat: vom Abgleich am Ort gespeichert, oder aus der Antwort auf das eigene STORE (die Outbox schreibt sie
an den Ort und in die Nutzdaten wartender Flag-Operationen derselben Mail, denn ein dahinter wartendes
Verschieben hat den Ort lokal schon entfernt). Ohne bekannte MODSEQ (ein Ort, den die Outbox nach einem
Verschieben aus COPYUID eingetragen hat) ist das STORE unbedingt. **Nicht** die HIGHESTMODSEQ des
Postfachs: Sie ist nach eigenen Verschiebungen älter als die Mail und erzeugte falsche Konflikte (vom
Eigenschaftstest gefunden). Bei `[MODIFIED]` liest die Outbox die Flags neu: steht der Server schon wie
gewünscht, `confirmed`, sonst `conflict: modified`, und die Flags des Servers werden lokal übernommen.

**Verschieben und Absturzfestigkeit.** Vor dem Senden speichert die Outbox UIDNEXT und UIDVALIDITY des
Ziels (STATUS, ohne Werte ein SELECT) in der Operation. Alles, was danach im Ziel ab dieser UIDNEXT
liegt, kam nach diesem Moment; die Prüfung nach einem Abbruch unterscheidet so die eigene Kopie von einer
älteren derselben Mail (Abgleich per Message-ID, ohne sie per Größe):

| Kopie im Ziel | Original in der Quelle | Ergebnis |
|---|---|---|
| ja | nein | `confirmed` (Ziel-UID bekannt) |
| ja | ja (COPY-Rückfall brach vor EXPUNGE ab) | nur das Original entfernen, nie ein zweites COPY |
| nein | ja | erneut senden (höchstens 5 Versuche, dann `failed`) |
| nein | nein | `conflict: gone` |

Lehnt der Server ein Verschieben ab (auch halb, im COPY-Rückfall), prüft die Outbox genauso, bevor sie
`failed` meldet. Flags nach einem Abbruch: steht der Wert auf dem Server, `confirmed`, sonst erneut.
Endgültiges Löschen nach einem Abbruch: Mail weg → `confirmed`; vor dem Senden weg → `conflict: gone`.

**Ohne MOVE, ohne UIDPLUS.** Ohne MOVE: `UID COPY`, `UID STORE +FLAGS.SILENT (\Deleted)`, dann
`UID EXPUNGE`. Ohne UIDPLUS: `UID SEARCH DELETED`; nur wenn darin keine fremden Mails sind, ein einfaches
`EXPUNGE`; sonst bleibt das Original markiert und die Operation trägt den Hinweis
`left_marked_deleted`. COPYUID zählt nur, wenn der Server UIDPLUS ankündigt. Zwischen SEARCH und
EXPUNGE kann ein anderer Client noch eine Mail markieren; dieses Fenster bleibt (ein Rundlauf).

**Fehler.** NO/BAD, fehlendes Ziel oder ungültige Eingaben → `failed`, lokale Änderung zurückgenommen,
Fehler als `IpcError` gespeichert. Abgebrochene Verbindung → der Schritt endet, die Operationen bleiben
`in_flight` und werden nach dem Wiederverbinden geprüft. Ein Panic im Schritt kostet nur die Verbindung
(eigener Tokio-Task). Vorher gewonnene Ergebnisse eines Schritts werden immer gespeichert.

**Rückgängig.** Wartendes wird aufgehoben (Zeile entfernt, lokale Änderung zurück), bestätigte oder
laufende Flag-Änderungen bekommen Gegenoperationen (MODSEQ aus dem aktuellen Ort aufgefrischt), ein
bestätigtes Verschieben wird vom jetzigen Ort im Ziel zurückverschoben. Nicht rückgängig zu machen
(`NotUndoable`, das ganze Rückgängig wird abgelehnt): endgültiges Löschen, ein angelegter Ordner, ein
gerade laufendes Verschieben, ein Verschieben, dessen Ziel-Ort lokal noch unbekannt ist (ohne UIDPLUS
bis zum nächsten Abgleich).

**Schreibzugriff.** `Store::set_writes_enabled` / `Outbox::set_writes_enabled`. Neue Konten bleiben aus.
Ausschalten lässt wartende Operationen mit `read_only` scheitern und nimmt sie zurück; laufende warten,
bis wieder eingeschaltet ist, und werden dann geprüft.

**Archiv-, Papierkorb-, Spam-Ordner fehlen.** Typisierter Fehler `ActionError::MissingMailbox(role)`.
Anlegen ist eine eigene Operation (`create_mailbox`): CREATE mit `USE (\Archive)` bei
CREATE-SPECIAL-USE; lehnt der Server das ab (`[USEATTR]`, z. B. weil schon ein anderer Ordner die Rolle
hat), ohne. Danach LIST, Ordnerliste speichern, Rolle lokal setzen (Quelle `user`), falls der Server sie
nicht vergeben hat. Ein schon vorhandener Ordner gilt als Erfolg (Wiederholung harmlos).

**Löschen im Papierkorb** verlangt die eigene Aktion `DeletePermanently`; `Delete` auf Mails im
Papierkorb ergibt `ActionError::ConfirmationRequired`.

**Spam/kein Spam** verschiebt nur (in den Junk-Ordner bzw. aus ihm in den Posteingang). **Abweichung:**
keine Schlagwörter `$Junk`/`$NotJunk` in dieser Fassung.

**Idempotenz.** `idempotency_key = <Anfrage-Id>:<Index>`. Dieselbe Anfrage zweimal (ein doppelt
gesendeter IPC-Aufruf mit derselben Id, `Outbox::apply_request`) ändert nichts.

**Ereignisse.** `OutboxEvent` (`operationChanged` mit vollständigem Zustand, `operationRemoved`,
`messagesChanged`, `mailboxesChanged`) auf `Outbox::subscribe`. `CoreEvent` und `ErrorCode` sind
unverändert, damit die erzeugten Bindings gleich bleiben. Nach jedem Schritt ruft die Outbox
`SyncHook::sync_requested(account, mailboxes)` für die auf dem Server geänderten Postfächer.

**Fehlercodes.** `ActionError` hängt als `CoreError::Action` an und bildet auf vorhandene Codes ab
(`read_only`, `not_found`, `invalid_argument`). Die Integration braucht eigene Codes für
`MissingMailbox` und `ConfirmationRequired` (mit Texten in `app/src/i18n/`), damit die Oberfläche
gezielt fragen kann.

**Schema.** Migration 4: Tabelle `operation` (Art, Aktion, Nutzdaten JSON, Idempotenzschlüssel,
Zustand, Versuche, letzter Fehler, Konflikt, Hinweis, Zeitstempel, Konto, Quellpostfach/-UID, Ziel,
Nachricht), `AUTOINCREMENT`, damit Ids nach dem Aufräumen nicht wiederkehren. Fremdschlüssel auf
Postfach und Nachricht mit `ON DELETE SET NULL`; ausgeführt wird nur aus den Nutzdaten. Indizes und
Plan-Tests für alle neuen Abfragen.

**Fehlerinjektion.** `Proxy::cut_responses(delay)` kappt offene Verbindungen sofort, lässt aber noch
`delay` lang Bytes zum Server durch; `WireTap::on_write(muster, …)` löst beim Schreiben eines Befehls
aus. Zusammen: „Verbindung direkt nach dem Befehl gekappt“. `ImapConnector::with_masked_capabilities`
blendet Fähigkeiten aus (MOVE, UIDPLUS) und testet die Rückfälle gegen den echten Testserver.

## Nachweise (22.11.2026, Testserver Stalwart 1.1.1, Konto bob)

`cargo test -p mail-engine --lib outbox -- --include-ignored --test-threads=1`: 9 von 9, zweimal
hintereinander; `cargo test -p mail-imap -- --include-ignored`: grün, darunter `read_only.rs`.
Eigenschaftstest: vier feste Seeds (mit MOVE, ohne MOVE, ohne MOVE und UIDPLUS, mit MOVE), je 150
Schritte mit Offline-Phasen, Rückgängig und Abgleich; Menge der Message-IDs über alle Ordner gleich bis
auf endgültig Gelöschte, jede Mail genau einmal, lokal nach dem Abgleich gleich.

## Ungeprüft und bekannte Grenzen

- Echte Server (Dovecot, Gmail, Exchange); nur Stalwart und der eigene Fake-Server. Server ohne MOVE und
  ohne UIDPLUS nur durch ausgeblendete Fähigkeiten.
- Ein Erstabgleichsfenster kann den Quell-Ort einer wartenden Verschiebung wieder eintragen (kurz doppelt
  sichtbar, bis der nächste Abgleich ihn entfernt).
- Der Abgleich meldet `newMail` auch für eine ungelesene Mail, die die Outbox in den Posteingang
  verschoben hat (Verhalten von `sync/pass.rs`).
- Wiedererkennen einer verschobenen Mail ohne Message-ID nur über die Größe.
- Windows.

## Für die Integration

`Outbox::new(store, connector, credentials, OutboxConfig::from_sync(&sync))`, `set_sync_hook` auf den
Abgleich des Kontos, `start()` beim Start (prüft `in_flight`), `wake(account)` wenn der Abgleich wieder
verbunden ist, `shutdown()` beim Beenden; Events weiterreichen, Commands für `apply`, `undo`, `retry`,
`discard`, `operations`, `create_mailbox`, `set_writes_enabled`.
