# 0042 — Anmeldung mit Microsoft und Google: OAuth 2.0 mit PKCE im System-Browser, IMAP und SMTP mit Token

**Status:** entschieden (Stufe 14, Blöcke B und C) · **Datum:** 23.11.2026 · **Code:**
`crates/mail-engine/src/oauth/`, `crates/mail-engine/src/credentials.rs`, `crates/mail-imap/src/sasl.rs`,
`crates/mail-imap/src/{backend,smtp/mod}.rs`, `crates/mail-model/src/oauth.rs`,
`app/src-tauri/src/{oauth.rs,commands/oauth.rs}`, `app/src/features/settings/accounts/OAuthSignIn.tsx`

## Kontext

Microsoft lässt Outlook.com, Hotmail und Microsoft 365 seit 2022/2024 nicht mehr mit einem Passwort
über IMAP und SMTP herein, nur mit OAuth 2.0. Gmail geht mit App-Passwort (Block A, 0034); „Mit Google
anmelden“ ist für Nutzer ohne App-Passwort (Google Workspace mit gesperrten App-Passwörtern) gedacht.

Der Mensch hat noch keine App bei Microsoft Entra oder in der Google Cloud registriert, es gibt also
keine Client-IDs. Alles außer der Registrierung ist gebaut und lokal getestet; die Client-IDs sind
Build-Konfiguration.

Der Testserver Stalwart 1.1.1 ist selbst ein OAuth-Server: Authorization Code mit PKCE (nur S256),
Refresh-Token mit Rotation, Widerruf nach RFC 7009, offene dynamische Client-Registrierung
(RFC 7591), Loopback-Weiterleitung mit beliebigem Port (RFC 8252), `XOAUTH2` und `OAUTHBEARER` für
IMAP. Für SMTP schaltet `testmail/provision.py` beide Verfahren zusätzlich frei. Damit läuft der ganze
Weg gegen einen echten Server, nur nicht gegen Microsoft und Google.

## Entscheidungen

1. **Ablauf: Authorization Code mit PKCE (S256) im System-Browser, Loopback-Weiterleitung.** Der
   Kern erzeugt `code_verifier` und `state` (je 256 Bit Zufall, 43 Zeichen base64url, über den
   Krypto-Provider der App), öffnet `127.0.0.1:<freier Port>` (für `localhost`-Weiterleitungen auch
   `::1` auf demselben Port) und schickt den Browser zur Anmeldeseite des Anbieters mit
   `response_type=code`, `client_id`, `redirect_uri`, `scope`, `state`, `code_challenge`,
   `code_challenge_method=S256` und `login_hint` (die Adresse). Angenommen wird genau eine Antwort:
   `GET /` mit genau dem gesendeten `state` (Vergleich in konstanter Zeit, jeder Parameter höchstens
   einmal). Andere Pfade (`/favicon.ico`, `/callback`) bekommen 404, ein fehlender oder fremder
   `state` 400; der Kern wartet dann weiter, so dass eine untergeschobene Antwort die Anmeldung weder
   abschließen noch abbrechen kann. Höchstens 16 KiB je Anfrage, 5 s je Verbindung, nichts davon im
   Log. Die Seite für den Browser (Titel, „fertig“, „fehlgeschlagen“) kommt als Text aus der
   Oberfläche (`OAuthPageTexts`, i18n), wird escaped und mit `Content-Security-Policy: default-src
   'none'` ausgeliefert. Der Code wird mit dem Verifier eingetauscht. Zeitlimit 5 Minuten,
   Abbrechen über `cancelOauthSignIn`; eine neue Anmeldung bricht die laufende ab.
2. **Weiterleitungsadressen.** Microsoft: `http://localhost:<port>` (in Entra als `http://localhost`
   registriert; Entra ignoriert den Port bei `localhost`, die Portal-Oberfläche nimmt `127.0.0.1`
   nicht ohne Manifest-Änderung). Google und alle anderen: `http://127.0.0.1:<port>/` (RFC 8252,
   Abschnitt 7.3; Googles Desktop-Clients brauchen keine Registrierung der Adresse). Stalwart
   vergleicht den Pfad genau, daher der Schrägstrich.
3. **Den Browser öffnet der Kern**, mit demselben Öffner wie Links (`open`, unter macOS `/usr/bin/open`,
   unter Windows `ShellExecuteW`). Die WebView zeigt die Anmeldeseite nie und öffnet kein Fenster
   dafür. Das ist die einzige Stelle, an der die App selbst eine Webseite öffnet: nur nach Klick auf
   „Mit Microsoft/Google anmelden“ oder „Erneut anmelden“, nur die feste Anmeldeadresse des Anbieters.
4. **Tokens.** Das Refresh-Token liegt im System-Schlüsselbund unter der Schlüsselbund-Referenz des
   Kontos `oauth:<microsoft|google>:<Konto-UUID>` (JSON `{"v":1,"refreshToken":…,"pieces":n}`),
   das Access-Token nur im Speicher. Windows Credential Manager nimmt höchstens 2.560 Byte je Eintrag
   (1.280 UTF-16-Zeichen); ein langes Refresh-Token wird deshalb in Stücke von 1.000 Zeichen geteilt
   (`<Referenz>#1`, `#2`, …, höchstens 8000 Zeichen), Löschen entfernt alle Stücke.
   `Credentials::get` verweigert solche Einträge: ein Token geht nie als Passwort an einen Server.
   Verbindungen fragen `Credentials::login`: Passwort, oder ein Access-Token, das noch mindestens fünf
   Minuten gilt (bei kurzlebigen Tokens die halbe Laufzeit), sonst erneuert (eine Erneuerung je Konto
   zur Zeit; ein rotiertes Refresh-Token ersetzt das gespeicherte). Ein Token gibt es nur für die
   IMAP- bzw. SMTP-Server des Anbieters (Liste im Profil), auch wenn die Kontoeinstellungen sich
   ändern. Lehnt ein Server das Token ab (`AuthFailed`), wird es einmal erneuert und die Verbindung
   einmal wiederholt (Abgleich, Lesen, Outbox, Senden, SMTP-Grenzen). `invalid_grant` (abgelaufen,
   widerrufen, Zustimmung zurückgezogen) wird `oauth_sign_in_required`: das Konto steht still
   (Zustand „Fehler“, auch kein Neuverbinden nach Netzwechsel), bis der Nutzer sich erneut anmeldet.
   Netzfehler beim Token-Endpunkt sind vorübergehend (Backoff wie bei einem Mailserver).
5. **IMAP und SMTP.** `XOAUTH2`, wo angeboten oder die Fähigkeiten vor der Anmeldung unbekannt sind
   (Microsoft und Google dokumentieren es, Microsoft bietet nur dies); `OAUTHBEARER` (RFC 7628), wenn
   ein Server nur das anbietet. Mit SASL-IR in einer Zeile. Die Fehler-Challenge des Servers wird wie
   vorgeschrieben beantwortet (leer bzw. `^A`), damit das endgültige `NO`/`535` als `AuthFailed`
   ankommt. Ein Token geht nie über LOGIN oder PLAIN. SMTP: lettre kennt `XOAUTH2`; `OAUTHBEARER`
   findet ein zweites EHLO. Der Nur-Lese-Schutz gilt unverändert: `AUTHENTICATE` steht auf der
   Allowlist, alles Schreibende nicht.
6. **Profile.** Microsoft: Authority `common`
   (`https://login.microsoftonline.com/common/oauth2/v2.0/{authorize,token}`), Scopes
   `https://outlook.office.com/IMAP.AccessAsUser.All`, `https://outlook.office.com/SMTP.Send`,
   `offline_access`, IMAP `outlook.office365.com`/`outlook.office.com`, SMTP `smtp.office365.com`,
   `smtp-mail.outlook.com`, `outlook.office365.com`; kein Client-Secret; keine Widerrufsadresse
   (öffentliche Clients haben keine). Google: `https://accounts.google.com/o/oauth2/v2/auth`,
   `https://oauth2.googleapis.com/{token,revoke}`, Scope `https://mail.google.com/`,
   `access_type=offline` und `prompt=consent` (sonst kommt kein Refresh-Token), IMAP `imap.gmail.com`,
   SMTP `smtp.gmail.com`. HTTP-Client wie bei der Kontoerkennung: rustls mit Systemprüfung, HTTP/1.1,
   ohne Weiterleitungen, Proxy, Cookies und Wiederholungen, 10 s Aufbau, 30 s je Anfrage, 64 KiB
   Antwort; HTTP nur zu Loopback-Adressen in Tests.
7. **Client-IDs sind Build-Konfiguration, kein Geheimnis.** `MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID`,
   `MAIL_APP_OAUTH_GOOGLE_CLIENT_ID`, `MAIL_APP_OAUTH_GOOGLE_CLIENT_SECRET` werden beim Bauen über
   `option_env!` gelesen (`app/src-tauri/src/oauth.rs`) und stehen im Klartext im Programm. Ohne sie
   ist der Anbieter in diesem Build nicht verfügbar (`getOauthAvailability`): bei Microsoft sagt die
   Einrichtung das ausdrücklich (ein Passwort geht dort ohnehin nicht), bei Google bleibt nur das
   App-Passwort. Nichts wird geraten.
8. **Googles „Client-Secret“** für Desktop-Clients ist nach Googles eigener Aussage bei installierten
   Apps kein Geheimnis; der Token-Endpunkt verlangt es trotzdem. Es wird wie die Client-ID behandelt:
   Build-Konfiguration, nicht im Repository, nicht im Schlüsselbund, nur an Googles Token- und
   Widerrufsadresse gesendet, nie im Log. Schutz gegen fremde Apps mit derselben ID gibt PKCE, nicht
   das Secret. Ohne Secret gilt Google als nicht verfügbar.
9. **Block C: IMAP mit OAuth statt Gmail-API.** Labels, eine Mail über `X-GM-MSGID`, Archivieren und
   Gesendet laufen seit Block A über IMAP. Die Gmail-API bräuchte einen eigenen Adapter (Abgleich über
   die History-ID, Label-Modell, Batch-Anfragen, Quoten) und brächte keinen kleineren Scope: auch
   `gmail.modify` ist ein „restricted scope“ mit derselben Sicherheitsbewertung wie
   `https://mail.google.com/`. Rückweg: `MailBackend`, wie in `docs/ARCHITECTURE.md` vorgesehen.
   Ebenso Block B: Microsoft Graph erst, wenn IMAP nicht reicht oder ein Mandant IMAP sperrt.
10. **Einrichtung.** Die Oberfläche erkennt den Anbieter am Hinweis der Anbietertabelle (`outlook`)
    oder am IMAP-Server (`outlook.office365.com`, `outlook.office.com`, `imap.gmail.com`; so auch
    Microsoft-365- und Workspace-Domains nach der Online-Suche). Bei Microsoft gibt es kein
    Passwortfeld und keinen „Hinzufügen“-Knopf, nur „Mit Microsoft anmelden“; bei Google steht „Oder
    mit Google anmelden“ neben dem App-Passwort. Zustände: Warten auf den Browser (mit „Anmeldung
    abbrechen“), abgebrochen (ruhiger Hinweis), fehlgeschlagen (Fehlercode mit Grund in den
    technischen Details). Der Kern prüft die Server noch einmal (sonst `invalid_argument` vor dem
    Browser), meldet sich mit dem Token bei IMAP an und speichert das Konto wie jedes neue **nur
    lesend** (`writes_enabled = false`). In den Kontoeinstellungen steht statt „Passwort ändern“ „Mit
    Microsoft/Google angemeldet“ und „Erneut anmelden“ (hervorgehoben bei `oauth_sign_in_required`);
    `updatePassword` lehnt solche Konten ab.
11. **Entfernen.** Vor dem Löschen der Schlüsselbund-Einträge widerruft der Kern das Refresh-Token,
    wo der Anbieter das anbietet (Google und der Testserver, RFC 7009 mit `client_id`; höchstens 10 s,
    ein Fehlschlag wird geloggt und das Entfernen geht weiter). Microsoft bietet öffentlichen Clients
    keinen Widerruf; dort verfällt das Token, oder der Nutzer entfernt die App unter
    account.microsoft.com bzw. myapps.microsoft.com.
12. **Einstellungs-Sync.** Tokens wandern nie: Konto-Datensätze haben kein Feld dafür, und der
    Schlüsselbund ist ohnehin je Gerät. Ein Gerät, das ein solches Konto aus dem Sync übernimmt,
    meldet sich dort selbst an. Wie der Datensatz die Anmeldeart trägt, steht im Abschnitt
    „Einstellungs-Sync“ unten.
13. **Neue Fehlercodes:** `oauth_unavailable`, `oauth_cancelled`, `oauth_timeout`, `oauth_denied`,
    `oauth_failed`, `oauth_sign_in_required`, je mit englischem und deutschem Text.

Keine neue Abhängigkeit: PKCE (sha2), Zufall (rustls/aws-lc-rs), Formulare (url), HTTP (reqwest),
SASL (eigener Code, lettre für SMTP) sind schon im Workspace.

## Einstellungs-Sync

Stand: Der Konto-Datensatz trägt die Anmeldeart noch nicht. Ein anderes Gerät übernimmt ein
Anmelde-Konto deshalb wie ein Passwort-Konto (`synced:<zufällig>`, nur lesend, ohne Passwort) und
fragt nach einem Passwort, das bei Microsoft nicht funktioniert. Ein Anmelde-Konto kann noch nicht
das Sync-Konto sein: die Verbindung des Sync-Ordners verlangt ein Passwort und bekommt den
Schlüsselbund-Eintrag nicht (sicherer Fehlschlag mit `keychain`). Die Änderung am Einstellungs-Sync
(Anbieter im Datensatz, Übernahme als `oauth:<Anbieter>:synced:<zufällig>`, Sync-Verbindung mit
Token) folgt, sobald die gleichzeitigen Änderungen anderer Stufen an denselben Stellen eingecheckt
sind.

## Getestet

| Was | Wogegen | Tests |
|---|---|---|
| PKCE (RFC-7636-Testvektor), Zufall, base64url | Unit | `oauth::pkce::tests` |
| Autorisierungsadresse: PKCE, `state`, Scopes, `login_hint`, kein Verifier, kein Secret | Unit | `oauth::flow::tests` |
| Loopback: nur `GET /` mit `state`, fremder `state`, wiederholte Parameter, andere Pfade, Escaping | Unit | `oauth::loopback::tests` |
| Ganzer Ablauf mit Microsoft- und Google-Profil, Verifier am Token-Endpunkt geprüft, Secret nur bei Google | Attrappe eines Autorisierungsservers im Testprozess, Browser-Skript | `oauth::tests::sign_in_sends_pkce_and_gets_tokens_through_the_loopback_redirect` |
| Untergeschobene Antwort zuerst, falscher `state` allein, falscher Pfad, falscher Port, Zeitlimit, Abbrechen, Ablehnung | Attrappe | `oauth::tests::{a_forged_state_is_refused_and_the_real_answer_still_counts, only_a_forged_state_or_another_path_never_finishes_the_sign_in, a_browser_that_never_returns_times_out_and_cancel_ends_the_wait, a_refusal_by_the_provider_is_denied_with_its_reason}` |
| Erneuern vor Ablauf, nach Ablehnung, Rotation, `invalid_grant` → `oauth_sign_in_required`, Netzfehler vorübergehend, Token nur für die Server des Anbieters, nie als Passwort, Widerruf | Attrappe | `oauth::tests::*`, `oauth::store::tests` |
| Token-Antworten, Fehlerbedeutung | Unit | `oauth::token_http::tests` |
| Konto über `AppCore`: Anmeldung, Abgleich nur mit Bearer, nur Refresh-Token im Schlüsselbund, nur lesend, kein Passwortwechsel, fremde Server vor dem Browser abgelehnt, Abbrechen/Ablehnen fügt nichts hinzu, abgelaufene Freigabe stoppt den Abgleich nach genau einer Erneuerung, „Erneut anmelden“, Entfernen widerruft und löscht | Fake-IMAP-Server und Attrappe | `oauth::tests::core::*` |
| `XOAUTH2`/`OAUTHBEARER` mit und ohne SASL-IR, Fehler-Challenge, nie LOGIN/PLAIN | geskriptete IMAP- und SMTP-Server | `mail-imap` `tests::bearer_*`, `smtp::tests::bearer_*`, `sasl::tests::bearer_messages_follow_their_specifications` |
| Diagnose: kein Refresh- oder Access-Token in der Datei | Unit mit Speicher-Schlüsselbund | `diagnostics::tests::the_file_holds_facts_and_counts_but_no_secret_and_no_mail_content` |
| Ende zu Ende: Client-Registrierung, Anmeldung über Stalwarts Login-API als „Browser“ (mit untergeschobener Antwort davor), Code-Tausch mit PKCE, Konto über `AppCore` mit `ImapConnector`, Abgleich, Erneuerung nach Ablauf mit Rotation, `XOAUTH2` (STARTTLS) und `OAUTHBEARER` (TLS) mit Mitschnitt, falsches Token, SMTP-Anmeldung und Senden mit `XOAUTH2`, Widerruf beim Entfernen; Stalwart verlangt den Verifier | lokaler Stalwart 1.1.1 | `crates/mail-engine/tests/oauth_server.rs` |
| Oberfläche: Microsoft ohne Passwortfeld, Warten mit Abbrechen, Fehler mit Grund, nicht verfügbar, Google neben dem App-Passwort oder verborgen, Erkennung am Server, „Erneut anmelden“ | Vitest | `app/tests/oauth-sign-in.test.tsx` |

Nicht getestet: echte Konten bei Microsoft und Google, Windows zur Laufzeit (Schlüsselbund-Grenze,
`::1`-Listener), der Desktop-Test des Anmeldewegs (der Test-Build öffnet nichts, der Weg über
Stalwarts Login-API ist im Rust-Test abgedeckt).

## Was der Mensch bereitstellen muss

- **Microsoft Entra** (kostenlos, entra.microsoft.com → App-Registrierungen → Neue Registrierung):
  Kontotypen „Konten in einem beliebigen Organisationsverzeichnis und persönliche Microsoft-Konten“;
  Plattform „Mobile und Desktopanwendungen“ mit Umleitungs-URI `http://localhost`; „Öffentliche
  Clientflows zulassen“ = Ja; API-Berechtigungen → Microsoft Graph → delegierte Berechtigungen:
  `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access` (die App fordert sie als
  `https://outlook.office.com/IMAP.AccessAsUser.All` usw. an). Kein Client-Secret. Die Anwendungs-ID
  (Client-ID) als `MAIL_APP_OAUTH_MICROSOFT_CLIENT_ID` beim Bauen setzen. Für fremde Mandanten:
  Herausgeberprüfung (Publisher Verification) empfohlen, sonst warnt die Zustimmungsseite; manche
  Mandanten verlangen Administrator-Zustimmung.
- **Google Cloud** (console.cloud.google.com): Projekt anlegen; Gmail-API muss nicht aktiviert sein
  (IMAP/SMTP), schadet aber nicht; OAuth-Zustimmungsbildschirm „Extern“ mit App-Name,
  Support-Adresse, Scope `https://mail.google.com/`, im Status „Testing“ mit eingetragenen
  Testnutzern (höchstens 100); OAuth-Client vom Typ „Desktop-App“. Client-ID und Client-Secret als
  `MAIL_APP_OAUTH_GOOGLE_CLIENT_ID` und `MAIL_APP_OAUTH_GOOGLE_CLIENT_SECRET` beim Bauen setzen. Für
  die Veröffentlichung: Verifizierung (siehe `docs/PROVIDERS.md`).
- **Handprüfung** mit einem Outlook.com-Konto, einem Microsoft-365-Konto und einem Gmail-Konto
  (Testnutzer): Anmeldung, Abgleich, Senden nach Freischalten, Erneuern nach über einer Stunde,
  „Erneut anmelden“ nach Widerruf im Konto des Anbieters, Entfernen (bei Google danach unter
  myaccount.google.com/permissions verschwunden).

## Grenzen

- Der `iss`-Parameter der Antwort (RFC 9207) wird nicht geprüft; Microsoft und Google senden ihn
  nicht, `state` und PKCE schützen vor untergeschobenen Codes.
- Meldet sich jemand im Browser mit einem anderen Konto an als eingegeben, scheitert die
  IMAP-Anmeldung mit `auth_failed` (kein Auslesen eines ID-Tokens). `login_hint` macht das
  unwahrscheinlich.
- Ein lokales Programm desselben Benutzers kann den Loopback-Port während der Anmeldung erreichen,
  aber ohne `state` nichts bewirken; es könnte die Anmeldeseite im Browser nicht sehen.
- Microsoft-365-Mandanten können IMAP oder SMTP AUTH abschalten; dann hilft nur Microsoft Graph
  (eigener Adapter, nicht gebaut).
