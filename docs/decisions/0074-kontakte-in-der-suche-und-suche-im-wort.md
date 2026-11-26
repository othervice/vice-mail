# 0074 — Kontakte in der Suche und Suche mitten im Wort

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkte 5 und 7 (vom Menschen am 26.11.2026 aus den Vorschlägen gewählt;
  Thunderbirds Suche nach Adressen fand er immer schlecht).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22).
- **Code:** Migration `0029_search_substrings.sql`; `crates/mail-model/src/search/compile.rs`
  (`TextMatch`, `TextClause`, `Substrings`, `contact_match`), `parse.rs` (`contact:`),
  `highlight.rs`, `server.rs`; `crates/mail-store/src/fts.rs` (beide Indexe, Job, Neuaufbau),
  `search.rs` (welcher Index die Suche führt), `views.rs`, `search_api.rs` (`full_text_sizes`);
  `crates/mail-engine/src/search/resolve.rs` (Indexzustand, `contact:` auflösen),
  `crates/mail-engine/src/compose/contacts.rs` (`search_addresses`), `search/bench.rs`;
  `app/src/features/search/SearchBar.tsx`, `ContactSuggestions.tsx`, `terms.ts`, `queries.ts`.
  Tests: `crates/mail-model/src/search/tests.rs` (`substring_table`, `contact_terms`, Fuzz),
  `crates/mail-store/src/substring_tests.rs`, `stage7_tests.rs`, `stage7_plan_tests.rs`,
  `views_tests.rs`, `crates/mail-engine/src/search/stage22_tests.rs`, `app/tests/search.test.tsx`,
  `app/tests/search-logic.test.ts`, `app/tests/e2e/search-contacts.e2e.mjs`.

## Punkt 7 — Suche mitten im Wort

### Ein zweiter Index neben dem Wortindex

`message_fts_tri` ist ein zweiter contentless FTS5-Index mit denselben Spalten und Zeilen-IDs wie
`message_fts` (Klasse Cache), zerlegt in überlappende Dreiergruppen von Zeichen
(`tokenize = 'trigram remove_diacritics 1'`). Eine Zeichenfolge ab drei Zeichen findet dort jede
Stelle, an der sie steht: `uster` findet „Muster“, `1234` die Nummer in „AB1234XY“. SQLite bildet
jeden Wert von `remove_diacritics` außer 0 auf dieselbe Faltung ab wie der Wortindex mit
`unicode61 remove_diacritics 2` (Groß-/Kleinschreibung, Akzente, kombinierende Zeichen); die Option
braucht SQLite 3.45, gebündelt ist 3.54.1 (geprüft im Quelltext von `libsqlite3-sys` 0.39.1).

Geschrieben wird er immer zusammen mit dem Wortindex (`fts::write`), in derselben Transaktion:
Kopfzeilen beim Speichern der Mail, Text und Dateinamen beim Speichern des Körpers, gelöscht über
einen eigenen Trigger. Ein Neuaufbau füllt beide Indexe neu (`message_fts_new`,
`message_fts_tri_new`) und tauscht beide in einer Transaktion. Der Job von Stufe 7 kennt ein drittes
Ziel, `message_fts_tri`: nur den Trigramm-Index füllen.

**Migration 0029** legt Tabelle und Trigger an, baut `search_index_job` mit der erweiterten
CHECK-Bedingung neu (eine Zeile, nichts verweist darauf) und legt bei vorhandenen Mails einen Job
`message_fts_tri` an: Vorhandene Mails werden im Hintergrund eingetragen wie nach Stufe 7, der Status
heißt so lange „Der Suchindex wird aufgebaut“. Ein noch laufendes Erstbefüllen (Ziel `message_fts`)
beginnt von vorn und füllt jetzt beide. Ein laufender Neuaufbau wird verworfen: Eine Migration kann
die zweite neue Tabelle nicht nur unter einer Bedingung anlegen, und die Suche benutzte bis dahin
ohnehin den vollständigen alten Index; der Neuaufbau lässt sich wieder starten.

### Der Mail-Text kommt hinein

Gemessen mit `search::bench` (100.000 Mails, 17.935 Körper lokal, Debug-Build, Einzelheiten in
`docs/testing/performance.md`, Stufe 22):

| | vorher | Trigramm nur Kopf und Dateinamen | Trigramm mit Text |
|---|---:|---:|---:|
| Datenbankdatei | 119 MB | 168 MB | 272 MB |
| davon Wortindex / Trigramm-Index | 33 MB / – | 33 / 48 MB | 33 / 153 MB |
| Körper speichern | 2.213/s | 1.373/s | 916/s |
| Neuaufbau beider Indexe | 18,2 s | 27,5 s | 27,2 s |
| langsamste Suche (Median, erste 50) | 20,7 ms (`-und`) | 30,2 ms (`-und`) | 29,3 ms (seltenes Wort) |

**Entschieden: Der Text kommt in den Trigramm-Index** (`SUBSTRING_BODY = true`).
- Was man mitten im Wort sucht, steht meist im Text: Bestell-, Rechnungs- und Sendungsnummern,
  deutsche Zusammensetzungen („auszug“ in „Kontoauszug“, das Beispiel aus 0003). Ohne den Text
  fände `1234` die Nummer nur in Betreff und Dateiname.
- Das Ziel bleibt weit erreicht: die langsamste Suche 29 ms im Debug-Build (Ziel 200 ms im
  Release-Build). Ein seltenes Wort wird langsamer (2,3 → 29 ms), weil seine Dreiergruppen
  häufige sind; Wörter beim Tippen und häufige Wörter bleiben bei 2–7 ms.
- **Kosten auf der Platte:** rund 3,7 Byte Index je Byte Text (der Wortindex braucht unter 1).
  Bei diesen 100.000 Mails wächst die Datenbank von 119 auf 272 MB; davon 48 MB für Kopfzeilen und
  Dateinamen aller Mails, 105 MB für den Text der 17.935 Körper (rund 6 KB je Körper). Es wächst
  mit dem lokal liegenden Text, nicht mit der Zahl der Mails: Wer „Vorab laden“ auf „Alles“ stellt,
  zahlt das für jede Mail. Der Text je Mail bleibt auf 256 KiB begrenzt wie im Wortindex.
- Speichern von Körpern wird langsamer (2,4-mal im Debug-Build); beim Vorausladen im Hintergrund
  fällt das nicht ins Gewicht, beim ersten Abgleich mit vielen Körpern etwas.
- **Rückweg:** `SUBSTRING_BODY = false` und ein Neuaufbau (dann 168 MB); die Suche nimmt dann für
  jedes Wort ab drei Zeichen beide Indexe (siehe unten), Text nur als ganzes Wort.

### Was ein Suchbegriff jetzt findet

- Ein Begriff **ohne Anführungszeichen mit mindestens drei Zeichen** (kombinierende Zeichen zählen
  nicht) findet wie bisher über den Wortindex (beim Tippen den Wortanfang) **oder** als Teilfolge
  über den Trigramm-Index. Kürzere Begriffe nur über den Wortindex: Der Trigramm-Index kann nichts
  unter drei Zeichen finden.
- **In Anführungszeichen** nur ganze Wörter, wie bisher: `"und"` findet „und“, nicht „Kunde“ oder
  „Grund“. So bleibt eine genaue Wortsuche möglich, und eine Phrase über einen Zeilenumbruch hinweg
  („Vielen⏎Dank“) findet weiter der Wortindex.
- Spaltenfilter (`from:`, `to:`, `cc:`, `subject:`, `filename:`) genauso, je Spalte.
- Ausschlüsse folgerichtig: `-uster` schließt jede Mail aus, in der „uster“ irgendwo steht.
- `OR` verbindet die Ausdrücke je Index.
- Hervorhebung: Ein solcher Begriff markiert zusätzlich jede Stelle, an der er im angezeigten Text
  steht (gleich gefaltet), nicht nur ganze Wörter.
- Die Treffer bleiben **neueste zuerst**: Beide Indexe haben `message.id` als Zeilen-ID.

**Vereinfachung, die nichts verliert:** Ein einzelnes Wort (nur Buchstaben und Ziffern) braucht den
Wortindex nicht, sobald der Trigramm-Index jede Mail enthält: Jeder Wort- oder Wortanfangstreffer ist
dann auch ein Teilfolgen-Treffer desselben gefalteten Textes. Solche Wörter einer Suche werden zu
einem `MATCH` auf dem Trigramm-Index verbunden (`max mustermann` ist eine Abfrage). Der Test
`a_single_word_finds_the_same_in_the_trigram_index_alone` prüft das mit Umlauten, `ß`, Ligaturen,
Griechisch, CJK, zerlegten Zeichen und Ziffern. Solange der Index noch gefüllt wird (Zustand
„wird aufgebaut“), nimmt jeder Begriff beide Indexe, damit kein Worttreffer fehlt; während eines
Neuaufbaus sind die alten Indexe vollständig.

### Wie die Abfrage läuft

`CompiledQuery::text` ist kein einzelner Ausdruck mehr, sondern `TextMatch { all, none }`: alle
Klauseln müssen passen, je Klausel ein Ausdruck für den Wortindex und/oder einen für den
Trigramm-Index (eine Klausel mit beiden heißt: in einem der beiden). Jede Eingabe bleibt ein
einziges FTS5-Zeichenkettenliteral wie seit Stufe 7; Syntax kommt nur aus dem Code.

Der Store führt die Suche über eine Klausel (bevorzugt eine mit nur dem Trigramm-Index, dann eine
mit nur dem Wortindex), rückwärts über die Zeilen-IDs mit Abbruch nach `limit + 1` wie bisher; die
Ausschlüsse dieses Index stehen im selben `MATCH` (`(…) NOT (…)`), alle anderen Klauseln als
`m.id IN (SELECT rowid … MATCH ?)` je Zeile (SQLite sammelt diese Treffer einmal je Abfrage). Hat
jede Klausel beide Indexe (`bob@example.com`, Wörter mit Satzzeichen), läuft sie als `UNION` zweier
Zweige, die SQLite ohne Sortieren nach Zeilen-ID zusammenführt (`MERGE (UNION)`, Plantest). Listen
mit Schnellfilter (Stufe 19) prüfen alle Klauseln je Zeile.

**Suche auf dem Server unverändert:** IMAP `SEARCH` sucht ohnehin Teilfolgen; die Kriterien bleiben
dieselben, und für Treffer vom Server wird der Text lokal nicht noch einmal geprüft (wie bisher).

## Punkt 5 — Kontakte in der Suche

### Vorschläge beim Tippen

Das Suchfeld ist eine Combobox (ARIA): Unter dem leeren Feld stehen wie bisher die letzten Suchen,
beim Tippen bis zu drei Kontakte, deren Name, Adresse oder Domain mit jedem Wort beginnt (die
Präfixsuche des Kontaktverzeichnisses aus Stufe 10/17, auch Umlaute ausgeschrieben). Gesucht wird
mit den einfachen Wörtern der Eingabe; Filter, Ausschlüsse und `OR` bleiben außen vor, mit einer
Phrase in Anführungszeichen gibt es keine Vorschläge. Alle Arten von Kontakten (auch Listen- und
Systemadressen: Die Adresse eines Shops findet dessen Belege). Der Fokus bleibt im Feld:
Pfeiltasten wählen (`aria-activedescendant`), Eingabe übernimmt, Escape schließt erst die Liste,
dann leert es das Feld, dann schließt es die Suche; ein Klick übernimmt auch. Die Volltextsuche zur
Eingabe läuft darunter weiter.

Die Wahl ersetzt die einfachen Wörter durch `contact:<Adresse>` (in der deutschen Oberfläche
`kontakt:`), Filter bleiben, der Bereich wird „Allen Konten“, und die Suche läuft wie nach der
Eingabetaste (zählt auch als letzte Suche). Der Chip heißt „Kontakt: …“.

### Der Filter `contact:`

Ein eigener Filter der Suchsprache (`contact:`, `kontakt:`), kein beim Wählen fest
zusammengesetztes `OR`: Die Engine löst ihn bei jeder Suche auf, in alle Adressen des Kontakts, der
die Adresse hat (auch zusammengeführte, jede wie geschrieben und als Schlüssel mit ASCII-Domain).
So folgen gespeicherte und letzte Suchen späteren Zusammenführungen. Eine Adresse ohne Kontakt
steht für sich, ein Wert, der keine Adresse ist, für seine Wörter in Absender, Empfängern und Kopie.

Gesucht wird im **Wortindex**, je Adresse eine Phrase in den Spalten `from`, `to` und `cc`
(`{from to cc} : "max@firma.de"`): Wortgrenzen machen sie genau, `an@firma.de` findet nicht
`jan@firma.de` (eine Teilfolgensuche täte das). Jede Adresse bleibt ein FTS5-Literal. Verworfen: die
Adressspalten der Nachrichten direkt (Empfänger stehen als JSON ohne Index, das hieße alle Mails
lesen) und ein beim Wählen festgeschriebenes `OR` (folgt keiner Zusammenführung).

`contact:` steht nicht in `OR`-Gruppen (wie die anderen Filter, die kein Text sind). Auf dem Server
wird es `OR FROM x (OR TO x CC x)` mit der geschriebenen Adresse; die anderen Adressen des Kontakts
kennt der Server nicht.

## Ungeprüft

- Release-Build: nicht gemessen (die Zahlen sind aus dem Debug-Build, vorher und nachher gleich
  gebaut; ein Release-Build mit LTO hätte einen zweiten Satz Abhängigkeiten auf die knappe Platte
  gelegt). Stufe 7 maß im Release-Build rund ein Zehntel der Debug-Zeiten.
- Kaltstart, echte Postfächer mit langen Texten, Windows.
