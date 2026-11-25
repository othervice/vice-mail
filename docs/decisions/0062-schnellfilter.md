# 0062 — Schnellfilter über der Liste

- **Datum:** 24./25.11.2026
- **Anlass:** Stufe 19, Aufgabe A: „Schnellfilter-Leiste über der Liste: Ungelesen, Markiert, Mit
  Anhang, Von Kontakten, Schlagwort, Textfeld (Absender, Betreff, Text); kombinierbar, mit
  ‚Zurücksetzen‘, per Tastatur erreichbar; gilt für die aktuelle Ansicht und zeigt die
  Trefferzahl.“ Vorgabe der Auftraggeberin: die Operatoren der Suche wiederverwenden statt einer
  zweiten Filterlogik.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 19, Aufgabe A)
- **Code:** `crates/mail-engine/src/list_views.rs` (`view_query`), `crates/mail-store/src/views.rs`
  (`row_conditions`, `count`, `tags`), `crates/mail-store/src/search.rs` (`conditions_on`),
  `app/src/features/views/QuickFilterBar.tsx`, `quickFilter.ts`, `focus.ts`, `useListView.tsx`,
  `app/src/features/organize/unread.tsx` (Schalter „Nur ungelesene“ entfällt im Kopf)
- **Tests:** `views_tests::quick_filters_combine_and_count`, `list_views_tests.rs`
  (`a_folder_sorts_and_filters_like_a_search_typed_in_it`, Widersprüche, ungültige Schlagwörter),
  Vitest `list-views.test.tsx` (Leiste, Tastatur, axe), `organize.test.tsx` (Stufe 16 über die
  Leiste), Desktop `list-views.e2e.mjs` (Teil 3)

## Entscheidung

**1. Ein Filter aus der Suche.** Der Text der Leiste ist Suchsprache (0013): Er wird mit dem Parser
und Compiler der Suche übersetzt, als hätte man ihn in dieser Ansicht gesucht (Bereich wie die
Ansicht, `account:`, `in:`, Datumsangaben, `from:`, `has:`, `is:` …), das letzte Wort als Präfix
wie beim Tippen in der Suche. Wörter treffen also, was die Suche trifft: Absender, Empfänger,
Betreff, Text und Dateinamen. Die Schalter setzen die Felder des übersetzten Filters (`unread`,
`flagged`, `has_attachment`); widerspricht ein Schalter dem Text (`is:read` und „Ungelesen“), passt
nichts. Der Store prüft dieselben Bedingungen (`search::conditions_on`, jetzt mit wählbarem Alias,
damit Gespräche sie auch auf die neuere Mail eines Threads anwenden) und den Text als
`l.message_id IN (… message_fts MATCH ?)`, auf den Orten der Ansicht in ihrer Reihenfolge (0061).
Ungelesen und Markiert gelten für den Ort in dieser Ansicht, wie die Zeile es zeigt.

**2. Zwei Bedingungen ohne Operator.** „Schlagwort“: ein IMAP-Keyword des Orts, ohne Unterschied
der ASCII-Groß- und Kleinschreibung; das Menü zeigt die Schlagwörter der Ansicht (ohne die mit `$`,
höchstens 200, `listViewTags`). „Von Kontakten“: der Absender ist ein Kontakt, an den du schon
geschrieben hast (`sent > 0`) oder den du benannt, notiert, zusammengeführt oder mit Bild versehen
hast. Seit Stufe 17 ist jede Adresse ein Kontakt (0050); „jede Adresse, von der Mail kam“ würde
nichts filtern. Beides gibt es nicht als Operator in der Suchsprache (siehe Verworfen).

**3. Trefferzahl.** Gezählt werden Mails der Ansicht, die passen (eine Mail in zwei Posteingängen
einer vereinten Ansicht zweimal), auch wenn die Liste Gespräche zeigt. Eigener Befehl
`countListView`, neben der ersten Seite geladen; die Leiste zeigt „N Treffer“ (`role="status"`),
bis die Zahl da ist „Wird gezählt …“, und behält die alte Zahl, bis die neue da ist.

**4. „Nur ungelesene“ ist jetzt „Ungelesen“ der Leiste.** Der Schalter im Kopf (Stufe 16) zieht in
die Leiste: derselbe Zustand je Ansicht, nur für diese Sitzung. Allein und bei „neueste zuerst“
bleibt es die Liste von Stufe 16 (mit ihrer leeren Ansicht „Keine ungelesenen Mails“); zusammen mit
anderen Teilen oder einer anderen Reihenfolge läuft es über die neue Liste. In beiden Fällen bleibt
die geöffnete Mail an ihrem Platz, wenn das Öffnen sie gelesen macht, bis die Auswahl weitergeht
(in der neuen Liste an der Stelle, die sie hatte, nicht nach Datum).

**5. Bedienung.** Die Leiste steht über der Liste von Ordnern, Posteingängen und vereinten
Ansichten (nicht über Später, Angepinnt, Aufgaben, Zum Lesen, Markiert, Suche). Die vier Schalter
und das Menü „Schlagwort“ sind eine Werkzeugleiste (ein Tab-Stopp, ← → Pos1 Ende); das Textfeld und
„Zurücksetzen“ (nur sichtbar, solange etwas gesetzt ist) sind eigene Stopps. Escape im Textfeld
leert den Text, ein zweites Escape den ganzen Filter. Cmd/Ctrl+Shift+K (Befehl „Schnellfilter“,
wie Thunderbird; in der Befehlspalette und änderbar wie jeder Befehl) setzt den Fokus ins Feld. Der
Text wirkt 150 ms nach der letzten Taste, wie die Suche; die Liste zeigt bis zur neuen Antwort die
bisherigen Zeilen.

**6. Je Ansicht und nur für diese Sitzung.** Der Filter gehört zur Ansicht, in der er gesetzt
wurde, und bleibt beim Wechsel zurück erhalten, nicht über einen Neustart (ein Filter ist keine
Einstellung). Kein Speichern als Ansicht: dafür gibt es die gespeicherten Suchen (Stufe 7).

## Verworfen

- **Eine zweite Filterlogik in der Oberfläche oder im Store** (eigene Bedingungen für Text,
  Absender, Betreff): genau das, was die Vorgabe ausschließt; die Suche hat Faltung, Präfixe,
  Phrasen und Operatoren schon.
- **Die Leiste als Suche anzeigen** (Treffer mit Hervorhebung, ohne Abschnitte, nur neueste zuerst):
  Der Filter soll die Ansicht in ihrer Reihenfolge einschränken, sortieren und als Gespräche zeigen;
  die Suche kennt nur „neueste zuerst“ über Mails.
- **`tag:` und `is:contact` als Operatoren der Suchsprache:** sinnvoll, aber eine Änderung von
  Parser, Server-Suche (IMAP `KEYWORD`), Hilfe und Chips; nicht in dieser Stufe.
- **Ein- und ausblendbare Leiste:** eine Einstellung mehr für eine Zeile von 40 px; der Kopf hätte
  einen weiteren Knopf gebraucht. Die Leiste steht immer da.
- **Gespräche zählen statt Mails:** `count(DISTINCT thread)` kostet bei großen Treffermengen eine
  Sortierung; die Mails sind das, was der Filter trifft.

## Folgen

- Ein seltener Filter (ein Schlagwort, das kaum vorkommt) läuft über den Index der Reihenfolge, bis
  er eine Seite voll hat oder der Ordner zu Ende ist. Gemessen bei 100.000 Mails: höchstens 10 ms
  (Median) für eine Seite ohne Treffer, die Trefferzahl bis 20 ms; Text in Datumsreihenfolge geht
  über die Treffer des Volltextindex. Bei 500.000 Mails ungeprüft (siehe
  `docs/testing/performance.md`, Stufe 19).
- Der Tab-Weg zur Liste wird zwei Stopps länger (Werkzeugleiste, Textfeld);
  `sidebar-keyboard.test.tsx` ist angepasst.
- Vitest-Tests, die den Schalter „Nur ungelesene“ im Kopf nutzten, nutzen jetzt den Schalter
  „Ungelesen“ der Leiste.

## Rückweg

Die Leiste ist eine Komponente über der Liste; ohne sie bleibt „Nur ungelesene“ über den Zustand
`useQuickFilterStore` erreichbar und könnte als Schalter in den Kopf zurück.
