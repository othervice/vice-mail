# Handprüfung unter Windows 11

Für alles, was vom Mac aus nicht automatisch geprüft werden kann (Stufe 6). Jeder Schritt hat ein
Feld **Ergebnis**: `ok`, oder was stattdessen passiert ist (mit Bildschirmfoto, wenn es hilft).
Weicht etwas ab, einfach weitermachen, solange die folgenden Schritte nicht davon abhängen.

Wichtig für den ganzen Test: nur ein echtes Konto, nur lesend. Den Schreibzugriff nicht
freischalten. Das Passwort gibt nur der Mensch in der App ein.

## 0. Vorbereitung

**0.1 Build-Stand notieren**
- **Tun:** Auf dem Mac `scripts/build-windows.sh` ausführen (oder den letzten Lauf nehmen). Die
  letzten Zeilen zeigen Pfad, Größe und SHA-256 des Installers.
- **Erwartet:** `…/target/x86_64-pc-windows-msvc/release/bundle/nsis/Vice Mail_<Version>_x64-setup.exe`,
  daneben dieselbe Prüfsumme in `….exe.sha256`.
- **Ergebnis:** Version: ____ · SHA-256 (erste 12 Zeichen): ____ · Commit (später unter „Über“): ____

**0.2 Windows notieren**
- **Tun:** `winver` ausführen. Unter Einstellungen → System → Bildschirm die Skalierung ablesen.
  Unter Einstellungen → Apps → Installierte Apps nach „WebView2“ suchen.
- **Erwartet:** Windows 11. „Microsoft Edge WebView2 Runtime“ ist installiert (gehört zu Windows 11).
- **Ergebnis:** Windows-Version/Build: ____ · Skalierung: ____ % · WebView2-Version: ____ ·
  Windows-Sprache: ____ · Hell/Dunkel: ____

## 1. Installation

**1.1 Installer übertragen und prüfen**
- **Tun:** Die `.exe` auf den Windows-Rechner kopieren (z. B. `scp` vom Mac mini, Pfad in
  Anführungszeichen, weil er Leerzeichen enthält). In PowerShell:
  `Get-FileHash -Algorithm SHA256 .\<Datei>.exe`
- **Erwartet:** Die Prüfsumme stimmt mit 0.1 überein.
- **Ergebnis:**

**1.2 SmartScreen**
- **Tun:** Installer per Doppelklick starten.
- **Erwartet:** Der Installer ist nicht signiert (Signierung kommt in Stufe 15). Windows zeigt
  „Der Computer wurde durch Windows geschützt“. Unter „Weitere Informationen“ erscheint
  „Trotzdem ausführen“; danach startet der Installer.
  Ist **Smart App Control** aktiv, blockiert Windows unsignierte Programme ganz, ohne „Trotzdem
  ausführen“. Dann nur notieren und **Smart App Control nicht ausschalten**: Das lässt sich nur durch
  eine Neuinstallation von Windows wieder einschalten.
- **Ergebnis:**

**1.3 Ablauf des Installers**
- **Tun:** Durch den Installer klicken.
- **Erwartet:**
  - Sprache des Installers wie die Windows-Sprache (Deutsch oder Englisch).
  - **Keine** Abfrage der Benutzerkontensteuerung (Installation nur für den aktuellen Benutzer).
  - Zielordner `%LOCALAPPDATA%\Vice Mail`.
  - WebView2 ist vorhanden, also wird nichts heruntergeladen. Fehlt es doch, lädt der Installer
    den WebView2-Bootstrapper von Microsoft und installiert ihn (braucht Internet).
  - Letzte Seite: „Vice Mail ausführen“ und „Desktop-Verknüpfung erstellen“ zum Ankreuzen.
- **Ergebnis:**

**1.4 Startmenü und Liste der Apps**
- **Tun:** Startmenü öffnen, nach dem Produktnamen suchen. Einstellungen → Apps → Installierte Apps.
- **Erwartet:** Ein Eintrag mit dem App-Symbol (blaugrünes Quadrat, weißer Umschlag mit V).
  In „Installierte Apps“ erscheinen Name, Version und Größe.
- **Ergebnis:**

## 2. Start, Fenster und Darstellung

**2.1 Erster Start**
- **Tun:** Die App aus dem Startmenü starten.
- **Erwartet:** Ein Fenster, etwa 1200 × 800, mittig, mit der normalen Windows-Titelleiste (Titel und
  App-Symbol links, drei Schaltflächen rechts). Kein Konsolenfenster. Leere Ansicht ohne
  Beispiel-Mails mit „Konto hinzufügen“. Unter Einstellungen → Über stehen Version, Commit,
  Build-Datum und „Windows“.
- **Ergebnis:**

**2.2 Symbol**
- **Tun:** App-Symbol ansehen: Taskleiste, Alt+Tab, Titelleiste, Startmenü, Desktopverknüpfung.
- **Erwartet:** Überall scharf, nirgends das Tauri-Logo oder ein leeres Standardsymbol.
- **Ergebnis:**

**2.3 Hell und dunkel**
- **Tun:** In der App Einstellungen → Darstellung → Erscheinungsbild nacheinander auf Hell, Dunkel
  und System stellen. Danach bei „System“ den Windows-Modus wechseln (Einstellungen →
  Personalisierung → Farben → Modus).
- **Erwartet:** Die App wechselt sofort. Die **Titelleiste wechselt mit**: dunkel bei Dunkel, hell
  bei Hell, auch wenn Windows auf dem anderen Modus steht. Bei „System“ folgen App und Titelleiste
  dem Windows-Modus ohne Neustart. Texte bleiben überall lesbar.
- **Ergebnis:**

**2.4 Sprache**
- **Tun:** Einstellungen → Allgemein → Sprache: Deutsch, English, System.
- **Erwartet:** Alle Texte wechseln sofort. „System“ folgt der Windows-Anzeigesprache. Datum und
  Uhrzeit in der Liste im passenden Format.
- **Ergebnis:**

**2.5 Schrift**
- **Tun:** Texte in Seitenleiste, Liste und Einstellungen ansehen und mit den Windows-Einstellungen
  vergleichen.
- **Erwartet:** Dieselbe Schrift wie die Windows-Einstellungen (Segoe UI Variable), sauber geglättet.
  Keine Serifenschrift, keine abgeschnittenen Ober- oder Unterlängen (g, j, Ä).
- **Ergebnis:**

**2.6 Titelleiste und Andocken**
- **Tun:**
  - Fenster an der Titelleiste ziehen. Doppelklick auf die Titelleiste.
  - Mit der Maus auf der Maximieren-Schaltfläche verweilen.
  - Win+← / Win+→ / Win+↑ / Win+↓. Fenster an den oberen Bildschirmrand ziehen.
  - Fenster an Rändern und Ecken kleiner ziehen, so klein es geht.
- **Erwartet:** Ziehen und Doppelklick (maximieren und zurück) wie bei jeder Windows-App. Das
  Andock-Layout (Snap Layouts) erscheint und funktioniert. Andocken links/rechts teilt den Bildschirm.
  Das Fenster wird nicht kleiner als etwa 800 × 560; nichts ragt heraus oder überlappt.
- **Ergebnis:**

**2.7 Skalierung 125 % und 150 %**
- **Tun:** Bei laufender App die Skalierung auf 125 %, dann 150 % stellen (Einstellungen → System →
  Bildschirm). Wenn vorhanden: Fenster auf einen zweiten Bildschirm mit anderer Skalierung ziehen.
  Danach die App neu starten.
- **Erwartet:** Texte und Symbole bleiben scharf (nicht verschwommen hochskaliert), Abstände
  stimmen, nichts ist abgeschnitten. Nach dem Wechsel passt sich das Fenster ohne Neustart an.
- **Ergebnis:** 125 %: ____ · 150 %: ____ · zweiter Bildschirm: ____

**2.8 Tastatur**
- **Tun:** Mit Tab durch Seitenleiste, Liste und Einstellungen gehen; in der Liste die Pfeiltasten.
  Tooltips und Menüs auf Tastenkürzel ansehen.
- **Erwartet:** Der Fokus ist immer sichtbar. Nirgends steht „⌘“ oder „Cmd“. (Eigene Tastenkürzel
  gibt es noch nicht; sie kommen in Stufe 8 und zeigen dann Strg bzw. Ctrl.)
- **Ergebnis:**

## 3. Konto einrichten (echtes Konto, nur lesend)

**3.1 Konto hinzufügen**
- **Tun:** „Konto hinzufügen“, Adresse und Passwort eingeben. Ist der Anbieter unbekannt, „Einstellungen
  online suchen“ oder Server von Hand eintragen.
- **Erwartet:** Die Verbindung wird geprüft, das Konto erscheint mit „Nur lesend“. Keine Meldung der
  Windows-Firewall (die App öffnet keinen Port). Ein Zertifikatsfehler erscheint nur, wenn der
  Server wirklich kein gültiges Zertifikat hat.
- **Ergebnis:** Anbieter bzw. Server-Typ (ohne Zugangsdaten): ____

**3.2 Nur lesend**
- **Tun:** Kontodetails öffnen.
- **Erwartet:** Das Konto steht auf „Nur lesend“. Den Schreibzugriff **nicht** einschalten.
- **Ergebnis:**

**3.3 Passwort in der Windows-Anmeldeinformationsverwaltung**
- **Tun:** Systemsteuerung → Anmeldeinformationsverwaltung → Windows-Anmeldeinformationen →
  Generische Anmeldeinformationen. Den Eintrag aufklappen, das Passwort **nicht** anzeigen lassen.
- **Erwartet:** Ein Eintrag `<Konto-UUID>.org.vicemail.desktop`, Benutzername = Konto-UUID,
  Beständigkeit „Lokaler Computer“. Sonst keine Einträge der App.
- **Ergebnis:**

## 4. Ordner und Liste

**4.1 Ordner**
- **Tun:** Seitenleiste ansehen.
- **Erwartet:** Alle Ordner des Servers, Posteingang, Gesendet, Entwürfe, Papierkorb usw. richtig
  erkannt. Umlaute in Ordnernamen richtig.
- **Ergebnis:**

**4.2 Liste**
- **Tun:** Posteingang öffnen, schnell bis ans Ende scrollen. Einen anderen Ordner öffnen.
- **Erwartet:** Die neuesten Mails sind sofort da, ältere kommen nach. Flüssiges Scrollen ohne leere
  Flächen. Absender und Betreffe mit Umlauten und Sonderzeichen richtig.
- **Ergebnis:** Anzahl Mails im Posteingang (ungefähr): ____ · Dauer bis vollständig: ____

**4.3 Neue Mail**
- **Tun:** Vom Handy eine Mail an das Konto schicken.
- **Erwartet:** Sie erscheint nach wenigen Sekunden ohne Zutun oben in der Liste.
- **Ergebnis:**

## 5. Offline

**5.1 Netz trennen**
- **Tun:** WLAN aus bzw. Kabel ziehen (oder Flugzeugmodus).
- **Erwartet:** Die Statusanzeige wechselt auf „Offline“ mit Zeitpunkt des nächsten Versuchs.
  Meist nach Sekunden, spätestens nach etwa 6 Minuten (so lange kann es dauern, bis eine stille
  Verbindung erkannt wird). Liste und Ordner bleiben benutzbar.
- **Ergebnis:** Zeit bis „Offline“: ____

**5.2 Netz wieder verbinden**
- **Tun:** Netz wieder einschalten; einmal abwarten, einmal „Jetzt versuchen“.
- **Erwartet:** Nach höchstens 2 Minuten (sofort mit „Jetzt versuchen“) wieder verbunden, ohne
  erneute Passwortabfrage. Mails, die in der Zwischenzeit kamen, erscheinen.
- **Ergebnis:**

**5.3 Ruhezustand (optional)**
- **Tun:** Rechner in den Energiesparmodus, nach einigen Minuten wecken.
- **Erwartet:** Die App verbindet sich innerhalb weniger Sekunden neu (0008: Uhrensprung über 20 s).
  Unter Windows ist ungeprüft, ob die monotone Uhr im Ruhezustand stehen bleibt; dann greifen nur die
  Fristen aus 5.1.
- **Ergebnis:**

## 6. Neustart

**6.1 Beenden und neu starten**
- **Tun:** App mit dem X schließen. Im Task-Manager (Details) nach `mail-app.exe` suchen. App aus dem
  Startmenü wieder starten.
- **Erwartet:** Kein Prozess bleibt zurück. Nach dem Start sind Konto, Ordner und Mails sofort da,
  ohne Passwortabfrage. Erscheinungsbild und Sprache sind wie eingestellt, die Titelleiste passt
  schon beim Öffnen.
- **Ergebnis:**

**6.2 Datenorte**
- **Tun:** Im Explorer `%LOCALAPPDATA%\org.vicemail.desktop` und `%APPDATA%\org.vicemail.desktop` öffnen.
- **Erwartet:** Unter `%LOCALAPPDATA%\org.vicemail.desktop` liegen `mail.sqlite3` (mit `-wal`/`-shm`),
  `logs\` und `EBWebView\`. Unter `%APPDATA%` gibt es den Ordner nicht (oder er ist leer).
- **Ergebnis:**

## 7. Deinstallation

**7.1 Konto entfernen**
- **Tun:** In der App Einstellungen → Konten → Konto → „Konto entfernen …“ und bestätigen. Danach die
  Anmeldeinformationsverwaltung neu öffnen.
- **Erwartet:** Das Konto ist weg, der Eintrag `<Konto-UUID>.org.vicemail.desktop` auch.
- **Ergebnis:**

**7.2 Deinstallieren**
- **Tun:** App beenden. Einstellungen → Apps → Installierte Apps → App → Deinstallieren. Im
  Deinstallationsprogramm „Lösche die Anwendungsdaten“ (englisch „Delete the application data“)
  ankreuzen.
- **Erwartet:** Keine Abfrage der Benutzerkontensteuerung. Danach sind weg: der Startmenü-Eintrag,
  die Desktopverknüpfung, `%LOCALAPPDATA%\Vice Mail` und `%LOCALAPPDATA%\org.vicemail.desktop`.
- **Ergebnis:**

**7.3 Reste**
- **Tun:** Anmeldeinformationsverwaltung und `%APPDATA%`/`%LOCALAPPDATA%` noch einmal ansehen.
- **Erwartet:** Nichts von der App. Bekannte Lücke: Wurde ein Konto **nicht** vorher in der App
  entfernt, bleibt sein Passwort in der Anmeldeinformationsverwaltung stehen; der Deinstaller
  löscht es nicht. Dann den Eintrag von Hand entfernen und hier notieren.
- **Ergebnis:**

**7.4 Erneut installieren (optional)**
- **Tun:** Installer noch einmal ausführen und starten.
- **Erwartet:** Leere Ansicht wie bei 2.1, keine alten Konten.
- **Ergebnis:**

## 8. Lesen und Links (Stufe 3)

**8.1 HTML-Mail**
- **Tun:** Eine Newsletter-Mail mit Bildern öffnen.
- **Erwartet:** Die Mail erscheint im Lesebereich, externe Bilder sind blockiert; ein Hinweis bietet
  „Bilder laden“ (einmal, für diesen Absender). Nach der Freigabe erscheinen die Bilder. Keine
  Ruckler beim Scrollen, die Höhe des Mail-Rahmens passt zum Inhalt.
- **Ergebnis:**

**8.2 Links**
- **Tun:** Auf einen Link in der Mail klicken, danach auf einen `mailto:`-Link.
- **Erwartet:** Vor dem Öffnen zeigt die App das echte Ziel (bei abweichendem Linktext eine Warnung).
  Der Link öffnet im Standardbrowser, nie im App-Fenster. `target=_blank` und `window.open` öffnen
  kein Fenster. (WebView2-Wächter, `docs/decisions/0011-webview2-frame-navigation.md`.)
- **Ergebnis:**

**8.3 Anhang**
- **Tun:** Einen Anhang speichern (Windows-Dialog) und einen öffnen.
- **Erwartet:** Der Dateiname bleibt erhalten (Umlaute), unzulässige Zeichen sind ersetzt. Ausführbare
  Dateien öffnet die App nicht direkt, sie fragt nach.
- **Ergebnis:**

## 9. Aktionen und Schreibzugriff (Stufe 4)

Das echte Konto ist nur lesend. Für diese Schritte ein Nebenkonto nehmen oder sie auslassen.

**9.1 Nur lesend**
- **Tun:** Eine Mail auswählen und `E` (Archivieren) bzw. `Entf` drücken.
- **Erwartet:** Nichts ändert sich auf dem Server. Die App erklärt, dass das Konto nur lesend ist, und
  zeigt den Weg zum Schalter in den Kontoeinstellungen.
- **Ergebnis:**

**9.2 Schreibzugriff freischalten (nur Nebenkonto)**
- **Tun:** Einstellungen → Konto → „Änderungen erlauben“, Erklärung lesen, einschalten.
- **Erwartet:** Der Schalter ist danach an. Andere Konten bleiben aus.
- **Ergebnis:**

**9.3 Aktionen**
- **Tun:** Als gelesen/ungelesen markieren, markieren (Fahne), archivieren, in den Papierkorb,
  verschieben (auch per Ziehen auf einen Ordner), mit Umschalt mehrere auswählen. Nach jeder Aktion
  „Rückgängig“ im Hinweis probieren.
- **Erwartet:** Die Liste reagiert sofort; im Webmail des Anbieters ist die Änderung nach wenigen
  Sekunden zu sehen. „Rückgängig“ stellt den Zustand wieder her. Im Papierkorb fragt „Endgültig
  löschen“ nach.
- **Ergebnis:**

**9.4 Offline und Aktivität**
- **Tun:** WLAN aus, zwei Mails archivieren, WLAN wieder an. Danach die Ansicht „Aktivität“ öffnen.
- **Erwartet:** Offline sind die Mails sofort archiviert und mit einem kleinen Symbol als ausstehend
  markiert. Nach dem Verbinden werden sie ausgeführt; „Aktivität“ zeigt sie als erledigt.
- **Ergebnis:**

## 10. KI über MCP (Stufe 12, wenn ein MCP-Client vorhanden ist)

- **Tun:** Einstellungen → KI über MCP einschalten, ein Konto freigeben, die angezeigte Konfiguration
  in Claude Desktop oder Claude Code eintragen. Den Client nach den letzten Mails fragen.
- **Erwartet:** Der Client findet `mail-mcp.exe` neben der App; er sieht nur das freigegebene Konto.
  Jeder Zugriff erscheint in der App unter „Zugriffe“. Not-Aus trennt den Client sofort.
- **Ergebnis:**

## Notizen und „Wartet auf“

_Platz für alles, was nicht in die Schritte passt._
