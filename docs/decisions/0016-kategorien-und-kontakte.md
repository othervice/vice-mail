# 0016 — Kategorien, Bündel, Kontakte, Signaturen und Vorlagen (reine Logik)

- **Datum:** 23.11.2026
- **Stufe:** 9 und 10, vorgezogen: nur die Logik ohne IO, ohne Migration, ohne Oberfläche
- **Status:** entschieden von der ausführenden Sitzung; Integration (Store, Engine, Oberfläche,
  Einstellungs-Sync) folgt in den Stufen 9 und 10
- **Code:** `crates/mail-model/src/category.rs` (Typen), `crates/mail-model/src/bundle.rs`
  (Bündel), `crates/mail-content/src/classify/` (Regeln), `crates/mail-content/src/contacts/`,
  `crates/mail-content/src/templates/`, `crates/mail-content/tests/classify_seed.rs`

## Ort

- **Typen in `mail-model`:** `MessageCategory`, `CategoryReason`, `CategoryDecision`, `SenderKey`,
  `CategoryCorrection` gehen über IPC („Warum hier?“), in die Datenbank und in den Sync; dort liegen
  serde und specta.
- **Bündel in `mail-model`:** arbeitet auf Listenzeilen, braucht kein Parsing.
- **Regeln, Kontakte, Signaturen, Vorlagen in `mail-content`:** dort liegen das Kopfzeilen-Parsing
  (mail-parser), IDNA (`url`), Punycode, ICU (NFC, NFKD, Zeichenklassen) und die Allowlist für
  Mail-HTML. Keine neue Abhängigkeit.
- Einzige Änderung an bestehendem Code außer `mod`/`pub use`: `compose/mod.rs` reicht das
  Dokumentmodell (`Block`, `Inline`, `fragment`, `html_blocks`, `text_blocks`, `trim_paragraphs`)
  crate-intern weiter, damit Signaturen und Vorlagen genau dieselbe Allowlist benutzen.

## Kategorien

Drei Kategorien: Personen, Benachrichtigungen, Newsletter. „Priorität“ ist keine vierte Kategorie,
sondern ein Merkmal daneben: eine Mail eines wichtigen Absenders bleibt z. B. Newsletter und steht
trotzdem im Block „Priorität“ (und nie in einem Bündel).

**Reihenfolge der Regeln** (die erste, die greift, entscheidet; die Begründung geht als
`CategoryReason` mit dem Beleg aus der Mail an die Oberfläche):

| # | Regel | Kategorie |
|---|---|---|
| 1 | Korrektur des Nutzers: Adresse vor Liste (`List-Id`) vor Domain (längste zuerst, Subdomains eingeschlossen) | wie festgelegt |
| 2 | `Auto-Submitted` außer `no` (RFC 3834) | Benachrichtigung |
| 3 | automatische Antwort: `Precedence: auto_reply`, `X-Autoreply`, `X-Autorespond`, Betreff „Automatische Antwort:“, „Abwesenheitsnotiz“, „Out of Office“ … | Benachrichtigung |
| 4 | Bericht: `multipart/report` (Zustellung, Lesebestätigung), `Return-Path: <>`, Absender `MAILER-DAEMON`/`postmaster` | Benachrichtigung |
| 5 | Kopfzeile eines bekannten Systems: GitHub, GitLab, Gitea, Forgejo, Jira, Bugzilla, Redmine, Phabricator, Gerrit, Jenkins, Discourse, Launchpad, Facebook | Benachrichtigung |
| 6 | Absender ist eine eigene Adresse (auch `name+tag@`) | Person |
| 7 | Antwort auf eine eigene Mail (lokales Signal der Engine) | Person |
| 8 | Liste oder Massenmail: `List-Post` (Diskussionsliste; `List-Post: NO` ist eine Ankündigungsliste), `List-Id`, `Precedence: bulk/list/junk`, Kampagnen-Kopfzeilen (`X-CSA-Complaints`, `X-Campaign…`, `X-Newsletter…`, `X-MC-User`, `X-Report-Abuse` …), Newsletter-Software im `X-Mailer`, Bounce-Domain eines Marketing-Dienstes (Mailchimp, HubSpot, Campaign Monitor, Klaviyo …) | Newsletter |
| 9 | der Nutzer hat dem Absender schon geschrieben (Kontakte) | Person |
| 10 | `List-Unsubscribe` | Newsletter |
| 11 | No-Reply-Adresse (`noreply`, `donotreply`, `nicht-antworten`, `keine-antwort`, `bounce` …) | Benachrichtigung |
| 12 | Adresse eines Programms (`notifications`, `alerts`, `rechnung`, `…bestaetigung`, `bestellung`, `versand`, `calendar` …) | Benachrichtigung |
| 13 | Newsletter-Adresse (`newsletter`, `news`, `angebote`, `marketing` …, auch als Subdomain) | Newsletter |
| 14 | Versanddienst ohne Abmeldung: `Feedback-ID`, `X-SES-Outgoing`, `X-SG-EID`, `X-Mailgun-Sid`, `X-PM-Message-Id`, Bounce-Adresse (VERP, `-bounces`, `owner-`) | Benachrichtigung |
| 15 | sonst | Person |

**Begründungen der Reihenfolge**
- Korrekturen gehen immer vor, auch vor `Auto-Submitted`.
- Maschinensignale (2–5) vor eigenen Mails und Antworten: eine Abwesenheitsnotiz auf die eigene
  Mail bleibt eine Benachrichtigung; GitHub schickt `List-Id` und `List-Unsubscribe` mit, ist aber
  eine Benachrichtigung.
- Antworten auf eigene Mails (7) vor Listen (8): wer auf der Liste auf meine Frage antwortet, ist
  ein Gespräch.
- Starke Listen-Signale (8) vor der Beziehung (9), schwache (10, 14) danach: ein Kontakt, der über
  ein CRM mit `List-Unsubscribe` schreibt, bleibt Person; ein Vereins-Newsletter mit `List-Id`
  bleibt Newsletter, auch wenn ich dem Verein einmal geschrieben habe.
- Rollen-Adressen (12) erst nach der Beziehung: wer `rechnung@` eine Frage gestellt hat, bekommt
  die Antwort bei den Personen. Bewusst nicht als Muster: `info@`, `service@`, `support@`, `team@`
  (oft echte Menschen). Eine falsch versteckte Personen-Mail wiegt schwerer als eine
  Benachrichtigung zu viel bei den Personen.
- Absendermuster falten Groß/Klein und Umlaute und ignorieren Trenner und Ziffern
  (`No-Reply`, `no_reply2`, `nicht.antworten`, `Bestellbestätigung`).
- Die `Sender`-Kopfzeile zählt nur als Bounce-Adresse einer Liste, nicht als Muster: Google
  Kalender schickt Einladungen von Personen mit `Sender: calendar-notification@google.com`.
- Nicht benutzt: `X-Auto-Response-Suppress` allein (eine Bitte an die Abwesenheitsantwort des
  Empfängers, keine Aussage über den Absender), „nicht in To/Cc“ (Bcc von Personen ist legitim).

**Speichern und Korrekturen.** Die Engine speichert das Ergebnis von `classify_automatic` (ohne
Nutzerregeln) mit der Mail und legt `apply_user_rules` darüber, auch erneut, wenn eine Korrektur
oder Wichtig-Markierung sich ändert. Dafür braucht jede Mail Absenderadresse (normalisiert),
Domain und List-Id. Lokale Signale (Kontakt, Antwort auf eigene Mail) gelten ab dem Eintreffen;
alte Mails werden nicht nachträglich umsortiert.

**Kopfzeilen.** `CLASSIFY_HEADER_FIELDS` listet, was die Regeln lesen; der Adapter holt genau
diese Felder beim Eintreffen (`BODY.PEEK[HEADER.FIELDS (…)]`) und reicht die Bytes an
`ClassifyHeaders::from_raw`. Die heute gespeicherten Kopfdaten (`MessageHeader`:
From, Betreff, `List-Id`, `List-Unsubscribe`, `Auto-Submitted`, `Precedence`) reichen über
`ClassifyHeaders::from_message_header` für die Regeln 1–3, 6–13 ohne `List-Post` und
Kampagnen-Kopfzeilen; Berichte (4), bekannte Systeme (5) und Versanddienste (14) brauchen die
zusätzlichen Felder.

**Trefferquote.** Seed-Korpus (76 Mails, von Hand eingeordnet): 75 von 76 = 98,7 %. Der Fehler ist
die Phishing-Mail `html_hostile.eml` von `service@bank.example` ohne jede Programm-Kopfzeile; sie
landet bei den Personen. Die 60 Tabellenfälle in `classify/tests.rs` (auch Paketdienst, Bahn,
ELSTER, Krankenkasse, Stadtwerke, Sparkasse, Shop mit Amazon SES, CleverReach-artige Versender mit
`X-CSA-Complaints`) sind nach diesen Regeln geschrieben und treffen deshalb alle; die echte Quote
zeigt erst der eigene Posteingang („Der Mensch prüft danach“ in Stufe 9).

**Bekannte Grenzen.** Phishing und Transaktionsmails ohne Programm-Kopfzeilen von neutralen
Adressen (`service@`) gelten als Person. Ein Newsletter nur mit `List-Unsubscribe` von einem
Absender, dem ich geschrieben habe, gilt als Person. Absender wie YouTube (No-Reply mit
`List-Unsubscribe`) gelten als Newsletter.

## Bündel

- Ein Bündel ist Kategorie × Datumsabschnitt (`BundleKey`). Den Abschnitt (heute, gestern, diese
  Woche, Monat) berechnet der Aufrufer in der Zeitzone des Nutzers; die Funktion kennt keine Uhr.
- Ein Bündel steht an der Stelle seiner neuesten Mail (erstes Vorkommen in der neueste-zuerst-Liste).
- Keyset: `BundleState` trägt die begonnenen Bündel zur nächsten Seite. Mails eines dort schon
  begonnenen Bündels kommen als `BundleRow::More` und werden in die frühere Zeile gehängt; die
  Zusammenfassung zählt alles bisher Gesehene. Ein Test schneidet die Liste an jeder Stelle und
  prüft, dass das Zusammengefügte gleich dem Ergebnis in einem Durchgang ist. `BundleState` ist
  serialisierbar; die Engine kann ihn mit dem Cursor reichen oder für die erste Seite nach einem
  Cursor aus den Mails des laufenden Abschnitts über dem Cursor neu aufbauen.
- Auch ein Bündel mit einer Mail bleibt ein Bündel (spätere Seiten können es vergrößern); die
  Oberfläche darf es wie eine normale Zeile zeichnen.
- Chips: die häufigsten Absender (Adresse ohne Groß/Klein), bei Gleichstand der mit der neueren
  Mail, höchstens drei; je Bündel werden höchstens 256 verschiedene Absender gezählt.
- Personen, wichtige Absender und (vom Aufrufer gesetzt) angepinnte Mails werden nie gebündelt.
  `BundleOptions::off()`: Kategorien aus, die Liste ist rein zeitlich.

## Kontakte

- **Schlüssel:** lokaler Teil in NFC und Kleinbuchstaben, Domain in ASCII-Kleinbuchstaben (IDNA,
  UTS 46 über `url`). Anzeige mit Unicode-Domain. Geschrieben wird an die zuletzt gesehene
  Schreibweise des lokalen Teils (Server dürften Groß/Klein theoretisch unterscheiden).
  Plus-Adressen und Gmail-Varianten (Punkte, `googlemail.com`) bleiben getrennte Kontakte und
  werden nur als Dubletten vorgeschlagen.
- **Quellen:** gesendete Mails: alle Empfänger (To, Cc, Bcc); empfangene: nur der Absender und nur
  bei Kategorie Person (`observations`). Ausgenommen: eigene Adressen (auch `+tag`), No-Reply-
  Adressen, ungültige Adressen.
- **Name:** vom Nutzer gesetzt, sonst der meistgesehene. Schreibweisen, die sich nur in Groß/Klein,
  Akzenten, Umlaut-Schreibung, Reihenfolge („Müller, Hans“) oder Initialen unterscheiden, zählen
  zusammen; gezeigt wird die häufigste Schreibweise (Gleichstand: neuer, dann gemischte
  Groß-/Kleinschreibung statt GROSS). Kein Name: Adressen, der lokale Teil der Adresse, Namen
  ohne Buchstaben. Höchstens 16 Namen je Kontakt.
- **Rangfolge:** `score(now) = Σ w·2^(−(now − t)/H)`, Halbwertszeit H = 30 Tage, gesendet w = 1,
  empfangen w = 0,25. Exponentieller Zerfall, weil er Häufigkeit und Aktualität in einer Zahl
  verbindet, ohne Verlauf fortgeschrieben werden kann (Reihenfolge der Nachrichten egal) und die
  Reihenfolge zweier Kontakte mit der Zeit nicht ändert. Gespeichert wird deshalb nur
  `key = log2(score(t)) + t/H` (eine REAL-Spalte, sortierbar per Index, nie neu zu berechnen);
  Hinzufügen und Zusammenführen sind ein Log-Sum-Exp. Beispiele: wöchentlich geschrieben ≈ 6,7;
  eine Mail gestern ≈ 1; zehn Mails vor einem Jahr < 0,01; vier empfangene = eine gesendete.
- **Suche:** jedes Wort der Eingabe muss den Anfang eines Worts aus Name oder Adresse treffen
  (auch ganze Adresse, lokaler Teil, Domain und ihre Teile, ASCII und Unicode). Gefaltet mit NFKD
  ohne Zeichen der Kategorien Mn/Mc/Me, Kleinbuchstaben, ß→ss, æ→ae, ø→o, ł→l usw.; Namen und
  Domains zusätzlich mit ausgeschriebenen Umlauten, damit „mue“ „Müller“ findet. Kein Teilwort
  („ller“ findet nichts). Sortiert nach Rang. Die Wörter je Kontakt werden bei jeder Änderung neu
  berechnet: 5 Suchen über 5.000 Kontakte 0,6 ms (Release, M4).
- **Dubletten (nur Vorschläge):** Plus-Adresse und Basisadresse; gleiches Gmail-Postfach; gleicher
  Name (mindestens zwei Wörter, Gruppen bis zehn) an verschiedenen Adressen; eine Adresse mit
  verschiedenen Namen (zur Namenswahl). Abgelehnte Paare kommen nicht wieder. Zusammengeführt wird
  nur mit `ContactBook::merge` (Operation `ContactMerge`): Adressen, Zähler, Rang (Log-Sum-Exp) und
  Namen werden vereint, die bevorzugte Adresse des behaltenen Kontakts bleibt vorn.

## Signaturen und Vorlagen

- HTML aus Signatur- und Vorlagen-Editor (oder eingefügt aus Outlook/Word) läuft durch dieselbe
  Allowlist wie Mail-HTML (`mail_html`): Absätze, Umbrüche, fett, kursiv, unterstrichen,
  durchgestrichen, Code, Listen, Zitate, Vorformatiertes, Links nur `http`, `https`, `mailto`
  (`tel:` wird Text). `SanitizeNotes` meldet entfernte Bilder und vereinfachte Formatierung.
  **Bilder in Signaturen gehen dabei verloren**, bis die Allowlist mit den Inline-Bildern (Stufe 10)
  `cid:`-Bilder kennt.
- Grenzen: Eingabe 2 MiB; Signatur 16 KiB, Vorlage 96 KiB als Editor-HTML (Sync-Datensätze 64 bzw.
  256 KiB); Betreff einer Vorlage 255 Zeichen, Leerraum zusammengefasst, ohne Steuerzeichen.
- Ein vom Nutzer getippter Trenner am Anfang („-- “, „--“, „—“) wird entfernt; der Trenner `-- `
  (RFC 3676, 4.3) kommt nur in die Textfassung, beim Zusammensetzen.
- **Platzhalter:** `{{first_name}}`, `{{last_name}}`, `{{name}}`, `{{email}}`, `{{date}}`,
  `{{my_name}}`, `{{my_first_name}}`, `{{my_email}}`, mit Ersatz `{{first_name|zusammen}}`,
  Groß/Klein egal, Leerzeichen erlaubt. Bezug ist der erste To-Empfänger. Ersetzt wird im
  Dokumentmodell (Textläufe, Vorformatiertes), nie im HTML-Quelltext und nie in Link-Zielen; Werte
  werden beim Schreiben maskiert und nicht erneut ausgewertet. Unbekannte Platzhalter bleiben
  stehen und werden gemeldet; fehlende Werte ohne Ersatz werden leer und gemeldet. Ein durch
  Formatierung geteilter Platzhalter wird nicht erkannt. Vorname: Titel (`Dr.`, `Prof.`, `Herr`,
  `Dipl.-Ing.` …), Zusätze in Klammern und nach `|` oder ` - ` weg, „Nachname, Vorname“ erkannt,
  Namenspartikel (`von der`) beim Nachnamen; ohne Anzeigename wird nichts aus der Adresse geraten.
- `{{date}}`: das Datum gibt der Aufrufer in der Zeitzone des Nutzers; Deutsch „03.11.2026“,
  Englisch „3 November 2026“.
- **Zusammensetzen** (`assemble_draft`): Text des Nutzers, Signatur, Zitat. Standard: Antwort und
  Signatur über dem Zitat; mit „unter dem Zitat antworten“ Zitat, Antwort, Signatur. Neue Mail:
  eine leere Zeile für den Cursor, darunter die Signatur. Weiterleitungen wie Antworten über dem
  Zitat. Die Textfassung hat `-- ` vor der Signatur. Der Composer hält die drei Teile getrennt (die
  Signatur bleibt wechselbar) und setzt erst beim Speichern oder Senden zusammen; ein vom Server
  wieder geöffneter Entwurf hat die Signatur als normalen Text im Inhalt.

## Für die Integration

- **Migration:** `message` bekommt `category` (`MessageCategory::as_str`), `category_reason`
  (JSON von `CategoryReason`), normalisierte Absenderadresse, Domain und List-Id; `important` wird
  beim Lesen per `apply_user_rules` oder als Spalte mit Neuberechnung bei Regeländerung gesetzt.
  Korrekturen und wichtige Absender als `rule` (sync, 16 KiB). `contact` mit einer Zeile je Adresse
  plus Kontaktgruppe (für Zusammenführungen), `rank` als REAL mit Index, Zähler, letzte Nutzung,
  Namen (bis 16) und `user_name`; von Hand bearbeitete Einträge und Zusammenführungen sind sync
  (Datenart fehlt noch im Register von `docs/SYNC.md`), abgelehnte Dubletten-Paare auch.
  `signature`/`template` als `{html, text}` bzw. `{subject, html}`; Standard-Signatur je Identität
  getrennt für neue Mails und Antworten.
- **Engine:** beim Eintreffen `CLASSIFY_HEADER_FIELDS` holen, Kontakt-Signal (`sent`) und
  „Antwort auf eigene Mail“ (In-Reply-To/References gegen eigene Message-IDs) bestimmen; beim
  Senden und Empfangen `observations` in die Kontakte. `MessageListItem` braucht `category` und
  `important`, damit `BundleItem` umsetzbar ist.
- **Oberfläche:** Texte für jede `CategoryReason`-Art in `app/src/i18n/` (Englisch, Deutsch);
  „immer als …“ je Adresse, Liste oder Domain; Hinweise aus `SanitizeNotes`; Platzhalter-Probleme.
