# 0058 — Anhang-Erinnerung und sichtbarer Tracker-Schutz

- **Datum:** 24.11.2026
- **Anlass:** Stufe 20, Aufgaben A und D (Auswahl des Menschen vom 24.11.2026 nach dem Vergleich
  mit Mailbird, Gmail, Outlook, Apple Mail, Spark und eM Client).
- **Status:** entschieden und umgesetzt von der ausführenden Sitzung (Stufe 20).
- **Code:** `crates/mail-content/src/attachment_reminder.rs` (Erkennung),
  `crates/mail-engine/src/compose/mod.rs` (`attachment_reminder`, Einstellung
  `compose.attachment_reminder`), `app/src/features/composer/AttachmentReminder.tsx`,
  `Composer.tsx`, `QuickReply.tsx`; `crates/mail-content/src/trackers.rs` (Listen, Pixel, Links),
  `crates/mail-content/src/html.rs` (`RenderOptions::block_trackers`, `RenderedHtml::trackers`),
  `crates/mail-engine/src/reading/mod.rs` und `links.rs` (Einstellungen `reading.block_trackers`,
  `reading.clean_links`, saubere Links), `app/src/features/reading/TrackerNotice.tsx`,
  `LinkCard.tsx`, `app/src/features/settings/ReadingSettings.tsx`.

## A — Anhang-Erinnerung

### Was als „eigener Text“ zählt

Der Composer hält Zitat und Signatur seit Stufe 5/10 getrennt (`quoteHtml`/`quoteText`,
`includeSignature`). Geprüft wird nur, was die Person geschrieben hat: `bodyText`, bei HTML der
Text von `bodyHtml` über `html_to_text` (Blockzitate werden dort zu `> `-Zeilen). Im Text selbst
endet die Prüfung an der Signaturtrennung (`-- `, auch ohne Leerzeichen) und am Kopf einer zitierten
oder weitergeleiteten Mail (`On … wrote:`, `Am … schrieb …:`, `--- Original Message ---`,
`--- Ursprüngliche Nachricht ---`, `--- Forwarded message ---`, `--- Weitergeleitete Nachricht ---`);
Zeilen mit `>` werden übersprungen.

### Wörter

Englisch und Deutsch, ganze Wörter ohne Rücksicht auf Groß- und Kleinschreibung: *attach,
attached, attaching, attachment(s), enclosed, enclosure(s)*; *Anhang, Anhänge, anhängen, angehängt,
anbei, Anlage(n), beigefügt, beiliegend, beigelegt* (auch in ae/ue-Schreibung). Zusammensetzungen
auf *-anhang/-anhänge* zählen (*Dateianhang*, *E-Mail-Anhänge*); *Anlage* nur als ganzes Wort
(sonst *Geldanlage*). Nicht gezählt: Wörter in Links und Adressen (`…/attachment.pdf`,
`anhang@…`) und ein Wort direkt nach einer Verneinung (*no, not, without, kein(e/en/em), ohne,
nicht*, bis zu zwei Wörter davor). Fehlalarme sind möglich; die Rückfrage lässt sich mit
„Trotzdem senden“ übergehen und unter Schreiben abschalten.

### Wann gefragt wird

Der Kern beantwortet `checkAttachmentReminder(draftId)` mit dem gefundenen Wort oder `null`. Der
Composer fragt vor „Senden“ (auch „Senden und archivieren“) und vor „Später senden“ – der
Zeitpunkt, an dem die Person noch da ist; die Schnellantwort vor ihrem Senden. Nie gefragt wird
beim Senden aus dem Postausgang: Wiederholungen, ein geplanter Versand zu seiner Zeit, „Jetzt
senden“ eines verpassten Versands. Anhänge und eingebettete Bilder zählen als „dabei“. Der Dialog
bietet „Trotzdem senden“, „Anhang hinzufügen“ (Dateidialog; in der Schnellantwort der volle
Composer mit demselben Entwurf) und „Weiterschreiben“. Die Einstellung `compose.attachment_reminder`
ist an (Standard) und wird abgeglichen wie die übrigen Schreib-Einstellungen.

## D — Tracker-Schutz sichtbar

### Was als Tracker zählt

Externe Bilder sind ohnehin blockiert, bis die Person sie lädt (Sicherheitsgrenze 2). Unter ihnen
erkennt die HTML-Pipeline:

1. **Zählpixel:** ein `<img>` mit externer Quelle, das höchstens 2 × 2 Pixel groß ist (Attribute
   `width`/`height` oder Inline-Stil, auch `max-width`/`max-height`), eine Seite 0 hat oder
   versteckt ist (`display:none`, `visibility:hidden`, Attribut `hidden`).
2. **Bekannte Tracking-Dienste:** Bilder (auch CSS-Hintergründe) von Hosts einer gepflegten Liste
   (`KNOWN_TRACKER_HOSTS`, Subdomains eingeschlossen): Öffnungs-Pixel von Mailchimp, Mandrill,
   SendGrid, SparkPost, Amazon SES, Mailjet, Brevo, Salesforce Marketing Cloud, Pardot, Eloqua,
   Marketo, HubSpot, Constant Contact, Campaign Monitor, iContact, MailerLite, Klaviyo,
   Customer.io, ConvertKit, Substack, Lese-Bestätigungs-Dienste von Mail-Add-ons
   (Mailtrack, Yesware, Bananatag, Streak, Superhuman …) sowie DoubleClick und Google Analytics.
   Content-Hosts dieser Dienste (Bildserver von Newslettern) stehen bewusst nicht in der Liste.
3. **Bilder mit Tracking-Parametern:** eine Bildadresse mit Parametern aus
   `TRACKING_PARAMETERS`/`TRACKING_PARAMETER_PREFIXES` (`utm_*`, `mtm_*`, `pk_*`, `hsa_*`,
   `fbclid`, `gclid`, `gbraid`, `wbraid`, `dclid`, `msclkid`, `twclid`, `ttclid`, `li_fat_id`,
   `yclid`, `igshid`, `mc_cid`, `mc_eid`, `_hsenc`, `_hsmi`, `__hssc`, `__hstc`, `__hsfp`,
   `mkt_tok`, `_ga`, `_gl`, `s_cid`, `oly_*`, `vero_*`, `rb_clickid`, `_openstat`, `wickedid`,
   MailerLite, Klaviyo, Sailthru, Drip, ActiveCampaign …). Nur Namen, die ausschließlich der
   Verfolgung dienen; `ref`, `source` u. ä. bleiben, weil Seiten sie brauchen.

Jede Adresse zählt einmal (`TrackerReport`: Pixel, bekannte Dienste, Parameter, bis zu acht Hosts).

### Was mit ihnen geschieht

- Zählpixel und Bilder bekannter Dienste werden **nie** geladen, auch nicht mit „Einmal laden“
  oder „Für Absender immer laden“: Sie stehen nicht in `remote_resources`, bekommen das
  transparente Platzhalterpixel und zählen nicht zu `blocked_remote` (eine Mail nur mit Pixel
  zeigt also keine Leiste „externe Inhalte laden“ mehr).
- Ein Bild mit Tracking-Parametern lädt auf Wunsch von seiner Adresse **ohne** sie; der Schlüssel
  in `RemotePolicy::Allow` ist die saubere Adresse, die Anfrage verrät die Parameter nie.
- Der Lesebereich zeigt über der Mail „n Tracker blockiert“ als Knopf; aufgeklappt erklärt er,
  was Tracker tun, was die App mit welcher Art getan hat und von welchen Hosts sie kamen.

### Links

Beim Beschreiben (`describeLink`) und Öffnen (`openLink`) eines Links entfernt der Kern die
Tracking-Parameter derselben Liste; die anderen Parameter behalten Reihenfolge und Schreibweise
(kein Neukodieren), ein leeres `?` entfällt, das Fragment bleibt. Die Linkkarte zeigt das saubere
Ziel und „Tracking-Parameter entfernt: utm_source, fbclid“. Ob eine Darstellung Links säubert,
merkt sich ihr Rendering (die Einstellung beim Rendern); Links aus KI-Ergebnissen folgen der zuletzt
gelesenen Einstellung. **Weiterleitungs-Links** von Tracking-Diensten (`click.…/…?u=`) werden
nicht aufgelöst: Das hieße, den Dienst anzurufen; die App öffnet sie unverändert, die Warnungen
der Linkprüfung bleiben.

### Einstellungen

Einstellungen → Lesen → Tracker: „Tracker in Nachrichten blockieren“ (`reading.block_trackers`)
und „Tracking aus Links entfernen“ (`reading.clean_links`), beide an, abgeglichen. Aus: externe
Bilder sind wieder alle gleich (geladen nur nach Freigabe), Links öffnen wie geschrieben.

## Abgelehnt

- **Tracker auch nach „Laden“ laden, nur zählen:** dann wäre der Schutz nur eine Anzeige.
- **Weiterleitungen auflösen** (HEAD/GET auf den Tracking-Link): verrät das Öffnen, und die App
  verbindet sich nur mit Servern, die die Person eingerichtet hat.
- **Fremde Blocklisten** (EasyPrivacy u. a.) zur Laufzeit laden: Netzverbindung zu Dritten; eine
  eingebaute Liste reicht für Mail und bleibt im Code prüfbar.
- **Erkennung über KI:** ohne KI gewünscht, und die Regeln genügen.

## Tests

`mail-content`: Wörter, Zitate, Signaturen, Verneinungen, Links; Parameter-Entfernung, Hosts,
Pixel; Pipeline (Zählung, nie gelistete Tracker, saubere Bildadresse, CSS). `mail-engine`:
Composer-Prüfung über `AppCore` (Text, Blockzitat, Verneinung, Anhang, Einstellung aus); Tracker
über `getMessage`, „Einmal laden“ gegen einen protokollierenden HTTP-Server (nur
`/banner.png?id=7` wird angefragt), `describeLink`/`openLink` mit der aufnehmenden
Öffnen-Funktion, Einstellungen aus. Vitest: Dialog, Einstellung, Hinweis, Linkkarte, axe.
Desktop-Test `stage20.e2e.mjs` (A, D).
