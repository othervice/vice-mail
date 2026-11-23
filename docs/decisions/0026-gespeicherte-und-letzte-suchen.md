# 0026 — Gespeicherte und letzte Suchen

Stand 23.11.2026. Stufe 7, Nachtrag. Die Aufgabe „Suche in der Oberfläche“ verlangt „letzte
Suchen“ und „Suchen speichern: sie erscheinen als Ansicht in der Leiste“. Beides fehlte im Kern
(im Plan als Tabelle `saved_view`, im Einstellungs-Sync als Datensatz-Art `savedView` „ohne lokale
Daten“). Diese Entscheidung legt Speicher, Commands, Fehler und Sync fest.

## Entscheidung

1. **Eine gespeicherte Suche ist Name plus Anfrage, sonst nichts.** Sie sucht immer in allen
   Konten (Bereich `all`); eingrenzen kann die Anfrage selbst mit `account:` und `in:` (0013). Kein
   gespeicherter Bereich, weil Bereiche lokale IDs tragen (Postfach, Konto, Gruppe), die auf einem
   anderen Gerät nichts bedeuten; Datensätze des Syncs verweisen nie über lokale IDs.
   Die Anfrage wird so gespeichert, wie sie getippt wurde (nur vorne und hinten gekürzt), nicht
   kompiliert: Datumsangaben wie `newer_than:7d` gelten so immer ab dem Öffnen, und eine spätere
   Fassung des Parsers liest sie neu.
2. **Tabelle `saved_view`** (Klasse sync): UUID, Name, Anfrage, Position, angelegt, geändert.
   Neue kommen ans Ende (`max(position) + 1`), Umbenennen lässt Anfrage und Platz. Trigger markieren
   jede Änderung für den Einstellungs-Sync (`sync_dirty`, wie bei `template`).
3. **Grenzen und Prüfung im Kern.** Name 1 bis 100 Zeichen, Leerraum zusammengefasst, keine
   Steuerzeichen; Anfrage 1 bis 2.048 Bytes (so viel liest der Parser, `MAX_QUERY_BYTES`), keine
   Steuerzeichen außer Leerraum; höchstens 100 gespeicherte Suchen. Namen sind ohne Rücksicht auf
   Groß/Klein eindeutig (Unicode-Kleinschreibung in Rust, weil SQLites `lower()` nur ASCII kennt),
   geprüft in derselben Schreib-Transaktion wie das Einfügen. Die Tabelle selbst hat keinen
   eindeutigen Index auf dem Namen: Zwei Geräte können je „Ungelesen“ anlegen, und der Sync muss
   beide übernehmen können, statt einen still zu verwerfen.
4. **Typisierte Fehler.** Zwei neue Fehlercodes, weil die Oberfläche darauf verschieden reagiert:
   `saved_search_exists` (Name vergeben; die Oberfläche lässt das Feld offen) und
   `saved_search_limit` (100 erreicht; erst eine löschen). Leere oder zu lange Namen und Anfragen
   sind `invalid_argument`, eine unbekannte ID `not_found`. Abgelehnt wird, bevor sich etwas ändert.
5. **Commands und Ereignis.** `listSavedSearches`, `saveSearch(name, query)`,
   `renameSavedSearch(savedSearchId, name)`, `deleteSavedSearch(savedSearchId)`. Ereignis
   `savedSearchesChanged` nach jeder Änderung hier und wenn ein Sync-Durchlauf gespeicherte Suchen
   eines anderen Geräts übernommen hat. Konstanten `MAX_SAVED_SEARCHES`,
   `MAX_SAVED_SEARCH_NAME_CHARS`, `MAX_RECENT_SEARCHES` gehen mit den Bindings an die Oberfläche.
6. **Sync: Art `savedView`** (schon registriert, „newer wins, edit beats delete“, 16 KiB). Inhalt
   jetzt streng `{name, query, position}` mit denselben Prüfungen wie lokal (unbekannte Felder
   abgelehnt). Bisher galt „irgendein Objekt“; da es keine lokalen Daten gab, hat kein Gerät solche
   Datensätze geschrieben, ein neues `format` ist daher nicht nötig. Übernommene Datensätze gehen ohne
   Prüfung der Namensgleichheit in die Tabelle (siehe 3).
7. **Letzte Suchen bleiben auf dem Gerät.** Eigene Tabelle `recent_search` (Klasse Gerät, ohne
   Trigger, nie im Sync): Anfrage als Schlüssel, zuletzt benutzt. Höchstens 10; dieselbe Anfrage
   noch einmal rückt nur nach oben (keine Dubletten), die älteste fällt beim Hinzufügen weg. Der
   Zeitstempel ist nie kleiner als der neueste vorhandene, damit eine zurückgestellte Uhr die
   Reihenfolge nicht umdreht. Commands `listRecentSearches`, `addRecentSearch(query)` (gibt die neue
   Liste zurück) und `clearRecentSearches`; kein Ereignis, weil nur die Oberfläche dieses Fensters sie
   ändert. Eine eigene Tabelle statt eines Schlüssels in `setting`, weil die Trigger von `setting`
   jede Änderung für den Sync markieren und das Hinzufügen so eine Zeile ist.
   Wann eine Suche als „benutzt“ gilt (Eingabe mit Enter, Öffnen eines Treffers), entscheidet die
   Oberfläche; beim Tippen wird nichts gemerkt.
8. **Migration** `0012_saved_searches.sql` (nur additiv: zwei Tabellen, ein Index, drei Trigger).

## Verworfen

- **Bereich mitspeichern** (Konto-UUID oder Postfachpfad): ginge über UUIDs, aber Postfachpfade
  unterscheiden sich zwischen Servern, und die Suchsprache kann es schon.
- **Gespeicherte Suchen als kompilierte Abfrage**: hängt am Parser und an der Zeitzone des Geräts.
- **Letzte Suchen synchronisieren**: verrät auf allen Geräten, wonach gesucht wurde, und ändert
  sich bei jeder Suche (Last für den Sync ohne Nutzen).

## Rückweg

- Bereich je gespeicherter Suche: Spalte `scope` (JSON mit Konto-UUIDs) und Feld `scope?` im
  Inhalt mit neuem `format` des Datensatzes.
- Reihenfolge ändern: Command `moveSavedSearch(id, position)`; die Position ist schon Teil des
  Datensatzes.

## Geprüft

- `cargo test -p mail-model --lib saved_search` (Namen und Anfragen bereinigt und begrenzt, Fehlercodes,
  UUID auf der Brücke), `error::tests` (Fehlercodes auf der Leitung).
- `cargo test -p mail-store --lib saved_searches`: Reihenfolge, Namensgleichheit ohne Groß/Klein auch
  über ASCII hinaus, Grenze 100; letzte Suchen rücken nach oben und behalten die neuesten;
  gespeicherte Suchen werden erfasst (Inhalt, neue Version nach Umbenennen, Grabstein nach Löschen)
  und von einem anderen Gerät übernommen und entfernt; letzte Suchen erzeugen keinen Datensatz.
- `cargo test -p mail-engine --lib search::saved`: über `AppCore` speichern, umbenennen, löschen mit
  allen Fehlercodes und dem Ereignis; letzte Suchen begrenzt, ohne Dubletten, nach einem Neustart
  noch da. `settings_sync::tests::capture`: `savedView` wird erfasst, eine letzte Suche steht in
  keinem Datensatz; `settings_sync::record::tests::saved_views_are_shaped_like_saved_searches`.
- `cargo test -p mail-app --features e2e saved_and_recent_searches_cross_the_bridge`: über den echten
  Command-Handler mit den Fehlercodes auf der Leitung.
