# 0085 — Seitenleiste nach Konten: „Alle Konten“, Favoriten, je Konto ein Abschnitt, Fuß mit Abgleich

- **Datum:** 26.11.2026
- **Anlass:** Stufe 23, Punkt 2, Schritt 2 („Seitenleiste“). Der Entwurf
  https://claude.ai/artifact/BuSPYpDvmSDm4s1JJoSZFX wurde am 26.11.2026 freigegeben („ja das dunkle
  design finde ich super. das nehmen wir.“, 0083). Die Stufendatei: „Alle Konten“ (Posteingang,
  Später, Markiert, Gesendet, die übrigen Ansichten unter „Mehr“), „Favoriten“, je Konto ein
  Abschnitt (Posteingang, Gesendet, „Weitere Ordner“), Abschnitte per Ziehen umsortieren, unten
  „Abgeglichen um …“, Abgleichen, Einstellungen. „Keine Funktion geht verloren.“
- **Status:** umgesetzt von der ausführenden Sitzung. **Ändert** den Aufbau der Seitenspalte
  (DESIGN.md, „Seitenspalte“, 23.11.2026), die Zeilen der Spalte (32 px, Balken; 0031, 0083), die
  Kontozeile von 0078 (jetzt ein Kopf) und „Ansichten bearbeiten“ (Stufe 17: eine Linie „Mehr“ in
  der Liste). Ziehen und Umsortieren bleiben wie in 0078, 0081 und 0082.
- **Code:** Modell `crates/mail-model/src/ui.rs` (`SidebarFold`, `UiPreferences::sidebar_folds`,
  `validate_sidebar_folds`, `MAX_SIDEBAR_FOLDS`), Kern `crates/mail-engine/src/ui_prefs.rs`
  (`ui.sidebar_folds`, nur Gerät), Shell `app/src-tauri/src/lib.rs` (Konstante); Oberfläche
  `app/src/app/SidebarColumn.tsx`, `AllAccountsTree.tsx` (neu), `SidebarFooter.tsx` (neu),
  `IconRail.tsx`, `features/mailboxes/MailboxTree.tsx`, `tree.ts` (`splitAccountTree`),
  `features/organize/folders.tsx` (Favoriten), `features/layout/columnViews.ts` (`MORE_ENTRY`,
  `DEFAULT_COLUMN_ORDER`, `columnGroups`), `EditViewsDialog.tsx`, `sidebarFolds.ts` (neu),
  `SectionRow.tsx`, `sidebarStyles.ts`, `features/unified/unifiedEntries.tsx` (vorher
  `UnifiedTree.tsx`), `features/accounts/accountColor.ts` (`accountText`), `stores/ui.ts`
  (Zuklappen von Abschnitten und Konten nicht mehr dort), `stores/notices.ts` (Kontext `sync`),
  `styles/tokens.css` (`--ui-sidebar-row`, `--ui-radius-mark`), `app.css` (`rounded-mark`); entfernt
  `features/search/SavedSearchNav.tsx`; Texte `nav.allAccounts`, `nav.more`, `nav.moreFolders`,
  `nav.moreFoldersOf`, `sidebar.footer.*`, `columnViews.moreLine`, `columnViews.movedMore`,
  `actions.failed.sync`.
- **Tests:** Modell `ui::tests::sidebar_folds_default_to_empty_and_are_checked`, angepasst
  `wire_format_matches_the_contract`; Kern `ui_prefs::tests::the_sidebar_folds_stay_on_this_device`,
  Sync-Szenario (`settings_sync.rs`: bleibt auf dem Gerät), `capture`; Vitest
  `sidebar-sections.test.tsx` (neu), `sidebar-column.test.tsx`, `sidebar-keyboard.test.tsx`,
  `column-views.test.tsx`, `contrast.test.ts`, angepasst `frame.test.tsx`, `folders.test.tsx`,
  `folder-create.test.tsx`, `no-avatars.test.tsx`, `saved-searches.test.tsx`,
  `unified-inbox.test.tsx`, `a11y.test.tsx`, `ui-preferences.test.tsx`; Desktop
  `stage23-sidebar.e2e.mjs` (neu), angepasst `frame.e2e.mjs`, `restart.e2e.mjs`,
  `mark-folder-read.e2e.mjs`, `spacing.e2e.mjs`, `actions.e2e.mjs`, `compose.e2e.mjs`,
  `send-later.e2e.mjs` und die Hilfen in `support/app.mjs` (`openSidebarParts`, `statusEntry`,
  `openStatusView`; `openView` öffnet „Mehr“ und „Weitere Ordner“ vorher); unverändert grün
  `stage22-accounts.e2e.mjs`, `stage22-accounts-restart.e2e.mjs`, `stage23-drag.e2e.mjs`. Shell
  `tests::ui_preference_commands_store_validate_and_emit`.

## Aufbau

Von oben (die Kopfzeile mit „Seitenleiste ausblenden“ bleibt; Schritt 3 legt sie in die
Werkzeugleiste über die ganze Breite):

1. **„Alle Konten“** (mit nur einem Konto heißt der Abschnitt „Ansichten“): Posteingang (der
   vereinte Posteingang, nur ab zwei Konten), die Ansichten vor der Linie „Mehr“ (Standard:
   Später, Markiert), Gesendet (nur ab zwei Konten), dann **„Mehr“**, zu bis der Mensch es öffnet:
   die Ansichten nach der Linie (Standard: Warten auf Antwort, Aufgaben, Zum Lesen, Angeheftete
   Anhänge, Anhänge, Angepinnt, Gespeicherte Ansichten, Kontakte), die Kontogruppen, die
   gespeicherten Suchen, Aktivität und Ausgang.
2. **„Favoriten“** wie in Stufe 16, die Symbole in der Farbe ihres Kontos.
3. **Je Konto ein Abschnitt** in der Reihenfolge des Menschen (0078): Kopf (Quadrat in der
   Kontofarbe, Listenname, bei Problemen das Zeichen), Posteingang und Gesendet mit Symbolen in der
   Kontofarbe, dann **„Weitere Ordner“**, zu bis der Mensch es öffnet, mit dem ganzen übrigen Baum.
4. **Fuß:** „Abgeglichen um 18:04“, „Jetzt abgleichen“, Einstellungen.

### Wo die bisherigen Einträge stehen

| Bisher | Jetzt |
|---|---|
| „Alle Posteingänge“ (ab zwei Konten) | „Alle Konten“ → Posteingang (der Kopf der Liste heißt weiter „Alle Posteingänge“) |
| Kontogruppen unter „Alle Posteingänge“ | „Mehr“, nach den Ansichten |
| Ansichten (Stufe 17, nach Wahl) | vor der Linie in „Alle Konten“, danach unter „Mehr“ |
| Gespeicherte Suchen (eigener Abschnitt) | „Mehr“, nach den Kontogruppen |
| Favoriten | „Favoriten“ |
| Konto mit allen Ordnern | Abschnitt des Kontos: Posteingang, Gesendet, „Weitere Ordner“ |
| Aktivität, Ausgang (untere Gruppe) | „Mehr“, am Ende; ihre roten Zahlen zeigt ein zugeklapptes „Mehr“ |
| Einstellungen (untere Gruppe) | Fuß |
| Ordner vom Server neu laden (Menü des Kontos) | bleibt; dazu „Jetzt abgleichen“ im Fuß für alle Konten |

Tastenkürzel, Befehlspalette, Menüs („…“, Rechtsklick, Umschalt+F10), Umbenennen und Verschieben
der Konten, „Als gelesen markieren“, Ordner anlegen, umbenennen und löschen, Farben, mbox und das
Ablegen gezogener Mails bleiben an ihren Zeilen.

## Was „Alle Konten“ zeigt

Der Posteingang dort ist der vereinte Posteingang (`UnifiedView::All`). **Später, Markiert und
Gesendet zeigen wie bisher das gewählte Konto** (wie die Einträge der schmalen Leiste; der Kopf
der Liste nennt es): Der Kern kennt vereinte Listen nur für den Posteingang. Für Später, Markiert
und die anderen Markierungen könnte er es (`MarkScope::All`), die Liste müsste dafür aber Zeilen
gemischter Konten wie der vereinte Posteingang führen (`MixedActions`, `MixedReading`); das gehört
zur Liste (Schritt 3) und ist hier nicht gebaut. Ein vereintes Gesendet bräuchte eine neue Liste im
Kern. Beides steht als offener Punkt im Bericht. Damit nie zwei Zeilen zugleich als gezeigt gelten,
öffnet Gesendet im Abschnitt eines Kontos (ab zwei Konten) den Ordner selbst (`showMailbox`);
Gesendet in „Alle Konten“ ist die Ansicht des gewählten Kontos.

## „Mehr“ in „Ansichten bearbeiten“

Welche Ansicht in „Alle Konten“ steht und welche unter „Mehr“, wählt der Mensch im selben Dialog
wie bisher: Die Liste hat einen Eintrag mehr, die **Linie „Unter ‚Mehr‘“** (`more`). Ansichten
davor stehen in „Alle Konten“, danach unter „Mehr“. Die Maus zieht die Linie wie eine Ansicht, die
Tastatur bewegt die Ansichten über sie hinweg (Alt+↑/↓), die Ansage nennt die Seite. Gespeichert
wird die Linie wie eine Ansicht in `ui.column_views` (synchronisiert). Der Standard (`[]`): Später,
Markiert, Linie, dann die übrigen in der Reihenfolge von Stufe 17. Eine Liste von vor Stufe 23 hat
keine Linie: Später und Markiert kommen nach vorn (in der Reihenfolge des Menschen), alle anderen
dahinter, jede behält ihre Sichtbarkeit. Eine ältere Version auf einem anderen Gerät kennt `more`
nicht und behält den Eintrag beim Speichern (Stufe 17), hängt ihn aber hinten an; ändert dort
jemand die Ansichten, stehen hier danach alle Ansichten in „Alle Konten“, bis die Linie wieder
gezogen wird.

## Gemerkt auf diesem Gerät

Offen oder zu: „Alle Konten“, „Mehr“, „Favoriten“, jedes Konto und seine „Weiteren Ordner“. Bisher
galt das Zuklappen bis zum Beenden der App; jetzt bleibt es über Neustarts. Gespeichert im Kern als
UI-Einstellung `UiPreferences.sidebarFolds` (`ui.sidebar_folds`), eine Liste `{ key, open }` mit
den Schlüsseln `allAccounts`, `more`, `favorites`, `account:<UUID>`, `folders:<UUID>`; nur was vom
Standard abweicht (Standard: alles offen außer „Mehr“ und „Weitere Ordner“), höchstens 128, älteste
zuerst verworfen. **Nicht synchronisiert**, wie die Fensterlayouts: Was offen ist, gehört zu diesem
Bildschirm, und die Schlüssel tragen Konto-IDs. Einträge entfernter Konten fallen beim nächsten
Speichern weg. Unterordner im Baum klappen weiter nur bis zum Beenden der App.

## Zeilen und Maße (aus dem Entwurf)

- **Zeilen** 28 px (`--ui-sidebar-row`; 6 px über und unter 13 px Text), Innenabstand 8 px,
  Rundung 7 px, Text in Primärfarbe, Symbol 16 px (Entwurf 17 px; die Symbole der App sind 16 px),
  8 px Abstand. „Mehr“ und „Weitere Ordner“ leiser (`text-secondary`), Pfeil an der Stelle des
  Symbols. Darunter 16 px je Ebene; der Pfeil eines Ordners mit Unterordnern sitzt in den 16 px vor
  seinem Symbol, statt einer eigenen Spalte (die bei jeder Zeile 20 px gekostet hätte).
- **Auswahl** `rail-selected` (`sideSel` des Entwurfs), halbfett, Symbol im Akzent; in den
  Abschnitten der Konten und bei den Favoriten bleibt das Symbol in der Kontofarbe. **Kein Balken**:
  Fläche, Gewicht und (wo es geht) die Farbe des Symbols unterscheiden die Zeile, nicht die Farbe
  allein.
- **Zähler** 12 px, Tabellenziffern, leise (`text-tertiary`); ungelesene Mails eines Posteingangs
  halbfett in `text-secondary`. Fehlgeschlagenes (Aktivität, Ausgang) als rote Plakette
  (`text-on-danger` auf `danger`): Roter Text auf `rail-selected` erreicht dunkel nur 4,45 : 1.
- **Köpfe** 11 px halbfett `text-tertiary`, 24 px hoch, 8 px Abstand zwischen den Abschnitten: von
  der letzten Zeile bis zur Mitte des Kopfs 20 px wie im Entwurf (14 px Luft und die halbe Zeile).
  Die Linie beim Ziehen eines Kontos (0082) liegt in der Mitte dieser 8 px. Konto: Quadrat 8 px mit
  Rundung 2 px (`rounded-mark`). Der Fuß hat seine Haarlinie 8 px vor den Rändern der Spalte.
- **Zugeklappt** zeigt ein Kopf, was er verbirgt: das Konto seine Summe ungelesener Mails, „Weitere
  Ordner“ die der eigenen Ordner darin, „Mehr“ und „Alle Konten“ die Zahl der Vorgänge und Sendungen,
  die den Menschen brauchen. Liegt die gezeigte Ansicht unter einem zugeklappten „Mehr“ oder
  „Weitere Ordner“, trägt diese Zeile die Fläche und ist der Tab-Stopp.

## Fuß

- **Text** aus dem Zustand der Konten, mit den Worten des Kopfs der Liste: ein Konto ohne Abgleich
  („Arbeit: Kein Abgleich“, `danger`), sonst eines offline („Club: Offline“), alle offline
  („Offline“), während eines Abgleichs „Gleicht ab …“, sonst „Abgeglichen um 18:04“: das Ende des
  letzten vollen Durchgangs (`lastSyncAt`) des Kontos, das am längsten nicht abgeglichen hat, mit
  Datum, wenn nicht von heute. Mehrere Konten: „2 Konten: Offline“, der Tooltip nennt sie.
- **„Jetzt abgleichen“** bittet jedes Konto um einen vollen Durchgang (`reloadFolders`, wie „Ordner
  vom Server neu laden“): verbunden sofort, getrennt eine neue Verbindung. Kein neuer Befehl des Kerns.
- **Einstellungen** wie bisher (`rail-settings`).
- **Aktivität und Ausgang** stehen unter „Mehr“, nicht im Fuß: Mit vier Symbolen passte „Abgeglichen
  um 18:04“ schon bei 240 px nicht mehr. Damit nichts verloren geht, zeigt ein zugeklapptes „Mehr“
  die rote Zahl; die Kürzel und die Palette bleiben, die schmale Leiste behält beide.

## Schmale Leiste

Gegliedert wie die Spalte: Posteingang, die Ansichten vor der Linie, Gesendet; Haarlinie; die
Ansichten unter „Mehr“, Entwürfe, Archiv, Papierkorb, Ordner. Aktiv: `rail-selected`, Symbol im
Akzent, kein Balken. Unter dem Punkt jedes Kontos steht die Zahl seiner ungelesenen Mails
(Posteingang und eigene Ordner, wie der Kopf des Kontos; 11 px, ab 1.000 „999+“), nicht als Plakette
auf dem Punkt: Dreistellige Zahlen hätten die Farbe verdeckt. Das Zeichen für offline oder Fehler
rückt dafür an den oberen Rand. Der Name sagt die Zahl mit („…, 3 ungelesen“).

## Tastatur, Screenreader, Ziehen

- Bäume nach WAI-ARIA wie bisher: „Alle Konten“ (mit „Mehr“ als Knoten), „Favoriten“, je Konto
  einer; der Fuß ist eine Symbolleiste. Mit zwei Konten, Favoriten, Gruppen und gespeicherten Suchen
  7 Tab-Stopps statt 8.
- Ziehen wie in 0078, 0081 und 0082: Der Kopf eines Kontos wird gezogen (Linie, Dimmen, Escape,
  Alt+↑/↓). Gezogene Mails öffnen ein zugeklapptes Konto und „Weitere Ordner“ nach 700 ms; der
  Posteingang in „Alle Konten“ nimmt sie wie der Posteingang der Leiste (jede in den Posteingang
  ihres Kontos).

## Verworfen

- **Aktivität und Ausgang als Symbole im Fuß.** Kein Platz für den Text (siehe Fuß).
- **„Weitere Ordner“ samt Posteingang und Gesendet.** Zwei Zeilen für denselben Ordner, beide als
  gezeigt markiert.
- **Feste Gruppen (Später und Markiert immer oben).** Die Stufendatei verlangt, dass die Wahl der
  Ansichten weiter wirkt; die Linie passt in die vorhandene Liste und Sync.
- **Zuklappen nur bis zum Beenden (wie bisher).** „Mehr“ wäre nach jedem Start wieder zu.
- **Zuklappen synchronisieren.** Die Schlüssel tragen Konto-IDs, und was offen ist, hängt am
  Bildschirm.

## Ungeprüft

- Windows (Mica hinter der Spalte, Schriftbild, Tastatur): `docs/testing/windows-checklist.md`,
  24.1 und 24.2.
- Echte Konten mit vielen Ordnern; geprüft mit dem Testserver (drei Konten) und den Vitest-Fakes.
- Die Zieh-Geste des Betriebssystems (wie 0082, Punkt 6).

## Nachtrag 26.11.2026 (Zusammenführung der vier Schritte)

- **Was „Alle Konten“ zeigt** gilt so nicht mehr: Später, Markiert, Gesendet und die übrigen
  Ansichten dort (auch unter „Mehr“) zeigen ab zwei Konten alle sichtbaren Konten in einer
  gemischten Liste wie der vereinte Posteingang; der Kern hat dafür ein vereintes Gesendet
  (Entscheidung 0087). Gesendet im Abschnitt eines Kontos bleibt der Ordner dieses Kontos. Die Zeile
  in „Alle Konten“ gilt nur als gezeigt, solange der Bereich „alle Konten“ ist; Gesendet eines
  Kontos ist sonst die Zeile seines Abschnitts.
- **Kopfzeile:** Der Kopf mit „Seitenleiste ausblenden“ ist mit Schritt 3 (0084) in die
  Werkzeugleiste über die ganze Breite gewandert; die Spalte beginnt mit „Alle Konten“. Mit zwei
  Konten, Favoriten, Gruppen und gespeicherten Suchen hat die Spalte jetzt 5 Tab-Stopps (Rand, vier
  Bäume) plus den Fuß.
- **Zahl am Punkt der schmalen Leiste:** Ab 1.000 ungelesenen Mails steht dort „999+“, der Name
  sagte aber die genaue Zahl („1.296 ungelesen“); axe in der echten WebView meldete das als
  `label-content-name-mismatch` (WCAG 2.5.3). Jetzt sagt der Name „999+ ungelesen“ wie der Punkt
  (`IconRail.tsx`, Text `sidebar.unreadOver`; Vitest `sidebar-sections.test.tsx`, Desktop
  `a11y.e2e.mjs`, `stage22-ui.e2e.mjs`). Die genauen Zahlen stehen in der Spalte (Posteingang, Ordner,
  zugeklappter Kopf des Kontos).
- **Desktop-Tests:** Specs, die Ordner unter „Weitere Ordner“ anklicken (`account-sync`,
  `reading`, `folders`, `stage19-contacts-folders`), öffnen es vorher (`openSidebarParts`);
  „Alle Posteingänge“ ist in den Specs die Zeile `unified-all`.
