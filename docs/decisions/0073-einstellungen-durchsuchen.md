# 0073 — Einstellungen durchsuchen: Verzeichnis aus i18n-Schlüsseln, Zeilen mit Anker

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 8 (vom Menschen am 26.11. gewählt): ein Suchfeld oben in den
  Einstellungen, das jede Option über Titel und Beschreibung in der eingestellten Sprache findet
  (z. B. „Signatur“, „Ton“), dorthin springt und sie kurz hervorhebt; mit Tastatur bedienbar
  (Cmd/Ctrl+F in den Einstellungen).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 22, Punkt 8).
- **Code:** `app/src/features/settings/search/settingsIndex.ts` (das Verzeichnis),
  `searchSettings.ts` (Suche), `jump.ts` (Sprung, Kürzel), `SettingsSearch.tsx` (Feld und
  Trefferliste), `app/src/components/settingAnchor.ts` (Anker der Zeilen), die Anker in `Switch`,
  `ChoiceGroup`, `SettingsSection` (weitere Abschnitte) und den Blöcken der Abschnitte,
  `features/settings/SettingsView.tsx`, Texte `settings.search.*` in `app/src/i18n/`
- **Tests:** Vitest `settings-search.test.tsx` (Suche in Deutsch und Englisch, Akzente,
  Reihenfolge; jede Zeile jedes Abschnitts im Verzeichnis, in zwei Zuständen und beiden Sprachen;
  jeder Eintrag erscheint irgendwo; Combobox und Listbox, Pfeiltasten, Eingabe, Klick, Escape,
  Cmd+F, gleiche Titel, Sprachwechsel; axe), Desktop `stage22-ui.e2e.mjs` (Cmd+F, „Signatur“,
  Sprung nach „Schreiben“, „Ton“, hell und dunkel)

## Entscheidung

**1. Ein festes Verzeichnis aus i18n-Schlüsseln.** `SETTINGS_INDEX` nennt je Option den Abschnitt,
den Schlüssel ihres Titels und den ihrer Beschreibung (bei Varianten je Plattform oder Zustand
mehrere); die Suche übersetzt sie in der Sprache der Oberfläche. Jeder Text steht damit nur einmal
in `app/src/i18n/`. Dazu findet der Name jedes Abschnitts den Abschnitt selbst („Die ganze Seite“).

**2. Die Zeilen tragen ihren Titel als Anker.** Jede Option markiert ihre Zeile mit dem angezeigten
Titel (`data-setting-title`): `Switch` und `ChoiceGroup` von selbst, weitere Abschnitte an ihrer
Überschrift, Blöcke (Überschrift mit Beschreibung) und Felder ausdrücklich. Wo zwei Zeilen eines
Abschnitts denselben Titel zeigen („Mitteilungen“, „Ton“), unterscheidet sie ihr Schlüssel
(`data-setting-key`). Steuerelemente ohne festen Titel (eine Zeile je Konto, die eigenen Regeln,
Signaturen, Formulare) bekommen keinen Anker; ihr Block (`data-setting-block`) wird über seine
Überschrift gefunden.

**3. Ein Test macht das Vergessen schwer.** `settings-search.test.tsx` öffnet jeden Abschnitt
(ohne und mit KI, MCP, laufendem Sync und Updater, in Englisch und Deutsch) und schlägt fehl, wenn
eine Zeile mit Anker nicht im Verzeichnis steht oder ihre Beschreibung nicht zeigt, wenn ein
Steuerelement in keiner Option und keinem Block liegt und wenn ein Eintrag des Verzeichnisses in
keinem Abschnitt erscheint (Ausnahmen mit Grund: Blöcke des Syncs, die nur in anderen Zuständen
zeigen). Eine neue Option ohne Eintrag fällt so beim ersten Testlauf auf.

**4. Suchen.** Groß- und Kleinschreibung und Akzente zählen nicht („uberfahren“ findet
„Überfahren“; je Zeichen gefaltet, damit die Fundstellen im angezeigten Text stimmen). Jedes Wort
der Eingabe muss in Titel oder Beschreibung stehen. Treffer stehen nach Abschnitten gruppiert in der
Reihenfolge der Abschnittsliste, in einer Gruppe zuerst der Abschnitt selbst, dann Titel, die mit
dem Wort beginnen, dann solche mit einem Wort, das damit beginnt, dann der Rest, sonst in der
Reihenfolge der Seite.

**5. Das Feld steht über der Abschnittsliste** (wie die Suche der Systemeinstellungen von macOS und
Windows), die Treffer schweben darunter über der Seite (26 rem breit). Es ist eine Combobox mit
Listbox und Gruppen: Pfeiltasten bewegen den aktiven Treffer, der Fokus bleibt im Feld
(`aria-activedescendant`), Eingabe oder Klick springt, Escape leert, Tab verlässt das Feld, eine
höfliche Statuszeile sagt die Zahl der Treffer oder dass keiner passt. Das Kürzel der Suche
(Cmd/Ctrl+F, „/“, wie der Nutzer es belegt hat) setzt in den Einstellungen den Fokus hinein; im
Rekorder der Tastenkürzel bleibt es eine Taste wie jede andere.

**6. Der Sprung.** Der Abschnitt öffnet (Konten: die Liste); sobald die Zeile da ist (ein
Abschnitt lädt vielleicht noch), steht sie in der Mitte, ihr Bedienelement bekommt den Fokus (die
gewählte Option einer Auswahl, sonst das erste freigegebene), sonst die Zeile selbst; wird ihr
Bedienelement gleich danach freigegeben, wandert der Fokus dorthin. Zwei Sekunden lang liegt
`accent-soft` 8 px um die Zeile (negativer Rand, das Layout bleibt), ein- und ausgeblendet wie jede
Farbe; bei „Bewegung reduzieren“ ohne Übergang, und gescrollt wird nie weich. Zeigt der Abschnitt
die Zeile in seinem Zustand nicht (ein Block des Syncs, solange er aus ist), bleibt der Fokus auf
der Seite.

## Verworfen

- **Das Verzeichnis zur Laufzeit aus den gerenderten Abschnitten lesen:** müsste alle Abschnitte
  einmal aufbauen (sie fragen den Kern), und Optionen fehlten in Zuständen, die gerade nicht
  zeigen.
- **Eine zweite Liste von Suchtexten:** liefe auseinander; die Schlüssel sind dieselben, die die
  Zeilen rendern, und der Test vergleicht beides.
- **Jede Zeile in ein eigenes `SettingRow` umbauen:** viele Änderungen in fremden Abschnitten ohne
  Gewinn; die vorhandenen Bausteine tragen den Anker selbst.
- **Die Treffer an Stelle der Seite zeigen:** die Seite verschwände beim Tippen; die schwebende Liste
  lässt sie stehen.
- **Aktionen (Knöpfe) und die Seiten einzelner Konten finden:** keine Optionen der Einstellungen;
  die Seite eines Kontos hängt am Konto, der Sprung wüsste nicht, an welchem.

## Folgen

- Neue Optionen brauchen einen Eintrag in `settingsIndex.ts`; nach dem Zusammenführen anderer
  Zweige von Stufe 22 nennt der Test die neuen Zeilen (Spamfilter, Passwortübernahme im Sync).
- Tokens `--ui-settings-search-width`, `--ui-settings-search-height`; Texte `settings.search.*`.
- `docs/DESIGN.md`, Abschnitt „Einstellungen durchsuchen“.

## Rückweg

Das Feld aus `SettingsView` nehmen; die Anker sind Datenattribute ohne Wirkung und können bleiben.
