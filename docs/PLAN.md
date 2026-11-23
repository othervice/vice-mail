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
| 08 | [Bedienung und Barrierefreiheit](stages/08-bedienung-und-barrierefreiheit.md) | fertig | Layouts, Tastatur, Command Palette |
| 09 | [Posteingang-Komfort](stages/09-posteingang-komfort.md) | fertig | Kategorien, Snooze, Benachrichtigungen, Standard-Mail-App |
| 10 | [Composer vollständig](stages/10-composer-vollstaendig.md) | fertig | Signaturen, Vorlagen, Kontakte, Später senden |
| 11 | [Einstellungs-Sync](stages/11-einstellungs-sync.md) | fertig | **Geräte gleichen sich über das Postfach ab** |
| 12 | [KI über MCP](stages/12-ki-ueber-mcp.md) | fertig | eigene KI (Claude, Cursor, LM Studio …) per MCP anschließen |
| 13 | [KI in der App (optional)](stages/13-ki-in-der-app.md) | fertig | Zusammenfassen, Vorschläge, Schreibhilfe, Fragen mit Quellen |
| 14 | [Weitere Anbieter](stages/14-weitere-anbieter.md) | in Arbeit (Block A fertig) | Microsoft, Gmail-Eigenheiten, JMAP |
| 16 | [Funktionsabgleich](stages/16-funktionsabgleich.md) | fertig | Lese-Liste, Ordnerfavoriten, Verschieben zwischen Konten, Schnellantwort, Lesebestätigung (0032) |
| 15 | [Veröffentlichung](stages/15-veroeffentlichung.md) | offen (Updates vorbereitet) | öffentliches Repo, signierte Builds, Updates |

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

Lauf vom 22.11.2026 abends bis 23.11.2026 nachmittags, Hauptsitzung mit bis zu vier Subagenten
gleichzeitig. Stand 23.11.2026, 14:00.

**Fertig:** Stufen 00–05, 07–13 und 16. Jede mit grünem `scripts/check.sh` im sauberen Worktree,
Servertests gegen den lokalen Stalwart und Desktop-Tests (WKWebView); Einzelheiten und offene
Handprüfungen unter „Ergebnis“ der Stufendateien. Letzte Läufe: `check.sh` auf a7dd759 (1.102 Rust-,
3.860 Vitest-Tests, mit Clippy für Windows, cargo deny, pnpm audit), Servertests 1.185 von 1.186 (der
eine Fehlschlag war ein Wettlauf zweier Testgruppen, behoben in 1b457f1), Desktop-Tests auf fe5cdec
mit den Testständen aus de1b915: 21 Spec-Dateien, 115 Tests, axe ohne schwere Befunde.

**In Arbeit:** 06 (braucht Läufe auf dem Windows-Rechner), 14 (Block A Gmail über IMAP fertig; B
Microsoft und C Google-Anmeldung brauchen App-Registrierungen des Menschen), 15 (Veröffentlichung;
Updates aus GitHub-Releases sind gebaut und lokal getestet).

**Prototypen** (Gitea-Vorab-Releases): `prototype-2026-11-23` (Windows), `-2` (Windows und macOS),
`-3` (Design und Logo), `-3.1` (Windows-Fix: Sicherung vor dem Datenbank-Umbau, „os error 5“),
`-4` (de1b915: Stufe 16, Orange-Design, Anhangsvorschau, schärferes Taskleisten-Symbol).

**Entscheidungen des Menschen in diesem Lauf:** KI über MCP zuerst (0010); Mailbird-Abgleich als
Stufe 16, kein Zählpixel-Tracking, keine eingebetteten Fremd-Apps, Kalender nach Version 1 (0032);
Open Source auf `github.com/othervice`, Name „Vice Mail“, eigenes Logo, erste Releases ohne Signatur
(0035). Von Claude entschieden: Orange als Standard-Akzent (0038), Updates nur mit Zustimmung (0037).

**Wartet auf den Menschen:**
- Prototyp 4 unter Windows testen (`docs/testing/windows-checklist.md`, Abschnitte 1–20), auch die
  Installation über eine ältere Version und das Taskleisten-Symbol bei 125/150 %.
- Den privaten Update-Schlüssel sichern (`docs/RELEASING.md`): er liegt nur im Schlüsselbund des Mac
  mini; ohne ihn lassen sich installierte Versionen nie wieder aktualisieren.
- Echtes Konto (bleibt nur lesend), Schreibzugriff für ein Nebenkonto, Gmail-Konto (Liste in 0034),
  MCP mit einem echten Client, Einstellungs-Sync mit zwei Geräten, Ollama oder API-Schlüssel für die
  KI in der App, Screenreader-Checkliste, Bildschirm mit 2560×1440.
- Für Stufe 14: App-Registrierung bei Microsoft Entra bzw. Google (Client-ID); für Stufe 15 das leere
  Repo unter `othervice` oder der Zugang über das angemeldete `gh`-Konto.
- Optional: OpenSSH auf dem Windows-Rechner, damit Tests dort laufen können.

**Nächste Schritte:** Ordnerliste als ein Tab-Stopp (läuft), Rückmeldungen aus dem Windows-Test
einarbeiten, dann Stufe 15 vorbereiten (Repo aufräumen, Übersetzung der Plan-Dokumente, Sicherheits-
prüfung).
