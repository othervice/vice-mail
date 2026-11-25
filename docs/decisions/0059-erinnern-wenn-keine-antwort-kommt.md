# 0059 — Erinnern, wenn keine Antwort kommt

- **Datum:** 24.11.2026
- **Anlass:** Stufe 20, Aufgabe B (Auswahl des Menschen vom 24.11.2026).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 20).
- **Code:** Migration `0024_follow_up.sql` (Tabelle `follow_up`, Spalte `draft.follow_up_at`),
  `crates/mail-store/src/follow_ups.rs`, `crates/mail-store/src/compose.rs` (`send_followups`),
  `crates/mail-model/src/follow_up.rs`, `crates/mail-engine/src/comfort/follow_up.rs`,
  `comfort/mod.rs` (Zeitgeber, Antwortprüfung, Block über dem Posteingang), `comfort/time.rs`,
  `settings_sync/record.rs` (Art `followUp`), `app/src/features/composer/FollowUpOption.tsx`,
  `app/src/features/comfort/FollowUpPicker.tsx`, `RowMarks.tsx`, `inboxRows.ts`,
  `listSources.tsx`, Aktionen `followUp`/`clearFollowUp`, Ansicht `waiting`.

## Entscheidung

### Wie Snooze, eigene Tabelle

Eine Erinnerung gehört zu einer gesendeten Mail, über Konto-UUID und Message-ID wie Snooze, Pins
und Aufgaben (nie über lokale IDs), lokal, nichts geht an den Server, auch nicht als Schlüsselwort.
Eigene Tabelle `follow_up` statt einer Spalte von `snooze`: Eine Mail kann zurückgestellt sein und
zugleich auf Antwort warten, und die Regeln (Antwort hebt auf) sind andere. `sent_at` ist der
Zeitpunkt des Sendens (für „Keine Antwort seit …“ und als Grenze für Antworten), `returned_at`
wie bei Snooze nur auf diesem Gerät.

### Setzen

- **Beim Senden:** im Composer unter den Sendeoptionen „Erinnern, wenn keine Antwort kommt:“ mit
  „Morgen“, „In 3 Tagen“, „Nächste Woche“ (jeweils 08:00, Zeitzone der Person, Zeiten vom Kern)
  oder Datum und Uhrzeit (eine Minute bis fünf Jahre voraus, wie Snooze). Gespeichert am Entwurf
  (`DraftSendOptions::follow_up_at`), eingefroren in den `SendJob` und erst in der Transaktion
  erfasst, die die Annahme durch den Server bestätigt – wie „Senden und archivieren“. Ein
  zurückgenommener oder gescheiterter Versand hinterlässt keine Erinnerung.
- **Danach:** Aktion „Erinnern, wenn keine Antwort kommt…“ (Menü wie Snooze) auf eigenen Mails
  (im Gesendet-Ordner, von der Adresse des Kontos oder mit Erinnerung), auch zum Verschieben;
  „Erinnerung entfernen“. Hat das Gespräch schon eine Antwort, bleibt keine Erinnerung, und der
  Hinweis sagt es.

### Antwort

Eine Antwort ist eine Mail des Gesprächs (Stufe 7: `thread_ref` kennt jede Message-ID, die eine
Mail des Gesprächs trägt oder nennt – auch wenn die eigene Kopie noch nicht geladen ist) in
**irgendeinem Ordner des Kontos**, die nach dem Senden einsortiert ist und **nicht** von einer
eigenen Adresse (Konten und Identitäten) kommt. Eigene Nachträge im Gespräch heben also nicht auf.

Geprüft wird
- nach neuer Mail eines Kontos (`messagesChanged`, gebündelt: eine Sekunde nach der letzten
  Änderung, nur wenn das Konto Erinnerungen hat; alle seine Erinnerungen, auch zurückgekehrte),
- wenn eine Erinnerung fällig wird (vor dem Zurückkehren),
- beim Setzen.

Eine beantwortete Erinnerung wird gelöscht; die Löschung geht über den Abgleich an die anderen
Geräte.

### Fällig ohne Antwort

Der Zeitgeber der Snoozes (`snooze_worker`) wartet auf den frühesten Zeitpunkt beider Arten und
prüft die Wanduhr mindestens alle 15 s, beim Start und nach `resume`. Eine fällige Erinnerung ohne
Antwort steht im Block „Noch keine Antwort“ über dem Posteingang (`InboxTop::follow_ups`), als Zeile
der gesendeten Mail in ihrem Gesendet-Ordner, mit der Marke „Keine Antwort seit 20. Nov.“; sie
bleibt, bis die Person sie entfernt, neu setzt oder eine Antwort kommt. Nur über Posteingängen
(ein Ordner, der kein Posteingang ist, zeigt sie nicht). Keine Systemmitteilung (Snooze hat eine;
hier reicht der Block, die Person hat den Zeitpunkt selbst gewählt).

### Ansicht „Warten auf Antwort“

Neue Ansicht `waiting` (nach „Später“, abschaltbar wie die anderen, 0050): alle Erinnerungen des
Kontos, die nächste fällige zuerst, danach die zurückgekehrten; höchstens 1.000.

### Abgleich

Art `followUp` (Datensatz `{ dueMs, sentAtMs }`), „neuere Version gewinnt“ je Mail, wie Snooze.
Ob sie zurückgekehrt ist, entscheidet jedes Gerät selbst; eine gleiche Zeit vom anderen Gerät
lässt den Zustand hier, eine neue Zeit lässt sie wieder warten. Ein neu verschlüsseltes Konto
(Rekey) nimmt seine Erinnerungen mit.

## Abgelehnt

- **Die Mail in den Posteingang verschieben oder kopieren:** schreibt auf dem Server und verdoppelt
  Mails; die Zeile im Block zeigt sie dort, wo sie liegt.
- **Antworten an der Betreffzeile erkennen:** das Gespräch nach Stufe 7 ist genauer; die
  Betreff-Heuristik der Threads greift dort ohnehin vorsichtig.
- **Ein Schlüsselwort auf dem Server** (wie `$Snoozed`): andere Programme kennen es nicht, und die
  Erinnerung soll lokal bleiben.

## Tests

`mail-model`/`mail-store`: Zeilen, Marken, Block nur über Posteingängen, Rückkehr, Verschieben,
Entfernen, Abgleich (gleiche Zeit hält den Zustand, neue Zeit wartet). `mail-engine` mit
verstellbarer Uhr gegen den Fake-Server: Rückkehr nach „Schlaf“, eigene Nachricht im Gespräch hebt
nicht auf, Antwort im Posteingang und im Archiv hebt auf, Setzen auf beantwortete Mail; Composer
(Erfassung erst bei Annahme). Servertest gegen Stalwart
(`crates/mail-engine/tests/compose.rs`): Senden mit Erinnerung, Uhr drei Tage vor, Block über dem
Posteingang, Antwort von alice beendet sie. Vitest und Desktop-Test `stage20.e2e.mjs` (B, mit
`e2e_advance_clock`).
