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
- Zugang: Das `gh`-Konto `othervice` ist auf dem Mac mini angemeldet (neben `opensourceminers`);
  für Stufe 15 genügt `gh auth switch --user othervice`.
- Name (Nachtrag 23.11.2026, vom Menschen): Der Produktname bleibt „Vice Mail“, mit eigenem Logo
  (drei Entwürfe zur Auswahl). Eine Marken- und Domain-Prüfung vor der Veröffentlichung bleibt
  empfohlen; die Bundle-ID `org.vicemail.desktop` hängt am Namen.
- Signierte Builds sind gewünscht (Nachtrag 23.11.2026). macOS: Apple-Developer-Programm (99 USD im
  Jahr), Developer-ID-Zertifikat und Notarisierung. Windows: Azure Artifact Signing nur für
  Organisationen in den zugelassenen Ländern (Privatpersonen nur USA/Kanada), sonst ein
  OV-Zertifikat einer Zertifizierungsstelle (z. B. Certum Open Source) mit Schlüssel auf Hardware
  oder im Cloud-HSM.
- Entscheidung des Menschen (23.11.2026): Veröffentlichung als Privatperson, zunächst **ohne
  Signatur**. `docs/INSTALL.md` erklärt die Warnungen von Gatekeeper und SmartScreen. Nachrüsten ist
  jederzeit möglich, auch als Privatperson ohne D-U-N-S-Nummer (die braucht Apple nur für Firmen):
  Apple-Developer-Programm als Einzelperson, für Windows ein OV-Zertifikat für Einzelpersonen.

## Rückweg

Solange niemand von außen beigetragen hat, lässt sich die Lizenzentscheidung noch ändern; danach nur
mit Zustimmung aller Beitragenden.
