# 0011 — WebView2: Navigationen im Mail-Frame abfangen

- **Datum:** 22.11.2026
- **Stufe:** 6 (Windows-Teil vorgezogen), betrifft Stufe 3 (Links im Mail-Frame)
- **Status:** entschieden von der Hauptsitzung am 22.11.2026: **Option B**. Die WebView2-Aufrufe
  kommen in eine eigene kleine Crate nur für Windows, die als einzige `unsafe_code = "deny"` mit
  einzeln begründeten `#[allow(unsafe_code)]` und `// SAFETY:`-Kommentaren hat; alle anderen Crates
  behalten `forbid`. Umsetzung zusammen mit dem Link-Abfang in Stufe 3; bis dahin bleibt das Feature
  `webview2-frame-guard` aus. Ohne den Abfang öffnen sich Links unter Windows nicht, es fließen aber
  keine echten Ziele ab: Der Frame kennt nur Platzhalter, neue Fenster lehnt `on_new_window` ab.
  **Umgesetzt in Stufe 3, Feature standardmäßig an** (Abschnitt „Umsetzung“ unten).

## Kontext

Unter macOS sieht der Navigations-Hook (`on_navigation`) auch Navigationen in Unter-Frames. Darauf
baut die Linkbehandlung aus 0005 auf: Die Bereinigung schreibt jedes `href` auf
`https://link.invalid/<token>/<index>` um, ein Klick navigiert den Mail-Frame dorthin, der Hook
bricht ab und meldet Token und Index an den Kern, der allein das echte Ziel kennt.

Unter Windows gilt das nicht. Geprüft im Quelltext von wry 0.57.3 (von Tauri 2.12.7 benutzt,
`wry/src/webview2/mod.rs`):

- wry abonniert für `with_navigation_handler` nur `NavigationStarting` (Zeile 781). Das Ereignis
  feuert in WebView2 nur für das Dokument der obersten Ebene. Navigationen in einem `iframe`
  melden sich über `FrameNavigationStarting`, das wry nicht abonniert. Tauris `on_navigation`
  sieht Klicks im Mail-Frame unter Windows also nicht.
- `NewWindowRequested` abonniert wry immer (Zeile 808). Ohne eigenen Handler setzt es
  `Handled = true`, es öffnet sich also kein Fenster. Mit Tauris `on_new_window` entscheidet die App.

Folge ohne Gegenmaßnahme: Ein Klick auf einen Link navigiert den Mail-Frame tatsächlich zu
`https://link.invalid/…` (die App-CSP erlaubt `frame-src http: https:`, 0005). Das echte Ziel
leakt nicht, denn im Dokument steht nur der Platzhalter. Aber der Kern erfährt vom Klick nichts,
der Link ist tot, der Frame zeigt eine Fehlerseite, und es geht eine DNS-Anfrage nach
`link.invalid` hinaus (die nie aufgelöst wird). Rutscht der Bereinigung je eine andere
Navigation durch, halten sie unter Windows nur Sandbox und CSP auf, kein Hook.

## Geht es ohne `unsafe`?

Nicht über `FrameNavigationStarting`. Der Weg ist:
`WebviewWindow::with_webview` → `PlatformWebview::controller()` (beides sicher) →
`ICoreWebView2Controller::CoreWebView2()` → `ICoreWebView2::add_FrameNavigationStarting(handler)`,
im Handler `ICoreWebView2NavigationStartingEventArgs::Uri()` und `SetCancel()`.

- Alle vier COM-Aufrufe sind in den `windows`-Bindings von webview2-com-sys 0.39.1 `unsafe fn`.
  Sicher sind nur die Hilfen von webview2-com: `NavigationStartingEventHandler::create` (baut
  das COM-Objekt aus einer Closure) und `take_pwstr` (kopiert und gibt den String frei).
- `forbid` lässt sich nicht lokal mit `#[allow(unsafe_code)]` überstimmen. Nachgewiesen mit
  `cargo xwin clippy` in einer Hilfs-Crate: `error[E0453]: allow(unsafe_code) incompatible with
  previous forbid` und `error: usage of an unsafe block`.
- Tauri 2.12.7 bietet keinen anderen Hook für Frame-Navigationen. `on_navigation` und
  `on_page_load` sehen nur den Hauptframe, `on_web_resource_request` nur Tauris eigenes Protokoll.

Ohne `unsafe` umgesetzt ist dagegen das Sperren neuer Fenster: `on_new_window` gibt für jede
Anfrage `NewWindowResponse::Deny` zurück (`app/src-tauri/src/lib.rs`, alle Plattformen). wry
setzt unter Windows dann `Handled = true`.

## Optionen

**A. Workspace `deny` statt `forbid`, eine Datei mit `allow`.** `unsafe_code = "deny"` in
`[workspace.lints.rust]`; nur `app/src-tauri/src/webview2_frames.rs` trägt
`#![allow(unsafe_code)]`. `scripts/check.sh` prüft per `grep`, dass `allow(unsafe_code)` sonst
nirgends steht.
- Vorteil: kleinster Umbau, das Feature fällt weg.
- Nachteil: `forbid` fällt für alle Crates. Der Schutz hängt dann an einer `grep`-Prüfung statt am
  Compiler.

**B. Eigene kleine Crate nur für Plattform-Aufrufe (Vorschlag).** Zum Beispiel
`crates/mail-platform`, nur `cfg(windows)`-Code, mit eigener Lint-Tabelle (`unsafe_code = "deny"`
plus `allow` in genau einem Modul). Alle bestehenden Crates behalten `forbid` unverändert. Die
Crate hängt nur an webview2-com und bietet eine sichere Funktion an, etwa
`guard_frame_navigations(&ICoreWebView2Controller, policy)`. Die Tauri-Verdrahtung (`with_webview`)
bleibt in `mail-app` und kommt ohne `unsafe` aus.
- Vorteil: Der Compiler garantiert `forbid` weiter überall, wo es heute gilt. Die unsichere
  Fläche liegt an einer Stelle, die man prüfen kann. Spätere Systemsignale, die 0008 wegen `unsafe`
  ausgelassen hat (Ruhezustand, Netzwechsel), hätten denselben Ort.
- Nachteil: eine fünfte Crate (die Architektur sagt „vier Crates statt elf“). Die Lint-Tabelle
  steht doppelt, denn `[lints] workspace = true` lässt keine Abweichung zu.

**C. Ohne `unsafe`: Platzhalter als eigenes Tauri-Protokoll.** Unter Windows schreibt die
Bereinigung Links auf `http://vmlink.localhost/<token>/<index>` um, und die Shell registriert
`register_asynchronous_uri_scheme_protocol("vmlink", …)`. wry fängt Anfragen an dieses Schema über
`WebResourceRequested` ab, mit `ICoreWebView2_22` ausdrücklich auch aus iframes
(`COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL`, Zeile 1058). Der Handler meldet den Klick an
den Kern und antwortet mit `204 No Content`; eine Navigation mit 204 bleibt ohne Wirkung, der Frame
behält sein Dokument. Alle anderen Frame-Navigationen sperrt die CSP: `tauri.windows.conf.json` mit
`frame-src 'self' http://vmlink.localhost`.
- Vorteil: kein `unsafe`, nur öffentliche Tauri-Schnittstellen.
- Nachteile: Der Platzhalter ist je Plattform verschieden, Bereinigung bzw. Engine müssen die
  Plattform kennen. Fremde Navigationen hält nur eine Sperre auf (CSP) statt einem Hook. Tauri
  behandelt eigene Protokolle als lokal und damit als IPC-berechtigt (0005, Rückweg). Das ist
  harmlos, solange der Handler nie ein Dokument ausliefert, muss aber im Blick bleiben. Ungeprüft
  sind das Verhalten bei 204 und ob Anfragen aus einem Sandbox-Frame ohne `allow-scripts` den
  Handler erreichen.

**D. Nichts tun.** Links in HTML-Mails wären unter Windows tot. Für Stufe 3 nicht annehmbar.

## Vorschlag

**B.** Die Regel „kein `unsafe`“ ist eine der wenigen Garantien, die der Compiler für eine App
mit feindlichen Eingaben gibt. Sie sollte nicht für alle Crates fallen, nur damit vier COM-Aufrufe
in einer Datei möglich werden. **A** ist der einfachere Rückweg, falls eine fünfte Crate nicht
gewollt ist. **C** bleibt der Weg, falls `unsafe` ganz ausgeschlossen bleiben soll; dann muss
Stufe 3 den Platzhalter je Plattform wählen.

## Stand des Codes

- `app/src-tauri/src/navigation.rs` (alle Plattformen, ohne `unsafe`): die Regel für den Mail-Frame.
  Erlaubt sind nur `about:srcdoc` und `about:blank`. Ein exakter Platzhalter (über
  `mail_model::parse_link_placeholder`) wird abgebrochen und gemeldet, alles andere abgebrochen
  und mit Schema und Host protokolliert. Zwei Tests, auch für knapp daneben liegende URLs (http,
  Port, Query, Fragment, Nutzerinfo, fremde Domain).
- `app/src-tauri/src/webview2_frames.rs` (`cfg(windows)` und Feature `webview2-frame-guard`):
  abonniert `FrameNavigationStarting` und fragt die Regel. Eine URL, die sich nicht lesen lässt,
  wird abgebrochen. Jeder `unsafe`-Block hat einen `SAFETY`-Kommentar.
- `app/src-tauri/src/lib.rs`: `install_frame_guard` hängt den Wächter nach dem Bau des Fensters
  ein. Die Meldung an den Kern ist bis Stufe 3 eine Log-Zeile. Stufe 3 reicht sie an dieselbe
  Stelle weiter wie den macOS-Hook.
- Neue direkte Abhängigkeit, nur unter Windows und nur mit dem Feature: `webview2-com` =0.39.1
  (MIT, Release 11.03.2026), genau die Version, die wry benutzt. Es kommt also keine neue Crate in
  den Build. Nach der Entscheidung gehört sie in die Stack-Tabelle von `docs/ARCHITECTURE.md`.

**Nachweise (22.11.2026):**

- Hilfs-Crate außerhalb des Workspace, die genau diese Datei per `#[path]` einbindet, mit
  `unsafe_code = "deny"` und den Clippy-Lints des Workspace plus `undocumented_unsafe_blocks`:
  `cargo xwin clippy --target x86_64-pc-windows-msvc -- -D warnings` ist fehlerfrei.
- Gegenprobe mit `forbid`: E0453 und „usage of an unsafe block“, wie erwartet.
- Workspace ohne Feature: `cargo xwin clippy --workspace --all-targets --target
  x86_64-pc-windows-msvc -- -D warnings` sauber für alle Crates außer `mail-content` (dort am
  22.11. ein Clippy-Fehler aus laufender Arbeit, auf macOS genauso); `mail-app` mit `--features e2e`
  ebenso sauber. Mit `--features webview2-frame-guard` scheitert `mail-app` im Workspace wie
  erwartet an E0453 und vier „usage of an unsafe block“.
- **Ungeprüft: alles zur Laufzeit unter Windows.** Ob `FrameNavigationStarting` beim Klick in
  einem Sandbox-Frame ohne `allow-scripts` feuert, ob `about:srcdoc` dabei als URL ankommt (sonst
  bliebe der Mail-Frame leer), ob `SetCancel` jede Anfrage verhindert und ob verschachtelte
  Frames mitgemeldet werden. Dazu kommen die übrigen offenen Punkte aus 0005 (CSP-Vererbung,
  Preconnect, `_blank`).

## Nach der Entscheidung (bei B)

1. Crate anlegen, das Modul ohne Tauri-Teil hineinverschieben, `mail-app` hängt unter
   `cfg(windows)` daran. Das Feature `webview2-frame-guard` entfällt.
2. `docs/ARCHITECTURE.md`: Repository- und Crate-Übersicht, Stack-Tabelle (webview2-com),
   Sicherheitsgrenzen Punkt 5 und 6 (Windows-Hook), die Regel zu `unsafe` mit der Ausnahme.
3. Stufe 3 meldet Klicks aus beiden Hooks an denselben Kern-Aufruf.
4. Windows-Handprüfung: der Korpus aus 0005 und `testmail/corpus/hostile/` unter WebView2, mit
   Zähl-Server und Positivkontrollen (`docs/testing/windows-checklist.md`, Abschnitt Stufe 3).

## Rückweg

Option C ohne `unsafe`. Die Regel (`navigation.rs`) bleibt dieselbe; nur die Stelle, die
Navigationen meldet, ändert sich.

## Umsetzung (Stufe 3, 22.11.2026)

Option B ist gebaut, von der Engine- und Shell-Sitzung von Stufe 3.

- **Crate `crates/webview2-guard`** (nur `cfg(windows)` hat Inhalt, auf anderen Plattformen ist
  sie leer): eine Funktion `guard_frame_navigations(&ICoreWebView2Controller, Arc<FramePolicy>)`.
  Eigener `[lints]`-Block statt `workspace = true`: `unsafe_code = "deny"`, sonst dieselben
  Clippy-Regeln wie der Workspace plus `undocumented_unsafe_blocks` und
  `multiple_unsafe_ops_per_block`. Vier `unsafe`-Stellen (`CoreWebView2`, `Uri`, `SetCancel`,
  `add_FrameNavigationStarting`), jede mit eigenem `#[allow(unsafe_code)]` an der Anweisung und
  einem `// SAFETY:`-Kommentar. Abhängigkeiten: `webview2-com` =0.39.1 (wie bisher, die Version
  von wry) und `tracing`. Alle anderen Crates behalten `forbid`.
- **`mail-app`**: hängt unter `cfg(windows)` an der Crate, über das Feature
  `webview2-frame-guard`, das jetzt **standardmäßig an** ist (`default = ["webview2-frame-guard"]`;
  abschaltbar zur Fehlersuche). `app/src-tauri/src/webview2_frames.rs` ist entfernt; die Shell
  holt den Controller über Tauris sicheres `with_webview` und übergibt die Regel aus
  `navigation.rs`. Ein Klick auf einen Platzhalter geht an denselben Kern-Aufruf wie unter macOS
  (`AppCore::link_activated` → Ereignis `linkActivated`).
- **macOS**: Der Navigations-Hook des Hauptfensters (`on_navigation`) ist jetzt gesetzt
  (`navigation::decide_main_navigation`): App-URLs (`tauri://localhost`, `http(s)://tauri.localhost`,
  unter `cfg(dev)` der Vite-Server), dann die Frame-Regel (`about:srcdoc`, `about:blank` erlauben;
  exakter Platzhalter → abbrechen und melden; alles andere abbrechen und mit Schema und Host
  protokollieren).
- **`scripts/check.sh`**, Schritt „unsafe only in webview2-guard“: `grep` nach `unsafe {`,
  `unsafe fn|impl|extern|trait` und `allow(… unsafe_code …)` in allen `.rs` unter `crates/` und
  `app/src-tauri/src`, dazu `unsafe_code = "allow|warn|deny"` in jeder `Cargo.toml`; Treffer
  außerhalb von `crates/webview2-guard/` lassen den Lauf scheitern. Positivkontrolle: In der
  Guard-Crate muss das Muster mindestens zwei Treffer finden. Gegenprobe am 22.11.: eine Datei mit
  `unsafe { 1 }` in `crates/mail-model/src/` ließ die Prüfung mit dem Fundort scheitern.

**Nachweise (22.11.2026):**

- `cargo xwin clippy -p webview2-guard -p mail-app -p mail-model -p mail-store -p mail-content
  --all-targets --target x86_64-pc-windows-msvc -- -D warnings` und `cargo xwin clippy -p
  mail-engine --lib …`: fehlerfrei, mit dem Feature (Standard). Der volle Lauf `--workspace
  --all-targets` scheiterte am selben Tag nur an Testcode paralleler Sitzungen (Stufe 4:
  `outbox/server_tests.rs`, `bool_assert_comparison`; Stufe 5: `mail-imap/src/smtp/tests.rs`,
  `Send` als Variante importiert), nicht an dieser Crate.
- macOS: `cargo clippy -p mail-app --all-targets -- -D warnings`, mit und ohne `--features e2e`,
  fehlerfrei; die Guard-Crate ist dort leer.
- **Weiter ungeprüft: alles zur Laufzeit unter Windows** (siehe oben). Die Windows-Handprüfung
  (`docs/testing/windows-checklist.md`, Abschnitt Stufe 3) steht aus.
