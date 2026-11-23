# 0019 — KI-Anbieter: OpenAI-kompatibel und zusätzlich die Anthropic Messages API

Stand 23.11.2026. Vorgezogener Unterbau von Stufe 13 (Crate `mail-ai`). Grundlage: Entscheidung
0010 („ob die OpenAI-kompatible Schnittstelle von Anthropic genügt oder ein eigener Anbieter für
die Messages API nötig ist, entscheidet Stufe 13") und die Anthropic-Dokumentation vom November
2026 (Skill `claude-api`, platform.claude.com).

## Kontext

Stufe 13 verlangt eine schlanke Anbieterschicht über die OpenAI-kompatible Chat-Schnittstelle mit
frei wählbarer Basis-URL (Ollama, LM Studio, llama.cpp-Server, OpenRouter, OpenAI). Für Anthropic
ist zu prüfen, ob deren OpenAI-kompatible Schnittstelle genügt. Feste Anforderungen der Stufe:
Streaming, Abbruch, typisierte Fehler und Ratenbegrenzung, strukturierte Ausgabe, und der
Stopp-Grund wird vor dem Lesen der Antwort geprüft, **auch auf Ablehnung**.

## Was die Dokumentation zur Kompatibilitätsschicht sagt

Seite „OpenAI SDK compatibility" (`/docs/en/cli-sdks-libraries/libraries/openai-sdk`):

1. Sie ist „primarily intended to test and compare model capabilities, and is not considered a
   long-term or production-ready solution for most use cases". Für den vollen Funktionsumfang
   verweist Anthropic auf die native API.
2. **Ablehnungen sind dort nicht erkennbar.** Die Sicherheitsklassifikatoren der aktuellen Modelle
   (Fable 5.x, Opus 5/5.5, Sonnet 5.5, Haiku 5.5) lehnen als normale Antwort ab: HTTP 200 mit
   `stop_reason: "refusal"` und `stop_details` (`category`, `explanation`), im Stream im
   `message_delta`, oft nach schon gestreamtem Teiltext, den man laut Doku verwerfen muss. In der
   Kompatibilitätsschicht ist `choices[].message.refusal` „Always empty", und wie eine Ablehnung
   auf `finish_reason` abgebildet wird, steht nirgends. Ein abgelehnter Teiltext könnte also als
   gültige Antwort durchgehen.
3. `response_format` nur als `json_schema` mit `strict: true`; `json_object` und `text` geben 400.
4. `temperature` unter 1 und `top_p` ungleich dem Standard geben bei Claude 4.7 und neuer 400.
   Fehlermeldungen sind „nicht gleichwertig" und nur zum Protokollieren gedacht.
5. Kein Prompt-Caching, keine Thinking-Ausgabe, kein server-seitiger Fallback bei Ablehnung.

## Entscheidung

1. **Zwei Anbieter hinter `AiProvider`:** `OpenAiCompatible` (`POST {base}/chat/completions`,
   `GET {base}/models`, Basis-URL mit `/v1`) und `AnthropicMessages` (`POST {base}/v1/messages`,
   `GET {base}/v1/models`). Anthropic wird über die Messages API angesprochen, mit direktem HTTP
   nach der Doku, weil es kein offizielles Rust-SDK gibt. Die Vorlage „Anthropic" in den
   Einstellungen nutzt diesen Anbieter. Technisch kann man Anthropic weiter als
   OpenAI-kompatibles Profil eintragen, dann aber ohne verlässliche Erkennung von Ablehnungen.
2. **Anfrage nach der Doku:** Header `x-api-key` und `anthropic-version: 2026-10-21`. Der Body
   enthält `model`, `max_tokens` (Standard 16 000, Platz für adaptives Denken), `system`, genau
   eine Nutzernachricht, `stream: true` und, wenn im Profil eingeschaltet,
   `output_config.format` mit dem JSON-Schema. Er enthält kein `temperature`, `top_p` oder
   `top_k`: ab API-Version 2026-10-21 lehnt die API sie bei jedem Modell mit 400 ab, auch mit dem
   Standardwert. Ein `thinking`-Parameter fehlt ebenfalls, weil aktuelle Modelle adaptiv denken
   (bei Opus 5.5 ist Abschalten sogar ein 400); Thinking-Blöcke im Stream werden ignoriert.
   Werkzeuge gibt es nicht. Als Modell wird `claude-opus-5-5` vorgeschlagen; gewählt wird aus
   `GET /v1/models` (seitenweise über `after_id`).
3. **Stopp-Grund zuerst:** `end_turn` und `stop_sequence` gelten als fertig. `max_tokens` und
   `model_context_window_exceeded` werden zu `AiError::Truncated`. `refusal` wird zu
   `AiError::Refused { category, explanation }`; der Teiltext wird verworfen, und `category`
   oder `explanation` dürfen `null` sein. Den Text gibt es nur über `Completed::into_text`, das
   zuerst den Stopp-Grund prüft. Für OpenAI-kompatible Server gilt dieselbe Regel (`length`,
   `content_filter`, `delta.refusal`).
4. **Fehler:** `event: error` im Stream (etwa `overloaded_error`) und HTTP 401/403, 404, 413, 429,
   503/529 und 5xx werden zu eigenen Fehlern, mit `retry-after`. Nichts wird automatisch
   wiederholt, damit eine Anfrage mit Mails nie ohne den Nutzer zweimal hinausgeht.
5. **Server-seitiger Fallback bei Ablehnung** (Beta `server-side-fallback-2026-07-01`,
   `fallbacks: "default"`): Die Doku empfiehlt ihn für die betroffenen Modelle. Er ist im Profil
   schaltbar (`refusal_fallback`, Standard an) und wird nur an `api.anthropic.com` gesendet, und
   nur für Modellfamilien, die ihn laut Doku unterstützen (`claude-fable-5*`, `claude-mythos-5*`,
   `claude-opus-5*`, `claude-sonnet-5-5`). Die Daten gehen dabei an denselben Anbieter. Welches
   Modell geantwortet hat, steht in `Finish::model` (aus dem `fallback`-Block des Streams).
6. **Strukturierte Ausgabe** ist je Profil abschaltbar (`structured_output`). Nicht jedes Modell
   und nicht jeder Server kann sie, laut Doku etwa Fable/Mythos in CMEK-Organisationen nicht, und
   bei OpenAI-kompatiblen Servern ist es unterschiedlich. Die Antwort wird in jedem Fall robust
   gelesen und gegen die gesendeten Mail-IDs geprüft.

## Begründung

- Die Stufe verlangt die Prüfung auf Ablehnung ausdrücklich, und nur die native API liefert dafür
  ein dokumentiertes Signal samt Kategorie.
- Anthropic selbst nennt die Kompatibilitätsschicht nicht produktionsreif.
- Der zusätzliche Code ist klein: ein Modul mit Anfrage, Stream-Decoder und Modell-Liste.
  Transport, Prompts, Datensparsamkeit und Prüfungen teilen sich beide Anbieter.
- Verworfen: nur die OpenAI-kompatible Schnittstelle, aus den Gründen oben. Ein Anthropic-SDK gibt
  es für Rust nicht.

## Folgen

- `docs/ARCHITECTURE.md`, Abschnitt KI: Statt „ein eigener Anbieter … nur, wenn die kompatible
  Schnittstelle nicht genügt" gilt jetzt: zwei Anbieter, OpenAI-kompatibel und Anthropic Messages
  API (diese Entscheidung). Das passt die Hauptsitzung an.
- Keine neue Abhängigkeit; `mail-ai` nutzt reqwest, rustls, url, getrandom, serde_json und tokio
  aus dem Baum und als eigene Crates nur `mail-model` und `mail-content`.
- Die Einstellungen der Stufe 13 kennen zwei Profilarten mit den Feldern von `ProviderConfig`.

## Rückweg

Dokumentiert Anthropic die Kompatibilitätsschicht eines Tages gleichwertig, vor allem die
Ablehnung, kann `AnthropicMessages` entfallen. Profile werden dann auf `OpenAiCompatible` mit der
Basis-URL `https://api.anthropic.com/v1` umgestellt.
