# Stufe 16 — Funktionsabgleich

**Ziel:** Vice Mail hat die Funktionen, die Mailbird bietet und die zu den Grundsätzen passen
(Entscheidung 0032). Jede Aufgabe ist einzeln umsetzbar.

**Voraussetzung:** Stufen 04, 05, 07, 09 und 10 fertig; für Farbthemen der Feinschliff aus 0031.

## Aufgaben

- [ ] **Lese-Liste.** „Zum Lesen“ als lokale Markierung neben „Aufgaben“ und „Später“, mit eigener Ansicht, Aktion und Kürzel; abgeglichen über den Einstellungs-Sync wie Aufgaben. Ändert nichts auf dem Server.
- [ ] **Ungelesen.** Schalter „Nur ungelesene“ im Listenkopf; Einstellung „Ungelesene oben gruppieren“ je Ansicht.
- [ ] **Lieblingsordner und Ordnerfarben.** Ordner als Favorit oben in der Seitenleiste und in der Palette; Farbe je Ordner (aus der Farbpalette der Kontofarben, nie die Farbe allein, immer mit Namen). Abgeglichen.
- [ ] **Zwischen Konten verschieben und kopieren.** Über die Outbox: APPEND im Zielkonto, prüfen (Message-ID), erst dann im Quellkonto entfernen; beide Konten brauchen Schreibzugriff. Fehlerfälle: Ziel abgelehnt, Verbindung nach APPEND gekappt (nie doppelt, nie verloren). Ziehen auf einen Ordner eines anderen Kontos fragt nach.
- [ ] **Senden und archivieren.** Beim Antworten die Ursprungsmail nach dem bestätigten Senden archivieren; als Einstellung und als eigene Taste.
- [ ] **Inline-Schnellantwort.** Antwortfeld unter der geöffneten Mail (reiner Text mit Zitat-Einstellung), wird beim Ausklappen zum vollen Composer; dieselben Entwürfe und dieselbe Outbox.
- [ ] **In der Mail suchen.** Strg/Cmd+F im Lesebereich sucht im angezeigten Text (auch im Mail-Frame, ohne Skripte im Frame), mit Treffern und Weiter/Zurück.
- [ ] **Anhänge.** Vorschau für Bilder, PDF und Text in einem gesicherten Fenster ohne Netz; „Alle speichern“ in einen Ordner; Anhänge anheften (eigene Ansicht „Angeheftete Anhänge“).
- [ ] **Absender blockieren.** Lokale Regel „Absender blockieren“: künftige Mails gehen in den Spam-Ordner (bzw. werden ausgeblendet, wenn das Konto nur lesend ist); Liste der blockierten Absender mit Aufheben; abgeglichen.
- [ ] **Benachrichtigungstöne.** Ton an/aus, Auswahl aus den Systemtönen bzw. mitgelieferten Tönen (Lizenz prüfen), je Konto stumm; Ruhezeiten gelten.
- [ ] **Farbthemen.** Mehrere Akzentfarben (Vorschlag: sechs), hell und dunkel, alle mit Kontrast nach WCAG AA; als Oberflächeneinstellung, abgeglichen.
- [ ] **Vorlagen mit Empfängern.** Vorlagen können An/CC/BCC und Betreff vorbelegen.
- [ ] **Lesebestätigung (MDN, RFC 8098).** Beim Senden je Nachricht anfordern; eingehende Anforderungen nur nach Rückfrage beantworten (Einstellung: immer fragen, nie senden); empfangene Bestätigungen an der gesendeten Mail anzeigen. Kein Zählpixel, kein Server.

## Abnahme

- [ ] Tests je Aufgabe (Kern und Oberfläche); für „Zwischen Konten verschieben“ Servertests mit Fehlerinjektion: genau eine Kopie im Ziel, die Quelle wird erst nach Bestätigung entfernt
- [ ] Test: Eine eingehende Lesebestätigungs-Anforderung führt ohne Zustimmung nie zu einer gesendeten Mail (Mitschnitt SMTP)
- [ ] Test: Lese-Liste, Favoriten, Ordnerfarben, blockierte Absender und Farbthema gleichen zwischen zwei Geräten ab
- [ ] axe ohne schwere Befunde für alle neuen Ansichten, hell und dunkel, in jedem Farbthema
- [ ] Desktop-Tests für Schnellantwort, Suchen in der Mail und Anhangsvorschau
- [ ] `scripts/check.sh` ist grün

## Nicht in dieser Stufe

- Zählpixel-Tracking, eingebettete Fremd-Apps (0032)
- Kalender, CalDAV, CardDAV (nach Version 1)
- Kontaktbilder aus dem Netz, LinkedIn-Suche, mobile Apps

## Der Mensch prüft danach

- Zwischen zwei echten Konten verschieben (Nebenkonten); Farbthemen ansehen; eine Lesebestätigung mit einem anderen Programm austauschen.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
