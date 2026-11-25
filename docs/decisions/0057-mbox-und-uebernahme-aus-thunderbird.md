# 0057 — mbox-Export und -Import, Übernahme aus Thunderbird

- **Datum:** 24.11.2026
- **Anlass:** Stufe 19, Teil C (Auswahl des Menschen vom 24.11.2026): Ordner als mbox exportieren,
  mbox in einen Ordner importieren (über die Outbox, nur mit Schreibzugriff, mit Fortschritt und
  Abbrechen), Übernahme aus einem Thunderbird-Profil (Konten, Identitäten, Signaturen, lokale
  Ordner; nie Passwörter).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 19 C).
- **Code:** `crates/mail-content/src/mbox/`, `crates/mail-content/src/thunderbird/`,
  `crates/mail-model/src/mbox.rs`, `crates/mail-model/src/takeover.rs`,
  `crates/mail-store/src/imports.rs`, `crates/mail-engine/src/mbox.rs`,
  `crates/mail-engine/src/takeover.rs`, `app/src-tauri/src/commands/mbox.rs`,
  `app/src-tauri/src/commands/takeover.rs`, `app/src/features/mbox/MboxJobs.tsx`,
  `app/src/features/settings/accounts/ThunderbirdTakeover.tsx`, Testprofil `testmail/thunderbird/`.

## Entscheidung

### mbox

1. **Format beim Export: mboxrd.** Trennzeile `From <Absender> <asctime UTC>`, jede Zeile, die mit
   `>*From ` beginnt, bekommt ein `>` mehr, Zeilenenden LF, nach jeder Mail eine Leerzeile. Die
   Mail so, wie der Server sie hat (gespeichert, sonst ganz geholt, nicht gespeichert); nicht mehr
   vorhandene werden ausgelassen und gezählt. Abgebrochen oder fehlgeschlagen wird die halbe Datei
   gelöscht. Ziel: der Speicherdialog des Systems.
2. **Lesen beim Import:** zeilenweise (`MboxReader`, ein Push-Parser ohne IO in `mail-content`;
   die Datei liest der Kern auf dem Blocking-Pool), eine Mail nach der anderen, höchstens 64 MiB je
   Mail (größere werden übersprungen), Zeilen über 64 MiB brechen ab („keine mbox-Datei“). Trennzeilen
   nur am Dateianfang oder nach einer Leerzeile und nur in der Form `From <Wort> <Datum oder Uhrzeit>`
   (mboxrd und Thunderbirds mboxo). `>From`-Quoting wird zurückgenommen, Zeilenenden werden CRLF.
   Thunderbirds `X-Mozilla-Status`, `-Status2`, `-Keys` fallen aus dem Kopf; als gelöscht markierte
   Mails werden übersprungen.
3. **Kennzeichen:** aus `X-Mozilla-Status` (gelesen, beantwortet, markiert), sonst aus `Status`/
   `X-Status` (mutt, procmail), sonst „gelesen“ (ein Archiv soll nicht den Ungelesen-Zähler füllen).
4. **Jede Mail ein `append` der Outbox** in den gewählten Ordner, mit eingefrorenen Bytes, ihren
   Kennzeichen und ihrer Message-ID (eine Mail ohne bekommt `Message-ID: <zufällig@mbox-import.invalid>`
   vorangestellt, sonst könnte die Outbox nach einem Absturz nicht prüfen, ob sie schon ankam).
   Im Aktivitätsprotokoll als „Kopieren“. **Nur mit Schreibzugriff**, geprüft bevor der
   Öffnen-Dialog kommt und bevor etwas gelesen wird.
5. **Fortschritt und Abbrechen:** Export und Import laufen als Aufträge im Hintergrund
   (`MboxJob`, Ereignis `mboxJobChanged` höchstens alle 250 ms und am Ende). Höchstens
   `MBOX_IMPORT_WINDOW` = 50 Appends warten gleichzeitig (und ihre eingefrorenen Kopien auf der
   Platte); gelesen wird weiter, sobald der Server welche übernommen hat. „Abbrechen“ hört auf zu
   lesen und nimmt die noch wartenden Appends aus der Outbox; was der Server schon hat, bleibt.
   Höchstens ein Auftrag je Ordner, höchstens 200.000 Mails je Auftrag. Die Oberfläche zeigt die
   Aufträge unten links mit Fortschrittsbalken (natives `<progress>`, kein Inline-Style).
6. Bestätigte Appends verschwinden nach einer Weile aus der Outbox (`linger`); ein Auftrag zählt
   eine nicht mehr vorhandene Operation darum als übernommen, fehlgeschlagene als ausgelassen.

### Übernahme aus Thunderbird

7. **Profil wählen:** die Shell nennt den üblichen Ort (macOS `~/Library/Thunderbird`, Windows
   `%APPDATA%\Thunderbird`, sonst `~/.thunderbird`), der Kern liest dort `profiles.ini` (Standard aus
   dem `[Install…]`-Abschnitt, sonst `Default=1`). Oder „Profilordner wählen …“ (Ordnerdialog): ein
   Profil (mit `prefs.js`) oder Thunderbirds Ordner selbst (dann sein Standardprofil).
8. **Gelesen wird nur:** `profiles.ini`, `prefs.js` (höchstens 16 MiB), die Signaturdateien, die
   Identitäten nennen (höchstens 64 KiB, nur Text), und die mbox-Dateien unter „Local Folders“.
   **Nie `logins.json` oder `key4.db`**: Passwörter werden nie gelesen oder übernommen. Das geparste
   Profil liegt unter einem Zufalls-Token im Speicher (höchstens vier), die Oberfläche sieht keinen
   Pfad.
9. **Vorschau:** Konten mit Art, Servern, Identitäten und Signaturen (als Text), lokale Ordner mit
   Pfad unter „Local Folders“ und Größe (leere Dateien, `.msf` und andere Nicht-mbox-Dateien bleiben
   draußen; `<Name>.sbd` sind Unterordner; Links werden nicht verfolgt). POP3- und andere Konten werden
   gezeigt, aber nicht übernommen (die App spricht IMAP und JMAP).
10. **Konto übernehmen:** die normale Kontoeinrichtung, vorausgefüllt mit Adresse, Name, IMAP- und
    SMTP-Server; der Mensch tippt das Passwort dort, der Kern prüft die Anmeldung wie immer, das Konto
    startet nur lesend. Eine unverschlüsselte Verbindung in Thunderbird wird STARTTLS (die App sendet
    nie ein Passwort unverschlüsselt) und die Vorschau sagt es. Danach legt
    `applyThunderbirdAccount` die weiteren Identitäten an (die eigene Adresse bekommt Name und
    Reply-To aus Thunderbird) und die Signaturen als Standard für neue Mails und Antworten (HTML durch
    die Allowlist der Signaturen, Text als Editor-HTML). Für ein Konto, das schon eingerichtet ist,
    gibt es nur diesen zweiten Schritt.
11. **Lokale Ordner** gehen über den mbox-Import (4., 5.) in einen Ordner eines Kontos mit
    Schreibzugriff; die Vorschau bietet Konto und Ordner zur Wahl (Archiv vorgewählt).

## Erwogen, nicht gewählt

- **mbox-Bibliothek** (`mail-parser` hat einen einfachen mbox-Leser): ohne Grenzen je Mail und ohne
  Thunderbirds Status; der eigene Push-Leser ist klein und begrenzt den Speicher.
- **Alles auf einmal einreihen:** ein großes Archiv würde die Outbox mit Tausenden Operationen und
  die Platte mit einer zweiten Kopie füllen; das Fenster von 50 hält beides klein.
- **Konten ohne Einrichtung anlegen:** hieße Konten ohne Passwort im Kern; die Einrichtung prüft
  Server und Anmeldung und bleibt der einzige Weg zu einem Konto.

## Folgen

- `docs/ARCHITECTURE.md`: Outbox (Import), Sicherheitsgrenzen (Dateien), Stack bleibt unverändert.
- Ungeprüft bis zu einem Test von Hand: der Windows-Pfad `%APPDATA%\Thunderbird` (die Shell nimmt
  Tauris `config_dir`, unter Windows das Roaming-AppData) und echte Profile anderer Thunderbird-Versionen.
