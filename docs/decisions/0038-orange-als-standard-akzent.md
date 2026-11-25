# 0038 — Orange aus dem Logo als Standard-Akzentfarbe

Stand 23.11.2026. Von Claude entschieden; der Mensch hat „ein Top-Design am Ende“ verlangt und die
Frage nach der Akzentfarbe offen gelassen.

## Kontext

Das Logo (`docs/brand/`, vom Menschen vorgegeben) ist Orange auf fast Schwarz. Die Oberfläche hatte
seit Stufe 1 einen Petrol-Akzent. Stufe 16 hat sechs Farbthemen gebracht (Petrol, Blau, Violett,
Rosé, Orange, Grün); Standard war Petrol.

## Entscheidung

Standard-Akzent ist **Orange**, abgeleitet aus „Vice orange“ `#FF7A00`, für Text, Fokusring und
Bedienelemente so abgestimmt, dass WCAG AA in Hell und Dunkel erfüllt ist (auf hellem Grund ein
dunkleres Orange für Text und Links, auf dunklem Grund das hellere). Flächen in reinem Markenorange
nur sparsam (aktiver Eintrag, Hauptknopf), nie großflächig, weil eine Mail-App stundenlang gelesen
wird. Petrol und die übrigen Themen bleiben wählbar; wer schon ein Thema gewählt hat, behält es.

## Begründung

- Logo, App-Symbol und Oberfläche sprechen eine Sprache; das wirkt bewusst gestaltet.
- Die Kontrastprüfung (`app/tests/contrast.test.ts`) deckt jedes Thema ab, also auch das neue
  Standardthema.

## Rückweg

Eine Zeile: der Standardwert von `UiPreferences.accentTheme` (bzw. der Rückfall in
`app/src/features/preferences/accent.ts`).

## Nachtrag 24.11.2026

Standard-Akzent ist seit Entscheidung 0053 Blau; Orange bleibt wählbar, eine gespeicherte Wahl
bleibt. Das Logo ist unverändert.

## Nachtrag 25.11.2026

Seit Entscheidung 0068 ist auch das Logo blau (vom Menschen freigegeben); Orange ist keine
Markenfarbe mehr, bleibt aber als Akzent-Thema wählbar.
