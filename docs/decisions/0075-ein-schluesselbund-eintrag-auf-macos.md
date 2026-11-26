# 0075 — Ein Schlüsselbund-Eintrag auf macOS

- **Datum:** 26.11.2026
- **Anlass:** Stufe 22, Punkt 4. Nach jedem Update von 0.1.x fragt macOS 5–6 Mal nach dem
  Schlüsselbund („Vice Mail möchte … in deinem Schlüsselbund verwenden“, Passwort und „Immer
  erlauben“), einmal je Eintrag. Die Builds sind ad hoc signiert; die Zugriffsliste eines Eintrags
  hängt an der Signatur, und die ändert sich mit jedem Build. Endgültig behebt das erst die
  Signatur mit Developer ID (Stufe 15).
- **Status:** Plan mit dem Menschen abgestimmt (26.11.2026), Einzelheiten von der ausführenden
  Sitzung entschieden
- **Code:** `crates/mail-engine/src/credentials/vault.rs` (`VaultStore`, `ItemStore`),
  `crates/mail-engine/src/credentials.rs` (`KeychainItems`, `SystemKeychain`,
  `SecretStore::migrate`, `Credentials::migrate`), `crates/mail-engine/src/core.rs`
  (`known_secret_names`, `migrate_secrets`), `app/tests/e2e/perf.e2e.mjs` (Prüfung auf Reste)
- **Tests:** `crates/mail-engine/src/credentials/vault/tests.rs` (ein nachgebauter Schlüsselbund,
  der sich wie macOS mit ad hoc signierten Builds verhält: jeder Eintrag kennt den Build, der ihn
  angelegt hat, und die Builds, denen der Nutzer ihn erlaubt hat; jeder andere Lesezugriff ist
  eine gezählte Abfrage; wahlweise darf ein neuer Build fremde Einträge ändern und löschen oder
  nicht), `credentials::tests::real_keychain_vault_round_trip` (echter Schlüsselbund, nur Dienst
  `org.vicemail.desktop.e2e`, von Hand in der GUI-Sitzung)

## Entscheidung

Auf macOS liegen alle Geheimnisse von Vice Mail in **einem** Eintrag (generisches Passwort, Dienst
= Bundle-ID). Sein Inhalt ist JSON mit Version:

```json
{"format": 1, "entries": {"<Name>": "<Geheimnis>", …}, "retired": ["<Name>", …]}
```

Die Namen sind die bisherigen Eintragsnamen (`CredentialRef`: Konto-UUID, `synced:<hex>`,
`oauth:<anbieter>:<id>` mit Stücken `#n`, `token:<id>`, `mcp-client:<id>`, `ai-profile:<id>`,
`settings-sync:key`). Die Schnittstelle `SecretStore` bleibt, wie sie ist; kein Aufrufer ändert
sich. Windows bleibt bei einem Eintrag je Geheimnis: die Anmeldeinformationsverwaltung fragt
nicht, und ein Eintrag fasst dort höchstens 2.560 Bytes (deshalb die Stücke langer OAuth-Tokens).

## Einzelheiten

- **Ein Schreiber.** Alle Zugriffe laufen durch eine Sperre im Prozess. Der Eintrag wird beim
  ersten Zugriff gelesen und im Speicher gehalten (dort liegen die Passwörter offener Verbindungen
  ohnehin). Schreiben heißt: Inhalt ändern, schreiben, zurücklesen, vergleichen.
- **Generationen statt Überschreiben.** Eine Änderung schreibt einen neuen Eintrag (`vault`, dann
  `vault.2`, `vault.3`, …), liest ihn zurück und löscht erst nach einem gleichen Ergebnis die
  älteren. Leser nehmen die höchste Generation; die Namen kommen aus der Suche des Schlüsselbunds,
  die nur Attribute liest und deshalb nie fragt. Zwei Gründe:
  1. Ein fehlgeschlagenes oder beschädigtes Schreiben lässt den bisherigen Inhalt als neuesten
     Eintrag stehen. Beim Überschreiben an Ort und Stelle wäre er weg.
  2. `perf.e2e.mjs` hat gezeigt, dass macOS eine Kopie der App einen Eintrag lesen, aber nicht
     löschen lässt („Invalid attempt to change the owner of this item“). Ob ein Update (gleicher
     Pfad, andere Signatur) einen Eintrag des Vorgängers ändern oder löschen darf, ist ungeprüft.
     Mit Generationen ist es egal: Der neue Eintrag gehört dem neuen Build. Ein alter, den macOS
     nicht freigibt, bleibt ungenutzt liegen und wird nie wieder gelesen.
- **Leer heißt weg.** Ist nichts mehr gespeichert, werden alle Generationen gelöscht, die älteren
  zuerst. Bleibt eine ältere stehen, wird stattdessen ein leerer Eintrag als neueste geschrieben,
  damit kein Rest mit alten Geheimnissen wieder zur neuesten wird.
- **Übernahme der alten Einträge.** Beim Start gibt der Kern alle Namen, die die Datenbank kennt
  (Konten mit den Stücken der Anmelde-Tokens, MCP-Clients, KI-Profile mit Schlüssel, den
  Sync-Schlüssel), an `Credentials::migrate`. Es liest die vorhandenen alten Einträge (dabei
  fragt macOS ein letztes Mal je Eintrag), schreibt den Sammel-Eintrag, prüft ihn und löscht die
  alten erst danach. Unbekannte Reste (z. B. von früher entfernten Konten) bleiben unberührt und
  kosten keine Abfrage. Ein Lesezugriff, der im Sammel-Eintrag nichts findet, sucht weiter nach
  einem alten Eintrag und übernimmt ihn dann. Eine abgelehnte Abfrage, ein Absturz oder ein
  fehlgeschlagenes Schreiben verliert deshalb nichts: Bis der Sammel-Eintrag das Geheimnis hat,
  bleibt der alte Eintrag gültig. Die Übernahme läuft im Hintergrund; das Fenster wartet nicht.
- **`retired`.** Lässt macOS einen alten Eintrag nicht löschen, beschattet ihn der Sammel-Eintrag.
  Wird der Name später gelöscht, würde der alte Eintrag beim nächsten Lesen wieder auftauchen.
  Deshalb kommt sein Name dann in `retired` und wird nie mehr gelesen.
- **Fehler** haben feste Texte ohne Inhalt. Ein unlesbarer Sammel-Eintrag oder einer einer
  neueren Version (`format` > 1) wird weder gelesen noch überschrieben: Lesen und Schreiben
  scheitern mit `keychain`, nichts geht verloren.
- **Zwei Prozesse** (die App zweimal gestartet) sind nicht abgedeckt: macOS startet ein Bundle
  normalerweise nur einmal, und die Datenbank gehört ohnehin einem Prozess.

## Folgen

- Nach einem ad hoc signierten Update fragt macOS einmal statt einmal je Konto und Schlüssel.
  „Erlauben“ statt „Immer erlauben“ fragt einmal je Start, bis die App zum ersten Mal schreibt
  (danach liest sie einen eigenen Eintrag).
- Das Update auf 0.1.3 fragt ein letztes Mal je alten Eintrag. Ob macOS die alten Einträge danach
  löschen lässt, ist ungeprüft (siehe oben); sonst bleiben sie ungenutzt im Schlüsselbund, und
  `docs/INSTALL.md` sagt, wie man sie entfernt.
- Ein Downgrade auf 0.1.2 findet die Passwörter nicht mehr (sie liegen im Sammel-Eintrag). Ein
  Downgrade wird ohnehin nicht unterstützt (die Datenbank verweigert ältere Versionen).
- Schlüsselbundverwaltung zeigt einen Eintrag `vault` (oder `vault.<n>`) statt einem je Konto.
- Die Desktop-Tests (Dienst `org.vicemail.desktop.e2e`) prüfen weiter, dass nach dem Entfernen
  der Konten nichts übrig ist; der Sammel-Eintrag verschwindet mit dem letzten Geheimnis.

## Geprüft

- Rust-Tests mit dem nachgebauten Schlüsselbund: drei Updates hintereinander fragen je einmal,
  ob neue Builds fremde Einträge ändern dürfen oder nicht; Übernahme mit einer Abfrage je altem
  Eintrag, keiner für fehlende Namen oder Reste; abgelehnte Abfrage und Schreibfehler verlieren
  nichts; ein gelöschter Name taucht aus einem alten Eintrag nicht wieder auf; beschädigtes
  Schreiben wird erkannt und lässt den alten Inhalt stehen; unlesbare und neuere Einträge werden
  nicht überschrieben; 8 Threads mit je 25 Schreibzugriffen verlieren nichts; Fehler und `Debug`
  enthalten kein Geheimnis; über `AppCore` zieht ein Konto beim Start in den Sammel-Eintrag um
  und verbindet sich mit seinem Passwort.
- Echter Schlüsselbund (Dienst `org.vicemail.desktop.e2e`, GUI-Sitzung, ein Build): drei alte
  Einträge übernommen und gelöscht, Liste der Namen über die Suche, nächste Generation, nach dem
  Löschen bleibt nichts.
- **Ungeprüft:** wie oft macOS beim echten Update von 0.1.2 auf 0.1.3 und von 0.1.3 auf 0.1.4
  fragt, und ob es dabei alte Einträge löschen lässt. Das geht nur mit zwei verschieden signierten
  Builds und einem Menschen, der die Abfragen beantwortet.
