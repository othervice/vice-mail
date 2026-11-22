# 0013 — Suchsprache und Threads (reine Logik)

- **Datum:** 23.11.2026
- **Stufe:** 7, vorgezogen: nur die Logik ohne IO, ohne Migration, ohne Oberfläche
- **Status:** entschieden von der ausführenden Sitzung; Integration (Store, Engine, Oberfläche) folgt in Stufe 7
- **Code:** `crates/mail-model/src/search/` (Parser, FTS-Ausdruck, Filter, Datumsrechnung),
  `crates/mail-model/src/threading/` (Zuordnung, Betreff-Normalisierung, In-Memory-Index),
  `crates/mail-content/tests/seed_threads.rs` (Seed-Mails über `mail_content::parse`)

## Ort

Beides liegt in `mail-model`: ohne IO, ohne Tokio, ohne Abhängigkeit auf andere eigene Crates, und
von Engine, Store und später dem MCP-Server (Stufe 12) und der KI-Suche (Stufe 13) gebraucht.
Öffentlich als Module `mail_model::search` und `mail_model::threading` (nicht am Crate-Wurzel
re-exportiert, damit Namen wie `Term`, `Filter`, `Span` nicht mit anderen Typen kollidieren). Keine
neue Abhängigkeit: die Kalenderrechnung (Tage ↔ Datum nach Howard Hinnant, Monatsarithmetik) ist
selbst geschrieben.

Die Tests mit den Seed-Mails liegen in `mail-content/tests/`: `mail-content` hängt von `mail-model`
ab, eine Dev-Abhängigkeit in die Gegenrichtung wäre ein Zyklus (zwei Kopien von `mail-model` im
Testbinary). Die vorbereiteten Fälle selbst (auch der lange Seed-Thread) sind in `mail-model` als
Daten nachgebaut.

## Suchsprache

**Syntax.** Terme durch Leerraum getrennt; Steuerzeichen zählen als Leerraum.
- Freier Text (alle FTS-Spalten), `"Phrase"`, `wort*` und `"phrase"*` als Präfix des letzten Worts.
- `schlüssel:wert`, `schlüssel:"wert mit leerzeichen"`, auch `schlüssel: wert` (Leerzeichen nach
  dem Doppelpunkt; der nächste einfache Term wird der Wert).
- `-` vor Wort, Phrase oder Filter schließt aus.
- Schlüssel ohne Rücksicht auf Groß/Klein.
- Was nur wie ein Filter aussieht, bleibt Text ohne Fehler: `10:30` (Schlüssel mit Ziffern), `Re:`
  (kein Wert, damit eingefügte Betreffs funktionieren), `https://…` (Wert beginnt mit `//`).
  Ein unbekannter Schlüssel mit Wert (`form:bob`) wird gemeldet und als Text gesucht.

**Filter und deutsche Aliasse.** Aliasse ja, weil Outlook und Apple Mail in deutscher Sprache
`von:`/`an:`/`betreff:` kennen; nur für die häufigen Schlüssel, `older_than:`/`newer_than:` nicht
(`älter_als:` tippt niemand).

| Filter | Aliasse | Werte | wird zu |
|---|---|---|---|
| `from:` `to:` `cc:` `subject:` `filename:` | `von:` `an:` – `betreff:` `datei:` | Text | FTS-Spaltenfilter `from` `to` `cc` `subject` `attachments` |
| `in:` | `ordner:` | Name; Rollenwörter `inbox`/`posteingang`, `sent`/`gesendet`, `drafts`/`entwürfe`, `archive`/`archiv`, `trash`/`papierkorb`, `junk`/`spam` | `MailboxSpec { name, role }` |
| `account:` | `konto:` | wie getippt | Text, die Engine löst Adresse/Name/UUID auf |
| `has:attachment` | `hat:anhang` | auch `attachments`, `anhänge` | `has_attachment` |
| `is:unread` `is:read` `is:flagged` `is:snoozed` | `ist:ungelesen` `gelesen` `markiert` `zurückgestellt` | auch `starred` | `unread`/`flagged`/`snoozed`; `is:read` = `-is:unread` |
| `before:` `after:` | `vor:` `nach:` | Datum (unten) | rowid-Bereich |
| `older_than:` `newer_than:` | – | `7d` `2w` `3m` `1y` (auch `t`, `j`) | rowid-Bereich |

**`OR` in Version 1: ja, nur zwischen Text-Termen** (freier Text und die fünf Textfilter), groß
geschrieben, ohne Klammern; bindet stärker als das implizite UND (`a b OR c` = `a UND (b ODER c)`,
wie Gmail). Gründe: FTS5 kann es ohne Mehrkosten, und 0003 schlägt für `ß`/`ss` genau das vor
(`grüße OR grüsse`). `OR` neben `in:`, `is:`, Datum usw. würde die schnelle Abfrage (FTS plus Filter im
Join) zerbrechen und wird als UND gelesen und gemeldet, ebenso `a OR -b`. `AND` darf geschrieben
werden und ändert nichts; `NOT` ist ein normales Wort (Ausschluss ist `-`). Kleines `or` ist ein Wort.

**Datumsangaben** (für `before:`/`after:`), alle Tage im lokalen Kalender des Nutzers:
- ISO `2026-11-22`, auch `2026/11/22`, `2026.11.22`
- deutsch `22.11.2026`, `22.11.26`, `22.11.` (laufendes Jahr), `1.3.2026`
- Monatsnamen deutsch und englisch, mit und ohne Punkt: `22. Nov 2026`, `22.Nov.2026`,
  `22-nov-2026`, `3. März`, `Nov 22, 2026`, `November 22` (mit Leerzeichen in Anführungszeichen)
- `today`/`heute`, `yesterday`/`gestern`
- **Jahr zuletzt mit `/` oder `-`** (`01/02/2026`, `22-11-2026`) ist mehrdeutig und wird nach einem
  Parameter gelesen (`SlashDateOrder`: Tag zuerst, Standard; Monat zuerst für US-Region). Nie
  geraten, nie still vertauscht: `11/22/2026` mit „Tag zuerst“ ist ein Fehler.
- Zweistellige Jahre nach POSIX `%y`: 69–99 → 1969–1999, 00–68 → 2000–2068.
- Nur Tage, keine Monate (`2026-11`) oder Jahre allein, keine Uhrzeiten.

**Bedeutung der Daten** (wie Gmail): `after:D` = ab D 00:00 (einschließlich), `before:D` = vor
D 00:00 (ausschließlich). `newer_than:7d` = ab jetzt minus 7 Tage, `older_than:7d` = davor. Monate und
Jahre zählen im Kalender (31.3. minus `1m` = 28./29.2.), die Uhrzeit bleibt über eine
Sommerzeitumstellung gleich. Verneint dreht die Grenze: `-after:D` = `before:D`. Mehrere Grenzen
schneiden sich. Der rowid-Bereich ist sekundengenau bis auf Mails, deren `id` bei einer Kollision
nach oben gerückt ist (0003).

**Zeitzone.** Der Parser bekommt `SearchContext { now, time_zone, slash_dates }`; `time_zone` ist
ein Trait (`utc_offset(utc) -> i32`), damit Mitternacht auch über die Sommerzeit hinweg stimmt.
`FixedOffset` gibt es für Tests und als Notbehelf.

**Präfix.** `*` nur nach mindestens 3 Zeichen des letzten Worts (`MIN_PREFIX_CHARS`); kürzere
Präfixe treffen zu viele Index-Einträge (0003 misst schon `rech*` mit 12 ms). Beim Tippen
(`CompileOptions::prefix_last_word`) wird das Wort, in dem die Eingabe endet, als Präfix gesucht,
wenn es ungequotet ist, mit einem Buchstaben oder einer Ziffer endet und die Mindestlänge hat.

**Fehler.** Der Parser bricht nie ab. Jeder Fehler hat eine Art und einen Bereich (Bytes; für die
Oberfläche `Span::to_utf16`). Der Term wird verworfen oder als Text gelesen, der Rest sucht weiter:
offenes Anführungszeichen (Phrase bis zum Ende), Filter ohne Wert, unbekannter Filter, unbekannter
Wert (`is:foo`), ungültiges Datum, ungültiges Alter, Term ohne Buchstaben und Ziffern (`!!!`, Emoji),
zu kurzer Präfix, `-` allein, falsch gesetztes `OR`, `OR` neben Nicht-Text, `OR` mit Ausschluss,
mehr als 2048 Bytes, mehr als 64 Terme. Widersprüche (`is:unread is:read`, `in:inbox -in:Posteingang`,
leerer Datumsbereich) meldet `CompiledQuery::conflicts`; die Abfrage hat dann keine Treffer.

**FTS5-Ausdruck** (`ParsedQuery::compile`):
- Jeder Text-Term ist genau ein String-Literal `"…"` mit verdoppelten inneren Anführungszeichen;
  davor höchstens `{spalte} : `, danach höchstens ` *`. Alle Syntax kommt aus dem Code (`AND`, `OR`,
  `NOT`, Klammern, Spaltennamen aus `FtsColumn`), nichts aus der Eingabe. `NEAR(`, `^`, `{from}:`,
  `col:x` aus der Eingabe landen als Text im Literal.
- Terme ohne Token-Zeichen werden nie ausgegeben: FTS5 behandelt eine leere Phrase als „trifft
  nichts“ (`"a" AND ""` findet nichts, mit sqlite3 3.54 geprüft).
- Form: `P1 AND P2 AND (o1 OR o2)`, mit Ausschlüssen `(…) NOT (n1 OR n2)`.
- Nur Ausschlüsse (`-spam`): FTS5 hat kein einstelliges NOT; `TextMatch::Exclude(ausdruck)` heißt
  für den Store `rowid NOT IN (SELECT rowid FROM message_fts WHERE message_fts MATCH ?)`.
- Text bleibt unverändert; Groß/Klein und Diakritika faltet der Tokenizer
  (`unicode61 remove_diacritics 2`).

Alles andere kommt als Daten (`CompiledQuery`): Konten und ausgeschlossene Konten, Postfächer und
ausgeschlossene Postfächer, `RowidRange { from (>=), until (<) }`, `unread`/`flagged`/`snoozed`/
`has_attachment` als `Option<bool>`. SQL dafür baut `mail-store`.

## Threads

**Über References.** Ein Thread ist die Menge der Message-IDs, die seine Mails tragen oder nennen:
eigene `Message-ID`, `In-Reply-To` (höchstens 4) und aus `References` die erste und die letzten 20
(so kürzen auch die Clients). Eine neue Mail kommt in den Thread, der eine ihrer IDs schon hält;
halten mehrere Threads ihre IDs (die fehlende Zwischen-Mail ist da), werden sie in den ältesten
(kleinste Thread-ID) zusammengeführt. IDs noch nicht vorhandener Mails werden mit gespeichert:
Antwort vor dem Original, Geschwister ohne Original finden sich. Weil das eine Vereinigung von
Mengen ist und kein Baum, sind Zyklen und Selbstbezüge harmlos; das Ergebnis über References hängt
nicht von der Reihenfolge ab. IDs werden robust gelesen: alles in `<…>`, ohne Klammern nur Wörter
mit `@` (so wird „Your message of Monday“ nie zur ID); leer, mit Leerraum oder länger als 250 Bytes
zählt nicht.

**Wiederverwendete Message-ID** (Massenversender): Hat die vorhandene Mail mit dieser ID einen
anderen normalisierten Betreff, verbindet die eigene ID nicht. Die zuerst gesehene Mail besitzt die
ID; Antworten, die sie nennen, kommen in deren Thread. Im ersten Abgleich (neu nach alt) kann das
die spätere Mail sein; ein Neuaufbau nach Datum ist kanonisch. Mails mit gleicher ID in zwei Ordnern
sind im Store ohnehin eine `message`-Zeile (0003).

**Betreff-Normalisierung.** Leerraum zusammenfassen, dann vorne beliebig oft und gemischt entfernen:
Antwort-Präfixe (Re, AW, Antw, Antwort, SV, VS, Odp, Ynt, RES, Rif, 回复, 回覆, 答复), Weiterleitungs-
Präfixe (Fwd, FW, WG, TR, RV, ENC, PD, VB, VL, Doorst, 转发, 轉寄), auch `Re[2]:`, `Re(2):`, `Re^2:`,
`RE :` und der Doppelpunkt `：`; `[Tags]` ohne Ziffern (`[rust-users]`, `[EXTERN]`). Tags mit
Ziffern bleiben (`[Ticket #123]`), damit verschiedene Tickets mit gleichem Text getrennt bleiben.
Dann klein. Diakritika bleiben (`Grüße` ≠ `Grusse`).

**Rückfall auf den Betreff**, nur für eine Mail ganz ohne Referenz-Kopfzeilen, wenn alles gilt:
1. gleicher normalisierter Betreff, nicht leer und **spezifisch**: nicht in einer Liste generischer
   Betreffs (Hallo, Test, Frage, Termin, Rechnung, Angebot, Danke, Quick question, Kein Betreff …,
   Englisch und Deutsch) und entweder zwei Wörter oder ein Wort mit mindestens 12 Zeichen;
2. eine der beiden ist eine **Antwort ohne References** (Antwort-Präfix) und nicht älter als die
   andere (eine Stunde Uhrenabweichung erlaubt). Zwei Originale mit gleichem Betreff sind zwei
   Gespräche; eine Weiterleitung ist keine Antwort;
3. höchstens **7 Tage** auseinander. Antworten ohne References (ältere Outlook-Versionen, manche
   Mobil-Clients) kommen meist innerhalb von Tagen; wöchentliche Mails mit festem Betreff liegen
   genau 7 Tage auseinander und brauchen zusätzlich Punkt 2 und 4;
4. ein **gemeinsamer Teilnehmer** (From, To, Cc) außer den eigenen Adressen des Kontos
   (`ThreadConfig::own_addresses`), denn die stehen in fast jeder Mail;
5. **keine Liste, kein Massenversand** auf beiden Seiten (`List-Id`, `List-Unsubscribe`,
   `Precedence: bulk/list/junk`, `Auto-Submitted` außer `no`). Strenger als „nicht bei
   unterschiedlicher List-Id“: Listen teilen die Listenadresse, Betreffs wiederholen sich, und
   Listensoftware erhält die References ohnehin.

Die zeitlich nächste passende Mail gewinnt (bei Gleichstand die kleinere Thread-ID); der Betreff
fügt höchstens einen Thread hinzu. Ein Original ohne References, das über seine eigene ID schon in
einem Thread hängt, sucht trotzdem nach einer passenden Antwort ohne References außerhalb und führt
deren Thread zu: dann ist egal, wer zuerst kam. **Verbleibende Reihenfolge-Abhängigkeit:** Eine
Antwort ohne References, die vor allen Mails ihres Gesprächs in ihrer Nähe eintrifft, bleibt allein,
wenn kein Original ohne References nachkommt. Sie wird nie falsch zusammengeführt.

**Nie über Konten hinweg:** Der Zustand ist je Konto (`ThreadIndex` je Konto, im Store
`account_id = ?` in jeder Abfrage).

**Schnittstelle.** `assign_thread(lookup, config, message) -> ThreadAssignment` entscheidet
deterministisch; `ThreadLookup` liefert drei Abfragen (Threads zu IDs, Betreffs zu einer eigenen
Message-ID, Mails mit Betreff-Schlüssel im Zeitfenster) mit eigenem Fehlertyp. `ThreadAssignment`
sagt, was zu schreiben ist: Ziel (vorhandener oder neuer Thread), aufgelöste Threads, zu speichernde
IDs, Betreff-Schlüssel, Grund. `ThreadIndex` ist die In-Memory-Umsetzung (Tests, Neuaufbau).

## Folgen für die Integration in Stufe 7

- **Store-Migration:** `message_fts` mit genau den Spalten `subject, "from", "to", cc, body,
  attachments` (Namen aus `FtsColumn::name`; `from`/`to` in DDL und `INSERT` quoten, in `MATCH` nicht).
  Neue Tabelle für Thread-IDs, etwa `thread_ref(account_id, ref_id, thread_id)` mit Schlüssel
  `(account_id, ref_id)`, für `threads_with_ids`. Spalte `message.subject_key` (aus
  `ThreadAssignment::subject_key` bzw. `threading::subject_key`) mit Index
  `(account_id, subject_key, id)` für `messages_with_subject`; `subjects_with_message_id` nutzt den
  vorhandenen Index `message_msgid`. Ein `ThreadMessage` lässt sich aus der `message`-Zeile bauen.
- **Store-Abfrage:** `TextMatch::Match` als `MATCH ?`, `Exclude` als `rowid NOT IN (…)`; `RowidRange`
  als `rowid >= ? AND rowid < ?`; `in:` über `message_location` (Rolle vor Name; Name als ganzer Pfad
  oder letzter Teil, ohne Groß/Klein); `is:read` als „nicht ungelesen“ (Semantik über mehrere Orte
  festlegen); `is:snoozed` braucht die Snooze-Tabelle (Stufe 9). Plan-Tests wie in 0003.
- **Engine:** liefert `now`, die lokale Zeitzone (`TimeZone`) und die Datumsreihenfolge aus der
  Region; löst `account:` gegen Adresse, Name und UUID auf; übergibt die eigenen Adressen (Konto und
  Identitäten) an `ThreadConfig`. Beim Löschen werden Threads nicht geteilt (die Vereinigung kann
  nicht zurück); ist ein Thread leer, verschwinden Zeile und IDs. Ein Neuaufbau nach Datum
  aufsteigend gibt das kanonische Ergebnis.
- **Oberfläche:** Fehlerarten brauchen Texte in `app/src/i18n/` (Englisch und Deutsch) und eigene
  IPC-Typen (Code plus UTF-16-Bereich); die Suchtypen sind noch nicht `specta`-exportiert.
- **MCP/KI:** kann dieselbe Sprache nutzen; `ParsedQuery::terms()` liefert die Terme für Chips.

## Geprüft

- `cargo test -p mail-model`: Tabellentests für jeden Filter, Aliasse, Kombinationen und fehlerhafte
  Eingaben, FTS-Ausdrücke einschließlich Injektionsversuchen, Datumsformen und -rechnung (auch
  Sommerzeit), Zufallseingaben (20.000 Runden: keine Panik, gültige Bereiche, nur erlaubte
  FTS5-Syntax); Thread-Fälle: Mailingliste, kaputte References (fehlend, vertauscht, zyklisch,
  abgeschnitten, sehr lang, Müll), gleicher Betreff ohne Zusammenhang, Antwort vor Original,
  Outlook ohne References, Weiterleitungen, wiederverwendete IDs, Konten, der 60er-Thread des Seeds
  in 44 Reihenfolgen.
- `cargo test -p mail-content --test seed_threads`: die Seed-Fixtures über `mail_content::parse`
  bleiben getrennt (beide Reihenfolgen); `--ignored`: frischer Dump `seed.py 1000` (995 Mails von
  alice) gruppiert in Datumsreihenfolge genau wie eine unabhängige Gruppierung über References,
  ohne einen einzigen Betreff-Treffer, obwohl rund 50 Füll-Mails `Re:`/`AW:` ohne References tragen.
- Außerhalb des Repos (Wegwerf-Programm gegen rusqlite 0.41, SQLite 3.54.1): 87.682 erzeugte
  Ausdrücke aus Zufallseingaben ohne FTS5-Syntaxfehler, Treffer der Beispielabfragen plausibel.

**Ungeprüft:** echte Postfächer; Verhalten mit echter lokaler Zeitzone (nur feste Offsets und eine
nachgebaute Berliner Sommerzeit); Laufzeit der Lookups im Store (es gibt sie noch nicht).

## Rückweg

- Weitere Aliasse, Datumsformen oder `OR` mit Klammern lassen sich im Parser ergänzen, ohne dass
  sich `CompiledQuery` ändert.
- Ist der Betreff-Rückfall zu streng oder zu locker, sind Fenster, Generika-Liste und Bedingungen
  Konstanten in `threading`; ein Neuaufbau der Threads aus `message` wendet sie auf alles an.
- Outlook schreibt `Thread-Index`/`Thread-Topic`; diese Kopfzeilen holt der Adapter noch nicht. Sie
  wären ein besserer Rückfall als der Betreff.
