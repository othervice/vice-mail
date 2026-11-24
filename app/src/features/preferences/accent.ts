import { create } from "zustand";
import { toCoreError, type AccentTheme as CoreAccentTheme, type CoreError } from "../../services/ipc";
import { saveUiPreferences } from "../keyboard/uiPreferences";

/**
 * Stage 16: accent themes (Settings → Appearance). Each one changes only the accent family of
 * tokens.css (accent, its soft fill, the selected row, the focus ring, text on the accent), in
 * light and dark; tests/contrast.test.ts checks all of them. Decision 0053: "blue" is the
 * default (the core reads a missing or unknown value as blue too); a stored choice stays, also
 * orange, the default of decision 0038 before.
 *
 * Decision 0036: the choice is a UI preference of the core (`UiPreferences.accentTheme`), synced
 * between devices like the text size. The store holds what the documents of this window apply;
 * `useUiPreferencesSync` fills it from the core (load, saves, `uiPreferencesChanged`).
 */
export const ACCENT_THEMES = ["teal", "blue", "violet", "rose", "orange", "green"] as const satisfies readonly CoreAccentTheme[];

export type AccentTheme = (typeof ACCENT_THEMES)[number];

// Fails to compile when the core gets a theme the UI does not offer.
type MissingTheme = Exclude<CoreAccentTheme, AccentTheme>;
const themesAreComplete: [MissingTheme] extends [never] ? true : never = true;
void themesAreComplete;

export const DEFAULT_ACCENT: AccentTheme = "blue";

export function isAccentTheme(value: unknown): value is AccentTheme {
  return typeof value === "string" && (ACCENT_THEMES as readonly string[]).includes(value);
}

/**
 * tokens.css switches the accent family on this attribute. Always set, the default included:
 * the root values of tokens.css are blue's (decision 0053), so the first frame before the core
 * answers has the default, and every theme, blue too, comes from its own block afterwards.
 */
export function applyAccent(accent: AccentTheme): void {
  document.documentElement.dataset["accent"] = accent;
}

interface AccentState {
  accent: AccentTheme;
  /** The core refused the last choice; the previous theme applies again. */
  saveError: CoreError | null;
  /** The user's choice: shown at once, saved to the core (rolled back if it refuses). */
  setAccent: (accent: AccentTheme) => void;
  /** What the core holds (anything unknown reads as the default). */
  applyStored: (accent: unknown) => void;
}

export const useAccentStore = create<AccentState>()((set) => ({
  accent: DEFAULT_ACCENT,
  saveError: null,
  setAccent: (accent) => {
    set({ accent, saveError: null });
    saveUiPreferences({ accentTheme: accent }).catch((error: unknown) => {
      // The cache is back at the previous value; `applyStored` has put it on screen again.
      set({ saveError: toCoreError(error) });
    });
  },
  applyStored: (accent) => {
    set({ accent: isAccentTheme(accent) ? accent : DEFAULT_ACCENT });
  },
}));
