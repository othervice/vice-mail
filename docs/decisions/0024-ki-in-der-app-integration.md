# 0024 — KI in der App: Einstellungen, Zustimmung, Protokoll und Schnittstelle

Stand 23.11.2026. Stufe 13, Backend. Baut auf 0010 (KI zuerst über MCP, KI in der App schlank
und optional) und 0019 (zwei Anbieter) auf. Die Oberfläche (Einstellungen, Zustimmungsdialog,
KI-Panel) folgt in einer eigenen Sitzung.

## Kontext

`crates/mail-ai` enthält Anbieter, Prompt-Schutz, Datensparsamkeit, Zusammenfassung und Fragen
als reine Funktionen. Offen waren die Verdrahtung in `AppCore`, Einstellungen, Zustimmung,
Protokoll, Antwortvorschläge, Schreibhilfe, offene Antworten und die Commands für die Oberfläche.
`docs/ARCHITECTURE.md` führte eine Tabelle `ai_profile` der Klasse „sync (ohne Schlüssel)“.

## Entscheidung

1. **Einstellungen als Geräte-Einstellung.** Schalter, Profile (Art, Basis-URL, Modell,
   strukturierte Ausgabe, Ausgabegrenze), Profil je Funktion, Cloud-Freigabe je Konto und die
   Zustimmung je Profil liegen als ein JSON-Wert in `setting` (`global`, `ai.settings`). Der
   Schlüssel ist nicht für den Einstellungs-Sync registriert, also Klasse Gerät. Keine Tabelle
   `ai_profile`. Gründe: Basis-URLs wie `localhost` oder `192.168.…` meinen dieses Gerät oder
   dieses Netz; Schlüssel und Zustimmung dürfen ohnehin nicht wandern; die Cloud-Freigabe je
   Konto ist eine Datenschutz-Entscheidung, die der Mensch auf jedem Gerät selbst trifft.
2. **Schlüssel im Schlüsselbund** unter `ai-profile:<id>`. Die Oberfläche schickt einen
   Schlüssel nur zum Kern (`AiKeyChange::Set`), keine Antwort enthält ihn (nur `hasKey`).
   `Debug` des Eingabetyps schwärzt ihn; Fehlertexte laufen durch die Bereinigung von `mail-ai`.
3. **Aus, bis eingeschaltet.** Ohne Schalter scheitert jede KI-Funktion, auch Modellliste und
   Verbindungstest, mit `ai_off`, bevor eine Verbindung entsteht. Eine Funktion ohne Profil oder
   ein Profil ohne Modell gilt ebenfalls als aus.
4. **Zustimmung je Cloud-Profil und Host.** Vor der ersten Anfrage an ein Profil, dessen
   Basis-URL nicht lokal ist (`mail_ai::locality`), lehnt der Kern mit `ai_consent_required` ab.
   Die Zustimmung gilt für den Host und Port zum Zeitpunkt der Zustimmung; ein neuer Host nimmt
   sie zurück. Lokale Profile brauchen keine. Modellliste und Verbindungstest senden nur den
   Schlüssel an seinen Anbieter, keine Mail und keine Frage, und brauchen deshalb keine
   Zustimmung (aber eingeschaltete KI).
5. **Cloud-Freigabe je Konto, Standard nein.** Bei Zusammenfassung und Antwortvorschlag prüft der
   Kern sie, bevor ein Body geladen wird, und `mail-ai` noch einmal direkt vor dem Senden. Bei
   Fragen werden Kandidaten aus Konten ohne Freigabe weggelassen und gezählt (`excluded`); die
   Frage selbst (Text des Nutzers, keine Mail) geht hinaus. Die Schreibhilfe gehört zu keinem
   Konto.
6. **Transparenz über ein Ereignis.** Jede Anfrage trägt eine von der Oberfläche gewählte ID.
   Vor jeder Anfrage an den Anbieter meldet `aiRequest` mit `sending` Anzahl Mails, Bytes,
   Konten, Host und Modell; danach `receiving` (Zeichen) und `finished`. Abbrechen über
   `cancelAiRequest`. Die Antworten sind JSON; gestreamt wird deshalb der Fortschritt, nicht der
   Text.
7. **Anfrage-Protokoll** in `ai_request` (Migration 0011): eine Zeile je Anfrage an den Anbieter
   mit Zeit, Funktion, Profilname, Host, lokal oder nicht, Modell, Anzahl Mails, Bytes, Ergebnis,
   Fehlercode, Dauer. Nie Frage, Mailtext, Antwort oder Schlüssel. 30 Tage, höchstens 10.000.
   Modellliste und Verbindungstest werden nicht protokolliert (kein Inhalt).
8. **Antwortvorschlag zweistufig.** `aiSuggestReply` liefert nur Text (Karte im Panel).
   `aiOpenReplyDraft` legt auf Klick einen normalen Antwortentwurf aus Stufe 5 mit diesem Text an
   und lässt ihn offen. Die KI legt selbst nie einen Entwurf an.
9. **Schreibhilfe** auf einer Auswahl (verbessern, kürzen, umformulieren, Ton, übersetzen in eine
   feste Liste von Sprachen) liefert eine Vorschau. Die Auswahl geht als Datenblock mit Nonce
   hinaus, unter einem festen Systemtext je Aufgabe.
10. **Fragen schrittweise** im Kern (`mail_ai::send_query`, Suche aus Stufe 7 über
    `search_exactly`, `mail_ai::send_answer`), damit jede der zwei Anfragen vorher angezeigt
    werden kann. Bis zu 20 Kandidaten. Ohne Kandidaten: ehrlich „nichts gefunden“, ohne zweite
    Anfrage.
11. **Hinweise auf offene Antworten ohne KI**, als Regeln über gespeicherte Kopfzeilen und
    Vorschautexte (gesendete Frage ohne Antwort nach `days` Tagen, Standard 3; direkte Frage an
    den Nutzer, nach einem Tag unbeantwortet; nie Newsletter, Listen, Benachrichtigungen,
    automatische Mails, No-Reply-Absender, Papierkorb, schon beantwortet). Weggeklickte Hinweise
    in `open_reply_dismissal`. Eigener Schalter, unabhängig von der KI. Die optionale Verfeinerung
    durch das Modell ist nicht gebaut; deshalb ist „offene Antworten“ keine Funktion mit
    Profilwahl.
12. **Fehlercodes** `ai_off`, `ai_consent_required`, `ai_not_permitted`, `ai_cancelled`,
    `ai_unreachable`, `ai_unauthorized`, `ai_busy`, `ai_refused`, `ai_failed` (`CoreError::Ai`).

## Folgen

- `docs/ARCHITECTURE.md`: Zeile `ai_profile` ersetzt durch `setting` `ai.settings` (Gerät) und die
  Tabellen `ai_request`, `open_reply_dismissal`; Abschnitt KI um die Verdrahtung ergänzt.
- `mail-ai` bekommt `prepare_reply`/`suggest_reply`, `prepare_writing`/`rewrite`,
  `send_query`/`send_answer`/`partition_candidates`; weiterhin ohne Store, Outbox oder IMAP.
- `mail-engine` hängt von `mail-ai` ab. Die TLS-Konfiguration kommt aus `mail_imap::tls`.

## Rückweg

Sollen Profile doch zwischen Geräten wandern, wird `ai.settings` (ohne `consent_*`, ohne
`cloud_accounts`) als synchronisierbare Einstellung registriert; Schlüssel bleiben je Gerät.
