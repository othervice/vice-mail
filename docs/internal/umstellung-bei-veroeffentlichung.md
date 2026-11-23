# Umstellung bei der Veröffentlichung

Stand 23.11.2026. Was am Tag der Veröffentlichung (Stufe 15, 0035) umgestellt wird, damit es ein
kleiner Schritt bleibt. Bis dahin gilt `CLAUDE.md` unverändert: Plan- und Entscheidungsdokumente
sind Deutsch, das Repo bleibt privat in Gitea.

## Schon vorbereitet (23.11.2026)

- Englisch und für die Öffentlichkeit geschrieben: `README.md`, `CONTRIBUTING.md`, `SECURITY.md`,
  `CHANGELOG.md`, `docs/PRIVACY.md`, `docs/README.md` (Aufbau der Dokumentation),
  `docs/SETUP.md` (übersetzt, Inhalt unverändert), `docs/images/README.md` (welche Bildschirmfotos).
  Schon vorher Englisch: `docs/INSTALL.md`, `docs/RELEASING.md`, `docs/PROVIDERS.md`,
  `docs/SYNC.md`, `docs/brand/README.md`, `testmail/README.md`.
- Projektprotokoll (bleibt Deutsch): `docs/PLAN.md`, `docs/SPEC.md`, `docs/DESIGN.md` tragen oben
  einen englischen Hinweis; `docs/stages/README.md` und `docs/decisions/README.md` erklären die
  Ordner auf Englisch. Nichts wurde verschoben, alle Verweise gelten weiter.
- `docs/ARCHITECTURE.md`: englische Übersetzung in `docs/en/ARCHITECTURE.md`, übersetzt vom Stand
  e0ad102 (Blob 065632e). Bis zur Umstellung ist die deutsche Datei maßgeblich und wird wie bisher
  gepflegt.
- Geschichte geprüft: `docs/internal/history-audit.md`.

## Am Tag der Veröffentlichung

1. **Entscheidungen aus der Prüfung der Geschichte** treffen (Autor-Adresse, welche Refs), siehe
   `docs/internal/history-audit.md`. Wird die Geschichte umgeschrieben, dann zuerst.
2. **Architektur umstellen:**
   ```sh
   git diff e0ad102 -- docs/ARCHITECTURE.md      # Änderungen seit der Übersetzung
   ```
   Diese Änderungen in `docs/en/ARCHITECTURE.md` übertragen, den Hinweis oben entfernen, dann
   `git mv -f docs/en/ARCHITECTURE.md docs/ARCHITECTURE.md` (das deutsche Original bleibt in der
   Geschichte). In `docs/README.md` den Absatz „`ARCHITECTURE.md` is still in German …“ und
   `en/` in der letzten Zeile streichen.
3. **`CLAUDE.md` anpassen:**
   - Abschnitt „Sprache“: `docs/ARCHITECTURE.md` und alle Dokumente außer dem Projektprotokoll
     (`PLAN.md`, `stages/`, `decisions/`, `SPEC.md`, `DESIGN.md`, `testing/`) sind Englisch;
     ob neue Stufen und Entscheidungen Deutsch bleiben oder Englisch werden, entscheidet der Mensch.
   - Abschnitt „Git“: `origin` ist GitHub (`github.com/othervice/vice-mail`), Gitea nur noch
     Sicherung oder entfernt; den Satz „Nichts nach GitHub pushen, bevor Stufe 15 das vorsieht“
     streichen.
4. **`SECURITY.md`:** `security@<domain>` durch die echte Adresse ersetzen (samt HTML-Kommentar).
   Auf GitHub unter Settings → Code security „Private vulnerability reporting“ einschalten, sonst
   führt der Link in `SECURITY.md` ins Leere.
5. **Bildschirmfotos** nach `docs/images/README.md` aufnehmen und im Abschnitt „Screenshots“ der
   `README.md` einkommentieren; die Zeile „Screenshots will follow …“ streichen.
6. **Erstes Release:** in `CHANGELOG.md` `[Unreleased]` in `[0.1.0] - <Datum>` umbenennen, einen
   neuen leeren `[Unreleased]`-Abschnitt anlegen, Verweise unten auf
   `compare/v0.1.0...HEAD` und `releases/tag/v0.1.0` setzen. In `README.md` „There is no public
   release yet …“ ersetzen; in `docs/INSTALL.md` und `docs/RELEASING.md` die Hinweise „Until the
   project is public …“ prüfen.
7. **Kleinigkeiten im Arbeitsstand** (Geschichte bleibt, wie sie ist): interne Gitea-Adresse in
   `docs/stages/00-umgebung-und-spikes.md:73`; Zeile „Auslieferung von Test-Installern“ (Gitea) in
   `docs/SPEC.md`, Entscheidungen; LAN-Adresse `192.168.10.251` in zwei Tests (Code, siehe Prüfung).
8. **GitHub-Repo:** Beschreibung, Themen, Lizenz wird aus `LICENSE` erkannt (unveränderter
   GPL-3.0-Text, gleiche SHA-1 wie die GNU-Projekte). Nur `main` und Release-Tags pushen
   (`git push github main`, `git push github v0.1.0`), kein `--mirror`, kein `--tags`.
