# Prüfung der Git-Geschichte vor der Veröffentlichung

Stand 23.11.2026, 16:00, Stufe 15 („Repo aufräumen“). Nur lesend geprüft, nichts umgeschrieben.

**Ergebnis in einem Satz:** Keine Geheimnisse, keine fremden Dateien, `design-reference/` war nie
eingecheckt. Vor dem ersten Push nach GitHub muss nichts aus der Geschichte entfernt werden; offen
sind nur Entscheidungen des Menschen zu Metadaten (Autor-Adresse, Session-Links) und welche Refs
veröffentlicht werden.

## Umfang

- Alle Refs, die lokal bekannt sind (`--all`): `main`, `origin/main`, `origin/prototype-3.1`,
  Tags `prototype-2026-11-23`, `prototype-2026-11-23-2`, `spike-c-0.1.0`.
  Auf Gitea liegen zusätzlich die Tags `prototype-2026-11-23-3`, `-3.1`, `-4`; `-3.1` zeigt auf
  76d4da2 (Branch `prototype-3.1`, mitgeprüft), die anderen liegen auf `main`.
- 114 Commits vom 22.11. bis 23.11.2026, 4.213 Objekte, Pack 5,9 MiB.
- Stand des Arbeitsbaums: 1a3772a. Ignorierte, nie eingecheckte Ordner im Arbeitsbaum:
  `design-reference/`, `dist/`, `target/`, `.xwin-cache/`, `testmail/.data/`, `testmail/bin/`,
  `node_modules/`, `.claude/`.

## Befehle

```sh
git log -p --all --no-color --full-history > history.patch        # 17 MB, ganze Geschichte als Diff
git rev-list --objects --all > objects.txt                          # jeder Pfad, der je existierte
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectname) %(objectsize) %(rest)'
git log --all --diff-filter=D --name-only                           # je gelöschte Dateien
git log --all --format='%an <%ae> | %cn <%ce>' | sort | uniq -c      # Autoren
git log --all --format=%B                                           # Commit-Texte
```

Durchsucht wurde `history.patch` (ohne die Kopfzeilen der Commits) mit `grep -E` nach:

| Muster | Wonach |
|---|---|
| `ghp_`, `gho_`, `ghs_`, `ghu_`, `github_pat_`, `glpat-`, `xox[abposr]-`, `AKIA…`, `AIza…`, `sk-…`, `sk-ant-` | Tokens von GitHub, GitLab, Slack, AWS, Google, OpenAI, Anthropic |
| `-----BEGIN … PRIVATE KEY`, `BEGIN OPENSSH`, `PuTTY-User-Key`, `AGE-SECRET-KEY-1…` | private Schlüssel |
| `untrusted comment: … secret key` und dessen Base64-Form (`dW50cnVzdGVkIGNvbW1lbnQ6IHJzaWduIGVuY3J5cHRlZCBzZWNyZXQga2V5`, `RWRTY0Iy`) | privater Update-Schlüssel (minisign/Tauri) |
| `password=`, `password:`, `secret=`, `token=`, `api_key=` mit Wert | Passwörter und Schlüssel in Konfiguration |
| `https?://user:pw@` | Zugangsdaten in URLs (auch Gitea mit Token) |
| `GITEA_TOKEN`, `TAURI_SIGNING_PRIVATE_KEY`, `APPLE_PASSWORD`, `APPLE_API_KEY`, `UNIFI_PASS`, `TRUENAS_KEY`, `JELLYFIN` | Umgebungsvariablen mit Geheimnissen |
| Zeichenfolgen ≥ 40 Zeichen aus Base64-Alphabet mit Groß-, Kleinbuchstaben und Ziffern (ohne Lockfiles, SVG, `.eml`, erzeugte Bindings, Snapshots) | unbekannte Schlüssel |
| `192.168.*`, `10.*` | interne Adressen |
| alle E-Mail-Adressen, nach Domain gezählt | personenbezogene Daten |
| `<old-domain>`, `/Users/othervice`, `C:\Users`, `opensourceminers`, `Jarvis`, `macmini`, `secrets.env`, `Proxmox` | Hinweise auf das private Netz und die Umgebung |
| Pfade mit `.png .jpg .webp .pdf .zip .dmg .exe .p12 .p8 .pem .cer .key .sqlite …` | Binärdateien, Zertifikate, Datenbanken |

## Befunde

### Geheimnisse: keine

- Alle Treffer für Passwörter, Tokens und Schlüssel sind Testdaten oder Code: die dokumentierten
  Testkonten des lokalen Servers (`alice-test-pw`, `bob-test-pw`, `admin-test-pw`,
  `testmail/README.md`), erfundene Werte in Tests (`sk-ant-test`, `sk-ant-api03-very-secret-value`,
  `s3cr3t-token-value`, `hunter2`, `wrong-secret-4711`), Variablennamen, Prüfungen in
  `scripts/apple-signing-setup.sh` (`-----BEGIN RSA PRIVATE KEY-----` als Vergleichstext) und
  `scripts/updater-signing.sh` (`export TAURI_SIGNING_PRIVATE_KEY="$key"`, Wert aus dem
  Schlüsselbund zur Laufzeit).
- Update-Schlüssel: Im Repo stehen nur **öffentliche** minisign-Schlüssel (der echte in
  `app/src-tauri/tauri.conf.json`, ein Testschlüssel `app/src-tauri/src/updater/fixtures/test-key.pub`)
  und eine Testsignatur (`update-0.2.0.sig`). Kein privater Schlüssel, auch nicht in Base64.
- Keine Zugangsdaten in URLs außer erfundenen (`https://user:pw@api.example.com/v1` in Tests).
- Keine Umgebungsvariablen mit echten Werten, keine `.env`-Dateien (`.gitignore` schließt sie aus).

### Fremde Dateien: keine

- `design-reference/` (fremde Screenshots `inbox-reference.jpg`, `polish-reference.png`,
  Logo-Vorlagen `.webp`) ist in keinem Commit enthalten: 0 Objekte mit diesem Pfad, ignoriert seit
  dem ersten Commit (`.gitignore`). Die Dokumente erwähnen die Dateien nur mit Namen
  (`docs/DESIGN.md`, `docs/decisions/0031-…`, Stufe 15).
- Die Benchmark-Proben, die laut `docs/testing/performance.md` das JPEG aus `design-reference/`
  enthielten, liegen außerhalb des Repos (`VICEMAIL_BODY_SAMPLES`); eingecheckt ist nur der
  Generator `crates/mail-store/src/body_bench.rs`.
- Binärdateien in der Geschichte: nur die eigenen App-Symbole (`app/src-tauri/icons/`, aus
  `source*.svg` erzeugt, `generate.sh`), die Marken-SVGs in `docs/brand/` und die Symbole der
  Spike-App (`spikes/tauri-app/src-tauri/icons/`, eigenes einfaches SVG, kein Tauri-Standardlogo).
  Größtes Objekt: `icon.icns` mit 611 KiB. Keine Installer, Disk-Images, Zertifikate, Datenbanken.
- Testmails (`testmail/corpus/`, `crates/mail-mcp/tests/corpus/`) sind synthetisch
  (`@vicemail.test`, `*.example`); Adressen echter Versanddienste (`mcsv.net`, `hubspotemail.net`,
  `amazonses.com`) stehen nur als Erkennungsmerkmale in `crates/mail-content/src/classify/`.
- Gelöschte Dateien in der Geschichte: nur drei, alle eigener Code bzw. Plan
  (`app/src-tauri/src/webview2_frames.rs`, zwei alte Stufendateien 12 und 13).
- Mailbird wird in `docs/decisions/0032-…`, `docs/stages/16-…`, `docs/DESIGN.md` und `docs/PLAN.md`
  beim Namen genannt (Funktionsvergleich, Zahl der Funktionen von getmailbird.com/features). Kein
  kopierter Text, keine Bilder. Unbedenklich, aber beim Übersetzen sachlich halten.

### `LICENSE`

Vollständiger Text der GPL-3.0 (674 Zeilen, 35.149 Byte), unverändert: SHA-1 `31a3d460…`, identisch
mit `COPYING` der GNU-Projekte gnutls, libidn2, libtasn1 und libunistring (Homebrew auf dem Mac
mini). „or later“ ergibt sich aus den Angaben im Projekt (`license = "GPL-3.0-or-later"` in
`Cargo.toml` und `app/package.json`, `LICENSE_SPDX` in `app/src/config/product.ts`, `README.md`);
Kopfzeilen in den Quelldateien gibt es nicht, die GPL verlangt sie nicht.

### Personenbezogene Daten und private Umgebung

| Fund | Wo | Bewertung |
|---|---|---|
| **Autor- und Committer-Adresse `othervice <<old-address>>`** in allen 114 Commits | Commit-Metadaten | Wird mit dem Push öffentlich und ist danach nicht mehr zurückzuholen. **Entscheidung des Menschen**, siehe unten |
| `Claude-Session: https://claude.ai/code/session_…` in 113 Commit-Texten, dazu `Co-Authored-By: Claude … <noreply@anthropic.com>` | Commit-Texte | Die Links führen nur mit Anmeldung des Kontoinhabers zu etwas; kein Geheimnis. Zeigt, dass Code mit Claude entstand (das ist ohnehin gewollt sichtbar). Entfernen hieße Geschichte umschreiben; nicht nötig |
| `http://192.168.30.175:3000/othervice/vice-mail/releases/tag/spike-c-0.1.0` | `docs/stages/00-umgebung-und-spikes.md:73` (3089f7c) | interne Gitea-Adresse, aus dem Internet nicht erreichbar. Harmlos; kann im Arbeitsstand ersetzt werden, Geschichte bleibt |
| `192.168.10.251` (Mac mini im LAN) | Tests `crates/mail-ai/src/endpoint.rs:233`, `crates/mail-engine/src/reading/remote.rs:365` (als Beispiel einer LAN-Adresse) | harmlos (private Adresse); bei Gelegenheit durch `192.168.1.x` ersetzen (Code, nicht Teil dieses Auftrags) |
| Mac mini „Jarvis“ | `docs/SETUP.md` (im Arbeitsstand jetzt entfernt, Geschichte enthält es) | harmlos |
| zweites `gh`-Konto `opensourceminers` | `docs/decisions/0035-…:28` | harmlos, öffentlich bekanntes Konto; bei Bedarf im Arbeitsstand kürzen |
| Adressen bei echten Anbietern (`hansmueller@gmail.com`, `emily.parker@icloud.com`, `max.muster@gmail.com`, `ich@posteo.de` u. a.) | Tests für Adress-Normalisierung und Anbietererkennung | Beispieladressen ohne Netzverkehr; keine bekannten echten Personen. Ersetzbar durch `*.example`, wo die Domain egal ist; bei Anbietererkennung braucht der Test die echte Domain |
| `i@izs.me` | `pnpm-lock.yaml` (Paket-Metadaten) | öffentlich, unbedenklich |

Keine Vorkommen von `<old-domain>` im Inhalt, keine Pfade `/Users/othervice` oder `C:\Users`, kein
`secrets.env`, keine Hinweise auf Proxmox oder andere Rechner des Heimnetzes.

## Was der Mensch vor dem ersten Push entscheidet

1. **Autor-Adresse.** Zwei Wege:
   - so lassen: `<old-address>` steht dauerhaft öffentlich in jedem Commit;
   - vor dem ersten Push umschreiben, z. B. auf die GitHub-Noreply-Adresse von `othervice`
     (`<ID>+othervice@users.noreply.github.com`, ID in den GitHub-Einstellungen unter „Emails“):
     `git filter-repo --mailmap mailmap.txt` (Werkzeug `git-filter-repo`, nicht eingerichtet) mit
     der Zeile `othervice <ID+othervice@users.noreply.github.com> <<old-address>>`.
     Folgen: **alle Commit-Hashes ändern sich** (auch die in Stufendateien, Entscheidungen und im
     Changelog zitierten wie a7dd759, de1b915, 76d4da2, die dann nicht mehr auflösen), alle Tags
     müssen neu gesetzt, die Gitea-Kopie ersetzt oder neu geklont werden; laufende Sitzungen und
     Worktrees arbeiten auf alten Hashes weiter. Danach `git config user.email` auf die neue Adresse
     setzen, sonst kommt die alte mit dem nächsten Commit zurück.
   - Empfehlung: Wenn die Adresse privat bleiben soll, jetzt umschreiben, bevor irgendetwas
     öffentlich ist; ein späteres Umschreiben eines öffentlichen Repos ist schlimmer. Die zitierten
     Hashes in `docs/` lassen sich danach mit einer Zuordnungstabelle (filter-repo schreibt
     `.git/filter-repo/commit-map`) ersetzen.
2. **Welche Refs veröffentlicht werden.** Empfehlung: nur `main` und die Tags, die zu öffentlichen
   Releases werden; die Prototyp-Tags und den Branch `prototype-3.1` nicht nach GitHub pushen
   (`git push github main`, kein `--mirror`, kein `--tags`). Sie verweisen auf Gitea-Releases, die
   es dort nicht gibt.
3. **`docs/internal/`** (dieser Bericht und die Umstellungsliste) enthält nichts Geheimes, kann also
   mit; wer es nicht öffentlich will, löscht den Ordner vor dem Push im Arbeitsstand.
4. **`CLAUDE.md`** wird öffentlich. Es beschreibt Arbeitsregeln und nennt Gitea als `origin`; mit
   der Umstellung anpassen (siehe `docs/internal/umstellung-bei-veroeffentlichung.md`).

## Nicht geprüft

- Inhalte, die nur auf Gitea liegen (Release-Anhänge der Prototypen, Wiki, Issues): sie gehen nicht
  mit einem Git-Push mit.
- Ein Werkzeug wie `gitleaks` oder `trufflehog` lief nicht (nicht installiert); die Suche oben ist
  eine gezielte Mustersuche. Vor dem Push kann `gitleaks detect --log-opts=--all` als zweite Meinung
  laufen.
