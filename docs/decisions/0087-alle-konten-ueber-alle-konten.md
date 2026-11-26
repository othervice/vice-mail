# 0087 — „Alle Konten“ zeigt alle Konten: Später, Markiert, Gesendet und die übrigen Ansichten

- **Datum:** 26.11.2026
- **Anlass:** Stufe 23, Punkt 2, Zusammenführung der vier Schritte. Schritt 2 (0085) hatte offen
  gelassen, dass in „Alle Konten“ nur der Posteingang alle Konten zeigt; Später, Markiert und
  Gesendet zeigten das gewählte Konto, unter einer Überschrift, die etwas anderes sagt. Der Auftrag
  der Zusammenführung: Später, Markiert und Gesendet unter „Alle Konten“ wirklich über alle Konten,
  wie der vereinte Posteingang (gemischte Liste mit dem Kontostreifen aus Schritt 3, Aktionen je
  Zeile mit ihrem eigenen Konto, Regeln für nur lesende Konten je Konto); ein vereintes Gesendet im
  Kern, schnell wie der vereinte Posteingang; Gesendet im Abschnitt eines Kontos bleibt dieses
  Konto; ebenso in der schmalen Leiste.
- **Status:** umgesetzt von der ausführenden Sitzung. **Ändert** 0085 (Abschnitt „Was ‚Alle Konten‘
  zeigt“). Ergänzt Stufe 7 (vereinter Posteingang, Kontogruppen, ausgeblendete Konten), Stufe 9
  (Ansichten markierter Mails, `MarkScope`) und 0065 (Aktionen in gemischten Listen).
- **Code:** Kern `crates/mail-store/src/groups.rs` (`unified_sent`, `role_mailboxes`),
  `search_api.rs` (`Store::list_unified_sent`, `list_unified_sent_threads`),
  `crates/mail-engine/src/search/mod.rs` (`AppCore::list_unified_sent`,
  `list_unified_sent_threads`), Shell `app/src-tauri/src/commands/threads.rs` (Commands
  `list_unified_sent`, `list_unified_sent_threads`), `lib.rs`; Oberfläche
  `features/unified/scope.ts` (neu: welche Ansichten dem Bereich folgen), `AllAccountsViews.tsx`
  (neu: `ScopedMarkView`, `ScopedSentView`), `UnifiedInbox.tsx` (`MixedPanes` aus dem vereinten
  Posteingang herausgelöst), `features/comfort/listSources.tsx` (`useMarkListSource`,
  `isMarkKind`), `comfort/queries.ts` (`mixedMarksQueryKey`, Neuladen), `threads/queries.ts`
  (`withThreadItems`), `ConversationToggle.tsx` (`always`), `features/mail/MailView.tsx`
  (Weiche, Kopf), `app/AllAccountsTree.tsx`, `app/SidebarColumn.tsx`, `stores/ui.ts`
  (`showAllAccountsView`), `services/ipc.ts`; Texte `unified.viewFailed`, `unified.sentEmpty.*`.
- **Tests:** Store `stage7_tests::unified_sent_mixes_the_sent_folders_of_the_visible_accounts`,
  Kern `search::tests::unified_sent_through_the_core`, Messung `search::bench` (zwei neue Zeilen,
  Grenzen im immer laufenden `small_data_meets_generous_thresholds`); Vitest
  `all-accounts-views.test.tsx` (neu), angepasst `sidebar-column.test.tsx`,
  `sidebar-sections.test.tsx`, `organize.test.tsx`; Fakes `support/comfort.ts` (Bereich der
  Markierungen), `support/search.ts` (vereintes Gesendet).

## Entscheidung

**1. Ein Bereich für die Ansichten, nicht nur für den Posteingang.** Den Zustand gab es schon:
`unifiedInbox` (alle sichtbaren Konten oder eine Kontogruppe) gegenüber dem gewählten Konto. Er
gilt jetzt für jede Ansicht, die zu „Alle Konten“ gehört: Posteingang, Später, Warten auf Antwort,
Aufgaben, Zum Lesen, Angeheftete Anhänge, Markiert, Angepinnt und Gesendet
(`features/unified/scope.ts`). Anhänge, gespeicherte Suchen und Kontakte zeigen ohnehin alle
Konten. Entwürfe, Archiv und Papierkorb bleiben Ordner des gewählten Kontos (jeder Abschnitt eines
Kontos hat sie unter „Weitere Ordner“).

**2. Die Zeilen von „Alle Konten“ setzen den Bereich.** Ab zwei Konten öffnet jede Ansicht in
„Alle Konten“ (auch unter „Mehr“) sie über alle sichtbaren Konten (`showAllAccountsView`);
Posteingang und Kontogruppen wie bisher. Gesendet im Abschnitt eines Kontos öffnet den Ordner
dieses Kontos (wie in 0085), ebenso jeder andere Ordner. Die Zeile in „Alle Konten“ gilt nur als
gezeigt, solange der Bereich „alle Konten“ ist; Später eines einzelnen Kontos (über die schmale
Leiste oder die Befehlspalette) hat dort keine Zeile, der Kopf der Liste nennt dann das Konto.

**3. Schmale Leiste.** Ihre Einträge folgen demselben Bereich: Nach „Alle Posteingänge“ (Menü im
Kopf der Liste) oder einer Zeile von „Alle Konten“ zeigen Posteingang, Später, Markiert, Gesendet
und die Ansichten darunter alle Konten; der Punkt eines Kontos wählt wie bisher dieses Konto, und
die Einträge zeigen dann seine Mails. So bleibt Später eines einzelnen Kontos erreichbar.

**4. Gemischte Listen wie der vereinte Posteingang.** Später, Markiert und die anderen
Markierungen fragen den Kern mit `MarkScope::All` (bzw. `Group`), das gab es seit Stufe 9. Die
Liste ist dieselbe wie beim vereinten Posteingang (`MixedPanes`): Kontostreifen am rechten Rand
(0084), der Name des Kontos im zugänglichen Namen der Zeile, Schnellaktionen, Kontextmenü und Ziehen
mit dem Ordner und Konto jeder Zeile (0065, 0081), die offene Mail mit den Aktionen und
Nur-lesend-Regeln ihres Kontos (`MixedReading`). Der Kopf der Liste nennt den Bereich („Alle
Konten“ oder den Namen der Gruppe) statt eines Kontos.

**5. Gesendet aller Konten im Kern.** Zwei neue Commands, gebaut wie der vereinte Posteingang:
`listUnifiedSent` (Seiten von Mails) und `listUnifiedSentThreads` (Gespräche, 0039), beide mit
`UnifiedView` (alle sichtbaren Konten oder eine Gruppe). Der Store sucht je sichtbarem Konto seinen
Ordner mit der Rolle `sent` (höchstens einer je Konto, Index `mailbox_role`) und mischt dieselben
Abfragen wie beim Posteingang: `UNION ALL` je Ordner mit Keyset, kein `IN (…)`, kein `OFFSET`. Ein
Konto ohne Gesendet-Ordner fehlt einfach. Gesendet zeigt die Empfänger statt des Absenders, wie der
Ordner eines Kontos.

**6. Neu laden.** Die gemischten Listen liegen unter `["unified", …]`, das die Ereignisse
`messagesChanged`, `mailboxesChanged`, `inboxChanged` und `accountsChanged` schon neu laden; die
Markierungen zusätzlich mit jedem Neuladen der Markierungslisten eines Kontos
(`refreshComfortLists`, `refreshComfortListsOf`), weil eine Markierung in jedem Konto in ihnen
auftauchen kann.

## Messung

`cargo test -p mail-engine --lib search::bench::bench_100k -- --ignored --nocapture` auf dem
Mac mini (M4), **Debug-Build** (ein Release-Build hätte alle Abhängigkeiten neu gebaut; der Platz
auf der Platte reichte dafür nicht sicher), 100.000 Mails in fünf Konten (Gesendet 12 % je Konto),
Median / p95 / max in ms über 100 Läufe:

| Messung | Median | p95 | max |
|---|---:|---:|---:|
| Vereinter Posteingang (5 Konten, 100) | 2,12 | 2,41 | 3,70 |
| Gesendet aller Konten (5 Konten, 100) | 2,17 | 2,45 | 3,23 |
| Thread-Liste vereinter Posteingang (100) | 7,86 | 9,35 | 10,17 |
| Thread-Liste Gesendet aller Konten (100) | 8,12 | 9,40 | 10,87 |

Gleich schnell wie der vereinte Posteingang (derselbe Plan); im Release-Build lag der vereinte
Posteingang bei 0,47 ms (`docs/testing/performance.md`, Stufe 7). Der immer laufende Test
`small_data_meets_generous_thresholds` hält die neuen Abfragen unter 100 bzw. 300 ms.

## Was bewusst fehlt

- **Sortieren, Schnellfilter, Tabelle, „Nur ungelesene“ und „Ungelesene oben“** gibt es für
  Gesendet aller Konten nicht: Sie hängen an einem Ordner oder am vereinten Posteingang
  (`InboxScope`); ein dritter Bereich dort wäre ein eigener Umbau im Kern. Der Ordner Gesendet jedes
  Kontos hat sie weiter. Der Umschalter „Gespräche“ steht dort immer in der Leiste (sonst steckt er
  unter 1.600 px im Menü „Listenoptionen“, das es hier nicht gibt).
- **Verschieben, Schlagwörter, Später und endgültiges Löschen in der Leiste über der offenen Mail**
  fehlen wie im vereinten Posteingang (0065): Sie brauchen die Auswahl eines Ordners; Schnellaktionen
  und Kontextmenü der Zeile haben sie.

## Verworfen

- **Später, Markiert und Gesendet in „Alle Konten“ immer über alle Konten, egal was gewählt ist.**
  Später eines einzelnen Kontos wäre dann nirgends mehr zu sehen, und der Punkt eines Kontos in der
  schmalen Leiste hätte für diese Ansichten keine Wirkung mehr.
- **Gesendet aller Konten über die Suche** (`in:sent` über alle Konten): Die Suche liefert Treffer
  mit Hervorhebung statt Seiten und Gespräche, und ihr Plan ist ein anderer.
- **Nur Später, Markiert und Gesendet umstellen, die Ansichten unter „Mehr“ nicht.** „Mehr“ steht in
  „Alle Konten“; eine Ansicht, die der Mensch vor die Linie zieht, hätte sonst plötzlich eine andere
  Bedeutung.

## Ungeprüft

- Windows: dieselbe Oberfläche, nicht von Hand geprüft (`docs/testing/windows-checklist.md`).
- Echte Konten: geprüft mit dem Testserver (zwei Konten, Gesendet von Bob für die Bildschirmfotos
  per IMAP ergänzt) und den Vitest-Fakes.
- Messung im Release-Build (siehe oben).
