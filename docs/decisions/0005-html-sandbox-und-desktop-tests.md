# 0005 — HTML-Sandbox unter WKWebView und Desktop-Tests mit WebdriverIO

- **Datum:** 22.11.2026
- **Stufe:** 0, Spike D (`spikes/tauri-app/`, Ansicht „Sandbox“, Test `e2e/sandbox.e2e.mjs`)
- **Status:** angenommen für macOS; Windows (WebView2) ungeprüft

## Kontext

`docs/ARCHITECTURE.md` (Sicherheitsgrenzen, Punkt 3) verlangt: HTML aus Mails wird in einem `iframe`
mit `sandbox` ohne `allow-scripts` und mit eigener CSP angezeigt. Dort läuft kein JavaScript, also ist
die Tauri-Schnittstelle nicht erreichbar. Links öffnen sich nie direkt.

Der Spike prüft diese **letzte Verteidigungslinie allein**: Es gibt noch keine Bereinigung mit ammonia
und keine Ersetzung von URLs (beides kommt in Stufe 3). Offen waren diese Fragen:

1. Läuft wirklich kein Script, auch nicht über Ereignis-Attribute, `javascript:`-Links oder SVG?
2. Löst ein blockiertes externes Bild eine Verbindung aus?
3. Welche CSP gilt in einem `srcdoc`-iframe? Erbt es die CSP der App? Greift eine eigene
   `<meta http-equiv>`-CSP? Kennt WebKit das `csp`-Attribut?
4. Wie fängt man Link-Klicks ab? Variante A ist `allow-same-origin` mit Klickbehandlung im
   Elternfenster, Variante B ist ein opaker Ursprung mit Navigationsabfang in Rust.
5. Spritzt Tauri `__TAURI_INTERNALS__` oder IPC in Unter-Frames ein?
6. Funktionieren Desktop-Tests mit WebdriverIO und `@wdio/tauri-service` unter macOS? Bleibt der
   eingebettete WebDriver aus ausgelieferten Builds heraus?

## Aufbau des Spikes

- **Korpus:** `src/sandbox/corpus.ts`, 53 feindliche Schnipsel. Jeder Schnipsel liegt in einem eigenen
  iframe (240×56 px). Jeder Vektor zeigt auf einen eigenen Pfad des Zähl-Servers, `/v/<vektor>/…`.
  Script-Vektoren versuchen zusätzlich, sich ohne Netz zu melden: per `postMessage` an das
  Elternfenster, über Marker `window.__vmRan` und `top.document.title`, mit
  `__TAURI_INTERNALS__.invoke('sandbox_breach')` (eigenes und das des Elternfensters), mit `fetch`
  und mit `new Image()`.
- **Varianten:** Jede Variante zeigt den ganzen Korpus. Die Varianten sind in `src/sandbox/frames.ts`
  beschrieben.

  | Satz | `sandbox` | Meta-CSP im `srcdoc` | Klickbehandlung im Elternfenster |
  |---|---|---|---|
  | A | `allow-same-origin` | ja | ja |
  | B | leer (opaker Ursprung) | ja | – |
  | N | `allow-same-origin` | nein | – |

  Satz N zeigt, was ohne Meta-CSP übrig bleibt: die Sandbox, die geerbte App-CSP und der Rust-Hook.
- **Meta-CSP im Frame:** `default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'`
- **Zähl-Server** (`e2e/counter-server.mjs`, Node ohne Abhängigkeiten): Er zählt auf `127.0.0.1:18999`
  und auf `:18997` (nur für Preconnect) **jede TCP-Verbindung und jede Anfrage** mit Pfad. Abgefragt
  und zurückgesetzt wird er über den Steuer-Port `:18998`, der selbst nicht mitzählt. Er antwortet mit
  `Connection: close` und schließt untätige Sockets nach 500 ms. So muss jede spätere Verbindung neu
  geöffnet werden und ist sichtbar.
- **Rust-Seite** (`src-tauri/src/sandbox.rs`):
  - Das Hauptfenster entsteht im Code, damit `WebviewWindowBuilder::on_navigation` gesetzt werden kann.
  - Der Hook erlaubt nur `tauri://localhost`, `about:srcdoc` und `about:blank`, dazu
    `http://tauri.localhost` (Windows) und den Vite-Server nur unter `cfg(dev)`. Alles andere wird
    abgebrochen, protokolliert und als Ereignis `SandboxEvent` an die Oberfläche gemeldet.
  - Jeder Command, der den Invoke-Handler erreicht, wird mitgeschrieben.
  - Das Command `sandbox_breach` zeichnet jeden Aufruf auf.
- **Testlauf:** `pnpm --dir spikes/tauri-app test:e2e` läuft so ab:
  1. die Test-App bauen (`--features e2e`)
  2. WebdriverIO startet den Zähl-Server und die App, jeden Satz laden, klicken und auswerten
  3. die Positivkontrollen laden
  4. Exit-Code ≠ 0, sobald ein Vektor durchkommt oder eine Kontrolle nicht anschlägt

  Klicks gibt es zweimal: als **echte NSEvents** in die WKWebView (Test-Command `e2e_native_mouse`,
  vertrauenswürdig, mit Nutzeraktivierung, wirkt auch in opaken Frames) und als **synthetischen Klick**
  vom Elternfenster (`el.click()`, so klickt der eingebettete WebDriver selbst).

## Ergebnisse je Vektor

Lauf vom 22.11.2026, zweimal hintereinander grün (je rund 46 s einschließlich inkrementellem Build).
Bedeutung der Kürzel:

- „blockiert“: keine Anfrage, keine Verbindung, kein Script, keine erlaubte Navigation
- „Hook“: Der Rust-Navigations-Hook hat das echte Ziel gesehen und abgebrochen.
- „still“: Die Sandbox verwirft den Klick. Niemand erfährt das Ziel.

Nachgewiesen wurde jeweils über den Zähl-Server (Anfragen und Verbindungen), die Script-Detektoren,
das Breach-Command, das Navigationsprotokoll in Rust und, in A und N, die Inspektion des Frames vom
Elternfenster aus (`location.href` bleibt `about:srcdoc`, kein Marker, `__TAURI_INTERNALS__` ist
`undefined`).

**Scripts.** In A, B und N gilt für alle Vektoren: kein Script, keine Verbindung.

| Vektor | Ergebnis |
|---|---|
| `<script>` inline | blockiert |
| `<script src="/sandbox-probe.js">` (aus `'self'`, die CSP erlaubt ihn, nur die Sandbox blockiert) | blockiert |
| `<script src="http://…">` | blockiert |
| `img onerror` | blockiert |
| `img onload` (data:-Bild lädt) | blockiert |
| `body onload` | blockiert |
| `details ontoggle` | blockiert |
| `input autofocus onfocus` | blockiert |
| `onmouseover` (echter Klick erzeugt ein trusted `mouseover`) | blockiert |
| `javascript:`-Link (geklickt) | blockiert |
| `iframe src="javascript:"` | blockiert, das innere Frame bleibt `about:blank` |
| SVG `<script>` | blockiert |
| SVG `onload` | blockiert |
| SVG `<animate attributeName=href values="javascript:…">` (geklickt) | blockiert |
| `meta refresh` auf `javascript:` | blockiert |
| verschachteltes `iframe srcdoc` mit Script | blockiert, die Sandbox wird vererbt |
| `iframe src="/index.html"` (die App selbst im Frame) | blockiert: Die App lädt in N, ihr JS läuft aber nicht, kein Command kommt an |

**Netz.** Ohne Interaktion, in A, B und N gleich.

| Vektor | Ergebnis |
|---|---|
| `img src` | blockiert |
| `img srcset` | blockiert |
| `picture source srcset` | blockiert |
| `video poster` | blockiert |
| `video src preload` | blockiert |
| `audio src preload` | blockiert |
| `input type=image` | blockiert |
| SVG `<image>` | blockiert |
| SVG `<use>` | blockiert |
| CSS `url()` im `style`-Attribut | blockiert |
| CSS `url()` im `<style>`-Block | blockiert |
| `@import` | blockiert |
| `@font-face` | blockiert |
| `<link rel=stylesheet>` | blockiert |
| `<link rel=prefetch>` | blockiert |
| `<link rel=preload>` | blockiert |
| **`<link rel=preconnect>`** | **Leck in A, B und N: eine TCP-Verbindung ohne HTTP-Anfrage.** WebKit wendet die CSP nicht an. `dns-prefetch` (Host `.invalid`) ist lokal nicht messbar |
| `body background` | blockiert |
| `table background` | blockiert |
| `<object data>` | blockiert |
| `<embed src>` | blockiert |
| `<iframe src="http://…">` | blockiert. In A und B greift die Meta-CSP (`frame-src`). In N sieht der **Hook** die Navigation und bricht sie ab |
| `<base href>` + relatives Bild | blockiert |
| `meta refresh` auf `http://…` | blockiert, keine Navigation |
| `<form action>` (Submit geklickt) | blockiert |

**Links.** Jeder Link wurde geklickt: in A und N nativ und synthetisch, in B nur nativ.

| Vektor | A | B | N |
|---|---|---|---|
| normaler Link `http://127.0.0.1:18999/…` | Hook | Hook | Hook |
| `target=_top` | still | still | still |
| **`target=_blank`** | still, **TCP-Verbindung zum Ziel-Host** | ebenso | ebenso |
| `a ping` | Hook, kein Ping | Hook, kein Ping | Hook, kein Ping |
| `a download` | Hook | Hook | Hook |
| SVG-`<a href>` | Hook | Hook | Hook |
| `mailto:` | still | still | still |
| eigenes Schema `vmlink:42` | still | still | still |
| `https://link.invalid/42` (so könnte die Bereinigung umschreiben) | Hook | Hook | Hook |

Für alle Sätze gilt außerdem:

- **Darstellung:** `<style>`-Block und `style`-Attribut wirken in A und N, gemessen über
  `getComputedStyle` vom Elternfenster aus. Mit der ursprünglichen App-CSP `style-src 'self'` galt
  das **nicht**: Die Farbe war `rgb(0, 0, 0)` (Lauf 3, siehe unten).
- **App-Fenster:** genau ein Fenster, URL `tauri://localhost`, Titel unverändert.
- **IPC:** Außer den Commands des Tests (`sandbox_report`, `sandbox_reset`, `e2e_native_mouse`)
  kommt nichts an. Es gibt keinen Breach.

Klickbehandlung im Elternfenster (Satz A): **Kein einziges Ereignis kam an.** Weder die
Capture-Listener für `click` und `auxclick` noch die für `mouseover` wurden aufgerufen, weder bei
echten noch bei synthetischen Klicks. Dasselbe galt für einen Listener, den der Test selbst
vom Elternfenster aus anhängte.

## Positivkontrollen

Alle Kontrollen schlagen an (Satz „controls“). Sie entscheiden darüber, ob „null Zugriffe“ überhaupt
etwas beweist.

| Kontrolle | Ergebnis | Belegt |
|---|---|---|
| Bild unter `/allowed/` (die e2e-CSP erlaubt den Pfad), Frame mit `allow-same-origin`, ohne Meta-CSP | 1 Anfrage | Der Zähler zählt. Die Sandbox allein blockiert keine Bilder |
| dasselbe im opaken Frame | 1 Anfrage | wie oben |
| dasselbe mit Meta-CSP | 0 | Die Meta-CSP im `srcdoc` wird durchgesetzt, zusätzlich zur App-CSP |
| Bild außerhalb von `/allowed/`, ohne Meta-CSP | 0 | Das `srcdoc`-iframe **erbt die CSP der App**. Zusammen mit der ersten Kontrolle ist damit belegt, dass die Sperre aus der CSP kommt |
| `csp="img-src 'none'"`-Attribut, Bild unter `/allowed/` | 1 Anfrage, `'csp' in HTMLIFrameElement.prototype` ist `false` | WebKit kennt das `csp`-Attribut nicht |
| `sandbox="allow-scripts"` + `/sandbox-probe.js` | Meldung kommt per `postMessage` | Der postMessage-Detektor erkennt ein laufendes Script |
| `sandbox="allow-scripts"` + Inline-Script | keine Meldung | Die geerbte `script-src 'self'` blockiert Inline-Scripts auch dort |
| `sandbox="allow-scripts allow-same-origin"` + Probe | Marker sichtbar, **Breach `#via-parent` aufgezeichnet** | Marker- und Breach-Detektor funktionieren. Diese Kombination gibt dem Frame vollen Zugriff auf die App (siehe unten) |
| echter Klick auf einen Link unter `/allowed/` im opaken Frame | 1 Anfrage, Hook meldet „allowed“ | Native Klicks aktivieren Links in Frames. Der Hook sieht Navigationen in Unter-Frames |
| synthetischer Klick vom Elternfenster, Frame mit `allow-same-origin` | 1 Anfrage | Synthetische Klicks aktivieren Links |
| echter Klick im Probe-Frame | trusted `mouseover`, `mousedown`, `click` | Der Zeiger kommt im Frame an. Ein Klick löst auch `mouseover` aus |

**Gegenprobe:** Satz N wurde absichtlich auf `allow-same-origin allow-scripts` geschwächt. Der Lauf
scheiterte daraufhin mit Exit-Code 1 und meldete `script-src-self: script ran`, den Breach
`script-src-self#via-parent` und das Command `sandbox_breach`. Danach wurde die Änderung zurückgenommen.

## Befund: Tauri-Einspritzung in Frames

- Tauri 2.12.7 legt seine Init-Scripts mit `for_main_frame_only: true` an (`tauri/src/manager/webview.rs`):
  `isTauri`, `__TAURI_INTERNALS__`, den Invoke-Schlüssel und die IPC-Laufzeit. In Frames **ohne**
  `allow-scripts` läuft ohnehin nichts. Vom Elternfenster aus gelesen ist `__TAURI_INTERNALS__` dort
  in allen Frames von A und N `undefined`, auch in verschachtelten Frames.
- Auch in Frames **mit** `allow-scripts` (Kontrollen) gilt: `__TAURI_INTERNALS__`, `isTauri` und
  `window.ipc` sind `undefined`.
- Vorhanden ist in jedem Frame nur `window.webkit.messageHandlers.ipc`, der Script-Message-Handler
  von wry. Ein Aufruf darüber ohne Invoke-Schlüssel wird verworfen, es gab keinen Breach. Tauri prüft
  dazu `__TAURI_INVOKE_KEY__`; für Custom Commands lehnt es entfernte Ursprünge (`about:srcdoc`) ab.
- Ein roher `fetch("ipc://localhost/sandbox_breach")` aus dem opaken Frame endet mit Status 500 und
  wird nicht ausgeführt, denn der Origin `null` ist ungültig.
- **Aber:** Mit `allow-scripts allow-same-origin` erreicht ein Frame `parent.__TAURI_INTERNALS__` und
  ruft jedes Command auf. Diese Kombination ist verboten.
- Für Stufe 3 gilt: Plugins, die Scripts mit `js_init_script_on_all_frames` einspritzen, und
  `initialization_script_for_all_frames` würden die Lage ändern. Sie dürfen nicht verwendet werden.

## Befund: Link-Klicks

- **Variante A funktioniert unter WKWebView nicht.** Ohne `allow-scripts` ruft WebKit **keine**
  Event-Listener auf Knoten des Frame-Dokuments auf, auch nicht solche, deren Funktion aus dem
  Elternfenster stammt. Das gilt für echte und für synthetische Ereignisse. `allow-same-origin` gibt dem
  Elternfenster nur **Lesezugriff** auf das DOM, etwa für Größe, berechnete Styles und Inspektion.
- **Der Navigations-Hook in Rust greift auch in Unter-Frames.** wry ruft unter macOS den
  Navigationshandler für jede `WKNavigationAction` auf, auch für Frames. Übergeben wird nur die URL,
  ohne Frame-Information. Der Hook entscheidet, bevor eine Anfrage das Gerät verlässt: null Verbindungen
  bei abgebrochenen Navigationen.
- **Die App-CSP muss solche Navigationen durchlassen.** Mit `frame-src 'self'` (über `default-src`)
  verwirft WebKit die Navigation eines Frames zu `http://…` **still, bevor der Hook sie sieht** (Lauf 3
  und 4). Dann erfährt niemand das Ziel. Mit `frame-src 'self' http: https:` sieht der Hook jede
  http(s)-Navigation. Der Hook bleibt damit die maßgebliche Sperre, auch für eingebettete
  `<iframe src=http…>`, wie Satz N zeigt.
- **Nicht beim Hook kommen an:**
  - `target=_top` und `target=_blank`: Die Sandbox verwirft sie, weil `allow-top-navigation` und
    `allow-popups` fehlen. `_blank` öffnet dabei trotzdem eine TCP-Verbindung zum Ziel-Host.
  - `mailto:` und eigene Schemata: Sie erreichen den Hook nicht, auch wenn `frame-src` sie erlaubt.
- Folge für Stufe 3: Die Bereinigung ersetzt **jedes** `href` durch einen Platzhalter unter http(s).
  Getestet ist `https://link.invalid/<n>`; der Hook sieht ihn und bricht ab. Die Bereinigung entfernt
  `target`, `ping` und `download`, und der Rust-Kern ordnet `<n>` dem echten Ziel zu. So kommen auch
  `mailto:`-Links an.

## Entscheidung für Stufe 3

1. **iframe:** `<iframe sandbox="allow-same-origin" srcdoc="…">`, sonst kein Token, nie `allow-scripts`.
   `allow-same-origin` gibt dem Elternfenster Lesezugriff auf das DOM: Höhe messen, Styles prüfen,
   Tests. Ereignisse kann das Elternfenster damit nicht abfangen. Satz B (opak) blockiert genauso
   vollständig; er ist der Rückweg.
2. **Meta-CSP in jedem `srcdoc`, Pflicht:**
   `default-src 'none'; img-src data: <Schema für cid- und freigegebene Bilder>; style-src 'unsafe-inline'; font-src data:; base-uri 'none'; form-action 'none'`.
   Sie ist die zweite, unabhängige Sperre gegen Scripts, denn `script-src` fällt auf `'none'` zurück.
   Wegen `allow-same-origin` sind sonst App-Scripts aus `'self'` erlaubt. Sie verhindert auch
   `eval` vom Elternfenster in den Frame; der WebDriver scheitert daran mit `EvalError`.
3. **App-CSP:** Das `srcdoc` erbt sie, also muss sie die Mail-Darstellung zulassen.

   ```
   default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: <Bild-Schema>;
   frame-src 'self' http: https:; connect-src ipc: http://ipc.localhost; object-src 'none';
   base-uri 'none'; form-action 'none'; frame-ancestors 'none'
   ```

   - `'unsafe-inline'` bei `style-src` ist nötig, sonst wirkt kein Mail-CSS.
   - `frame-src http: https:` ist nötig, damit Link-Navigationen beim Hook ankommen.
   - `index.html` darf keine Inline-Styles enthalten. Sonst hängt Tauri Hashes an `style-src`, und
     `'unsafe-inline'` wird wirkungslos (`tauri/src/manager/mod.rs` `set_csp`; ungeprüft, ob das hier
     zuträfe).
4. **Links:** Der Navigations-Hook (`on_navigation`) am Hauptfenster ist die einzige Stelle, die
   Link-Ziele erfährt. Er erlaubt nur App-URLs, `about:srcdoc` und `about:blank`, bricht alles andere
   ab und schickt ein typisiertes Ereignis an die Oberfläche. Dort folgt die Linkprüfung, dann das
   Öffnen im System-Browser. Die Bereinigung schreibt Links um (siehe Befund Link-Klicks).
5. **Bereinigung (ammonia) als Pflicht, nicht als Kür:** Zwei Lecks hält nur sie auf: `<link>` muss
   vollständig entfernt werden (Preconnect), und `target` muss weg (Preconnect bei `_blank`). Beide
   gehören in den Korpus unter `testmail/corpus/hostile/`.
6. **Tauri-Schnittstelle:** Sie bleibt im Hauptframe. Es gibt keine Init-Scripts für alle Frames.
   Die Kombination `allow-scripts allow-same-origin` ist im Code verboten, ein Test prüft das Attribut.

## Entscheidung: Desktop-Tests

**WebdriverIO mit `@wdio/tauri-service` und eingebettetem WebDriver funktioniert unter macOS.** Die
Sitzung meldet `webkit 605.1.15 on macos`; geprüft wurde unter macOS 27.2.1 mit WebKit 22625.2.8.11.9.

| Teil | Version | Lizenz |
|---|---|---|
| `@wdio/tauri-service` | 1.5.1 (24.10.2026) | MIT |
| `tauri-plugin-wdio-webdriver` (Crate, eingebetteter W3C-WebDriver) | =1.5.1 (24.10.2026) | MIT |
| `webdriverio`, `@wdio/cli`, `@wdio/local-runner`, `@wdio/mocha-framework` | 9.31.9 | MIT |
| `@wdio/spec-reporter` / `@wdio/globals` | 9.31.2 / 9.31.3 | MIT |
| `objc2` / `objc2-foundation` / `objc2-app-kit` (nur Feature `e2e`, echte Klicks) | 0.6.5 / 0.3.2 / 0.3.2 | MIT |

- **Nur in Test-Builds.** Die Cargo-Abhängigkeit ist `optional` und wird nur über das Feature
  `e2e = ["dep:tauri-plugin-wdio-webdriver", …]` eingebunden. Registriert wird sie mit
  `#[cfg(feature = "e2e")]`, ebenso das Test-Command `e2e_native_mouse` und die Ausnahme `/allowed/`
  im Navigations-Hook. Nachweis mit `pnpm test:release-clean` (`e2e/check-release.mjs`):
  - Das e2e-Binary enthält alle fünf Marker (`TAURI_WEBDRIVER_PORT`, `wdio-webdriver`, die
    Startmeldung des Plugins, `e2e_native_mouse`, `wdioEvalResult`). Das ist die Positivkontrolle.
  - Das Release-Binary (`CI=true pnpm tauri build --bundles app`, 5,4 MB) enthält keinen davon.
  - `cargo tree -i tauri-plugin-wdio-webdriver` findet das Paket ohne Feature nicht, mit
    `--features e2e` schon.
  - Zur Laufzeit antwortet `GET 127.0.0.1:4499/status` beim e2e-Build mit 200. Beim Release-Build
    mit `TAURI_WEBDRIVER_PORT=4499` lauscht nichts.
  - Der Spike-C-Selbsttest des Release-Builds besteht weiter (`SPIKE_SELFTEST=1`, Exit 0).
- **Ohne `tauri-plugin-wdio`.** Das zweite Plugin des Projekts (Execute-API, Mocks, Log-Weiterleitung)
  verlangt `withGlobalTauri: true`, ein Frontend-Import und eigene Capabilities. Es wird nicht gebraucht:
  Standard-WebDriver mit `browser.execute` reicht. Dabei gibt es einen Stolperstein. Der Service ruft
  vor `getTitle`, `$` und Klicks `plugin:wdio|get_window_states` auf und wartet ohne das Plugin
  jedes Mal 5 s. Ein `browser.switchToWindow(<eigener Handle>)` am Anfang schaltet das ab.
- **Grenzen des eingebetteten WebDrivers unter WKWebView:**
  - Klicks sind synthetisches JS (`el.click()`), ohne Nutzeraktivierung.
  - `switchToFrame` und `$` im Frame laufen über `contentWindow.eval` und scheitern an der Frame-CSP
    (`EvalError … 'unsafe-eval'`). In opake Frames kommt er gar nicht.
  - Tests prüfen Mail-Frames deshalb über Detektoren (Zähl-Server, postMessage, Breach-Command,
    Navigationsprotokoll) und über DOM-Lesezugriff vom Elternfenster.
- **Echte Klicks:** Das Test-Command `e2e_native_mouse` schickt `NSEvent`-Mausereignisse direkt an die
  WKWebView (`mouseDown:`/`mouseUp:`). Die Ereignisse sind vertrauenswürdig und landen pixelgenau. Zum
  Viewport kommt der Versatz der Titelleiste (32 px), berechnet aus `bounds − innerHeight`.
  - `NSWindow.sendEvent` wirkte nicht.
  - `mouseMoved` erreicht WebKit nur bei einem Key-Fenster. Eine per SSH gestartete App wird nie key,
    `activateIgnoringOtherApps` half nicht. Hover entsteht deshalb über den Klick, der vorher
    `mouseover` liefert.
  - Der Test braucht eine angemeldete GUI-Sitzung; es öffnet sich kurz ein Fenster.
- **WebdriverIO 10** (aktuell 10.2.0) verlangt `@wdio/tauri-service` 2.0.0-next.0 (Vorabversion vom
  08.11.2026). Bis es ein stabiles 2.0 gibt, bleibt das Projekt auf 9.31.
- **Laufzeit:** ein voller Lauf `pnpm test:e2e` dauert 46 s: inkrementeller Debug-Build, 5 Tests,
  rund 220 Frames.

Damit gilt die Zeile „Tests“ in der Stack-Tabelle für macOS, mit Versionen. Eine Alternative musste
nicht geprüft werden. Der Rückweg wäre ein In-App-Testlauf wie `SPIKE_SELFTEST` aus Spike C, über
den Zähl-Server von Node aus gesteuert. Die Detektoren des Spikes funktionieren auch ohne WebDriver.

## Was der Test als bekanntes Leck führt

`e2e/sandbox.e2e.mjs` hat eine Liste `KNOWN_LEAKS`. Darin stehen `link-preconnect` (Port 18997) und
`link-target-blank` (eine Verbindung zu 18999 beim Klick). Beides sind TCP-Verbindungen ohne
HTTP-Anfrage, die Sandbox und CSP nicht verhindern. Jede andere Verbindung und jede HTTP-Anfrage
lässt den Lauf scheitern. In Stufe 3 läuft der Korpus durch die Bereinigung. Dann muss die Liste leer
sein und der Zähler auch **Verbindungen**, nicht nur Anfragen, mit null melden.

## Nachweise

- `pnpm --dir spikes/tauri-app test:e2e`: 5 Tests bestanden, Exit 0, zweimal hintereinander
  (40,2 s und 40,4 s Testzeit). Die Ergebnisse je Vektor schreibt der Lauf nach
  `spikes/tauri-app/e2e/results/sandbox-results.json`; die Datei ist nicht eingecheckt.
- Gegenprobe mit absichtlich geschwächter Sandbox: Exit 1 mit den erwarteten Meldungen.
- `cargo test` im Spike: 9 Tests, darunter 3 für die Navigationsregel (erlaubt, verboten, die
  `/allowed/`-Ausnahme ohne Feature). `cargo clippy --all-targets -D warnings` lief mit und ohne
  `e2e` fehlerfrei, ebenso `cargo fmt --check` und `pnpm typecheck`.
- `pnpm test:release-clean`: OK, siehe oben.
- Zwischenläufe, die zu den Befunden führten:
  - **Lauf 2/3:** App-CSP `style-src 'self'`. `<style>` und `style=""` im Frame bleiben wirkungslos
    (`rgb(0, 0, 0)`); `frame-src 'self'` verschluckt alle Link-Navigationen vor dem Hook.
  - **Debug-Lauf:** Ein Capture-Listener, den das Elternfenster an das Frame-Dokument hängt, wird
    bei `a.click()` nicht aufgerufen.

## Ungeprüft und offen

- **Windows (WebView2), alles ungeprüft:**
  - wry abonniert unter Windows nur `NavigationStarting`, nicht `FrameNavigationStarting`
    (`wry/src/webview2/mod.rs`). Der Navigations-Hook sieht dort Unter-Frames also **wahrscheinlich
    nicht**. Stufe 3 braucht dann einen eigenen Handler über `with_webview` →
    `CoreWebView2::add_FrameNavigationStarting`, oder eine andere Lösung.
  - Ob Chromium Listener des Elternfensters in Frames ohne Scripts aufruft (das hieße Variante A unter
    Windows), ist offen.
  - Weiter offen: CSP-Vererbung ins `srcdoc` (laut Spezifikation ja), das Verhalten bei Preconnect
    und `_blank`, das `csp`-Attribut (Chromium unterstützt es), die App-URL `http://tauri.localhost`,
    IPC über `http://ipc.localhost`.
  - Das Plugin `tauri-plugin-wdio-webdriver` nennt Windows als unterstützt (über WebView2); im
    Spike lief es dort nicht. `e2e_native_mouse` gibt es nur für macOS.
- DNS-Anfragen sind lokal nicht messbar. Ob `dns-prefetch` oder ein Klick auf `https://link.invalid/…`
  eine DNS-Anfrage auslöst, ist ungeprüft. `.invalid` löst nie auf; bei den getesteten
  IP-Adressen gab es nach Hook-Abbruch keine Verbindung.
- Höhenmessung: Das Elternfenster kann `scrollHeight` lesen. Ob ein `ResizeObserver` auf Knoten im
  Frame feuert, ist ungeprüft; Listener tun es nicht. Für echtes Auto-Sizing muss Stufe 3 das prüfen.
- Freigegebene externe Bilder über den Rust-Kern (ein eigenes Schema in `img-src`) sind nicht
  Teil des Spikes.
- `mailto:` gelangt nicht zum Hook; ohne Umschreiben in Stufe 3 wären `mailto:`-Links tot.
- Lizenzen der Testwerkzeuge: Sie sind nur Dev-Abhängigkeiten und werden nicht ausgeliefert. Darunter
  sind `css-value` (Lizenz „Unknown“), `@promptbook/utils` (CC-BY-4.0) und `jszip`
  (MIT oder GPL-3.0). Alle kommen transitiv über webdriverio. `pnpm-workspace.yaml` verbietet die
  Download-Skripte von `edgedriver`, `geckodriver` und `esbuild` (`allowBuilds: false`); sie werden
  nicht gebraucht.

## Rückweg

- **Opaker Frame statt `allow-same-origin`:** `sandbox=""`. Satz B blockiert alles genauso. Verloren
  geht nur der DOM-Lesezugriff des Elternfensters, also Höhe und Inspektion. Danach folgt der
  Lesebereich mit fester Frame-Höhe und eigenem Scrollen, und die Tests nutzen nur Detektoren.
- **Eigene CSP ohne Vererbung:** Stört `'unsafe-inline'` in der App-CSP, kann die Mail statt als
  `srcdoc` als Dokument über ein eigenes Protokoll kommen (`register_uri_scheme_protocol`, etwa
  `vmmail://`). Sie bekommt dann einen eigenen CSP-Header, und die App-CSP bleibt streng. Das kostet
  den DOM-Zugriff (anderer Ursprung). Zu beachten: Tauri behandelt eigene Protokolle als **lokal**,
  also als IPC-berechtigt. Der Weg ist nicht gebaut.
- **Ohne WebdriverIO:** ein In-App-Testlauf (`SPIKE_SELFTEST`-Muster) mit denselben Detektoren,
  gesteuert von einem Node-Skript, das Zähl-Server und App startet.
