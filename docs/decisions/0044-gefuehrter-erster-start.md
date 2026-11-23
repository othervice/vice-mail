# 0044 — Geführter erster Start

- **Datum:** 23.11.2026
- **Anlass:** Stufe 15, Aufgabe „Erster Start“: geführte Einrichtung für neue Nutzer (Konto, Thema,
  optional Sync und KI). Bis dahin zeigte eine neue Installation den leeren Zustand mit „Konto
  hinzufügen“ und drei Zusagen; die Frage nach automatischen Updates (0037) kam als eigene Karte
  nach dem ersten Konto.
- **Status:** entschieden von der ausführenden Sitzung
- **Code:** `crates/mail-model/src/first_start.rs`, `crates/mail-engine/src/first_start.rs` (mit
  Tests), `app/src-tauri/src/commands/first_start.rs`, `app/src/features/first-start/`,
  `app/src/features/settings/accounts/AddAccount.tsx` (`embedded`), `app/src/app/App.tsx`
- **Ändert:** 0037 (wo die einmalige Update-Frage gestellt wird)

## Entscheidung

1. **Fünf kurze Schritte auf einer Karte, statt des Rahmens.** Willkommen (was die App ist, vier
   Zusagen), Konto (das vorhandene Formular), Darstellung (Thema und Akzent, sofort angewandt),
   Optional (Einstellungs-Sync, KI, Updates), Fertig (das Konto liest nur, wo man Änderungen
   erlaubt). Leiste, Seitenspalte und Liste erscheinen erst danach: Eine Einrichtung, neben der
   schon die ganze App bedienbar ist, wäre nicht geführt, und ohne Konto zeigt der Rahmen nur
   leere Zustände. „Einrichtung überspringen“ steht auf jedem Schritt außer dem letzten (dort
   beendet der Hauptknopf ihn mit einem Klick).
2. **Der Kern merkt sich das Ende, je Gerät.** Geräte-Einstellung `first_start` in der Tabelle
   `setting` (`{"outcome":"completed"}`, `"skipped"` oder `"notNeeded"`), nicht in
   `SYNCED_SETTINGS`: Ein zweites Gerät hat seine eigenen Konten und seine eigene Update-Frage.
   Keine Migration. Commands `getFirstStart` (zeigen: nichts gespeichert und kein Konto) und
   `finishFirstStart("completed" | "skipped")`. Nicht in `UiPreferences`: Die speichert jedes
   Fenster als Ganzes, ein Teil davon wird abgeglichen, und die Regel „bestehende Installation“
   braucht die Konten, die nur der Kern kennt.
3. **Bestehende Installationen sehen ihn nie.** Hat die Installation beim ersten Fragen schon ein
   Konto, speichert der Kern `notNeeded`; auch nach dem Entfernen des letzten Kontos erscheint der
   erste Start nicht mehr. Ein unlesbarer Wert gilt als beendet (lieber einmal nicht zeigen als
   jemanden, der längst eingerichtet ist, erneut durch die Schritte schicken). Wird die App
   mittendrin beendet, erscheint er beim nächsten Start wieder, solange es kein Konto gibt; mit
   einem im Ablauf hinzugefügten Konto zählt die Installation dann als eingerichtet.
4. **Die Oberfläche liest die Antwort einmal.** Ein Konto aus Schritt 2 beendet die folgenden
   Schritte nicht. Kann der Kern nicht antworten, erscheint der Rahmen (der Fehler ist an der
   Brücke protokolliert, und der Rahmen zeigt den Zustand des Kerns).
5. **Die Update-Frage aus 0037 steht im Schritt „Optional“** (ändert 0037, „Die einmalige
   Frage“): ein Schalter „Automatisch nach Updates suchen“, aus, mit demselben Text wie unter
   Einstellungen → Über. Sie gilt als beantwortet (`setUpdateAutoCheck` mit dem Wert des
   Schalters, `consentAnswered`), sobald der Schritt gezeigt und nach vorn verlassen oder von dort
   aus übersprungen wurde, auch ohne Konto. Begründung: 0037 wollte die Frage nicht vor die
   Kontoeinrichtung stellen, wo sie zwischen Menschen und ihrem Ziel steht. Im geführten Ablauf
   kommt sie nach dem Konto, gehört zu den optionalen Extras, ist aus und lässt sich übergehen.
   Die Karte nach dem ersten Konto bleibt als Rückfall für alle, die den Schritt nie gesehen haben
   (vorher übersprungen, ältere Installationen). Nie zweimal: Beide Stellen setzen dieselbe
   Antwort. Ein Build ohne Updater zeigt den Schalter nicht und speichert nichts.
6. **Einstellungs-Sync und KI ohne Nachbau.** „Jetzt einrichten“ öffnet die vorhandene Seite des
   Einstellungs-Sync im Schritt; „Zurück zur Einrichtung“ führt zurück, der Fokus auf die Zeile.
   Ohne Konto nur der Hinweis, dass der Sync ein Konto braucht. KI: ein Satz, dass sie optional
   und aus ist und wo man sie einschaltet (zuerst MCP, 0010); keine Schaltfläche, nichts wird
   eingeschaltet.
7. **Konto: das vorhandene Formular, eingebettet.** Ohne eigenen Titel, ohne „Zurück zu den
   Konten“ und „Abbrechen“ (der Schritt hat „Später einrichten“). Nach dem Hinzufügen bleibt der
   Ablauf im Schritt und meldet das Konto, statt in den Posteingang zu springen.
8. **Nichts geht verloren.** Besuchte Schritte bleiben verborgen eingehängt, so behält das
   Formular bei Zurück und Weiter seine Eingaben. Escape tut im ersten Start nichts. Der Fokus geht
   bei jedem Schritt auf dessen Überschrift.
9. **Vierte Zusage.** „Keine Telemetrie“ steht neben „zuerst nur lesend“, „kein Cloud-Dienst
   dazwischen“ und „Passwort im Schlüsselbund“, im ersten Start und im leeren Zustand ohne Konto.

## Rückweg

`FirstStartGate` aus `app/src/app/App.tsx` nehmen: Die App startet wieder mit dem leeren Zustand,
die Einstellung `first_start` bleibt unbeachtet liegen. Die Update-Karte aus 0037 ist unverändert
vorhanden und fragt dann wieder nach dem ersten Konto.
