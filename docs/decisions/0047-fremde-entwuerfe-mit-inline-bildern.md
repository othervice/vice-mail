# 0047 — Entwürfe anderer Programme: Inline-Bilder bleiben, sonst bleibt das Original

- **Datum:** 24.11.2026
- **Anlass:** Abgleich mit `docs/SPEC.md` („Keine verlorene Mail“). Ein Entwurf, den ein anderes
  Programm im Entwürfe-Ordner abgelegt hatte, verlor beim Öffnen seine `cid:`-Bilder: Die
  Allowlist für fremdes HTML (`editor_html`) entfernt jedes Bild, die Teile wurden gewöhnliche
  Anhänge, und die erste Sicherung ersetzte die Server-Kopie (0018). Das Original war danach weg.
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-content/src/compose/html.rs` (`foreign_draft_html`),
  `crates/mail-engine/src/compose/mod.rs` (`open_from_message`),
  `crates/mail-store/migrations/0018_draft_kept_original.sql`, `Draft::original_kept`,
  Hinweis im Composer (`composer.originalKept`)
- **Tests:** `crates/mail-content/src/compose/foreign_tests.rs`,
  `crates/mail-engine/src/compose/tests.rs` (`inline_images_of_a_foreign_draft_stay_inline_through_editing_and_sending`,
  `a_foreign_draft_the_editor_cannot_keep_completely_leaves_its_original_alone`, Fake-Server),
  `crates/mail-engine/tests/compose.rs` (`inline_images_of_a_foreign_draft_survive_saving_and_sending`,
  Stalwart), `app/tests/composer.test.tsx`

## Regel

- **Eigene Inline-Bilder bleiben Inline-Teile.** Ein `cid:`-Bild, dessen Teil zur Mail selbst
  gehört, ein Rasterbild (PNG, JPEG, GIF, WebP) ist und die Grenze für Inline-Bilder einhält, wird
  eine Datei des Entwurfs mit Content-ID wie ein im Composer eingefügtes Bild (Stufe 10, 0023):
  der Editor zeigt es, Sichern und Senden schreiben es als `multipart/related`. Die Content-ID
  bleibt, wenn sie gültig ist; sonst (Gmail schreibt `ii_…` ohne `@`, der Leser benennt Bilder
  ohne Content-ID selbst) bekommt das Bild eine neue in der Domain des Absenders, und das HTML
  zeigt auf sie. Ein `cid:` wird wie im Lesebereich gefunden (Prozent-Dekodierung, sonst ein
  eindeutiger Treffer ohne Groß- und Kleinschreibung).
- **Was nicht darstellbar ist, lässt das Original unberührt.** Zählt `foreign_draft_html` etwas,
  das der Editor nicht halten kann (Bild aus dem Netz oder `data:`, `cid:` ohne behaltbaren Teil,
  Bild in vorformatiertem Text, in einem Link oder als Hintergrund, Video, eingebettetes Objekt,
  SVG), ist die Mail nur teilweise gespeichert oder beschädigt (jede `ParseProblem` außer einem
  unlesbaren Datum), lässt sich ein Anhang nicht kopieren oder fehlt die Message-ID, dann bekommt
  der Entwurf eine eigene Message-ID und keine Server-Kopie. Das Original wird nie ersetzt oder
  entfernt; die Fassung der App liegt daneben. `draft.kept_original` hält die Message-ID des
  Originals (leer ohne eine), damit ein zweites Öffnen denselben Entwurf findet, und der Composer
  sagt es (`Draft::original_kept`).
- **Formatierung** (Tabellen, Farben, Schriften) geht wie bisher durch die Allowlist verloren und
  zählt nicht als „nicht darstellbar“: Der Text bleibt, und das ist die bekannte Grenze aus 0018.

## Verworfen

- Fremde Entwürfe gar nicht mehr zu ersetzen: Dann läge nach jeder Bearbeitung eine zweite Fassung
  im Ordner, auch wenn nichts verloren ginge.
- Das Original mit derselben Message-ID zu behalten: Eine Mail ist je Postfach eine Zeile
  (`message_location` mit Schlüssel aus Postfach und Mail), zwei Fassungen mit derselben ID im
  selben Ordner fielen lokal zusammen.
