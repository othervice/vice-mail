# 0067 — Keine Farben in ausgehenden Mails

- **Datum:** 25.11.2026
- **Anlass:** Stufe 21, Rückmeldung 9 zu 0.1.1: verhindern, dass helle Schrift aus dem dunklen
  Design beim Empfänger hell auf Weiß ankommt. Geplant: Der Composer schreibt keine Text- oder
  Hintergrundfarbe, außer man wählt sie ausdrücklich; eingefügter Text verliert Farben, die nur aus
  dem dunklen Modus stammen; Zitate behalten die Originalfarben (nicht die umgerechneten aus 0066);
  ausgehende Mails sagen `color-scheme: light dark`.
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 21, Punkt 9)
- **Code:** `crates/mail-content/src/compose/html.rs` (`document`, `STYLE_PRE`),
  `crates/mail-content/src/compose/paste.rs` (unverändert), `crates/mail-engine/src/compose/mod.rs`
  (Zitat aus dem gespeicherten Original, unverändert)
- **Tests:** `compose::tests::outgoing_html_has_no_colour_the_user_did_not_choose_and_declares_both_schemes`,
  `compose::tests::a_reply_quotes_the_original_not_the_dark_rendering`, Schnappschüsse in
  `compose/snapshots/`, `tests/fuzz.rs` (Composer-Ausgabe), Server-Test
  `sent_html_has_no_colours_and_a_reply_quotes_the_original_not_the_dark_rendering`
  (`crates/mail-engine/tests/compose.rs`)

## Stand vorher

Die Allowlist des Composers (0023) übernimmt aus Editor, Einfügen und Zitat nur Struktur und
Auszeichnung (Absätze, fett, kursiv, Listen, Zitate, Code, Links, eigene Inline-Bilder); jeder
`style`, jede `<font>` und jedes `bgcolor` fällt weg, der Editor bietet keine Farbwahl. Helle
Schrift aus dem dunklen Design kam also nie in eine Mail. Geschrieben wurden aber zwei Farben, die
niemand gewählt hat: die graue Linie links an Zitaten (`#cccccc`) und der hellgraue Grund von
Codeblöcken (`background-color:#f4f4f4`). Zeigt ein Programm des Empfängers die Mail dunkel und
macht die Standardschrift hell, steht Code hell auf Hellgrau.

## Entscheidung

**1. Keine Text- oder Hintergrundfarbe ohne ausdrückliche Wahl.** Da der Editor keine Farbwahl hat,
heißt das: nie. Codeblöcke bekommen statt des hellgrauen Grunds eine graue Linie
(`border:1px solid #cccccc`), wie Zitate. Eine Linie in mittlerem Grau ist auf hellem und dunklem
Grund sichtbar und keine Text- oder Hintergrundfarbe. Kommt später eine Farbwahl, schreibt sie nur
die gewählten Farben.

**2. Einfügen: alle Farben weg, nicht nur die des Designs.** Eingefügtes HTML geht wie bisher durch
die Allowlist; kein `style`, kein `<font color>`, kein `bgcolor` übersteht es. Eine Regel, die nur
„Farben aus dem dunklen Design“ entfernt (fast weiße Schrift, der Grund der Karte), müsste raten,
welche Farbe gewollt war; dieselbe helle Schrift kann aus dem dunklen Lesebereich der App, aus einer
Website im dunklen Modus oder aus einem Dokument stammen. Die klare Regel ist: Farbe kommt nur aus
einer Wahl im Composer.

**3. Zitate aus dem Original, nie aus der Darstellung.** Antworten und Weiterleiten bauen das Zitat
aus der gespeicherten Mail (oder ihrem Text, wenn nur ein Teil gespeichert ist), nicht aus dem
`srcdoc` des Lesebereichs. Die Umrechnung aus 0066 kann daher nie in ein Zitat geraten. Das Zitat
übernimmt Inhalt und Auszeichnung des Originals, aber wie seit 0023 keine Farben. Abweichung vom
Plan („Zitate behalten die Originalfarben“): Die Originalfarben eines Newsletters im Zitat einer Mail,
die `color-scheme: light dark` sagt, brächten genau das zurück, was 9 verhindern soll (dunkelgraue
Schrift `#333` auf dem dunklen Grund eines Empfängers). Sollen Zitate Farben behalten, braucht es
eine eigene Entscheidung, etwa ein Zitat als helles Blatt mit `only light`.

**4. Beide Schemata angeben.** Jede ausgehende HTML-Mail sagt im Kopf
`<meta name="color-scheme" content="light dark">` und
`<meta name="supported-color-schemes" content="light dark">` (die zweite Form für ältere
Apple-Mail-Versionen). Programme, die das lesen, zeigen die Mail in ihrem dunklen Design mit eigener
Standardfarbe, ohne sie umzufärben; da die Mail keine Farben setzt, ist sie dort lesbar. Kein
`<style>` mit `:root{color-scheme:…}`: ausgehende Mails haben nur Inline-Stile (0023).

## Verworfen

- **Weißer Grund und schwarze Schrift fest in jede Mail:** zwingt die Mail überall hell und ist das,
  was Programme mit dunklem Design dann schlecht umkehren.
- **`@media (prefers-color-scheme: dark)` in ausgehenden Mails:** bräuchte ein `<style>`, das Gmail
  und andere entfernen; ohne gesetzte Farben gibt es nichts umzuschalten.
- **`only light`:** würde Empfängern das dunkle Design verbieten, ohne Grund.
- **Farben aus dem Original im Zitat:** siehe 3.

## Folgen

- Die Schnappschüsse der Mail-HTML (`compose/snapshots/`, `templates/snapshots/`) enthalten die
  beiden `<meta>` und die Linie am Codeblock.
- `docs/ARCHITECTURE.md` (HTML im Composer), `docs/DESIGN.md` (Verfassen).
