# 0050 — Kontakte: jede Adresse, eigene Bilder, „Als Spam markieren“

- **Datum:** 24.11.2026
- **Anlass:** Wunsch des Menschen vom 24.11.2026 (Stufe 17, Block B): jede Adresse aus allen
  Mails wird Kontakt, eigene Bilder statt nur Initialen, eine schön gestaltete Kontaktseite mit
  guter Suche, Spam-Adressen sperren.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Block B).
- **Code:** `crates/mail-content/src/contacts/book.rs` (`observations`, `contact_kind`,
  `counted_before_stage17`), `crates/mail-content/src/contact_picture.rs`,
  `crates/mail-mcp/src/preview.rs` (`--contact-picture`), `crates/mail-store/migrations/0020_contacts_view.sql`,
  `crates/mail-store/src/contacts_view.rs`, `crates/mail-engine/src/compose/contacts.rs` (Auftrag,
  Nachlesen), `crates/mail-engine/src/contacts_view.rs` (Notizen, Bilder, Spam),
  `app/src-tauri/src/commands/contacts.rs`, `app/src/features/contacts/` (Ansicht, Zuschnitt,
  Spam), `app/src/features/accounts/Avatar.tsx` und `features/contacts/pictures.ts`.

## Entscheidung

### Jede Adresse wird Kontakt

- Aus jeder Mail: bei eigenen (gesendeten) Mails jeder Empfänger in An, Cc und Bcc
  (`Direction::Sent`), bei empfangenen der erste Absender (`Received`) und jede weitere Adresse aus
  Von, An, Cc und Antwort an (`Seen`). Eigene Adressen bleiben draußen wie bisher; Adressen, die
  niemand liest (`noreply@`, `MAILER-DAEMON@`), sind jetzt Kontakte, aber markiert.
- **Art** (`contact.kind`, `mail_content::contact_kind`): `system`, wenn niemand die Adresse liest
  oder ein Programm von ihr schreibt (Muster der Kategorien) und der Nutzer ihr nie geschrieben hat;
  `list`, wenn sie ein Newsletter-Muster hat oder nur in Mails gesehen wurde, die nicht von einer
  Person kommen (Kategorie ≠ Person); sonst `person`. Die Ansicht zeigt standardmäßig nur Personen,
  ein Schalter zeigt alle; die Autovervollständigung bietet Listen- und Systemadressen nur an,
  wenn der Nutzer ihnen schon geschrieben hat; die Doppelten-Suche sieht nur Personen.
- **Rang wie bisher:** Nur gesendete Mails und Mails von Personen zählen für den Rang; Newsletter,
  Benachrichtigungen, Kopien und Antwort-an zählen als `received` bzw. `seen` mit Datum, aber ohne
  Rang. Die Reihenfolge der bisherigen Kontakte bleibt.
- **Antwort an:** neue Mails bringen sie aus dem Kopfblock mit, den der Adapter für die
  Kategorien ohnehin holt (`Classified::reply_to`, Spalte `message.reply_to_addrs`).
- **Rückwirkend, ohne den Start zu blockieren:** Mails, die die Regeln von Stufe 10 schon gelesen
  haben (`contacts_seen = 1`), liest der bestehende Hintergrundauftrag nach neuen Mails noch einmal
  (Teilindex `message_contacts_backfill`, älteste zuerst, je 500), aber nur, was Stufe 10 ausließ
  (`counted_before_stage17`); danach `contacts_seen = 2`. Ein Test belegt: Stufe 10 plus Nachlesen
  gibt dasselbe Verzeichnis wie Neulesen. Antwort-an solcher Mails kommt aus dem gespeicherten
  Mailtext, wenn er ganz und höchstens 1 MiB groß vorliegt; sonst fehlt sie. Beim ersten Nachlesen
  je Lauf werden die Arten aller Zeilen neu berechnet (Zeilen von Stufe 10, die das Nachlesen nie
  berührt).
- **Kosten der Migration:** zwei Indizes über `message` (`message_contacts_backfill`, Teilindex;
  `message_sender` über die Absenderschlüssel) und fünf Spalten an `contact`. Bei sehr großen
  Datenbanken kostet das einmalig Sekunden beim Start der neuen Version (nicht gemessen bei
  500.000 Mails, siehe „Offen“).

### Eigene Bilder

- **Wahl:** Systemdialog (Rust, `rfd`) oder Ziehen auf das Fenster (nur Pfade eines Drops, den die
  Shell selbst gesehen hat, `drops.rs`). Die Oberfläche bekommt nie einen Pfad zu lesen; der Kern
  liest höchstens 20 MiB (`picture_too_large` sonst, vor dem Lesen geprüft).
- **Zwei Schritte im Helfer** (wie Vorschau und Bilder in Mails, 0043/0046: `mail-mcp
  --contact-picture`, 20 s, 512 MiB): `source` dekodiert die Datei (PNG, JPEG, GIF, WebP nach Bytes,
  Grenzen der Vorschau, aufrecht gedreht, Transparenz auf Weiß) und kodiert das ganze Bild neu,
  höchstens 1024 px je Seite, für den Zuschnitt; der Kern hält es 15 Minuten unter einem Token
  (höchstens vier). `crop x y size` schneidet das Quadrat aus diesem Bild, skaliert auf 256 × 256
  und kodiert JPEG, Qualität 85 abwärts bis 35, bis es höchstens 40 KiB sind. Die Antwort des
  Helfers wird geprüft (genau `data:image/jpeg;base64,…`, JPEG-Anfang, Größe, Maße). Typisierte
  Fehler `picture_too_large`, `picture_unsupported`, `picture_damaged`; ein abgelaufenes Token ist
  `not_found`. Ohne Helfer (Tests, Entwicklungs-Build ohne die Datei) dieselben Schritte im
  Prozess auf dem Blocking-Pool.
- **Speicher:** in der Datenbank (`contact_picture`, ein BLOB je Adresse), nicht im Datenordner:
  transaktional mit dem Kontakt, in den Sicherungen vor Migrationen enthalten, über Trigger im
  Sync erfasst, kein Aufräumen von Dateien. 40 KiB je Bild halten die Datenbank klein.
- **Zugehörigkeit:** das Bild gehört der bevorzugten Adresse beim Setzen; die anderen Adressen des
  Kontakts verlieren ihres. Angezeigt wird das Bild der ersten Adresse des Kontakts, die eines hat.
- **Anzeige:** der Avatar fragt pro Fenster einmal die Liste, welche Adressen ein Bild haben
  (`listContactPictures`, Adressen in Kleinschrift, wie geschrieben und mit ASCII-Domain), und je
  Bild und Revision einmal die Daten (`getContactPicture`); `contactsChanged` lädt neu. Damit
  zeigen Listenzeilen, Lesebereich, Konversationen, Composer-Chips und Kontakte das Bild, ohne dass
  `MessageRow` mehr übergeben muss als bisher. Ohne Bild bleiben die Initialen.
- **Nie aus dem Netz:** kein Gravatar, kein BIMI, keine Favicons.

### „Als Spam markieren“

- Sperrt jede Adresse des Kontakts als blockierten Absender von Stufe 16. Damit gilt dasselbe wie
  dort: Neue Mails von ihr gehen im Posteingang über die Outbox in den Spam-Ordner, wo das Konto
  schreiben darf und einen hat, sonst werden sie in den Posteingangslisten verborgen; keine Regeln,
  keine Benachrichtigung. Es gibt keine zweite Sperrliste.
- Danach fragt die Ansicht, ob vorhandene Mails mitgehen: Mails von diesen Adressen außerhalb von
  Spam, Papierkorb, Gesendet, Entwürfe und Gmails „Alle Nachrichten“ (`message_sender`), je Konto
  ein Stapel über die Outbox (`MailAction::Spam`), nur für Konten mit Schreibzugriff. Konten ohne
  Schreibzugriff behalten ihre Mails; Frage und Hinweis sagen, wie viele.
- **Rückgängig:** hebt nur die Sperren auf, die diese Markierung gesetzt hat, und macht die Stapel
  rückgängig (`undo_action`); was sich nicht mehr rückgängig machen lässt, wird gezählt.
  „Kein Spam“ hebt die Sperren eines gesperrten Kontakts auf.

### Einstellungs-Sync

- Der Datensatz `contact` trägt zusätzlich `note` (höchstens 1.000 Zeichen, Grenze des
  Datensatzes 8 KiB statt 4 KiB). Neue Datenart `contactPicture` (je normalisierter Adresse,
  `{"jpeg": base64}`, höchstens 56 KiB, neuere Version gewinnt, Löschen wird angewandt). Ein
  Bild aus dem Sync wird gespeichert, aber erst angezeigt, nachdem der Helfer dieses Geräts es neu
  kodiert hat (`display`, nicht synchronisiert); lehnt er ab, bleibt es unsichtbar.
- Spam-Markierungen reisen als `blockedSender` (Stufe 16). Gezählte Daten (Zähler, Rang, Namen aus
  Mails, Art) reisen nie: nur, was der Nutzer selbst gesetzt hat.
- Grenzen F2 unverändert: ein Bild ist weit unter 2 MiB je Eintrag; 5.000 Einträge und 64 MiB je
  Durchgang reichen für über tausend Bilder in einem Durchgang.
- Geräte mit älterer Version lehnen `contactPicture` als unbekannte Art und einen
  `contact`-Datensatz mit `note` ab (Hinweis „abgelehnt“); vor der ersten Veröffentlichung
  hingenommen.

### Ansicht und Einstellungen

- Ansicht `contacts` (Spalte, Leiste, Befehlspalette `goContacts`, Standard nach „Gespeicherte
  Ansichten“), von `AppShell` statt `MailView` gezeigt, über alle Konten. Einstellungen → Kontakte
  verweist darauf und behält die Doppelten-Vorschläge.

## Messung

- Suche im Speicher: `contacts_view_tests::search_over_10000_contacts_by_name_address_and_domain_is_fast`
  (Debug-Build, Mac mini M4, andere Sitzungen bauten gleichzeitig): 0,8–8,0 ms je Suche bei
  10.000 Kontakten, auch ohne Treffer (ganzer Index gelesen); Grenze im Test 100 ms.

## Erwogen, nicht gewählt

- **Bilder im Datenordner als Dateien:** braucht Aufräumen, Sicherung und eigenen Sync-Weg.
- **Bild im Kontaktdatensatz des Syncs:** jede Namens- oder Notizänderung würde das Bild neu
  hochladen.
- **Alles neu zählen nach dem Update** (Zähler leeren, `contacts_seen` zurücksetzen): einfacher,
  aber die Autovervollständigung wäre bis zum Ende des Nachlesens schlechter, und Zeilen ohne
  gespeicherte Mail verlören ihre Zahlen.
- **Bilder im App-Prozess dekodieren:** widerspricht 0046.

## Offen und ungeprüft

- Windows: Dialog, Drop, Helfer-Modus und Anzeige sind geschrieben, dort aber nicht gelaufen.
- Dauer der Migration 0020 bei 500.000 Mails nicht gemessen.
- Antwort-an alter Mails ohne gespeicherten Mailtext wird nicht nachgetragen.

## Nachtrag 26.11.2026

Der Teil „Eigene Bilder“ und die Datenart `contactPicture` des Einstellungs-Syncs sind seit Stufe 22
entfernt (Entscheidung 0072): Migration 0028 löscht die gespeicherten Bilder, Einträge älterer
Geräte im Sync-Ordner werden still ignoriert, vCard-Fotos übersprungen. Kontakte, Notizen und
„Als Spam markieren“ bleiben wie hier beschrieben.
