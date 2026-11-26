# 0077 — Lernender Spamfilter (lokal)

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 6 (Auswahl des Menschen vom 26.11.2026): ein Filter, der lokal aus
  „Spam“ und „Kein Spam“ lernt, wie bei Thunderbird; ohne Server, ohne neue Abhängigkeit.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22, Worktree
  `wt-s22-spam`).
- **Code:** `crates/mail-content/src/spam/` (Tokens, Bewertung, Korpus-Messung),
  Migration `0031_spam_filter.sql`, `crates/mail-store/src/spam.rs` (Modell, Lerneinträge, Urteile,
  Kandidaten, Fakten zum Absender; Markierung der Listenzeilen in `marks.rs`),
  `crates/mail-model/src/spam_filter.rs`, `crates/mail-engine/src/spam/` (Lernen, neue Mails,
  Lernen aus vorhandenen Mails, Server-Test), `reading/mod.rs` (`spam_tokens`), `core.rs`
  (`apply_action`, `undo_action`), `comfort/mod.rs` (Ankunft), `settings_sync/record.rs`
  (`spam.filter`), `app/src-tauri/src/commands/spam.rs`, `app/src/features/spam/`,
  `comfort/RowMarks.tsx`, `comfort/notices.ts`.

## Entscheidung

### Verfahren

- **Robinson/Fisher wie SpamBayes und Thunderbird**, in `mail-content` ohne neue Abhängigkeit:
  je Token die Wahrscheinlichkeit nach Robinson (`s = 0,45`, `x = 0,5`), Tokens näher als 0,1 an
  0,5 zählen nicht, höchstens die 150 stärksten; Fishers Chi-Quadrat-Kombination für „Spam“ und
  „Ham“, Wert `(1 + S − H) / 2`. Ein Token zählt je Mail einmal.
- **Tokens** (`spam_tokens`): Wörter aus Betreff und Text (3–12 Zeichen, Kleinschrift, Satzzeichen
  am Rand weg, längere als `skip:<Anfang> <Zehner>`), bei HTML nur der sichtbare Text, dazu
  „versteckter Text vorhanden“, Zahl der Bilder und Links und die Hosts der Links; Absender,
  Antwort an, Return-Path und Sender als Adresse, Domain samt Eltern und Name; „Antwort an zeigt
  auf eine andere Domain“; Namen aller Kopfzeilen; aus `Received` Hostnamen, IPv4-Präfixe und
  Protokoll; `Authentication-Results` (`spf=…`, `dkim=…`, `dmarc=…`), `Received-SPF`,
  DKIM-Domain und ob sie zum Absender passt; das Urteil eines Server-Filters (`X-Spam-*`,
  `X-Rspamd-*`, Microsofts SCL); Content-Type, Zeichensatz, Anhangstypen und -endungen; Zahl der
  Empfänger, `undisclosed-recipients`. Höchstens 2.000 Tokens, Text bis 64 KiB, HTML bis 256 KiB.
- **Nur Hashes in der Datenbank:** jedes Token als 64-Bit-FNV-1a (stabil über Versionen und
  Plattformen, `std`-Hasher wäre es nicht). Die Datenbank enthält so keine Wörter aus Mails.
- **Ohne gespeicherten Text** liefern die gespeicherten Kopfdaten eine Teilmenge in derselben
  Schreibweise (Absender, Antwort an, Empfängerzahl, Betreff, Liste, Vorschau;
  `spam_tokens_of_fields`, ein Test prüft „Teilmenge“). Weniger Tokens heißt: Wert näher an 0,5,
  also vorsichtiger.

### Schwelle und wann er handelt (gemessen, siehe `docs/testing/spam-filter.md`)

- **Ab 0,99** gilt eine Mail als Spam (SpamBayes und Thunderbird nehmen 0,9). Im SpamAssassin-
  Korpus (4.150 Ham, 1.896 Spam, zehnfache Kreuzvalidierung) 0,05 % falsch als Spam bei 8,8 %
  übersehenem Spam; bei 0,9 wären es 0,17 %.
- **Erst nach 50 Spam- und 200 anderen gelernten Mails.** Mit wenigen gleich vielen Mails je Klasse
  ist der Filter unbrauchbar (10/10: 6,6 % falsch als Spam bei 0,99; 50/50: 1,2 %); entscheidend ist
  viel gelernter Ham (50/200: 0 %, 200/1.000: 0,06 %).
- **Doppelt so viel Ham wie Spam:** Lernt die Person mehr Spam, als halb so viel Ham da ist, lernt
  der Filter danach bis zu 200 gelesene Posteingangsmails als Ham nach (beantwortete zuerst; Mails,
  die er selbst schon für wahrscheinlich Spam hält, ab 0,9, nicht).

### Lernen

- **Aus „Als Spam markieren“ und „Kein Spam“** (`applyAction` mit `spam`/`notSpam`, nur der
  Person; die eigenen Verschiebungen des Kerns – blockierte Absender, Kontakt als Spam, der Filter
  selbst – lehren nichts). Gelernt wird im Hintergrund in Reihenfolge (ein Arbeiter), aus dem
  gespeicherten Text oder sonst den Kopfdaten, ohne Netz: Die Mail ist nach dem Verschieben bis zum
  nächsten Abgleich ohne Ort auf dem Server. Neue Posteingangsmails holt der Filter ohnehin beim
  Eintreffen (und das Vorabladen), daher ist der Text meist da.
- **Jede gelernte Mail merkt sich genau ihre Tokens** (`spam_trained.tokens`). Wird sie in der
  anderen Klasse gelernt (Spam → Kein Spam), wird sie zuerst mit diesen Tokens verlernt, auch wenn
  sich ihr Text inzwischen geändert hat (z. B. erst jetzt ganz gespeichert).
- **Rückgängig:** Der Lerneintrag trägt den Stapel der Outbox; `undoAction` verlernt ihn. Einen
  Stapel, mit dem der Filter selbst verschoben hat, lernt „Rückgängig“ als Ham („war kein Spam“).
- **Aus vorhandenen Mails** auf Wunsch (Knopf in den Einstellungen): die neuesten Mails der
  Spam-Ordner als Spam (höchstens 500), gelesene Posteingangsmails als Ham (höchstens 1.000,
  beantwortete zuerst), im Hintergrund mit Fortschritt (`spamFilterChanged`) und „Anhalten“. Nicht
  gespeicherte Mails werden wie beim Öffnen nur lesend geholt (`BODY.PEEK`), ohne die Listen
  neu zu laden; offline lernt er die Kopfdaten. Bereits gelernte Mails werden übersprungen.
- **Gelöschte Mails** nehmen ihren Lerneintrag mit (Fremdschlüssel), das Gelernte bleibt in den
  Zählern. Über 400.000 Tokens fallen die weg, die nur eine Mail hatte.

### Neue Mails

- Wo heute die blockierten Absender greifen (neue Mail in einem Posteingang, nie beim ersten
  Abgleich, `handle_arrival`): erst blockierte Absender wie bisher, dann der Filter, dann Regeln
  und Mitteilung. Die Kategorie entsteht schon beim Speichern und bleibt unberührt.
- **Nie Spam:** eigene Adressen (Konten und Identitäten), Kontakte, denen die Person geschrieben
  hat oder die sie benannt, notiert, zusammengeführt oder in eine Gruppe gesteckt hat (jede Adresse
  ist seit 0050 Kontakt, daher nicht „irgendein Kontakt“), Antworten auf eigene Mails (In-Reply-To
  oder References nennt eine Mail von einer eigenen Adresse), Absender, deren Bilder die Person
  immer laden lässt, und als wichtig markierte Absender, Domains oder Listen.
- **Schon gelernte Mails urteilt er nicht noch einmal:** „Kein Spam“ oder ein rückgängig gemachtes
  Verschieben bringt die Mail als neue Mail in den Posteingang; vorher wartet der Filter, bis das
  dafür eingereihte Lernen erledigt ist (der Server-Test hat diesen Fall gefunden). Jede
  Verschiebung des Filters hat eine eigene Anfrage-ID.
- Der Text wird beim Eintreffen geholt, wenn er nicht gespeichert ist (wie beim Öffnen, lesend);
  scheitert das, entscheiden die Kopfdaten.
- **Ab 0,99:** Darf das Konto schreiben, gehen alle Spam-Mails eines Fensters als ein Stapel über
  die Outbox in den Spam-Ordner (`MailAction::Spam`); fehlt der Ordner oder darf das Konto nicht
  schreiben, werden sie nur auf diesem Gerät als „Spam-Verdacht“ markiert. In beiden Fällen keine
  Regeln, keine Mitteilung; das Ereignis `spamFiltered` (Konto, verschoben, markiert, Stapel)
  zeigt einen Hinweis mit „Rückgängig“.
- **Urteil sichtbar** (`spam_verdict`): Listenzeilen tragen `spam` (`suspected` immer, `moved` nur
  im Spam-Ordner), der Lesebereich zeigt eine Notiz mit dem Wert („zu 99,7 % sicher“) und „Kein
  Spam“ (verschoben: zurück über die Outbox; Verdacht: Markierung weg, als Ham gelernt) und, wo
  möglich, „In den Spam-Ordner“. Für Konten ohne Schreibzugriff gibt es dafür
  `markSpamLocally` (lernt und markiert nur lokal, „Rückgängig“ markiert wieder).

### Einstellung

- `spam.filter` = `{ enabled, excludedAccounts }`, **global mit Ausnahmen je Konto**, Standard
  aus, synchronisiert wie andere Einstellungen. Einfacher als je Konto ein Schalter, und neue
  Konten sind gleich dabei. Was gelernt wurde, ist Klasse **Device** und reist nie (es wäre groß
  und enthält Hinweise auf Mails); ein zweites Gerät lernt selbst (oder per Knopf aus den
  Ordnern) und handelt erst nach den Mindestzahlen.
- Beschriftet: „läuft nur, solange die App läuft, auch im Hintergrund“, „bleibt auf diesem
  Gerät“. Ausschalten hält ein laufendes Lernen an und behält das Gelernte; „Gelerntes
  vergessen …“ (mit Rückfrage) leert alle vier Tabellen.

## Messung

Siehe `docs/testing/spam-filter.md`: Trefferquote im SpamAssassin-Korpus, Lernkurve, Kosten je
Mail, Listen. Kurz: 0,05 % falsch als Spam, 8,8 % übersehen (zehnfach kreuzvalidiert, 0,99);
Zerlegen einer Mail im Debug-Build Median 0,6–1,0 ms, Nachschlagen von 200 Tokens 0,6 ms;
Server-Test: Spam nach 243–286 ms im Spam-Ordner.

## Erwogen, nicht gewählt

- **Graham („A Plan for Spam“) mit 15 Tokens:** urteilt zu hart, Werte nahe 0 oder 1 auch bei
  wenig Beleg; Fisher kennt „unsicher“.
- **Tokens im Klartext** (wie Thunderbirds `training.dat`): erklärbarer, aber Wörter aus Mails in
  der Datenbank und mehr Platz; die Notiz nennt deshalb nur den Wert, nicht die Gründe.
- **Bigramme** (SpamBayes experimentell): mehr Speicher und Zeit, im Korpus nicht nötig.
- **Lernen bei jedem Verschieben in den Spam-Ordner:** auch Regeln und Ziehen würden lehren,
  die Person meint damit nicht immer „Spam“.
- **Lernen ohne Schwelle für Ham („alles Gelesene ist Ham“):** gelesener Spam würde den Filter
  vergiften; deshalb nur aufgefüllt und mit Zweifel ab 0,9.
- **Text vor dem Lernen von „Spam“ holen:** verzögerte die Aktion um eine Netzrunde; danach hat
  die Mail bis zum Abgleich keinen Ort mehr.
- **Synchronisieren des Modells:** groß, ändert sich ständig, und zwei Geräte lernen
  Unterschiedliches; die Einstellung reicht.
- **Neue Abhängigkeit** (z. B. eine Bayes-Bibliothek): das Verfahren ist wenige hundert Zeilen,
  ohne Pflegerisiko.

## Offen und ungeprüft

- Nur am SpamAssassin-Korpus von 2002–2005 gemessen (englisch, alte Muster); an echten deutschen
  Postfächern nicht. Chinesisch, Japanisch und Thai ohne Leerzeichen zerfallen nicht in Wörter
  (nur `skip:`-Tokens); Thunderbird zerlegt solche Texte eigens.
- Die Regeln „nie Spam“ sind im Korpus nicht nachgebildet; im Alltag senken sie Fehlalarme weiter.
- Ein zweites Gerät mit eingeschaltetem Filter verschiebt dieselbe neue Mail ebenfalls; die zweite
  Verschiebung findet sie nicht mehr (Konflikt „nicht mehr da“ in der Aktivität), wie bei
  blockierten Absendern.
- Windows: nicht auf dem Gerät geprüft (keine plattformeigene Logik).
