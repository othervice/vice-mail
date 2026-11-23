import { create } from "zustand";
import { readLocal, writeLocal } from "../keyboard/localFallback";

/**
 * Stage 16: accent themes (Settings → Appearance). Each one changes only the accent family of
 * tokens.css (accent, its soft fill, the selected row, the focus ring, text on the accent), in
 * light and dark; tests/contrast.test.ts checks all of them. "teal" is the app's own accent and
 * stays the default until the human decides otherwise.
 */
export const ACCENT_THEMES = ["teal", "blue", "violet", "rose", "orange", "green"] as const;

export type AccentTheme = (typeof ACCENT_THEMES)[number];

export const DEFAULT_ACCENT: AccentTheme = "teal";

export function isAccentTheme(value: unknown): value is AccentTheme {
  return typeof value === "string" && (ACCENT_THEMES as readonly string[]).includes(value);
}

/** tokens.css switches the accent family on this attribute; the default has none. */
export function applyAccent(accent: AccentTheme): void {
  if (accent === DEFAULT_ACCENT) delete document.documentElement.dataset["accent"];
  else document.documentElement.dataset["accent"] = accent;
}

// ---- FALLBACK (stage 16): kept on this device only ----
// The stage asks for a synced UI setting, but neither `Preferences` nor `UiPreferences` of the
// core has a field for it yet (needed: `UiPreferences.accentTheme`, synced like the text size).
// Until then the choice lives in the web view's storage of this device, validated on reading.
// When the core field exists, this block goes and the store reads `uiPreferences` instead.

/** The stored accent, or the default when nothing (valid) is stored or storage is off. */
export function readStoredAccent(): AccentTheme {
  const stored = readLocal("accentTheme");
  return isAccentTheme(stored) ? stored : DEFAULT_ACCENT;
}

interface AccentState {
  accent: AccentTheme;
  /** False when the browser storage refused it: the choice then lasts until the app quits. */
  stored: boolean;
  setAccent: (accent: AccentTheme) => void;
  /** Another window of the app changed it (the `storage` event). */
  reload: () => void;
}

export const useAccentStore = create<AccentState>()((set) => ({
  accent: readStoredAccent(),
  stored: true,
  setAccent: (accent) => {
    set({ accent, stored: writeLocal("accentTheme", accent) });
  },
  reload: () => {
    set({ accent: readStoredAccent() });
  },
}));
