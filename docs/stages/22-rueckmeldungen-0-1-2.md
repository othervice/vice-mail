# Stufe 22 — Rückmeldungen zu 0.1.2

**Ziel:** Alles, was der Mensch beim Test von 0.1.2 findet, gesammelt umsetzen und in einem Zug
bauen.

**Anlass:** Test von 0.1.2 am 26.11.2026. Wie in Stufe 21: erst alle Rückmeldungen sammeln,
gebaut wird erst, wenn der Mensch sagt, dass die Liste vollständig ist.

**Stand:** Fertig am 26.11.2026, ergibt 0.1.3. Liste vollständig („das reicht dann auch erstmal …
los bau 0.1.3“), danach kamen noch Punkt 11 und 12 dazu.
Ergibt 0.1.3. Aufteilung: 4+10 (Geheimnisse), 6 (Spamfilter), 5+7 (Suche), 3+8 (Oberfläche),
danach 1+2 (Konten) und 9 (Windows); je ein Subagent in eigenem Worktree.

## Rückmeldungen

### 1 — Konten in der linken Spalte verschieben

- Gewünscht: Die Reihenfolge der Konten links selbst festlegen. Heute stehen sie in der
  Reihenfolge, in der sie angelegt wurden (`ORDER BY created_at, id` in
  `crates/mail-store/src/accounts.rs`); eine Reihenfolge gibt es bisher nur für Ordnerfavoriten
  und Regeln.
- Plan: Konten in der linken Spalte per Ziehen verschieben, dazu „Nach oben“/„Nach unten“ im Menü
  des Kontos und in Einstellungen → Konten (Tastatur und Bildschirmleser). Neue Spalte für die
  Position am Konto, Migration übernimmt die heutige Reihenfolge. Dieselbe Reihenfolge überall,
  wo Konten aufgezählt werden: linke Spalte, schmale Leiste, Einstellungen, Kontowahl im
  Composer, „Alle Posteingänge“-Auswahl. Läuft über die Einstellungs-Synchronisierung mit.

### 2 — Anzeigenamen eines Kontos bearbeiten

- Gewünscht: Den Namen, unter dem ein Konto links steht, ändern können.
- Stand im Code: `account.display_name` wird beim Anlegen gesetzt und ist zugleich der
  Absendername der ersten Identität; es gibt keinen Befehl, ihn zu ändern.
- Plan: Eigener Name für die Liste („Name in der Liste“), Standard die Adresse, änderbar per
  Doppelklick bzw. „Umbenennen“ im Menü des Kontos und in Einstellungen → Konten. Der
  Absendername bleibt getrennt bei den Identitäten, damit ein Listenname wie „Arbeit“ nicht in
  gesendeten Mails landet. Leerer Name heißt: Adresse anzeigen. Synchronisiert.

### 3 — Keine Bilder und Initialen mehr

- Gewünscht: Die runden Bilder mit Initialen überall entfernen; eigene Bilder hinterlegt ohnehin
  niemand, sie kosten nur Platz. Kontakte bleiben, nur ohne Bilder. „Alles mit Bildern raus aus
  dem Client.“
- Gemeint sind Absender-, Kontakt- und Kontobilder. Bilder in Mails und Anhänge bleiben.
- Heute zu sehen in: Mail-Liste und Tabellen-/Kompaktzeilen (`MessageRow`, `InboxListRows`),
  Lesebereich (`MessageHeader`), Composer-Empfänger (`RecipientField`), Kontakte (Liste, Detail,
  Gruppen), Popup für neue Mails, schmale Leiste (`IconRail`), Einstellungen → Konten.
- Plan:
  - `Avatar` und die Kontaktbilder aus Stufe 17 entfernen (Auswahl, Zuschnitt, Speicher,
    Synchronisierung, Dekodieren im Hilfsprozess). Gespeicherte Bilder löscht eine Migration;
    vCard-Import übernimmt kein `PHOTO` mehr, der Export schreibt keins. Entscheidung festhalten,
    sie ersetzt den Bild-Teil von Stufe 17.
  - Was der Ring trug, bleibt erhalten: In Listen mit mehreren Konten zeigt ein kleiner Punkt in
    der Kontofarbe, zu welchem Konto die Mail gehört.
  - Ungelesen-Punkt und Auswahlkästchen bekommen eine schmale feste Spalte am Zeilenanfang (das
    Kästchen erscheint beim Überfahren dort), ohne die leere Spalte, die in Stufe 21 Punkt 4
    entfernt wurde.
  - Schmale Leiste: Konten als Punkt in der Kontofarbe mit dem Namen als Tooltip.
  - Abstands-Test aus Stufe 21 (0071) auf die neuen Zeilen anpassen.

Punkte 4 bis 9: vom Menschen am 26.11. aus den Vorschlägen gewählt (nicht gewählt: „Nur bei
Personen melden“, „Ziehen in Finder/Explorer“).

### 4 — Ein Schlüsselbund-Eintrag auf macOS

- Anlass: Nach jedem Update fragt macOS 5–6 Mal nach dem Schlüsselbund, einmal je Eintrag
  (ad-hoc-Signatur, die Zugriffsliste hängt am cdhash). Endgültig behebt das erst die Signatur
  mit Developer ID (Stufe 15).
- Plan: Auf macOS liegen alle Geheimnisse von Vice Mail in einem einzigen Eintrag (Dienst
  `org.vicemail.desktop`), darin eine Tabelle Name → Geheimnis für Konten, OAuth-Tokens,
  MCP-Clients, KI-Profile und den Sync-Schlüssel. Dann gibt es nach einem Update eine Abfrage statt
  einer je Eintrag. Beim ersten Start übernimmt die App die alten Einträge (dabei fragt macOS ein
  letztes Mal je Eintrag), schreibt den Sammel-Eintrag und löscht die alten erst danach.
- Schreiben nur aus einem Prozess mit Sperre, damit gleichzeitige Änderungen nichts überschreiben;
  bricht die Übernahme ab, bleiben die alten Einträge gültig.
- Windows bleibt bei einem Eintrag je Geheimnis: Die Anmeldeinformationsverwaltung fragt nicht,
  und ein Eintrag fasst dort höchstens 2.560 Bytes.
- Entscheidung festhalten; `docs/SECURITY_MODEL.md` und `docs/PRIVACY.md` anpassen. Tests nur
  gegen den Testdienst `org.vicemail.desktop.e2e`.

### 5 — Kontakte in der Suche

- Plan: Beim Tippen in die Suche stehen oben bis zu drei passende Kontakte (Name, Adresse oder
  Domain, auch per Anfang). Auswahl zeigt alle Mails von und an diese Person über alle Konten
  (Absender, Empfänger oder CC, alle Adressen des Kontakts). Mit Tastatur bedienbar; die normale
  Volltextsuche darunter bleibt.

### 6 — Lernender Spamfilter (lokal)

- Plan: Ein Filter, der lokal aus „Spam“ und „Kein Spam“ lernt, wie bei Thunderbird; ohne Server,
  ohne neue Abhängigkeit. Er wertet Kopfzeilen und Text der Mails aus.
- Erkannte Mails gehen über die Outbox in den Spam-Ordner, mit Rückgängig. Bei nur lesenden
  Konten oder ohne Spam-Ordner werden sie nur als „Spam-Verdacht“ markiert.
- Bekannte Kontakte, eigene Adressen und Antworten in eigenen Gesprächen gelten nie als Spam.
  Blockierte Absender bleiben wie bisher.
- Standard aus. Beim Einschalten kann er aus dem vorhandenen Spam-Ordner lernen. Läuft nur,
  solange die App läuft, und ist so beschriftet.
- Größter Punkt der Liste. Die Trefferquote wird mit einem Testkorpus gemessen, und die Zahlen
  werden hier eingetragen.

### 7 — Suche mitten im Wort

- Plan: Zusätzlich zum Wortindex ein Index aus Dreiergruppen von Zeichen (FTS5 `trigram`), mit
  dem `uster` auch „Muster“ findet und `1234` eine Nummer mitten im Wort.
  - Sicher für Absender, Empfänger, Betreff und Dateinamen.
  - Für den Mail-Text erst nach Messung von Größe und Tempo bei 100.000 Mails; das Ziel bleibt
    unter 200 ms bis zu den ersten Treffern.
- Suchbegriffe mit weniger als drei Zeichen nutzen weiter den Wortindex. Der neue Index wird
  im Hintergrund aufgebaut wie der bestehende.

### 8 — Einstellungen durchsuchen

- Plan: Suchfeld oben in den Einstellungen. Es findet jede Option über Titel und Beschreibung in
  der eingestellten Sprache (z. B. „Signatur“, „Ton“), springt dorthin und hebt die Option kurz
  hervor. Mit Tastatur bedienbar (Cmd/Ctrl+F in den Einstellungen).

### 9 — Windows-11-Optik

- Plan: Fensterhintergrund mit Mica wie bei Windows-11-Apps, Titelleiste in den Farben der App,
  beim Überfahren des Maximieren-Knopfs die Andock-Leiste von Windows.
- Auf dem Mac nicht prüfbar. Die CI prüft auf Windows nur den Build; wie es aussieht, muss der
  Mensch auf dem PC ansehen.

### 10 — Passwörter von einem anderen Gerät übernehmen

- Gewünscht (26.11.): Nach dem Beitreten zum Einstellungs-Sync die Passwörter nicht pro Konto neu
  eintippen müssen. Heute starten übernommene Konten mit „Passwort fehlt“ (`docs/SYNC.md`,
  „Accounts taken over“).
- Plan:
  - Der neue Rechner erzeugt ein Schlüsselpaar nur für diese Übernahme. Den öffentlichen Teil
    legt er als Anfrage in den Sync-Ordner. Die Anfrage läuft nach 15 Minuten ab.
  - Beide Geräte zeigen denselben kurzen Code. Auf einem eingerichteten Gerät bestätigt der
    Mensch, dass die Codes gleich sind, und wählt die Konten.
  - Erst dann verschlüsselt das alte Gerät die Passwörter für genau diesen Schlüssel. Der neue
    Rechner holt sie ab, legt sie in den Schlüsselbund und löscht Anfrage und Antwort aus dem
    Ordner.
- Nur für Konten, deren Server auf beiden Geräten genau gleich eingetragen sind (Grenze aus
  Entscheidung 0040). Ein Passwort geht also nie an einen Server, den nur der Ordner nennt.
- Schreibzugriff bleibt auf dem neuen Gerät aus. OAuth-Konten melden sich auf jedem Gerät selbst
  an.
- Der private Schlüssel liegt nur im Speicher, ein Neustart verwirft die Anfrage. Die Passwörter
  liegen nie dauerhaft im Postfach.
- Entscheidung festhalten; `docs/SYNC.md`, `docs/SECURITY_MODEL.md` und `docs/PRIVACY.md`
  anpassen.

### 11 — Windows: Taskleisten-Symbol noch orange, Zahl am Symbol pixelig

- Gemeldet (26.11., nach dem Start des Baus): Unter Windows ist das Taskleisten-Symbol weiterhin
  orange statt blau. Die Zahl am Symbol bei neuen Mails ist noch pixelig, sie soll „super
  hochauflösend“ sein.
- Geprüft:
  - `mail-app.exe` und der Installer von 0.1.2 enthalten das blaue Symbol (die PNG-Bilder aus
    `icons/icon.ico` von v0.1.2 stecken unverändert darin, die von v0.1.1 nicht).
  - Orange heißt also entweder, dass auf dem PC noch 0.1.1 läuft, oder dass Windows das alte
    Symbol der angehefteten Verknüpfung aus seinem Symbol-Zwischenspeicher zeigt. Der Installer
    meldet der Shell nach dem Installieren keine Änderung (`installer-hooks.nsh`).
- Die Zahl zeichnet `app/src-tauri/src/badge.rs` (Stufe 21, Entscheidung 0068) mit zwei
  Bitmap-Schriften (3×5 und 5×7), nur ganzzahlig vergrößert. Die Kanten sind scharf, die Ziffern
  aber bewusst klötzchenhaft. Das ist das „pixelig“.
- Plan:
  - Ziffern als echte Schrift mit Kantenglättung in genau der Pixelgröße zeichnen, die Windows
    zeigt, für das Taskleisten-Overlay und das Symbol im Infobereich: die Systemschrift (Segoe
    UI) oder eine mitgelieferte, lizenzverträgliche Schrift, ohne `unsafe`.
  - Bilder bei 100, 125, 150 und 200 % als Testbilder ablegen und ansehen.
  - Der Installer lässt Windows nach Installation und Update die Symbole neu laden
    (`SHChangeNotify`), damit angeheftete Verknüpfungen das neue Symbol zeigen.
- Wartet auf: welche Version auf dem PC läuft (Einstellungen → Über).

### 12 — Mails per Ziehen in Ordner verschieben, auch aus „Alle Posteingänge“ in andere Konten

- Gewünscht (26.11.): Mails per Drag-and-drop in einen Ordner verschieben, z. B. aus „Alle
  Posteingänge“ in einen Ordner eines anderen Postfachs.
- Stand im Code:
  - Ziehen geht nur in der Liste eines einzelnen Ordners (`MailView.tsx`, `onDragRows`). Die
    Sitzung in `app/src/stores/drag.ts` kennt genau ein Konto und einen Quellordner.
  - In gemischten Listen gibt es kein Ziehen: „Alle Posteingänge“, Ansichten und Suchergebnisse.
  - Beim Ablegen in einem anderen Konto fragt die App nach „Verschieben oder Kopieren“
    (`moveOrAsk`, Entscheidung 0033). Das Verschieben selbst läuft über `append` im Zielkonto
    und danach über `expunge` in der Quelle.
- Plan:
  - Ziehen aus jeder Liste, auch mit Mails aus mehreren Konten und Ordnern, auf jeden Ordner
    links: Favoriten, Ordner aller Konten, schmale Leiste.
  - Ablegen verschiebt, mit Rückgängig und sichtbarem Zustand. Mit gedrückter Wahltaste (Mac)
    bzw. Strg (Windows) wird kopiert. Die Frage aus 0033 entfällt beim Ziehen, das Ziehen ist
    die Antwort. Im Menü „Verschieben nach“ bleibt sie.
  - Mails, die schon im Zielordner liegen, bleiben unberührt. Nur lesende Konten sind als Quelle
    zum Verschieben gesperrt, ebenso als Ziel. Der Zielordner zeigt beim Überfahren, warum.
  - Ordner klappen beim Darüberhalten auf. Die Liste scrollt am Rand mit.
  - Zieh-Bild mit der Anzahl.
- Gehört zum Strang „Konten“ (Punkte 1 und 2), der die linke Spalte ebenfalls zum Ziehen
  umbaut.

## Ergebnis

Erledigt am 26.11.2026: alle zwölf Punkte, ergibt 0.1.3. Sechs Subagenten in eigenen Worktrees
(Suche 5+7, Oberfläche 3+8, Geheimnisse 4+10, Spamfilter 6, Windows 9+11, Konten 1+2+12),
zusammengeführt auf `main`. Entscheidungen 0072 (keine Bilder), 0073 (Einstellungen
durchsuchen), 0074 (Kontakte in der Suche, Suche im Wort), 0075 (ein Schlüsselbund-Eintrag),
0076 (Passwörter übernehmen), 0077 (Spamfilter), 0078 (Reihenfolge und Namen der Konten), 0079
(Windows-11-Optik), 0080 (Ziffern der Zahl am Symbol), 0081 (Ziehen aus jeder Liste). Migrationen
0028 (keine Kontaktbilder), 0029 (Trigramm-Index), 0031 (Spamfilter), 0032 (Reihenfolge der
Konten); 0030 blieb frei. Neue Abhängigkeit: keine (skrifa war schon über hayro im Build); neue
Schrift: Teilmenge von Selawik Semibold (OFL) als „Badge Digits“.

- **1:** Spalte `account.position` (leer bis zur ersten Änderung, die bisherige Reihenfolge
  bleibt). Ziehen der Kontoüberschrift links, „Nach oben/unten“ im Kontomenü, Alt+Pfeil, Pfeile
  in Einstellungen → Konten. Überall dieselbe Reihenfolge; im Sync als Feld `position` (jede
  Änderung vergibt allen Konten neue, höhere Positionen, damit die neuere Reihenfolge als Ganzes
  gewinnt).
- **2:** `account.display_name` ist jetzt der Name in den Listen, Doppelklick, F2 oder
  „Umbenennen“; leer zeigt die Adresse. Nichts Ausgehendes liest ihn mehr (KI-Antworten nehmen den
  Absendernamen der Identität). Neue Konten starten ohne Listennamen.
- **3:** Alle Bilder und Initialen entfernt (Liste, Lesebereich, Composer, Kontakte, Popup,
  schmale Leiste, Einstellungen), dazu die Kontaktbilder aus Stufe 17 samt Befehlen, Hilfsprozess-
  Modus und Sync-Art (alte Datensätze anderer Geräte werden still ignoriert). Zeilen beginnen mit
  einer 16-px-Spalte für Ungelesen-Punkt bzw. Kästchen. In gemischten Listen bleibt die
  Kontobeschriftung (Punkt und Name) auch beim Überfahren sichtbar (ein Punkt vor dem Absender war
  vom Ungelesen-Punkt nicht zu unterscheiden). Schmale Leiste: Konten als Farbpunkte mit Namen.
- **4:** Auf macOS ein Tresor-Eintrag in Generationen (`vault`, `vault.2`, …: neu schreiben,
  zurücklesen, dann ältere löschen). Übernahme der alten Einträge beim Start nach den Namen aus der
  Datenbank, Rückfall auf alte Einträge beim Lesen. Windows unverändert (ein Eintrag je
  Geheimnis).
- **5:** Bis zu drei Kontakte oben in der Suche (Name, Adresse, Domain, auch per Anfang); Auswahl
  setzt den neuen Filter `contact:`/`kontakt:` (alle Adressen des Kontakts in Von/An/Cc, über alle
  Konten).
- **6:** Robinson/Fisher wie SpamBayes, Tokens als 64-Bit-Hashes (keine Wörter aus Mails in der
  Datenbank), Klasse Gerät, nur der Schalter wird abgeglichen. Greift ab 0,99 und erst nach 50
  Spam- und 200 anderen Mails. Nie Spam: eigene Adressen, angeschriebene Kontakte, Antworten auf
  eigene Mails, Absender mit erlaubten Bildern, wichtige Absender. Gemessen am
  SpamAssassin-Korpus (10-fach Kreuzvalidierung): 0,05 % falsch als Spam, 8,8 % Spam verpasst
  (`docs/testing/spam-filter.md`).
- **7:** Zweiter FTS5-Index `message_fts_tri` (Trigramme) über alle Spalten einschließlich Text.
  Gemessen bei 100.000 Mails: Datenbank 119 → 272 MB, langsamste erste Treffer 20,7 → 29 ms
  (Ziel 200 ms), Speichern von Bodies 2.213 → 916 je Sekunde. Ab 3 Zeichen findet ein Begriff
  ohne Anführungszeichen auch Teile von Wörtern.
- **8:** Suchfeld über der Abschnittsliste der Einstellungen (Cmd/Ctrl+F), Index aus denselben
  i18n-Schlüsseln, die die Zeilen zeigen; ein Test rendert jeden Abschnitt und meldet jede Zeile,
  die im Index fehlt.
- **9:** Mica in der Titelleiste aller Fenster (Windows 11), im Hauptfenster auch hinter Leiste
  und Ordnerspalte; Liste, Lesebereich, Composer, Menüs bleiben deckend. Rückfall ohne
  Transparenz, vor Build 22000 und mit Kontrastdesign. Native Fenstertasten (Andock-Leiste von
  Windows). Kontrasttest gegen hellstes/dunkelstes Mica.
- **10:** Anfrage mit Festlegung auf den Schlüssel → Herausforderung mit Zufallswert vom
  gebenden Gerät → Schlüssel offenlegen → sechsstelliger Code aus allen drei (Abweichung vom Plan:
  ein Code nur aus Schlüssel und Anfrage ließe sich mit etwa einer Million Versuchen erzwingen).
  Nur Konten mit gleichen Servern auf beiden Geräten (0040), keine OAuth-Tokens, Schreiben bleibt
  aus, privater Schlüssel nur im Speicher, Einträge laufen nach 15 Minuten ab.
- **11:** Geprüft: `mail-app.exe` und Installer von 0.1.2 enthalten das blaue Symbol. Ziffern
  jetzt aus Selawik Semibold mit Hinting in genau der Pixelgröße (eigener Scanline-Rasterizer,
  gleiche Pixel auf jeder Plattform), Vorschaubilder für 100–200 % angesehen. Der Installer
  meldet Explorer nach Installation und Update das geänderte Symbol (`SHChangeNotify`, auch für
  angeheftete Verknüpfungen); mit makensis 3.13 in einem kleinen Rahmen übersetzt, nicht in der
  Vorlage von Tauri.
- **12:** Ein Befehl `dropMessages` gruppiert die Zeilen: Zeilen des Zielkontos werden
  verschoben, andere Konten über Kopieren-dann-Entfernen (0033), Zeilen schon im Ziel bleiben.
  Ablegen verschiebt ohne Frage, Wahltaste/Strg kopiert; „Verschieben nach“ im Menü fragt weiter.
  Nur lesende Konten lehnen beim Überfahren mit Begründung ab. Ordner klappen nach 700 ms auf,
  die Spalte scrollt am Rand. Rückgängig nimmt Verschiebungen innerhalb eines Konto zurück; über
  Konten nur, solange die Kopie noch nicht auf dem Server ist (sagt die Meldung).

**Geprüft:**
- `scripts/check.sh` grün auf `fc10011` (alle zwölf Punkte zusammengeführt): 1.665 Rust-Tests,
  4.707 Vitest, Clippy für macOS und Windows, Lizenzen, Advisories.
- GitHub-CI auf `ed2d186` (Version 0.1.3): `check` grün auf macOS und auf echtem Windows.
- Server-Tests gegen den lokalen Testserver auf `ed2d186`: 1.777 grün.
- Desktop: 43 Spec-Dateien (214 Tests) in drei Gesamtläufen je 42/43; jede Spec-Datei war in
  mindestens einem Gesamtlauf grün (Ausreißer unten). `stage22-ui` und `spacing` danach einzeln
  grün, `spacing` zweimal.
- README-Bilder neu erzeugt (ohne Bilder und Initialen).
- Vorschaubilder der Zahl am Symbol (100–200 %) und Bildschirmfotos aller Stränge angesehen.

**Ungeprüft:**
- Alles auf einem echten Windows-PC: Mica, Ziffern der Zahl am Symbol, Symbol nach dem Update,
  Strg zum Kopieren (Prüfliste 22.1–22.4 in `docs/testing/windows-checklist.md`).
- Wie oft macOS bei einem echten Update fragt (0.1.2 → 0.1.3 und danach), und ob ein neuer Build
  Tresor-Einträge eines älteren löschen darf.
- Passwörter übernehmen zwischen zwei echten Geräten; Sync mit einem Gerät auf 0.1.2.
- Spamfilter mit echten, deutschsprachigen Postfächern; Release-Build-Zeiten.
- Die echte Zieh-Geste des Systems (die Desktop-Tests lösen die Drag-Ereignisse in der Seite aus).
- Ein echtes Update über die App.
- Wackelige Tests:
  - `comfort::tests::popup::new_mail_shows_as_popup_cards_by_the_same_rules` scheiterte einmal
    unter hoher Last (danach 7 von 7 grün).
  - `spacing.e2e.mjs` fand im dritten Gesamtlauf bei 900×600 das Gespräch nicht aufgeklappt
    (nach einem Neustart des WebDrivers); einzeln zweimal grün.
- Behoben während der Prüfung:
  - `stage22-ui` fand in beiden ersten Gesamtläufen Bobs Mail in „Alle Posteingänge“ nicht:
    frühere Specs legen kurz vorher neuere Mails ab, die Mail fiel aus den zwölf geprüften
    Zeilen. Sie ist jetzt die neueste (`18f321d`).
  - Der Server-Test des Spamfilters ließ Mails im Junk des Testservers liegen und brachte damit
    die Zählung des IMAP-Ordnertests durcheinander (`a39af7d`).

**Wartet auf:** welche Version auf dem Windows-PC lief (Punkt 11); Test von 0.1.3 auf Mac und
Windows, zuerst ein Update über die App; Prüfliste 22.1–22.4 auf Windows.
