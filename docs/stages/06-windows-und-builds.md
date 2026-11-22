# Stufe 06 — Windows und Builds

**Ziel:** Die App lässt sich auf Windows 11 und auf einem zweiten Mac installieren und durchläuft
dort den vollständigen Mail-Ablauf. Plattformunterschiede sind gefunden und behoben.

**Voraussetzung:** Stufe 05 fertig. Zuerst prüfen, welcher Zugang zum Windows-Rechner besteht
(SSH vom Mac mini aus). Ohne Zugang: alles liefern, was ohne geht, und die Handprüfung vorbereiten.

## Aufgaben

- [ ] **Windows-Build.** `scripts/build-windows.sh` erzeugt per Cross-Build einen NSIS-Installer. WebView2 wird bei Bedarf über den Bootstrapper nachinstalliert. App-Symbole in allen Größen.
- [ ] **macOS-Build.** `.app` und `.dmg`, ohne Entwickler-Signatur. `docs/INSTALL.md` (Englisch) beschreibt die Installation ehrlich, einschließlich der Gatekeeper- und SmartScreen-Warnungen bei unsignierten Builds.
- [ ] **Beides in einem.** `scripts/release-local.sh` baut beide und legt sie mit Versionsnummer unter `dist/` ab. Version und Build-Stand stehen in „Über“.
- [ ] **Plattformpunkte unter Windows prüfen und beheben:**
  - Schlüsselbund über die Windows-Anmeldeinformationen
  - Datenverzeichnis, lange Pfade, bereinigte Dateinamen auf NTFS
  - Titelleiste, Fenster andocken, Skalierung bei hoher Auflösung
  - Schrift (Segoe UI Variable), Tastenkürzel mit Ctrl statt Cmd
  - Systemdialoge zum Speichern und Öffnen
- [ ] **Tests unter Windows.** Mit Zugang: Rust-Tests, den durchgehenden Mail-Ablauf und die HTML-Sandbox-Tests aus Stufe 3 unter WebView2 ausführen. Der Testserver läuft dafür entweder unter Windows (Stalwart gibt es dafür) oder auf dem Mac mini, im lokalen Netz erreichbar.
- [ ] **Handprüfung.** `docs/testing/windows-checklist.md`: Schritt-für-Schritt-Liste für alles, was nicht automatisch lief, mit Feld für Ergebnis.

## Abnahme

- [ ] Installer installiert und deinstalliert sauber unter Windows 11
- [ ] unter Windows: Konto einrichten, Liste, Mail lesen, Anhang speichern, antworten, senden, archivieren
- [ ] die HTML-Sandbox-Tests bestehen unter WebView2, oder sie stehen ausdrücklich als ungeprüft im Ergebnis
- [ ] das `.dmg` lässt sich auf einem zweiten Mac installieren und starten
- [ ] `scripts/check.sh` ist grün; `scripts/release-local.sh` läuft durch

## Nicht in dieser Stufe

- Signierung, Notarisierung, automatische Updates (Stufe 15)
- Windows auf ARM

## Der Mensch prüft danach

- Zugang zum Windows-Rechner bereitstellen oder die Checkliste abarbeiten.
- Die App auf MacBook und Windows-Rechner installieren und mit einem echten Konto benutzen.

## Ergebnis

_Von der ausführenden Sitzung auszufüllen._
