# 0017 — Suche, Threads und Konten im Store und in der Engine

- **Datum:** 23.11.2026
- **Stufe:** 7 (Backend: Store, Engine, Commands; ohne Oberfläche)
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-store/migrations/0005_search_threads.sql`, `crates/mail-store/src/{fts,search,threads,lists,groups,search_api}.rs`,
  `crates/mail-engine/src/search/`, `crates/mail-engine/src/reading/index_text.rs`,
  `crates/mail-model/src/views.rs`, `crates/mail-model/src/search/{highlight,server}.rs`,
  `crates/mail-imap/src/search.rs`, `app/src-tauri/src/commands/{search,threads,groups}.rs`
- **Messung:** `docs/testing/performance.md`

Setzt 0003 (Schema, Suche) und 0013 (Suchsprache, Threads) um. Hier steht, wo die Umsetzung vom
Plan abweicht oder ihn genauer festlegt.

## Volltextindex

- **Wann geschrieben wird.** Kopfzeilen (`subject`, `from`, `to`, `cc`) in der Transaktion, die die
  Nachricht speichert (`store_headers`); `body` und `attachments` in der Transaktion, die den Body
  festhält. Damit gibt es nie einen Body ohne Indextext und keinen Aufrufer, der das Indexieren
  vergessen kann.
- **Wer den Text macht.** Der Store parst keine Mails. Die Engine gibt ihm beim Start eine
  `BodyTextSource` (`ContentText`, gleiche Zerlegung wie der Lesebereich über `mail-content`): Text des
  Textteils, sonst Text des HTML-Teils, dazu die Anhangnamen. Der Store ruft sie auf dem Blocking-Pool
  auf, bevor er die Body-Datei schreibt. Ohne Quelle werden nur Kopfzeilen indexiert.
- **Obergrenze 256 KiB Text je Mail** (`MAX_BODY_TEXT_BYTES`): lange Newsletter und Protokolle
  blähen den Index sonst auf, ohne der Suche viel zu nützen.
- **Teil-Bodies großer Mails:** Ein Aufruf mit Textteilen indexiert; ein späterer Aufruf nur mit
  Anhängen (Abruf auf Wunsch) lässt den Text stehen. Ein ganzer Body ersetzt den Text eines Teil-Bodys.
- **Body verlässt den Cache:** Der Text bleibt im Index (die Mail lag einmal lokal, Öffnen holt sie neu).
  „Im Einklang“ heißt daher: genau eine Indexzeile je Nachricht, mit den gespeicherten Kopfzeilen und
  dem zuletzt indexierten Body-Text.
- **Bestehende Daten und Neuaufbau** laufen über denselben Auftrag (`search_index_job`, eine Zeile):
  Bestandsmails nach der Migration direkt in `message_fts`, ein Neuaufbau in `message_fts_new`. In
  Stapeln zu 500 nach aufsteigender ID: ein Leser liefert die IDs mit Body-Stand, die Texte entstehen
  auf dem Blocking-Pool (jede Body-Datei einmal gelesen), eine Schreib-Transaktion schreibt sie und
  setzt den Auftrag weiter. Während eines Neuaufbaus geht jede Indexänderung in beide Tabellen; eine
  Stapelzeile wird übersprungen, wenn sich der Body seit dem Lesen geändert hat oder die Nachricht weg
  ist. Am Ende werden die Tabellen in einer Transaktion getauscht, die Trigger neu angelegt (ein
  Umbenennen würde sie auf die umbenannten Tabellen umschreiben), die alte Tabelle in der nächsten
  Transaktion gelöscht. Die Suche nutzt bis dahin die alte Tabelle.
- **Hervorhebungen rechnet Rust**, nicht FTS5: `highlight()`/`snippet()` liefern bei einem
  contentless Index nichts (mit sqlite3 3.54 geprüft). `mail_model::search::highlight` sucht die
  Wörter der positiven Textterme in Betreff, Absender und Vorschau, ohne Groß/Klein und ohne die
  diakritischen Zeichen lateinischer Buchstaben (wie der Tokenizer); Treffer tief im Body haben keinen
  Bereich. Bereiche gehen als UTF-16 über die Brücke.
- Für den Indexaufbau gibt es **kein Ereignis**; der Zustand steht in jeder Suchantwort und in
  `getSearchIndexStatus`.

## Suche

- Die Engine löst `account:` auf (Adresse, Name, UUID oder Domain, ohne Groß/Klein) und `in:` (Rolle
  vor Name; Name als ganzer Pfad oder letzter Teil). Unbekanntes führt zu einer Problemstelle;
  ein positiver unbekannter Term findet nichts. Postfachnamen liest sie dafür ohne Zähler
  (`mailbox_names`), denn die Gesamtzahl je Postfach kostete bei 500.000 Mails 5 ms je Suche.
- **Treiber der Abfrage:** mit Text FTS5 (`rowid DESC`, Datum und Cursor als rowid-Bereich in FTS5);
  ohne Text, aber mit Postfach ein `UNION ALL`-Zweig je Postfach über den Ortsschlüssel (bei
  `is:unread` über `loc_unseen`); sonst `message` per Primärschlüssel abwärts. Filter auf `message`
  stehen mit unärem `+`, damit der Planer sie nicht als Index nimmt (sonst las er per
  `message_msgid` alle Mails eines Kontos und sortierte).
- `is:unread`/`is:read`/`is:flagged` gelten für die Orte im ersten `in:` bzw. im Postfach-Bereich,
  sonst für irgendeinen Ort. `is:read` heißt „nirgends ungelesen“. `is:snoozed` findet vor Stufe 9 nichts.
- Ausgeblendete Konten fehlen bei „alle“ und in Gruppen, nicht bei einem ausdrücklich gewählten
  Konto oder Postfach.
- **Zeitzone:** `jiff` 1.0.2 (MIT OR Unlicense, lag schon im Lockfile) liest die Zone des Systems
  samt Sommerzeit, auf Windows mit mitgelieferter tzdb. Ohne Abhängigkeit hätte es nur einen festen
  Versatz (0013: „Notbehelf“) oder einen eigenen TZif-Leser gegeben, der auf Windows nicht reicht.
  Die Datumsreihenfolge (`01/02/2026`) schickt die Oberfläche mit (`monthFirst`), weil nur sie die
  Region kennt.

## Auf dem Server weitersuchen

- Neue Methode `MailBackend::search_text` (Standard: `Unsupported`), umgesetzt in `mail-imap` als
  `UID SEARCH` über die Nur-Lese-Leseverbindung. Zeichenketten als Quoted Strings (die Befehlsschicht
  sendet eine Zeile; die Nur-Lese-Prüfung lehnt Zeilenumbrüche ab), mit `CHARSET UTF-8`, sobald eine
  nicht ASCII ist. Gegen Stalwart geprüft, auch mit Umlauten.
- Die Kriterien (`ParsedQuery::server_query`) sind eine Obermenge; Datum, `has:attachment`, Konten und
  Postfächer prüft danach der Store an den lokalen Kopfdaten. Zurück kommen nur Mails ohne
  vollständigen lokalen Body (die anderen deckt der lokale Index ab), höchstens die neuesten 5.000
  Kandidaten.
- Gesucht wird in den Postfächern von Bereich bzw. `in:`, sonst in allen außer Papierkorb und Spam,
  bei „Alle Nachrichten“ nur dort. Ohne Textbedingung wird nichts gesendet.

## Threads

- **Zuordnung im Schreib-Thread**, in der Transaktion des Fensters: `ThreadLookup` über `thread_ref`
  (alle IDs, die ein Thread trägt oder nennt, wie `ThreadIndex`), `message_msgid` und
  `message_subject`. Abgeglichen mit der Referenz in `mail-model` (gleiche Gruppierung).
- **`subject_key` nur für mögliche Rückfall-Partner** (kein Massenversand, spezifischer Betreff):
  der Index bleibt klein; ein Neuaufbau der Threads wendet geänderte Regeln auf alles an.
- **Ungelesen wird beim Lesen gezählt, nicht gepflegt.** `thread` hält Anzahl und neueste Nachricht
  (gepflegt beim Einfügen, Zusammenführen und per Trigger beim Löschen). Die Zahl der Ungelesenen je
  Thread zählt die Thread-Liste aus `message_thread` und `loc_message`. Grund: Flags ändern sich an
  vielen Stellen (Abgleich, Outbox, Rücknahme, Kontolöschung mit Kaskaden, deren Trigger die
  Nachricht schon nicht mehr sehen); eine gepflegte Zahl müsste jede davon kennen. Kosten wachsen mit
  der Thread-Größe: eine Zeile mit 7.620 Mails kostet rund 2 ms mehr.
- **Thread-Liste eines Postfachs:** je Thread die neueste Mail *in diesem Postfach* (eine Mail ist die
  Zeile, wenn keine neuere desselben Threads dort liegt), Keyset über deren ID. Eine Antwort nur in
  „Gesendet“ hebt den Thread im Posteingang daher nicht nach oben; `lastActivity` zeigt sie. Eine
  Sortierung nach der neuesten Mail überall bräuchte eine gepflegte Tabelle je Postfach und Thread.
- **Bestandsmails** (vor Stufe 7 gespeichert, `thread_id` NULL) ordnet ein Hintergrund-Task in
  aufsteigender ID zu (Datumsreihenfolge, das kanonische Ergebnis aus 0013). Bis dahin ist jede eine
  eigene Zeile. Thread-IDs ändern sich beim Zusammenführen; die Oberfläche öffnet Threads über eine
  Message-ID.
- Der Hintergrund-Task läuft nur, wenn Arbeit da ist, und hält den Store nur schwach: sonst bliebe die
  Datenbank nach dem Beenden des Kerns offen (der WAL-Checkpoint beim Schließen fiele aus).

## Konten

- **Kontogruppen** (`account_group`, `account_group_member`): Klasse sync, Verweis über die
  Konto-UUID. Der Einstellungs-Sync kennt dafür noch keine Datensatz-Art (`RecordKind` ist
  geschlossen); Stufe 11 muss eine ergänzen.
- **Ausblenden** (`account_hidden`): Klasse Gerät; der Abgleich des Kontos läuft weiter.
- **Identitäten:** `identity` bekommt eine UUID (der Einstellungs-Sync nennt Identitäten per UUID;
  bestehende per Migration, neue per Trigger) und eine eindeutige Adresse je Konto ohne Groß/Klein. Die
  Standard-Identität lässt sich nicht löschen. Antworten wählen die Identität mit
  `mail_content::choose_identity` aus From/To/Cc der gespeicherten Kopfdaten.

## Folgen und Kosten

- Der Abgleich schreibt je Fenster von 500 Kopfdaten 4,7 ms länger (5,7 → 10,4 ms, gleiche Daten,
  gemessen gegen den Stand vor Stufe 7). Für den laufenden Abgleich unter 1 ms, für einen Erstabgleich
  von 100.000 Mails knapp 1 s Datenbankzeit mehr. Alle Lese-Ziele bleiben um Größenordnungen unterschritten.
- Plan-Tests erlauben beim Mischen mehrerer Postfächer einen kleinen temporären B-Tree je Zweig
  (höchstens Limit + 1 Zeilen, wie 0003 für den vereinten Posteingang) und die konstante Zeile
  skalarer Unterabfragen; sonst kein Scan.

## Rückweg

- Ungelesen gepflegt: Spalte in `thread` und Pflege an allen Flag-Schreibwegen; die Abfrage ändert
  sich nur an einer Stelle (`lists::thread_branch`).
- Sortierung nach Aktivität überall: Tabelle `mailbox_thread(mailbox_id, thread_id, last_message_id)`.
- Hervorhebung im Body: Body-Datei laden und denselben Abgleich laufen lassen (rund 1 ms je Treffer).
- Ohne `jiff`: fester Versatz aus der Oberfläche (`FixedOffset`), mit einer Stunde Fehler über die
  Sommerzeit hinweg.
