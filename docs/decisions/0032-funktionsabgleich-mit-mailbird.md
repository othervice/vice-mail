# 0032 — Funktionsabgleich mit Mailbird

Stand 23.11.2026, vom Menschen entschieden (Fragen beantwortet am 23.11.2026).

## Kontext

Der Mensch möchte den Funktionsumfang von Mailbird (Vergleichstabelle auf
getmailbird.com/features, 53 Funktionen in 7 Gruppen) auch in Vice Mail. Rund 35 davon sind mit den
Stufen 01–13 schon vorhanden (mehrere Konten, vereinter Posteingang, KI über MCP, lokale und
optionale KI, Regeln, Snooze, Aufgaben, Gesprächsansicht, Später senden, Rückgängig, Vorlagen,
Identitäten, Signaturen mit Bildern, Inline-Bilder, Rechtschreibprüfung, Suche über alle Konten mit
Anhangsuche, Blockieren externer Inhalte und Tracker, Abmelden, verzögertes Als-gelesen-Markieren,
Layouts, Zoom, Tastenkürzel, Hell/Dunkel, Drucken, Standard-Mail-App, Mails lokal).

## Entscheidungen

1. **Fehlende Funktionen, die zu den Grundsätzen passen, kommen in eine neue Stufe 16
   „Funktionsabgleich“** vor der Veröffentlichung: Lese-Liste, Filter „Ungelesen“ und Ungelesene
   oben, Lieblingsordner und Ordnerfarben, Mails zwischen Konten verschieben und kopieren, „Senden
   und archivieren“, Inline-Schnellantwort, Suchen in der geöffneten Mail, Anhänge (Vorschau, alle
   speichern, anheften), Absender blockieren, Benachrichtigungstöne, Farbthemen, Vorlagen mit
   Empfängern, Lesebestätigung (Punkt 2).
2. **Kein Tracking mit Zählpixel.** Statt „E-Mail-Tracking“ gibt es nur die Standard-
   Lesebestätigung (MDN, RFC 8098, Kopfzeile `Disposition-Notification-To`): Beim Senden je
   Nachricht anforderbar; der Empfänger entscheidet, ob er sie schickt. Eingehende Anforderungen
   beantwortet Vice Mail nie von selbst, sondern nur nach Rückfrage (Einstellung: immer fragen oder
   nie senden). Kein Pixel, kein eigener Server.
3. **Keine eingebetteten Fremd-Apps** (Mailbirds „45+ Integrationen“ in einer Seitenleiste). Vice
   Mail bleibt ein Mail-Client; KI-Clients kommen über MCP (Stufe 12).
4. **Kalender und Adressbuch-Abgleich (CalDAV, CardDAV) nach Version 1**, als eigene Stufe nach der
   Veröffentlichung. Bis dahin werden Einladungen angezeigt und Kontakte lokal aus Mails gebildet.
5. **Nicht geplant** (vorerst, Frage offen gelassen): Kontaktbilder aus dem Netz (Gravatar, BIMI),
   LinkedIn-Suche, mobile Apps, weitere Oberflächensprachen über Englisch und Deutsch hinaus.
6. **Microsoft Exchange, Outlook.com und Gmail-Anmeldung** gehören zum Abgleich; Stufe 14 ist
   damit „geplant“ statt „bei Bedarf“. Exchange vor Ort (EWS) kommt nur, wenn ein solches Konto
   gebraucht wird; Microsoft 365 und Outlook.com laufen über Block B (OAuth, IMAP/SMTP, notfalls
   Graph).

## Folgen

- `docs/SPEC.md`: neue Funktionen eingetragen, Ausschlüsse ergänzt.
- `docs/PLAN.md`: Stufe 16 vor Stufe 15; Stufe 14 geplant.
- `docs/stages/16-funktionsabgleich.md` neu.

## Rückweg

Jede Funktion der Stufe 16 ist einzeln umsetzbar. Ausschlüsse (2–5) kann der Mensch jederzeit neu
entscheiden; Tracking mit Pixel bliebe aber ein Widerspruch zu Grundsatz 3 („Privat“).
