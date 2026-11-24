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

## Nachtrag 23.11.2026: macOS-Signatur vorbereitet

Der Mensch hat die Mitgliedschaft im Apple-Developer-Programm als Einzelperson beantragt; die
Aktivierung steht aus. Signiert werden soll **nur macOS**; **Windows bleibt unsigniert** (kein
OV-Zertifikat beantragt, SmartScreen-Abschnitt in `docs/INSTALL.md` gilt weiter). Releases vor der
Aktivierung bleiben ad hoc signiert.

Vorbereitet (Ablauf und Portal-Schritte in `docs/RELEASING.md`, „macOS signing and notarisation“):

- `scripts/apple-signing-setup.sh`: privater Schlüssel (RSA 2048) direkt im Anmelde-Schlüsselbund,
  nur über Pipes, nie als Datei; Zertifikatsanfrage (CSR) nach `~/Downloads`; Import des
  `.cer` mit Prüfung, dass es zum Schlüssel passt, und Apples Zwischenzertifikat bei Bedarf;
  Notarisierungs-Zugang als notarytool-Profil `vicemail-notary` im Schlüsselbund, bevorzugt mit
  einem App-Store-Connect-API-Schlüssel (Team-Schlüssel, Rolle „Developer“; persönliche Schlüssel
  kann notarytool nicht nutzen), sonst Apple-Account mit App-spezifischem Passwort; die `.p8`-Datei
  wird nur nach Bestätigung gelöscht und erst, nachdem das Profil ohne sie funktioniert hat;
  `--check` prüft Identität, eine Testsignatur und das Profil, ohne Geheimnisse auszugeben.
- `scripts/build-macos.sh` erkennt eine gültige Identität „Developer ID Application“ selbst
  (oder `VICEMAIL_SIGNING_IDENTITY`) und signiert dann von innen nach außen (`mail-mcp`, dann die
  App) mit Hardened Runtime und sicherem Zeitstempel, notarisiert in zwei Runden (App als Zip,
  danach das Disk-Image mit der gestapelten App), stapelt beide und prüft mit `codesign`, `spctl`
  („accepted, source=Notarized Developer ID“) und `stapler validate`. Mit `VICEMAIL_UPDATER=1`
  enthält das Update-Archiv die notarisierte App. Scheitert etwas, bricht der Build ab; ohne
  Identität bleibt er ad hoc wie bisher. Tauri signiert und notarisiert dabei nicht selbst: Es
  kennt kein notarytool-Profil, nur Umgebungsvariablen mit Passwort oder Pfad zur `.p8`
  (`APPLE_PASSWORD`, `APPLE_API_KEY_PATH`), und es baut Disk-Image und Update-Archiv im selben
  Lauf wie die App, also bevor das Skript sie stapeln könnte. Deshalb legt Tauri nur die App an;
  Disk-Image und Update-Archiv entstehen im Skript aus der gestapelten App.
- Entitlements: keine (`app/src-tauri/Entitlements.plist`, leer, mit Begründung). WKWebView
  führt JavaScript im WebContent-Prozess von WebKit aus, die Passwörter liegen im klassischen
  Schlüsselbund (`apple-native-keyring-store`, Feature `keychain`), ausgehende Verbindungen
  brauchen außerhalb der Sandbox keine Berechtigung. Die Ad-hoc-Builds laufen seit Tauri 2 schon
  mit Hardened Runtime ohne Entitlements.
- Getestet mit einem selbst signierten Testzertifikat in einem Wegwerf-Schlüsselbund (siehe
  Stufe 15, Ergebnis). Ungeprüft bis zur Aktivierung: Notarisierung, Stapeln, Urteil von
  Gatekeeper, das Verhalten auf einem zweiten Mac und die einmalige Schlüsselbund-Abfrage beim
  Wechsel von der unsignierten zur signierten App.

## Rückweg

Solange niemand von außen beigetragen hat, lässt sich die Lizenzentscheidung noch ändern; danach nur
mit Zustimmung aller Beitragenden.

## Nachtrag 24.11.2026: Herausgeber BITWATER LLC

Entscheidung des Menschen: Herausgeber von Vice Mail ist die Firma **BITWATER LLC** (D-U-N-S
146941332), nicht mehr eine Privatperson. Das gilt für den Installer (`bundle.publisher`), das
Copyright (`bundle.copyright`, „© 2026 BITWATER LLC“), die Seite „Über“ (`PUBLISHER` in
`app/src/config/product.ts`), die README und die Apple-Mitgliedschaft, die als Organisation läuft
(Developer-ID-Zertifikat auf BITWATER LLC; Schritte in `docs/RELEASING.md`). Die Lizenz bleibt
GPL-3.0-or-later, Repo und Konto bleiben `github.com/othervice`.

- Windows bleibt **vorerst unsigniert** (Entscheidung des Menschen). Mit der D-U-N-S-Nummer kämen
  später Azure Artifact Signing (Firmen in den USA, der EU und Kanada) oder ein OV-Zertifikat in
  Frage.
- v0.1.0 erscheint unsigniert; die erste signierte Version (macOS) ist 0.1.1, sobald die
  Mitgliedschaft aktiv ist. Sie kommt als Update in der App.
- Die Autor-Adresse der Geschichte ist seit dem 24.11.2026 `othervice@bitwaterlabs.com`.
