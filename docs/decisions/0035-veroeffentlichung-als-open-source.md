# 0035 — Veröffentlichung als Open Source auf GitHub unter `othervice`

Stand 23.11.2026, vom Menschen entschieden.

## Entscheidung

Vice Mail wird Open Source unter GPL-3.0-or-later (wie seit Stufe 0 festgelegt) und nicht als
Lizenz verkauft. Veröffentlicht wird, wenn Stufe 15 so weit ist, als öffentliches Repo unter
`https://github.com/othervice`. Bis dahin gilt weiter: Das Repo bleibt privat in Gitea, nichts geht
nach GitHub.

## Begründung

- Ein Mail-Client sieht Zugangsdaten und alle Mails; „privat, ohne Cloud-Dienst, ohne Telemetrie“
  ist nur prüfbar glaubwürdig, wenn der Code offen ist.
- Ein Verkauf bräuchte Bezahlung, Lizenzprüfung, Steuer und Support, die laut `docs/SPEC.md`
  ausdrücklich nicht zu Version 1 gehören.
- Einnahmen bleiben möglich, ohne die Lizenz zu ändern: Spenden und Sponsoring, auf Wunsch
  bezahlte signierte Builds über einen Store, Support.

## Folgen für Stufe 15

- Vor dem ersten Push nach GitHub: Geschichte auf Geheimnisse und fremde Dateien prüfen
  (`design-reference/` darf nie eingecheckt worden sein), Plan-Dokumente übersetzen oder als
  internes Archiv kennzeichnen (Aufgaben der Stufe 15).
- Zugang: Das Repo legt der Mensch unter `othervice` an oder gibt dafür einen Zugang frei; das auf
  dem Mac mini angemeldete `gh`-Konto ist ein anderes.
- Noch offen und vor Stufe 15 zu entscheiden: der endgültige Produktname (Marken- und
  Domain-Prüfung, die Bundle-ID `org.vicemail.desktop` hängt daran) und ob signierte Builds
  gewünscht sind (Apple-Developer-Programm, Windows-Signatur).

## Rückweg

Solange niemand von außen beigetragen hat, lässt sich die Lizenzentscheidung noch ändern; danach nur
mit Zustimmung aller Beitragenden.
