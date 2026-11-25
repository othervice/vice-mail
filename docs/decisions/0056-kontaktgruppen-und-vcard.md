# 0056 — Kontaktgruppen und vCard

- **Datum:** 24.11.2026
- **Anlass:** Stufe 19, Teil C (Auswahl des Menschen vom 24.11.2026): Kontaktgruppen
  (Verteilerlisten) auf der Kontaktseite und im Composer, vCard-Import und -Export (3.0 und 4.0).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 19 C).
- **Code:** `crates/mail-content/src/vcard/` (`read_vcards`, `write_vcards`),
  `crates/mail-model/src/contact_lists.rs`, `crates/mail-store/migrations/0022_contact_lists.sql`,
  `crates/mail-store/src/contact_lists.rs`, `crates/mail-store/src/settings_sync.rs` (Art
  `contactList`), `crates/mail-engine/src/contact_lists.rs`, `crates/mail-engine/src/vcard.rs`,
  `crates/mail-engine/src/settings_sync/record.rs`, `app/src-tauri/src/commands/contact_lists.rs`,
  `app/src/features/contacts/groups/`, `app/src/features/composer/RecipientField.tsx`,
  `app/src/features/composer/Composer.tsx` (Hinweis auf Bcc).

## Entscheidung

### Kontaktgruppen

1. **Eigene Tabellen** `contact_list` (UUID, Name, gefaltete Suchwörter) und `contact_list_member`
   (normalisierter Adressschlüssel, Adresse wie eingegeben, Name dazu, Reihenfolge), Migration
   `0022`. Ein Mitglied braucht keinen Kontakt im Verzeichnis: das Verzeichnis ist ein Cache aus
   Mails, eine Gruppe gehört dem Nutzer. Gezeigt wird der Name des Mitglieds, sonst der des
   Kontakts mit dieser Adresse. Jede Adresse einmal je Gruppe.
2. **Grenzen:** höchstens `MAX_CONTACT_LISTS` = 500 Gruppen, `MAX_CONTACT_LIST_MEMBERS` = 1.000
   Mitglieder je Gruppe, Name 1 bis `MAX_CONTACT_LIST_NAME_CHARS` = 100 Zeichen auf einer Zeile ohne
   Steuer- und Richtungszeichen. Adressen prüft der Kern wie beim Senden (`invalid_address`).
3. **Einstellungs-Sync: ja.** Namen, Notizen, Zusammenführungen und Bilder von Kontakten reisen seit
   Stufe 10/17 mit; eine Gruppe ist ebenso etwas, das der Nutzer anlegt. Neue Art `contactList`
   (UUID, „Bearbeiten schlägt Löschen“ wie Kontogruppen, höchstens 512 KiB). Der Datensatz trägt
   Name, Suchwörter und je Mitglied Adresse, Name und Schlüssel; das empfangende Gerät prüft, dass
   es genau das selbst gespeichert hätte (`contact_lists::check_synced`), sonst wird er abgelehnt.
4. **Kontaktseite:** oben in der Listenspalte „Personen | Gruppen“ (Tabs). Gruppen: anlegen,
   umbenennen, Mitglieder hinzufügen (Feld mit Vorschlägen wie im Composer) und entfernen, löschen
   mit Rückfrage (die Kontakte bleiben).
5. **Composer:** die Autovervollständigung von An, Cc und Bcc schlägt Gruppen vor
   (`suggestContactLists`: jedes Wort der Eingabe beginnt ein Wort des Namens, ohne Akzente und
   Groß/klein), über den Kontakten, mit Zahl der Mitglieder. Gewählt wird die Gruppe zu ihren
   Mitgliedern (`getContactList`, beim Wählen geladen; vorhandene Adressen bleiben einmal). Stehen
   danach in An und Cc zusammen mindestens `MANY_RECIPIENTS` = 10 Empfänger, zeigt der Composer
   einen Hinweis: „… sehen die Adressen der anderen“ mit „Gruppe nach Bcc verschieben“ (die
   Mitglieder dieser Gruppe wandern nach Bcc) oder „So lassen“.
6. Jede Änderung sendet `contactsChanged` (alle Fenster laden Kontakte und Gruppen neu).

### vCard

7. **Eigener Leser und Schreiber in `mail-content`** statt einer neuen Abhängigkeit: im
   `Cargo.lock` steht keine vCard-Bibliothek; der Kalenderleser (Stufe 9) ist ebenfalls eigen, und
   vCard ist ein kleines Zeilenformat. Lesen: 2.1, 3.0 und 4.0 (Faltung, Quoted-Printable mit
   weichen Umbrüchen, `CHARSET` über die Zeichensätze von mail-parser, ohne Angabe nicht-UTF-8 als
   Windows-1252, Gruppenpräfixe wie `item1.EMAIL`, `PREF` und `TYPE=pref`). Gruppen: `KIND:group`
   mit `MEMBER` (`mailto:`, `urn:uuid:`) und Apples `X-ADDRESSBOOKSERVER-KIND/-MEMBER`. Schreiben:
   3.0 oder 4.0, CRLF, Faltung bei 75 Oktetten an Zeichengrenzen. Grenzen: Datei 16 MiB,
   10.000 Karten, 20 Adressen je Karte, 1.000 Mitglieder, Bild 4 MiB, Zeile 6 MiB; linear, nie Panik.
8. **Import** (Öffnen-Dialog des Systems, `.vcf`): je Karte werden unbekannte Adressen neue
   Kontakte (der Name der Karte als Name des Nutzers, damit er bleibt und synchronisiert wird);
   bekannte Kontakte behalten ihre Zähler und bekommen Name und Notiz nur, wo sie keine haben.
   **Dubletten werden zusammengeführt:** mehrere Adressen einer Karte, oder eine Karte mit
   Adressen verschiedener Kontakte, werden ein Kontakt (Kopf: der Kontakt der ersten bekannten
   Adresse, sonst die erste). Gruppenkarten werden Gruppen; eine Gruppe gleichen Namens wird
   ergänzt, nicht doppelt angelegt. Bericht: Karten, neu, ergänzt, zusammengeführt, Gruppen,
   Bilder, Probleme (höchstens 32).
9. **Bilder aus vCards laufen durch den abgeschotteten Hilfsprozess** (0046, 0050): nur
   eingebettete Bytes (Base64, `data:`), nie Links (werden nie geladen und als Problem gemeldet),
   nur PNG, JPEG, GIF, WebP nach ihren Bytes; der Hilfsprozess dekodiert und kodiert neu
   (`PictureStep::Source`), der Kern schneidet das mittige Quadrat aus (`PictureStep::Crop`,
   256 × 256 JPEG ≤ 40 KiB). Gesetzt nur, wo der Kontakt noch kein Bild hat.
10. **Export** (Speicherdialog): „Alle Kontakte“ (Personen, wie die Ansicht standardmäßig) mit allen
    Gruppen, ein Kontakt, oder eine Gruppe mit einer Karte je Mitglied. Version 3.0 (liest jedes
    Adressbuch: macOS Kontakte, Outlook, Google) oder 4.0 (was Thunderbird schreibt). Eine Karte je
    Kontakt mit Name, allen Adressen (die bevorzugte zuerst), Notiz und Bild; UIDs aus dem Schlüssel
    des Kontakts abgeleitet (gleicher Export, gleiche Karten); Gruppenmitglieder über die UID ihrer
    Karte in derselben Datei, sonst über die Adresse.

## Erwogen, nicht gewählt

- **Die Bibliothek `calcard`** (Stalwart, Apache-2.0 OR MIT, kann vCard und iCalendar): wäre eine
  neue Abhängigkeit mit eigenem Parser für beides, während der Kalenderleser schon eigen ist; der
  Umfang hier (Name, Adressen, Notiz, Bild, Gruppen) ist klein.
- **Gruppen nur lokal:** widerspräche dem, was mit Kontakten schon synchronisiert wird.
- **Gruppen als Kontakt-Art im Verzeichnis:** das Verzeichnis ist ein Cache, der neu aufgebaut
  werden kann; eine Gruppe darf dabei nicht verloren gehen.

## Folgen

- `docs/ARCHITECTURE.md`: Datenmodell (`contact_list`, `contact_list_member`), Sync-Regeln.
- Migration `0022` folgt nach dem Zusammenführen auf `0021` (Stufe 20).
