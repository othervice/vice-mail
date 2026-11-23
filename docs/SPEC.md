# Produktbeschreibung

## Rahmen

- Ein schneller, ruhiger E-Mail-Client für macOS und Windows, der im Alltag angenehmer ist als
  Apple Mail oder Thunderbird.
- Open Source, kostenlos, für jeden nutzbar. Zuerst für den Eigengebrauch, danach öffentlich.
- Kein eigener Cloud-Dienst, kein Benutzerkonto bei uns, keine Telemetrie, keine Werbung.
  Alles, was die App braucht, liegt auf dem Gerät oder im eigenen Postfach.
- Plattformen: macOS auf Apple Silicon (Intel, wenn der Build ohne Mehraufwand geht) und
  Windows 11 x64. Windows ARM64, Linux und Mobilgeräte später; die Architektur verbaut sie nicht.

## Grundsätze

1. **Schnell:** Jede Grundaktion reagiert sofort. Die Oberfläche wartet nie auf das Netz.
2. **Ruhig:** Eine Zeile pro Mail, wenig Farbe, keine Animation ohne Zweck.
3. **Privat:** Externe Inhalte sind blockiert, bis der Nutzer sie freigibt. Nichts verlässt das Gerät ungefragt.
4. **Zuverlässig:** Keine verlorene Mail, kein doppelter Versand, kein stiller Fehler.
5. **KI optional:** Alles funktioniert ohne KI. KI macht nur Vorschläge und führt nie selbst Aktionen aus.

## Konten

| Kontotyp | Wann | Hinweis |
|---|---|---|
| IMAP/SMTP allgemein, eigene Server | Stufe 2 | Kern des Produkts |
| iCloud Mail | Stufe 2 | über IMAP mit App-Passwort |
| Gmail | Stufe 2 | über IMAP mit App-Passwort; Google-Login erst mit Googles Verifizierung |
| Microsoft 365, Outlook.com | Stufe 14 | braucht OAuth und eine App-Registrierung bei Microsoft |
| JMAP (Fastmail, Stalwart) | Stufe 14 | optional |
| Exchange ohne IMAP/Graph (EWS, MAPI) | nie geplant | nicht als unterstützt ausweisen |

Mehrere Konten gleichzeitig, jedes mit eigener Farbe. Die Herkunft jeder Mail ist immer sichtbar.

## Funktionsumfang

### Posteingang und Liste
- Vereinter Posteingang über alle Konten, einzelne Konten, selbst definierte Kontogruppen; Konten ausblendbar.
- Virtualisierte Liste, die auch bei 500.000 Mails flüssig bleibt.
- Zeile: Absender, Betreff, Vorschau, Zeit, ungelesen, Anhang, Markierung, Kontofarbe.
- Gruppierung nach Konversation, abschaltbar.
- Kategorien ohne KI: Personen, Benachrichtigungen, Newsletter, dazu ein Block „Priorität“ für
  wichtige Absender. Gebündelte Zeilen für Benachrichtigungen und Newsletter. Abschaltbar.
- Ansichten: Ungelesen, Markiert, Angepinnt, Später (Snooze), Aufgaben, Zum Lesen, Entwürfe, Gesendet,
  Archiv, Papierkorb, Spam, Ordner und Labels, gespeicherte Suchen.
- Schalter „Nur ungelesene“, Ungelesene oben gruppieren; Lieblingsordner und Ordnerfarben (Stufe 16).

### Lesen
- Thread-Ansicht, HTML sicher dargestellt, reiner Text als Rückfall.
- Externe Bilder und Tracking-Pixel blockiert; Freigabe einmalig oder dauerhaft pro Absender.
- Linkziel vor dem Öffnen sichtbar; Warnung, wenn Linktext und Ziel nicht zusammenpassen.
- Anhänge anzeigen, speichern, öffnen; nie automatisch ausführen.
- Kalendereinladungen lesbar anzeigen und an den System-Kalender übergeben.
- „Abmelden“ bei Mails mit List-Unsubscribe-Kopfzeile.
- Drucken, als PDF sichern, als `.eml` exportieren; `.eml`-Dateien öffnen.
- Lesebereich auf Wunsch in eigenem Fenster.

### Aktionen
- Gelesen/ungelesen, markieren, archivieren, löschen, verschieben, Spam, Schlagwörter.
- Mehrfachauswahl, Rückgängig für die letzte Aktion.
- Anpinnen, Snooze, „Als Aufgabe markieren“ (Markierung mit eigener Ansicht „Aufgaben“ und Erledigt-Schalter),
  „Zum Lesen“ (Stufe 16).
- Mails zwischen Konten verschieben und kopieren; Absender blockieren (Stufe 16).
- Lokale Regeln: Bedingung (Absender, Empfänger, Betreff, Liste, Konto) und Aktion (verschieben,
  als gelesen markieren, Kategorie, anpinnen, Benachrichtigung an/aus). Sie laufen nur, während die
  App läuft, und sind so beschriftet.

### Verfassen
- Rich Text und reiner Text; Ausgabe als `multipart/alternative` mit mailtauglichem HTML.
- Empfänger mit Autovervollständigung, CC, BCC, mehrere Absenderidentitäten.
- Anhänge und Inline-Bilder per Auswahl und Drag-and-drop.
- Entwürfe speichern sich selbst, lokal und im Entwürfe-Ordner des Servers.
- Signaturen (mehrere, HTML, pro Identität), Vorlagen.
- Senden rückgängig über kurze Verzögerung.
- Später senden: nur lokal, und die Oberfläche sagt klar, dass die App zum Zeitpunkt laufen muss.
- Rechtschreibprüfung des Systems.
- Inline-Schnellantwort unter der geöffneten Mail, „Senden und archivieren“, Vorlagen mit Empfängern
  (Stufe 16).
- Lesebestätigung nur nach Standard (MDN): anfordern je Nachricht; eingehende Anforderungen werden nie
  ohne Rückfrage beantwortet. Kein Tracking mit Zählpixel (0032).

### Suche
- Volltext offline über alles, was lokal vorliegt; erste Treffer beim Tippen.
- Filter: `from:` `to:` `cc:` `subject:` `in:` `account:` `filename:` `has:attachment` `is:unread`
  `is:flagged` `is:snoozed` `before:` `after:` `older_than:` `newer_than:`, Phrasen in Anführungszeichen, `-` zum Ausschließen.
- Auf Wunsch auf dem Server weitersuchen.

### Bedienung
- Layouts: Liste mit Lesebereich rechts, Lesebereich unten, nur Liste; kompakt und komfortabel.
- Command Palette (Cmd+K / Ctrl+K) für Navigation und alle Aktionen.
- Vollständige Tastaturbedienung, Kürzel änderbar, Konflikte werden erkannt.
- Heller und dunkler Modus, folgt dem System; mehrere Farbthemen (Stufe 16).
- Sprachen: Englisch und Deutsch; Spanisch vorbereitet.

### System
- Als Standard-Mail-App registrierbar (`mailto:`).
- Native Benachrichtigungen für neue Mails, fehlgeschlagene Sendungen, fällige Snoozes; pro Konto
  steuerbar, Ruhezeiten, Inhalt auf dem Sperrbildschirm ausblendbar.
- Zähler am Dock- bzw. Taskleisten-Symbol.
- Benachrichtigungstöne, je Konto stumm schaltbar (Stufe 16).

### Kontakte
- Lokales Verzeichnis aus gesendeten und empfangenen Mails, nach Häufigkeit und Aktualität sortiert.
- Namen bearbeiten, Dubletten zusammenführen.

### Einstellungs-Sync
- Gleicht zwischen Geräten ab, was der Mailserver nicht kennt: Kontenliste ohne Passwörter,
  Identitäten, Signaturen, Vorlagen, Einstellungen, Kürzel, Snooze, Pins, gespeicherte Suchen,
  Regeln, KI-Einstellungen ohne Schlüssel.
- Läuft über einen Ordner im eigenen Postfach, verschlüsselt mit einer Passphrase. Kein Server von uns.
- Mails, Ordner, Gelesen-Status und Entwürfe gleicht weiterhin allein der Mailserver ab.
- Passwörter und Tokens bleiben auf dem Gerät.

### KI
- Eigene KI anschließen über eine lokale MCP-Schnittstelle (Claude Desktop, Claude Code, Cursor,
  LM Studio und andere MCP-fähige Clients): Mails durchsuchen, Threads lesen, Entwürfe anlegen.
  Je Konto freizugeben, standardmäßig nur lesend, nie senden, löschen oder verschieben.
- Optional KI direkt in der App, Anbieter frei wählbar: lokal (Ollama, LM Studio und alles mit
  OpenAI-kompatibler Schnittstelle) oder mit eigenem API-Schlüssel.
- Funktionen: Thread zusammenfassen, Antwortvorschläge, Schreibhilfe (verbessern, kürzen,
  umformulieren, Ton, übersetzen), Suche in natürlicher Sprache, Hinweise auf mögliche offene Antworten.
- Jede Aussage einer Zusammenfassung oder Suchantwort verweist auf die Mail, aus der sie stammt.
- Standard ist aus. Pro Konto und pro Funktion schaltbar. Vor jeder Übertragung an einen
  Cloud-Anbieter steht, was gesendet wird.
- KI sendet, löscht und verschiebt nie. Ergebnisse landen als bearbeitbarer Entwurf oder Vorschlag.

## Barrierefreiheit

- Jede Funktion ist per Tastatur erreichbar, der Fokus ist immer sichtbar.
- Kontrast mindestens 4,5:1 für Text und 3:1 für Bedienelemente, in beiden Modi.
- Sinnvolle Rollen und Beschriftungen für VoiceOver und Narrator.
- Schriftgröße 80 bis 150 Prozent, „Bewegung reduzieren“ wird beachtet.
- Keine Information nur über Farbe: Kontofarben haben immer auch einen Namen.

## Leistungsziele

Ziele für aktuelle Hardware, gemessen in Stufe 7 und danach bei jeder Stufe mit Listen- oder Suchänderung.

| Messgröße | Ziel |
|---|---|
| Warmstart bis benutzbarer Posteingang | unter 1 s |
| Kaltstart bis benutzbarer Posteingang | unter 2 s |
| Wechsel zwischen lokal vorhandenen Mails | unter 100 ms |
| Suche bis erste Treffer | unter 200 ms bei 100.000 Mails |
| Sichtbare Reaktion auf eine Aktion | unter 100 ms |
| Scrollen | 60 Bilder pro Sekunde |
| Arbeitsspeicher im Leerlauf | unter 250 MB |

Geprüft wird mit 10.000, 100.000 und 500.000 Mails, mehreren Konten, sehr langen Threads und großen Anhängen.

## Nicht in Version 1

- PGP und S/MIME
- Kalender und Adressbuch-Abgleich (CalDAV, CardDAV); Einladungen werden nur angezeigt. Kommt als eigene Stufe nach Version 1 (0032)
- POP3
- Serverseitige Filter (Sieve)
- Eigener Sync-Server; die Sync-Schicht ist aber so gebaut, dass ein zweiter Transportweg möglich bleibt
- Eingebautes KI-Modell; lokale KI läuft über Ollama oder LM Studio
- Verschlüsselung der lokalen Datenbank; Schutz über FileVault bzw. BitLocker
- Bezahlfunktionen, Lizenzprüfung, Telemetrie, automatische Absturzberichte
- Threads über mehrere Konten hinweg
- Tracking gesendeter Mails mit Zählpixel; nur die Standard-Lesebestätigung (0032)
- Eingebettete Fremd-Apps (Messenger, Notizen, Chat-KI) in einer Seitenleiste (0032)
- Kontaktbilder aus dem Netz (Gravatar, BIMI), LinkedIn-Suche, mobile Apps (vorerst, 0032)

## Entscheidungen

| Frage | Entscheidung | Stand |
|---|---|---|
| Lizenz | GPL-3.0-or-later | vom Menschen bestätigt am 22.11.2026 |
| Veröffentlichung | Open Source, öffentliches Repo unter `github.com/othervice`, sobald Stufe 15 so weit ist; kein Lizenzverkauf (0035) | vom Menschen entschieden am 23.11.2026 |
| Produktname | „Vice Mail“ (Arbeitstitel) | 22.11.2026 |
| Bundle-ID (technische Kennung für macOS und Windows) | `org.vicemail.desktop`; vor Stufe 15 noch änderbar, danach nicht mehr ohne Datenumzug | von Claude gewählt am 22.11.2026, der Mensch braucht keine eigene |
| Auslieferung von Test-Installern | als Anhang eines Vorab-Releases im Gitea-Repo, Link per Benachrichtigung an den Menschen | 22.11.2026 |
| Updates in der App | Tauri-Updater mit signierten Dateien von den GitHub-Releases; automatische Prüfung nur mit Zustimmung (Standard aus, einmal gefragt nach dem ersten Konto, höchstens täglich), „Jetzt suchen“ jederzeit, Installation nur nach Klick (0037) | vorbereitet am 23.11.2026, live mit Stufe 15 |

## Offene Entscheidungen

| Frage | Empfehlung | Bis wann |
|---|---|---|
| Zugang zum Windows-Rechner für Tests | SSH vom Mac mini aus, sonst Handprüfung nach Checkliste | vor Stufe 6 |
| KI-Client zum Testen der MCP-Schnittstelle | vorhandenes Claude Desktop oder Claude Code | vor Stufe 12 |
| KI-Anbieter für KI in der App | Ollama lokal, API-Schlüssel nach Wahl | vor Stufe 13 |
