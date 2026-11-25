# 0060 — Anhänge-Ansicht und Postfach-Speicher

- **Datum:** 24.11.2026
- **Anlass:** Stufe 20, Aufgaben C und E (Auswahl des Menschen vom 24.11.2026).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 20).
- **Code:** Migration `0025_attachment_index.sql`, `crates/mail-store/src/attachment_index.rs`,
  `crates/mail-store/src/fts.rs` (`BodyText::attachment_index`),
  `crates/mail-engine/src/reading/index_text.rs`, `crates/mail-engine/src/attachments_view.rs`,
  `crates/mail-engine/src/reading/mod.rs` (Anhang ohne gespeicherten Körper wird geladen),
  `crates/mail-model/src/attachments_view.rs`, `app/src/features/reading/AttachmentsView.tsx`;
  `crates/mail-model/src/quota.rs`, `crates/mail-model/src/backend.rs` (`MailBackend::quota`),
  `crates/mail-imap/src/backend.rs` (`GETQUOTAROOT`), `crates/mail-imap/src/command.rs`
  (Lese-Allowlist, `[OVERQUOTA]`), `crates/mail-jmap/src/backend.rs` (`Quota/get`),
  `crates/mail-jmap/src/error.rs` (`overQuota`), `crates/mail-engine/src/reading/fetch.rs`,
  `app/src/features/settings/accounts/QuotaBlock.tsx`, `app/src/features/accounts/quota.ts`,
  `app/src/features/mailboxes/MailboxTree.tsx`, Suchfilter `larger:`
  (`crates/mail-model/src/search/parse.rs`, `compile.rs`, `crates/mail-store/src/search.rs`),
  `testmail/quota.py`.

## C — Anhänge-Ansicht

### Woher die Liste kommt

Eine neue Cache-Tabelle `attachment` (Mail-ID, Index, Name, Name klein für die Suche, Typ, Art,
Größe, IMAP-Teil) hält die Anhänge jeder Mail, deren Körper auf diesem Computer liegt – ganz, oder
bei großen Mails die Struktur mit den Textteilen (Anhänge „noch nicht geladen“). Geschrieben wird
in derselben Transaktion wie der Suchindex des Körpers, aus derselben Liste, die der Lesebereich
zeigt (`view::Content`): Der Index ist der des Lesebereichs, Öffnen, Sichern und Vorschau sind
dessen Befehle. Bereits gespeicherte Körper füllt einmal der Suchindex-Job nach (die Migration
legt ihn an, wenn es Körper gibt).

Bewusst **nicht** aus der `BODYSTRUCTURE` des Kopf-Abgleichs: deren Teile haben nicht die Indizes
des Lesebereichs (eingebettete Bilder, die das HTML zeigt, fehlen dort), bereits abgeglichene Mails
hätten keine Zeilen ohne erneutes Abrufen aller Strukturen, und der Abgleich würde für jede Mail mehr
schreiben. Die Ansicht zeigt deshalb die Anhänge der Mails auf diesem Computer; wie weit das
zurückreicht, bestimmt „Vorab laden“ (Einstellungen → Lesen; Standard 30 Tage für Posteingang,
Gesendet, Entwürfe, Markiert; „Alles“ für alles), und jede geöffnete Mail kommt dazu. Die Ansicht
sagt das. Ein aus dem Cache entfernter Körper behält seine Zeilen (wie der Suchindex seinen Text);
öffnen, sichern oder vorschauen lädt die Mail dann wie das Öffnen im Lesebereich.

### Abfrage und Oberfläche

Neueste Mail zuerst, Keyset über (Mail-ID, Index): ein Rückwärtsgang über den Primärschlüssel oder
den Index `attachment_kind (kind, message_id)`; Konto (`m.account_id IN …`), Dateiname
(`instr` auf dem kleinen Namen) und Absender (Name oder Adresse) sind Filter auf diesem Gang, die
Mail ein Zugriff über den Schlüssel. Papierkorb und Spam bleiben weg; angezeigt wird die Mail im
Posteingang, sonst in einem anderen Ordner. Gemessen (Debug-Build, 20.000 Anhänge): erste Seite mit
jedem Filter unter 400 ms, ohne Filter wenige Millisekunden; Planprüfung im Test. Die Oberfläche
ist eine virtuelle Liste (TanStack Virtual wie die Mailliste) mit Seiten zu 200, Filtern nach Art
(Bilder, PDF, Dokumente, Tabellen, Archive, Sonstige), Absender und Konto und einer Suche im
Dateinamen. Je Zeile: Zur Mail, Vorschau (Bilder nur über den abgeschotteten Hilfsprozess, 0043/0046),
Öffnen (ausführbare erst nach der Warnung), Sichern. Die Ansicht `attachments` steht nach
„Angeheftete Anhänge“ unter den Ansichten, abschaltbar wie die anderen (0050).

## E — Postfach-Speicher

### Abfragen

`MailBackend::quota()` (Standard: nichts gemeldet). IMAP: `GETQUOTAROOT INBOX` (RFC 9208) nur mit
`QUOTA`/`QUOTA=…`; die Ressource `STORAGE` (Einheiten zu 1024 Oktetten) der ersten Wurzel, die der
Server für die INBOX nennt. `GETQUOTAROOT` und `GETQUOTA` stehen auf der Lese-Allowlist, die
Abfrage läuft über die Leseverbindung (nur lesend). JMAP: `Quota/get` (RFC 9425) mit
`urn:ietf:params:jmap:quota`, wenn die Sitzung es anbietet; die Quote vom Typ `octets` für Mail
(`types` mit `Mail` oder leer), `account` vor `domain` und `global`; `Quota/get` steht auf der
JMAP-Allowlist. Ohne Grenze (`hardLimit`/Limit 0) gilt „nicht gemeldet“.

### Anzeige

`getAccountQuota(accountId, refresh)` antwortet `reported` (belegt, Grenze, Prozent, ab 90 %
`nearlyFull`), `notReported` („Der Server nennt keinen Speicherplatz“) oder `unknown` (offline,
abgelehnt; mit Fehler). Der Kern hält eine Antwort 15 Minuten, „Erneut prüfen“ fragt sofort. In den
Kontoeinstellungen: belegt von Grenze, frei, ein `<meter>` (kein Inline-Stil nötig, CSP), ab 90 %
ein Hinweis mit „Große Mails suchen“. In der Spalte trägt das Konto ab 90 % ein Symbol und der
Name sagt „Postfach zu 92 % voll“.

### Volles Postfach

`NO [OVERQUOTA]` (IMAP, RFC 9208/5530) und `overQuota` (JMAP-SetError) werden zu
`BackendError::OverQuota`, Fehlercode `over_quota`, nicht vorübergehend. Scheitert damit das
Ablegen eines Entwurfs oder der Gesendet-Kopie, zeigt die App dort, wo solche Fehler erscheinen
(Aktivität, Postausgang, Sendehinweis), „Das Postfach auf dem Server ist voll“ mit dem Weg: `larger:10M` suchen, Unnötiges löschen,
Papierkorb leeren. Dafür kennt die Suchsprache den Filter **`larger:`** (`größer:`, Einheiten K, M,
G zu 1024, B; `-larger:` = kleiner), lokal über `message.size` (RFC822.SIZE); „größte zuerst“ als
eigene Sortierung gibt es nicht, `larger:` findet die großen.

### Testserver

Stalwart 1.1.1 meldet `QUOTA` und `QUOTA=RES-STORAGE`; ohne gesetzte Grenze antwortet es
`* QUOTA "#1" ()` (nicht gemeldet). `testmail/quota.py` setzt über die Verwaltungs-API eine
Grenze (`quotas.maxDiskQuota`) oder entfernt sie; dann meldet der Server `STORAGE` und lehnt ein
zu großes `APPEND` mit `NO [OVERQUOTA] Quota exceeded.` ab. JMAP-Quoten sind gegen ein Testdouble
geprüft (Auswahl der Quote), nicht gegen Stalwarts JMAP.

## Abgelehnt

- **Anhänge-Ansicht aus BODYSTRUCTURE** (siehe oben).
- **Speicher vom Server bei jedem Abgleich lesen:** eine Abfrage pro Kontoansicht und Viertelstunde
  reicht; der Abgleich bleibt unverändert.
- **Eigene Sortierung „größte zuerst“ in der Suche:** die Treffer kommen über den Index nach Datum;
  `larger:` reicht, um Platz zu schaffen.

## Tests

`mail-store`: Zeilen mit Filtern und Seiten, Papierkorb weg, Ersatz beim neuen Körper, Plan und
Zeit mit 20.000 Anhängen. `mail-engine`: Ansicht über `AppCore`, Sichern mit dem Index, auch nach
entferntem Körper (wird geladen); Quote über die Leseverbindung, gehalten und erneut gefragt.
`mail-imap`: `GETQUOTAROOT` per Skript-Server (Wurzel, Einheiten, Ablehnung, ohne Fähigkeit nichts),
`[OVERQUOTA]`; Servertest `tests/quota.rs` gegen Stalwart. `mail-jmap`: Auswahl der Quote.
`mail-model`: `larger:`. Vitest und Desktop-Test `stage20.e2e.mjs` (C, E).
