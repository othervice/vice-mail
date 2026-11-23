# 0037 — Updates in der App: Tauri-Updater, nur mit Zustimmung

Stand 23.11.2026. Vorbereitet für Stufe 15; live geht die Funktion mit dem ersten öffentlichen
Release unter `github.com/othervice` (0035). Bis dahin gibt es die Update-Datei dort nicht, und eine
Prüfung endet sichtbar mit „Die App konnte nicht nach Updates suchen“.

## Entscheidung

- **Bibliothek:** `tauri-plugin-updater` =2.14.1 (Apache-2.0 OR MIT, Release vom 28.10.2026, Teil
  von `tauri-apps/plugins-workspace`). Ohne Default-Features: kein `ring`-Provider, kein
  System-Proxy, kein zip (Artefakte von Tauri 1). TLS kommt aus dem reqwest des Workspace
  (`rustls-no-provider`) mit unserer Konfiguration aus `mail_imap::tls` (`aws-lc-rs`,
  System-Zertifikatsprüfung). Exakt gepinnt, weil wir `configure_client` nutzen, das laut Plugin
  an seine reqwest-Version gebunden ist. `tauri-plugin-process` ist nicht nötig: der Neustart ist
  `AppHandle::request_restart` aus Tauri selbst.
- **Nur aus Rust.** Die Oberfläche ruft fünf eigene Commands (`getUpdateInfo`,
  `setUpdateAutoCheck`, `checkForUpdates`, `downloadUpdate`, `installUpdate`) und hört auf
  `updateStatusChanged`. Keine Capability gewährt `updater:*`; ein Test prüft das.
- **Aufteilung:** `mail-engine/src/updates.rs` entscheidet, *ob* gefragt wird (Schalter, Frage,
  Tagesdrossel, Reihenfolge der Schritte) und hält den Zustand; `updates/http.rs` legt die
  HTTP-Regeln fest. Die Shell (`app/src-tauri/src/updater.rs`) setzt nur das Plugin als
  `UpdateSource` dahinter.
- **Endpunkt:** eine Stelle, `plugins.updater.endpoints[0]` in `tauri.conf.json`:
  `https://github.com/othervice/vice-mail/releases/latest/download/latest.json`. Ändert sich der
  Repo-Name, wird nur diese Zeile angepasst; `scripts/release-local.sh` leitet die
  Download-Adressen daraus ab. Überschreiben nur im Desktop-Test-Build
  (`MAIL_APP_E2E_UPDATE_ENDPOINT`, Feature `e2e`) und in den Rust-Tests.
- **Erlaubte Hosts:** der Host des Endpunkts, bei `github.com` dazu GitHubs Dateihosts
  (`objects.githubusercontent.com`, `release-assets.githubusercontent.com`). Weiterleitungen nur
  per HTTPS und nur dorthin, höchstens fünf. Eine Download-Adresse aus dem Manifest außerhalb
  dieser Hosts wird abgelehnt, bevor etwas geladen wird: Das Manifest selbst ist nicht signiert,
  also darf es die App zu keinem anderen Server schicken. HTTP nur, wenn der Endpunkt selbst eine
  Loopback-Adresse ist (Tests).
- **Signatur:** minisign (Ed25519) über `tauri signer`. Öffentlicher Schlüssel in
  `tauri.conf.json` (`plugins.updater.pubkey`, Schlüssel-ID `CBF2BA7119C9327E`).
  `requireSignedVersion: true`: Die Datei muss für genau die angekündigte Version signiert sein;
  so kann ein verändertes Manifest keine alte, echt signierte Version als neue ausgeben. Eine
  niedrigere oder gleiche Version wird nie angeboten (`allowDowngrades` bleibt aus).
- **Privater Schlüssel:** nur im Anmelde-Schlüsselbund des Build-Macs, Dienst
  `org.vicemail.updater`, Einträge `private-key`, `password` (und `public-key`). Erzeugt mit
  `scripts/updater-key.sh generate`: Passwort im Speicher, Tauri-CLI über ihre Node-API im selben
  Prozess, `security` über die Standardeingabe; nichts davon in einer Datei, einem Log, einer
  Umgebungsdatei oder einer Befehlszeile. Die Build-Skripte lesen ihn nur mit
  `VICEMAIL_UPDATER=1` (`scripts/updater-signing.sh`) und nur in die Umgebung des Builds.
  Sicherung und Wechsel stehen in `docs/RELEASING.md`. **Geht der Schlüssel verloren, kann keine
  installierte Version mehr ein Update annehmen** (sie vertrauen nur diesem öffentlichen
  Schlüssel); dann bleibt nur die Installation von Hand.
- **Artefakte:** macOS `<App>.app.tar.gz` plus `.sig`, Windows der NSIS-`-setup.exe` selbst plus
  `.sig` (`createUpdaterArtifacts`, nur bei `VICEMAIL_UPDATER=1`). `scripts/updater-manifest.mjs`
  prüft jede Signatur selbst gegen den öffentlichen Schlüssel (Ed25519 über BLAKE2b-512, Node
  `crypto`, kein weiteres Werkzeug) und schreibt `latest.json` mit `version`, `notes`,
  `pub_date` und `platforms` für `darwin-aarch64` und `windows-x86_64`.
- **Installation:** nur nach Klick, in zwei Schritten: „Update herunterladen“ (Download und
  Signaturprüfung, die Bytes bleiben im Speicher) und „Neu starten und installieren“. macOS
  ersetzt das App-Bundle an Ort und Stelle und startet neu (über die Ereignisschleife, damit der
  Kern wie bei jedem Beenden sauber herunterfährt). Windows startet den NSIS-Installer im Modus
  `passive` (`/P /UPDATE /R`): nur ein Fortschrittsbalken, keine Seite „vor der Installation
  deinstallieren“, danach startet die App neu; vorher fährt der Kern herunter
  (`before_exit`). Eine Entwicklungs- oder vom Disk-Image gestartete Kopie ersetzt sich nie
  selbst (`update_install_failed` mit Hinweis).

## Datenschutz und Zustimmung

Die Prüfung ist die einzige Verbindung zu einem Server, den der Nutzer nicht eingerichtet hat.
Deshalb:

- Gerätespezifische Einstellung `updates` in der Tabelle `setting` (nicht in `SYNCED_SETTINGS`,
  wird also nie mit anderen Geräten abgeglichen): `autoCheck` (Standard **aus**),
  `consentAnswered`, `lastCheckMs`. Keine Migration nötig.
- **Automatisch** wird nur mit eingeschaltetem Schalter geprüft, höchstens einmal je 24 Stunden.
  Jeder Versuch zählt, auch ein fehlgeschlagener; eine Uhr, die zurückgestellt wurde, gilt als
  fällig. Der Hintergrundtask schaut stündlich nach (erstmals eine Minute nach dem Start), fragt
  aber nur, wenn das fällig ist und noch kein Update bekannt ist.
- **„Jetzt suchen“** in Einstellungen → Über fragt einmal, unabhängig vom Schalter, und schaltet
  ihn nicht ein.
- **Die einmalige Frage** erscheint, sobald das erste Konto eingerichtet ist, nicht beim ersten
  Start. Begründung: Beim ersten Start steht die Kontoeinrichtung im Vordergrund; eine Frage
  davor würde Menschen fragen, die die App nur ansehen, und stünde zwischen ihnen und dem
  eigentlichen Ziel. Nach dem ersten Konto benutzt jemand die App wirklich, und die Frage kommt als
  ruhige Karte unten links (kein modaler Dialog, nichts wartet auf sie). Beide Antworten gelten als
  beantwortet; „Nein, danke“ lässt alles aus.
- **Was GitHub sieht:** die IP-Adresse und den Zeitpunkt der Anfrage, beim Download welche Datei
  geladen wird. Die App schickt keine Kontodaten, keine Nutzungsdaten und keine Kennung: feste
  Manifest-Adresse ohne Version, neutrale Kennung (`Mozilla/5.0`), keine Cookies, kein Referrer,
  kein Proxy, keine Wiederholungen. Abweichend vom Auftrag („sieht die App-Version“) wird die
  Version gar nicht gesendet; der Text sagt genau das, was tatsächlich übertragen wird.
- Versionshinweise zeigt die Oberfläche nur als Text (nie als HTML oder Markdown), auf
  4.000 Zeichen gekürzt und ohne Steuerzeichen.

## Fehler

Typisiert, mit Texten auf Englisch und Deutsch: `update_check_failed`, `update_download_failed`,
`update_signature_invalid` (falsche Signatur oder für eine andere Version signiert),
`update_install_failed`, `update_not_ready` (Schritt ohne gefundenes oder geladenes Update). Ein
fehlgeschlagener Schritt erscheint im Zustand (`failed` mit Fehler); ein Command scheitert nur, wenn
der Schritt gar nicht beginnen kann.

## Geprüft

- `mail-model`: Drossel, Standardwerte, Fehlercodes.
- `mail-engine` (Fake-Quelle mit Zähler): keine Anfrage bei ausgeschaltetem Schalter, „Jetzt
  suchen“ als Positivkontrolle, höchstens eine Prüfung je Tag (auch bei Fehlern), Frage erst nach
  dem ersten Konto und nur einmal, Einstellung übersteht den Neustart, Download und Installation
  nur auf Anforderung und in der Reihenfolge, abgelehnter Download wiederholbar, Hinweise als Text;
  HTTP-Regeln (Hosts, HTTPS, Loopback nur für Loopback-Endpunkte).
- `mail-app` mit dem echten Plugin auf der Mock-Runtime gegen einen lokalen HTTP-Server, der jede
  TCP-Verbindung zählt: null Verbindungen ohne Zustimmung und ohne Klick, eine für „Jetzt suchen“;
  korrekt signierte neuere Version lädt und wird geprüft; manipulierte Datei, beschädigte Signatur
  und für eine andere Version signierte Datei werden abgelehnt; niedrigere und gleiche Version
  werden nicht angeboten; fremde Download-Adresse wird abgelehnt, ohne den anderen Server zu
  kontaktieren. Die Fixtures sind mit einem Wegwerf-Schlüssel signiert, dessen privater Teil nie
  gespeichert wurde.
- Vitest: Über-Seite (Schalter, Erklärung, Prüfen, Hinweise als Text, Fortschritt, Neustart erst
  nach Klick, Fehler mit Titel und Hinweis, Deutsch), Frage, ruhiger Hinweis, axe.
- Desktop-Test `app/tests/e2e/updates.e2e.mjs` geschrieben, **nicht ausgeführt**.

## Rückweg

Schalter aus genügt, um jede Verbindung zu vermeiden. Ohne Updater: Plugin und `start_updates`
entfernen, die Oberfläche zeigt dann „Dieser Build aktualisiert sich nicht selbst“. Ein neuer
Schlüssel ist ein Wechsel mit Übergangsversion (`docs/RELEASING.md`). Zurück auf eine ältere
Version geht nur von Hand; die Datenbank-Sicherungen aus Stufe 1 bleiben der Weg zurück.

## Nachtrag 23.11.2026 (0044)

Die einmalige Frage steht jetzt im Schritt „Optional“ des geführten ersten Starts, als Schalter
(aus); sie gilt als beantwortet, sobald der Schritt nach vorn verlassen oder von dort übersprungen
wird, auch ohne Konto. Die Karte nach dem ersten Konto bleibt als Rückfall für alle, die diesen
Schritt nie gesehen haben (vorher übersprungen, Installationen von vor dem ersten Start). Beide
Stellen setzen dieselbe Antwort, gefragt wird also nie zweimal. Begründung in 0044.
