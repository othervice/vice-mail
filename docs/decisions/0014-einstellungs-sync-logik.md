# 0014 — Einstellungs-Sync: Logik, Kryptografie-Bausteine, offene Punkte entschieden

Stand 23.11.2026, Stufe 11 (vorgezogen: reine Logik). Code: `crates/mail-engine/src/settings_sync/`,
Format: `docs/SYNC.md`.

## Entscheidungen

**age 0.11.5 statt 0.12.** age 0.12 zieht für seine `tag`-Empfänger immer P-256 und damit
`crypto-bigint` 0.5.5 mit der Sicherheitsmeldung RUSTSEC-2026-0387 in den Baum; `cargo deny` würde
`scripts/check.sh` rot machen. Wir nutzen nur X25519 und scrypt, die betroffene Stelle liefe nie.
0.11.5 (14.07.2026) wird parallel gepflegt und hat dasselbe Dateiformat. Wechsel auf 0.12, sobald die
Meldung dort behoben ist oder ein Grund dafür auftaucht. Folge: eine harmlose Future-Incompat-Warnung
zu `proc-macro-error2` beim Bauen.

**Opake IDs mit HMAC-SHA-256** (`hmac` 0.13, `sha2` 0.11, RustCrypto), abgeleitet aus dem
Sync-Schlüssel. Nicht `ring` (soll später durch aws-lc-rs ersetzt werden, 0002) und nicht blake3
(C/Assembler-Build).

**scrypt-Arbeitsfaktor log2(N) = 18** (256 MiB, etwa 0,35 s auf dem M4), einmal je Gerät. Beim
Entschlüsseln höchstens 20, damit ein gefälschter Schlüsseldatensatz das Gerät nicht lahmlegt.

**Im Schlüsselbund liegt der Sync-Schlüssel, nicht die Passphrase.** Beides schützt gleich viel;
mit der Passphrase kostete jeder App-Start einen scrypt-Lauf mit 256 MiB. Die Stufendatei ist angepasst.

**Schreibzugriff auf den Sync-Ordner.** Neue Konten sind nur lesend; der Sync-Ordner braucht aber
APPEND und EXPUNGE. Wer „Sync einschalten“ für ein Konto bestätigt, erlaubt ausdrücklich das
Schreiben **nur in diesem einen Ordner**. Der Adapter bekommt dafür eine auf den Ordner beschränkte
Schreibverbindung; alle anderen Ordner des Kontos bleiben nur lesend, solange der Mensch den
Schreibzugriff nicht für das ganze Konto freischaltet. Die Oberfläche sagt das beim Einschalten.
(Annahme der Hauptsitzung; der Mensch kann sie ändern.)

**Datenarten ergänzen.** `remote_content_sender` (Absender-Freigaben für externe Bilder) und von Hand
bearbeitete Kontakte sind in der Architektur als „sync“ markiert und kommen in Stufe 11 ins
Verzeichnis der synchronisierbaren Arten.

**`writes_enabled` wird nie abgeglichen.** Übernommene Konten starten auf jedem Gerät nur lesend.

## Rückweg

Die Krypto steckt hinter `SyncKey`; ein Wechsel der age-Version ändert das Dateiformat nicht.
