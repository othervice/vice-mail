# 0010 — KI zuerst über eine MCP-Schnittstelle, KI in der App nur schlank und optional

Stand 22.11.2026, vom Menschen bestätigt.

## Kontext

Der Plan sah zwei KI-Stufen vor: Stufe 12 mit eigenem Crate `mail-ai`, zwei Anbietern
(OpenAI-kompatibel und Anthropic Messages API), Zusammenfassung, Antwortvorschlägen, Schreibhilfe und
KI-Panel; Stufe 13 mit Suche in natürlicher Sprache und Hinweisen auf offene Antworten. Das ist viel
eigener Code: Anbieterschicht, Prompts, Schlüsselverwaltung, Oberfläche. Gleichzeitig nutzen viele
Menschen bereits einen KI-Client, der das Model Context Protocol (MCP) spricht (Claude Desktop,
Claude Code, Cursor, LM Studio und andere), und möchten ihre eigene KI an ihre Mails anschließen.

## Entscheidung

1. **Stufe 12 wird „KI über MCP“.** Die App bietet einen lokalen MCP-Server an. Jeder MCP-fähige
   KI-Client kann damit Mails durchsuchen, Threads lesen und Entwürfe anlegen. Die App enthält dafür
   weder Anbieter noch API-Schlüssel noch Prompts.
2. **Stufe 13 wird „KI in der App (optional)“.** Für Funktionen, die in der Oberfläche selbst
   stattfinden (Zusammenfassung im Lesebereich, Antwortvorschläge, Schreibhilfe, Suche in
   natürlicher Sprache), gibt es eine einzige schlanke Anbieterschicht über die OpenAI-kompatible
   Chat-Schnittstelle mit frei wählbarer Basis-URL (Ollama, LM Studio, llama.cpp-Server, OpenRouter,
   OpenAI). Anthropic bietet ebenfalls eine OpenAI-kompatible Schnittstelle; ob sie genügt oder ein
   eigener Anbieter für die Messages API nötig ist, entscheidet Stufe 13 (vorher Skill `claude-api` laden).
3. Die Vorbedingung „Ollama oder API-Schlüssel vor Stufe 12“ entfällt. Zum Testen von Stufe 12
   genügt ein vorhandener MCP-Client.

## Begründung

- **Jede KI passt**, ohne dass die App Anbieter kennen muss; lokale Modelle über MCP-fähige Clients ebenso.
- **Passt zu den Grundsätzen:** kein eigener Dienst, keine Schlüssel in der App, nichts verlässt das
  Gerät, solange der Nutzer keinen Cloud-Client anschließt.
- **Weniger Code** als zwei Anbieter samt Prompts und KI-Oberfläche; die Engine-Funktionen (Suche,
  Thread lesen, Entwurf anlegen) gibt es ohnehin.

## Sicherheitsregeln für den MCP-Server

Mit MCP gilt die bisherige Regel „das Modell bekommt keine Werkzeuge“ nicht mehr: Der fremde Agent
hat eigene Werkzeuge (z. B. Webzugriff), und Mailinhalte können ihn mit eingeschleusten Anweisungen
manipulieren. Die App kann den Client nicht kontrollieren, nur begrenzen, was er bekommt und tun darf:

- **Aus, bis der Nutzer ihn einschaltet.** Je Konto ausdrücklich freigeben; nicht freigegebene Konten
  sind für den Client unsichtbar.
- **Nur lokal.** Transport stdio (vom Client gestartetes Hilfsprogramm) oder HTTP nur auf
  `127.0.0.1` mit Zugangstoken je Client; nie im Netz erreichbar. Stufe 12 wählt und begründet.
- **Standardmäßig nur lesend.** Schreibend höchstens „Entwurf anlegen“, je Konto zuschaltbar. Es gibt
  kein Werkzeug zum Senden, Löschen, Verschieben, Markieren oder Ändern von Einstellungen, auch nicht
  später auf Wunsch eines Clients.
- **Inhalte als Daten.** Werkzeuge liefern bereinigten Text (nie rohes HTML, nie externe Bilder),
  Anhänge nur mit Namen; Antworten sind als nicht vertrauenswürdiger Mailinhalt gekennzeichnet.
  Größen- und Mengengrenzen je Aufruf.
- **Sichtbar und widerrufbar.** Jeder Zugriff erscheint in der App (Client, Werkzeug, Anzahl Mails);
  Not-Aus und Widerruf einzelner Clients jederzeit.
- **Ehrlich in der Oberfläche:** Die App erklärt beim Einschalten, dass ein Cloud-Client die
  gelesenen Mails an seinen Anbieter überträgt und dass die App das nicht verhindern kann.

## Folgen

- `docs/ARCHITECTURE.md`, Abschnitt KI und erlaubte Netzverbindungen, sowie `docs/SPEC.md`, Abschnitt
  KI, sind angepasst.
- Die Stufendateien heißen jetzt `12-ki-ueber-mcp.md` und `13-ki-in-der-app.md`.
- Stufe 12 braucht die Suche aus Stufe 07 und die Entwürfe aus Stufe 05, nicht mehr Stufe 13.

## Rückweg

Die Engine-Funktionen hinter den MCP-Werkzeugen sind dieselben, die eine eingebaute KI nutzen würde.
Stufe 13 kann die eingebaute Anbieterschicht jederzeit ausbauen; MCP bleibt daneben bestehen.
