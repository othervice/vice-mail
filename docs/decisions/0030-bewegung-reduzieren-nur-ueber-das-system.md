# 0030 — „Bewegung reduzieren“ folgt nur der Systemeinstellung

Stand 23.11.2026, Stufe 8.

## Kontext

Stufe 8 verlangt, dass die App „Bewegung reduzieren“ respektiert. `docs/SPEC.md` nennt die
Systemeinstellung; `docs/DESIGN.md` legt die Bewegungs-Token fest (120/180/240 ms, sanftes
Ausklingen, 0 ms bei reduzierter Bewegung). Offen war, ob die App zusätzlich einen eigenen Schalter
bekommt.

## Entscheidung

Die App folgt nur `prefers-reduced-motion` des Systems (macOS: Bedienungshilfen → Anzeige →
Bewegung reduzieren; Windows: Einstellungen → Barrierefreiheit → Visuelle Effekte →
Animationseffekte). Es gibt keinen eigenen Schalter in der App.

## Begründung

- Die einzige Bewegung der Oberfläche ist `transition-colors` mit dem kurzen Token; nichts gleitet,
  dreht oder springt. Ein eigener Schalter hätte fast nichts abzuschalten.
- Eine zweite Stelle für dieselbe Einstellung widerspricht der Systemeinstellung, die die Menschen
  für alle Programme gesetzt haben.
- `UiPreferences` (0025) bräuchte ein weiteres Feld samt Abgleich; der Nutzen rechtfertigt das nicht.

## Absicherung

`app/tests/motion.test.tsx` legt die Token und die Regel für reduzierte Bewegung fest und schlägt bei
jeder neuen Bewegungsklasse, `@keyframes`, weichem Scrollen oder Skript-Animation fehl.

## Rückweg

Kommt später echte Bewegung dazu (z. B. aufklappende Bereiche), wird ein Schalter in `UiPreferences`
ergänzt, der die Systemeinstellung nur verschärfen kann.
