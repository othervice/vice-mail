# 0072 — Keine Bilder und Initialen: Markenspalte und Kontopunkt

- **Datum:** 26.11.2026
- **Anlass:** Rückmeldung 3 zu 0.1.2 (Stufe 22): „Ich denke, wir wollen die Bilder mit den
  Initialen entfernen; eigene Bilder hinterlegt ohnehin niemand, und es kostet nur Platz.
  Kontakte bleiben, nur ohne Bilder, und nirgends im Posteingang usw. diese Bilder. Alles mit
  Bildern raus aus dem Client.“ Gemeint sind Absender-, Kontakt- und Kontobilder; Bilder in Mails
  und Anhänge bleiben.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22, Punkt 3).
  **Ersetzt** den Bild-Teil von Stufe 17 (Entscheidung 0050, „Eigene Bilder“, und den Datensatz
  `contactPicture` des Einstellungs-Syncs), die Avatare mit Initialen und Tönungen aus Entscheidung
  0031 und das „Kästchen an der Stelle des Avatars“ aus 0065 (Punkte 3 und 4).
- **Code:** Kern: `crates/mail-content/src/vcard/` (`PHOTO` übersprungen), entfernt
  `crates/mail-content/src/contact_picture.rs`, `crates/mail-mcp/src/preview.rs` (Modus
  `--contact-picture` entfernt), `crates/mail-model/src/contacts.rs`, `popup.rs`, `error.rs`,
  `contact_lists.rs`, `crates/mail-store/migrations/0028_no_contact_pictures.sql`,
  `crates/mail-store/src/contacts_view.rs`, `settings_sync.rs`, `views.rs`,
  `crates/mail-engine/src/contacts_view.rs`, `vcard.rs`, `compose/contacts.rs`, `comfort/popup.rs`,
  `settings_sync/record.rs` (`RETIRED_KINDS`), `crypto.rs` (`OpenError::Retired`), `plan.rs`
  (`Notice::Retired`), `service.rs`, `app/src-tauri/src/commands/contacts.rs`. Oberfläche:
  entfernt `app/src/features/accounts/Avatar.tsx`, `senderTint.ts`,
  `features/contacts/PictureEditor.tsx`, `pictures.ts`; `features/messages/MessageRow.tsx`
  (`SelectionMark`, `AccountDot`, `TreeLine`), `MessageList.tsx`, `views/TableHeader.tsx`,
  `comfort/InboxListRows.tsx`, `app/IconRail.tsx`, `contacts/ContactDetail.tsx`,
  `ContactsView.tsx`, `groups/GroupsPane.tsx`, `popup/NewMailPopupView.tsx`,
  `reading/MessageHeader.tsx`, `composer/RecipientField.tsx`, `settings/AccountsSettings.tsx`,
  `settings/accounts/AccountDetail.tsx`, `styles/tokens.css` (`--ui-row-mark`,
  `--ui-rail-account-dot`; ohne `--ui-avatar*`, Tönungen, Zuschnitt, Schrift 9 px)
- **Tests:** Rust `contacts_view_tests::the_migration_removes_pictures_and_their_sync_records`,
  `notes_are_the_user_s_and_are_captured_for_the_sync`, `vcard::tests::photos_are_skipped_without_a_problem`,
  `written_cards_follow_each_version`, Engine `vcard::tests` (Import ohne Bild, Export ohne
  `PHOTO`), `settings_sync::crypto::tests::newer_formats_and_kinds_are_unsupported_not_damaged`,
  `settings_sync::tests::scenarios::picture_records_of_an_older_version_are_ignored_quietly`,
  `record::tests::registry_lists_every_kind_once_in_enum_order`; Vitest `no-avatars.test.tsx`
  (Markenspalte in Zeilen, schmalen Zeilen komfortabel und kompakt, Tabelle; Kontopunkt in
  gemischten Listen; schmale Leiste; Einstellungen → Konten; axe), `contacts-view.test.tsx`,
  `new-mail-popup.test.tsx`, `conversation-tree.test.tsx`, `conversations.test.tsx`,
  `actions.test.tsx`, `unified-inbox.test.tsx`, `search.test.tsx`, `contrast.test.ts`; Desktop
  `stage22-ui.e2e.mjs`, `context-menu.e2e.mjs`, `conversations.e2e.mjs`, `contacts.e2e.mjs`,
  `spacing.e2e.mjs`

## Entscheidung

**1. Alles mit Bildern raus.** Es gibt keine Avatare, keine Initialen, keine Kontakt- und keine
Kontobilder mehr: nicht in der Mailliste (Zeilen, Tabelle, Kindzeilen, Bündel), nicht im
Lesebereich, nicht an den Empfänger-Chips des Composers, nicht in den Kontakten (Liste, Seite,
Gruppen), nicht im Popup für neue Mails, nicht in der schmalen Leiste und nicht in Einstellungen →
Konten. Mit den Bildern gehen ihre Auswahl (Systemdialog, Ziehen auf das Fenster), der Zuschnitt,
die Befehle (`pickContactPicture`, `loadDroppedContactPicture`, `setContactPicture`,
`removeContactPicture`, `getContactPicture`, `listContactPictures`), die Fehlercodes
`picture_too_large`, `picture_unsupported`, `picture_damaged`, das Feld `picture` an Kontakten und
Popup-Karten, `pictures` im vCard-Bericht und der Modus `--contact-picture` des Helfers.

**2. Gespeicherte Bilder löscht Migration 0028.** Sie entfernt die Tabelle `contact_picture` samt
ihren Triggern und die Zeilen dieses Geräts in `sync_record` und `sync_dirty` mit der Art
`contactPicture` (sie enthielten dieselben JPEGs in Base64). Bilder lagen nur in der Datenbank,
nie als Dateien. Die Sicherungen vor Migrationen (höchstens drei, `backups/`) behalten sie, bis
neuere sie ersetzen: Sie sind der Rückweg einer fehlgeschlagenen Migration.

**3. Bild-Datensätze anderer Geräte werden still ignoriert.** `contactPicture` ist eine
„stillgelegte Art“ (`RETIRED_KINDS`): Ein solcher Eintrag im Sync-Ordner wird geöffnet und, ohne
seinen Inhalt zu prüfen, verworfen (`OpenError::Retired`, `Notice::Retired`): nie angewandt, nie
geschrieben, nie gelöscht und nicht als „abgelehnt“ gezählt (der Status zeigt ihn nicht). Er bleibt
für Geräte mit älterer Version auf dem Server. Wie jeder Eintrag, der hier kein Datensatz ist, wird
er erneut geladen, wenn sich der Ordner ändert.

**4. vCard.** Der Import überspringt `PHOTO` wie jede Eigenschaft, die die App nicht liest: weder
dekodiert noch als Problem gemeldet (die Zeilengrenze von 6 MiB fasst weiter ein Foto von 4 MiB, so
kommt der Rest der Karte an). Der Export schreibt kein `PHOTO`.

**5. Was der Ring trug, bleibt: der Punkt in der Kontofarbe.** In Listen, die Konten mischen
(vereinter Posteingang, Ansichten, Suche über Konten), zeigt das Kontoetikett nach dem Betreff
(Punkt in der Kontofarbe, 8 px, und Name, seit Stufe 8) das Konto. Bisher wich es mit Markierungen
und Zeit den Schnellaktionen, und der Ring des Avatars trug das Konto in der Zeit (0065, Punkt 2).
Jetzt bleibt das Etikett stehen, vor den Schnellaktionen; nur Markierungen und Zeit weichen. Fehlt
Platz, gibt es wie bisher erst seinen Namen her, bis nur der Punkt bleibt. Tooltip und Name der
Zeile nennen das Konto (SPEC: eine Kontofarbe steht nie allein). Die Tabelle behält ihre Spalte
„Konto“ mit Punkt und Name.

**6. Markenspalte statt Avatar.** Jede Zeile (Zeile, Tabelle, Kindzeile, Bündel) beginnt nach
ihrem Vorlauf (`--ui-row-lead`, 16 px) mit einer schmalen festen Spalte, so breit wie das
Auswahlkästchen (`--ui-row-mark`, 16 px): ungelesen der Punkt (8 px, `accent`), beim Überfahren und
bei Mehrfachauswahl (dann in jeder Zeile) an seiner Stelle das Kästchen, wie die Tabelle es seit
0061 tut. Ungelesen ist dazu halbfett, nie nur der Punkt. Der Absender folgt 8 px danach (wie
Symbol und Text, 0071). Vom Rand der Zeile bis zum Absender sind es 40 px (mit Avatar 60 px, vor
0065 84 px); eine gelesene Zeile hat davor 32 px ohne Inhalt. Die Spalte steht auf der Linie von
Titel der Ansicht und Abschnitten („Heute“), wo vorher die Avatare standen (0071, Punkt 5). Die
Tabelle bekommt denselben Vorlauf (vorher 12 px) samt Platzhalter im Kopf, damit ihre Marken auf
derselben Linie stehen. In schmalen Zeilen steht die Marke mittig zur ersten Zeile.

**7. Die Linie der Gespräche** läuft mittig unter der Markenspalte (`--ui-tree-line-start`). In
der breiten Zeile beginnt sie in der Mitte der Zeile, wo die Marke steht; in der schmalen ist die
Zeile ein Raster aus drei Zeilen (`1fr auto 1fr`), der Inhalt in der mittleren, und die Linie liegt
in einem Kasten von der Oberkante des Inhalts bis zur Unterkante der Zeile und beginnt dort mittig
zur ersten Zeile: Sie braucht keine Höhen und setzt immer an der Marke an. Der Punkt für ungelesen
deckt ihren Anfang.

**8. Bündel, Leiste, Konten.** Bündel zeigen statt der Symbolkachel das Symbol (16 px,
`text-secondary`) vor ihrem Namen, wo der Absender steht, die Chips ohne Mini-Avatar. In der
schmalen Leiste ist jedes Konto ein Punkt in seiner Farbe (12 px, `--ui-rail-account-dot`) in
seinem 40-px-Eintrag, Name als Tooltip und zugänglicher Name; das Zustandsabzeichen (offline,
Fehler) steht unten am Ende des Eintrags statt auf dem Bild. Einstellungen → Konten zeigt den
Punkt (8 px) auf der Höhe des Namens, die Seite eines Kontos einen 12-px-Punkt am Titel.

## Verworfen

- **Kontopunkt in der Markenspalte:** dann hätte ein Punkt zwei Bedeutungen (ungelesen oder
  Konto), und beides zugleich ginge nicht.
- **Kontopunkt vor dem Absender** (so zuerst gebaut und im Bildschirmfoto geprüft): neben dem
  Ungelesen-Punkt standen zwei Punkte, bei einem blauen Konto im blauen Akzent kaum zu
  unterscheiden („• • Grace Hopper“).
- **Farbbalken am Rand der Zeile:** schon in 0065 verworfen (nur Farbe, fällt mit Auswahl und
  Fokusring zusammen).
- **Avatare abschaltbar lassen:** der Mensch will „alles mit Bildern raus“; eine Einstellung hielte
  den ganzen Bildweg am Leben (Helfer, Sync, Zuschnitt) für etwas, das niemand nutzt.
- **Bild-Einträge im Sync-Ordner löschen:** ältere Geräte halten die Bilder noch und würden sie
  als verlorene Datensätze wieder hochladen; ignorieren kostet nur ein erneutes Laden bei
  Änderungen im Ordner.
- **Eine Merkliste ignorierter Einträge (damit sie nicht erneut geladen werden):** eigene Tabelle
  und Planlogik für eine Handvoll Einträge; die Kosten tragen schon heute Einträge neuerer
  Versionen.
- **Breitere Markenspalte (24 px) für mehr Luft:** genau die leere Spalte, die 0065 entfernt hat.

## Folgen

- Tokens entfernt: `--ui-avatar`, `--ui-avatar-mini`, `--ui-avatar-large`, `--ui-cropper-size`,
  `--ui-cropper-mask`, die acht Tönungen `--ui-avatar-n-bg/-text` (hell und dunkel),
  `--ui-font-size-9`; neu `--ui-row-mark`, `--ui-rail-account-dot`.
- IPC-Vertrag kleiner (sechs Befehle, drei Fehlercodes, zwei Felder). Ältere Geräte im selben Sync
  behalten ihre Bilder und sehen die eines neueren Geräts nie mehr geändert.
- `docs/DESIGN.md` (Zeile, Markenspalte, Konto, Bündel, Kindzeilen, Linie, Kontakte, Popup,
  Tabelle, Kanten, Tokens), `docs/ARCHITECTURE.md`, `docs/SYNC.md`, `docs/SECURITY_MODEL.md`,
  README.
- Unter Windows nicht geprüft (Segoe UI, Abstände wie auf dem Mac erwartet).

## Rückweg

`Avatar`, die Tönungen und den Bildweg von Stufe 17 aus der Geschichte zurückholen und die
stillgelegte Art wieder registrieren; gelöschte Bilder kommen nur aus einer Sicherung vor der
Migration zurück.
