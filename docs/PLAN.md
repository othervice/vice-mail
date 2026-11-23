# Plan

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
| 08 | [Bedienung und Barrierefreiheit](stages/08-bedienung-und-barrierefreiheit.md) | in Arbeit | Layouts, Tastatur, Command Palette |
| 09 | [Posteingang-Komfort](stages/09-posteingang-komfort.md) | in Arbeit | Kategorien, Snooze, Benachrichtigungen, Standard-Mail-App |
| 10 | [Composer vollständig](stages/10-composer-vollstaendig.md) | fertig | Signaturen, Vorlagen, Kontakte, Später senden |
| 11 | [Einstellungs-Sync](stages/11-einstellungs-sync.md) | fertig | **Geräte gleichen sich über das Postfach ab** |
| 12 | [KI über MCP](stages/12-ki-ueber-mcp.md) | fertig | eigene KI (Claude, Cursor, LM Studio …) per MCP anschließen |
| 13 | [KI in der App (optional)](stages/13-ki-in-der-app.md) | fertig | Zusammenfassen, Vorschläge, Schreibhilfe, Fragen mit Quellen |
| 14 | [Weitere Anbieter](stages/14-weitere-anbieter.md) | bei Bedarf | Microsoft, Gmail-Eigenheiten, JMAP |
| 15 | [Veröffentlichung](stages/15-veroeffentlichung.md) | offen | öffentliches Repo, signierte Builds, Updates |

Status: `offen`, `in Arbeit`, `wartet auf Mensch`, `fertig`.

## Reihenfolge

Stufen 00 bis 06 sind der kritische Pfad und bauen streng aufeinander auf. Danach ist die Reihenfolge
weniger fest: 07 bis 10 lassen sich tauschen, 11 braucht 09 (Snooze, Pins) und 10 (Signaturen,
Vorlagen), 12 braucht 05 und 07, 13 braucht 07 und 12. Stufe 14 kommt, sobald ein Konto sie braucht.

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

Nachtlauf 22./23.11.2026, Hauptsitzung mit bis zu vier Subagenten gleichzeitig. Zwischenstand
23.11., 08:00 (der Lauf geht weiter; dieser Abschnitt wird am Ende ersetzt).

**Fertig:** Stufen 00–04, 11, 12. Jeweils `scripts/check.sh` grün im sauberen Worktree, die
Servertests gegen den lokalen Stalwart und die Desktop-Tests (WKWebView) grün; Einzelheiten unter
„Ergebnis“ der Stufendateien.

**Windows-Prototyp:** Gitea-Vorab-Release `prototype-2026-11-23` (Commit 5d40e88, Stufen 1–5, 7,
9 Teil A, 11, 12 mit Oberfläche). Testanleitung: `docs/testing/windows-checklist.md`. Die macOS-.dmg
folgt nach der Windows-Version.

**In Arbeit** (Code und Tests weitgehend da, offen steht in der jeweiligen Stufendatei):
- 05: zwei fehlende Tests (Entwurf aus einem anderen Programm, Sofortversand offline).
- 06: braucht den Windows-Rechner (Installation, Laufzeitprüfungen, HTML-Sandbox unter WebView2).
  `scripts/check.sh` übersetzt den Windows-Code jetzt mit (`cargo xwin clippy`).
- 07: „Letzte Suchen“ und „Suchen speichern“ werden gebaut.
- 08: Einstellungen und Layout ziehen vom Browser-Speicher in den Kern; Mail und Composer im
  eigenen Fenster; Desktop-Tests für Tastatur und Layouts laufen noch nicht.
- 09: `.eml`-Dateien öffnen fehlt; Handprüfung Benachrichtigungen und Standard-Mail-App.
- 10: Oberfläche committet, Desktop-Tests und Rechtschreibprüfung offen.
- 13: Oberfläche wird gebaut; Prüfsatz mit echtem Modell braucht Ollama.

**Wartet auf den Menschen:** Windows-Prototyp testen; ein echtes Konto einrichten (bleibt nur
lesend) und entscheiden, für welches Konto Schreibzugriff freigeschaltet wird; MCP mit einem echten
Client prüfen; Einstellungs-Sync mit zwei Geräten; Ollama oder einen API-Schlüssel für Stufe 13
(nur in der App eingeben); Screenreader-Checkliste (`docs/testing/screenreader-checklist.md`).

**Offen aufgefallen:** Nach sehr vielen gerenderten Zeilen (alle 9.959 des Posteingangs zweimal)
ruckelt das Scrollen in derselben Sitzung (längster Frame etwa 250 ms); wird untersucht.
