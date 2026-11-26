# 0078 — Konten: eigene Reihenfolge und Name in der Liste

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkte 1 und 2. Der Mensch: „In der Liste links muss man die Konten
  verschieben können, heute stehen sie in der Reihenfolge, in der sie angelegt wurden.“ und „Ich
  muss die Namen der Postfächer bearbeiten können … die Reihenfolge und den Anzeigenamen.“
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22, Punkte 1 und 2).
- **Code:** Migration `crates/mail-store/migrations/0032_account_order.sql`,
  `crates/mail-store/src/accounts.rs` (`reorder`, `rename`, Reihenfolge der Liste),
  `settings_sync.rs` (Feld `position`), `crates/mail-engine/src/core.rs` (`reorder_accounts`,
  `rename_account`), `settings_sync/record.rs`, `ai/mod.rs` (Absendername für KI-Antworten),
  Commands `reorderAccounts`, `renameAccount`; Oberfläche `features/accounts/arrange.ts`,
  `accountDrag.ts`, `features/mailboxes/MailboxTree.tsx` (Kontozeile), `app/SidebarColumn.tsx`,
  `features/settings/AccountsSettings.tsx`, `settings/accounts/AccountDetail.tsx`, Texte
  `accountEdit.*`
- **Tests:** Store `account_order_tests.rs` (Reihenfolge, frische Positionen, Migration, Umbenennen,
  Position im Sync-Datensatz), Engine `settings_sync/tests/scenarios.rs`
  (`account_names_and_the_newest_order_reach_every_device`), `ai/tests.rs` (Antwort mit dem
  Absendernamen), Vitest `account-order.test.tsx`, Desktop `stage22-accounts.e2e.mjs`

## Entscheidung

**1. Eine Position je Konto, NULL heißt „nicht platziert“.** `account.position` (Migration 0032)
ist leer, bis der Mensch ein Konto verschiebt. Jede Liste von Konten (`listAccounts`, damit linke
Spalte, schmale Leiste, Einstellungen → Konten, „Schreiben“ je Konto, „Alle Posteingänge“-Auswahl,
Gruppen, Benachrichtigungen, KI, MCP, Diagnose; der Composer selbst wählt nur unter den Adressen
des Kontos, aus dem er schreibt) sortiert `position IS NULL, position, created_at, id`: erst
die platzierten Konten, dann die übrigen in der Reihenfolge, in der sie dazukamen. Die Migration
schreibt nichts um; die heutige Reihenfolge bleibt so von selbst, und Datensätze im Sync ändern sich
nicht von allein. Ein neues Konto steht hinten.

**2. Jedes Verschieben gibt allen Konten eine frische Position.** `reorderAccounts(ids)` nimmt die
ganze gewünschte Reihenfolge (fehlende Konten folgen in ihrer Reihenfolge, doppelte zählen einmal,
ein unbekanntes Konto ändert nichts) und vergibt `max(position) + 1 + i`, auch an Konten, deren
Platz gleich blieb. Grund: Der Kontodatensatz im Sync führt jedes Feld mit eigener Version
(„per Feld“). Bliebe eine Position unverändert, behielte sie ihre alte Version, und zwei
gleichzeitige Umordnungen auf zwei Geräten ergäben eine Mischung, die keiner gewollt hat. Mit frischen
Werten trägt jedes Konto die Version der neueren Umordnung, und die neuere Reihenfolge gewinnt als
Ganzes. Gleichstände (zwei Geräte im selben Augenblick) löst `created_at, id` überall gleich auf.
Die Zahlen wachsen mit jedem Verschieben um die Zahl der Konten; bei `i64` ohne Belang.

**3. Im Sync nur, wenn platziert.** Der Kontodatensatz trägt `position` erst, wenn der Mensch
verschoben hat (wie `protocol` nur bei JMAP-Konten). Ein Gerät mit 0.1.2 oder älter lehnt einen
Kontodatensatz mit `position` als unbekannte Struktur ab (`deny_unknown_fields`); der Datensatz
bleibt im Ordner, nichts geht verloren, nach dem Update wird er übernommen. Bis dahin lädt das alte
Gerät seine eigene Fassung erneut hoch und das neue Gerät räumt sie wieder ab (dieselbe Wirkung wie
bei Vorlagen mit Empfängern, 0033). Wer nie verschiebt, merkt davon nichts. Ein Datensatz ohne
`position` lässt die Position dieses Geräts stehen.

**4. `display_name` ist der Name in der Liste.** Geprüft, wo der Name eines Kontos hingeht:
Spalte, Leiste und jede Kontoliste der Oberfläche, Popup für neue Mails, `account:` in der Suche,
`list_accounts` über MCP, Diagnose (geschwärzt) und Sync (`displayName`). In gesendete Mails gelangte
er nur über zwei Wege: Beim Anlegen wurde er zum Absendernamen der ersten Identität kopiert (eine
Kopie, später unabhängig), und die KI-Antwort nannte ihn der KI als Namen des Nutzers. Der zweite Weg
ist geschlossen: Die KI bekommt den Absendernamen der Standardidentität. Damit reicht ein Feld; ein
eigenes Feld „Listenname“ neben einem dann toten `display_name` wäre doppelt. Der Absendername
bleibt allein bei den Identitäten, ein Listenname wie „Arbeit“ landet nie in einer Mail.

**5. Neue Konten zeigen ihre Adresse.** „Dein Name“ beim Einrichten wird nur noch Absendername der
ersten Identität; der Listenname eines neuen Kontos ist leer, die Listen zeigen die Adresse
(„Standard die Adresse“ aus dem Plan). Bestehende Konten behalten ihren Namen, so sieht die Spalte
nach dem Update aus wie vorher. Leer heißt überall: Adresse.

**6. Umbenennen und verschieben.** Name: getrimmt, höchstens 100 Zeichen, keine Steuerzeichen
(`invalid_argument`). In der linken Spalte per Doppelklick, F2 oder „Umbenennen“ im Menü des Kontos,
an Ort und Stelle (Enter oder Verlassen speichert, Escape bricht ab, die Statuszeile sagt, wie das
Konto jetzt heißt), auf der Seite des Kontos als Feld „Name in der Liste“. Reihenfolge: die
Kontozeile ziehen (eigener Datentyp `application/x-mail-account`, Ordner nehmen ihn nie, eine Linie
zeigt den Platz vor oder hinter einem Konto), „Nach oben“/„Nach unten“ im Menü, Alt+Pfeil hoch/runter
auf der Zeile, Knöpfe je Konto unter Einstellungen → Konten; der Fokus bleibt am Konto bzw. am Knopf,
die neue Stelle wird angesagt.

## Ungeprüft

- Zwei echte Geräte mit gleichzeitiger Umordnung; geprüft im Szenario-Test der Engine und mit dem
  Store.
- Ein Gerät mit 0.1.2 im selben Sync (Abschnitt 3 beschreibt das Verhalten nach dem Code der
  Ablehnung, nicht nach einem Lauf).

## Rückweg

Die Spalte bleibt; `ORDER BY created_at, id` zurück und das Feld `position` nicht mehr schreiben.
Ein eigener Listenname wäre eine neue Spalte plus Sync-Feld; `display_name` bliebe dann der Name aus
der Einrichtung.
