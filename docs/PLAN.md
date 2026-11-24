# Plan

> **English note:** this is the German project plan, kept as a project log. See [README.md](README.md) for how the documentation is organised.

Sechzehn Stufen, nacheinander. Jede Stufe ist ein abgeschlossener Auftrag für eine Sitzung, mit
Aufgaben, überprüfbarer Abnahme und dem, was ausdrücklich nicht dazugehört.

## Eine Stufe starten

Im Projektordner eine neue Sitzung öffnen und schreiben:

> Führe die nächste offene Stufe aus `docs/PLAN.md` aus.

`CLAUDE.md` wird automatisch geladen und enthält die Arbeitsregeln. Nach jeder Stufe steht unter
„Ergebnis“ in der Stufendatei, was gebaut und geprüft wurde und was auf den Menschen wartet.

## Stufen

| Nr. | Stufe | Status | Danach |
|---|---|---|---|
| 00 | [Umgebung und Risiko-Spikes](stages/00-umgebung-und-spikes.md) | fertig | Werkzeuge stehen, die vier riskanten Annahmen sind geprüft |
| 01 | [Projektgerüst und Design-Grundlage](stages/01-projektgeruest.md) | fertig | App startet mit leerer Oberfläche im Zieldesign |
| 02 | [Konto und Synchronisierung, nur lesend](stages/02-konto-und-sync-lesend.md) | fertig | echte Postfächer erscheinen als Liste |
| 03 | [Nachricht lesen](stages/03-nachricht-lesen.md) | fertig | Mails und Anhänge sicher lesbar, auch offline |
| 04 | [Aktionen und Outbox](stages/04-aktionen-und-outbox.md) | fertig | archivieren, löschen, verschieben, markieren |
| 05 | [Verfassen und Senden](stages/05-verfassen-und-senden.md) | fertig | **erste täglich nutzbare Version auf dem Mac** |
| 06 | [Windows und Builds](stages/06-windows-und-builds.md) | wartet auf Windows-Test | **installierbar auf Windows und MacBook** |
| 07 | [Suche, Threads, mehrere Konten](stages/07-suche-threads-konten.md) | fertig | vereinter Posteingang, schnelle Suche |
| 08 | [Bedienung und Barrierefreiheit](stages/08-bedienung-und-barrierefreiheit.md) | fertig | Layouts, Tastatur, Command Palette |
| 09 | [Posteingang-Komfort](stages/09-posteingang-komfort.md) | fertig | Kategorien, Snooze, Benachrichtigungen, Standard-Mail-App |
| 10 | [Composer vollständig](stages/10-composer-vollstaendig.md) | fertig | Signaturen, Vorlagen, Kontakte, Später senden |
| 11 | [Einstellungs-Sync](stages/11-einstellungs-sync.md) | fertig | **Geräte gleichen sich über das Postfach ab** |
| 12 | [KI über MCP](stages/12-ki-ueber-mcp.md) | fertig | eigene KI (Claude, Cursor, LM Studio …) per MCP anschließen |
| 13 | [KI in der App (optional)](stages/13-ki-in-der-app.md) | fertig | Zusammenfassen, Vorschläge, Schreibhilfe, Fragen mit Quellen |
| 14 | [Weitere Anbieter](stages/14-weitere-anbieter.md) | wartet auf Mensch (Code fertig: A–D; B/C brauchen App-Registrierungen und Handprüfungen) | Microsoft, Gmail-Eigenheiten, JMAP |
| 16 | [Funktionsabgleich](stages/16-funktionsabgleich.md) | fertig | Lese-Liste, Ordnerfavoriten, Verschieben zwischen Konten, Schnellantwort, Lesebestätigung (0032) |
| 17 | [Gespräche und Kontakte](stages/17-gespraeche-und-kontakte.md) | fertig | Gespräche aufklappen wie in Thunderbird, Kontakte mit Bildern, eigene Ansichten; vor dem ersten öffentlichen Release |
| 18 | [Spalten und Popup für neue Mails](stages/18-spalten-und-neue-mail-popup.md) | in Arbeit | Grenze Liste/Lesebereich ziehen, Aufklappen trotz Schnellaktionen, Popup mit Lesen/Löschen, Ordner anlegen, Hell mit Blau und Dunkel wie GitHub |
| 19 | [Thunderbird-Funktionen](stages/19-thunderbird-funktionen.md) | offen, nach 18 | Sortieren, Tabellenansicht, Schnellfilter, Quelltext, Umleiten, Als neu, Kontaktgruppen, vCard/mbox, Übernahme aus Thunderbird |
| 15 | [Veröffentlichung](stages/15-veroeffentlichung.md) | wartet auf Mensch (alles ohne Veröffentlichung fertig) | öffentliches Repo, signierte Builds, Updates |

Status: `offen`, `in Arbeit`, `wartet auf Mensch`, `fertig`.

## Reihenfolge

Stufen 00 bis 06 sind der kritische Pfad und bauen streng aufeinander auf. Danach ist die Reihenfolge
weniger fest: 07 bis 10 lassen sich tauschen, 11 braucht 09 (Snooze, Pins) und 10 (Signaturen,
Vorlagen), 12 braucht 05 und 07, 13 braucht 07 und 12. Stufe 16 (Funktionsabgleich, 0032) und Stufe 14 (weitere Anbieter, seit 0032
geplant) kommen vor Stufe 15; Kalender und Adressbuch-Abgleich folgen nach Version 1.

## Was der Mensch beisteuert

| Wann | Was |
|---|---|
| ~~vor Stufe 1~~ | ~~Lizenz und Bundle-ID~~: erledigt, siehe `docs/SPEC.md`, Entscheidungen |
| nach Stufe 0 | den erzeugten Windows-Installer einmal auf dem Windows-Rechner starten |
| nach Stufe 2 | ein echtes Konto in der App einrichten (bleibt nur lesend) und Vollständigkeit prüfen |
| nach Stufe 4 | entscheiden, für welches echte Konto Schreibzugriff freigeschaltet wird |
| vor Stufe 6 | Zugang zum Windows-Rechner klären (SSH vom Mac mini oder Handprüfung nach Checkliste) |
| vor Stufe 12 | einen MCP-fähigen KI-Client zum Testen (z. B. Claude Desktop oder Claude Code) |
| vor Stufe 13 | Ollama oder einen API-Schlüssel, falls KI in der App gewünscht |
| vor Stufe 15 | Apple-Developer-Konto und Windows-Signaturzertifikat, falls signierte Builds gewünscht |

## Mehrere Stufen am Stück

Gilt, wenn der Auftrag lautet, die Stufen nacheinander ohne Begleitung abzuarbeiten (z. B. über Nacht).

**Ablauf**
- Die Hauptsitzung leitet: Sie liest die Stufendatei, plant, verteilt, prüft, committet und pusht.
  Umfangreiche Umsetzungsarbeit gibt sie an Subagenten ab, damit ihr eigener Kontext für viele Stufen reicht.
- Nach jeder Stufe direkt mit der nächsten weitermachen. Nicht zwischen den Stufen auf Rückmeldung warten.
- Die Hauptsitzung nimmt keinen Bericht ungeprüft hin: Sie führt `scripts/check.sh` und die Tests
  der Abnahme selbst aus, bevor sie eine Stufe als fertig einträgt.

**Parallel arbeiten**
- Unabhängige Aufgaben einer Stufe laufen als parallele Subagenten, z. B. in Stufe 0 die Spikes
  B und C neben dem Aufbau des Testservers, danach A und D. Voneinander abhängige Aufgaben laufen nacheinander.
- Parallele Subagenten arbeiten in getrennten Verzeichnissen oder mit `isolation: "worktree"`.
  Sie committen nicht; das macht die Hauptsitzung nach der Prüfung.
- Jeder Subagent bekommt im Auftrag: die betroffene Aufgabe wörtlich, die Pfade zu `CLAUDE.md`,
  `docs/ARCHITECTURE.md` und der Stufendatei, was schon feststeht, und was er zurückmelden soll
  (geänderte Dateien, ausgeführte Befehle mit Ergebnis, offene Punkte).
- Nicht mehr als vier Subagenten gleichzeitig.

**Wenn etwas hakt**
- Punkte unter „Der Mensch prüft danach“ blockieren nicht. Sie kommen unter „Ergebnis → Wartet auf“.
- Fehlt eine Entscheidung, die sich mit einer begründeten Annahme treffen lässt: Annahme treffen, als
  Entscheidung festhalten, weitermachen, und den Menschen per Benachrichtigung informieren.
- Nur wenn auf dem kritischen Pfad (Stufen 0 bis 6) wirklich nichts mehr ohne den Menschen geht:
  alles Mögliche fertigstellen, dann den Menschen fragen (Benachrichtigung und Frage) und warten.
  Ab Stufe 7 lieber eine blockierte Stufe parken und eine unabhängige vorziehen (siehe Reihenfolge).
- Stufe 14 nur, wenn der Mensch sie anfordert. Stufe 15 erst nach seinen Entscheidungen dort.

**Den Menschen informieren**
- Am Ende jeder Stufe eine kurze Benachrichtigung (`PushNotification`): Stufe, Ergebnis in einem Satz, ob etwas wartet.
- Installer für Tests (ab Stufe 0 der Spike, ab Stufe 6 die App) als Anhang eines Vorab-Releases im
  Gitea-Repo hochladen (API: Release anlegen, dann Asset anhängen) und den Link in die Benachrichtigung schreiben.
- Am Ende des Laufs einen Überblick in `docs/PLAN.md` unter „Letzter Lauf“: was fertig ist, was wartet, was als Nächstes kommt.

## Regeln für alle Stufen

- Abnahme heißt nachgewiesen, nicht angenommen. Was nicht lief, steht als ungeprüft im Ergebnis.
- Leistungsziele aus `docs/SPEC.md` werden ab Stufe 7 bei jeder Änderung an Liste oder Suche neu gemessen.
- Der Windows-Build wird ab Stufe 6 am Ende jeder Stufe neu erzeugt, damit Plattformfehler früh auffallen.
- Jede Stufe endet mit grünem `scripts/check.sh` und einem Push.

## Letzter Lauf

Lauf vom 22.11.2026 abends bis 24.11.2026 morgens, Hauptsitzung mit bis zu vier Subagenten
gleichzeitig; Unterbrechungen durch einen Stromausfall (23.11., 15:54) und zwei Nutzungslimits.
Am 23.11. gegen 17:50 entschied der Mensch: keine Prototypen mehr, alles fertig machen, dann
ein finaler Build. Stand 24.11.2026.

**Fertig:** Stufen 00–05, 07–13 und 16; dazu von 14 und 15 alles, was ohne den Menschen geht.
- Stufe 14: Gmail über IMAP (A), Anmeldung mit Microsoft und Google über OAuth mit PKCE (B, C;
  Entscheidung 0042, getestet gegen den lokalen Stalwart und einen nachgebauten Anmeldeserver),
  JMAP mit Passwort oder API-Token, Push und Einstellungs-Sync (D, 0041), IMAP- und JMAP-Konto
  zusammen im vereinten Posteingang.
- Stufe 15: Lizenzliste in App und Installern, Dokumente für die Veröffentlichung, Datenschutz,
  Updates aus GitHub-Releases (0037), Sicherheitsprüfung mit Bedrohungsmodell
  (`docs/SECURITY_MODEL.md`) und Fuzzing, geführter erster Start (0044), „Diagnose exportieren“,
  CI auf dem privaten Gitea mit Runner auf dem Mac mini, GitHub-Workflow für macOS und Windows
  vorbereitet, macOS-Signatur und Notarisierung vorbereitet.
- Sicherheit: 9 Befunde in der Prüfung behoben, danach F1 (Server-Wechsel aus dem Sync wartet
  auf Bestätigung, 0040), C-2 (Vorschauen in einem eigenen Prozess mit Zeit- und Speichergrenze,
  0043), C-4 (jedes Bild einer Mail im selben Prozess neu kodiert, 0046) und elf kleinere (N-3, C-8, C-9, F2, V-02, V-07, F4, F5, N-4, C-6, G-1). Offen
  mit Begründung: V-04, V-05, F3 (nur unter Windows prüfbar), N-1 (Änderung in async-imap),
  C-5 (Leistung), N-3-Rest (Größe der Update-Beschreibung, liegt im Plugin).
- Abgleich mit `docs/SPEC.md`: Ansicht „Markiert“, Ordner umbenennen und löschen, fremde
  Entwürfe behalten ihre Bilder (0047), Hinweis bei nicht lesbarer Datenbank, „Gehe zu Ordner“.
- Gestaltung: README-Bilder aus Testdaten (`docs/images/`, reproduzierbar mit
  `README_SHOTS=1`), dabei gefundene Mängel behoben (KI-Bereich, Signatur, Suchergebnisse,
  Schrift von Mails ohne eigene Schrift, Werkzeugleiste).
- Leistung in der gebauten App gemessen (`docs/testing/performance.md`, 10.000 bis 500.000
  Mails): alle Ziele erreicht, Speicher als Footprint gemessen (0048); eine langsame Abfrage
  behoben.

**Finale Prüfung** auf de8cad7 im sauberen Worktree (24.11.2026, 08:40–09:05): `scripts/check.sh`
grün (1.326 Rust-, 3.988 Vitest-Tests, Clippy auch für Windows, cargo deny, volles pnpm audit,
Lizenzliste); Servertests gegen den lokalen Stalwart 1.423 von 1.423 grün; Desktop-Tests
(WKWebView, über `scripts/e2e-gui.sh` in der grafischen Sitzung) 23 von 23 Spec-Dateien, 126 Tests
grün. **Finaler Build** aus de8cad7: siehe Stufe 15, „Finaler Build“, und das Gitea-Release
`v0.1.0`.

**Wartet auf den Menschen:**
- Den finalen Build unter Windows testen (`docs/testing/windows-checklist.md`), auch die
  Installation über einen Prototyp und das Taskleisten-Symbol bei 125/150 %; damit wird Stufe 06
  fertig. Den ersten Start einmal mit einem echten Konto durchgehen, auf Mac und Windows.
- Den privaten Update-Schlüssel und sein Passwort sichern (`docs/RELEASING.md`, „Back it up now“).
- Stufe 14: App-Registrierung bei Microsoft Entra und ein Google-Cloud-Projekt (genaue Angaben im
  Ergebnis der Stufe 14); danach die Handprüfungen mit Outlook.com, Microsoft 365 und Gmail.
  Fastmail mit API-Token ist ungetestet.
- macOS-Signatur: Herausgeber ist BITWATER LLC (0035, Nachtrag 24.11.). Apple hat die Anmeldung als
  Einzelperson am 24.11. abgebrochen und erstattet; die Anmeldung als Organisation ist am 24.11.
  eingereicht (Enrollment ID CMV992434U) und wartet auf Apples Prüfung der Zeichnungsberechtigung (`docs/RELEASING.md`, „Enrolling as an organisation“), dann die Schritte unter
  „After the membership is active“. Erste signierte Version: 0.1.1.
- Vor dem ersten Push auf GitHub: Autor-Adresse in der Geschichte und die Refs
  (`docs/internal/history-audit.md`), Kontaktadresse in `SECURITY.md`, dann die Umstellung nach
  `docs/internal/umstellung-bei-veroeffentlichung.md`.
- Echtes Konto (bleibt nur lesend), Schreibzugriff für ein Nebenkonto, MCP mit einem echten
  Client, Einstellungs-Sync mit zwei Geräten, Ollama oder API-Schlüssel für die KI in der App,
  Screenreader-Checkliste, Bildschirm mit 2560×1440.
- Offene Frage „Something else“ zu den Extras (Stufe 16).

**Nächste Schritte:** Rückmeldungen aus dem Windows-Test einarbeiten; nach den Registrierungen
Stufe 14 abnehmen; nach den Entscheidungen zur Geschichte die Veröffentlichung auf GitHub
(Stufe 15: erster Push, GitHub-CI auf macOS und Windows, erstes öffentliches Release mit
Update-Signatur). Danach Kalender und Adressbuch-Abgleich (nach Version 1).
