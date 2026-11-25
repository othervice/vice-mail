# 0061 — Liste sortieren und als Tabelle zeigen

- **Datum:** 24./25.11.2026
- **Anlass:** Stufe 19, Aufgabe A (Vergleich mit Thunderbird vom 24.11.2026, gewählt „Sortieren +
  Schnellfilter“): Liste sortieren nach Datum, Absender, Empfänger (Gesendet, Entwürfe), Betreff
  und Größe, auf- und absteigend, je Ansicht gespeichert; Gespräche nach ihrer neuesten Mail; eine
  Tabellenansicht der breiten Liste mit sortierenden Spaltenköpfen, ziehbaren Breiten und wählbaren
  Spalten. Leistungsziele aus `docs/SPEC.md` gelten weiter (500.000 Mails).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 19, Aufgabe A)
- **Code:** `crates/mail-model/src/list_view.rs` (Sortschlüssel, Typen),
  `crates/mail-store/migrations/0026_list_sort.sql`, `crates/mail-store/src/sort_keys.rs`,
  `crates/mail-store/src/views.rs`, `crates/mail-engine/src/list_views.rs`,
  `app/src-tauri/src/commands/views.rs`, `app/src/features/views/` (`order.ts`, `table.ts`,
  `TableHeader.tsx`, `ViewMenu.tsx`, `source.tsx`, `useListView.tsx`),
  `app/src/features/messages/MessageRow.tsx` (`TableRowContent`), `MessageList.tsx` (`table`)
- **Tests:** `views_tests.rs` (jede Reihenfolge, beide Richtungen, Seiten ohne Lücke, Gleichstand,
  leere Werte, Faltung, Gespräche, vereinte Ansicht, Migration, Plan jeder Abfrage),
  `list_views_tests.rs` (Kern), `stage_19_list_view_commands_are_wired` (Brücke),
  Vitest `list-views.test.tsx` (mit axe hell und dunkel), Desktop `list-views.e2e.mjs`,
  Messung `search::bench`, `bench_migration_0026_sort_keys`, `perf.e2e.mjs` (Abschnitt Stufe 19)

## Entscheidung

**1. Sortschlüssel an jedem Ort einer Mail, je ein Index.** `message_location` bekommt die
Schlüssel ihrer Mail: `sort_sender` (der Name, den die Zeile zeigt: Anzeigename, sonst Adresse),
`sort_recipient` (der erste Empfänger ebenso), `sort_subject` (Betreff ohne `Re:`, `AW:`, `Fwd:`,
`WG:` …, `display_subject` aus Stufe 17) und `size`, mit je einem Index
`(mailbox_id, Schlüssel, message_id)`. Eine sortierte Seite ist damit wie die Seite „neueste
zuerst“ ein Bereich eines Index der Ortstabelle, Keyset über `(Schlüssel, message_id)` als
Zeilenwert, nie eine Sortierung des ganzen Ordners (0003). Datum aufsteigend läuft rückwärts über
den Schlüssel der Ortstabelle, ohne Index. Die Schlüssel ändern sich nie (Absender, Empfänger,
Betreff und Größe einer Mail stehen fest); `NOT NULL`, damit kein Keyset-Vergleich auf NULL
trifft.

**2. Faltung in Rust, aus SQL aufgerufen.** `mail_model::sort_text`: Kleinbuchstaben, Akzente
lateinischer Buchstaben weg (Latin-1 und Latin Extended-A per Tabelle, zerlegte Akzente ebenso,
`ß` → `ss`, `æ` → `ae`), Satzzeichen vor dem ersten Buchstaben oder der ersten Ziffer weg
(`"Bob"` steht unter B), Leerraum zusammengefasst, höchstens 40 Zeichen. Das entspricht DIN 5007-1
(ä wie a). Der Store meldet drei Funktionen an seiner Schreibverbindung an (`vm_sort_name`,
`vm_sort_recipient`, `vm_sort_subject`, deterministisch und `INNOCUOUS`); jede Anweisung, die einen
Ort einfügt (`INSERT_LOCATION`, `INSERT_LOCATION_IF_FREE`), füllt die Spalten aus der Zeile von
`message` (`INSERT … SELECT`), und die Migration 0026 füllt die vorhandenen Orte mit denselben
Funktionen. Dafür braucht rusqlite sein Feature `functions` (kein neues Paket).

**3. Gleichstand nach Datum in derselben Richtung.** A bis Z heißt bei gleichem Absender älteste
zuerst, Z bis A neueste zuerst. Das ist der Index in einer Richtung; Neueste-zuerst bei A bis Z
bräuchte einen zweiten Index je Schlüssel oder einen Index mit gemischter Richtung, der dann Z bis A
verdirbt.

**4. Neueste zuerst bleibt die eigene Liste jeder Ansicht.** Solange eine Ansicht neueste zuerst
sortiert und nichts filtert (oder nur „Ungelesen“, die Liste von Stufe 16), bleibt alles, wie es
war: Datumsabschnitte, Prioritätsblock, Bündel, angepinnte und zurückgekommene Mails oben,
„Ungelesene oben“. Erst eine andere Reihenfolge (oder ein Schnellfilter, 0062) holt die Seiten über
die neuen Befehle `listViewMessages` / `listViewConversations`. Diese Liste ist schlicht: keine
Bündel, kein Block oben; in Datumsreihenfolge mit Abschnitten, sonst ohne (jede Zeile nennt ihr
Datum). „Ungelesene oben“ wartet in einer anderen Reihenfolge; das Menü sagt es („nur bei
‚Neueste zuerst‘“).

**5. Posteingänge behalten ihre Regeln.** Im Posteingang und in vereinten Ansichten fehlen wie in
den gefilterten Posteingangslisten (Stufe 9/16) zurückgestellte Mails (bis sie zurückkommen) und
Mails blockierter Absender; angepinnte stehen an ihrem Platz in der Reihenfolge. Ordner ohne Rolle
„Posteingang“ zeigen alle Mails wie ihre Liste „neueste zuerst“.

**6. Gespräche nach ihrer neuesten Mail.** Eine Gesprächszeile ist wie in 0039 die neueste Mail
des Threads im Ordner, die zur Liste gehört (bei Filtern: die neueste passende); sie steht an der
Stelle ihres eigenen Schlüssels. Zahl, Ungelesene und Teilnehmer zählen das ganze Gespräch. Kinder
eines aufgeklappten Gesprächs bleiben in ihrer Reihenfolge (älteste zuerst, 0049).

**7. Gespeichert je Ansicht, nur auf diesem Gerät.** `ui.list_orders`: je Ansichtsschlüssel (wie
bei „Ungelesene oben“: `mailbox:<id>`, `unified:all`, `group:<id>`) die Reihenfolge; „neueste
zuerst“ wird nicht gespeichert, höchstens 1.000 Ansichten. Nicht im Einstellungs-Sync: Die
Schlüssel nennen lokale Ordner-IDs, die es auf einem anderen Gerät nicht gibt. Empfänger stehen
im Menü nur in Gesendet und Entwürfen (dort nennen die Zeilen die Empfänger) oder wenn die Ansicht
schon danach sortiert ist; der Spaltenkopf „Empfänger“ der Tabelle sortiert überall.

**8. Tabelle als Darstellung der breiten Liste.** `ui.list_table` (nur auf diesem Gerät, wie die
Größen des Fensters, 0025): Darstellung Zeilen (Standard) oder Tabelle, je Spalte sichtbar und
Breite (48 bis 800 px bei 100 % Schrift). Spalten: Markiert, Anhang, Absender (in Gesendet und
Entwürfen „Empfänger“: wer die Zeile nennt), Empfänger (aus), Betreff (immer, nimmt den Rest),
Konto (nur wo sich Konten mischen), Datum, Größe. Schmale Listen (neben dem Lesebereich, unter
720 px) zeigen immer Zeilen. Gewählt im Ansichtsmenü des Kopfes und unter Einstellungen →
Darstellung; Spalten ein- und ausblenden im Ansichtsmenü.

**9. Kopf als eigene Tabelle, die Liste bleibt Liste.** Über der Liste steht eine ARIA-Tabelle mit
einer Kopfzeile (`columnheader`, `aria-sort` am sortierten Kopf, benannt nach der Spalte). Ein
Klick auf einen sortierbaren Kopf sortiert danach (Startrichtung wie im Menü: Namen und Betreff A
bis Z, Datum und Größe absteigend), ein zweiter dreht die Richtung. Am Ende jeder ziehbaren Spalte
ein Fenster-Trenner wie an der Grenze zum Lesebereich (`ResizeHandle`, ← → 16 px, Pos1, Ende,
Doppelklick auf den Standard); die Breite wirkt sofort und wird gespeichert, sobald sie 400 ms
ruht. Die Liste bleibt `listbox` bzw. `tree` mit den Namen der Zeilen aus Stufe 8/17 (die
Tabellenzeile ist nur gezeichnet, `aria-hidden`); Tastatur, Auswahl, Aufklappen und
Schnellaktionen sind dieselben. Spaltenbreiten gehen als `--table-columns` (CSS-Grid) per CSSOM an
Kopf und Liste (CSP); beide halten denselben Rand für die Bildlaufleiste frei.

**10. Größe in der Listenzeile.** `MessageListItem.size` (Bytes, fehlt, wenn unbekannt) für die
Spalte Größe; jede Listenabfrage liest `m.size` als Spalte 14.

## Verworfen

- **In Rust im Speicher sortieren** (alle Orte eines Ordners lesen und sortieren, je Seite oder
  zwischengespeichert): 500.000 Zeilen je Seite oder ein Zwischenspeicher, der bei jeder Änderung
  des Abgleichs veraltet.
- **Index auf `message` (Konto, Schlüssel, id) und den Ort per Probe prüfen:** schnell für große
  Ordner, für kleine Ordner eines großen Kontos ein Lauf über fast das ganze Konto je Seite.
- **ICU-Sortierung oder eine SQLite-Kollation:** eine eigene Kollation macht die Indizes für jede
  Verbindung ohne sie unlesbar; ICU in `mail-model` bringt eine schwere Abhängigkeit in die Typen.
  Die Tabelle deckt alle lateinisch geschriebenen europäischen Sprachen ab; andere Schriften
  sortieren nach Codepunkt der Kleinbuchstaben.
- **Faltung in SQL** (`lower` und `replace`-Ketten): rund 140 `replace` je Wert, beim Abgleich
  messbar langsamer, und `lower` kennt nur ASCII.
- **Trigger, der die Schlüssel nachträgt:** wie der Kategorie-Trigger (Stufe 8/9) ein zweites
  Schreiben jeder neuen Zeile samt Indizes; außerdem bräuchte jede Verbindung, die schreibt, die
  Funktionen.
- **Tabelle als `grid`/`treegrid`:** würde Spalten zu Tab-Stopps und die Liste zu einem anderen
  Widget machen; Stufe 8 hat die Liste bewusst als `listbox` gebaut (siehe `MessageList`).
- **Reihenfolge im Einstellungs-Sync:** lokale IDs; ein stabiler Schlüssel (Konto-UUID und Pfad)
  wäre eine eigene Aufgabe, wie für „Ungelesene oben“.

## Folgen

- **Speicher und Schreiben:** vier Spalten und vier Indizes mehr je Ort. Gemessen (siehe
  `docs/testing/performance.md`, Stufe 19): bei 100.000 Mails 8,5 MB Indizes (500.000: 42 MB);
  die Datenbank des Benchmarks wächst von 98 auf 118 MB.
- **Migration 0026:** einmal beim ersten Start nach dem Update, mit Sicherung vorher: 1,2 s bei
  100.000, 7,9 s bei 500.000 Orten (Sicherung eingeschlossen, gemessen unter Last).
- **Seiten:** eine sortierte Seite kostet wie eine Seite „neueste zuerst“ (0,3 ms bei 100.000
  Mails im Kern), als Gespräche wie die Gespräche von 0039.
- **Neue Pfade, die Orte einfügen,** müssen die Schlüssel füllen (`INSERT … SELECT` mit den
  Funktionen); `views_tests::every_location_gets_its_sort_keys` prüft die bestehenden.
- Die Einträge der Migrationsliste zählen die Version: 0026 folgt in `MIGRATIONS` auf die
  Migrationen 0021 bis 0025 der parallelen Aufgaben, wenn sie zusammengeführt werden.

## Rückweg

Die Befehle `listViewMessages`/`listViewConversations` und die Darstellung lassen sich in der
Oberfläche abschalten, ohne dass sich die Listen „neueste zuerst“ ändern. Die Spalten und Indizes
ließen sich mit einer weiteren Migration entfernen; nichts anderes liest sie.
