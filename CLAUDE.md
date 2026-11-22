# Vice Mail

Open-Source-E-Mail-Client für macOS und Windows: Tauri 2, Rust-Kern, React-Oberfläche.
Selbst hostbar, ohne eigenen Cloud-Dienst, ohne Telemetrie. „Vice Mail“ ist ein Arbeitstitel.

## Zuerst lesen

| Datei | Wofür |
|---|---|
| `docs/PLAN.md` | Stufen, Status, welche Stufe dran ist |
| `docs/stages/NN-*.md` | Auftrag der aktuellen Stufe |
| `docs/ARCHITECTURE.md` | Stack, Modulgrenzen, Regeln der Mail-Engine, Entscheidungen |
| `docs/SPEC.md` | Funktionsumfang und was ausdrücklich nicht dazugehört |
| `docs/DESIGN.md` | Oberfläche (nur bei UI-Arbeit nötig) |

## Eine Sitzung, eine Stufe

1. In `docs/PLAN.md` die nächste offene Stufe nehmen und ihre Datei vollständig lesen.
2. Nur die Aufgaben dieser Stufe umsetzen. Was unter „Nicht in dieser Stufe“ steht, bleibt liegen.
3. Jede Aufgabe einzeln: umsetzen, testen, abhaken, committen.
4. Am Ende: `scripts/check.sh` grün, jeden Abnahmepunkt tatsächlich nachweisen, den Abschnitt
   „Ergebnis“ der Stufendatei ausfüllen, Status in `docs/PLAN.md` setzen, pushen.
5. Wird die Stufe nicht fertig: Stand unter „Ergebnis“ festhalten (erledigt, offen, nächster Schritt)
   und committen. Lieber sauber unterbrechen als Abnahmepunkte weglassen.
6. Braucht etwas den Menschen (Entscheidung, Zugang, Test auf Windows): alles andere fertig machen
   und es unter „Ergebnis → Wartet auf“ eintragen.

Stellt sich eine Annahme des Plans als falsch heraus: Entscheidung als `docs/decisions/NNNN-titel.md`
festhalten, `docs/ARCHITECTURE.md` anpassen und die betroffenen späteren Stufendateien korrigieren.

## Definition of Done

Eine Aufgabe ist fertig, wenn der Code integriert ist, Fehlerfälle behandelt sind, Tests dafür
existieren und bestehen und `scripts/check.sh` grün ist. Unter „Ergebnis“ steht nur, was wirklich
ausgeführt und geprüft wurde. Was nicht geprüft werden konnte, steht dort ausdrücklich als ungeprüft.

## Architekturgrenzen

- Die Oberfläche spricht nur über typisierte Commands und Events mit dem Kern. Die erzeugten Bindings
  (`app/src/generated/`, darin `invoke`) und `@tauri-apps/api` werden nur in `app/src/services/ipc.ts`
  importiert. Das Frontend greift nie auf Datenbank, Dateisystem oder Netz zu.
- `app/src-tauri` bleibt dünn: Fenster, Commands, Verdrahtung. Logik gehört in die Crates, damit sie
  ohne WebView testbar ist.
- `mail-model` enthält nur Typen und Traits, kein IO. Adapter (`mail-imap`) kennen weder Oberfläche
  noch Datenbank. Keine zirkulären Abhängigkeiten.
- Jede schreibende Mail-Aktion läuft über die Outbox (`operation`-Tabelle) und hat einen sichtbaren Zustand.
- HTML aus E-Mails ist nicht vertrauenswürdig und wird nur über die Pipeline aus
  `docs/ARCHITECTURE.md` (Abschnitt Sicherheitsgrenzen) angezeigt.

## Echte Postfächer schützen

- Sitzungen arbeiten ausschließlich mit dem lokalen Testserver (`scripts/testmail.sh`).
- Echte Zugangsdaten gibt nur der Mensch in der App ein. Nie danach fragen, nie in Dateien schreiben.
- Neue Konten sind nur lesend (`writes_enabled = false`). Schreibzugriff schaltet der Mensch pro Konto frei.

## Regeln

- Keine erfundenen Daten in der Oberfläche. Ohne Konto zeigt die App leere Zustände, keine Beispiel-Mails.
- Keine Geheimnisse in Code, Logs, Datenbank oder Repo. Passwörter und Schlüssel liegen im System-Schlüsselbund.
- Keine Netzverbindung außer zu Servern, die der Nutzer eingerichtet hat (Liste in `docs/ARCHITECTURE.md`).
- Keine UI-Texte in Komponenten. Alle Texte kommen aus `app/src/i18n/` (Englisch und Deutsch).
- Der Produktname steht nur in `app/src-tauri/tauri.conf.json` und `app/src/config/product.ts`.
- Neue Abhängigkeit nur mit Zweck, aktiver Pflege (Release in den letzten 12 Monaten) und Lizenz,
  die zu GPL-3.0 passt. Eintrag in der Stack-Tabelle von `docs/ARCHITECTURE.md`. TLS über rustls, kein OpenSSL.
- Fehler nie verschlucken: typisierte Fehler bis zur Oberfläche, dort verständlich anzeigen.

## Sprache

Code, Bezeichner, Kommentare, Commit-Messages, README und die Basistexte der Oberfläche sind Englisch.
Plan- und Entscheidungsdokumente unter `docs/` sind Deutsch, bis das Projekt veröffentlicht wird.

## Git

`origin` ist Gitea (privates Repo `vice-mail`, bis zur Veröffentlichung). Ein Commit pro abgeschlossener
Aufgabe, Push am Ende der Stufe. Nichts nach GitHub pushen, bevor Stufe 15 das vorsieht.

## Befehle

Stehen ab Stufe 0 bzw. 1 bereit:

```
scripts/check.sh                  # Format, Lint, alle Tests, Typecheck, Lizenz- und Sicherheitsprüfung
scripts/testmail.sh start|stop|seed   # lokaler IMAP/SMTP-Testserver
pnpm --dir app tauri dev          # App im Entwicklungsmodus
scripts/build-windows.sh          # Windows-Installer vom Mac aus (ab Stufe 6)
```
