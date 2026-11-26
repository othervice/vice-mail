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

## 11. Schreiben und Senden (Stufe 5)

Senden nur von einem Konto mit freigeschaltetem Schreibzugriff (Abschnitt 9.2), am besten an die
eigene Adresse.

**11.1 Neue Mail und Entwurf**
- **Tun:** „Neue Nachricht“ (Strg+N), Empfänger, Betreff und Text eingeben, eine Datei anhängen
  (Dialog und Ziehen aus dem Explorer), den Composer schließen.
- **Erwartet:** Der Entwurf bleibt erhalten und liegt nach wenigen Sekunden genau einmal im
  Entwürfe-Ordner des Servers. Wieder öffnen (Doppelklick oder Enter im Ordner „Entwürfe“) zeigt
  denselben Stand.
- **Ergebnis:**

**11.2 Senden mit Verzögerung**
- **Tun:** Senden (Strg+Enter), im Hinweis sofort „Rückgängig“; danach erneut senden und warten.
- **Erwartet:** „Rückgängig“ öffnet den Entwurf wieder, nichts wird gesendet. Beim zweiten Mal kommt
  die Mail genau einmal an, eine Kopie liegt in „Gesendet“. Umlaute und Emoji in Betreff und
  Dateiname bleiben erhalten.
- **Ergebnis:**

**11.3 Antworten und Weiterleiten**
- **Tun:** Auf die angekommene Mail antworten (R), allen antworten (A), weiterleiten (F).
- **Erwartet:** Zitat und Empfänger stimmen, die Antwort erscheint im selben Gesprächsverlauf.
  Die Ansicht „Ausgang“ zeigt laufende und fehlgeschlagene Sendungen.
- **Ergebnis:**

## 12. Suche und mehrere Konten (Stufe 7)

**12.1 Suche**
- **Tun:** Suche öffnen (Strg+F oder /), einen Begriff, dann `from:` und einen Zeitraum eingeben;
  „Auf dem Server suchen“ ausprobieren.
- **Erwartet:** Treffer erscheinen beim Tippen, Fundstellen sind markiert; Tippfehler in der
  Suchsprache werden erklärt.
- **Ergebnis:**

**12.2 Gespräche und vereinter Posteingang (mit zwei Konten)**
- **Tun:** In „Gesendet“ oder „Archiv“ die Gesprächsansicht einschalten; zweites Konto hinzufügen
  und den vereinten Posteingang öffnen.
- **Erwartet:** Zusammengehörige Mails stehen in einer Zeile mit Anzahl. Im vereinten Posteingang
  trägt jede Zeile die Farbe ihres Kontos; ausgeblendete Konten fehlen.
- **Ergebnis:**

## 13. Sortierter Posteingang (Stufe 9, Teil A)

- **Tun:** Posteingang ansehen; ein Bündel (Newsletter, Benachrichtigungen) aufklappen; eine Mail
  anheften (P), auf später legen (B) und als Aufgabe markieren (T); „Warum hier?“ im Lesebereich.
- **Erwartet:** Angeheftete und zurückgekehrte Mails stehen oben, Bündel klappen an Ort und Stelle
  auf. Anheften, Später und Aufgabe ändern nichts auf dem Server (auch nicht bei nur lesendem
  Konto). Die Ansichten „Später“, „Angeheftet“ und „Aufgaben“ zeigen die Mails.
- **Ergebnis:**

## 14. Einstellungs-Sync (Stufe 11, braucht zwei Geräte)

Das Einschalten erlaubt das Schreiben nur in den einen Ordner `Settings-Sync` des gewählten Kontos.

- **Tun:** Auf dem Mac oder Windows: Einstellungen → Einstellungs-Sync für ein Konto einschalten,
  Passphrase vergeben. Auf dem zweiten Gerät dasselbe Konto einrichten, dem Sync beitreten.
- **Erwartet:** Vorschau zeigt, was übernommen wird; danach gleichen Design, Sprache, Signaturen
  und Kontengruppen ab. Passwörter werden nicht übertragen, das zweite Gerät fragt danach. Eine
  falsche Passphrase ändert nichts.
- **Ergebnis:**

## 15. Bedienung (Stufe 8, ab Prototyp 2)

**15.1 Command Palette und Kürzel**
- **Tun:** Strg+K, „arch“ tippen, Enter; danach Einstellungen → Tastatur ein Kürzel ändern und eine
  schon belegte Taste ausprobieren; App neu starten.
- **Erwartet:** Die Palette findet die Aktion und führt sie aus. Das geänderte Kürzel wirkt sofort und
  nach dem Neustart; eine Doppelbelegung wird mit dem Namen des anderen Befehls abgelehnt. Windows-
  Systemkürzel (Alt+F4, Win+…) lassen sich nicht belegen.
- **Ergebnis:**

**15.2 Layouts und eigene Fenster**
- **Tun:** Einstellungen → Darstellung: Lesebereich rechts, unten, statt der Liste; kompakte Zeilen;
  Textgröße 150 %. Eine Mail über die Palette „In neuem Fenster öffnen“, einen Entwurf „Im eigenen
  Fenster“ öffnen; Fenster mit Windows+Pfeil andocken, Skalierung 150 % in Windows probieren.
- **Erwartet:** Alles bleibt bedienbar, nichts wird abgeschnitten. Links in der Mail im eigenen
  Fenster werden genauso geprüft wie im Hauptfenster. Der Composer im eigenen Fenster schließt sich
  nach dem Senden.
- **Ergebnis:**

## 16. Posteingang-Komfort Teil B (Stufe 9, ab Prototyp 2)

- **Tun:** Einen Newsletter mit „Abmelden“-Leiste öffnen und die Frage abbrechen; eine
  Kalendereinladung öffnen; eine Mail „Als .eml speichern“ und die Datei im Explorer doppelklicken;
  eine Mail drucken (Strg+P, auch „Als PDF“); Einstellungen → Allgemein: Standard-Mail-App;
  im Browser einen `mailto:`-Link anklicken.
- **Erwartet:** Ohne Klick geht keine Abmelde-Anfrage hinaus. Die Einladung zeigt Zeit und Ort
  richtig (auch Sommerzeit). Die `.eml`-Datei öffnet sich nur lesend in einem eigenen Fenster. Der
  Ausdruck zeigt nur die Mail, hell. Windows öffnet für die Standard-App die Systemeinstellungen; ist
  die App Standard, öffnet der `mailto:`-Link einen Entwurf.
- **Ergebnis:**

## 17. Composer vollständig (Stufe 10, ab Prototyp 2, Nebenkonto mit Schreibzugriff)

- **Tun:** Einstellungen → Schreiben: eine HTML-Signatur mit Bild anlegen, eine Vorlage mit
  `{{firstName}}`-Platzhalter. Neue Mail: Überschrift, Aufzählung, durchgestrichen, ein Bild
  einfügen (Dialog und Ziehen), Text aus Word oder einer Webseite einfügen, Vorlage einfügen,
  Rechtschreibung prüfen (rote Wellenlinie, Sprache). „Später senden“ in zwei Minuten, App offen
  lassen. Eine Mail „Als Anhang weiterleiten“.
- **Erwartet:** Beim Empfänger (auch bei einem anderen Anbieter) sehen Formatierung, Bild und
  Signatur richtig aus. Eingefügte Inhalte enthalten keine fremden Bilder oder Skripte. Die geplante
  Mail geht pünktlich hinaus, solange die App läuft. Die Rechtschreibprüfung von WebView2 arbeitet
  in der eingestellten Sprache.
- **Ergebnis:**

## 18. KI in der App (Stufe 13, ab Prototyp 2, optional)

- **Tun:** Ollama installieren (`ollama pull llama3.2` o. ä.) oder einen API-Schlüssel in der App
  eingeben (nie anderswo). Einstellungen → KI in der App: einschalten, Anbieter anlegen, Modelle
  laden, Verbindung testen, Funktionen zuordnen. Ein Gespräch zusammenfassen, einen Antwortvorschlag
  als Entwurf öffnen, die Schreibhilfe auf eine Auswahl anwenden, eine Frage zu den Mails stellen.
  Bei einem Cloud-Anbieter: Zustimmung lesen.
- **Erwartet:** Ohne Einschalten passiert nichts. Vor der ersten Cloud-Anfrage erscheint die
  Zustimmung mit Größe und Ziel; jede Anfrage zeigt Mails, Größe und Ziel. Ohne Internet funktioniert
  Ollama weiter. Aussagen verweisen auf ihre Mails; Links in Antworten werden vor dem Öffnen geprüft.
  Der Schlüssel erscheint nirgends in der Oberfläche.
- **Ergebnis:**

## 19. Neuere Version installieren (ab Prototyp 2) und Updates in der App (Stufe 15, 0037)

- **Tun (von Hand):** Den Installer einer neueren Version über eine installierte ältere starten.
  Auf der Seite „Bereits installiert“ die vorgewählte Option „Vor der Installation deinstallieren“
  lassen, „Weiter“; im Deinstallationsfenster „Lösche die Anwendungsdaten“ **nicht** ankreuzen.
- **Erwartet:** Ein Durchgang erledigt beides; danach startet die neue Version mit allen Konten,
  Mails und Einstellungen (Passwörter in der Anmeldeinformationsverwaltung unverändert).
- **Tun (in der App, erst nach dem ersten öffentlichen Release):** Einstellungen → Über: Schalter
  ist aus; „Jetzt suchen“, „Update herunterladen“, „Neu starten und installieren“.
- **Erwartet:** Ohne Klick keine Verbindung. Der Installer läuft mit Fortschrittsbalken, ohne
  Fragen und ohne die Seite „Bereits installiert“; danach startet die App in der neuen Version mit
  allen Daten. SmartScreen fragt beim Update nicht (der Installer wird nicht aus dem Browser
  geladen); falls doch, notieren.
- **Ergebnis:**

## 20. Funktionsabgleich (Stufe 16, ab Prototyp 4)

**20.1 Ordnen**
- **Tun:** Eine Mail auf die Lese-Liste setzen (Q) und die Ansicht „Lese-Liste“ öffnen; im
  Listenkopf „Nur ungelesene“ einschalten; einen Ordner über sein „…“-Menü zum Favoriten machen und
  ihm eine Farbe geben; einen Absender blockieren und wieder freigeben.
- **Erwartet:** Nichts davon ändert etwas auf dem Server. Favoriten stehen oben in der
  Seitenleiste, die Farbe steht immer neben dem Namen. Mails blockierter Absender verschwinden aus
  dem Posteingang, ein Hinweis nennt ihre Zahl.
- **Ergebnis:**

**20.2 Zwischen Konten verschieben (zwei Nebenkonten mit Schreibzugriff)**
- **Tun:** Eine Mail auf einen Ordner des anderen Kontos ziehen; im Dialog „Verschieben“ wählen.
  Danach eine zweite „Kopieren“.
- **Erwartet:** Der Dialog erklärt, was passiert. Die Mail liegt danach genau einmal im Ziel; beim
  Verschieben verschwindet sie erst danach aus der Quelle. „Aktivität“ zeigt den Fortschritt.
- **Ergebnis:**

**20.3 Schnellantwort, Suchen, Anhänge**
- **Tun:** Unter einer geöffneten Mail in das Antwortfeld tippen und mit Strg+Enter senden; in einer
  langen Mail Strg+F drücken und ein Wort suchen; bei einer Mail mit Bild, PDF und Textanhang die
  Vorschau öffnen (Leertaste), „Alle speichern“, einen Anhang anheften.
- **Erwartet:** Die Antwort geht mit der üblichen Verzögerung hinaus. Die Suche markiert Treffer auch
  im Mailtext, Enter springt weiter, Escape schließt. Die Vorschau zeigt Bild, PDF-Seiten und Text
  ohne Verzögerung; HTML- oder SVG-Anhänge haben keine Vorschau. „Alle speichern“ überschreibt keine
  vorhandene Datei.
- **Ergebnis:**

**20.4 Lesebestätigung, Töne, Farbthemen**
- **Tun:** Eine Mail mit „Lesebestätigung anfordern“ an dich selbst senden und beim Empfang „Nicht
  senden“ wählen; Benachrichtigungston einstellen und eine neue Mail abwarten; unter Darstellung die
  Farbthemen durchprobieren.
- **Erwartet:** Ohne Klick auf „Bestätigung senden“ wird nie eine Bestätigung gesendet. Der Ton ist
  zu hören (außer in Ruhezeiten). Jedes Farbthema ist in Hell und Dunkel gut lesbar.
- **Ergebnis:**

## 21. Hintergrund, Symbol im Infobereich, Autostart, scharfer Zähler (Stufe 21, 0068–0070)

**21.1 Symbole und Zähler bei jeder Skalierung**
- **Tun:** Unter Einstellungen → System → Anzeige die Skalierung nacheinander auf 100, 125, 150
  und 200 % stellen. Jeweils: das App-Symbol in der Taskleiste, in der Titelleiste und im
  Infobereich ansehen; eine neue Mail abwarten (oder eine als ungelesen markieren) und die Zahl am
  Taskleisten-Knopf und am Symbol im Infobereich ansehen. Einmal mehr als 99 ungelesene Mails
  (Testkonto).
- **Erwartet:** Logo blau, scharf, ohne weiche Kanten, auch nach dem Wechsel der Skalierung bei
  laufender App. Ziffern weiß auf Rot, jedes Pixel klar, keine grauen Ränder; über 99 steht „99+“.
  Tooltip des Symbols: „Vice Mail: N ungelesen“. (Ab 0.1.3 sehen die Ziffern anders aus: 22.1.)
- **Ergebnis:** (Bildschirmfotos bei 100 und 150 % beilegen)

**21.2 Fenster schließen, im Hintergrund weiter, wieder öffnen**
- **Tun:** Das Hauptfenster mit dem X schließen. Im Hinweis „Weiterlaufen“ wählen. Eine Mail an das
  Konto senden. Auf „Lesen“ im Popup klicken. Fenster erneut schließen, dann Linksklick auf das
  Symbol im Infobereich; erneut schließen, dann Vice Mail aus dem Startmenü starten.
- **Erwartet:** Der Hinweis kommt nur beim ersten Mal. Ohne Fenster ist kein Taskleisten-Knopf zu
  sehen, das Symbol im Infobereich bleibt. Das Popup erscheint auch ohne Fenster; „Lesen“, der
  Klick auf das Symbol und der zweite Start öffnen das Fenster an seinem alten Platz, „Lesen“ mit
  der Mail. Task-Manager: nur ein Prozess „Vice Mail“ (plus WebView2), ohne Fenster deutlich
  weniger Arbeitsspeicher (Spalte „Arbeitsspeicher (privater Arbeitssatz)“ vorher/nachher
  notieren).
- **Ergebnis:**

**21.3 Menü des Symbols**
- **Tun:** Rechtsklick auf das Symbol im Infobereich: „Neue Nachricht“; dann „Mitteilungen
  pausieren › Für 1 Stunde“, eine Mail senden, „Mitteilungen fortsetzen“, noch eine senden;
  zuletzt „Vice Mail beenden“.
- **Erwartet:** Menü in der Sprache der App. „Neue Nachricht“ öffnet das Fenster mit einem leeren
  Entwurf. Während der Pause kein Popup, kein Ton, die Zahl ändert sich trotzdem; im Menü steht
  „Pausiert bis HH:MM“. Nach dem Fortsetzen kommt das Popup für die zweite Mail, nicht für die
  erste. „Beenden“ beendet den Prozess (Task-Manager).
- **Ergebnis:**

**21.4 Mit dem System starten**
- **Tun:** Einstellungen → Allgemein → „Mit dem System starten“ einschalten. Task-Manager →
  Autostart-Apps ansehen. Abmelden und wieder anmelden. Dann dort im Task-Manager deaktivieren und
  die Einstellung in der App erneut ansehen; in der App wieder einschalten. Zuletzt deinstallieren.
- **Erwartet:** „Vice Mail“ steht unter Autostart-Apps als aktiviert. Nach der Anmeldung läuft die
  App ohne Fenster (Symbol im Infobereich, Popup bei neuer Mail). Im Task-Manager deaktiviert zeigt
  die App den Schalter aus; in der App eingeschaltet ist es im Task-Manager wieder aktiviert. Nach
  dem Deinstallieren ist der Eintrag weg (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
  Ein Update (neuere Version drüber installieren) lässt den Eintrag stehen.
- **Ergebnis:**

## 22. Windows-11-Optik und Zähler mit echter Schrift (Stufe 22, 0079, 0080)

**22.1 Zähler: Ziffern wie Segoe UI, scharf**
- **Tun:** Wie 21.1 bei 100, 125, 150 und 200 %: eine, zwei und mehr als 99 ungelesene Mails
  (z. B. 7, 42, 100+), am Taskleisten-Knopf und am Symbol im Infobereich; jeweils mit der
  Bildschirmlupe (Win+Plus) vergrößert ansehen. Dazu die Vergleichsbilder vom Mac danebenlegen.
- **Erwartet:** Echte Ziffern (Form wie Segoe UI, halbfett), weiß auf Rot, gerade Striche voll
  weiß ohne graue Spalte, nur Bögen und Schrägen mit weichen Pixeln; Ziffern mittig, mit rotem
  Rand rundherum. Taskleiste: eine Ziffer im Kreis, zwei Ziffern und „99+“ in einem Rechteck mit
  runden Ecken. Infobereich: Kreis oder Pille unten rechts, eine Ziffer nicht kleiner als zwei.
  Die Zahl springt beim Wechsel von 41 auf 42 nicht in der Größe. Nach dem Wechsel der Skalierung
  bei laufender App sofort wieder scharf.
- **Ergebnis:** (Bildschirmfotos bei 100 und 150 % beilegen)

**22.2 Symbol nach dem Update blau**
- **Tun:** Vorher notieren, welche Version läuft (Einstellungen → Über) und ob Vice Mail an die
  Taskleiste angeheftet ist. 0.1.3 über die laufende Version installieren (Update in der App oder
  Installer). Danach Taskleiste, Startmenü, Desktop-Verknüpfung und Alt+Tab ansehen, ohne
  abzumelden.
- **Erwartet:** Überall das blaue Symbol, auch an der angehefteten Verknüpfung, spätestens wenige
  Sekunden nach dem Installieren. Falls die angeheftete noch orange ist: lösen und neu anheften,
  und notieren, dass `SHChangeNotify` nicht gereicht hat (dann hilft `ie4uinit.exe -show`).
- **Ergebnis:** Version vorher: ____ · angeheftet: ja/nein · Symbol danach: ____

**22.3 Mica, Titelleiste, Andock-Layouts**
- **Tun:** Unter Einstellungen → Personalisierung → Farben „Transparenzeffekte“ einschalten. Vice
  Mail öffnen, in der App Darstellung Hell, dann Dunkel, dann System wählen; das Fenster über ein
  helles, ein dunkles und ein buntes Hintergrundbild schieben. Die Spalte der Konten und Ordner
  einklappen und wieder aufklappen, das Fenster schmal ziehen, bis die Spalte über dem Inhalt
  liegt. Die Maus über den Maximieren-Knopf halten. Eine Mail in eigenem Fenster und einen
  Entwurf im Fenster öffnen.
- **Erwartet:** Titelleiste und linker Rand (Rahmen, schmale Leiste oder Spalte) sind ein Stück
  Mica, ohne Naht, hell oder dunkel wie die App und beim Umschalten sofort mit ihr. Liste,
  Lesebereich, Menüs und Dialoge undurchsichtig wie bisher; über der Liste liegende Spalte
  undurchsichtig. Text in der Spalte gut lesbar über jedem Hintergrundbild. Beim Größenändern kein
  Flackern, keine schwarzen oder weißen Ränder. Am Maximieren-Knopf erscheinen die Andock-Layouts.
  Mail- und Entwurfsfenster: Titelleiste in Mica, Inhalt wie bisher. Popup für neue Mails
  unverändert.
- **Ergebnis:** (Bildschirmfotos hell und dunkel beilegen)

**22.4 Ohne Transparenz, mit Kontrastdesign**
- **Tun:** Bei offenem Fenster „Transparenzeffekte“ ausschalten und ansehen; Vice Mail schließen
  und neu öffnen. Dann unter Barrierefreiheit → Kontrastdesigns eines wählen (z. B. „Wüste“) und
  wieder „Keines“.
- **Erwartet:** Ohne Transparenz sofort feste Flächen in den Farben der App (die Titelleiste in
  einer festen Farbe von Windows; gibt WebView2 die Einstellung nicht weiter, ist auch die Spalte
  in dieser festen Farbe, nicht durchsichtig: bitte notieren, welches von beiden), nach dem
  Neustart wie in 0.1.2. Im Kontrastdesign die Farben
  des Designs, nichts durchsichtig, alles lesbar. Wieder eingeschaltet: im nächsten Fenster
  wieder Mica.
- **Ergebnis:**

## 23. Ziehen mit der Maus (Stufe 23, 0082)

Unter 0.1.3 ging unter Windows kein Ziehen in der App. Bitte mit der echten Maus (oder dem
Touchpad) prüfen, nicht mit der Tastatur. Zwei Konten einrichten, beide mit Schreibzugriff.

**23.1 Konten umsortieren in der Spalte und in der schmalen Leiste**
- **Tun:** In der Spalte links die Kopfzeile des zweiten Kontos nehmen und langsam über das erste
  ziehen, ohne loszulassen; dann in die obere Hälfte des ersten Kontos und loslassen. Danach
  dasselbe zurück, aber vor dem Loslassen Esc drücken. Ein Konto außerhalb der Spalte (über der
  Mail-Liste) loslassen. Die Spalte einklappen und in der schmalen Leiste einen Farbpunkt über den
  anderen ziehen und loslassen. Vice Mail beenden („Beenden“ im Symbol im Infobereich) und neu
  starten.
- **Erwartet:** Beim Ziehen eine blaue Linie (4 px) in der Lücke, in die das Konto käme; das
  gezogene Konto bleibt halb durchsichtig an seinem Platz. Loslassen ordnet es dort ein, auch in der
  Leiste. Esc bricht ab, außerhalb der Liste passiert nichts. Nach dem Neustart dieselbe
  Reihenfolge überall (Spalte, Leiste, Einstellungen → Konten).
- **Ergebnis:**

**23.2 Mails auf Ordner ziehen (verschieben, mit Strg kopieren)**
- **Tun:** Eine Mail aus dem Posteingang auf einen eigenen Ordner ziehen und loslassen. Eine
  zweite mit gedrückter Strg-Taste auf denselben Ordner ziehen. Aus „Alle Posteingänge“ eine Mail
  auf einen Ordner des anderen Kontos ziehen. Eine Mail auf einen zugeklappten Ordner ziehen und
  dort kurz halten. Eine Mail auf „Papierkorb“ in der schmalen Leiste ziehen.
- **Erwartet:** Beim Überfahren ist der Ordner hervorgehoben, daneben „1 Mail nach … verschieben“
  bzw. mit Strg „… kopieren“ (der Zeiger zeigt das Pluszeichen). Loslassen verschiebt bzw. kopiert,
  danach ein Hinweis (im selben Konto mit „Rückgängig“, sonst „Aktivität zeigen“); der zugeklappte
  Ordner klappt nach einem Moment auf. Der Papierkorb der Leiste nimmt die Mail in den Papierkorb
  ihres eigenen Kontos.
- **Ergebnis:**

**23.3 Datei aus dem Explorer auf eine neue Mail**
- **Tun:** Eine neue Mail öffnen (im Fenster und als Karte über dem Lesebereich). Aus dem
  Explorer eine PDF-Datei und ein Foto (JPEG/PNG) zusammen auf das Fenster ziehen, erst nur
  darüber halten, dann loslassen. Dasselbe mit „Reiner Text“. Einen Ordner aus dem Explorer auf die
  Mail ziehen. Eine Datei auf die Mail-Liste ziehen, während keine neue Mail offen ist. Unter
  Einstellungen → Schreiben eine Signatur bearbeiten und ein kleines PNG darauf ziehen.
- **Erwartet:** Über den Anhängen der Mail ein gestrichelter Streifen „Lass die Dateien los, um sie
  anzuhängen.“ (eine verkleinerte Mail kommt dabei zurück). Formatiert:
  das Foto steht im Text, die PDF als Anhang; „Reiner Text“: beide als Anhang. Ein Ordner wird
  nicht angehängt, eine Meldung sagt es. Ohne offene Mail zeigt der Zeiger „nicht erlaubt“, nichts
  öffnet sich, Vice Mail bleibt, wie es war. Das PNG erscheint in der Signatur.
- **Ergebnis:**

**23.4 Neue Mail als Karte (Stufe 23, 0086)**
- **Tun:** Eine Mail im Lesebereich öffnen, dann „Neue Mail“. Mit der Maus eine andere Mail in der
  Liste anklicken, die Grenze zwischen Liste und Lesebereich ziehen. In der Karte jedes Menü
  öffnen: „Von“ (bei zwei Absendern), Absatzformat, „Weitere Formatierung“, den Pfeil an „Senden“,
  Erinnern, „…“. Mit Tab und Umschalt+Tab durch die Karte gehen, ein Menü mit ↓ öffnen und mit Esc
  schließen, dann Esc in der Karte. „Verkleinern“, eine Mail lesen, die Leiste unten anklicken.
  Eine Antwort öffnen und „Zitat anzeigen“. Im dunklen Design und mit Mica wiederholen.
- **Erwartet:** Die Karte liegt mit runden Ecken und Schatten über dem Lesebereich, nicht
  durchsichtig, die Liste daneben bleibt bedienbar, die Karte folgt der Grenze. Menüs öffnen nach
  oben, wenn unten kein Platz ist, nichts wird abgeschnitten. Tab bleibt in der Karte, Esc im Menü
  schließt nur das Menü, Esc in der Karte schließt sie (der Entwurf bleibt). Verkleinert eine
  Leiste unten rechts; ein Klick bringt die Karte mit dem Text zurück. Das Zitat ist erst
  eingeklappt.
- **Ergebnis:**

## 24. Seitenleiste nach Konten (Stufe 23, 0085)

Zwei oder drei Konten einrichten; Windows 11 mit Transparenzeffekten (Mica) und ohne.

**24.1 Aufbau, Mica, Schrift**
- **Tun:** Vice Mail öffnen, die Spalte links ansehen (hell und dunkel, ein anderes Farbthema).
  „Mehr“ und „Weitere Ordner“ eines Kontos aufklappen, mit der Maus über Zeilen fahren. Das Fenster
  schmal ziehen (Spalte wird zur Leiste) und wieder breit.
- **Erwartet:** Oben „Alle Konten“ mit Posteingang (Zahl halbfett), Später, Markiert, Gesendet,
  „Mehr“; dann „Favoriten“; je Konto ein Kopf mit farbigem Quadrat und Namen, Posteingang und
  Gesendet mit Symbolen in der Kontofarbe, „Weitere Ordner“. Unten „Abgeglichen um …“, ein
  Pfeilkreis und ein Zahnrad. Mica scheint durch die Spalte, der Text bleibt gut lesbar, Inter
  sieht aus wie auf dem Mac. Die gewählte Zeile ist grau hinterlegt und halbfett, ohne blauen
  Balken. In der schmalen Leiste stehen die ungelesenen Mails unter dem Punkt jedes Kontos.
- **Ergebnis:**

**24.2 Gemerkt, Abgleichen, Tastatur**
- **Tun:** „Mehr“ öffnen, „Weitere Ordner“ eines Kontos öffnen, ein Konto zuklappen; Vice Mail
  beenden und neu starten. „Jetzt abgleichen“ anklicken. Mit Tab durch die Spalte gehen, mit den
  Pfeiltasten in „Alle Konten“ und in einem Konto bewegen, → und ← auf „Mehr“.
- **Erwartet:** Nach dem Neustart ist alles so offen oder zu wie vorher. Nach „Jetzt abgleichen“
  steht kurz „Gleicht ab …“, danach die neue Uhrzeit. Tab hält in „Alle Konten“, bei den Favoriten,
  bei jedem Konto und im Fuß je einmal; → und ← klappen „Mehr“ auf und zu.
- **Ergebnis:**

## Notizen und „Wartet auf“

_Platz für alles, was nicht in die Schritte passt._
